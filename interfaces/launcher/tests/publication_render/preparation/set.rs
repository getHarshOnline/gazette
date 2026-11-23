//! Complete retained site sets use the real `SQLite`, filesystem and Git phase owners.

use gazette_sdk::{
    authored, publication::content::Disposition, release, release_commit, workspace::Key,
};

use super::super::{sites, worktree::metadata};
use super::{
    build, check::disclosure_policy, enable_site, fs, initialize, options, private_directory,
    published_fixture, read, request, source, store, unchanged, with_destinations,
};

const SITES: [&str; 2] = ["site:one", "site:other"];

fn key(value: &str) -> Key {
    value.parse().expect("fixture key")
}

fn render(inputs: &build::AcquiredPublicationSet) -> build::RenderedPublicationSet<'_> {
    render_selected(inputs, true)
}

fn render_selected(
    inputs: &build::AcquiredPublicationSet,
    selected: bool,
) -> build::RenderedPublicationSet<'_> {
    render_profile(inputs, selected, false)
}

fn render_profile(
    inputs: &build::AcquiredPublicationSet,
    selected: bool,
    preview: bool,
) -> build::RenderedPublicationSet<'_> {
    let page = "page.html".parse().expect("page");
    let alias = "alias.html".parse().expect("alias");
    let model = "model.json".parse().expect("model");
    let mut options = sites::yearless_options(&SITES, &page, &alias, &model);
    options.selection = selected.then(|| authored::Selection {
        content: ["collection:home", "content:one", "content:other"]
            .map(|id| (key(id), Disposition::Include))
            .into_iter()
            .collect(),
        assets: Default::default(),
    });
    options.preview_urls = preview.then(|| {
        [
            ("site:one", "http://127.0.0.1:8001/mount/"),
            ("site:other", "http://127.0.0.1:8002/"),
        ]
        .map(|(site, url)| (key(site), url.parse().expect("local preview URL")))
        .into_iter()
        .collect()
    });
    build::render_publications(inputs, options, Default::default()).expect("shared semantic render")
}

fn permitted() -> build::LocalPreparationOptions {
    build::LocalPreparationOptions {
        stage: options(),
        worktree: release_commit::WorktreeOptions {
            allow_initialize: true,
            ..Default::default()
        },
        ..Default::default()
    }
}

fn requests<'a>(
    keys: &'a [Key; 2],
    journal: &'a std::path::Path,
    header: store::JournalHeader,
    parent: &'a std::path::Path,
    recipe: &'a release_commit::CommitMetadata,
    rules: &'a build::DisclosurePolicy,
) -> [build::SitePreparationRequest<'a>; 2] {
    std::array::from_fn(|i| {
        let mut stage = request(journal, header, parent, ["one", "other"][i]);
        stage.id = source::Sha256Digest::of(keys[i].as_str().as_bytes());
        build::SitePreparationRequest {
            site: &keys[i],
            local: build::LocalPreparationRequest {
                output_policy: None,
                qualification: None,
                coordinator: None,
                stage,
                metadata: recipe,
                parent_source: None,
                disclosure: rules,
            },
        }
    })
}

