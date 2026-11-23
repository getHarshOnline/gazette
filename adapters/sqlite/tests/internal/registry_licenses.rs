//! Exact optional content licences share native registry custody, not legal approval.

use super::*;

const COLLECTION_LICENSE: &str = "https://example.invalid/collection-license";

fn licensed_data(license: Option<&str>) -> Data {
    let mut data = Data::new(true);
    let mut nodes: Vec<_> = data.graph.nodes().cloned().collect();
    for node in &mut nodes {
        let declared = if node.id.as_str() == "collection:one" {
            Some(COLLECTION_LICENSE)
        } else {
            license
        };
        node.license = declared.map(|uri| uri.parse().expect("typed content licence"));
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
    .expect("existing content graph with direct licence declarations");
    data
}

fn license(connection: &Connection, content: &str) -> Option<String> {
    connection
        .query_row(
            "SELECT license FROM gazette_registry_content WHERE id=?1",
            [content],
            |row| row.get(0),
        )
        .expect("independent native licence read")
}

#[test]
fn optional_license_replacement_replay_and_absence_preserve_exact_rows_without_inheritance() {
    let (root, mut writer) = fixture();
    let path = root.path().join(journal::DATABASE);
    let mut previous: Option<gazette_core::store::RegistryHead> = None;
    for declared in [
        None,
        Some("https://example.invalid/License?version=1#Terms"),
        Some("https://example.invalid/License?version=2#Terms"),
        None,
    ] {
        let data = licensed_data(declared);
        let model = data.model();
        let documents = data.documents();
        let receipt = writer
            .replace_registry(
                input(&model, &documents),
                RegistryWriteOptions {
                    allow_replace: true,
                },
            )
            .expect("complete native registry replacement");
        let connection = Connection::open(&path).expect("independent native read");
        assert_eq!(
            license(&connection, "content:one"),
            declared.map(str::to_owned)
        );
        assert_eq!(
            license(&connection, "collection:one"),
            Some(COLLECTION_LICENSE.into()),
            "a collection declaration never supplies its child's licence"
        );
        connection.close().expect("close native read");
        assert_eq!(
            Sqlite
                .inspect_registry(&path, options())
                .expect("native row verification")
                .registry,
            Some(receipt.registry)
        );
        if let Some(prior) = previous {
            assert_ne!(prior.rows_sha256, receipt.registry.rows_sha256);
            assert_ne!(prior.semantic_sha256, receipt.registry.semantic_sha256);
            assert_eq!(prior.rows, receipt.registry.rows);
        }
        let repeated = writer
            .replace_registry(
                input(&model, &documents),
                RegistryWriteOptions {
                    allow_replace: true,
                },
            )
            .expect("exact repeated declaration");
        assert_eq!(receipt.registry, repeated.registry);
        assert_eq!(receipt.journal, repeated.journal);
        previous = Some(receipt.registry);
    }
    writer.close().expect("verified writer close");
    assert_eq!(
        Sqlite
            .inspect_registry(&path, options())
            .expect("reopen exact optional rows")
            .registry,
        previous
    );
}

#[test]
fn license_replacement_failure_rolls_back_and_native_tampering_is_detected() {
    let (root, mut writer) = fixture();
    let path = root.path().join(journal::DATABASE);
    let original = licensed_data(Some("https://example.invalid/license/original"));
    let original_model = original.model();
    let original_documents = original.documents();
    let receipt = writer
        .replace_registry(
            input(&original_model, &original_documents),
            RegistryWriteOptions {
                allow_replace: true,
            },
        )
        .expect("initial licensed content");
    writer
        .close()
        .expect("release writer before native fault injection");
    let changed = licensed_data(None);
    let changed_model = changed.model();
    let changed_documents = changed.documents();
    let desired = Desired::prepare(input(&changed_model, &changed_documents), options())
        .expect("qualified complete replacement");
    let mut connection = Connection::open(&path).expect("native transaction seam");
    crate::configure_connection(&connection, options()).expect("native connection policy");
    connection.authorizer(Some(|context: AuthContext<'_>| {
        if matches!(context.action, AuthAction::Insert { table_name } if table_name == "gazette_registry_content") {
            Authorization::Deny
        } else {
            Authorization::Allow
        }
    })).expect("reject content insert after complete-set deletion");
    let transaction = connection.transaction().expect("native transaction");
    assert!(desired.replace(&transaction, options()).is_err());
    transaction
        .rollback()
        .expect("rollback complete replacement");
    connection
        .authorizer(None::<fn(AuthContext<'_>) -> Authorization>)
        .expect("remove native fault");
    assert_eq!(
        license(&connection, "content:one"),
        Some("https://example.invalid/license/original".into())
    );
    assert_eq!(
        verify(&connection, options()).expect("original row identity"),
        Some(receipt.registry)
    );
    connection
        .execute(
            "UPDATE gazette_registry_content SET license=NULL WHERE id='content:one'",
            [],
        )
        .expect("shape-valid native tampering");
    assert!(matches!(
        verify(&connection, options()),
        Err(Error::RegistryVerification)
    ));
}
