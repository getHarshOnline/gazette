//! Real native plain approval, complete-set delivery and historical replay without signing.

use super::{Local, consume_options, fs, options, read, release, store, verification};
use release::session::{self, Operation, Outcome, Session};
use std::time::Duration;

struct Fixture {
    local: Local,
    session: Session,
    input: session::ReviewedInput,
}

impl Fixture {
    fn new() -> Self {
        let mut local = Local::candidate_qualified();
        let mut source = local.prepared.fixture.config();
        for (ordinal, slug) in ["one", "other"].into_iter().enumerate() {
            source = super::super::enable_site(source, slug);
            let anchor = format!("site='site:{slug}'\npublic_base_url=");
            assert!(source.contains(&anchor));
            source = source.replace(&anchor, &format!(
                "site='site:{slug}'\ndestination={{kind='git',remote={:?},branch='publication'}}\npublic_base_url=",
                local.paths[ordinal].to_str().unwrap()));
        }
        let open = session::OpenRequest {
            coordinator: local.prepared.coordinator.as_ref().unwrap().1.clone(),
            expected: local.prepared.header,
            protected_roots: local.prepared.protected.clone(),
            writer: options().writer,
            allow_anchor_write: true,
            git: Some(local.git()),
            authority: None,
            reviewed: Some(release::ReviewedSelection {
                workspace: gazette_sdk::build::WorkspaceInput::Supplied { source },
                sites: Some(vec![
                    "site:one".parse().unwrap(),
                    "site:other".parse().unwrap(),
                ]),
                credentials: vec![release::ReviewedCredential {
                    publisher: "owner:one".parse().unwrap(),
                    owner: "fixture-owner".into(),
                    credential: release::GitCredentialSelection {
                        helper: "/explicit/fixture-owner".into(),
                        allow_credentials: false,
                    },
                }],
                allow_read: false,
                limits: Default::default(),
            }),
        };
        let parsed = session::parse_open(&serde_json::to_vec(&open).unwrap()).unwrap();
        let (mut session, ready) = Session::open(parsed).unwrap();
        local.prepared.header = ready.header;
        let before = fs::read(&local.prepared.database).unwrap();
        let review = session.execute(&Operation::Review {
            expected: ready.header,
            input: session::ReviewInput {
                record_id: "reviewed-fixture".into(),
                nonce: "reviewed-once".into(),
                approved_at: 12,
                selections: local.prepared.selections.clone(),
            },
            options: verification(),
        });
        let Outcome::Reviewed { approval } = review else {
            panic!("complete review: {review:?}")
        };
        assert_eq!(approval.ordered_targets.len(), 2);
        assert_eq!(
            approval.publisher_approvals[0].authorized_target_ordinals,
            [0, 1]
        );
        assert_eq!(
            approval.publisher_approvals[0].credential_owner,
            "fixture-owner"
        );
        assert_eq!(
            fs::read(&local.prepared.database).unwrap(),
            before,
            "review does not approve or write history"
        );
        assert_eq!(local.reads(), 0, "review does not contact destinations");
        let input = session::ReviewedInput {
            approval: format!(" \n{}\t", serde_json::to_string(&approval).unwrap()),
            confirmed_sha256: approval.canonical_body_sha256,
            observed_at: 12,
            selections: local.prepared.selections.clone(),
        };
        Self {
            local,
            session,
            input,
        }
    }

    fn execute(&mut self, operation: Operation) -> Outcome {
        let bytes = serde_json::to_vec(&operation).unwrap();
        let parsed = session::parse_operation(&bytes).unwrap();
        self.session.execute(&parsed)
    }

    fn register(&mut self, input: session::ReviewedInput) -> Outcome {
        self.execute(Operation::RegisterReviewed {
            expected: self.local.prepared.header,
            input,
            options: release::RegisterAuthorizationOptions {
                verification: verification(),
                record: store::ReleaseAuthorizationWriteOptions { allow_record: true },
            },
        })
    }

    fn dispatch(&mut self, ordinal: u32, number: u64) -> Outcome {
        self.execute(Operation::DispatchReviewed {
            expected: self.local.prepared.header,
            input: self.input.clone(),
            ordinal,
            number,
            options: release::DispatchAttemptOptions {
                verification: verification(),
                read: release::RemoteReadOptions {
                    allow_read: true,
                    timeout: Duration::from_secs(30),
                    ..Default::default()
                },
                record: store::ReleaseAttemptOptions { allow_record: true },
                dispatch: release::DispatchOptions {
                    profile: release::DispatchProfile::LocalTransaction,
                    allow_dispatch: true,
                    timeout: Duration::from_secs(30),
                    ..Default::default()
                },
            },
        })
    }
}