#[test]
fn complete_set_preserves_request_order_and_independent_native_batch_readback() {
    let fixture = published_fixture(&[]);
    let config = enable_site(
        enable_site(with_destinations(fixture.config()), "one"),
        "other",
    );
    let inputs = sites::acquire_set(&config, &SITES);
    let rendered = render(&inputs);
    let keys = SITES.map(key);
    let root = private_directory();
    let journal = initialize(root.path());
    let parent = private_directory();
    let recipe = metadata();
    let rules = disclosure_policy();
    let mut requested = requests(
        &keys,
        root.path(),
        journal.header,
        parent.path(),
        &recipe,
        &rules,
    );
    requested.reverse();
    let result = build::prepare_local_set(&rendered, &requested, permitted())
        .expect("complete native preparation");
    assert_eq!(
        result.iter().map(|r| &r.site).collect::<Vec<_>>(),
        requested.iter().map(|r| r.site).collect::<Vec<_>>()
    );
    let header = result.last().expect("complete set").preparation.journal;
    assert!(result[0].preparation.journal.writer_fence < header.writer_fence);
    let mut selections = Vec::new();
    for entry in &result {
        let receipt = &entry.preparation;
        let output = rendered.sites()[&entry.site]
            .as_ref()
            .expect("site")
            .site()
            .output();
        assert_eq!(receipt.record.intent.output, output.inventory().identity());
        let path = receipt.record.intent.address.path();
        for file in output.inventory().entries() {
            if let source::Content::File { .. } = file.content {
                assert_eq!(
                    fs::read(path.join(file.path.as_str())).expect("actual staged file"),
                    output.file(&file.path).expect("retained bytes")
                );
            }
        }
        let observed = store::inspect_preparation(
            &journal.database,
            receipt.record.intent.id,
            Some(&header),
            read(),
        )
        .expect("independent SQLite read");
        assert_eq!(observed.record.as_ref(), Some(&receipt.record));
        selections.push(release::PreparationSelection {
            output_policy: None,
            qualification: None,
            preparation_id: receipt.record.intent.id,
            site: entry.site.clone(),
            domain: format!(
                "{}.example",
                entry
                    .site
                    .as_str()
                    .strip_prefix("site:")
                    .expect("fixture site")
            ),
            commit: receipt
                .record
                .worktree
                .as_ref()
                .expect("Git recipe")
                .completion
                .as_ref()
                .expect("Git readback")
                .report
                .commit
                .clone(),
            expected_remote_oid_or_unborn: release::RemoteState::Unborn,
        });
    }
    let batch = release::inspect_preparations(
        release::PreparationBatchRequest {
            database: &journal.database,
            expected: header,
            selections: &selections,
            protected_roots: &[fixture.directory.path().to_owned()],
        },
        release::PreparationBatchOptions {
            inspection: release_commit::JournalWorktreeInspectionOptions {
                read: read(),
                ..Default::default()
            },
            ..Default::default()
        },
    )
    .expect("existing batch owner joins exact independently observed worktrees");
    assert_eq!(batch.journal(), header);
    assert_eq!(batch.targets().len(), 2);
}

#[test]
fn mixed_destination_set_refuses_whole_selection_without_silent_shrink() {
    let fixture = published_fixture(&[]);
    let config = enable_site(with_destinations(fixture.config()), "one");
    let inputs = sites::acquire_set(&config, &SITES);
    let rendered = render(&inputs);
    let keys = SITES.map(key);
    let root = private_directory();
    let journal = initialize(root.path());
    let baseline = fs::read(&journal.database).expect("baseline");
    let parent = private_directory();
    let metadata = metadata();
    let policy = disclosure_policy();
    let requested = requests(
        &keys,
        root.path(),
        journal.header,
        parent.path(),
        &metadata,
        &policy,
    );
    let error = build::prepare_local_set(&rendered, &requested, permitted())
        .expect_err("disabled member must refuse the exact complete set");
    assert_eq!(error.requested, keys);
    assert!(error.completed.is_empty());
    let build::LocalPreparationSetFailure::Deployability(eligibility) = error.cause.as_ref() else {
        panic!("dedicated complete-set deployability refusal required: {error:?}")
    };
    assert_eq!(eligibility.code(), "publication_not_deployable");
    assert_eq!(
        eligibility
            .sites()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        ["site:other"]
    );
    for item in &requested {
        unchanged(&journal, item.local.stage.id, &baseline);
    }
    assert_eq!(
        fs::read_dir(parent.path()).expect("stage parent").count(),
        0
    );
}

