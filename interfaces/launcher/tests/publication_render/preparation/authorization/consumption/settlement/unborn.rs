//! First publication to exact local bare destinations, with fixture-only signed authority.

use super::{Fixture, Local, Repository, fs, release, store};
use std::{os::unix::fs::PermissionsExt, process::Command};

fn absent(readback: &release::TargetReadback) {
    assert_eq!(
        readback,
        &release::TargetReadback::Observed {
            advertisement: release::RefAdvertisement::Absent,
            comparison: release::RefComparison::Prestate,
        },
        "independently qualified local absence, never generic NotAdvertised"
    );
}

#[test]
fn empty_destinations_receive_prepared_root_commits_and_settle_the_exact_signed_batch() {
    let local = Local::unborn();
    for (ordinal, record) in local.prepared.records.iter().enumerate() {
        let destination = Repository::open_bare(&local.paths[ordinal])
            .expect("independent exact empty destination inspection");
        assert_eq!(
            destination
                .references()
                .expect("complete ref namespace")
                .count(),
            0
        );
        let recipe = record
            .worktree
            .as_ref()
            .expect("actual durable Git preparation");
        assert!(recipe.metadata.parent.is_none());
        assert!(recipe.parent_source.is_none());
        let worktree = Repository::open(record.intent.address.path())
            .expect("real prepared deployment repository");
        let commit = worktree
            .find_commit(
                git2::Oid::from_str(local.prepared.selections[ordinal].commit.as_str())
                    .expect("prepared commit OID"),
            )
            .expect("native root object");
        assert_eq!(
            commit.parent_count(),
            0,
            "no manufactured ancestor for first publication"
        );
        assert!(!commit.tree().expect("complete published tree").is_empty());
    }
    let mut fixture = Fixture::from_local(local);
    let authorization: release::Authorization =
        serde_json::from_slice(&fixture.bytes).expect("original fixture-only signed authorization");
    assert_eq!(
        fixture.consumed.authorization_sha256,
        gazette_sdk::source::Sha256Digest::of(&fixture.bytes)
    );
    assert_eq!(fixture.consumed.targets.len(), 2);
    assert_eq!(fixture.consumed.readbacks.len(), 2);
    for (ordinal, target) in fixture.binding.ordered_targets.iter().enumerate() {
        assert_eq!(
            target.expected_remote_oid_or_unborn,
            release::RemoteState::Unborn
        );
        assert_eq!(fixture.consumed.targets[ordinal].target, *target);
        absent(&fixture.consumed.readbacks[ordinal].readback);
    }
    assert_eq!(fixture.consumed.authorization_id, authorization.record_id);
    let mut immutable = fixture.local.prepared.snapshot();
    immutable.remove(&fixture.local.prepared.database);

    let first = fixture
        .dispatch(0, 1)
        .expect("real create-only push of first root commit");
    absent(&first.start.prestate);
    let first_result = first.result.as_ref().expect("durable native result");
    assert_eq!(
        first_result
            .transport
            .expect("complete created-branch porcelain report")
            .status,
        release::DispatchStatus::Updated
    );
    assert!(matches!(
        first_result.readback,
        release::TargetReadback::Observed {
            comparison: release::RefComparison::Intended,
            ..
        }
    ));
    let calls = fixture.local.reads();
    assert_eq!(
        fixture.dispatch(0, 1).expect("original attempt replay"),
        first
    );
    assert_eq!(
        fixture.local.reads(),
        calls,
        "replay never invokes Git again"
    );

    let partial = fixture
        .reconcile(1)
        .expect("complete set observes the still-empty sibling");
    assert_eq!(
        partial.record.settlement.state,
        store::ReleaseSettlementState::ReconciliationRequired
    );
    assert_eq!(
        partial
            .record
            .settlement
            .targets
            .iter()
            .map(|row| row.outcome)
            .collect::<Vec<_>>(),
        [
            store::ReleaseTargetOutcome::Intended,
            store::ReleaseTargetOutcome::NoEffect
        ]
    );
    assert_eq!(
        Repository::open_bare(&fixture.local.paths[1])
            .expect("second destination")
            .references()
            .expect("second namespace")
            .count(),
        0
    );
    let second = fixture
        .dispatch(1, 1)
        .expect("same signed batch creates its second exact branch");
    absent(&second.start.prestate);
    assert_eq!(
        second
            .result
            .as_ref()
            .expect("second result")
            .transport
            .expect("complete created-branch report")
            .status,
        release::DispatchStatus::Updated
    );
    let complete = fixture
        .reconcile(2)
        .expect("fresh native read-back settles both original targets");
    assert_eq!(
        complete.record.settlement.state,
        store::ReleaseSettlementState::Succeeded
    );
    assert_eq!(complete.record.settlement.targets.len(), 2);
    assert!(
        complete
            .record
            .settlement
            .targets
            .iter()
            .all(|row| row.outcome == store::ReleaseTargetOutcome::Intended)
    );
    for (ordinal, target) in fixture.binding.ordered_targets.iter().enumerate() {
        let repository = Repository::open_bare(&fixture.local.paths[ordinal])
            .expect("independent final destination open");
        let oid = repository
            .find_reference("refs/heads/publication")
            .expect("exact created branch")
            .target()
            .expect("direct branch OID");
        assert_eq!(oid.to_string(), target.local_commit_oid.as_str());
        assert_eq!(
            repository
                .find_commit(oid)
                .expect("actual receiver commit")
                .parent_count(),
            0
        );
        assert_eq!(
            repository.references().expect("complete namespace").count(),
            1
        );
    }
    let calls = fixture.local.reads();
    let replay = fixture.reconcile(2).expect("terminal record replay");
    assert!(!replay.inserted);
    assert_eq!(replay.record, complete.record);
    fixture
        .dispatch(0, 2)
        .expect_err("terminal settlement does not grant another effect");
    assert_eq!(fixture.local.reads(), calls);
    let history = fixture.history();
    assert_eq!(history.history.consumption, Some(fixture.consumed.clone()));
    assert_eq!(history.history.attempts, [first, second]);
    assert_eq!(history.records, [partial.record, complete.record.clone()]);
    fixture.close();
    assert_eq!(fixture.history().records.last(), Some(&complete.record));
    let mut after = fixture.local.prepared.snapshot();
    after.remove(&fixture.local.prepared.database);
    assert_eq!(
        after, immutable,
        "source, prepared output and deployment metadata remain unchanged"
    );

    let output = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .arg("inspect-release-reconciliations")
        .arg(&fixture.local.prepared.database)
        .arg(&fixture.consumed.authorization_id)
        .args([
            "--allow-read",
            "--allow-coordination-files",
            "--expected-header",
        ])
        .arg(serde_json::to_string(&fixture.local.prepared.header).expect("exact held header"))
        .output()
        .expect("real CLI reads durable first-publication history");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).expect("complete CLI JSON"),
        serde_json::to_value(fixture.history()).expect("same SDK history")
    );
    assert_eq!(
        fixture.local.reads(),
        calls,
        "history inspection does not read or push destinations"
    );
}

