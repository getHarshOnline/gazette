//! Complete configured destinations use retained renders and real synchronous native writes.

use std::{
    collections::BTreeMap,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
};

use gazette_adapter_fs::FileSystem;
use gazette_sdk::workspace::Key;

use super::{Fixture, build, bytes, fs, sites, source, tempdir};

const SELECTED: [&str; 2] = ["site:one", "site:other"];

fn key(site: &str) -> Key {
    site.parse().expect("exact site")
}

fn destinations(config: &str, outputs: &[(&str, &Path)]) -> String {
    let mut configured = config.to_owned();
    for (site, path) in outputs {
        configured = configured.replacen(
            &format!("site='{site}'\npublic_base_url"),
            &format!(
                "site='{site}'\noutput={{directory={:?}}}\npublic_base_url",
                path.to_str().expect("native output directory")
            ),
            1,
        );
    }
    configured
}

fn render_set<'a>(
    inputs: &'a build::AcquiredPublicationSet,
    selected: &[&str],
    model: &str,
) -> build::RenderedPublicationSet<'a> {
    let page = "page.html".parse().expect("page");
    let alias = "alias.html".parse().expect("alias");
    let model = model.parse().expect("explicit model output");
    build::render_publications(
        inputs,
        sites::options(selected, &page, &alias, &model),
        Default::default(),
    )
    .expect("real complete shared rendering phase")
}

fn snapshot(path: &Path) -> source::TreeIdentity {
    source::inspect(path, None, Default::default())
        .expect("actual complete disposable tree")
        .identity()
}

fn assert_generated(rendered: &build::PublicationRender<'_>, root: &Path) {
    for entry in rendered.site().output().inventory().entries() {
        match entry.content {
            source::Content::Directory => assert!(root.join(entry.path.as_str()).is_dir()),
            source::Content::File { .. } => assert_eq!(
                fs::read(root.join(entry.path.as_str())).expect("native retained readback"),
                bytes(rendered.site().output(), entry.path.as_str()),
                "exact output {}",
                entry.path
            ),
        }
    }
}

fn user_root(path: &Path, pointer: bool) {
    fs::create_dir_all(path).expect("existing output root");
    fs::write(path.join("user.txt"), b"not generated ownership").expect("user bytes");
    if pointer {
        fs::write(path.join(".git"), b"gitdir: ../private-admin\n")
            .expect("administrative pointer");
    } else {
        fs::create_dir(path.join(".git")).expect("administrative directory");
        fs::write(path.join(".git/private"), b"Git bytes are unrelated").expect("Git bytes");
    }
}

struct PermissionRestore {
    path: PathBuf,
    original: fs::Permissions,
}
impl PermissionRestore {
    fn readonly(path: &Path) -> Self {
        let original = fs::metadata(path)
            .expect("directory permissions")
            .permissions();
        fs::set_permissions(path, fs::Permissions::from_mode(0o500))
            .expect("read/search-only directory");
        Self {
            path: path.to_owned(),
            original,
        }
    }
}
impl Drop for PermissionRestore {
    fn drop(&mut self) {
        let _ = fs::set_permissions(&self.path, self.original.clone());
    }
}

