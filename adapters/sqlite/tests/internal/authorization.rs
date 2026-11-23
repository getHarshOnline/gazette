//! Native custody tests. Supplied authorization bodies are representation-only,
//! deliberately unsigned; application tests own cryptographic/native publication proof.

use super::*;
use gazette_core::{
    release::{
        Authorization, AuthorizationSchema, Binding, CommitMetadata, CommitReport, CommitSignature,
        RemoteState, Target,
    },
    source::TreeIdentity,
    stage::Address,
    store::{
        InitializeOptions, PreparationCommitInput, PreparationInput, PreparationRetentionInput,
        PreparationWorktreeInput, PreparationWriteOptions, ReleaseAuthorizationEffect,
        ReleaseAuthorizationWriteOptions, SessionOptions, WriterOptions,
    },
};
use gazette_ports::store::{
    JournalWriter, PreparationWriter, ReleaseAuthorizationWriter, StoreInitializer, WriterConnector,
};
use std::{fs, os::unix::fs::PermissionsExt};

fn options() -> ReadOptions {
    ReadOptions {
        allow_coordination_files: true,
        ..Default::default()
    }
}
fn writer_options(read: ReadOptions) -> WriterOptions {
    WriterOptions {
        allow_generation_advance: true,
        session: SessionOptions {
            allow_local_reservation: true,
            read,
        },
    }
}
fn permission() -> ReleaseAuthorizationWriteOptions {
    ReleaseAuthorizationWriteOptions { allow_record: true }
}
fn fixture() -> (tempfile::TempDir, crate::Writer) {
    let root = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .expect("private journal");
    let receipt = Sqlite
        .initialize(
            root.path(),
            InitializeOptions {
                allow_create: true,
                session: writer_options(options()).session,
                ..Default::default()
            },
        )
        .expect("fresh schema");
    let writer = Sqlite
        .acquire_writer(root.path(), receipt.header, writer_options(options()))
        .expect("held writer");
    (root, writer)
}

struct Data {
    raw: String,
    binding: gazette_core::release::ReleaseBinding,
    ids: Vec<Sha256Digest>,
}
impl Data {
    fn input(&self) -> ReleaseAuthorizationInput<'_> {
        ReleaseAuthorizationInput {
            authorization: &self.raw,
            binding: &self.binding,
            preparations_sha256: Sha256Digest::of(b"application-owned join"),
            preparation_ids: &self.ids,
        }
    }
    fn body(&self) -> Authorization {
        self.input()
            .validate()
            .expect("representation only")
            .as_signed()
            .unwrap()
            .clone()
    }
    fn change(&mut self, change: impl FnOnce(&mut Authorization)) {
        let mut body = self.body();
        change(&mut body);
        self.binding.as_signed_mut().unwrap().ordered_targets = body.ordered_targets.clone();
        self.raw = serde_json::to_string(&body).expect("test body");
    }
}

