//! Canonical copies remain distinct routed objects through native registry replacement.

use super::*;
use gazette_core::{publication::registry::Route, source::EntryPath};

fn data(copy: bool) -> Data {
    let mut data = Data::new(false);
    let mut nodes = vec![serde_json::json!({
        "id":"content:one","site":"site:one","title":"Original","lifecycle":"published",
        "route":{"path":"/original/"},"structure":{"kind":"page","family":{"kind":"article"}}
    })];
    if copy {
        nodes.push(serde_json::json!({
            "id":"content:copy","site":"site:one","title":"Copy","lifecycle":"published",
            "route":{"path":"/copy/","canonical":"content:one"},"structure":{"kind":"page","family":{"kind":"article"}}
        }));
    }
    data.graph = Graph::resolve(
        Declaration {
            nodes: nodes
                .into_iter()
                .map(|value| serde_json::from_value(value).unwrap())
                .collect(),
            memberships: vec![],
            related: vec![],
            browsing: vec![],
            projects: vec![],
        },
        Default::default(),
    )
    .expect("native distinct content graph");
    data
}

fn model<'a>(data: &'a Data, original: &'a EntryPath, copy: &'a EntryPath) -> Projection<'a> {
    let mut model = data.model();
    for node in data.graph.nodes() {
        let canonical = node.route.as_ref().unwrap().canonical.as_ref();
        model.routes.insert(
            &node.id,
            Route {
                url: if canonical.is_some() {
                    "https://one.example/copy/"
                } else {
                    "https://one.example/original/"
                },
                canonical,
                canonical_url: "https://one.example/original/",
                output: if canonical.is_some() { copy } else { original },
                aliases: vec![],
                mirrors: vec![],
            },
        );
    }
    model
}

fn routes(connection: &Connection) -> Vec<(String, String, Option<String>, String, String)> {
    connection.prepare("SELECT content,url,canonical_content,canonical_url,output FROM gazette_registry_routes ORDER BY content")
        .unwrap().query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)))
        .unwrap().collect::<rusqlite::Result<_>>().unwrap()
}

#[test]
fn canonical_copies_reconcile_reopen_and_remove_without_conflating_serving_addresses() {
    let (root, mut writer) = fixture();
    let data = data(true);
    let original: EntryPath = "original/index.html".parse().unwrap();
    let copy: EntryPath = "copy/index.html".parse().unwrap();
    let model = model(&data, &original, &copy);
    let documents = data.documents();
    let receipt = writer
        .replace_registry(
            input(&model, &documents),
            RegistryWriteOptions {
                allow_replace: true,
            },
        )
        .unwrap();
    let path = root.path().join(journal::DATABASE);
    assert_eq!(
        Sqlite.inspect_registry(&path, options()).unwrap().registry,
        Some(receipt.registry)
    );
    assert_eq!(
        routes(&Connection::open(&path).unwrap()),
        vec![
            (
                "content:copy".into(),
                "https://one.example/copy/".into(),
                Some("content:one".into()),
                "https://one.example/original/".into(),
                "copy/index.html".into()
            ),
            (
                "content:one".into(),
                "https://one.example/original/".into(),
                None,
                "https://one.example/original/".into(),
                "original/index.html".into()
            ),
        ]
    );
    let repeated = writer
        .replace_registry(
            input(&model, &documents),
            RegistryWriteOptions {
                allow_replace: true,
            },
        )
        .unwrap();
    assert_eq!(repeated.registry, receipt.registry);
    assert_eq!(repeated.journal, receipt.journal);
    let without_copy = self::data(false);
    let removed_model = self::model(&without_copy, &original, &copy);
    let remaining_documents = without_copy.documents();
    let removed = writer
        .replace_registry(
            input(&removed_model, &remaining_documents),
            RegistryWriteOptions {
                allow_replace: true,
            },
        )
        .unwrap();
    assert_ne!(removed.registry.rows_sha256, receipt.registry.rows_sha256);
    assert_ne!(
        removed.registry.semantic_sha256,
        receipt.registry.semantic_sha256
    );
    assert_eq!(removed.journal, receipt.journal);
    writer.close().unwrap();
    assert_eq!(
        Sqlite.inspect_registry(&path, options()).unwrap().registry,
        Some(removed.registry)
    );
    let connection = Connection::open(&path).unwrap();
    assert_eq!(routes(&connection).len(), 1);
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM gazette_registry_content", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM gazette_registry_mirrors", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        0,
        "copies are not redirect mirrors"
    );
}

