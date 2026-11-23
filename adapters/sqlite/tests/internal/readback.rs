//! Real `SQLite` history tests; supplied observations do not claim native Git truth.

use super::*;
use gazette_core::{
    release::{
        RefAdvertisement, RefComparison, RemoteReadError, RemoteReadFailure, RemoteState, Target,
        TargetReadback, TargetReadbackRow,
    },
    store::{
        InitializeOptions, RefObservationEffect, RefObservationWriteOptions, RegistryInput,
        RegistryWriteOptions, SessionOptions, WriterOptions,
    },
};
use gazette_ports::store::{
    JournalWriter, RefObservationWriter, RegistryWriter, StoreInitializer, WriterConnector,
};
use std::{collections::BTreeMap, fs, os::unix::fs::PermissionsExt};

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

fn fixture() -> (tempfile::TempDir, crate::Writer) {
    let root = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .expect("private SQLite fixture");
    let initial = Sqlite
        .initialize(
            root.path(),
            InitializeOptions {
                allow_create: true,
                session: writer_options(options()).session,
                ..Default::default()
            },
        )
        .expect("fresh native journal");
    let writer = Sqlite
        .acquire_writer(root.path(), initial.header, writer_options(options()))
        .expect("held native writer");
    (root, writer)
}

fn data() -> (Vec<Target>, Vec<TargetReadbackRow>) {
    let oid: gazette_core::source::CommitId = "1111111111111111111111111111111111111111"
        .parse()
        .expect("fixed full OID");
    let targets = (0..3)
        .map(|ordinal| Target {
            ordinal,
            domain: format!("site{ordinal}.example.test"),
            remote: format!("/private/fixture-{ordinal}.git"),
            branch: "main".into(),
            refspec: format!("{oid}:refs/heads/main"),
            expected_remote_oid_or_unborn: RemoteState::Unborn,
            prepared_release_receipt_ref: Sha256Digest::of(&[ordinal as u8]),
            local_commit_oid: oid.clone(),
            output_digest: Sha256Digest::of(b"output"),
        })
        .collect();
    let rows = vec![
        TargetReadbackRow {
            ordinal: 0,
            readback: TargetReadback::Observed {
                advertisement: RefAdvertisement::Commit { oid },
                comparison: RefComparison::Intended,
            },
        },
        TargetReadbackRow {
            ordinal: 1,
            readback: TargetReadback::Observed {
                advertisement: RefAdvertisement::NotAdvertised,
                comparison: RefComparison::Indeterminate,
            },
        },
        TargetReadbackRow {
            ordinal: 2,
            readback: TargetReadback::Unavailable {
                error: RemoteReadError::new(RemoteReadFailure::Transport),
            },
        },
    ];
    (targets, rows)
}

fn input<'a>(targets: &'a [Target], readbacks: &'a [TargetReadbackRow]) -> RefObservationInput<'a> {
    RefObservationInput {
        id: Sha256Digest::of(b"exact observation"),
        targets,
        readbacks,
    }
}

fn permission() -> RefObservationWriteOptions {
    RefObservationWriteOptions { allow_record: true }
}

#[test]
fn complete_history_reopens_and_replays_without_rewriting_original_creation() {
    let (root, mut writer) = fixture();
    let (targets, rows) = data();
    let input = input(&targets, &rows);
    let receipt = writer
        .record_ref_observation(input, permission())
        .expect("append complete native row");
    assert!(receipt.inserted);
    assert_eq!(receipt.record.as_input(), input);
    assert_eq!(receipt.record.created, receipt.journal);
    assert_eq!(
        writer
            .inspect_ref_observation(input.id)
            .expect("held read")
            .record,
        Some(receipt.record.clone())
    );
    writer.close().expect("close first lifetime");
    let path = root.path().join(crate::journal::DATABASE);
    let bytes = fs::read(&path).expect("before read-only inspection");
    assert_eq!(
        Sqlite
            .inspect_ref_observation(&path, input.id, options())
            .expect("independent history")
            .record,
        Some(receipt.record.clone())
    );
    assert!(
        Sqlite
            .inspect_ref_observation(&path, Sha256Digest::of(b"absent"), options())
            .expect("explicit absence")
            .record
            .is_none()
    );
    assert_eq!(fs::read(&path).expect("unmodified history"), bytes);
    let mut writer = Sqlite
        .acquire_writer(root.path(), receipt.journal, writer_options(options()))
        .expect("next generation");
    let bytes = fs::read(&path).expect("new generation before replay");
    let replay = writer
        .record_ref_observation(input, permission())
        .expect("exact replay");
    assert!(!replay.inserted);
    assert_eq!(replay.record, receipt.record);
    assert_eq!(
        replay.journal.writer_fence,
        receipt.journal.writer_fence + 1
    );
    assert_eq!(fs::read(&path).expect("no replay insert"), bytes);
    writer.close().expect("verified replay lifetime");
}