#[test]
fn malformed_membership_and_journal_selection_refuse_before_any_write() {
    let fixture = published_fixture(&[]);
    let inputs = sites::acquire_set(&fixture.config(), &SITES);
    let rendered = render(&inputs);
    let keys = SITES.map(key);
    let root = private_directory();
    let journal = initialize(root.path());
    let bytes = fs::read(&journal.database).expect("baseline");
    let parent = private_directory();
    let other_root = private_directory();
    let recipe = metadata();
    let rules = disclosure_policy();
    let extra = key("site:extra");
    for case in 0..7 {
        let mut requested = requests(
            &keys,
            root.path(),
            journal.header,
            parent.path(),
            &recipe,
            &rules,
        )
        .to_vec();
        match case {
            0 => requested.clear(),
            1 => {
                requested.pop();
            }
            2 => requested[1].site = requested[0].site,
            3 => requested[1].site = &extra,
            4 => requested[1].local.stage.id = requested[0].local.stage.id,
            5 => requested[1].local.stage.journal = other_root.path(),
            6 => requested[1].local.stage.expected.writer_fence += 1,
            _ => unreachable!(),
        }
        let error = build::prepare_local_set(&rendered, &requested, permitted())
            .expect_err("invalid complete set");
        assert!(
            matches!(*error.cause, build::LocalPreparationSetFailure::Refused(_)),
            "case {case}: {error}"
        );
        assert!(error.completed.is_empty());
        assert_eq!(
            error.requested,
            requested.iter().map(|r| r.site.clone()).collect::<Vec<_>>()
        );
        for site in &keys {
            unchanged(
                &journal,
                source::Sha256Digest::of(site.as_str().as_bytes()),
                &bytes,
            );
        }
        assert_eq!(
            fs::read_dir(parent.path()).expect("stage parent").count(),
            0
        );
    }
}

#[test]
fn mixed_custody_selections_cannot_silently_use_the_first_sites_connector() {
    let fixture = published_fixture(&[]);
    let inputs = sites::acquire_set(&fixture.config(), &SITES);
    let rendered = render(&inputs);
    let keys = SITES.map(key);
    let root = private_directory();
    let journal = initialize(root.path());
    let baseline = fs::read(&journal.database).expect("journal baseline");
    let anchor = private_directory();
    let parent = private_directory();
    let selected = store::coordinator::LocalCoordinatorSelection {
        anchor_root: anchor.path().to_owned(),
        binding: store::coordinator::LocalCoordinatorBinding {
            coordinator_id: "fixture".into(),
            principal_scope: source::Sha256Digest::of(b"fixture scope"),
            journal_root: root.path().to_owned(),
            journal_id: journal.header.journal_id,
            journal_epoch: journal.header.journal_epoch,
        },
    };
    let metadata = metadata();
    let policy = disclosure_policy();
    for selected_site in 0..2 {
        let mut requests = requests(
            &keys,
            root.path(),
            journal.header,
            parent.path(),
            &metadata,
            &policy,
        );
        requests[selected_site].local.coordinator = Some(&selected);
        let error = build::prepare_local_set(
            &rendered,
            &requests,
            build::LocalPreparationOptions {
                allow_anchor_write: true,
                ..permitted()
            },
        )
        .expect_err("complete set cannot mix ordinary and guarded custody");
        assert!(matches!(
            error.cause.as_ref(),
            build::LocalPreparationSetFailure::Refused(_)
        ));
        assert_eq!(error.requested, keys);
        assert!(error.completed.is_empty());
        assert_eq!(
            fs::read(&journal.database).expect("no journal change"),
            baseline
        );
        assert_eq!(
            fs::read_dir(anchor.path())
                .expect("no anchor access effect")
                .count(),
            0
        );
        assert_eq!(fs::read_dir(parent.path()).expect("no stages").count(), 0);
    }
}

#[test]
fn all_local_preconditions_are_checked_before_first_site_starts() {
    let fixture = published_fixture(&[]);
    let inputs = sites::acquire_set(&fixture.config(), &SITES);
    let rendered = render(&inputs);
    let keys = SITES.map(key);
    let root = private_directory();
    let journal = initialize(root.path());
    let bytes = fs::read(&journal.database).expect("baseline");
    let parent = private_directory();
    let recipe = metadata();
    let mut invalid = metadata();
    invalid.message.clear();
    let rules = disclosure_policy();
    let empty_rules = build::DisclosurePolicy {
        rules: Default::default(),
    };
    for case in 0..6 {
        let mut requested = requests(
            &keys,
            root.path(),
            journal.header,
            parent.path(),
            &recipe,
            &rules,
        );
        let mut options = permitted();
        match case {
            0 => requested[1].local.metadata = &invalid,
            1 => requested[1].local.stage.name = "../invalid",
            2 => options.stage.evidence_bytes = 1,
            3 => options.stage.output.total_bytes = 1,
            4 => options.worktree.allow_initialize = false,
            5 => requested[1].local.disclosure = &empty_rules,
            _ => unreachable!(),
        }
        let error = build::prepare_local_set(&rendered, &requested, options)
            .expect_err("no prefix effects on preflight refusal");
        assert!(error.completed.is_empty(), "case {case}: {error}");
        assert!(matches!(
            *error.cause,
            build::LocalPreparationSetFailure::Site { .. }
        ));
        for site in &keys {
            unchanged(
                &journal,
                source::Sha256Digest::of(site.as_str().as_bytes()),
                &bytes,
            );
        }
        assert_eq!(
            fs::read_dir(parent.path()).expect("stage parent").count(),
            0
        );
    }
}

