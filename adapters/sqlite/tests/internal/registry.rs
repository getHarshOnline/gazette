//! Real relational replacement, independent read-back and native transaction failures.

use super::*;
use gazette_core::{
    publication::{
        self, Key, Principal, PrincipalKind,
        content::{ContentNode, Declaration, Graph},
        registry::{Projection, Provenance, Site},
    },
    store::{
        InitializeOptions, RegistryEffect, RegistryWriteOptions, SessionOptions, WriterOptions,
    },
};
use gazette_ports::store::{JournalWriter, RegistryWriter, StoreInitializer, WriterConnector};
use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
use std::{collections::BTreeMap, fs, os::unix::fs::PermissionsExt};

#[path = "registry_roles.rs"]
mod roles;

#[path = "registry_principal_websites.rs"]
mod principal_websites;

#[path = "registry_brands.rs"]
mod brands;

#[path = "registry_mirrors.rs"]
mod mirrors;

#[path = "registry_projects.rs"]
mod projects;

#[path = "registry_placements.rs"]
mod placements;

#[path = "registry_project_licenses.rs"]
mod project_licenses;

#[path = "registry_origins.rs"]
mod origins;

#[path = "registry_licenses.rs"]
mod licenses;

#[path = "registry_taxonomies.rs"]
mod taxonomies;

#[path = "registry_extensions.rs"]
mod extensions;

#[path = "registry_canonical.rs"]
mod canonical;

#[path = "registry_discussions.rs"]
mod discussions;
#[path = "registry_subjects.rs"]
mod subjects;

fn options() -> ReadOptions {
    ReadOptions {
        allow_coordination_files: true,
        ..Default::default()
    }
}

fn fixture() -> (tempfile::TempDir, crate::Writer) {
    let root = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .expect("private store");
    let session = SessionOptions {
        allow_local_reservation: true,
        read: options(),
    };
    let initial = Sqlite
        .initialize(
            root.path(),
            InitializeOptions {
                allow_create: true,
                session,
                ..Default::default()
            },
        )
        .expect("actual initial schema");
    let writer = Sqlite
        .acquire_writer(
            root.path(),
            initial.header,
            WriterOptions {
                allow_generation_advance: true,
                session,
            },
        )
        .expect("held native generation");
    (root, writer)
}

struct Data {
    principal: Principal,
    site: Key,
    source: Key,
    graph: Graph,
}

impl Data {
    fn new(populated: bool) -> Self {
        let principal = Principal {
            id: "person:one".parse().expect("key"),
            kind: PrincipalKind::Person,
            name: Some("Exact principal".into()),
            website: None,
            same_as: Vec::new(),
        };
        let nodes: Vec<ContentNode> = if populated {
            [
                serde_json::json!({"id":"collection:one","site":"site:one","title":"Collection","lifecycle":"archived","structure":{"kind":"collection","parent":null}}),
                serde_json::json!({"id":"content:one","site":"site:one","title":"Draft","lifecycle":"draft","route":{"path":"/draft/","aliases":["/old/"]},"attributions":[{"principal":"person:one","role":"rights-holder"},{"principal":"person:one","role":"author"}],"structure":{"kind":"page","family":{"kind":"article"}}}),
            ].into_iter().map(|value| serde_json::from_value(value).expect("typed node")).collect()
        } else {
            vec![]
        };
        let memberships = if populated {
            vec![publication::content::CollectionMembership {
                content: "content:one".parse().expect("key"),
                collection: "collection:one".parse().expect("key"),
            }]
        } else {
            vec![]
        };
        let graph = Graph::resolve(
            Declaration {
                nodes,
                memberships,
                related: vec![],
                browsing: vec![],
                projects: vec![],
            },
            Default::default(),
        )
        .expect("actual graph resolution");
        Self {
            principal,
            site: "site:one".parse().expect("key"),
            source: "source:one".parse().expect("key"),
            graph,
        }
    }