#[test]
fn conflict_preserves_original_and_invalidates_writer() {
    let (root, mut writer) = fixture();
    let (mut targets, rows) = data();
    let original = writer
        .record_ref_observation(input(&targets, &rows), permission())
        .expect("original record");
    targets[0].domain = "changed.example.test".into();
    let error = writer
        .record_ref_observation(input(&targets, &rows), permission())
        .expect_err("immutable coordinate conflict");
    assert!(matches!(error.cause, Error::ReadbackConflict));
    assert_eq!(error.effect, RefObservationEffect::NotAttempted);
    assert!(matches!(writer.inspect(), Err(Error::Changed)));
    drop(writer);
    assert_eq!(
        Sqlite
            .inspect_ref_observation(
                &root.path().join(crate::journal::DATABASE),
                original.record.id,
                options()
            )
            .expect("original preserved")
            .record,
        Some(original.record)
    );
}

#[test]
fn exact_bound_replay_uses_original_fence_across_decimal_width_change() {
    let (root, mut writer) = fixture();
    // Reach the boundary through real native acquisition, not a synthetic receipt.
    while writer
        .inspect()
        .expect("current native fence")
        .header
        .writer_fence
        < 9
    {
        let header = writer
            .inspect()
            .expect("retained acquisition expectation")
            .header;
        writer.close().expect("release current lifetime");
        writer = Sqlite
            .acquire_writer(root.path(), header, writer_options(options()))
            .expect("next exact generation");
    }
    let (mut targets, rows) = data();
    // Keep the exact row ceiling above the unrelated native schema-record sizes.
    targets[0].domain = "x".repeat(8192);
    let input = input(&targets, &rows);
    let original = writer
        .record_ref_observation(input, permission())
        .expect("record at fence nine");
    assert_eq!(original.record.created.writer_fence, 9);
    let encoded = payload(input, original.record.created, options()).expect("original payload");
    writer.close().expect("close fence nine");
    let bounded = ReadOptions {
        value_bytes: i32::try_from(encoded.len() + ROW_BYTES).expect("exact native row bound"),
        ..options()
    };
    let mut writer = Sqlite
        .acquire_writer(root.path(), original.journal, writer_options(bounded))
        .expect("acquire fence ten with exact old-record bound");
    let current = writer.inspect().expect("fence ten").header;
    assert_eq!(current.writer_fence, 10);
    assert!(
        matches!(payload(input, current, bounded), Err(Error::StorageLimit)),
        "a hypothetical new record has one extra digit"
    );
    let path = root.path().join(crate::journal::DATABASE);
    let before = fs::read(&path).expect("before exact replay");
    let replay = writer
        .record_ref_observation(input, permission())
        .expect("original exact-bound evidence remains replayable");
    assert!(!replay.inserted);
    assert_eq!(replay.record, original.record);
    assert_eq!(replay.journal, current);
    assert_eq!(fs::read(&path).expect("no replay rewrite"), before);
    writer.close().expect("valid exact replay lifetime");
}

#[test]
fn permission_incomplete_and_inconsistent_inputs_have_no_record_effects() {
    let (root, mut writer) = fixture();
    let (targets, mut rows) = data();
    let path = root.path().join(crate::journal::DATABASE);
    let bytes = fs::read(&path).expect("before refusals");
    let error = writer
        .record_ref_observation(input(&targets, &rows), Default::default())
        .expect_err("explicit history permission");
    assert!(matches!(error.cause, Error::ReadbackPermission));
    let error = writer
        .record_ref_observation(input(&targets, &rows[..2]), permission())
        .expect_err("complete rows required");
    assert!(matches!(error.cause, Error::ReadbackVerification));
    rows.swap(0, 1);
    assert!(
        writer
            .record_ref_observation(input(&targets, &rows), permission())
            .is_err()
    );
    rows.swap(0, 1);
    rows[0].readback = TargetReadback::Observed {
        advertisement: RefAdvertisement::NotAdvertised,
        comparison: RefComparison::Intended,
    };
    assert!(
        writer
            .record_ref_observation(input(&targets, &rows), permission())
            .is_err()
    );
    assert_eq!(fs::read(&path).expect("refusals do not mutate"), bytes);
    writer.close().expect("pure refusals preserve lifetime");
}

