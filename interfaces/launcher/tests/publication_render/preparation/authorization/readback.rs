//! Real native advertisements over locally prepared targets, without pushes or settlement.

#[path = "readback/durable.rs"]
mod durable;

use git2::{Oid, Repository, Signature};

use super::{
    BTreeMap, Path, PathBuf, Prepared, TempDir, Value, cli, files, fs, json, private_directory,
    release,
};

struct Local {
    prepared: Prepared,
    remotes: TempDir,
    paths: [PathBuf; 2],
    isolation: TempDir,
}

impl Local {
    fn git(&self) -> release::GitSelection {
        self.git_with(crate::native::git().to_owned())
    }

    fn git_with(&self, program: PathBuf) -> release::GitSelection {
        release::GitSelection {
            program,
            isolation: self.isolation.path().to_path_buf(),
            credentials: None,
        }
    }

    fn new() -> Self {
        let remotes = private_directory();
        let paths = [
            remotes.path().join("one.git"),
            remotes.path().join("other.git"),
        ];
        for path in &paths {
            Repository::init_bare(path).expect("disposable native bare destination");
        }
        let prepared = Prepared::with_destinations(
            false,
            Some([
                paths[0].to_str().expect("native remote path"),
                paths[1].to_str().expect("native remote path"),
            ]),
        );
        Self {
            prepared,
            remotes,
            paths,
            isolation: private_directory(),
        }
    }

    fn targets(&self) -> release::TargetSet {
        let batch = self.prepared.inspect();
        for (ordinal, target) in batch.targets().iter().enumerate() {
            assert_eq!(Path::new(&target.remote), self.paths[ordinal].as_path());
        }
        release::check_targets(batch.targets(), Default::default())
            .expect("same complete native batch target selection")
    }

    fn install_intended(&self, ordinal: usize) {
        let native = Repository::open(self.prepared.records[ordinal].intent.address.path())
            .expect("existing prepared worktree");
        let destination =
            Repository::open_bare(&self.paths[ordinal]).expect("bare fixture destination");
        let source = native.odb().expect("prepared native object store");
        let output = destination.odb().expect("fixture native object store");
        source
            .foreach(|oid| {
                let object = source
                    .read(*oid)
                    .expect("exact complete native fixture object");
                assert_eq!(
                    output
                        .write(object.kind(), object.data())
                        .expect("fixture-only object copy"),
                    *oid
                );
                true
            })
            .expect("bounded real fixture object traversal");
        let oid = Oid::from_str(self.prepared.selections[ordinal].commit.as_str())
            .expect("known prepared commit");
        destination
            .reference("refs/heads/publication", oid, true, "fixture setup only")
            .expect("fixture advertises intended commit; no actual push");
    }

    fn snapshot(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        let mut snapshot = self.prepared.snapshot();
        for path in &self.paths {
            files(path, &mut snapshot);
        }
        files(self.isolation.path(), &mut snapshot);
        snapshot
    }

    fn unchanged(&self, snapshot: &BTreeMap<PathBuf, Vec<u8>>) {
        assert_eq!(
            &self.snapshot(),
            snapshot,
            "readback never changes source, journal, output, Git objects or refs"
        );
        assert!(
            fs::read_dir(self.isolation.path())
                .expect("existing isolation")
                .next()
                .is_none()
        );
    }

    fn observe(&self, targets: &release::TargetSet) -> release::ReadbackReport {
        release::observe_targets(&self.git(), targets, permitted())
            .expect("complete local native observation, not effect success")
    }

    fn envelope(&self, targets: &release::TargetSet) -> String {
        json!({"git":self.git(), "targets":targets.targets()}).to_string()
    }
}

fn permitted() -> release::RemoteReadOptions {
    release::RemoteReadOptions {
        allow_read: true,
        allow_network: false,
        ..Default::default()
    }
}

fn observed(
    row: &release::TargetReadbackRow,
    ordinal: u32,
    oid: &str,
    comparison: release::RefComparison,
) {
    assert_eq!(row.ordinal, ordinal);
    let release::TargetReadback::Observed {
        advertisement: release::RefAdvertisement::Commit { oid: actual },
        comparison: actual_comparison,
    } = &row.readback
    else {
        panic!("complete native commit advertisement expected: {row:?}");
    };
    assert_eq!(actual.as_str(), oid);
    assert_eq!(*actual_comparison, comparison);
}

#[test]
fn actual_prepared_targets_reach_sdk_cli_with_qualified_local_absence() {
    let local = Local::new();
    local.install_intended(0);
    let targets = local.targets();
    // No authorization body, active signature, time, authority acquisition or
    // execution journal is an input to this independent read-only operation.
    let snapshot = local.snapshot();
    let report = local.observe(&targets);
    assert_eq!(report.target_set_sha256, targets.identity());
    assert_eq!(report.targets.len(), 2);
    observed(
        &report.targets[0],
        0,
        local.prepared.selections[0].commit.as_str(),
        release::RefComparison::Intended,
    );
    assert_eq!(report.targets[1].ordinal, 1);
    assert!(matches!(
        report.targets[1].readback,
        release::TargetReadback::Observed {
            advertisement: release::RefAdvertisement::Absent,
            comparison: release::RefComparison::Prestate,
        }
    ));
    let expected = serde_json::to_value(&report).expect("native complete report");
    assert_eq!(expected["status"], "native-ref-observations-not-settlement");
    assert!(
        !serde_json::to_string(&expected)
            .expect("report")
            .contains("unborn")
    );
    let output = cli(
        &local.envelope(&targets),
        &["observe-release-targets", "--allow-read"],
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).expect("actual CLI native report"),
        expected
    );
    local.unchanged(&snapshot);

    local.install_intended(1);
    let snapshot = local.snapshot();
    let changed = local.observe(&targets);
    observed(
        &changed.targets[1],
        1,
        local.prepared.selections[1].commit.as_str(),
        release::RefComparison::Intended,
    );
    assert_eq!(
        changed.target_set_sha256, report.target_set_sha256,
        "observations are excluded from selected intent identity"
    );
    assert_ne!(
        serde_json::to_value(changed).expect("later observation"),
        expected
    );
    assert_eq!(
        local.targets().identity(),
        targets.identity(),
        "local publication history remains unchanged"
    );
    local.unchanged(&snapshot);
}

