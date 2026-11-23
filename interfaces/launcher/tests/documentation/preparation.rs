//! Retained provider inputs acquire local history, never authored-publication approval.

use super::{Fixture, Path, TempDir, Value, acquire, artifact, build, file, fs, json, source};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signer, SigningKey};
use gazette_sdk::{release, release_commit, store};
use serde_json::value::RawValue;
use std::{collections::BTreeMap, os::unix::fs::PermissionsExt, time::Duration};

fn private_directory() -> TempDir {
    tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .expect("private disposable preparation root")
}

fn read() -> store::ReadOptions {
    store::ReadOptions {
        allow_coordination_files: true,
        ..Default::default()
    }
}

fn initialize(root: &Path) -> store::InitializationReceipt {
    store::initialize(
        root,
        store::InitializeOptions {
            allow_create: true,
            session: store::SessionOptions {
                allow_local_reservation: true,
                read: read(),
            },
            ..Default::default()
        },
    )
    .expect("fresh native SQLite journal")
}

fn options() -> build::PreparationOptions {
    build::PreparationOptions {
        allow_stage: true,
        record: store::PreparationWriteOptions { allow_record: true },
        writer: store::WriterOptions {
            allow_generation_advance: true,
            session: store::SessionOptions {
                allow_local_reservation: true,
                read: read(),
            },
        },
        ..Default::default()
    }
}

fn request<'a>(
    root: &'a Path,
    expected: store::JournalHeader,
    parent: &'a Path,
) -> build::PreparationRequest<'a> {
    build::PreparationRequest {
        journal: root,
        expected,
        parent,
        name: "retained-docs",
        id: source::Sha256Digest::of(b"exact generated documentation preparation"),
    }
}

fn disclosure_policy() -> build::DisclosurePolicy {
    use build::{DisclosureClass as Class, DisclosurePattern as Pattern, DisclosureRule};
    let rules = [
        (Class::LocalPath, "rule:path", "/private/fixture-path"),
        (Class::PrivateEndpoint, "rule:endpoint", "private.invalid"),
        (Class::Secret, "rule:secret", "fixture-secret-not-present"),
        (
            Class::GeneratedArtifact,
            "rule:artifact",
            "private-artifact-not-present",
        ),
        (
            Class::FixtureClaim,
            "rule:claim",
            "fixture-claim-not-present",
        ),
        (
            Class::InternalIdentifier,
            "rule:identifier",
            "private-identifier-not-present",
        ),
    ];
    build::DisclosurePolicy {
        rules: BTreeMap::from_iter(rules.map(|(class, key, value)| {
            (
                key.parse().expect("rule key"),
                DisclosureRule {
                    class,
                    pattern: Pattern::Literal {
                        value: value.into(),
                    },
                },
            )
        })),
    }
}

fn documentation_qualification(
    record: &store::PreparationRecord,
    check: &build::DocumentationPreparationCheck<'_, '_>,
    limits: build::DisclosureProbeLimits,
) -> build::DocumentationPreparationQualification {
    let raw: BTreeMap<String, &RawValue> =
        serde_json::from_str(&record.intent.evidence).expect("exact compact preparation evidence");
    let disclosure = serde_json::to_value(check.disclosure()).expect("typed disclosure evidence");
    build::DocumentationPreparationQualification {
        inputs_sha256: source::Sha256Digest::of(raw["inputs"].get().as_bytes()),
        repository_sha256: source::Sha256Digest::of(raw["repository"].get().as_bytes()),
        artifacts_sha256: source::Sha256Digest::of(raw["artifacts"].get().as_bytes()),
        output: record.intent.output,
        scanner: disclosure["scanner"].as_str().unwrap().into(),
        encoded_profile: disclosure["encoded_profile"].as_str().unwrap().into(),
        filename_profile: disclosure["filename_profile"].as_str().unwrap().into(),
        generic_rules_sha256: serde_json::from_value(disclosure["generic_rules_sha256"].clone())
            .unwrap(),
        resource_profile: build::DisclosureProbeLimits::RESOURCE_PROFILE.into(),
        resource_limits_sha256: limits.resource_limits_sha256(),
        disclosure_policy_sha256: serde_json::from_value(disclosure["policy_sha256"].clone())
            .unwrap(),
    }
}

