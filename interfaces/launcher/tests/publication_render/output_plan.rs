//! Real retained outputs plan safely and materialize only through explicit local consent.

use std::path::Path;

use gazette_adapter_fs::FileSystem;

use super::{Fixture, PAGE, acquire, build, bytes, fs, render, source, tempdir};

fn configuration(fixture: &Fixture, directory: &Path) -> String {
    fixture.config().replacen(
        "site='site:one'\npublic_base_url",
        &format!(
            "site='site:one'\noutput={{directory={:?}}}\npublic_base_url",
            directory.to_str().expect("native destination")
        ),
        1,
    )
}

fn snapshot(root: &Path) -> source::TreeIdentity {
    source::inspect(root, None, Default::default())
        .expect("whole disposable tree")
        .identity()
}

fn materialized_previous(rendered: &build::PublicationRender<'_>) -> build::MaterializedOutput {
    rendered
        .plan_output(&FileSystem, None, Default::default())
        .expect("first generation has no inferred ownership")
        .materialize(&FileSystem, true)
        .expect("actual successful local write/read-back establishes the receipt")
}

#[test]
fn fresh_plan_preserves_git_and_unrelated_entries_without_destination_effects() {
    let fixture = Fixture::new(&[]);
    let destination = tempdir().expect("existing destination repository");
    let repository = git2::Repository::init(destination.path()).expect("real destination Git");
    fs::write(
        destination.path().join("README.user"),
        b"unrelated owner bytes",
    )
    .expect("user sentinel");
    fs::create_dir(destination.path().join("guide")).expect("shared existing directory");
    fs::write(destination.path().join("guide/user.txt"), b"shared child").expect("unrelated child");
    let before = snapshot(destination.path());
    let administration = snapshot(repository.path());
    let inputs = acquire(&configuration(&fixture, destination.path()));
    let rendered = render(&inputs).expect("real source and Tera publication");
    let plan = rendered
        .plan_output(&FileSystem, None, Default::default())
        .expect("write-free plan alongside unrelated native files");
    assert_eq!(plan.root(), destination.path());
    assert!(std::ptr::eq(plan.generated(), rendered.site().output()));
    for inventory in [plan.observed(), plan.expected()] {
        assert!(inventory.entries().iter().all(|entry| {
            entry.path.as_str() != ".git" && !entry.path.as_str().starts_with(".git/")
        }));
        for path in ["README.user", "guide/user.txt"] {
            assert!(
                inventory
                    .entries()
                    .iter()
                    .any(|entry| entry.path.as_str() == path)
            );
        }
    }
    assert!(plan.changes().iter().all(|change| {
        !["README.user", "guide", "guide/user.txt"].contains(&change.path.as_str())
            && change.before.is_none()
            && change.after.is_some()
    }));
    assert!(
        plan.changes()
            .iter()
            .any(|change| change.path.as_str() == "guide/index.html")
    );
    assert_eq!(snapshot(destination.path()), before);
    assert_eq!(snapshot(repository.path()), administration);
}

#[test]
fn identical_native_bytes_require_independent_prior_ownership_and_then_have_no_changes() {
    let fixture = Fixture::new(&[]);
    let destination = tempdir().expect("destination");
    let inputs = acquire(&configuration(&fixture, destination.path()));
    let previous = render(&inputs).expect("prior generated publication");
    let unowned = tempdir().expect("render-only native bytes do not establish ownership");
    let unowned_inputs = acquire(&configuration(&fixture, unowned.path()));
    let unowned_render = render(&unowned_inputs).expect("render without materialization");
    fs::write(
        unowned.path().join("index.html"),
        bytes(unowned_render.site().output(), "index.html"),
    )
    .expect("an unrelated actor copied matching render bytes");
    let unowned_before = snapshot(unowned.path());
    assert_eq!(
        unowned_render
            .plan_output(&FileSystem, None, Default::default())
            .expect_err("a render and matching existing bytes are not a receipt")
            .code(),
        "output_collision"
    );
    assert_eq!(snapshot(unowned.path()), unowned_before);
    let previous = materialized_previous(&previous);
    fs::write(destination.path().join("user.txt"), b"preserved").expect("unowned sentinel");
    let before = snapshot(destination.path());
    let current = render(&inputs).expect("identical repeated retained build");
    assert_eq!(
        current
            .plan_output(&FileSystem, None, Default::default())
            .expect_err("identical bytes alone never transfer ownership")
            .code(),
        "output_collision"
    );
    let plan = current
        .plan_output(&FileSystem, Some(&previous), Default::default())
        .expect("successful same-owner materialization receipt");
    assert!(plan.changes().is_empty());
    assert_eq!(plan.expected(), plan.observed());
    assert_eq!(snapshot(destination.path()), before);
}