fn prepared(writer: &mut crate::Writer) -> Data {
    let write = PreparationWriteOptions { allow_record: true };
    let mut ids = Vec::new();
    let mut targets = Vec::new();
    for ordinal in 0..2_u32 {
        let id = Sha256Digest::of(&ordinal.to_be_bytes());
        let output: TreeIdentity = serde_json::from_value(serde_json::json!({"format": TreeIdentity::FORMAT, "sha256": Sha256Digest::of(&[ordinal as u8])})).expect("typed claimed tree");
        let address = Address {
            parent: "/private/fixture-stages".into(),
            name: format!("site-{ordinal}"),
        };
        let intent = writer
            .record_preparation_intent(
                PreparationInput {
                    id,
                    address: &address,
                    destination: None,
                    output,
                    entries: 1,
                    bytes: 7,
                    evidence: "{}",
                    evidence_sha256: Sha256Digest::of(b"{}"),
                },
                write,
            )
            .expect("native intent");
        let retained = writer
            .record_preparation_retention(
                PreparationRetentionInput {
                    intent: &intent.record.intent,
                    output,
                    entries: 1,
                    bytes: 7,
                },
                write,
            )
            .expect("native retention");
        let signature = CommitSignature {
            name: "Fixture".into(),
            email: "fixture@example.invalid".into(),
            seconds: 1,
            offset_minutes: 0,
        };
        let metadata = CommitMetadata {
            parent: None,
            author: signature.clone(),
            committer: signature,
            message: "Exact recipe\n".into(),
        };
        let worktree = writer
            .record_preparation_worktree(
                PreparationWorktreeInput {
                    intent: &retained.record.intent,
                    retention: retained.record.retention.as_ref().expect("retained"),
                    check: None,
                    metadata: &metadata,
                    parent_source: None,
                },
                write,
            )
            .expect("native recipe");
        let report = CommitReport {
            commit: format!("{:040x}", ordinal + 1)
                .parse()
                .expect("full commit"),
            output,
            parent: None,
            entries: 1,
            bytes: 7,
        };
        writer
            .record_preparation_commit(
                PreparationCommitInput {
                    intent: &worktree.record.intent,
                    retention: worktree.record.retention.as_ref().expect("retained"),
                    check: None,
                    worktree: worktree.record.worktree.as_ref().expect("recipe"),
                    report: &report,
                },
                write,
            )
            .expect("native completion record");
        targets.push(Target {
            ordinal,
            domain: format!("site{ordinal}.example.test"),
            remote: format!("/private/site{ordinal}.git"),
            branch: "main".into(),
            refspec: format!("{}:refs/heads/main", report.commit),
            expected_remote_oid_or_unborn: RemoteState::Unborn,
            prepared_release_receipt_ref: Sha256Digest::of(b"application-owned receipt"),
            local_commit_oid: report.commit,
            output_digest: output.sha256(),
        });
        ids.push(id);
    }
    let header = writer.inspect().expect("held coordinates").header;
    let digest = Sha256Digest::of(b"representation only, not signed");
    let body = Authorization {
        schema_id: AuthorizationSchema::WebsiteReleaseBatchAuthorizationV1,
        record_id: "record-one".into(),
        nonce: "nonce-one".into(),
        coordinator_id: "fixture-coordinator".into(),
        journal_id: header.journal_id,
        journal_epoch: header.journal_epoch,
        writer_fence: header.writer_fence,
        ordered_targets: targets.clone(),
        validation_evidence_digest: digest,
        disclosure_evidence_digest: digest,
        policy_evidence_digest: digest,
        scanner_revision: "fixture".into(),
        generic_rule_set_digest: digest,
        website_policy_projection_digest: digest,
        approved_at: 1,
        not_before: 1,
        expires_at: 2,
        canonical_body_sha256: digest,
        publisher_authorizations: vec![],
    };
    let binding = Binding {
        canonical_body_sha256: digest,
        coordinator_id: body.coordinator_id.clone(),
        journal: header,
        ordered_targets: targets,
        publishers: vec![],
        authority_frontier_sha256: digest,
        revocation_frontier_sha256: digest,
        supersession_frontier_sha256: digest,
        observed_at: 1,
        emergency_stop: false,
    };
    Data {
        raw: serde_json::to_string_pretty(&body).expect("exact transport whitespace"),
        binding: binding.into(),
        ids,
    }
}

#[test]
fn fresh_registration_reopens_and_exact_later_replay_preserves_original_binding() {
    let (root, mut writer) = fixture();
    let data = prepared(&mut writer);
    let path = Sqlite::journal_database(root.path());
    let receipt = writer
        .record_release_authorization(&path, data.input(), permission())
        .expect("atomic pending registration");
    assert!(receipt.inserted);
    assert_eq!(receipt.record.as_input(), data.input());
    assert_eq!(receipt.record.created, *data.binding.journal());
    writer.close().expect("closed lifetime");
    let before = fs::read(&path).expect("history bytes");
    assert_eq!(
        Sqlite
            .inspect_release_authorization(&path, "record-one", options())
            .expect("read historical pending")
            .record,
        Some(receipt.record.clone())
    );
    assert!(
        Sqlite
            .inspect_release_authorization(&path, "absent", options())
            .expect("explicit absence")
            .record
            .is_none()
    );
    assert_eq!(fs::read(&path).expect("read did not mutate"), before);
    let mut writer = Sqlite
        .acquire_writer(root.path(), receipt.journal, writer_options(options()))
        .expect("later writer");
    let before = fs::read(&path).expect("advanced generation");
    let replay = writer
        .record_release_authorization(&path, data.input(), permission())
        .expect("storage replay only");
    assert!(!replay.inserted);
    assert_eq!(replay.record, receipt.record);
    assert_eq!(
        replay.journal.writer_fence,
        receipt.journal.writer_fence + 1
    );
    assert_eq!(fs::read(&path).expect("replay did not rewrite"), before);
    writer.close().expect("close replay lifetime");
}