fn signed_authorization(batch: &release::PreparationBatch) -> (Vec<u8>, release::Binding) {
    let mut body: release::Authorization = serde_json::from_str(include_str!(
        "../../../../crates/gazette-compiler/tests/release_authorization/authorization.json"
    ))
    .unwrap();
    let mut binding: release::Binding = serde_json::from_str(include_str!(
        "../../../../crates/gazette-compiler/tests/release_authorization/binding.json"
    ))
    .unwrap();
    let header = batch.journal();
    body.journal_id = header.journal_id;
    body.journal_epoch = header.journal_epoch;
    body.writer_fence = header.writer_fence;
    body.ordered_targets = batch.targets().to_vec();
    binding.journal = header;
    binding.ordered_targets = batch.targets().to_vec();
    let evidence = batch.evidence();
    body.validation_evidence_digest = evidence.validation_evidence_digest;
    body.disclosure_evidence_digest = evidence.disclosure_evidence_digest;
    body.policy_evidence_digest = evidence.policy_evidence_digest;
    body.scanner_revision = evidence.scanner_revision.clone();
    body.generic_rule_set_digest = evidence.generic_rule_set_digest;
    body.website_policy_projection_digest = evidence.website_policy_projection_digest;
    body.publisher_authorizations.truncate(1);
    body.publisher_authorizations[0].authorized_target_ordinals = vec![0];
    binding.publishers.truncate(1);
    binding.publishers[0].authorized_target_ordinals = vec![0];
    let mut canonical = serde_json::to_value(&body).unwrap();
    canonical
        .as_object_mut()
        .unwrap()
        .remove("canonical_body_sha256");
    for publisher in canonical["publisher_authorizations"]
        .as_array_mut()
        .unwrap()
    {
        publisher
            .as_object_mut()
            .unwrap()
            .remove("signature_envelope");
    }
    let digest = source::Sha256Digest::of(&serde_jcs::to_vec(&canonical).unwrap());
    body.canonical_body_sha256 = digest;
    binding.canonical_body_sha256 = digest;
    let key = SigningKey::from_bytes(&[3; 32]);
    assert_eq!(
        binding.publishers[0].public_key,
        STANDARD.encode(key.verifying_key().to_bytes())
    );
    let mut message = b"WEBSITE_RELEASE_BATCH_AUTHORIZATION_V1\0".to_vec();
    message.extend_from_slice(digest.as_bytes());
    body.publisher_authorizations[0]
        .signature_envelope
        .signature = STANDARD.encode(key.sign(&message).to_bytes());
    (serde_json::to_vec(&body).unwrap(), binding)
}

struct FixtureAuthority {
    binding: release::Binding,
}

impl release::AuthoritySource for FixtureAuthority {
    fn acquire_authority(&mut self) -> Result<release::Binding, release::AuthorityError> {
        Ok(self.binding.clone())
    }
}

#[test]
fn retained_modules_reach_checked_generic_release_selection_without_another_release_system() {
    for custom_limits in [false, true] {
        retained_modules_with_resource_limits(custom_limits);
    }
}

