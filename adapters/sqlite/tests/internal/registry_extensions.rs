//! Exact extension contracts/data share registry replacement and independent read-back.

use super::*;
use gazette_core::publication::{
    ExtensionSchema,
    content::{ContentKind, PageFamily},
};
use serde_json::{Value, json};

fn schema(maximum: u64) -> ExtensionSchema {
    ExtensionSchema {
        id: "schema:count".parse().expect("selected schema key"),
        schema: json!({
            "$schema":"https://json-schema.org/draft/2020-12/schema",
            "type":"object", "required":["count"], "additionalProperties":false,
            "properties":{"count":{"type":"integer","maximum":maximum},"note":{"type":"string"}}
        }),
    }
}

fn extension_data(value: Value) -> Data {
    let mut data = Data::new(true);
    let mut nodes: Vec<_> = data.graph.nodes().cloned().collect();
    nodes
        .iter_mut()
        .find(|node| node.id.as_str() == "content:one")
        .expect("existing page")
        .structure = ContentKind::Page {
        family: PageFamily::Extension {
            schema: "schema:count".parse().unwrap(),
            data: value,
            presentation: None,
        },
    };
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
    .expect("existing structural graph owner");
    data
}

#[test]
fn schema_and_payload_persist_exactly_replay_change_identity_and_remove_as_one_set() {
    let (root, mut writer) = fixture();
    let path = root.path().join(journal::DATABASE);
    let mut previous: Option<gazette_core::store::RegistryHead> = None;
    for (maximum, count) in [(5, 1), (5, 2), (8, 2)] {
        let schema = schema(maximum);
        let value = json!({"count":count,"note":"&amp; 🦀"});
        let data = extension_data(value.clone());
        let mut model = data.model();
        model.extensions.insert(&schema.id, &schema);
        let documents = data.documents();
        let receipt = writer
            .replace_registry(
                input(&model, &documents),
                RegistryWriteOptions {
                    allow_replace: true,
                },
            )
            .expect("complete registry replacement with borrowed schema");
        let connection = Connection::open(&path).expect("independent native rows");
        let stored_schema: String = connection
            .query_row(
                "SELECT schema_json FROM gazette_registry_extensions WHERE id='schema:count'",
                [],
                |row| row.get(0),
            )
            .expect("exact schema row");
        assert_eq!(
            serde_json::from_str::<Value>(&stored_schema).unwrap(),
            schema.schema
        );
        let (reference, stored_data): (String, String) = connection.query_row(
            "SELECT extension_schema,extension_data FROM gazette_registry_content WHERE id='content:one'", [], |row| Ok((row.get(0)?, row.get(1)?)),
        ).expect("paired content extension columns");
        assert_eq!(reference, schema.id.as_str());
        assert_eq!(serde_json::from_str::<Value>(&stored_data).unwrap(), value);
        let unrelated: (Option<String>, Option<String>) = connection.query_row(
            "SELECT extension_schema,extension_data FROM gazette_registry_content WHERE id='collection:one'", [], |row| Ok((row.get(0)?, row.get(1)?)),
        ).expect("ordinary container remains distinct");
        assert_eq!(unrelated, (None, None));
        connection.close().expect("close native read");
        assert_eq!(
            Sqlite
                .inspect_registry(&path, options())
                .expect("independent complete read-back")
                .registry,
            Some(receipt.registry)
        );
        if let Some(prior) = previous {
            assert_eq!(prior.rows, receipt.registry.rows);
            assert_ne!(prior.semantic_sha256, receipt.registry.semantic_sha256);
            assert_ne!(prior.rows_sha256, receipt.registry.rows_sha256);
        }
        let replay = writer
            .replace_registry(
                input(&model, &documents),
                RegistryWriteOptions {
                    allow_replace: true,
                },
            )
            .expect("exact schema and data replay");
        assert_eq!(receipt.registry, replay.registry);
        assert_eq!(receipt.journal, replay.journal);
        previous = Some(receipt.registry);
    }
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
        .expect("complete-set removal of schema and dependent content");
    assert_eq!(cleared.registry.rows, 2, "principal and site remain");
    assert_ne!(Some(cleared.registry), previous);
    writer.close().expect("verified writer close");
    assert_eq!(
        Sqlite
            .inspect_registry(&path, options())
            .expect("reopen cleared registry")
            .registry,
        Some(cleared.registry)
    );
    let connection = Connection::open(&path).expect("removed native rows");
    for table in ["gazette_registry_extensions", "gazette_registry_content"] {
        assert_eq!(
            connection
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
}

#[test]
fn missing_schema_and_invalid_native_pairs_preserve_the_previous_registry() {
    let (root, mut writer) = fixture();
    let path = root.path().join(journal::DATABASE);
    let schema = schema(5);
    let data = extension_data(json!({"count":1}));
    let mut model = data.model();
    model.extensions.insert(&schema.id, &schema);
    let documents = data.documents();
    let receipt = writer
        .replace_registry(
            input(&model, &documents),
            RegistryWriteOptions {
                allow_replace: true,
            },
        )
        .expect("initial complete extension registry");
    let before = fs::read(&path).expect("previous native bytes");
    model.extensions.clear();
    let error = writer
        .replace_registry(
            input(&model, &documents),
            RegistryWriteOptions {
                allow_replace: true,
            },
        )
        .expect_err("missing selected schema fails foreign-key qualification");
    assert_eq!(error.effect, RegistryEffect::NotAttempted);
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(
        Sqlite
            .inspect_registry(&path, options())
            .expect("original registry after refusal")
            .registry,
        Some(receipt.registry)
    );
    writer
        .close()
        .expect("release before native constraint checks");
    let connection = Connection::open(&path).expect("actual constraint boundary");
    crate::configure_connection(&connection, options()).expect("native foreign-key policy");
    for sql in [
        "UPDATE gazette_registry_content SET extension_schema='schema:missing' WHERE id='content:one'",
        "UPDATE gazette_registry_content SET extension_data=NULL WHERE id='content:one'",
        "UPDATE gazette_registry_content SET extension_data='not-json' WHERE id='content:one'",
        "UPDATE gazette_registry_content SET extension_schema='schema:count',extension_data='{}' WHERE id='collection:one'",
        "UPDATE gazette_registry_extensions SET schema_json='not-json' WHERE id='schema:count'",
    ] {
        assert!(
            connection.execute_batch(sql).is_err(),
            "native constraint missing: {sql}"
        );
        assert_eq!(
            verify(&connection, options()).expect("failed statement preserves exact previous rows"),
            Some(receipt.registry)
        );
    }
}

#[test]
fn shape_valid_schema_or_payload_tampering_is_detected_without_schema_revalidation_or_repair() {
    for sql in [
        "UPDATE gazette_registry_extensions SET schema_json='{\"type\":\"string\"}' WHERE id='schema:count'",
        "UPDATE gazette_registry_content SET extension_data='{\"count\":999}' WHERE id='content:one'",
    ] {
        let (root, mut writer) = fixture();
        let path = root.path().join(journal::DATABASE);
        let schema = schema(5);
        let data = extension_data(json!({"count":1}));
        let mut model = data.model();
        model.extensions.insert(&schema.id, &schema);
        let documents = data.documents();
        writer
            .replace_registry(
                input(&model, &documents),
                RegistryWriteOptions {
                    allow_replace: true,
                },
            )
            .expect("valid initial exact rows");
        writer
            .close()
            .expect("close before deliberate native corruption");
        Connection::open(&path)
            .expect("external fixture connection")
            .execute_batch(sql)
            .expect(
                "SQLite checks JSON/storage shape, not compiler-owned domain schema validation",
            );
        let corrupt = fs::read(&path).expect("actual corrupted bytes");
        assert!(
            matches!(
                Sqlite.inspect_registry(&path, options()),
                Err(Error::RegistryVerification)
            ),
            "changed exact row identity: {sql}"
        );
        assert_eq!(
            fs::read(&path).unwrap(),
            corrupt,
            "read-back never repairs changed rows"
        );
    }
}