    fn model(&self) -> Projection<'_> {
        Projection {
            principals: BTreeMap::from([(&self.principal.id, &self.principal)]),
            projects: BTreeMap::new(),
            taxonomies: BTreeMap::new(),
            extensions: BTreeMap::new(),
            brands: BTreeMap::new(),
            sites: BTreeMap::from([(
                &self.site,
                Site {
                    publisher: &self.principal.id,
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
            content: &self.graph,
            routes: BTreeMap::new(),
            provenance: self
                .graph
                .nodes()
                .map(|node| {
                    (
                        &node.id,
                        Provenance {
                            source: &self.source,
                            entry: node.id.as_str(),
                            project: None,
                        },
                    )
                })
                .collect(),
        }
    }

    fn documents(&self) -> BTreeMap<&Key, Sha256Digest> {
        self.graph
            .nodes()
            .map(|node| (&node.id, Sha256Digest::of(node.title.as_bytes())))
            .collect()
    }
}

#[test]
fn registry_retains_collection_purpose_outline_and_order_without_reusing_leaf_family() {
    use publication::content::{CollectionPurpose, ContentKind, NavigationItem};
    let mut data = Data::new(true);
    let mut nodes: Vec<_> = data.graph.nodes().cloned().collect();
    for node in &mut nodes {
        if let ContentKind::Collection {
            purpose,
            navigation,
            member_order,
            ..
        } = &mut node.structure
        {
            *purpose = Some(CollectionPurpose::Documentation);
            *member_order = vec!["content:one".parse().unwrap()];
            *navigation = Some(vec![NavigationItem::Group {
                group: "Use the tool".into(),
                pages: vec![NavigationItem::Content {
                    content: "content:one".parse().unwrap(),
                }],
            }]);
        }
    }
    data.graph = Graph::resolve(
        Declaration {
            nodes,
            memberships: data.graph.memberships().iter().cloned().collect(),
            related: vec![],
            browsing: vec![],
            projects: vec![],
        },
        Default::default(),
    )
    .unwrap();
    let (root, mut writer) = fixture();
    let model = data.model();
    let documents = data.documents();
    let receipt = writer
        .replace_registry(
            input(&model, &documents),
            RegistryWriteOptions {
                allow_replace: true,
            },
        )
        .unwrap();
    let path = root.path().join(crate::journal::DATABASE);
    let actual = Sqlite.inspect_registry(&path, options()).unwrap();
    assert_eq!(actual.registry, Some(receipt.registry));
    let connection = Connection::open(&path).unwrap();
    let collection: (Option<String>, String) = connection.query_row("SELECT page_family,collection_purpose FROM gazette_registry_content WHERE id='collection:one'", [], |row| Ok((row.get(0)?,row.get(1)?))).unwrap();
    assert_eq!(collection, (None, "documentation".into()));
    let navigation: String = connection.query_row("SELECT collection_navigation_json FROM gazette_registry_content WHERE id='collection:one'", [], |row| row.get(0)).unwrap();
    let restored: Vec<NavigationItem> = serde_json::from_str(&navigation).unwrap();
    let ContentKind::Collection {
        navigation: Some(declared),
        ..
    } = &data
        .graph
        .node(&"collection:one".parse().unwrap())
        .unwrap()
        .structure
    else {
        panic!()
    };
    assert_eq!(&restored, declared);
    let order: String = connection.query_row("SELECT collection_member_order_json FROM gazette_registry_content WHERE id='collection:one'", [], |row| row.get(0)).unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<String>>(&order).unwrap(),
        ["content:one"]
    );
    let leaf_order: Option<String> = connection.query_row("SELECT collection_member_order_json FROM gazette_registry_content WHERE id='content:one'", [], |row| row.get(0)).unwrap();
    assert!(leaf_order.is_none());
    let page: (String, Option<String>) = connection.query_row("SELECT page_family,collection_purpose FROM gazette_registry_content WHERE id='content:one'", [], |row| Ok((row.get(0)?,row.get(1)?))).unwrap();
    assert_eq!(page, ("article".into(), None));
}

fn input<'a>(
    model: &'a Projection<'a>,
    documents: &'a BTreeMap<&'a Key, Sha256Digest>,
) -> RegistryInput<'a> {
    RegistryInput {
        model,
        documents,
        semantic_sha256: model.semantic_sha256(1 << 20).expect("semantic identity"),
        configuration_sha256: Sha256Digest::of(b"exact config"),
    }
}