#[test]
fn record_or_scoped_nonce_conflict_preserves_original_and_poisoned_lifetime() {
    for same_record in [true, false] {
        let (root, mut writer) = fixture();
        let mut data = prepared(&mut writer);
        let path = Sqlite::journal_database(root.path());
        let original = writer
            .record_release_authorization(&path, data.input(), permission())
            .expect("original pending");
        if same_record {
            data.raw.push(' ');
        } else {
            data.change(|body| body.record_id = "another-record".into());
        }
        let before = fs::read(&path).expect("before conflict");
        let error = writer
            .record_release_authorization(&path, data.input(), permission())
            .expect_err("immutable conflict");
        assert!(matches!(error.cause, Error::ReleaseAuthorizationConflict));
        assert_eq!(error.effect, ReleaseAuthorizationEffect::NotAttempted);
        assert!(writer.inspect().is_err());
        drop(writer);
        assert_eq!(fs::read(&path).expect("no conflicting insert"), before);
        assert_eq!(
            Sqlite
                .inspect_release_authorization(&path, "record-one", options())
                .expect("original survives")
                .record,
            Some(original.record)
        );
    }
}

#[test]
fn missing_duplicate_reordered_and_wrong_completed_preparations_refuse() {
    for change in ["missing", "duplicate", "reordered", "commit", "output"] {
        let (root, mut writer) = fixture();
        let mut data = prepared(&mut writer);
        let path = Sqlite::journal_database(root.path());
        match change {
            "missing" => data.ids[1] = Sha256Digest::of(b"not prepared"),
            "duplicate" => data.ids[1] = data.ids[0],
            "reordered" => data.ids.swap(0, 1),
            "commit" => data.change(|body| {
                body.ordered_targets[0].local_commit_oid =
                    "9999999999999999999999999999999999999999"
                        .parse()
                        .expect("different commit")
            }),
            "output" => data.change(|body| {
                body.ordered_targets[0].output_digest = Sha256Digest::of(b"changed tree")
            }),
            _ => unreachable!(),
        }
        let before = fs::read(&path).expect("native preimage");
        let error = writer
            .record_release_authorization(&path, data.input(), permission())
            .expect_err(change);
        assert_eq!(
            error.effect,
            ReleaseAuthorizationEffect::NotAttempted,
            "{change}"
        );
        drop(writer);
        assert_eq!(
            fs::read(&path).expect("no partial registration"),
            before,
            "{change}"
        );
        assert!(
            Sqlite
                .inspect_release_authorization(&path, "record-one", options())
                .expect("no row")
                .record
                .is_none()
        );
    }
}

#[test]
fn permission_database_binding_and_stale_original_generation_refuse_without_insert() {
    let (root, mut writer) = fixture();
    let data = prepared(&mut writer);
    let path = Sqlite::journal_database(root.path());
    let before = fs::read(&path).expect("preimage");
    assert!(matches!(
        writer
            .record_release_authorization(&path, data.input(), Default::default())
            .expect_err("permission")
            .cause,
        Error::ReleaseAuthorizationPermission
    ));
    let copy = tempfile::tempdir().expect("unselected copy directory");
    let copy_path = Sqlite::journal_database(copy.path());
    fs::copy(&path, &copy_path).expect("identical header and bytes in another namespace");
    assert!(matches!(
        writer
            .record_release_authorization(&copy_path, data.input(), permission())
            .expect_err("copy cannot supply preparation verification")
            .cause,
        Error::JournalMismatch
    ));
    assert_eq!(fs::read(&path).expect("pure refusal"), before);
    let header = writer.inspect().expect("usable lifetime").header;
    writer.close().expect("close before later fence");
    let mut writer = Sqlite
        .acquire_writer(root.path(), header, writer_options(options()))
        .expect("later fence");
    let before = fs::read(&path).expect("new fence");
    let error = writer
        .record_release_authorization(&path, data.input(), permission())
        .expect_err("new insert cannot use old binding");
    assert!(matches!(error.cause, Error::JournalMismatch));
    assert_eq!(error.effect, ReleaseAuthorizationEffect::NotAttempted);
    drop(writer);
    assert_eq!(fs::read(&path).expect("no old-fence registration"), before);
}