#[test]
fn reviewed_native_complete_set_registers_consumes_dispatches_and_reconciles_without_authority() {
    let mut fixture = Fixture::new();
    let header = fixture.local.prepared.header;
    assert!(fixture.session.selection().authority.is_none());
    let registered = fixture.register(fixture.input.clone());
    let Outcome::Registered { receipt } = registered else {
        panic!("plain register: {registered:?}")
    };
    assert_eq!(receipt.record.authorization, fixture.input.approval);
    assert!(matches!(
        receipt.record.binding,
        release::ReleaseBinding::Reviewed(_)
    ));
    let consumed = fixture.execute(Operation::ConsumeReviewed {
        expected: header,
        input: fixture.input.clone(),
        options: consume_options(),
    });
    let Outcome::Consumed { receipt } = consumed else {
        panic!("plain consume: {consumed:?}")
    };
    assert_eq!(receipt.record.targets.len(), 2);
    assert_eq!(receipt.record.readbacks.len(), 2);
    let reads = fixture.local.reads();
    assert!(reads >= 2, "both pre-states read independently");
    let twice = fixture.execute(Operation::ConsumeReviewed {
        expected: header,
        input: fixture.input.clone(),
        options: consume_options(),
    });
    assert!(twice.is_error(), "plain approval is one-use");
    assert_eq!(
        fixture.local.reads(),
        reads,
        "no re-observation after prior consumption"
    );
    for ordinal in 0..2 {
        let dispatched = fixture.dispatch(ordinal, 1);
        let Outcome::Dispatched { attempt } = dispatched else {
            panic!("plain dispatch: {dispatched:?}")
        };
        assert!(matches!(
            attempt.result.as_ref().unwrap().readback,
            release::TargetReadback::Observed {
                comparison: release::RefComparison::Intended,
                ..
            }
        ));
        let before = fixture.local.reads();
        let replay = fixture.dispatch(ordinal, 1);
        let Outcome::Dispatched { attempt: replay } = replay else {
            panic!("exact replay")
        };
        assert_eq!(replay, attempt);
        assert_eq!(
            fixture.local.reads(),
            before,
            "replay never contacts the transport"
        );
        assert!(
            fixture.dispatch(ordinal, 2).is_error(),
            "success cannot be a retry"
        );
    }
    let settled = fixture.execute(Operation::Reconcile {
        expected: header,
        authorization_id: "reviewed-fixture".into(),
        number: 1,
        options: release::ReconcileReleaseOptions {
            read: release::RemoteReadOptions {
                allow_read: true,
                timeout: Duration::from_secs(30),
                ..Default::default()
            },
            record: store::ReleaseReconciliationOptions {
                allow_record: true,
                ..Default::default()
            },
            ..Default::default()
        },
    });
    let Outcome::Reconciled { receipt } = settled else {
        panic!("complete settlement: {settled:?}")
    };
    assert_eq!(
        receipt.record.settlement.state,
        store::ReleaseSettlementState::Succeeded
    );
    assert!(matches!(fixture.execute(Operation::Close), Outcome::Closed));
    let history = store::inspect_release_attempts(
        &fixture.local.prepared.database,
        "reviewed-fixture",
        Some(&header),
        read(),
    )
    .unwrap();
    assert_eq!(history.attempts.len(), 2);
}

#[test]
fn reviewed_confirmation_scope_and_complete_set_refuse_substitution_before_registration() {
    let mut fixture = Fixture::new();
    for case in 0..4 {
        let mut input = fixture.input.clone();
        match case {
            0 => input.confirmed_sha256 = gazette_sdk::source::Sha256Digest::of(b"not confirmed"),
            1 => {
                input.selections.pop();
            }
            2 => input.selections.reverse(),
            _ => {
                let mut body: release::ReviewedApproval =
                    serde_json::from_str(&input.approval).unwrap();
                body.publisher_approvals[0].credential_owner = "other-owner".into();
                body.canonical_body_sha256 =
                    release::reviewed_body_digest(&body, release::Limits::default()).unwrap();
                input.confirmed_sha256 = body.canonical_body_sha256;
                input.approval = serde_json::to_string(&body).unwrap();
            }
        }
        let result = fixture.register(input);
        assert!(result.is_error(), "case {case}: {result:?}");
        assert!(
            store::inspect_release_authorization(
                &fixture.local.prepared.database,
                "reviewed-fixture",
                None,
                read()
            )
            .unwrap()
            .record
            .is_none()
        );
        assert_eq!(fixture.local.reads(), 0);
    }
    assert!(matches!(
        fixture.register(fixture.input.clone()),
        Outcome::Registered { .. }
    ));
    assert!(matches!(fixture.execute(Operation::Close), Outcome::Closed));
}