#[test]
fn replacement_and_obsolete_alias_removal_preserve_shared_directories_and_user_files() {
    let destination = tempdir().expect("destination");
    let old_fixture = Fixture::new(&[]);
    let old_inputs = acquire(&configuration(&old_fixture, destination.path()));
    let previous = render(&old_inputs).expect("prior publication with alias");
    let previous = materialized_previous(&previous);
    fs::write(destination.path().join("guide/user.txt"), b"not generated")
        .expect("shared-directory child");
    let page = PAGE
        .replace("aliases=['/old.html']", "aliases=[]")
        .replace("**Retained** body", "**Changed** body");
    let current_fixture = Fixture::new(&[("one/page.md", page.as_bytes())]);
    let inputs = acquire(&configuration(&current_fixture, destination.path()));
    let current = render(&inputs).expect("new complete publication");
    let before = snapshot(destination.path());
    let plan = current
        .plan_output(&FileSystem, Some(&previous), Default::default())
        .expect("bounded exact-owned changes");
    let removed = plan
        .changes()
        .iter()
        .find(|change| change.path.as_str() == "old.html")
        .expect("obsolete generated alias removal");
    assert!(matches!(removed.before, Some(source::Content::File { .. })));
    assert!(removed.after.is_none());
    let changed = plan
        .changes()
        .iter()
        .find(|change| change.path.as_str() == "guide/index.html")
        .expect("changed generated page");
    assert!(changed.before.is_some() && changed.after.is_some());
    assert_ne!(changed.before, changed.after);
    assert!(
        plan.changes()
            .iter()
            .all(|change| change.path.as_str() != "guide")
    );
    assert!(
        plan.expected()
            .entries()
            .iter()
            .any(|entry| entry.path.as_str() == "guide/user.txt")
    );
    assert!(
        !plan
            .expected()
            .entries()
            .iter()
            .any(|entry| entry.path.as_str() == "old.html")
    );
    assert_eq!(snapshot(destination.path()), before);
    assert!(
        destination.path().join("old.html").is_file(),
        "plan is not execution"
    );
}

#[test]
fn modified_or_missing_prior_generated_entries_refuse_without_mutation() {
    let fixture = Fixture::new(&[]);
    for missing in [false, true] {
        let destination = tempdir().expect("destination");
        let inputs = acquire(&configuration(&fixture, destination.path()));
        let previous = render(&inputs).expect("prior publication");
        let previous = materialized_previous(&previous);
        let path = destination.path().join("guide/index.html");
        if missing {
            fs::remove_file(path).expect("fixture actor removed generated output");
        } else {
            fs::write(path, b"user-modified generated file").expect("fixture actor edit");
        }
        let before = snapshot(destination.path());
        let current = render(&inputs).expect("same retained generation");
        let error = current
            .plan_output(&FileSystem, Some(&previous), Default::default())
            .expect_err("prior ownership requires exact current bytes");
        assert_eq!(error.code(), "output_changed");
        assert!(error.to_string().contains("guide/index.html"));
        assert_eq!(snapshot(destination.path()), before);
    }
}