#[test]
fn full_replacement_removes_stale_content_and_preserves_journal_and_catalogs() {
    let (root, mut writer) = fixture();
    use gazette_core::{
        release::{CommitMetadata, CommitReport, CommitSignature},
        source::TreeIdentity,
        store::{
            CheckpointInput, CheckpointWriteOptions, PreparationInput, PreparationRetentionInput,
            PreparationWriteOptions,
        },
    };
    use gazette_ports::store::{
        CheckpointReader, CheckpointWriter, PreparationReader, PreparationWriter,
    };
    let signature = CommitSignature {
        name: "Fixture".into(),
        email: "fixture@example.test".into(),
        seconds: 1,
        offset_minutes: 0,
    };
    let metadata = CommitMetadata {
        parent: None,
        author: signature.clone(),
        committer: signature,
        message: "retained checkpoint".into(),
    };
    let report = CommitReport {
        commit: "1111111111111111111111111111111111111111".parse().expect("checkpoint OID"),
        output: serde_json::from_value(serde_json::json!({"format":TreeIdentity::FORMAT,"sha256":Sha256Digest::of(b"checkpoint tree")})).expect("typed checkpoint tree"),
        parent: None, entries: 1, bytes: 12,
    };
    let checkpoint = writer
        .record_checkpoint(
            CheckpointInput {
                id: Sha256Digest::of(b"durable checkpoint"),
                repository: "/private/checkpoint.git",
                metadata: &metadata,
                commit: &report,
            },
            CheckpointWriteOptions { allow_record: true },
        )
        .expect("durable evidence before registry replacement");
    let address = gazette_core::stage::Address {
        parent: "/private/prepared-output".into(),
        name: "exact-stage".into(),
    };
    let evidence = "{\"private\":\"retained exact render evidence\"}";
    let intent = writer
        .record_preparation_intent(
            PreparationInput {
                id: Sha256Digest::of(b"durable preparation"),
                address: &address,
                destination: None,
                output: report.output,
                entries: report.entries as u64,
                bytes: report.bytes,
                evidence,
                evidence_sha256: Sha256Digest::of(evidence.as_bytes()),
            },
            PreparationWriteOptions { allow_record: true },
        )
        .expect("independent preparation intent");
    let preparation = writer
        .record_preparation_retention(
            PreparationRetentionInput {
                intent: &intent.record.intent,
                output: report.output,
                entries: report.entries as u64,
                bytes: report.bytes,
            },
            PreparationWriteOptions { allow_record: true },
        )
        .expect("independent preparation retention");
    let check_evidence = "{\"private_check\":\"exact retained checker evidence\"}";
    let preparation = writer
        .record_preparation_check(
            gazette_core::store::PreparationCheckInput {
                intent: &preparation.record.intent,
                retention: preparation
                    .record
                    .retention
                    .as_ref()
                    .expect("retained stage"),
                evidence: check_evidence,
                evidence_sha256: gazette_core::source::Sha256Digest::of(check_evidence.as_bytes()),
            },
            PreparationWriteOptions { allow_record: true },
        )
        .expect("independent check evidence before registry replacement");
    let preparation = writer
        .record_preparation_worktree(
            gazette_core::store::PreparationWorktreeInput {
                intent: &preparation.record.intent,
                retention: preparation
                    .record
                    .retention
                    .as_ref()
                    .expect("retained stage"),
                metadata: &metadata,
                check: preparation.record.check.as_ref(),
                parent_source: None,
            },
            PreparationWriteOptions { allow_record: true },
        )
        .expect("durable Git intent before registry replacement");
    let preparation = writer
        .record_preparation_commit(
            gazette_core::store::PreparationCommitInput {
                intent: &preparation.record.intent,
                retention: preparation
                    .record
                    .retention
                    .as_ref()
                    .expect("retained stage"),
                worktree: preparation.record.worktree.as_ref().expect("Git intent"),
                check: preparation.record.check.as_ref(),
                report: &report,
            },
            PreparationWriteOptions { allow_record: true },
        )
        .expect("durable Git completion before registry replacement");
    let path = root.path().join(journal::DATABASE);
    let journal = writer.inspect().expect("held generation").header;
    assert!(
        Sqlite
            .inspect_registry(&path, options())
            .expect("fresh empty registry")
            .registry
            .is_none()
    );
    let full = Data::new(true);
    let model = full.model();
    let documents = full.documents();
    let receipt = writer
        .replace_registry(
            input(&model, &documents),
            RegistryWriteOptions {
                allow_replace: true,
            },
        )
        .expect("complete real replacement");
    assert_eq!(receipt.journal, journal);
    assert!(receipt.registry.rows > 2);
    let inspection = Sqlite
        .inspect_registry(&path, options())
        .expect("independent registry read-back");
    assert_eq!(inspection.journal.header, journal);
    assert_eq!(inspection.registry, Some(receipt.registry));
    let repeated = writer
        .replace_registry(
            input(&model, &documents),
            RegistryWriteOptions {
                allow_replace: true,
            },
        )
        .expect("idempotent desired state");
    assert_eq!(receipt.registry, repeated.registry);
    assert_eq!(
        Sqlite
            .inspect_checkpoint(&path, checkpoint.checkpoint.id, options())
            .expect("registry replacement preserves independent history")
            .checkpoint,
        Some(checkpoint.checkpoint.clone())
    );
    let connection = Connection::open(&path).expect("read fixture rows");
    let roles: Vec<String> = connection
        .prepare("SELECT role FROM gazette_registry_attributions ORDER BY position")
        .expect("ordered SQL")
        .query_map([], |row| row.get(0))
        .expect("query")
        .collect::<rusqlite::Result<_>>()
        .expect("values");
    assert_eq!(roles, ["rights-holder", "author"]);
    let declared: String = connection
        .query_row(
            "SELECT declared_path FROM gazette_registry_content WHERE id='content:one'",
            [],
            |row| row.get(0),
        )
        .expect("uncompiled route intent");
    assert_eq!(declared, "/draft/");
    let alias: String = connection
        .query_row(
            "SELECT path FROM gazette_registry_declared_aliases WHERE content='content:one'",
            [],
            |row| row.get(0),
        )
        .expect("uncompiled alias intent");
    assert_eq!(alias, "/old/");
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM gazette_registry_routes", [], |row| {
                row.get::<_, i64>(0)
            })
            .expect("no invented public URL"),
        0
    );
    connection.close().expect("close fixture read");
    let empty = Data::new(false);
    let model = empty.model();
    let documents = empty.documents();
    let cleared = writer
        .replace_registry(
            input(&model, &documents),
            RegistryWriteOptions {
                allow_replace: true,
            },
        )
        .expect("complete empty content, not skip");
    assert_eq!(cleared.registry.rows, 2, "principal and site remain");
    assert_ne!(cleared.registry.rows_sha256, receipt.registry.rows_sha256);
    assert_eq!(cleared.journal, journal);
    writer.close().expect("final held-lifetime verification");
    assert_eq!(
        Sqlite
            .inspect_checkpoint(&path, checkpoint.checkpoint.id, options())
            .expect("complete registry removals preserve durable checkpoint")
            .checkpoint,
        Some(checkpoint.checkpoint)
    );
    let connection = Connection::open(&path).expect("read final tables");
    assert_eq!(
        Sqlite
            .inspect_preparation(&path, preparation.record.intent.id, options())
            .expect("complete registry replacement preserves every preparation phase")
            .record,
        Some(preparation.record)
    );
    for table in [
        "content",
        "memberships",
        "attributions",
        "provenance",
        "declared_aliases",
    ] {
        let count: i64 = connection
            .query_row(
                &format!("SELECT count(*) FROM gazette_registry_{table}"),
                [],
                |row| row.get(0),
            )
            .expect("removed rows");
        assert_eq!(count, 0);
    }
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM gazette_journal", [], |row| row
                .get::<_, i64>(0))
            .expect("journal retained"),
        1
    );
}

