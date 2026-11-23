//! Actual held-session operations share one native fence and freshly selected authority.

use super::{
    Local, PermissionsExt, consume_options, declaration, fs, options, read, release, sign, store,
    verification,
};
use git2::Repository;
use release::session::{self, Operation, Outcome, PreparedInput, Session};
use std::{collections::BTreeMap, path::PathBuf, time::Duration};

#[path = "session/https.rs"]
mod https;
#[path = "session/interfaces.rs"]
mod interfaces;

struct Selected {
    local: Local,
    open: session::OpenRequest,
    calls: PathBuf,
    current: PathBuf,
    changed: PathBuf,
    switch: PathBuf,
}

struct Fixture {
    local: Local,
    session: Session,
    input: PreparedInput,
    binding: release::Binding,
    calls: PathBuf,
    current: PathBuf,
    changed: PathBuf,
    switch: PathBuf,
}

impl Selected {
    fn new(with_authority: bool) -> Self {
        let local = Local::new(true);
        let git = local.git();
        Self::with_local(local, with_authority, git)
    }

    fn with_local(local: Local, with_authority: bool, git: session::GitSelection) -> Self {
        let command = local.control.path().join("public-authority");
        let calls = local.control.path().join("authority-calls");
        let current = local.control.path().join("public-binding.json");
        let changed = local.control.path().join("changed-public-binding.json");
        let switch = local.control.path().join("select-final-change");
        let seen = local.control.path().join("initial-consumption-observed");
        fs::write(&command, format!(
            "#!/bin/sh\nprintf x >> '{}'\nif [ -f '{}' ]; then\n if [ -f '{}' ]; then\n  exec /bin/cat '{}'\n fi\n : > '{}'\nfi\nexec /bin/cat '{}'\n",
            calls.display(), switch.display(), seen.display(), changed.display(), seen.display(), current.display()
        )).expect("fixture provider reads independently selected current public records");
        fs::set_permissions(&command, fs::Permissions::from_mode(0o700))
            .expect("private provider script");
        let selected = session::OpenRequest {
            coordinator: local
                .prepared
                .coordinator
                .as_ref()
                .expect("native enrolled anchor")
                .1
                .clone(),
            expected: local.prepared.header,
            protected_roots: local.prepared.protected.clone(),
            writer: options().writer,
            allow_anchor_write: true,
            git: Some(git),
            reviewed: None,
            authority: with_authority.then(|| session::AuthorityCommand {
                command: gazette_sdk::process::Command {
                    program: command.to_str().expect("UTF-8 fixture program").to_owned(),
                    executable: None,
                    arguments: vec![],
                    directory: local
                        .control
                        .path()
                        .to_str()
                        .expect("UTF-8 fixture directory")
                        .to_owned(),
                    environment: BTreeMap::new(),
                },
                input: String::new(),
                allow_execute: true,
                max_input_bytes: 0,
                max_output_bytes: 1 << 20,
                timeout_ms: 10_000,
                limits: Default::default(),
            }),
        };
        Self {
            local,
            open: selected,
            calls,
            current,
            changed,
            switch,
        }
    }

    fn authorize(&mut self, ready: store::JournalHeader) -> (PreparedInput, release::Binding) {
        assert!(ready.writer_fence > self.local.prepared.header.writer_fence);
        self.local.prepared.header = ready;
        assert!(
            !self.calls.exists(),
            "opening custody does not acquire authority"
        );
        assert!(
            !self.current.exists(),
            "no authorization exists before actual readiness"
        );
        // Fixture owner signs only after independently reading the actual held generation.
        let (mut body, mut binding) = declaration(&self.local.prepared.inspect());
        body.coordinator_id = "coordinator:fixture".into();
        binding.coordinator_id = "coordinator:fixture".into();
        let (bytes, binding) = sign(body, binding);
        fs::write(
            &self.current,
            serde_json::to_vec(&binding).expect("independent public binding"),
        )
        .expect("owner publishes fixture binding only after readiness");
        let input = PreparedInput {
            authorization: format!(
                " \n{}\t",
                std::str::from_utf8(&bytes).expect("exact signed bytes")
            ),
            selections: self.local.prepared.selections.clone(),
        };
        (input, binding)
    }
}