#[test]
fn inconsistent_canonical_projections_fail_before_existing_database_effects() {
    let (root, mut writer) = fixture();
    let data = data(true);
    let original: EntryPath = "original/index.html".parse().unwrap();
    let copy: EntryPath = "copy/index.html".parse().unwrap();
    let copy_id: Key = "content:copy".parse().unwrap();
    let original_id: Key = "content:one".parse().unwrap();
    let documents = data.documents();
    let valid = model(&data, &original, &copy);
    writer
        .replace_registry(
            input(&valid, &documents),
            RegistryWriteOptions {
                allow_replace: true,
            },
        )
        .unwrap();
    let path = root.path().join(journal::DATABASE);
    let before = fs::read(&path).unwrap();
    for fault in 0..4 {
        let mut bad = model(&data, &original, &copy);
        match fault {
            0 => {
                bad.routes.remove(&original_id);
            }
            1 => {
                bad.routes.get_mut(&copy_id).unwrap().canonical = None;
            }
            2 => {
                bad.routes.get_mut(&copy_id).unwrap().canonical_url = "https://one.example/wrong/";
            }
            3 => {
                bad.routes.get_mut(&original_id).unwrap().canonical = Some(&copy_id);
            }
            _ => unreachable!(),
        }
        let error = writer
            .replace_registry(
                input(&bad, &documents),
                RegistryWriteOptions {
                    allow_replace: true,
                },
            )
            .unwrap_err();
        assert_eq!(error.effect, RegistryEffect::NotAttempted);
        assert_eq!(fs::read(&path).unwrap(), before);
    }
    writer.close().unwrap();
}

#[test]
fn canonical_foreign_keys_rollback_and_complete_row_digest_detect_native_tampering() {
    let (root, mut writer) = fixture();
    let data = data(true);
    let original: EntryPath = "original/index.html".parse().unwrap();
    let copy: EntryPath = "copy/index.html".parse().unwrap();
    let model = model(&data, &original, &copy);
    let documents = data.documents();
    let receipt = writer
        .replace_registry(
            input(&model, &documents),
            RegistryWriteOptions {
                allow_replace: true,
            },
        )
        .unwrap();
    writer.close().unwrap();
    let path = root.path().join(journal::DATABASE);
    let mut connection = Connection::open(&path).unwrap();
    crate::configure_connection(&connection, options()).unwrap();
    let before = routes(&connection);
    for update in [
        "UPDATE gazette_registry_routes SET canonical_content='content:absent' WHERE content='content:copy'",
        "UPDATE gazette_registry_routes SET canonical_content='content:copy' WHERE content='content:copy'",
        "UPDATE gazette_registry_routes SET canonical_url='https://one.example/wrong/' WHERE content='content:copy'",
        "UPDATE gazette_registry_routes SET canonical_content=NULL WHERE content='content:copy'",
    ] {
        assert!(
            connection.execute(update, []).is_err(),
            "native canonical endpoint constraint"
        );
        assert_eq!(routes(&connection), before);
    }
    let desired = Desired::prepare(input(&model, &documents), options()).unwrap();
    connection.authorizer(Some(|context: AuthContext<'_>| {
        if matches!(context.action, AuthAction::Insert { table_name } if table_name == "gazette_registry_routes") {
            Authorization::Deny
        } else { Authorization::Allow }
    })).unwrap();
    let transaction = connection.transaction().unwrap();
    assert!(desired.replace(&transaction, options()).is_err());
    transaction.rollback().unwrap();
    connection
        .authorizer(None::<fn(AuthContext<'_>) -> Authorization>)
        .unwrap();
    assert_eq!(routes(&connection), before);
    assert_eq!(
        crate::registry::verify(&connection, options()).unwrap(),
        Some(receipt.registry)
    );
    connection.execute("UPDATE gazette_registry_routes SET url='https://one.example/tampered-copy/' WHERE content='content:copy'", []).unwrap();
    assert!(
        crate::registry::verify(&connection, options()).is_err(),
        "valid SQL still cannot bypass complete row identity"
    );
}
