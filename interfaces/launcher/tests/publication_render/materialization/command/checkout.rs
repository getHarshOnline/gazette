//! Joined native configured-repository setup and ordinary journal-owned output delivery.

#[path = "checkout/carriers.rs"]
mod carriers;

use super::*;
use gazette_sdk::release;
use std::{os::unix::fs::MetadataExt, path::PathBuf, time::Duration};

struct Setup {
    _fixture: Fixture,
    _parent: tempfile::TempDir,
    roots: Vec<PathBuf>,
    config: String,
    selections: BTreeMap<Key, store::CheckoutSelection>,
    parents: BTreeMap<Key, source::CommitId>,
    existing_git: u64,
}
impl Setup {
    fn new() -> Self {
        let fixture = Fixture::new(&[]);
        let parent = tempdir().unwrap();
        let roots = vec![parent.path().join("one"), parent.path().join("other")];
        let isolation = parent.path().join("isolation");
        fs::create_dir(&isolation).unwrap();
        fs::set_permissions(&isolation, fs::Permissions::from_mode(0o700)).unwrap();
        let mut config = fixture.config();
        let mut selections = BTreeMap::new();
        let mut parents = BTreeMap::new();
        let mut existing_git = 0;
        for (i, site) in SELECTED.into_iter().enumerate() {
            let remote = parent.path().join(format!("remote-{i}.git"));
            let repo = git2::Repository::init_bare(&remote).unwrap();
            let blob = repo.blob(b"Existing remote README\n").unwrap();
            let mut tree = repo.treebuilder(None).unwrap();
            tree.insert("README.md", blob, 0o100644).unwrap();
            let tree = repo.find_tree(tree.write().unwrap()).unwrap();
            let signature =
                git2::Signature::new("Fixture", "fixture@example.invalid", &git2::Time::new(1, 0))
                    .unwrap();
            let commit = repo
                .commit(
                    Some("refs/heads/main"),
                    &signature,
                    &signature,
                    "remote README",
                    &tree,
                    &[],
                )
                .unwrap();
            let expected: source::CommitId = commit.to_string().parse().unwrap();
            parents.insert(key(site), expected.clone());
            let local = if i == 0 {
                release::OutputCheckoutState::Missing
            } else {
                let output = git2::Repository::init(&roots[i]).unwrap();
                output.remote("origin", remote.to_str().unwrap()).unwrap();
                fs::write(roots[i].join("user.txt"), b"independent owner bytes").unwrap();
                existing_git = fs::metadata(roots[i].join(".git")).unwrap().ino();
                release::OutputCheckoutState::Unborn
            };
            let anchor = format!("site='{site}'\npublic_base_url");
            assert!(config.contains(&anchor));
            config=config.replacen(&anchor,&format!("site='{site}'\noutput={{directory={:?}}}\ndestination={{kind='git',remote={:?},branch='main'}}\npublic_base_url",roots[i].to_str().unwrap(),remote.to_str().unwrap()),1);
            selections.insert(
                key(site),
                store::CheckoutSelection {
                    git: release::GitSelection {
                        program: crate::native::git().to_path_buf(),
                        isolation: isolation.clone(),
                        credentials: None,
                    },
                    expected: local,
                    history: store::CheckoutHistory::Acquire {
                        expected,
                        root: parent.path().join(format!("history-{i}.git")),
                        allow_acquire: true,
                    },
                    read: release::RemoteReadOptions {
                        allow_read: true,
                        timeout: Duration::from_secs(20),
                        ..Default::default()
                    },
                    options: release::OutputCheckoutOptions {
                        allow_initialize: true,
                        allow_update: false,
                        ..Default::default()
                    },
                },
            );
        }
        Self {
            _fixture: fixture,
            _parent: parent,
            roots,
            config,
            selections,
            parents,
            existing_git,
        }
    }
    fn run(
        &self,
        kernel: &Kernel,
        selections: BTreeMap<Key, store::CheckoutSelection>,
    ) -> command::MaterializationReport {
        kernel
            .execute(command::Materialize {
                handle: "writer".into(),
                render: render(
                    serde_json::to_vec(&sites::request(&self.config, &SELECTED)).unwrap(),
                ),
                ids: ids(),
                checkouts: Some(selections),
                limits: Default::default(),
                options: options(),
                reserve: |_: &str, _: &command::Selection, _: Option<store::JournalHeader>| {
                    Ok::<(), &'static str>(())
                },
            })
            .unwrap()
    }
}