impl Fixture {
    fn new(with_authority: bool) -> Self {
        Self::from_selected(Selected::new(with_authority))
    }

    fn from_selected(mut selected: Selected) -> Self {
        let parsed =
            session::parse_open(&serde_json::to_vec(&selected.open).expect("shared open encoding"))
                .expect("real shared open parser");
        let (session, ready) = Session::open(parsed)
            .unwrap_or_else(|error| panic!("actual held readiness: {error:?}"));
        let (input, binding) = selected.authorize(ready.header);
        Self {
            local: selected.local,
            session,
            input,
            binding,
            calls: selected.calls,
            current: selected.current,
            changed: selected.changed,
            switch: selected.switch,
        }
    }

    fn execute(&mut self, operation: Operation) -> Outcome {
        let parsed = session::parse_operation(
            &serde_json::to_vec(&operation).expect("exact operation encoding"),
        )
        .expect("shared strict operation parser");
        self.session.execute(&parsed)
    }

    fn authority_calls(&self) -> usize {
        fs::read(&self.calls).map_or(0, |bytes| bytes.len())
    }

    fn register(&mut self) -> store::ReleaseAuthorizationReceipt {
        let outcome = self.execute(Operation::Register {
            expected: self.local.prepared.header,
            input: self.input.clone(),
            options: release::RegisterAuthorizationOptions {
                verification: verification(),
                record: store::ReleaseAuthorizationWriteOptions { allow_record: true },
            },
        });
        let Outcome::Registered { receipt } = outcome else {
            panic!("actual native registration: {outcome:?}")
        };
        assert_eq!(receipt.record.authorization, self.input.authorization);
        assert_eq!(receipt.record.binding.as_signed(), Some(&self.binding));
        assert_eq!(receipt.journal, self.local.prepared.header);
        receipt
    }

    fn consume(&mut self) -> Outcome {
        let mut options = consume_options();
        // This held-session fixture can qualify seven independent local Git
        // destinations while the full workspace suite is under load. Keep the
        // provider deadline bounded without turning host scheduling pressure
        // into release-semantics evidence.
        options.read.timeout = Duration::from_secs(60);
        self.execute(Operation::Consume {
            expected: self.local.prepared.header,
            input: self.input.clone(),
            options,
        })
    }

    fn reconcile(&mut self, id: &str) -> Outcome {
        self.execute(Operation::Reconcile {
            expected: self.local.prepared.header,
            authorization_id: id.to_owned(),
            number: 1,
            options: release::ReconcileReleaseOptions {
                read: release::RemoteReadOptions {
                    allow_read: true,
                    timeout: Duration::from_secs(10),
                    ..Default::default()
                },
                record: store::ReleaseReconciliationOptions {
                    allow_record: true,
                    ..Default::default()
                },
                ..Default::default()
            },
        })
    }
}