#[test]
fn branch_created_after_absence_read_cannot_be_overwritten_by_first_publication() {
    let mut fixture = Fixture::from_local(Local::unborn());
    let repository =
        Repository::open_bare(&fixture.local.paths[0]).expect("disposable racing destination");
    let tree_id = repository
        .treebuilder(None)
        .expect("independent empty tree")
        .write()
        .expect("retain competing tree object without a ref");
    let tree = repository.find_tree(tree_id).expect("read competing tree");
    let signature = git2::Signature::new(
        "Other fixture",
        "other@example.invalid",
        &git2::Time::new(0, 0),
    )
    .expect("fixture-only competing identity");
    let competing = repository
        .commit(
            None,
            &signature,
            &signature,
            "competing first publication",
            &tree,
            &[],
        )
        .expect("real independent root commit, initially unadvertised");
    assert_eq!(
        repository
            .references()
            .expect("still empty namespace")
            .count(),
        0
    );
    let marker = fixture.local.control.path().join("racing-ref-created");
    let pushes = fixture.local.control.path().join("push-invocations");
    fs::write(&fixture.local.program, format!(
        "#!/bin/sh\nprintf x >> '{}'\nfor arg do\n if [ \"$arg\" = push ]; then\n printf p >> '{}'\n if [ ! -f '{}' ]; then\n {git} --git-dir='{}' update-ref refs/heads/publication '{}' '' || exit 97\n printf created > '{}'\n fi\n fi\ndone\nexec {git} \"$@\"\n",
        fixture.local.calls.display(), pushes.display(), marker.display(),
        fixture.local.paths[0].display(), competing, marker.display(),
        git = crate::native::shell(crate::native::git()),
    )).expect("native wrapper creates competing branch only after Gazette reaches push");
    fs::set_permissions(&fixture.local.program, fs::Permissions::from_mode(0o700))
        .expect("private real-Git wrapper");

    let rejected = fixture
        .dispatch(0, 1)
        .expect("native non-force create refusal is durable evidence");
    absent(&rejected.start.prestate);
    assert_eq!(
        fs::read(&marker).expect("race actually occurred at dispatch"),
        b"created"
    );
    let result = rejected
        .result
        .as_ref()
        .expect("rejection and independent read-back survive");
    assert_eq!(
        result
            .transport
            .expect("complete native non-fast-forward rejection report")
            .status,
        release::DispatchStatus::Rejected
    );
    assert!(
        matches!(&result.readback, release::TargetReadback::Observed {
        advertisement: release::RefAdvertisement::Commit { oid },
        comparison: release::RefComparison::Conflict,
    } if oid.as_str() == competing.to_string())
    );
    assert!(
        !result.permits_retry(),
        "changed prestate never permits automatic retry"
    );
    assert_eq!(
        repository
            .find_reference("refs/heads/publication")
            .expect("competing branch survives")
            .target(),
        Some(competing)
    );
    let calls = fixture.local.reads();
    assert_eq!(
        fixture
            .dispatch(0, 1)
            .expect("exact rejected attempt replay"),
        rejected
    );
    assert_eq!(fixture.local.reads(), calls);
    fixture
        .dispatch(0, 2)
        .expect_err("a fresh attempt cannot replace the competing branch");
    assert_eq!(fs::read(&pushes).expect("native push count"), b"p");
    let second = fixture
        .dispatch(1, 1)
        .expect("the original independent sibling remains creatable");
    assert_eq!(
        second
            .result
            .as_ref()
            .expect("second native result")
            .transport
            .expect("second created report")
            .status,
        release::DispatchStatus::Updated
    );
    let settlement = fixture
        .reconcile(1)
        .expect("complete native conflict/success reconciliation");
    assert_eq!(
        settlement.record.settlement.state,
        store::ReleaseSettlementState::PartialTerminal
    );
    assert_eq!(
        settlement
            .record
            .settlement
            .targets
            .iter()
            .map(|row| row.outcome)
            .collect::<Vec<_>>(),
        [
            store::ReleaseTargetOutcome::Conflict,
            store::ReleaseTargetOutcome::Intended
        ]
    );
    assert_eq!(
        fixture.history().history.consumption,
        Some(fixture.consumed.clone())
    );
    assert_eq!(fixture.history().history.attempts, [rejected, second]);
    assert_eq!(
        fs::read(&pushes).expect("exact two attempted effects"),
        b"pp"
    );
    fixture.close();
    assert_eq!(fixture.history().records, [settlement.record]);
    assert_eq!(
        repository
            .find_reference("refs/heads/publication")
            .expect("unmodified competing branch")
            .target(),
        Some(competing)
    );
}