#[test]
fn configured_checkout_acquires_readme_ancestry_then_materializes_complete_native_set() {
    let fixture = Setup::new();
    let (journal, initialized) = initialize();
    let kernel = Kernel::new();
    open_writer(&kernel, journal.path(), initialized.header);
    let bytes = serde_json::to_vec(&fixture.selections).unwrap();
    let selected = store::parse_output_checkouts(&bytes).unwrap();
    let report = fixture.run(&kernel, selected);
    assert_eq!(report.checkouts.len(), 2);
    let results = report
        .outcome
        .expect("native complete configured setup and output");
    assert_eq!(results.len(), 2);
    for (i, site) in SELECTED.into_iter().enumerate() {
        let repo = git2::Repository::open(&fixture.roots[i]).unwrap();
        assert_eq!(repo.head().unwrap().name().unwrap(), "refs/heads/main");
        assert_eq!(
            repo.head().unwrap().target().unwrap().to_string(),
            fixture.parents[&key(site)].as_str()
        );
        assert!(
            !fixture.roots[i].join("README.md").exists(),
            "history does not overwrite output or become unowned files"
        );
        assert!(fixture.roots[i].join("model.json").exists());
        assert!(results[&key(site)].history.record.completion.is_some());
        assert_eq!(
            results[&key(site)].history.record.intent.binding,
            report.checkouts[&key(site)].binding
        );
    }
    assert_eq!(
        fs::metadata(fixture.roots[1].join(".git")).unwrap().ino(),
        fixture.existing_git
    );
    assert_eq!(
        fs::read(fixture.roots[1].join("user.txt")).unwrap(),
        b"independent owner bytes"
    );
    assert!(matches!(
        operate(&kernel, "writer", true).outcome,
        command::Outcome::Closed
    ));
}

#[test]
fn configured_checkout_requires_full_set_and_preserves_completed_metadata_when_next_remote_changed()
{
    let fixture = Setup::new();
    let (journal, initialized) = initialize();
    let kernel = Kernel::new();
    open_writer(&kernel, journal.path(), initialized.header);
    let mut partial = fixture.selections.clone();
    partial.remove(&key("site:other"));
    let report = fixture.run(&kernel, partial);
    let Err(command::MaterializationFailure::Checkout(error)) = report.outcome else {
        panic!("complete membership must refuse")
    };
    assert!(error.completed.is_empty());
    assert!(!fixture.roots[0].exists());
    let mut changed = fixture.selections.clone();
    if let store::CheckoutHistory::Acquire { expected, .. } =
        &mut changed.get_mut(&key("site:other")).unwrap().history
    {
        *expected = "1111111111111111111111111111111111111111".parse().unwrap();
    }
    let report = fixture.run(&kernel, changed);
    let Err(command::MaterializationFailure::Checkout(error)) = report.outcome else {
        panic!("changed remote must refuse")
    };
    assert_eq!(error.completed.len(), 1);
    assert_eq!(error.failed, Some(key("site:other")));
    assert_eq!(error.reason, "output_checkout_remote_prestate");
    for root in &fixture.roots {
        assert!(
            !root.join("model.json").exists(),
            "complete output planning/writing never began"
        );
    }
    assert_eq!(
        fs::read(fixture.roots[1].join("user.txt")).unwrap(),
        b"independent owner bytes"
    );
    assert!(matches!(
        operate(&kernel, "writer", true).outcome,
        command::Outcome::Closed
    ));
}