#[test]
fn one_session_connects_two_native_targets_under_exact_ready_fence_and_raw_intent() {
    let mut fixture = Fixture::new(true);
    let header = fixture.local.prepared.header;
    let mut contender = fixture.session.selection().clone();
    contender.expected = header;
    let error = Session::open(contender)
        .err()
        .expect("second writer cannot take active custody");
    assert!(
        matches!(error, session::OpenError::Acquisition { .. }),
        "{error:?}"
    );
    assert_eq!(fixture.authority_calls(), 0);
    let registered = fixture.register();
    assert_eq!(fixture.authority_calls(), 1);
    let consumed = fixture.consume();
    let Outcome::Consumed { receipt: consumed } = consumed else {
        panic!("real native consumption: {consumed:?}")
    };
    assert_eq!(consumed.journal, header);
    assert_eq!(consumed.record.created, header);
    assert_eq!(consumed.record.targets.len(), 2);
    assert_eq!(
        fixture.authority_calls(),
        3,
        "consumption refreshes authority twice"
    );
    for ordinal in 0..2 {
        let outcome = fixture.execute(Operation::Dispatch {
            expected: header,
            input: fixture.input.clone(),
            ordinal,
            number: 1,
            options: release::DispatchAttemptOptions {
                verification: verification(),
                read: release::RemoteReadOptions {
                    allow_read: true,
                    timeout: Duration::from_secs(10),
                    ..Default::default()
                },
                record: store::ReleaseAttemptOptions { allow_record: true },
                dispatch: release::DispatchOptions {
                    allow_dispatch: true,
                    timeout: Duration::from_secs(10),
                    ..Default::default()
                },
            },
        });
        let Outcome::Dispatched { attempt } = outcome else {
            panic!("native exact target dispatch: {outcome:?}")
        };
        assert_eq!(attempt.start.created, header);
        let result = attempt.result.expect("complete native effect and readback");
        assert_eq!(result.created, header);
        assert!(matches!(
            result.readback,
            release::TargetReadback::Observed {
                comparison: release::RefComparison::Intended,
                ..
            }
        ));
    }
    assert_eq!(fixture.authority_calls(), 7);
    let id = consumed.record.authorization_id.clone();
    let outcome = fixture.reconcile(&id);
    let Outcome::Reconciled { receipt: settled } = outcome else {
        panic!("native complete reconciliation: {outcome:?}")
    };
    assert_eq!(
        settled.record.settlement.state,
        store::ReleaseSettlementState::Succeeded
    );
    assert_eq!(
        fixture.authority_calls(),
        7,
        "reconciliation never requires new authority"
    );
    assert!(matches!(fixture.execute(Operation::Close), Outcome::Closed));
    assert!(fixture.session.is_closed());
    let closed = fixture.execute(Operation::Inspect);
    assert!(matches!(
        closed,
        Outcome::Failed {
            failure: session::Failure {
                cause: session::Cause::Selection {
                    code: "release_session_closed"
                },
                ..
            }
        }
    ));
    let history = store::inspect_release_authorization(
        &fixture.local.prepared.database,
        &id,
        Some(&header),
        read(),
    )
    .expect("independent closed-journal read")
    .record
    .expect("original registration");
    assert_eq!(history, registered.record);
    assert_eq!(
        fs::read(&fixture.current).expect("provider source unchanged"),
        serde_json::to_vec(&fixture.binding).expect("original public bytes")
    );

    let mut later = fixture.session.selection().clone();
    later.expected = header;
    later.authority = None;
    let (session, ready) =
        Session::open(later).unwrap_or_else(|error| panic!("clean later holder: {error:?}"));
    fixture.session = session;
    fixture.local.prepared.header = ready.header;
    let reads = fixture.local.reads();
    let outcome = fixture.reconcile(&id);
    let Outcome::Reconciled { receipt: replay } = outcome else {
        panic!("authority-free exact historical replay: {outcome:?}")
    };
    assert!(!replay.inserted);
    assert_eq!(replay.record, settled.record);
    assert_eq!(
        fixture.local.reads(),
        reads,
        "historical replay never invokes Git"
    );
    assert!(matches!(fixture.execute(Operation::Close), Outcome::Closed));
}