fn retained_modules_with_resource_limits(custom_limits: bool) {
    let bytes = artifact();
    let fixture = Fixture::new(&bytes);
    let remote_root = private_directory();
    let remote = remote_root.path().join("domain-doc.git");
    git2::Repository::init_bare(&remote).expect("disposable local domain_doc destination");
    let configuration = fixture.configuration().replace(
        "https://example.invalid/configured-domain-doc.git",
        remote.to_str().unwrap(),
    );
    let inputs = acquire(&configuration);
    let assembled = build::assemble_documentation(&inputs, Default::default())
        .expect("real TypeDoc conversion and multi-project Mintlify assembly");
    let output = assembled.output().inventory().identity();
    assert_eq!(assembled.artifacts().len(), 3);
    let navigation: Value = serde_json::from_slice(file(assembled.output(), "docs.json"))
        .expect("assembled navigation");
    super::assert_typedoc_navigation(&navigation);

    // Preparation must use retained bytes, not rerun a generator or reopen current source.
    fs::write(
        fixture.root.path().join("generated/library.json"),
        b"changed after capture",
    )
    .expect("advance native source after acquisition");
    let journal_root = private_directory();
    let journal = initialize(journal_root.path());
    let parent = private_directory();
    let coordinator_anchor = private_directory();
    let seal = store::coordinator::enroll(
        store::coordinator::EnrollmentRequest {
            anchor_root: coordinator_anchor.path(),
            journal_root: journal_root.path(),
            coordinator_id: "coordinator:test",
            principal_scope: source::Sha256Digest::of(b"generated documentation fixture"),
            protected_roots: &[
                fixture.root.path().to_path_buf(),
                parent.path().to_path_buf(),
                remote_root.path().to_path_buf(),
            ],
        },
        options().writer.session,
        true,
    )
    .expect("enroll disposable release coordinator before preparation history");
    let coordinator = store::coordinator::LocalCoordinatorSelection {
        anchor_root: coordinator_anchor.path().to_path_buf(),
        binding: seal.binding,
    };
    let receipt = build::prepare_documentation(
        &assembled,
        request(journal_root.path(), journal.header, parent.path()),
        Some(&coordinator),
        true,
        options(),
    )
    .expect("durably stage exact retained provider inputs");
    let record = &receipt.record;
    let stage = record.intent.address.path();
    assert_eq!(record.intent.output, output);
    assert_eq!(
        record.retention.as_ref().expect("retained phase").output,
        output
    );
    assert!(matches!(
        record.intent.destination.as_ref(),
        Some(release::PublicationDestination::Git { remote: configured_remote, branch })
            if configured_remote == remote.to_str().unwrap()
                && branch == "published"
    ));
    assert!(record.check.is_none() && record.worktree.is_none());
    assert!(
        !fixture.intended().exists(),
        "configured destination is not created"
    );
    assert_eq!(
        source::inspect(&stage, None, Default::default())
            .expect("native retained read-back")
            .identity(),
        output
    );
    assert_eq!(
        fs::read(stage.join("artifacts/second-first/library.json")).unwrap(),
        bytes
    );
    let evidence: Value = serde_json::from_str(&record.intent.evidence).expect("typed evidence");
    assert_eq!(
        evidence,
        json!({
            "format": "gazette.documentation-preparation/1",
            "artifact_class": "documentation-build-input",
            "repository_source_provenance": build::repository_source_provenance(),
            "inputs": inputs.inspect(),
            "brand": "brand:one",
            "repository": assembled.repository(),
            "artifacts": assembled.artifacts(),
            "output": output
        })
    );
    assert_eq!(
        record.intent.evidence_sha256,
        source::Sha256Digest::of(record.intent.evidence.as_bytes())
    );
    let reopened = store::inspect_preparation(
        &journal.database,
        record.intent.id,
        Some(&receipt.journal),
        read(),
    )
    .expect("independent SQLite reopen after writer close");
    assert_eq!(reopened.record.as_ref(), Some(record));

    let mut limits = build::DisclosureProbeLimits::default();
    if custom_limits {
        limits.work_bytes += 1;
        limits.html.tokens += 1;
    }
    let mut check =
        build::check_documentation_preparation(&assembled, &disclosure_policy(), limits)
            .expect("required disclosure scan over exact generated repository");
    let qualification = documentation_qualification(record, &check, limits);
    if custom_limits {
        let mut wrong = qualification.clone();
        wrong.resource_limits_sha256 =
            build::DisclosureProbeLimits::default().resource_limits_sha256();
        assert!(check.qualify(&wrong, 1 << 20).is_err());
    }
    check
        .qualify(&qualification, 1 << 20)
        .expect("independently selected generated-documentation profile");
    let checked = build::record_documentation_preparation_check(
        &check,
        build::PreparationCheckRequest {
            journal: journal_root.path(),
            expected: receipt.journal,
            preparation: record,
        },
        Some(&coordinator),
        true,
        build::PreparationCheckOptions {
            record: options().record,
            writer: options().writer,
            ..Default::default()
        },
    )
    .expect("documentation-specific check uses shared retained history");
    assert!(checked.record.check.is_some());

    let signature = release_commit::CommitSignature {
        name: "Local fixture".into(),
        email: "fixture@example.invalid".into(),
        seconds: 100,
        offset_minutes: 330,
    };
    let metadata = release_commit::CommitMetadata {
        parent: None,
        author: signature.clone(),
        committer: signature,
        message: "Exact retained documentation inputs\n".into(),
    };
    let protected = [fixture.root.path().to_path_buf()];
    let completed = release_commit::prepare_coordinated_journal_worktree(
        &coordinator,
        true,
        release_commit::JournalWorktreeRequest {
            journal: journal_root.path(),
            expected: checked.journal,
            preparation: &checked.record,
            metadata: &metadata,
            parent_source: None,
            protected_roots: &protected,
        },
        release_commit::JournalWorktreeOptions {
            record: options().record,
            writer: options().writer,
            worktree: release_commit::WorktreeOptions {
                allow_initialize: true,
                ..Default::default()
            },
        },
    )
    .expect("same generic technical worktree mechanics consume documentation receipt");
    assert!(completed.record.check.is_some());
    let worktree = completed.record.worktree.as_ref().expect("worktree recipe");
    assert_eq!(
        worktree.check_sha256,
        completed
            .record
            .check
            .as_ref()
            .map(|check| check.evidence_sha256)
    );
    let commit = &worktree
        .completion
        .as_ref()
        .expect("durable native completion")
        .report
        .commit;
    let inspected = release_commit::inspect_journal_worktree(
        release_commit::JournalWorktreeInspectionRequest {
            database: &journal.database,
            preparation_id: record.intent.id,
            expected_header: Some(&completed.journal),
            commit,
            protected_roots: &protected,
        },
        release_commit::JournalWorktreeInspectionOptions {
            read: read(),
            ..Default::default()
        },
    )
    .expect("independent native Git and reopened history read-back");
    assert_eq!(inspected.history.record.as_ref(), Some(&completed.record));
    let selections = [release::PreparationSelection {
        preparation_id: record.intent.id,
        site: "site:one".parse().unwrap(),
        domain: "docs.example.invalid".into(),
        commit: commit.clone(),
        expected_remote_oid_or_unborn: release::RemoteState::Unborn,
        output_policy: None,
        qualification: Some(release::PreparationAdmission::GeneratedDocumentation {
            qualification: Box::new(qualification.clone()),
        }),
    }];
    let mut writer = store::coordinator::acquire_writer(
        &coordinator.anchor_root,
        &coordinator.binding,
        completed.journal,
        options().writer,
        true,
    )
    .expect("hold one exact release writer before signing");
    let release_header = writer.inspect().unwrap().header;
    let preparation_request = || release::PreparationBatchRequest {
        database: &journal.database,
        expected: release_header,
        selections: &selections,
        protected_roots: &protected,
    };
    let release_options = release::PreparationBatchOptions {
        inspection: release_commit::JournalWorktreeInspectionOptions {
            read: read(),
            ..Default::default()
        },
        ..Default::default()
    };
    let batch = release::inspect_preparations(preparation_request(), release_options)
        .expect("checked documentation output enters the generic release evidence join");
    assert_eq!(batch.targets().len(), 1);
    assert_eq!(batch.targets()[0].domain, "docs.example.invalid");
    assert_eq!(batch.targets()[0].remote, remote.to_str().unwrap());
    assert_eq!(batch.targets()[0].branch, "published");
    let (authorization, binding) = signed_authorization(&batch);
    let verified = release::verify_prepared_authorization(
        release::PreparedAuthorizationRequest {
            authorization: &authorization,
            binding: &binding,
            preparations: preparation_request(),
        },
        release::PreparedAuthorizationOptions {
            preparations: release::PreparationBatchOptions {
                inspection: release_commit::JournalWorktreeInspectionOptions {
                    read: read(),
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Default::default()
        },
    )
    .expect("signed generated-documentation target reopens through generic release admission");
    assert_eq!(
        verified.verification.authorization_sha256,
        source::Sha256Digest::of(&authorization)
    );
    writer
        .register_release_authorization(
            release::PreparedAuthorizationRequest {
                authorization: &authorization,
                binding: &binding,
                preparations: preparation_request(),
            },
            release::RegisterAuthorizationOptions {
                verification: release::PreparedAuthorizationOptions {
                    preparations: release_options,
                    ..Default::default()
                },
                record: store::ReleaseAuthorizationWriteOptions { allow_record: true },
            },
        )
        .expect("retain exact signed generated-documentation authorization");
    let git_isolation = private_directory();
    let git = release::GitSelection {
        program: crate::native::git().to_owned(),
        isolation: git_isolation.path().to_path_buf(),
        credentials: None,
    };
    let reads = release::RemoteReadOptions {
        allow_read: true,
        allow_network: false,
        timeout: Duration::from_secs(10),
        ..Default::default()
    };
    let mut authority = FixtureAuthority {
        binding: binding.clone(),
    };
    let consumed = writer
        .consume_release_authorization(
            &mut authority,
            &git,
            release::ConsumeAuthorizationRequest {
                authorization: &authorization,
                preparations: preparation_request(),
            },
            release::ConsumeAuthorizationOptions {
                verification: release::PreparedAuthorizationOptions {
                    preparations: release_options,
                    ..Default::default()
                },
                read: reads,
                leases: store::ReleaseLeaseOptions {
                    allow_acquire: true,
                },
                consume: store::ReleaseConsumptionOptions {
                    allow_consume: true,
                },
            },
        )
        .expect("consume one-use local authorization after exact unborn read-back")
        .record;
    let attempt = writer
        .dispatch_release_attempt(
            &mut authority,
            &git,
            release::DispatchAttemptRequest {
                authorization: &authorization,
                preparations: preparation_request(),
                ordinal: 0,
                number: 1,
            },
            release::DispatchAttemptOptions {
                verification: release::PreparedAuthorizationOptions {
                    preparations: release_options,
                    ..Default::default()
                },
                read: reads,
                record: store::ReleaseAttemptOptions { allow_record: true },
                dispatch: release::DispatchOptions {
                    profile: release::DispatchProfile::LocalTransaction,
                    allow_dispatch: true,
                    timeout: Duration::from_secs(10),
                    ..Default::default()
                },
            },
        )
        .expect("generic local Git transaction dispatches exact domain_doc commit");
    assert!(matches!(
        attempt.result.as_ref().map(|result| &result.readback),
        Some(release::TargetReadback::Observed {
            comparison: release::RefComparison::Intended,
            ..
        })
    ));
    let settlement = writer
        .reconcile_release(
            &git,
            release::ReconcileReleaseRequest {
                database: &journal.database,
                expected: release_header,
                authorization_id: &consumed.authorization_id,
                number: 1,
            },
            release::ReconcileReleaseOptions {
                read: reads,
                record: store::ReleaseReconciliationOptions {
                    allow_record: true,
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .expect("generic read-back settles the generated-documentation release");
    assert_eq!(
        settlement.record.settlement.state,
        store::ReleaseSettlementState::Succeeded
    );
    writer
        .close()
        .expect("close release writer after settlement");
    let destination = git2::Repository::open_bare(&remote).unwrap();
    assert_eq!(
        destination
            .refname_to_id("refs/heads/published")
            .unwrap()
            .to_string(),
        commit.as_str()
    );
    assert!(!fixture.intended().exists());
}

#[test]
fn preparation_replay_never_replaces_stage_or_configured_destination() {
    let fixture = Fixture::new(&artifact());
    fs::create_dir(fixture.intended()).expect("existing separately owned destination");
    let sentinel = fixture.intended().join("owner-file");
    fs::write(&sentinel, b"native destination remains owned").unwrap();
    let inputs = acquire(&fixture.configuration());
    let assembled = build::assemble_documentation(&inputs, Default::default()).unwrap();
    let journal_root = private_directory();
    let journal = initialize(journal_root.path());
    let parent = private_directory();
    let receipt = build::prepare_documentation(
        &assembled,
        request(journal_root.path(), journal.header, parent.path()),
        None,
        false,
        options(),
    )
    .expect("independent selected stage");
    let stage = receipt.record.intent.address.path();
    let changed = stage.join("docs.json");
    fs::write(&changed, b"changed retained tree must not be overwritten").unwrap();
    let error = build::prepare_documentation(
        &assembled,
        request(journal_root.path(), receipt.journal, parent.path()),
        None,
        false,
        options(),
    )
    .expect_err("same coordinate does not authorize a replacement stage");
    assert!(matches!(*error.cause, build::PreparationFailure::Replay));
    assert!(
        error.writer_acquired,
        "replay is detected through durable history"
    );
    let replay = error
        .receipt
        .as_ref()
        .expect("exact original history returned");
    assert!(!replay.inserted);
    assert_eq!(replay.record, receipt.record);
    assert_eq!(error.stage.as_ref(), Some(&stage));
    assert_eq!(
        fs::read(&changed).unwrap(),
        b"changed retained tree must not be overwritten"
    );
    assert_eq!(
        fs::read(&sentinel).unwrap(),
        b"native destination remains owned"
    );
    let reopened = store::inspect_preparation(
        &journal.database,
        receipt.record.intent.id,
        error.journal.as_ref(),
        read(),
    )
    .expect("original history survives refused replay");
    assert_eq!(reopened.record.as_ref(), Some(&receipt.record));
}

#[test]
fn preparation_preflight_refuses_permissions_bounds_and_source_overlap_without_effects() {
    let fixture = Fixture::new(&artifact());
    let inputs = acquire(&fixture.configuration());
    let assembled = build::assemble_documentation(&inputs, Default::default()).unwrap();
    let journal_root = private_directory();
    let journal = initialize(journal_root.path());
    let parent = private_directory();
    let initial = fs::read(&journal.database).unwrap();
    for (index, (selected_parent, selected_options)) in [
        (parent.path(), build::PreparationOptions::default()),
        (
            parent.path(),
            build::PreparationOptions {
                evidence_bytes: 1,
                ..options()
            },
        ),
        (fixture.root.path(), options()),
    ]
    .into_iter()
    .enumerate()
    {
        let request = request(journal_root.path(), journal.header, selected_parent);
        let error =
            build::prepare_documentation(&assembled, request, None, false, selected_options)
                .expect_err("preflight must refuse before acquiring writer or creating output");
        assert!(matches!(
            (index, error.cause.as_ref()),
            (0, build::PreparationFailure::Permission)
                | (1, build::PreparationFailure::Evidence(_))
                | (2, build::PreparationFailure::Stage(_))
        ));
        assert!(!error.writer_acquired);
        assert!(error.journal.is_none() && error.receipt.is_none() && error.stage.is_none());
        assert!(!selected_parent.join(request.name).exists());
        assert_eq!(fs::read(&journal.database).unwrap(), initial);
        let reopened = store::inspect_preparation(
            &journal.database,
            request.id,
            Some(&journal.header),
            read(),
        )
        .expect("unchanged independent history");
        assert!(reopened.record.is_none());
    }
    assert_eq!(
        fs::read(fixture.root.path().join("generated/library.json")).unwrap(),
        artifact()
    );
    assert!(!fixture.intended().exists());
}

#[test]
fn generated_destination_requires_deployable_while_local_only_preparation_remains_available() {
    let fixture = Fixture::new(&artifact());
    let configured = fixture.configuration();
    let disabled = configured.replacen(
        "id='site:one'\npublisher='owner:one'\ndeployable=true",
        "id='site:one'\npublisher='owner:one'\ndeployable=false",
        1,
    );
    assert_ne!(
        disabled, configured,
        "fixture must disable the selected site"
    );
    let disabled_inputs = acquire(&disabled);
    let assembled = build::assemble_documentation(&disabled_inputs, Default::default()).unwrap();
    let journal_root = private_directory();
    let journal = initialize(journal_root.path());
    let parent = private_directory();
    let error = build::prepare_documentation(
        &assembled,
        request(journal_root.path(), journal.header, parent.path()),
        None,
        false,
        options(),
    )
    .expect_err("a configured generated-repository destination honors the site fence");
    assert!(matches!(
        error.cause.as_ref(),
        build::PreparationFailure::Deployability(cause)
            if cause.sites() == ["site:one".parse().unwrap()]
    ));
    assert!(!error.writer_acquired);
    assert!(parent.path().read_dir().unwrap().next().is_none());

    let local = disabled.replace(
        "destination={kind='git',remote='https://example.invalid/configured-domain-doc.git',branch='published'}\n",
        "",
    );
    let local_inputs = acquire(&local);
    let assembled = build::assemble_documentation(&local_inputs, Default::default()).unwrap();
    let receipt = build::prepare_documentation(
        &assembled,
        request(journal_root.path(), journal.header, parent.path()),
        None,
        false,
        options(),
    )
    .expect("disabled site may still retain a local-only generated repository");
    assert!(receipt.record.intent.destination.is_none());
}