#[test]
fn previous_receipt_cannot_transfer_ownership_between_roots_or_sites() {
    let fixture = Fixture::new(&[]);
    let first = tempdir().expect("first root");
    let second = tempdir().expect("second root");
    let first_inputs = acquire(&configuration(&fixture, first.path()));
    let previous = render(&first_inputs).expect("first site/root");
    let previous = materialized_previous(&previous);
    let first_before = snapshot(first.path());
    let second_config = configuration(&fixture, second.path());
    let second_inputs = acquire(&second_config);
    let current = render(&second_inputs).expect("same site, different root");
    assert_eq!(
        current
            .plan_output(&FileSystem, Some(&previous), Default::default())
            .expect_err("root is part of the owner binding")
            .code(),
        "output_prior_owner"
    );
    let other_config = fixture.config().replace(
        "site='site:other'\npublic_base_url",
        &format!(
            "site='site:other'\noutput={{directory={:?}}}\npublic_base_url",
            first.path().to_str().unwrap()
        ),
    );
    let other_inputs = build::acquire_publication(
        &other_config,
        &"site:other".parse().unwrap(),
        Default::default(),
        source::PreparationLimits {
            sources: 8,
            entries: 128,
            bytes: 1024 * 1024,
            per_source: source::RevisionLimits::default(),
        },
    )
    .expect("other site's own exact sources");
    let other = render(&other_inputs).expect("other site same configured root");
    assert_eq!(
        other
            .plan_output(&FileSystem, Some(&previous), Default::default())
            .expect_err("site is part of the owner binding")
            .code(),
        "output_prior_owner"
    );
    assert_eq!(snapshot(first.path()), first_before);
    assert!(fs::read_dir(second.path()).unwrap().next().is_none());
}

#[test]
fn absent_detached_missing_and_source_overlapping_destinations_are_not_inferred() {
    let fixture = Fixture::new(&[]);
    let parent = tempdir().expect("native output parent");
    for (config, code) in [
        (fixture.config(), "output_unconfigured"),
        (
            configuration(&fixture, Path::new("relative-output")),
            "output_coordinate",
        ),
        (
            configuration(&fixture, &parent.path().join("missing")),
            "output_observation",
        ),
        (
            configuration(&fixture, fixture.directory.path()),
            "output_observation",
        ),
    ] {
        let inputs = acquire(&config);
        let rendered = render(&inputs).expect("render itself does not use output coordinates");
        assert_eq!(
            rendered
                .plan_output(&FileSystem, None, Default::default())
                .expect_err("no implicit location or source-overlapping destination")
                .code(),
            code
        );
    }
    assert!(!parent.path().join("missing").exists());
    assert!(fs::read_dir(parent.path()).unwrap().next().is_none());
}

#[test]
fn canonical_collisions_and_complete_expected_inventory_bounds_refuse() {
    for (page, existing, directory) in [
        (PAGE.to_owned(), "GUIDE", true),
        (PAGE.replace("/old.html", "/É.html"), "e\u{301}.html", false),
    ] {
        let fixture = Fixture::new(&[("one/page.md", page.as_bytes())]);
        let destination = tempdir().expect("collision root");
        let path = destination.path().join(existing);
        if directory {
            fs::create_dir(path).unwrap();
        } else {
            fs::write(path, b"user bytes").unwrap();
        }
        let before = snapshot(destination.path());
        let inputs = acquire(&configuration(&fixture, destination.path()));
        let rendered = render(&inputs).expect("valid generated route spelling");
        assert_eq!(
            rendered
                .plan_output(&FileSystem, None, Default::default())
                .expect_err("canonical path equivalence is not ownership")
                .code(),
            "output_collision"
        );
        assert_eq!(snapshot(destination.path()), before);
    }
    let fixture = Fixture::new(&[]);
    let destination = tempdir().expect("aggregate root");
    fs::write(
        destination.path().join("sentinel"),
        b"unrelated byte budget",
    )
    .unwrap();
    let inputs = acquire(&configuration(&fixture, destination.path()));
    let rendered = render(&inputs).expect("complete output");
    let before = snapshot(destination.path());
    let limits = source::Limits {
        total_bytes: rendered.site().output().inventory().total_bytes()
            + b"unrelated byte budget".len() as u64
            - 1,
        ..Default::default()
    };
    assert_eq!(
        rendered
            .plan_output(&FileSystem, None, limits)
            .expect_err("generated plus preserved bytes exceed aggregate ceiling")
            .code(),
        "output_limit_or_structure"
    );
    assert_eq!(
        rendered
            .plan_output(
                &FileSystem,
                None,
                source::Limits {
                    entries: 2,
                    ..Default::default()
                }
            )
            .expect_err("complete proposed entry set exceeds ceiling")
            .code(),
        "output_limit_or_structure"
    );
    assert_eq!(snapshot(destination.path()), before);
}