#[test]
fn complete_native_output_set_preserves_git_and_users_and_replays_actual_private_receipts() {
    let fixture = Fixture::new(&[]);
    let parent = tempdir().expect("disjoint destinations");
    let roots = [parent.path().join("one"), parent.path().join("other")];
    user_root(&roots[0], false);
    user_root(&roots[1], true);
    let git = snapshot(&roots[0].join(".git"));
    let config = destinations(
        &fixture.config(),
        &[(SELECTED[0], &roots[0]), (SELECTED[1], &roots[1])],
    );
    let inputs = sites::acquire_set(&config, &SELECTED);
    let rendered = render_set(&inputs, &SELECTED, "model.json");
    assert!(rendered.is_complete());
    fs::write(
        fixture.directory.path().join("one/page.md"),
        b"changed source after retained rendering",
    )
    .expect("test-owned source drift");
    fs::write(
        fixture.directory.path().join("theme/templates/page.html"),
        b"changed template after retained rendering",
    )
    .expect("test-owned template drift");
    let source_before = snapshot(fixture.directory.path());
    let before = roots.each_ref().map(|root| snapshot(root));
    let plan = build::plan_outputs(&rendered, &BTreeMap::new(), Default::default())
        .expect("complete write-free preflight");
    assert_eq!(
        plan.sites().keys().cloned().collect::<Vec<_>>(),
        SELECTED.map(key)
    );
    let expected = plan
        .sites()
        .iter()
        .map(|(site, plan)| {
            (
                site.clone(),
                serde_json::to_value(plan.expected()).expect("complete expected tree"),
            )
        })
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        roots.each_ref().map(|root| snapshot(root)),
        before,
        "planning causes no writes"
    );
    let receipts =
        build::materialize_outputs(plan, true).expect("all actual native writes and readbacks");
    for (site, root) in SELECTED.into_iter().zip(&roots) {
        let id = key(site);
        let receipt = &receipts[&id];
        let publication = rendered.sites()[&id].as_ref().expect("retained site");
        assert_eq!(receipt.site(), &id);
        assert_eq!(receipt.root(), root);
        assert_eq!(receipt.generated(), publication.site().output().inventory());
        assert_eq!(
            serde_json::to_value(receipt.inventory()).expect("native receipt"),
            expected[&id]
        );
        assert_generated(publication, root);
        assert_eq!(
            fs::read(root.join("user.txt")).expect("user remains"),
            b"not generated ownership"
        );
    }
    assert_eq!(snapshot(&roots[0].join(".git")), git);
    assert_eq!(
        fs::read(roots[1].join(".git")).expect("Git pointer preserved"),
        b"gitdir: ../private-admin\n"
    );
    let first = roots.each_ref().map(|root| snapshot(root));
    let repeat = rendered
        .plan_outputs(&FileSystem, &receipts, Default::default())
        .expect("same exact retained receipts");
    assert!(
        repeat
            .sites()
            .values()
            .all(|plan| plan.changes().is_empty())
    );
    let repeated = build::materialize_outputs(repeat, true).expect("complete no-op readback");
    for site in SELECTED {
        assert_eq!(
            repeated[&key(site)].inventory(),
            receipts[&key(site)].inventory()
        );
        assert_eq!(
            repeated[&key(site)].generated(),
            receipts[&key(site)].generated()
        );
    }
    assert_eq!(roots.each_ref().map(|root| snapshot(root)), first);
    assert_eq!(
        snapshot(fixture.directory.path()),
        source_before,
        "materialization preserves changed native inputs while using retained bytes"
    );
}

#[test]
fn any_missing_or_invalid_destination_refuses_the_full_set_before_every_write() {
    for invalid in ["unconfigured", "missing", "file"] {
        let fixture = Fixture::new(&[]);
        let parent = tempdir().expect("destination owner");
        let one = parent.path().join("one");
        user_root(&one, false);
        let other = parent.path().join("other");
        if invalid == "file" {
            fs::write(&other, b"not a destination directory").expect("invalid root");
        }
        let bindings = if invalid == "unconfigured" {
            vec![(SELECTED[0], one.as_path())]
        } else {
            vec![(SELECTED[0], one.as_path()), (SELECTED[1], other.as_path())]
        };
        let inputs = sites::acquire_set(&destinations(&fixture.config(), &bindings), &SELECTED);
        let rendered = render_set(&inputs, &SELECTED, "model.json");
        let before = snapshot(parent.path());
        let error = build::plan_outputs(&rendered, &BTreeMap::new(), Default::default())
            .expect_err("one failed destination forbids any writable subset");
        assert_eq!(error.requested, SELECTED.map(key));
        let build::OutputSetPlanFailure::Site { site, error } = error.cause else {
            panic!("exact failed destination required");
        };
        assert_eq!(site, key("site:other"));
        assert_eq!(
            error.code(),
            if invalid == "unconfigured" {
                "output_unconfigured"
            } else {
                "output_observation"
            }
        );
        assert_eq!(snapshot(parent.path()), before);
        assert!(!one.join("index.html").exists());
    }
}

#[test]
fn equal_nested_and_ancestor_alias_output_roots_are_refused_without_effects() {
    for case in ["equal", "nested", "ancestor-alias"] {
        let fixture = Fixture::new(&[]);
        let parent = tempdir().expect("destination owner");
        let one = parent.path().join("real/selected");
        user_root(&one, false);
        let other = match case {
            "equal" => one.clone(),
            "nested" => {
                let nested = one.join("nested");
                fs::create_dir(&nested).expect("nested destination");
                nested
            }
            _ => {
                symlink(parent.path().join("real"), parent.path().join("alias"))
                    .expect("ancestor alias");
                parent.path().join("alias/selected")
            }
        };
        let inputs = sites::acquire_set(
            &destinations(
                &fixture.config(),
                &[(SELECTED[0], &one), (SELECTED[1], &other)],
            ),
            &SELECTED,
        );
        let rendered = render_set(&inputs, &SELECTED, "model.json");
        let before = snapshot(&one);
        let error = build::plan_outputs(&rendered, &BTreeMap::new(), Default::default())
            .expect_err("native identities/ancestry, not path spelling");
        assert_eq!(error.requested, SELECTED.map(key));
        assert!(matches!(
            error.cause,
            build::OutputSetPlanFailure::Site {
                error: build::OutputPlanError::Observation(
                    source::DestinationError::SourceOverlap { .. }
                ),
                ..
            }
        ));
        assert_eq!(snapshot(&one), before);
        assert!(!one.join("index.html").exists());
    }
}