#[test]
fn permission_forged_identity_coverage_and_foreign_keys_fail_before_store_mutation() {
    let (root, mut writer) = fixture();
    let path = root.path().join(journal::DATABASE);
    let before = fs::read(&path).expect("exact baseline");
    let data = Data::new(true);
    let mut model = data.model();
    let documents = data.documents();
    let error = writer
        .replace_registry(input(&model, &documents), RegistryWriteOptions::default())
        .expect_err("separate registry permission");
    assert!(matches!(error.cause, Error::RegistryPermission));
    assert_eq!(error.effect, RegistryEffect::NotAttempted);
    let mut wrong = input(&model, &documents);
    wrong.semantic_sha256 = Sha256Digest::of(b"wrong");
    let error = writer
        .replace_registry(
            wrong,
            RegistryWriteOptions {
                allow_replace: true,
            },
        )
        .expect_err("exact semantic input binding");
    assert!(matches!(error.cause, Error::RegistryVerification));
    assert_eq!(error.effect, RegistryEffect::NotAttempted);
    let absent_documents = BTreeMap::new();
    let error = writer
        .replace_registry(
            input(&model, &absent_documents),
            RegistryWriteOptions {
                allow_replace: true,
            },
        )
        .expect_err("complete document coverage");
    assert_eq!(error.effect, RegistryEffect::NotAttempted);
    let missing: Key = "person:missing".parse().expect("key");
    model.sites.get_mut(&data.site).expect("site").publisher = &missing;
    let error = writer
        .replace_registry(
            input(&model, &documents),
            RegistryWriteOptions {
                allow_replace: true,
            },
        )
        .expect_err("native constraint qualification before destination");
    assert_eq!(error.effect, RegistryEffect::NotAttempted);
    assert_eq!(fs::read(&path).expect("unchanged store"), before);
    assert!(
        Sqlite
            .inspect_registry(&path, options())
            .expect("still fresh")
            .registry
            .is_none()
    );
}