#[test]
fn generated_file_and_directory_transitions_are_not_replacements() {
    let old_fixture = Fixture::new(&[]);
    let destination = tempdir().expect("transition root");
    let old_inputs = acquire(&configuration(&old_fixture, destination.path()));
    let previous = render(&old_inputs).expect("old alias is a generated file");
    let previous = materialized_previous(&previous);
    let page = PAGE.replace(
        "path='/guide/',aliases=['/old.html']",
        "path='/old.html/',aliases=[]",
    );
    let new_fixture = Fixture::new(&[("one/page.md", page.as_bytes())]);
    let inputs = acquire(&configuration(&new_fixture, destination.path()));
    let current = render(&inputs).expect("new native route requires a directory");
    let before = snapshot(destination.path());
    assert_eq!(
        current
            .plan_output(&FileSystem, Some(&previous), Default::default())
            .expect_err("file-to-directory transition is not ordinary owned replacement")
            .code(),
        "output_collision"
    );
    let page = PAGE.replace(
        "path='/guide/',aliases=['/old.html']",
        "path='/new/',aliases=[]",
    );
    let file_fixture = Fixture::new(&[("one/page.md", page.as_bytes())]);
    let file_inputs = acquire(&configuration(&file_fixture, destination.path()));
    let file = build::render_publication(
        &file_inputs,
        build::SiteRenderOptions {
            render_year: None,
            model_output: Some(
                &"guide"
                    .parse()
                    .expect("file destination formerly a directory"),
            ),
            graph: None,
            limits: Default::default(),
        },
        Default::default(),
    )
    .expect("new generated debug artifact requests existing directory spelling");
    assert_eq!(
        file.plan_output(&FileSystem, Some(&previous), Default::default())
            .expect_err("shared directory must not become a generated file")
            .code(),
        "output_collision"
    );
    assert_eq!(snapshot(destination.path()), before);
}

fn assert_generated_bytes(rendered: &build::PublicationRender<'_>, destination: &Path) {
    for entry in rendered.site().output().inventory().entries() {
        let path = destination.join(entry.path.as_str());
        match entry.content {
            source::Content::Directory => {
                assert!(path.is_dir(), "generated directory {}", entry.path)
            }
            source::Content::File { .. } => assert_eq!(
                fs::read(path).expect("actual materialized generated file"),
                bytes(rendered.site().output(), entry.path.as_str()),
                "exact retained bytes at {}",
                entry.path
            ),
        }
    }
}

#[test]
fn actual_materialization_reads_back_retained_bytes_and_repeated_generation_is_identical() {
    let fixture = Fixture::new(&[]);
    let source_before = snapshot(fixture.directory.path());
    let destination = tempdir().expect("existing local output repository");
    let repository = git2::Repository::init(destination.path()).expect("native output Git");
    fs::write(destination.path().join("README.user"), b"owner sentinel").unwrap();
    fs::create_dir(destination.path().join("guide")).unwrap();
    fs::write(destination.path().join("guide/user.txt"), b"shared child").unwrap();
    let administration = snapshot(repository.path());
    let inputs = acquire(&configuration(&fixture, destination.path()));
    let previous = render(&inputs).expect("real retained Tera publication");
    let plan = previous
        .plan_output(&FileSystem, None, Default::default())
        .unwrap();
    let expected = serde_json::to_value(plan.expected()).unwrap();
    let written = plan
        .materialize(&FileSystem, true)
        .expect("explicitly authorized synchronous local materialization");
    assert_eq!(serde_json::to_value(written.inventory()).unwrap(), expected);
    assert_eq!(written.site(), inputs.site());
    assert_eq!(written.root(), destination.path());
    assert_eq!(written.generated(), previous.site().output().inventory());
    assert_generated_bytes(&previous, destination.path());
    assert_eq!(
        fs::read(destination.path().join("README.user")).unwrap(),
        b"owner sentinel"
    );
    assert_eq!(
        fs::read(destination.path().join("guide/user.txt")).unwrap(),
        b"shared child"
    );
    assert_eq!(snapshot(repository.path()), administration);
    let before_repeat = snapshot(destination.path());
    let repeated = render(&inputs).expect("deterministic retained rebuild");
    let repeated_plan = repeated
        .plan_output(&FileSystem, Some(&written), Default::default())
        .unwrap();
    assert!(repeated_plan.changes().is_empty());
    let repeated_inventory = repeated_plan
        .materialize(&FileSystem, true)
        .expect("zero-change plan still requires exact read-back");
    assert_eq!(repeated_inventory.inventory(), written.inventory());
    assert_eq!(repeated_inventory.generated(), written.generated());
    assert_generated_bytes(&repeated, destination.path());
    assert_eq!(snapshot(destination.path()), before_repeat);
    assert_eq!(snapshot(fixture.directory.path()), source_before);
}