#[test]
fn payload_budgets_apply_to_encoded_bytes_and_reads_before_decoding() {
    let (root, mut writer) = fixture();
    let (mut targets, rows) = data();
    targets[0].domain = "\\\"".repeat(200);
    let header = writer.inspect().expect("held header").header;
    let bytes = payload(input(&targets, &rows), header, options()).expect("bounded encoded input");
    let exact = i32::try_from(bytes.len() + ROW_BYTES).expect("small budget");
    assert!(
        payload(
            input(&targets, &rows),
            header,
            ReadOptions {
                value_bytes: exact,
                ..options()
            }
        )
        .is_ok()
    );
    assert!(matches!(
        payload(
            input(&targets, &rows),
            header,
            ReadOptions {
                value_bytes: exact - 1,
                ..options()
            }
        ),
        Err(Error::StorageLimit)
    ));
    let receipt = writer
        .record_ref_observation(input(&targets, &rows), permission())
        .expect("stored escaped value");
    writer.close().expect("close writer");
    let connection = Connection::open(root.path().join(crate::journal::DATABASE))
        .expect("test borrowed native row");
    assert!(matches!(
        read(
            &connection,
            receipt.record.id,
            header,
            ReadOptions {
                value_bytes: exact - 1,
                ..options()
            }
        ),
        Err(Error::StorageLimit)
    ));
    connection.close().expect("close native read fixture");
}

#[test]
fn corruption_and_noncanonical_shapes_are_rejected_without_repair() {
    for change in [
        "hash",
        "duplicate",
        "unknown",
        "array",
        "coordinate",
        "whitespace",
        "incomplete",
    ] {
        let (root, mut writer) = fixture();
        let (targets, rows) = data();
        let receipt = writer
            .record_ref_observation(input(&targets, &rows), permission())
            .expect("valid history");
        writer.close().expect("release before corruption");
        let mut value = serde_json::to_value(&receipt.record).expect("native record value");
        match change {
            "unknown" => {
                value["unexpected"] = true.into();
            }
            "array" => {
                value["created"] = serde_json::json!([1, receipt.journal.journal_id, 1, 1]);
            }
            "coordinate" => {
                value["created"]["writer_fence"] = (receipt.journal.writer_fence + 1).into();
            }
            "incomplete" => {
                value["readbacks"].as_array_mut().expect("rows").pop();
            }
            _ => {}
        }
        let mut bytes = if matches!(change, "unknown" | "array" | "coordinate" | "incomplete") {
            serde_json::to_vec(&value).expect("malformed test projection")
        } else {
            serde_json::to_vec(&receipt.record).expect("canonical original")
        };
        if change == "duplicate" {
            bytes.splice(1..1, b"\"id\":\"duplicate\",".iter().copied());
        }
        if change == "whitespace" {
            bytes.push(b' ');
        }
        let digest = if change == "hash" {
            Sha256Digest::of(b"wrong")
        } else {
            Sha256Digest::of(&bytes)
        };
        let path = root.path().join(crate::journal::DATABASE);
        let connection = Connection::open(&path).expect("controlled corruption");
        connection
            .execute(
                "UPDATE gazette_ref_observations SET payload=?1,payload_sha256=?2",
                params![bytes, digest.as_bytes()],
            )
            .expect("replace test evidence");
        connection.close().expect("close corruption fixture");
        let before = fs::read(&path).expect("corrupted bytes");
        assert!(
            Sqlite
                .inspect_ref_observation(&path, receipt.record.id, options())
                .is_err(),
            "{change}"
        );
        assert_eq!(fs::read(&path).expect("no repair"), before, "{change}");
    }
}

