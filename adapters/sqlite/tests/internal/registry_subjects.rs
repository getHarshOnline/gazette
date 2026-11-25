//! Page-subject changes preserve the registry schema and journal history.

use super::*;
use crate::schema;
use rusqlite::types::Value;

fn data(subject: serde_json::Value, about: bool) -> Data {
    let mut data = Data::new(false);
    let mut node = serde_json::json!({
        "id":"content:one","site":"site:one","title":"Page","lifecycle":"draft",
        "structure":{"kind":"page","family":{"kind":"utility"}}
    });
    if !subject.is_null() {
        node["subject"] = subject;
    }
    if about {
        node["schema_type"] = serde_json::json!("AboutPage");
    }
    data.graph = Graph::resolve(
        Declaration {
            nodes: vec![serde_json::from_value(node).unwrap()],
            memberships: vec![],
            related: vec![],
            browsing: vec![],
            projects: vec![],
        },
        Default::default(),
    )
    .unwrap();
    data
}

fn history(connection: &Connection) -> BTreeMap<String, Vec<Vec<Value>>> {
    let names: Vec<String> = connection.prepare("SELECT name FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'gazette_registry_%' AND name NOT LIKE 'sqlite_%' ORDER BY name").unwrap()
        .query_map([], |row| row.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
    names
        .into_iter()
        .map(|name| {
            let mut statement = connection
                .prepare(&format!("SELECT * FROM {name}"))
                .unwrap();
            let columns = statement.column_count();
            let rows = statement
                .query_map([], |row| {
                    (0..columns)
                        .map(|index| row.get(index))
                        .collect::<rusqlite::Result<Vec<Value>>>()
                })
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap();
            (name, rows)
        })
        .collect()
}

#[test]
fn v6_subject_heads_change_without_rewriting_rows_schema_or_journal_history() {
    assert_eq!(schema::VERSION, 6);
    assert_eq!(
        schema::digest().to_string(),
        "1c0a88175582ea2a8a023e5f4ce02f47b17241f393fc55fa5329984fbc3c5eb0"
    );
    let (root, mut writer) = fixture();
    let path = root.path().join(journal::DATABASE);
    let connection = Connection::open(&path).unwrap();
    let before = history(&connection);
    let header = writer.inspect().unwrap().header;
    let other = Principal {
        legal_name: None,
        id: "person:other".parse().unwrap(),
        kind: PrincipalKind::Person,
        name: None,
        website: None,
        same_as: vec![],
    };
    let service = serde_json::json!({"kind":"service","id":"service:one","name":"Name","provider":"person:one","description":"Description"});
    let mut cases = vec![(serde_json::Value::Null, false), (service.clone(), false)];
    for (field, value) in [
        ("id", "service:other"),
        ("name", "Other name"),
        ("provider", "person:other"),
        ("description", "Other description"),
    ] {
        let mut changed = service.clone();
        changed[field] = serde_json::json!(value);
        cases.push((changed, false));
    }
    cases.push((
        serde_json::json!({"kind":"principal","principal":"person:one"}),
        true,
    ));
    cases.push((
        serde_json::json!({"kind":"principal","principal":"person:other"}),
        true,
    ));
    let mut semantic = std::collections::BTreeSet::new();
    let mut row_digest = None;
    for (subject, about) in cases {
        let data = data(subject, about);
        let documents = data.documents();
        let mut model = data.model();
        model.principals.insert(&other.id, &other);
        let receipt = writer
            .replace_registry(
                input(&model, &documents),
                RegistryWriteOptions {
                    allow_replace: true,
                },
            )
            .unwrap();
        assert!(semantic.insert(receipt.registry.semantic_sha256));
        if let Some(expected) = row_digest {
            assert_eq!(receipt.registry.rows_sha256, expected);
        } else {
            row_digest = Some(receipt.registry.rows_sha256);
        }
        assert_eq!(receipt.journal, header);
        let inspected = Sqlite.inspect_registry(&path, options()).unwrap();
        assert_eq!(inspected.registry, Some(receipt.registry));
        assert_eq!(inspected.journal.header, header);
        assert_eq!(history(&connection), before);
        assert_eq!(
            schema::digest().to_string(),
            "1c0a88175582ea2a8a023e5f4ce02f47b17241f393fc55fa5329984fbc3c5eb0"
        );
    }
    assert_eq!(
        connection
            .pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
            .unwrap(),
        6
    );
}