#[test]
fn actual_update_replaces_only_retained_owned_files_and_removes_obsolete_alias() {
    let destination = tempdir().expect("existing output repository");
    let repository = git2::Repository::init(destination.path()).unwrap();
    fs::write(destination.path().join("user.txt"), b"not Gazette-owned").unwrap();
    let administration = snapshot(repository.path());
    let old_fixture = Fixture::new(&[]);
    let old_inputs = acquire(&configuration(&old_fixture, destination.path()));
    let previous = render(&old_inputs).unwrap();
    let previous_receipt = previous
        .plan_output(&FileSystem, None, Default::default())
        .unwrap()
        .materialize(&FileSystem, true)
        .expect("first actual generation");
    fs::write(
        destination.path().join("guide/user.txt"),
        b"user content beside generated page",
    )
    .unwrap();
    let old_html = fs::read(destination.path().join("guide/index.html")).unwrap();
    let page = PAGE
        .replace("aliases=['/old.html']", "aliases=[]")
        .replace("**Retained** body", "**Updated** retained body");
    let fixture = Fixture::new(&[("one/page.md", page.as_bytes())]);
    let source_before = snapshot(fixture.directory.path());
    let inputs = acquire(&configuration(&fixture, destination.path()));
    let current = render(&inputs).unwrap();
    let plan = current
        .plan_output(&FileSystem, Some(&previous_receipt), Default::default())
        .unwrap();
    let expected = serde_json::to_value(plan.expected()).unwrap();
    assert!(
        plan.changes()
            .iter()
            .any(|change| change.path.as_str() == "old.html" && change.after.is_none())
    );
    let written = plan
        .materialize(&FileSystem, true)
        .expect("exact owned update and removal");
    assert_eq!(serde_json::to_value(written.inventory()).unwrap(), expected);
    assert_eq!(written.generated(), current.site().output().inventory());
    assert_generated_bytes(&current, destination.path());
    assert_ne!(
        fs::read(destination.path().join("guide/index.html")).unwrap(),
        old_html
    );
    assert!(!destination.path().join("old.html").exists());
    assert_eq!(
        fs::read(destination.path().join("user.txt")).unwrap(),
        b"not Gazette-owned"
    );
    assert_eq!(
        fs::read(destination.path().join("guide/user.txt")).unwrap(),
        b"user content beside generated page"
    );
    assert_eq!(snapshot(repository.path()), administration);
    assert_eq!(snapshot(fixture.directory.path()), source_before);
}

#[test]
fn missing_write_permission_refuses_before_any_generated_effect() {
    let fixture = Fixture::new(&[]);
    let destination = tempdir().expect("existing destination");
    fs::write(destination.path().join("sentinel"), b"unchanged").unwrap();
    let inputs = acquire(&configuration(&fixture, destination.path()));
    let rendered = render(&inputs).unwrap();
    let plan = rendered
        .plan_output(&FileSystem, None, Default::default())
        .unwrap();
    let before = snapshot(destination.path());
    let error = plan
        .materialize(&FileSystem, false)
        .expect_err("planning is not local write permission");
    assert!(matches!(
        std::error::Error::source(&error.cause)
            .and_then(|cause| cause.downcast_ref::<source::Error>()),
        Some(source::Error::InvalidInventory { reason, .. })
            if *reason == "explicit local output-write permission required"
    ));
    assert!(
        error.attempted.is_empty(),
        "no native output effect was started"
    );
    assert!(error.residue.is_none());
    assert_eq!(snapshot(destination.path()), before);
    assert!(!destination.path().join("index.html").exists());
}