#[test]
fn destination_aliases_are_rejected_across_the_complete_set() {
    let fixture = published_fixture(&[]);
    let inputs = sites::acquire_set(&fixture.config(), &SITES);
    let rendered = render(&inputs);
    let keys = SITES.map(key);
    let root = private_directory();
    let journal = initialize(root.path());
    let bytes = fs::read(&journal.database).expect("baseline");
    let parent = private_directory();
    let recipe = metadata();
    let rules = disclosure_policy();
    for names in [
        ["same", "same"],
        ["One", "one"],
        ["caf\u{e9}", "cafe\u{301}"],
    ] {
        let mut requested = requests(
            &keys,
            root.path(),
            journal.header,
            parent.path(),
            &recipe,
            &rules,
        );
        for i in 0..2 {
            requested[i].local.stage.name = names[i];
        }
        let error = build::prepare_local_set(&rendered, &requested, permitted())
            .expect_err("ambiguous destinations");
        assert!(matches!(
            *error.cause,
            build::LocalPreparationSetFailure::Refused(_)
        ));
        assert!(error.completed.is_empty());
        for item in &requested {
            unchanged(&journal, item.local.stage.id, &bytes);
        }
        assert_eq!(
            fs::read_dir(parent.path()).expect("stage parent").count(),
            0
        );
    }
}

#[test]
fn unselected_and_incomplete_sets_cannot_start_preparation() {
    for (incomplete, preview) in [(false, false), (true, false), (false, true)] {
        let fixture = if incomplete {
            published_fixture(&[("theme/templates/page.html", b"{{ no_such_value }}")])
        } else {
            published_fixture(&[])
        };
        let inputs = sites::acquire_set(&fixture.config(), &SITES);
        let rendered = render_profile(&inputs, incomplete || preview, preview);
        assert_eq!(rendered.is_complete(), !incomplete);
        let keys = SITES.map(key);
        let root = private_directory();
        let journal = initialize(root.path());
        let bytes = fs::read(&journal.database).expect("baseline");
        let parent = private_directory();
        let recipe = metadata();
        let rules = disclosure_policy();
        let requested = requests(
            &keys,
            root.path(),
            journal.header,
            parent.path(),
            &recipe,
            &rules,
        );
        let error = build::prepare_local_set(&rendered, &requested, permitted())
            .expect_err("invalid render mode");
        assert!(matches!(
            *error.cause,
            build::LocalPreparationSetFailure::Refused(_)
        ));
        assert!(error.completed.is_empty());
        for item in &requested {
            unchanged(&journal, item.local.stage.id, &bytes);
        }
        assert_eq!(
            fs::read_dir(parent.path()).expect("stage parent").count(),
            0
        );
    }
}