#[test]
fn partial_link_insert_is_rolled_back_with_its_pending_record() {
    let (root, mut writer) = fixture();
    let data = prepared(&mut writer);
    let header = writer.inspect().expect("header").header;
    writer.close().expect("close before SQL fault test");
    let path = Sqlite::journal_database(root.path());
    let mut connection = Connection::open(&path).expect("controlled native fault");
    connection
        .execute_batch("PRAGMA foreign_keys=ON")
        .expect("enforce FKs");
    {
        let transaction = connection.transaction().expect("same atomic owner");
        let mut ids = data.ids.clone();
        ids[1] = Sha256Digest::of(b"missing second link");
        let bytes = payload(data.input(), options()).expect("bounded payload");
        assert!(
            insert(
                &transaction,
                &gazette_core::release::ReleaseDocument::Signed(Box::new(data.body())),
                &ids,
                &bytes
            )
            .is_err()
        );
        // Registration and first link were executed before the genuine FK failure.
        let rows: i64 = transaction
            .query_row(
                "SELECT count(*) FROM gazette_release_authorization_preparations",
                [],
                |row| row.get(0),
            )
            .expect("partial transaction");
        assert_eq!(rows, 1);
    }
    assert!(
        read(&connection, "record-one", header, options())
            .expect("rolled back row")
            .is_none()
    );
    let rows: i64 = connection
        .query_row(
            "SELECT count(*) FROM gazette_release_authorization_preparations",
            [],
            |row| row.get(0),
        )
        .expect("rollback links");
    assert_eq!(rows, 0);
}

#[test]
fn exact_encoded_budget_and_raw_oversize_refuse_before_parsing_or_insert() {
    let (root, mut writer) = fixture();
    let mut data = prepared(&mut writer);
    let bytes = payload(data.input(), options()).expect("bounded projection");
    let bound = i32::try_from(bytes.len() + ROW_BYTES).expect("test bound");
    assert!(
        payload(
            data.input(),
            ReadOptions {
                value_bytes: bound,
                ..options()
            }
        )
        .is_ok()
    );
    assert!(matches!(
        payload(
            data.input(),
            ReadOptions {
                value_bytes: bound - 1,
                ..options()
            }
        ),
        Err(Error::StorageLimit)
    ));
    data.raw = "not-json".repeat(bound as usize);
    assert!(
        matches!(
            payload(
                data.input(),
                ReadOptions {
                    value_bytes: bound,
                    ..options()
                }
            ),
            Err(Error::StorageLimit)
        ),
        "size refusal precedes malformed JSON parsing"
    );
    let path = Sqlite::journal_database(root.path());
    assert!(
        Sqlite
            .inspect_release_authorization(&path, "record-one", options())
            .expect("no insertion")
            .record
            .is_none()
    );
    writer.close().expect("pure budget check");
}

#[test]
fn changed_payload_keys_coordinates_and_links_fail_historical_inspection_without_repair() {
    for change in [
        "digest",
        "unknown",
        "array",
        "whitespace",
        "created",
        "nonce",
        "link",
        "retroactive",
    ] {
        let (root, mut writer) = fixture();
        let data = prepared(&mut writer);
        let path = Sqlite::journal_database(root.path());
        let receipt = writer
            .record_release_authorization(&path, data.input(), permission())
            .expect("pending");
        writer.close().expect("close before corruption");
        let mut value = serde_json::to_value(&receipt.record).expect("record projection");
        match change {
            "unknown" => {
                value["extra"] = true.into();
            }
            "array" => {
                value["created"] = serde_json::json!([1, receipt.journal.journal_id, 1, 1]);
            }
            "created" => {
                value["created"]["writer_fence"] = (receipt.journal.writer_fence + 1).into();
            }
            _ => {}
        }
        let mut bytes = if matches!(change, "unknown" | "array" | "created") {
            serde_json::to_vec(&value).expect("changed projection")
        } else {
            serde_json::to_vec(&receipt.record).expect("original canonical projection")
        };
        if change == "whitespace" {
            bytes.push(b' ');
        }
        let digest = if change == "digest" {
            Sha256Digest::of(b"wrong")
        } else {
            Sha256Digest::of(&bytes)
        };
        let connection = Connection::open(&path).expect("controlled corruption");
        connection
            .execute(
                "UPDATE gazette_release_authorizations SET payload=?1,payload_sha256=?2",
                params![bytes, digest.as_bytes()],
            )
            .expect("change test row");
        if change == "nonce" {
            connection
                .execute(
                    "UPDATE gazette_release_authorizations SET nonce='different'",
                    [],
                )
                .expect("change key");
        }
        if change == "link" {
            connection
                .execute(
                    "DELETE FROM gazette_release_authorization_preparations WHERE ordinal=1",
                    [],
                )
                .expect("remove required link");
        }
        if change == "retroactive" {
            connection.execute_batch("UPDATE gazette_journal SET writer_fence=writer_fence+1; UPDATE gazette_preparation_commits SET recorded_fence=recorded_fence+1;").expect("completion falsely moved after original registration");
        }
        connection.close().expect("close corruptor");
        let before = fs::read(&path).expect("corrupted preimage");
        assert!(
            Sqlite
                .inspect_release_authorization(&path, "record-one", options())
                .is_err(),
            "{change}"
        );
        assert_eq!(fs::read(&path).expect("no repair"), before, "{change}");
    }
}