#[test]
fn native_failure_after_deletes_rolls_back_complete_old_rows_and_head() {
    let (root, mut writer) = fixture();
    let full = Data::new(true);
    let model = full.model();
    let documents = full.documents();
    let receipt = writer
        .replace_registry(
            input(&model, &documents),
            RegistryWriteOptions {
                allow_replace: true,
            },
        )
        .expect("old complete registry");
    writer.close().expect("release fixture writer");
    let path = root.path().join(journal::DATABASE);
    let before = fs::read(&path).expect("old bytes");
    let empty = Data::new(false);
    let model = empty.model();
    let documents = empty.documents();
    let desired =
        Desired::prepare(input(&model, &documents), options()).expect("qualified replacement");
    let mut connection = Connection::open(&path).expect("native failure fixture");
    crate::configure_connection(&connection, options()).expect("actual adapter policy");
    connection.authorizer(Some(|context: AuthContext<'_>| {
        if matches!(context.action, AuthAction::Insert { table_name } if table_name == "gazette_registry_principals") { Authorization::Deny } else { Authorization::Allow }
    })).expect("fail insertion after owned deletes");
    let transaction = connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .expect("actual transaction");
    assert!(desired.replace(&transaction, options()).is_err());
    transaction.rollback().expect("native rollback");
    connection.close().expect("close failed native transaction");
    assert_eq!(fs::read(&path).expect("old bytes survive"), before);
    let after = Sqlite
        .inspect_registry(&path, options())
        .expect("independent previous-state read");
    assert_eq!(after.registry, Some(receipt.registry));
    assert_eq!(after.journal.header, receipt.journal);
}

#[test]
fn altered_rows_head_and_foreign_key_corruption_are_refused() {
    for sql in [
        "UPDATE gazette_registry_content SET title='tampered' WHERE id='content:one'",
        "UPDATE gazette_registry_head SET rows=rows+1",
        "DELETE FROM gazette_registry_head",
        "PRAGMA foreign_keys=OFF; UPDATE gazette_registry_sites SET publisher='missing:principal'",
    ] {
        let (root, mut writer) = fixture();
        let data = Data::new(true);
        let model = data.model();
        let documents = data.documents();
        writer
            .replace_registry(
                input(&model, &documents),
                RegistryWriteOptions {
                    allow_replace: true,
                },
            )
            .expect("valid registry");
        writer.close().expect("close before test corruption");
        let path = root.path().join(journal::DATABASE);
        Connection::open(&path)
            .expect("external fixture writer")
            .execute_batch(sql)
            .expect("exact corrupting change");
        let before = fs::read(&path).expect("corrupt bytes");
        assert!(Sqlite.inspect_registry(&path, options()).is_err(), "{sql}");
        assert_eq!(fs::read(&path).expect("inspection must not repair"), before);
    }
}