#[test]
fn stale_generation_refuses_append_and_registry_replacement_preserves_history() {
    use gazette_core::publication::{
        Key, Principal, PrincipalKind,
        content::{Declaration, Graph},
        registry::{Projection, Site},
    };
    let (root, mut writer) = fixture();
    let (targets, rows) = data();
    let receipt = writer
        .record_ref_observation(input(&targets, &rows), permission())
        .expect("original history");
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
    .expect("real empty graph");
    let principal = Principal {
        id: "person:one".parse().expect("registry principal key"),
        kind: PrincipalKind::Person,
        name: Some("Exact principal".into()),
        website: None,
        same_as: Vec::new(),
    };
    let site: Key = "site:one".parse().expect("registry site key");
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
    for _ in 0..2 {
        writer
            .replace_registry(
                RegistryInput {
                    model: &model,
                    documents: &BTreeMap::new(),
                    semantic_sha256: model
                        .semantic_sha256(1 << 20)
                        .expect("native registry identity"),
                    configuration_sha256: Sha256Digest::of(b"configuration"),
                },
                RegistryWriteOptions {
                    allow_replace: true,
                },
            )
            .expect("actual complete registry replacement");
    }
    assert_eq!(
        writer
            .inspect_ref_observation(receipt.record.id)
            .expect("history after replacement")
            .record,
        Some(receipt.record.clone())
    );
    let path = root.path().join(crate::journal::DATABASE);
    let connection = Connection::open(&path).expect("noncooperating stale-header fixture");
    connection
        .execute("UPDATE gazette_journal SET writer_fence=writer_fence+1", [])
        .expect("external generation advance");
    connection.close().expect("close stale fixture");
    let mut next = input(&targets, &rows);
    next.id = Sha256Digest::of(b"never recorded");
    let error = writer
        .record_ref_observation(next, permission())
        .expect_err("stale lifetime");
    assert!(matches!(error.cause, Error::JournalMismatch));
    assert_eq!(error.effect, RefObservationEffect::NotAttempted);
    assert!(writer.close().is_err());
    assert!(
        Sqlite
            .inspect_ref_observation(&path, next.id, options())
            .expect("no stale insertion")
            .record
            .is_none()
    );
    assert_eq!(
        Sqlite
            .inspect_ref_observation(&path, receipt.record.id, options())
            .expect("preserved history")
            .record,
        Some(receipt.record)
    );
}

#[test]
fn committed_growth_retains_unverified_effect_and_independent_recovery_evidence() {
    let (root, mut writer) = fixture();
    let header = writer.inspect().expect("initial held header").header;
    writer.close().expect("close measurement lifetime");
    let path = root.path().join(crate::journal::DATABASE);
    let size = fs::metadata(&path).expect("native baseline bytes").len();
    let connection = Connection::open(&path).expect("measure native allocation");
    let free_pages: i64 = connection
        .pragma_query_value(None, "freelist_count", |row| row.get(0))
        .expect("native free pages");
    assert_eq!(free_pages, 0, "growth fixture must require new pages");
    connection.close().expect("close measurement");
    let bounded = ReadOptions {
        storage_bytes: size,
        ..options()
    };
    let mut writer = Sqlite
        .acquire_writer(root.path(), header, writer_options(bounded))
        .expect("acquire within old storage ceiling");
    let current = writer.inspect().expect("acquired coordinates").header;
    let (mut targets, rows) = data();
    targets[0].domain = "x".repeat(usize::try_from(size / 2).expect("bounded test payload"));
    let input = input(&targets, &rows);
    payload(input, current, bounded).expect("payload itself fits before insertion");
    let error = writer
        .record_ref_observation(input, permission())
        .expect_err("postcommit growth exceeds independent read-back ceiling");
    assert!(matches!(error.cause, Error::StorageLimit));
    assert_eq!(error.effect, RefObservationEffect::Unverified);
    assert!(writer.close().is_err());
    assert!(fs::metadata(&path).expect("committed database").len() > size);
    let recovered = Sqlite
        .inspect_ref_observation(&path, input.id, options())
        .expect("independent larger-bound inspection");
    assert_eq!(recovered.journal.header, current);
    let record = recovered
        .record
        .expect("failed delivery did not erase committed record");
    assert_eq!(record.as_input(), input);
    assert_eq!(record.created, current);
}