#[test]
fn replacing_derived_registry_preserves_pending_history() {
    use gazette_core::{
        publication::{
            Key, Principal, PrincipalKind,
            content::{Declaration, Graph},
            registry::{Projection, Site},
        },
        store::{RegistryInput, RegistryWriteOptions},
    };
    use gazette_ports::store::RegistryWriter;
    use std::collections::BTreeMap;
    let (root, mut writer) = fixture();
    let data = prepared(&mut writer);
    let path = Sqlite::journal_database(root.path());
    let pending = writer
        .record_release_authorization(&path, data.input(), permission())
        .expect("pending custody");
    let graph = Graph::resolve(
        Declaration {
            nodes: vec![],
            memberships: vec![],
            related: vec![],
            browsing: vec![],
            projects: vec![],
        },
        Default::default(),
    )
    .expect("empty graph");
    let principal = Principal {
        id: "person:one".parse().expect("principal key"),
        kind: PrincipalKind::Person,
        name: None,
        website: None,
        same_as: Vec::new(),
    };
    let site: Key = "site:one".parse().expect("site key");
    let model = Projection {
        principals: BTreeMap::from([(&principal.id, &principal)]),
        projects: BTreeMap::new(),
        taxonomies: BTreeMap::new(),
        extensions: BTreeMap::new(),
        brands: BTreeMap::new(),
        sites: BTreeMap::from([(
            &site,
            Site {
                publisher: &principal.id,
                rights_holder: None,
                default_author: None,
                roles: &[],
                deployable: false,
                brand: None,
                public_base_url: None,
                repository: None,
                discussion: None,
            },
        )]),
        placements: vec![],
        content: &graph,
        routes: BTreeMap::new(),
        provenance: BTreeMap::new(),
    };
    for revision in [b"first", b"other"] {
        writer
            .replace_registry(
                RegistryInput {
                    model: &model,
                    documents: &BTreeMap::new(),
                    semantic_sha256: model
                        .semantic_sha256(1 << 20)
                        .expect("native model identity"),
                    configuration_sha256: Sha256Digest::of(revision),
                },
                RegistryWriteOptions {
                    allow_replace: true,
                },
            )
            .expect("real registry replacement");
        assert_eq!(
            writer
                .inspect_release_authorization("record-one")
                .expect("history survives")
                .record,
            Some(pending.record.clone())
        );
    }
    writer.close().expect("registry and history lifetime close");
}

#[test]
fn committed_growth_retains_uncertain_effect_and_independent_pending_readback() {
    let (root, mut writer) = fixture();
    let mut data = prepared(&mut writer);
    let mut body = data.body();
    let header = writer.inspect().expect("original header").header;
    writer.close().expect("close for measurement");
    let path = Sqlite::journal_database(root.path());
    let size = fs::metadata(&path).expect("baseline allocation").len();
    let bound = ReadOptions {
        storage_bytes: size,
        ..options()
    };
    let mut writer = Sqlite
        .acquire_writer(root.path(), header, writer_options(bound))
        .expect("bounded writer");
    let current = writer.inspect().expect("actual held generation").header;
    data.binding.as_signed_mut().unwrap().journal = current;
    body.writer_fence = current.writer_fence;
    data.raw = serde_json::to_string(&body).expect("new generation body");
    data.raw
        .push_str(&" ".repeat(usize::try_from(size / 2).expect("bounded growth")));
    payload(data.input(), bound).expect("encoded payload fits input budget");
    let error = writer
        .record_release_authorization(&path, data.input(), permission())
        .expect_err("postcommit growth readback exceeds original storage limit");
    assert!(matches!(error.cause, Error::StorageLimit));
    assert_eq!(error.effect, ReleaseAuthorizationEffect::Unverified);
    assert!(writer.close().is_err());
    assert!(fs::metadata(&path).expect("committed size").len() > size);
    let inspection = Sqlite
        .inspect_release_authorization(&path, "record-one", options())
        .expect("independent wider-bound inspection");
    assert_eq!(inspection.journal.header, current);
    let record = inspection
        .record
        .expect("receipt loss does not erase pending custody");
    assert_eq!(record.as_input(), data.input());
    assert_eq!(record.created, current);
}
