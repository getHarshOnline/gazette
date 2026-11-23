//! Same-held retry after a real receiver rejection, without replacing consumed intent.

use super::{Fixture, Repository, fs, read, release, store};
use std::os::unix::fs::PermissionsExt;

#[test]
fn rejected_receive_pack_retries_original_consumption_and_settles_without_replay_dispatch() {
    let mut fixture = Fixture::new();
    let pushes = fixture.local.control.path().join("push-calls");
    let received = fixture.local.control.path().join("received-targets");
    let allowed = fixture.local.control.path().join("receiver-accepts");
    let hook = fixture.local.paths[0].join("hooks/pre-receive");
    fs::write(
        &fixture.local.program,
        format!(
            "#!/bin/sh\nprintf x >> '{}'\nfor arg do\n if [ \"$arg\" = push ]; then printf p >> '{}'; fi\ndone\nexec {} \"$@\"\n",
            fixture.local.calls.display(),
            pushes.display(),
            crate::native::shell(crate::native::git())
        ),
    )
    .expect("count actual push invocations separately from native reads");
    fs::write(
        &hook,
        format!(
            "#!/bin/sh\nwhile read old new ref; do\n printf '%s %s %s\\n' \"$old\" \"$new\" \"$ref\" >> '{}'\ndone\ntest -f '{}'\n",
            received.display(), allowed.display()
        ),
    )
    .expect("receiver records the real requested update and initially rejects it");
    for path in [&fixture.local.program, &hook] {
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .expect("private disposable native executable");
    }
    let hook_bytes = fs::read(&hook).expect("fixed receiver hook");
    let authorization = store::inspect_release_authorization(
        &fixture.local.prepared.database,
        &fixture.consumed.authorization_id,
        Some(&fixture.local.prepared.header),
        read(),
    )
    .expect("original durable signed registration");
    let mut immutable = fixture.local.prepared.snapshot();
    immutable.remove(&fixture.local.prepared.database);
    let target = fixture.binding.ordered_targets[0].clone();
    let release::RemoteState::Commit { oid: parent } = &target.expected_remote_oid_or_unborn else {
        panic!("fixture has reviewed parent commits");
    };
    let expected_update = format!(
        "{parent} {} refs/heads/publication\n",
        target.local_commit_oid
    );

    let rejected = fixture
        .dispatch(0, 1)
        .expect("real rejected receive-pack result is retained");
    let rejection = rejected.result.as_ref().expect("immutable first result");
    assert_eq!(
        rejection
            .transport
            .expect("complete native rejection report")
            .status,
        release::DispatchStatus::Rejected
    );
    assert_eq!(rejection.readback, rejected.start.prestate);
    assert!(rejection.permits_retry());
    assert_eq!(fs::read(&pushes).expect("one actual dispatch"), b"p");
    assert_eq!(
        fs::read_to_string(&received).expect("receiver ran"),
        expected_update
    );
    for (ordinal, original) in fixture.binding.ordered_targets.iter().enumerate() {
        let release::RemoteState::Commit { oid } = &original.expected_remote_oid_or_unborn else {
            panic!("both reviewed parents exist")
        };
        let repository =
            Repository::open_bare(&fixture.local.paths[ordinal]).expect("native receiver");
        assert_eq!(
            repository
                .find_reference("refs/heads/publication")
                .expect("reviewed branch")
                .target()
                .expect("direct ref")
                .to_string(),
            oid.as_str(),
            "rejection changes neither target nor sibling"
        );
        assert_eq!(
            repository
                .references()
                .expect("complete ref namespace")
                .count(),
            1
        );
    }
    let calls = fixture.local.reads();
    assert_eq!(fixture.dispatch(0, 1).expect("rejection replay"), rejected);
    assert_eq!(
        fixture.local.reads(),
        calls,
        "replay never queries or dispatches the provider"
    );

    fs::write(&allowed, b"accept the original requested update")
        .expect("only disposable receiver acceptance changes; no new authorization");
    let retried = fixture
        .dispatch(0, 2)
        .expect("same-held retry of identical consumed target");
    assert_eq!(fs::read(&pushes).expect("two real dispatches"), b"pp");
    assert_eq!(
        fs::read_to_string(&received).expect("both actual receiver calls"),
        expected_update.repeat(2)
    );
    assert_eq!(
        fs::read(&hook).expect("same native receiver hook"),
        hook_bytes
    );
    assert_eq!(retried.start.binding, rejected.start.binding);
    assert_eq!(retried.start.prestate, rejected.start.prestate);
    assert_eq!(retried.start.created, rejected.start.created);
    assert_eq!(
        retried
            .start
            .target(&fixture.consumed)
            .expect("original target"),
        &target
    );
    let retry_result = retried.result.as_ref().expect("immutable second result");
    assert_eq!(
        retry_result.transport.expect("native update report").status,
        release::DispatchStatus::Updated
    );
    assert!(!retry_result.permits_retry());
    assert!(matches!(
        retry_result.readback,
        release::TargetReadback::Observed {
            comparison: release::RefComparison::Intended,
            ..
        }
    ));
    assert_eq!(
        fixture.history().history.attempts,
        [rejected.clone(), retried.clone()]
    );

    let sibling = fixture
        .dispatch(1, 1)
        .expect("complete the sibling from the same consumed batch");
    assert_eq!(
        fs::read(&pushes).expect("exactly three actual pushes"),
        b"ppp"
    );
    let complete = fixture
        .reconcile(1)
        .expect("complete-set native read-back and settlement");
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
    let attempts = [rejected, retried, sibling];
    for (attempt, (ordinal, number)) in attempts.iter().zip([(0, 1), (0, 2), (1, 1)]) {
        assert_eq!(
            attempt.start.key.authorization_id,
            fixture.consumed.authorization_id
        );
        assert_eq!(
            (attempt.start.key.ordinal, attempt.start.key.number),
            (ordinal, number)
        );
        assert_eq!(attempt.start.created, fixture.consumed.created);
        assert_eq!(
            attempt.result.as_ref().expect("each immutable result").key,
            attempt.start.key
        );
    }
    let calls = fixture.local.reads();
    for attempt in &attempts {
        assert_eq!(
            fixture
                .dispatch(attempt.start.key.ordinal, attempt.start.key.number)
                .expect("exact original attempt replay even after settlement"),
            *attempt
        );
    }
    let replay = fixture
        .reconcile(1)
        .expect("exact succeeded settlement replay");
    assert!(!replay.inserted);
    assert_eq!(replay.record, complete.record);
    assert_eq!(
        fixture.local.reads(),
        calls,
        "all exact replays are provider-free"
    );
    assert_eq!(fs::read(&pushes).expect("no replay dispatch"), b"ppp");
    let history = fixture.history();
    assert_eq!(history.history.consumption, Some(fixture.consumed.clone()));
    assert_eq!(history.history.attempts, attempts);
    assert_eq!(history.records, [complete.record]);
    let retained = store::inspect_release_authorization(
        &fixture.local.prepared.database,
        &fixture.consumed.authorization_id,
        Some(&fixture.local.prepared.header),
        read(),
    )
    .expect("original authorization remains the only consumed authority");
    assert_eq!(retained.record, authorization.record);
    fixture.close();
    assert_eq!(fixture.history().history.attempts, attempts);
    let mut after = fixture.local.prepared.snapshot();
    after.remove(&fixture.local.prepared.database);
    assert_eq!(
        after, immutable,
        "prepared commits, output, source and destination binding bytes never change"
    );
}