#[test]
fn changed_destination_between_plan_and_write_refuses_the_original_plan() {
    let fixture = Fixture::new(&[]);
    for prior_generated in [false, true] {
        let destination = tempdir().expect("existing destination");
        fs::write(destination.path().join("sentinel"), b"observed owner bytes").unwrap();
        let inputs = acquire(&configuration(&fixture, destination.path()));
        let previous = render(&inputs).unwrap();
        let previous_receipt = prior_generated.then(|| materialized_previous(&previous));
        let current = render(&inputs).unwrap();
        let plan = current
            .plan_output(&FileSystem, previous_receipt.as_ref(), Default::default())
            .unwrap();
        let changed = if prior_generated {
            "guide/index.html"
        } else {
            "sentinel"
        };
        fs::write(
            destination.path().join(changed),
            b"actor changed bytes after plan",
        )
        .unwrap();
        let before_write = snapshot(destination.path());
        let error = plan
            .materialize(&FileSystem, true)
            .expect_err("complete original observation must still hold before effects");
        assert!(matches!(
            std::error::Error::source(&error.cause)
                .and_then(|cause| cause.downcast_ref::<source::Error>()),
            Some(source::Error::Changed(path)) if path == destination.path()
        ));
        assert!(
            error.attempted.is_empty(),
            "stale plan must not start output effects"
        );
        assert!(error.residue.is_none());
        assert_eq!(snapshot(destination.path()), before_write);
        assert_eq!(
            fs::read(destination.path().join(changed)).unwrap(),
            b"actor changed bytes after plan"
        );
    }
}

fn replace_owned_destination(root: &Path, displaced: &Path, inventory: &source::Inventory) {
    fs::rename(root, displaced).expect("move only the disposable output root");
    fs::create_dir(root).expect("a distinct native root at the same configured path");
    for entry in inventory.entries() {
        let target = root.join(entry.path.as_str());
        match entry.content {
            source::Content::Directory => fs::create_dir(target).expect("identical directory tree"),
            source::Content::File { .. } => {
                fs::write(
                    target,
                    fs::read(displaced.join(entry.path.as_str())).unwrap(),
                )
                .expect("identical bytes do not transfer destination identity");
            }
        }
    }
    assert_eq!(snapshot(root), inventory.identity());
}

#[test]
fn identical_tree_at_replaced_native_root_refuses_the_original_plan_before_effects() {
    let fixture = Fixture::new(&[]);
    for previous_generation in [false, true] {
        let parent = tempdir().expect("owned output fixture parent");
        let root = parent.path().join("output");
        let displaced = parent.path().join("original");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("sentinel"), b"unchanged owner bytes").unwrap();
        let inputs = acquire(&configuration(&fixture, &root));
        let rendered = render(&inputs).unwrap();
        let previous = previous_generation.then(|| materialized_previous(&rendered));
        let plan = rendered
            .plan_output(&FileSystem, previous.as_ref(), Default::default())
            .unwrap();
        replace_owned_destination(&root, &displaced, plan.observed());
        let original = snapshot(&displaced);
        let replacement = snapshot(&root);
        let error = plan
            .materialize(&FileSystem, true)
            .expect_err("same bytes and pathname cannot rebind the original plan");
        assert!(matches!(
            std::error::Error::source(&error.cause)
                .and_then(|cause| cause.downcast_ref::<source::Error>()),
            Some(source::Error::Changed(path)) if path == &root
        ));
        assert!(error.attempted.is_empty());
        assert!(error.residue.is_none());
        assert_eq!(snapshot(&root), replacement);
        assert_eq!(snapshot(&displaced), original);
    }
}

#[test]
fn prior_ownership_receipt_refuses_identical_output_at_a_replaced_root() {
    let parent = tempdir().expect("owned output fixture parent");
    let root = parent.path().join("output");
    let displaced = parent.path().join("original");
    fs::create_dir(&root).unwrap();
    let fixture = Fixture::new(&[]);
    let inputs = acquire(&configuration(&fixture, &root));
    let rendered = render(&inputs).unwrap();
    let previous = materialized_previous(&rendered);
    replace_owned_destination(&root, &displaced, previous.inventory());
    let replacement = snapshot(&root);
    let original = snapshot(&displaced);
    assert_eq!(
        rendered
            .plan_output(&FileSystem, Some(&previous), Default::default())
            .expect_err("receipt is bound to the original native output root")
            .code(),
        "output_prior_owner"
    );
    assert_eq!(snapshot(&root), replacement);
    assert_eq!(snapshot(&displaced), original);
}