#[test]
fn real_conflict_prestate_and_unavailable_rows_do_not_omit_other_targets() {
    let local = Local::new();
    local.install_intended(1);
    let remote = Repository::open_bare(&local.paths[0]).expect("local bare fixture");
    let tree_oid = remote
        .treebuilder(None)
        .expect("native tree builder")
        .write()
        .expect("empty tree");
    let tree = remote.find_tree(tree_oid).expect("native empty tree");
    let signature = Signature::new("Fixture", "fixture@example.invalid", &git2::Time::new(0, 0))
        .expect("fixed local identity");
    let different = remote
        .commit(
            Some("refs/heads/publication"),
            &signature,
            &signature,
            "different native fixture commit",
            &tree,
            &[],
        )
        .expect("fixture-only competing advertised commit");
    let targets = local.targets();
    let snapshot = local.snapshot();
    let report = local.observe(&targets);
    observed(
        &report.targets[0],
        0,
        &different.to_string(),
        release::RefComparison::Conflict,
    );
    observed(
        &report.targets[1],
        1,
        local.prepared.selections[1].commit.as_str(),
        release::RefComparison::Intended,
    );

    // An independently supplied read-only selection can retain a known pre-state;
    // this is not falsely claimed to be the original root-commit preparation recipe.
    let mut prior = targets.targets().to_vec();
    prior[0].expected_remote_oid_or_unborn = release::RemoteState::Commit {
        oid: different
            .to_string()
            .parse()
            .expect("actual observed coordinate"),
    };
    let prior = release::check_targets(&prior, Default::default())
        .expect("explicit valid prior-state selection");
    let report = local.observe(&prior);
    observed(
        &report.targets[0],
        0,
        &different.to_string(),
        release::RefComparison::Prestate,
    );
    assert_ne!(prior.identity(), targets.identity());
    assert_eq!(
        prior.targets()[0].output_digest,
        targets.targets()[0].output_digest
    );

    let mut mixed = targets.targets().to_vec();
    mixed[0].remote = local
        .remotes
        .path()
        .join("missing.git")
        .to_str()
        .expect("exact missing fixture destination")
        .into();
    let mixed = release::check_targets(&mixed, Default::default())
        .expect("structurally valid unavailable destination");
    let report = local.observe(&mixed);
    assert_eq!(report.targets.len(), 2);
    assert_eq!(report.targets[0].ordinal, 0);
    assert!(
        matches!(report.targets[0].readback, release::TargetReadback::Unavailable { error }
        if error.failure == release::RemoteReadFailure::Transport)
    );
    observed(
        &report.targets[1],
        1,
        local.prepared.selections[1].commit.as_str(),
        release::RefComparison::Intended,
    );
    let output = cli(
        &local.envelope(&mixed),
        &["observe-release-targets", "--allow-read"],
    );
    assert!(
        output.status.success(),
        "a complete mixed report is not release success: {output:?}"
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).expect("mixed actual CLI report"),
        serde_json::to_value(report).expect("SDK report")
    );
    local.unchanged(&snapshot);
}

#[test]
fn read_permissions_response_limits_and_complete_cli_delivery_fail_without_native_mutation() {
    let local = Local::new();
    local.install_intended(0);
    local.install_intended(1);
    let targets = local.targets();
    let snapshot = local.snapshot();
    let error = release::observe_targets(&local.git(), &targets, Default::default())
        .expect_err("native reads require explicit acknowledgement");
    assert_eq!(error.failure, release::RemoteReadFailure::Permission);
    let report = release::observe_targets(
        &local.git(),
        &targets,
        release::RemoteReadOptions {
            max_response_bytes: 0,
            ..permitted()
        },
    )
    .expect("complete per-target failures, not truncated advertisements");
    assert_eq!(report.targets.len(), 2);
    assert!(report.targets.iter().all(
        |row| matches!(row.readback, release::TargetReadback::Unavailable { error }
        if error.failure == release::RemoteReadFailure::Limit)
    ));
    let expected =
        serde_json::to_vec(&local.observe(&targets)).expect("complete native report encoding");
    let exact = (expected.len() + 1).to_string();
    let one_under = expected.len().to_string();
    let output = cli(
        &local.envelope(&targets),
        &[
            "observe-release-targets",
            "--allow-read",
            "--max-output-bytes",
            &exact,
        ],
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, [expected.as_slice(), b"\n"].concat());
    let output = cli(
        &local.envelope(&targets),
        &[
            "observe-release-targets",
            "--allow-read",
            "--max-output-bytes",
            &one_under,
        ],
    );
    assert!(!output.status.success());
    assert!(output.stdout.is_empty(), "no partial report prefix");
    assert!(String::from_utf8_lossy(&output.stderr).contains("release_read_output_limit"));
    local.unchanged(&snapshot);
}