#[test]
fn seven_candidate_qualified_targets_reach_terminal_settlement_in_one_release_session() {
    let local = Local::seven_site_candidate_qualified();
    let git = local.git();
    let mut fixture = Fixture::from_selected(Selected::with_local(local, true, git));
    let header = fixture.local.prepared.header;
    let expected_ordinals = (0..7).collect::<Vec<_>>();

    let batch = fixture.local.prepared.inspect();
    assert_eq!(batch.targets().len(), 7, "exact qualified release set");
    assert_eq!(fixture.local.prepared.records.len(), 7);
    assert_eq!(fixture.local.prepared.selections.len(), 7);
    for (ordinal, (record, selection)) in fixture
        .local
        .prepared
        .records
        .iter()
        .zip(&fixture.local.prepared.selections)
        .enumerate()
    {
        assert_eq!(batch.targets()[ordinal].ordinal, ordinal as u32);
        let qualification = selection
            .qualification
            .as_ref()
            .and_then(release::PreparationAdmission::as_authored)
            .expect("every selected target retains authored qualification");
        assert!(qualification.candidate_policy.is_some());
        let check: serde_json::Value = serde_json::from_str(
            &record
                .check
                .as_ref()
                .expect("retained qualified check")
                .evidence,
        )
        .expect("retained check JSON");
        assert_eq!(
            check["technical_certificate"]["requirements"],
            serde_json::json!(qualification)
        );
        assert_eq!(
            check["technical_certificate"]["output"],
            serde_json::json!(record.intent.output)
        );
    }

    let authorization: release::Authorization =
        serde_json::from_str(fixture.input.authorization.trim())
            .expect("original seven-target authorization");
    assert_eq!(
        authorization
            .ordered_targets
            .iter()
            .map(|target| target.ordinal)
            .collect::<Vec<_>>(),
        expected_ordinals
    );
    assert_eq!(
        authorization.publisher_authorizations[0].authorized_target_ordinals,
        expected_ordinals
    );
    assert_eq!(
        fixture.binding.publishers[0].authorized_target_ordinals,
        expected_ordinals
    );

    let mut shrunken = fixture.input.clone();
    shrunken.selections.pop();
    let refused = fixture.execute(Operation::Register {
        expected: header,
        input: shrunken,
        options: release::RegisterAuthorizationOptions {
            verification: verification(),
            record: store::ReleaseAuthorizationWriteOptions { allow_record: true },
        },
    });
    assert!(
        matches!(
            refused,
            Outcome::Failed {
                failure: session::Failure {
                    cause: session::Cause::Verification { .. },
                    ..
                }
            }
        ),
        "six targets cannot register the signed seven-target action: {refused:?}"
    );
    assert!(
        store::inspect_release_authorization(
            &fixture.local.prepared.database,
            &authorization.record_id,
            Some(&header),
            read(),
        )
        .expect("inspect refused shrunken registration")
        .record
        .is_none()
    );

    let registered = fixture.register();
    assert_eq!(registered.record.binding.ordered_targets().len(), 7);
    let consumed = fixture.consume();
    let Outcome::Consumed { receipt: consumed } = consumed else {
        panic!("complete seven-target native consumption: {consumed:?}")
    };
    assert_eq!(consumed.record.targets.len(), 7);
    assert_eq!(
        consumed
            .record
            .targets
            .iter()
            .map(|target| target.target.ordinal)
            .collect::<Vec<_>>(),
        expected_ordinals
    );
    assert_eq!(consumed.record.leases.targets.len(), 7);
    assert_eq!(consumed.record.readbacks.len(), 7);

    for ordinal in 0..7 {
        let outcome = fixture.execute(Operation::Dispatch {
            expected: header,
            input: fixture.input.clone(),
            ordinal,
            number: 1,
            options: release::DispatchAttemptOptions {
                verification: verification(),
                read: release::RemoteReadOptions {
                    allow_read: true,
                    timeout: Duration::from_secs(10),
                    ..Default::default()
                },
                record: store::ReleaseAttemptOptions { allow_record: true },
                dispatch: release::DispatchOptions {
                    profile: release::DispatchProfile::LocalTransaction,
                    allow_dispatch: true,
                    timeout: Duration::from_secs(10),
                    ..Default::default()
                },
            },
        });
        let Outcome::Dispatched { attempt } = outcome else {
            panic!("native target {ordinal} dispatch: {outcome:?}")
        };
        assert_eq!(attempt.start.key.ordinal, ordinal);
        assert!(matches!(
            attempt.result.expect("complete native read-back").readback,
            release::TargetReadback::Observed {
                comparison: release::RefComparison::Intended,
                ..
            }
        ));

        let destination = Repository::open_bare(&fixture.local.paths[ordinal as usize])
            .expect("native destination repository");
        let oid = destination
            .refname_to_id("refs/heads/publication")
            .expect("native destination branch");
        let source = Repository::open(
            fixture.local.prepared.records[ordinal as usize]
                .intent
                .address
                .path(),
        )
        .expect("retained prepared repository");
        assert_eq!(
            destination.find_commit(oid).unwrap().tree_id(),
            source.find_commit(oid).unwrap().tree_id(),
            "destination {ordinal} contains the exact qualified prepared tree"
        );
    }

    let attempts = store::inspect_release_attempts(
        &fixture.local.prepared.database,
        &authorization.record_id,
        Some(&header),
        read(),
    )
    .expect("seven retained native attempts");
    assert_eq!(attempts.attempts.len(), 7);
    assert_eq!(
        attempts
            .attempts
            .iter()
            .map(|attempt| attempt.start.key.ordinal)
            .collect::<Vec<_>>(),
        expected_ordinals
    );

    let outcome = fixture.reconcile(&authorization.record_id);
    let Outcome::Reconciled { receipt: settled } = outcome else {
        panic!("complete seven-target settlement: {outcome:?}")
    };
    assert_eq!(
        settled.record.settlement.state,
        store::ReleaseSettlementState::Succeeded
    );
    assert_eq!(settled.record.settlement.targets.len(), 7);
    assert!(
        settled
            .record
            .settlement
            .targets
            .iter()
            .all(|target| target.outcome == store::ReleaseTargetOutcome::Intended)
    );
    assert!(matches!(fixture.execute(Operation::Close), Outcome::Closed));

    let history = store::inspect_release_reconciliations(
        &fixture.local.prepared.database,
        &authorization.record_id,
        Some(&header),
        read(),
    )
    .expect("closed journal retains the complete seven-target transaction");
    assert_eq!(history.history.attempts, attempts.attempts);
    assert_eq!(history.records, vec![settled.record]);
}