#[test]
fn aggregate_output_bounds_charge_complete_observed_and_proposed_trees() {
    let fixture = Fixture::new(&[]);
    let parent = tempdir().expect("destination owner");
    let roots = [parent.path().join("one"), parent.path().join("other")];
    for root in &roots {
        user_root(root, false);
    }
    let inputs = sites::acquire_set(
        &destinations(
            &fixture.config(),
            &[(SELECTED[0], &roots[0]), (SELECTED[1], &roots[1])],
        ),
        &SELECTED,
    );
    let rendered = render_set(&inputs, &SELECTED, "model.json");
    let plan = build::plan_outputs(&rendered, &BTreeMap::new(), Default::default())
        .expect("measure actual complete native plans");
    let entries = plan
        .sites()
        .values()
        .map(|plan| plan.observed().entries().len() + plan.expected().entries().len())
        .sum();
    let total_bytes = plan
        .sites()
        .values()
        .map(|plan| plan.observed().total_bytes() + plan.expected().total_bytes())
        .sum();
    let exact = source::Limits {
        entries,
        total_bytes,
        ..Default::default()
    };
    drop(plan);
    let before = roots.each_ref().map(|root| snapshot(root));
    assert_eq!(
        build::plan_outputs(&rendered, &BTreeMap::new(), exact)
            .expect("exact aggregate boundary")
            .sites()
            .len(),
        2
    );
    for limits in [
        source::Limits {
            entries: entries - 1,
            ..exact
        },
        source::Limits {
            total_bytes: total_bytes - 1,
            ..exact
        },
    ] {
        let error = build::plan_outputs(&rendered, &BTreeMap::new(), limits)
            .expect_err("one-byte/entry shortage is not a per-site allowance");
        assert_eq!(error.requested, SELECTED.map(key));
        match &error.cause {
            build::OutputSetPlanFailure::Refused(reason) => assert!(
                [
                    "complete observed/proposed entry bound exhausted",
                    "complete observed/proposed byte bound exhausted"
                ]
                .contains(reason)
            ),
            build::OutputSetPlanFailure::Site { error, .. } => {
                let mut cause: Option<&dyn std::error::Error> = Some(error);
                let mut bounded = false;
                while let Some(current) = cause {
                    bounded |= matches!(
                        current.downcast_ref::<source::Error>(),
                        Some(source::Error::Limit { .. })
                    );
                    cause = current.source();
                }
                assert!(
                    bounded,
                    "original native or complete-tree limit remains visible: {error}"
                );
            }
        }
        assert_eq!(roots.each_ref().map(|root| snapshot(root)), before);
    }
}

#[test]
fn incomplete_render_retains_requested_membership_and_never_reads_invalid_destinations() {
    let fixture = Fixture::new(&[]);
    let parent = tempdir().expect("missing destination parent");
    let roots = [
        parent.path().join("missing-one"),
        parent.path().join("missing-other"),
    ];
    let inputs = sites::acquire_set(
        &destinations(
            &fixture.config(),
            &[(SELECTED[0], &roots[0]), (SELECTED[1], &roots[1])],
        ),
        &SELECTED,
    );
    let page = "page.html".parse().expect("page");
    let alias = "alias.html".parse().expect("alias");
    let model = "model.json".parse().expect("model");
    let collision = "index.html".parse().expect("colliding first site output");
    let mut options = sites::options(&SELECTED, &page, &alias, &model);
    options
        .sites
        .get_mut(&key("site:one"))
        .expect("first requested site")
        .model_output = Some(&collision);
    let rendered = build::render_publications(&inputs, options, Default::default())
        .expect("complete result with isolated rendering failure");
    assert!(!rendered.is_complete());
    assert!(rendered.sites()[&key("site:one")].is_err());
    assert!(rendered.sites()[&key("site:other")].is_ok());
    let error = build::plan_outputs(&rendered, &BTreeMap::new(), Default::default())
        .expect_err("render completeness precedes native destination access");
    assert_eq!(error.requested, SELECTED.map(key));
    assert!(matches!(
        error.cause,
        build::OutputSetPlanFailure::Refused("every requested site must have a complete render")
    ));
    assert_eq!(
        fs::read_dir(parent.path())
            .expect("no destination invented")
            .count(),
        0
    );
}