#[test]
fn cross_member_parent_history_and_nested_destinations_are_protected_before_effects() {
    let fixture = published_fixture(&[]);
    let inputs = sites::acquire_set(&fixture.config(), &SITES);
    let rendered = render(&inputs);
    let keys = SITES.map(key);
    let root = private_directory();
    let journal = initialize(root.path());
    let bytes = fs::read(&journal.database).expect("baseline");
    let parent = private_directory();
    let independent = private_directory();
    let recipe = metadata();
    let mut with_parent = metadata();
    with_parent.parent = Some(
        fixture
            .commit
            .to_string()
            .parse()
            .expect("valid fixture commit ID"),
    );
    let history = release_commit::ParentSource::Bare {
        path: parent.path().to_owned(),
    };
    let rules = disclosure_policy();
    let mut requested = requests(
        &keys,
        root.path(),
        journal.header,
        parent.path(),
        &recipe,
        &rules,
    );
    requested[1].local.stage.parent = independent.path();
    requested[1].local.metadata = &with_parent;
    requested[1].local.parent_source = Some(&history);
    let error = build::prepare_local_set(&rendered, &requested, permitted())
        .expect_err("first site cannot stage within second site's history source");
    assert!(error.completed.is_empty());
    let build::LocalPreparationSetFailure::Site { site, .. } = error.cause.as_ref() else {
        panic!("native root separation failure")
    };
    assert_eq!(site, &keys[0]);
    for item in &requested {
        unchanged(&journal, item.local.stage.id, &bytes);
    }
    assert_eq!(
        fs::read_dir(parent.path())
            .expect("unchanged history source")
            .count(),
        0
    );

    // The native stage planner does not promise child absence. A complete-set
    // comparison must still catch a target that contains another target's parent.
    let nested = parent.path().join("one");
    fs::create_dir(&nested).expect("preexisting target parent");
    let mut requested = requests(
        &keys,
        root.path(),
        journal.header,
        parent.path(),
        &recipe,
        &rules,
    );
    requested[1].local.stage.parent = &nested;
    let error = build::prepare_local_set(&rendered, &requested, permitted())
        .expect_err("nested stage addresses overlap");
    assert!(matches!(
        *error.cause,
        build::LocalPreparationSetFailure::Refused(_)
    ));
    assert!(error.completed.is_empty());
    for item in &requested {
        unchanged(&journal, item.local.stage.id, &bytes);
    }
    assert_eq!(
        fs::read_dir(&nested)
            .expect("unchanged nested parent")
            .count(),
        0
    );
}

#[test]
fn later_native_failure_preserves_completed_prefix_and_never_recreates_on_replay() {
    let fixture = published_fixture(&[]);
    let inputs = sites::acquire_set(&fixture.config(), &SITES);
    let rendered = render(&inputs);
    let keys = SITES.map(key);
    let root = private_directory();
    let journal = initialize(root.path());
    let parent = private_directory();
    let recipe = metadata();
    let rules = disclosure_policy();
    let mut requested = requests(
        &keys,
        root.path(),
        journal.header,
        parent.path(),
        &recipe,
        &rules,
    );
    fs::create_dir(parent.path().join("other")).expect("existing foreign child");
    fs::write(
        parent.path().join("other/sentinel"),
        b"preserve unrelated work",
    )
    .expect("existing bytes");
    let error = build::prepare_local_set(&rendered, &requested, permitted())
        .expect_err("no complete-set success after collision");
    assert_eq!(error.requested, keys);
    assert_eq!(error.completed.len(), 1);
    assert_eq!(error.completed[0].site, keys[0]);
    let build::LocalPreparationSetFailure::Site {
        site,
        error: failed,
    } = error.cause.as_ref()
    else {
        panic!("native site failure required")
    };
    assert_eq!(site, &keys[1]);
    let build::LocalPreparationFailure::Stage(stage_error) = failed.cause.as_ref() else {
        panic!("stage collision required")
    };
    assert!(stage_error.writer_acquired);
    let header = stage_error
        .journal
        .expect("later writer observation survives");
    let observed = store::inspect_preparation(
        &journal.database,
        requested[0].local.stage.id,
        Some(&header),
        read(),
    )
    .expect("first site's durable completion survives");
    assert_eq!(
        observed.record.as_ref(),
        Some(&error.completed[0].preparation.record)
    );
    assert_eq!(
        fs::read(parent.path().join("other/sentinel")).expect("unchanged foreign bytes"),
        b"preserve unrelated work"
    );
    fs::rename(
        parent.path().join("one"),
        parent.path().join("retained-one"),
    )
    .expect("move only completed disposable fixture");
    for item in &mut requested {
        item.local.stage.expected = header;
    }
    let replay = build::prepare_local_set(&rendered, &requested, permitted())
        .expect_err("history never grants recreation");
    assert!(replay.completed.is_empty());
    let build::LocalPreparationSetFailure::Site { site, .. } = replay.cause.as_ref() else {
        panic!("native replay refusal")
    };
    assert_eq!(site, &keys[0]);
    assert!(!parent.path().join("one").exists());
    assert!(parent.path().join("retained-one/.git").exists());
}