#[test]
fn independently_changed_final_authority_refuses_consumption_and_retains_acquired_evidence() {
    for revoked in [false, true] {
        let mut fixture = Fixture::new(true);
        fixture.register();
        let mut final_binding = fixture.binding.clone();
        if revoked {
            final_binding.publishers[0].state = release::KeyState::Revoked;
        } else {
            final_binding.emergency_stop = true;
        }
        fs::write(
            &fixture.changed,
            serde_json::to_vec(&final_binding).expect("changed owner record"),
        )
        .expect("retain independently revised public authority");
        fs::write(&fixture.switch, b"select fresh observation")
            .expect("select final read variation");
        let outcome = fixture.consume();
        let Outcome::Failed { failure } = outcome else {
            panic!("final authority must refuse: {outcome:?}")
        };
        assert_eq!(
            fixture.authority_calls(),
            3,
            "registration plus two fresh consumption observations"
        );
        assert!(
            matches!(failure.cause, session::Cause::Verification { .. }),
            "{failure:?}"
        );
        assert!(
            failure.authority.is_some(),
            "retain actual final authority transport observation"
        );
        let session::Evidence::Consumption {
            leases,
            observations,
        } = failure.evidence
        else {
            panic!("preserved native consumption phases")
        };
        assert_eq!(leases.expect("earlier complete lease set").targets.len(), 2);
        assert_eq!(
            observations
                .expect("earlier full native prestates")
                .targets
                .len(),
            2
        );
        fixture
            .local
            .assert_pending(fixture.input.authorization.as_bytes());
        assert!(matches!(fixture.execute(Operation::Close), Outcome::Closed));
    }
}

#[test]
fn absent_authority_allows_inspection_and_close_but_drop_preserves_interrupted_custody() {
    let mut fixture = Fixture::new(false);
    assert!(matches!(
        fixture.execute(Operation::Inspect),
        Outcome::Inspected { .. }
    ));
    let denied = fixture.execute(Operation::Register {
        expected: fixture.local.prepared.header,
        input: fixture.input.clone(),
        options: release::RegisterAuthorizationOptions {
            verification: verification(),
            record: store::ReleaseAuthorizationWriteOptions { allow_record: true },
        },
    });
    assert!(matches!(
        denied,
        Outcome::Failed {
            failure: session::Failure {
                cause: session::Cause::Selection {
                    code: "release_session_authority_unconfigured"
                },
                ..
            }
        }
    ));
    assert_eq!(fixture.authority_calls(), 0);
    let mut reopen = fixture.session.selection().clone();
    reopen.expected = fixture.local.prepared.header;
    assert!(matches!(fixture.execute(Operation::Close), Outcome::Closed));
    let (session, ready) = Session::open(reopen.clone())
        .unwrap_or_else(|error| panic!("explicit clean close permits later holder: {error:?}"));
    reopen.expected = ready.header;
    drop(session);
    let state = store::coordinator::inspect(
        &reopen.coordinator.anchor_root,
        &reopen.coordinator.binding,
        options().writer.session,
    )
    .expect("inspect exact independent interrupted anchor");
    assert!(
        matches!(state,store::coordinator::LocalCoordinatorState::InProgress {reserved_generation,..} if reserved_generation==ready.header.writer_fence)
    );
    let refused = Session::open(reopen)
        .err()
        .expect("drop cannot silently seal or repair interrupted custody");
    assert!(
        matches!(refused, session::OpenError::Acquisition { .. }),
        "{refused:?}"
    );
}
