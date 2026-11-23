//! Taxonomy catalogs and content classifications share one exact registry replacement.

use super::*;
use gazette_core::publication::{Taxonomy, TaxonomyTerm, content::TaxonomyMembership};

fn taxonomy() -> Taxonomy {
    Taxonomy {
        id: "taxonomy:topic".parse().expect("taxonomy key"),
        label: "Exact topics".into(),
        language: Some("en-US".parse().expect("normalized language")),
        terms: vec![
            TaxonomyTerm {
                id: "term:a".parse().expect("term key"),
                label: "First".into(),
            },
            TaxonomyTerm {
                id: "term:z".parse().expect("term key"),
                label: "Last".into(),
            },
        ],
    }
}

fn classified_data() -> Data {
    let mut data = Data::new(true);
    let mut nodes: Vec<_> = data.graph.nodes().cloned().collect();
    nodes
        .iter_mut()
        .find(|node| node.id.as_str() == "content:one")
        .expect("existing page")
        .taxonomies = vec![TaxonomyMembership {
        taxonomy: "taxonomy:topic".parse().expect("taxonomy key"),
        term: "term:z".parse().expect("term key"),
    }];
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
    .expect("existing graph with exact classification");
    data
}

#[test]
fn catalogs_terms_and_memberships_persist_replay_and_remove_as_one_set() {
    let (root, mut writer) = fixture();
    let path = root.path().join(journal::DATABASE);
    let taxonomy = taxonomy();
    let data = classified_data();
    let mut model = data.model();
    model.taxonomies.insert(&taxonomy.id, &taxonomy);
    let documents = data.documents();

    let receipt = writer
        .replace_registry(
            input(&model, &documents),
            RegistryWriteOptions {
                allow_replace: true,
            },
        )
        .expect("complete classified registry replacement");
    let connection = Connection::open(&path).expect("independent taxonomy read");
    let catalog: (String, String, Option<String>) = connection
        .query_row(
            "SELECT id,label,language FROM gazette_registry_taxonomies",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("exact taxonomy row");
    assert_eq!(
        catalog,
        (
            "taxonomy:topic".into(),
            "Exact topics".into(),
            Some("en-US".into())
        )
    );
    let terms: Vec<(String, String)> = connection
        .prepare("SELECT id,label FROM gazette_registry_terms ORDER BY id")
        .expect("ordered term query")
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .expect("term rows")
        .collect::<rusqlite::Result<_>>()
        .expect("exact terms");
    assert_eq!(
        terms,
        [
            ("term:a".into(), "First".into()),
            ("term:z".into(), "Last".into())
        ]
    );
    let classification: (String, String, String) = connection
        .query_row(
            "SELECT content,taxonomy,term FROM gazette_registry_classifications",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("exact classification row");
    assert_eq!(
        classification,
        (
            "content:one".into(),
            "taxonomy:topic".into(),
            "term:z".into()
        )
    );
    connection.close().expect("close independent taxonomy read");
    assert_eq!(
        Sqlite
            .inspect_registry(&path, options())
            .expect("complete native verification")
            .registry,
        Some(receipt.registry)
    );
    let replay = writer
        .replace_registry(
            input(&model, &documents),
            RegistryWriteOptions {
                allow_replace: true,
            },
        )
        .expect("exact classified replay");
    assert_eq!(replay.registry, receipt.registry);

    let empty = Data::new(false);
    let empty_model = empty.model();
    let empty_documents = empty.documents();
    writer
        .replace_registry(
            input(&empty_model, &empty_documents),
            RegistryWriteOptions {
                allow_replace: true,
            },
        )
        .expect("complete classification removal");
    let connection = Connection::open(&path).expect("independent removal read");
    for table in ["taxonomies", "terms", "classifications"] {
        let rows: i64 = connection
            .query_row(
                &format!("SELECT count(*) FROM gazette_registry_{table}"),
                [],
                |row| row.get(0),
            )
            .expect("removed taxonomy rows");
        assert_eq!(rows, 0, "{table}");
    }
}