#[test]
fn exact_schema_comparison_rejects_added_removed_or_modified_native_objects() {
    for sql in [
        "CREATE TABLE stray(value TEXT)",
        "CREATE INDEX stray ON gazette_registry_content(title)",
        "DROP INDEX gazette_registry_primary_journey",
        "ALTER TABLE gazette_registry_principals ADD COLUMN guessed_authority TEXT",
    ] {
        let mut connection = Connection::open_in_memory().expect("schema fixture");
        crate::schema::initialize(&mut connection).expect("sole native DDL");
        connection.execute_batch(sql).expect("schema mutation");
        assert!(
            matches!(journal::inspect(&connection), Err(Error::JournalSchema)),
            "{sql}"
        );
    }
}

#[test]
fn committed_growth_beyond_observed_ceiling_invalidates_writer_without_claiming_rollback() {
    let (root, mut initial_writer) = fixture();
    let path = root.path().join(journal::DATABASE);
    let previous = initial_writer.inspect().expect("initial generation").header;
    initial_writer.close().expect("release initial lifetime");
    let initial_bytes = fs::metadata(&path).expect("measured native size").len();
    let connection = Connection::open(&path).expect("inspect native page allocation");
    let page_bytes: u32 = connection
        .pragma_query_value(None, "page_size", |row| row.get(0))
        .expect("native page size");
    let free_pages: i64 = connection
        .pragma_query_value(None, "freelist_count", |row| row.get(0))
        .expect("native free pages");
    assert_eq!(free_pages, 0, "fresh schema has no spare overflow pages");
    connection.close().expect("close native measurement");
    let bounded = ReadOptions {
        storage_bytes: initial_bytes,
        ..options()
    };
    let mut writer = Sqlite
        .acquire_writer(
            root.path(),
            previous,
            WriterOptions {
                allow_generation_advance: true,
                session: SessionOptions {
                    allow_local_reservation: true,
                    read: bounded,
                },
            },
        )
        .expect("acquire existing generation within measured ceiling");
    let acquired = writer.inspect().expect("exact acquired generation").header;
    assert_eq!(acquired.writer_fence, previous.writer_fence + 1);

    // A single value larger than a native page requires overflow allocation.
    // Half the measured initial size leaves room for bounded row/identity framing;
    // neither the SQLite page size nor platform-specific schema file size is assumed.
    let text_bytes = initial_bytes / 2;
    assert!(text_bytes > u64::from(page_bytes) * 2);
    assert!(text_bytes < u64::try_from(options().value_bytes).expect("positive value bound"));
    let mut data = Data::new(false);
    data.principal.name =
        Some("p".repeat(usize::try_from(text_bytes).expect("bounded native size")));
    let model = data.model();
    let documents = data.documents();
    let desired = Desired::prepare(input(&model, &documents), bounded)
        .expect("replacement itself fits preparation limits");
    let error = writer
        .replace_registry(
            input(&model, &documents),
            RegistryWriteOptions {
                allow_replace: true,
            },
        )
        .expect_err("post-commit storage observation exceeds ceiling");
    assert!(matches!(error.cause, Error::StorageLimit));
    assert_eq!(error.effect, RegistryEffect::Unverified);
    assert!(fs::metadata(&path).expect("committed native size").len() > initial_bytes);
    assert!(
        matches!(writer.inspect(), Err(Error::Changed)),
        "failed lifetime cannot be reused"
    );
    assert!(
        matches!(writer.close(), Err(Error::Changed)),
        "close cannot promote unverified state"
    );

    let actual = Sqlite
        .inspect_registry(&path, options())
        .expect("independent read with sufficient observation bound");
    assert_eq!(
        actual.journal.header, acquired,
        "no generation reset or hidden acquisition"
    );
    assert_eq!(
        actual.registry,
        Some(desired.head),
        "committed replacement survives the failed receipt"
    );
    let connection = Connection::open(&path).expect("read committed scalar");
    let stored: String = connection
        .query_row(
            "SELECT name FROM gazette_registry_principals WHERE id='person:one'",
            [],
            |row| row.get(0),
        )
        .expect("committed complete principal");
    assert_eq!(Some(stored), data.principal.name);
    connection.close().expect("close independent proof");
}
