//! Project SPDX expressions retain private derived-state custody without becoming permission.

use super::*;
use gazette_core::publication::Project;

fn project(license: Option<&str>) -> Project {
    serde_json::from_value(serde_json::json!({
        "id":"project:one",
        "license":license,
        "lifecycle":"active",
        "visibility":"public",
        "availability":"available"
    }))
    .expect("typed project declaration")
}

fn license(connection: &Connection) -> Option<String> {
    connection
        .query_row(
            "SELECT license FROM gazette_registry_projects WHERE id='project:one'",
            [],
            |row| row.get(0),
        )
        .expect("native optional project licence")
}

#[test]
fn project_license_replaces_replays_and_removes_without_content_fallback() {
    let (root, mut writer) = fixture();
    let path = root.path().join(journal::DATABASE);
    let data = Data::new(false);
    let documents = data.documents();
    let mut previous: Option<gazette_core::store::RegistryHead> = None;
    for declared in [
        None,
        Some("MIT OR Apache-2.0"),
        Some("AGPL-3.0-or-later"),
        None,
    ] {
        let project = project(declared);
        let mut model = data.model();
        model.projects.insert(&project.id, &project);
        let receipt = writer
            .replace_registry(
                input(&model, &documents),
                RegistryWriteOptions {
                    allow_replace: true,
                },
            )
            .expect("complete project replacement");
        let connection = Connection::open(&path).expect("independent native read");
        assert_eq!(license(&connection).as_deref(), declared);
        assert_eq!(
            Sqlite
                .inspect_registry(&path, options())
                .expect("verified rows")
                .registry,
            Some(receipt.registry)
        );
        if let Some(prior) = previous {
            assert_ne!(prior.rows_sha256, receipt.registry.rows_sha256);
            assert_ne!(prior.semantic_sha256, receipt.registry.semantic_sha256);
        }
        let replay = writer
            .replace_registry(
                input(&model, &documents),
                RegistryWriteOptions {
                    allow_replace: true,
                },
            )
            .expect("exact project replay");
        assert_eq!(receipt.registry, replay.registry);
        assert_eq!(receipt.journal, replay.journal);
        previous = Some(receipt.registry);
    }
    writer
        .close()
        .expect("final native generation verification");
}

#[test]
fn invalid_expression_is_refused_before_store_and_native_tampering_fails_readback() {
    assert!(
        serde_json::from_value::<Project>(serde_json::json!({
            "id":"project:one",
            "license":"MIT/Apache-2.0",
            "lifecycle":"active",
            "visibility":"public",
            "availability":"available"
        }))
        .is_err()
    );

    let (root, mut writer) = fixture();
    let path = root.path().join(journal::DATABASE);
    let data = Data::new(false);
    let current = project(Some("MIT"));
    let mut model = data.model();
    model.projects.insert(&current.id, &current);
    let documents = data.documents();
    writer
        .replace_registry(
            input(&model, &documents),
            RegistryWriteOptions {
                allow_replace: true,
            },
        )
        .expect("valid project licence");
    writer.close().expect("release writer");
    let connection = Connection::open(&path).expect("native fault seam");
    connection
        .execute(
            "UPDATE gazette_registry_projects SET license='not-an-spdx-expression'",
            [],
        )
        .expect("tamper native row");
    assert!(matches!(
        verify(&connection, options()),
        Err(Error::RegistryVerification)
    ));
}