#[test]
fn later_native_failure_preserves_completed_receipt_partial_paths_and_unattempted_site() {
    let (_fixture, base, _) = sites::seven_site_fixture();
    let selected = ["site:rawthoughts", "site:getharsh", "site:causality"];
    let ordered = ["site:causality", "site:getharsh", "site:rawthoughts"];
    let parent = tempdir().expect("three disjoint destinations");
    let roots = [
        parent.path().join("causality"),
        parent.path().join("getharsh"),
        parent.path().join("rawthoughts"),
    ];
    for root in &roots {
        user_root(root, true);
    }
    fs::create_dir(roots[1].join("getharsh")).expect("preexisting shared readonly child");
    let config = destinations(
        &base,
        &ordered
            .into_iter()
            .zip(roots.iter().map(PathBuf::as_path))
            .collect::<Vec<_>>(),
    );
    let inputs = sites::acquire_set(&config, &selected);
    let rendered = render_set(&inputs, &selected, "a-model.json");
    assert!(rendered.is_complete());
    let plan = build::plan_outputs(&rendered, &BTreeMap::new(), Default::default())
        .expect("complete preflight before permission effects");
    let expected_first = serde_json::to_value(plan.sites()[&key(ordered[0])].expected())
        .expect("complete first-site target");
    let remaining_before = snapshot(&roots[2]);
    let permissions = PermissionRestore::readonly(&roots[1].join("getharsh"));
    let original_mode = permissions.original.mode();
    let error = build::materialize_outputs(plan, true)
        .expect_err("real later permission failure, not complete-set success");
    assert_eq!(error.requested, ordered.map(key));
    assert_eq!(error.failed, key("site:getharsh"));
    assert_eq!(
        error.completed.keys().cloned().collect::<Vec<_>>(),
        [key("site:causality")]
    );
    let completed = &error.completed[&key("site:causality")];
    assert_eq!(
        serde_json::to_value(completed.inventory()).expect("actual receipt"),
        expected_first
    );
    assert_generated(
        rendered.sites()[&key("site:causality")]
            .as_ref()
            .expect("first retained output"),
        &roots[0],
    );
    assert_eq!(
        error
            .error
            .attempted
            .iter()
            .map(|path| path.as_str())
            .collect::<Vec<_>>(),
        ["a-model.json", "getharsh/index.html"]
    );
    assert!(
        error.error.residue.is_none(),
        "first temporary settled and failed second open acquired none"
    );
    assert!(
        matches!(error.error.cause, source::DestinationError::Observation(source::Error::Io { ref source, .. }) if source.kind() == std::io::ErrorKind::PermissionDenied)
    );
    assert_eq!(
        fs::read(roots[1].join("a-model.json")).expect("failed site's first effect retained"),
        bytes(
            rendered.sites()[&key("site:getharsh")]
                .as_ref()
                .expect("second retained render")
                .site()
                .output(),
            "a-model.json"
        )
    );
    assert!(!roots[1].join("getharsh/index.html").exists());
    assert_eq!(
        snapshot(&roots[2]),
        remaining_before,
        "subsequent site was never attempted"
    );
    for root in &roots {
        assert_eq!(
            fs::read(root.join("user.txt")).expect("user remains"),
            b"not generated ownership"
        );
        assert_eq!(
            fs::read(root.join(".git")).expect("administration remains"),
            b"gitdir: ../private-admin\n"
        );
    }
    assert_eq!(
        fs::read_dir(roots[1].join("getharsh"))
            .expect("no temporary in denied child")
            .count(),
        0
    );
    assert!(
        fs::read_dir(&roots[1])
            .expect("no unsettled temporary")
            .all(|entry| !entry
                .expect("native entry")
                .file_name()
                .to_string_lossy()
                .starts_with(".gazette-write-"))
    );
    drop(permissions);
    assert_eq!(
        fs::metadata(roots[1].join("getharsh"))
            .expect("restored mode")
            .permissions()
            .mode(),
        original_mode
    );
}
