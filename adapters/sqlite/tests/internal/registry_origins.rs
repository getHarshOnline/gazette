//! Explicit project lineage shares complete-set custody; it grants no ownership.

use super::*;
use gazette_core::publication::{Project, ProjectOrigin};

fn project(id: &str, origin: Option<ProjectOrigin>) -> Project {
    serde_json::from_value(serde_json::json!({
        "id": id, "name": "Exact project", "origin": origin,
        "lifecycle": "done", "visibility": "private", "availability": "unavailable"
    }))
    .expect("shared project declaration")
}

fn origin(connection: &Connection) -> Option<(String, Option<String>)> {
    use rusqlite::OptionalExtension;
    connection
        .query_row(
            "SELECT kind, upstream FROM gazette_registry_project_origins WHERE project='project:one'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .expect("independent native origin row")
}

#[test]
fn origin_absence_variants_replacement_replay_and_catalog_removal_remain_exact() {
    let (root, mut writer) = fixture();
    let path = root.path().join(journal::DATABASE);
    let data = Data::new(false);
    let documents = data.documents();
    let upstream = project("project:upstream", None);
    let mut previous: Option<gazette_core::store::RegistryHead> = None;
    for (declared, expected) in [
        (None, None),
        (Some(ProjectOrigin::Original {}), Some(("original", None))),
        (
            Some(ProjectOrigin::SuccessorFork {
                upstream: upstream.id.clone(),
            }),
            Some(("successor-fork", Some("project:upstream"))),
        ),
        (
            Some(ProjectOrigin::ConvenienceMirror {
                upstream: upstream.id.clone(),
            }),
            Some(("convenience-mirror", Some("project:upstream"))),
        ),
        (None, None),
        (Some(ProjectOrigin::Original {}), Some(("original", None))),
    ] {
        let current = project("project:one", declared);
        let mut model = data.model();
        model.projects = BTreeMap::from([(&current.id, &current), (&upstream.id, &upstream)]);
        let receipt = writer
            .replace_registry(
                input(&model, &documents),
                RegistryWriteOptions {
                    allow_replace: true,
                },
            )
            .expect("replace complete lineage catalog");
        let connection = Connection::open(&path).expect("native independent read");
        assert_eq!(
            origin(&connection),
            expected.map(|(kind, upstream)| (kind.into(), upstream.map(str::to_owned)))
        );
        let state: (String, String, String) = connection.query_row("SELECT lifecycle, visibility, availability FROM gazette_registry_projects WHERE id='project:one'", [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).expect("independent state axes");
        assert_eq!(
            state,
            ("done".into(), "private".into(), "unavailable".into())
        );
        connection.close().expect("close independent read");
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
            .expect("exact replay");
        assert_eq!(receipt.registry, replay.registry);
        assert_eq!(receipt.journal, replay.journal);
        previous = Some(receipt.registry);
    }
    let model = data.model();
    let removed = writer
        .replace_registry(
            input(&model, &documents),
            RegistryWriteOptions {
                allow_replace: true,
            },
        )
        .expect("remove both project catalogs");
    writer.close().expect("verified native generation close");
    let connection = Connection::open(&path).expect("reopened native read");
    for table in [
        "gazette_registry_projects",
        "gazette_registry_project_origins",
    ] {
        assert_eq!(
            connection
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| row
                    .get::<_, i64>(0))
                .expect("catalog count"),
            0
        );
    }
    assert_eq!(
        verify(&connection, options()).expect("removed set head"),
        Some(removed.registry)
    );
}

#[test]
fn origin_replacement_rolls_back_and_valid_shape_tampering_is_detected() {
    let (root, mut writer) = fixture();
    let path = root.path().join(journal::DATABASE);
    let data = Data::new(false);
    let documents = data.documents();
    let upstream = project("project:upstream", None);
    let current = project(
        "project:one",
        Some(ProjectOrigin::SuccessorFork {
            upstream: upstream.id.clone(),
        }),
    );
    let changed = project("project:one", Some(ProjectOrigin::Original {}));
    let mut model = data.model();
    model.projects = BTreeMap::from([(&current.id, &current), (&upstream.id, &upstream)]);
    let receipt = writer
        .replace_registry(
            input(&model, &documents),
            RegistryWriteOptions {
                allow_replace: true,
            },
        )
        .expect("initial lineage");
    writer.close().expect("release native fault seam");
    model.projects.insert(&changed.id, &changed);
    let desired =
        Desired::prepare(input(&model, &documents), options()).expect("qualified replacement");
    let mut connection = Connection::open(&path).expect("native fault seam");
    crate::configure_connection(&connection, options()).expect("native policy");
    connection.authorizer(Some(|context: AuthContext<'_>| {
        if matches!(context.action, AuthAction::Insert { table_name } if table_name == "gazette_registry_project_origins") { Authorization::Deny } else { Authorization::Allow }
    })).expect("refuse lineage insert after complete deletions");
    let transaction = connection.transaction().expect("atomic replacement");
    assert!(desired.replace(&transaction, options()).is_err());
    transaction.rollback().expect("restore prior complete rows");
    connection
        .authorizer(None::<fn(AuthContext<'_>) -> Authorization>)
        .expect("remove fault");
    assert_eq!(
        origin(&connection),
        Some(("successor-fork".into(), Some("project:upstream".into())))
    );
    assert_eq!(
        verify(&connection, options()).expect("unchanged prior head"),
        Some(receipt.registry)
    );
    connection
        .execute(
            "UPDATE gazette_registry_project_origins SET kind='convenience-mirror'",
            [],
        )
        .expect("shape-valid native corruption");
    assert!(matches!(
        verify(&connection, options()),
        Err(Error::RegistryVerification)
    ));
}

#[test]
fn native_origin_constraints_reject_wrong_kind_shape_self_and_unknown_endpoints() {
    let (root, mut writer) = fixture();
    let path = root.path().join(journal::DATABASE);
    let data = Data::new(false);
    let documents = data.documents();
    let upstream = project("project:upstream", None);
    let current = project(
        "project:one",
        Some(ProjectOrigin::SuccessorFork {
            upstream: upstream.id.clone(),
        }),
    );
    let mut model = data.model();
    model.projects = BTreeMap::from([(&current.id, &current), (&upstream.id, &upstream)]);
    let receipt = writer
        .replace_registry(
            input(&model, &documents),
            RegistryWriteOptions {
                allow_replace: true,
            },
        )
        .expect("initial valid lineage");
    writer.close().expect("release before native checks");
    let mut connection = Connection::open(&path).expect("native constraint seam");
    crate::configure_connection(&connection, options()).expect("native policy");
    for assignment in [
        "kind='client'",
        "kind='original'",
        "upstream=NULL",
        "upstream='project:one'",
        "upstream='project:missing'",
        "project='project:missing'",
    ] {
        let transaction = connection.transaction().expect("isolated malformed update");
        let result = transaction.execute(
            &format!("UPDATE gazette_registry_project_origins SET {assignment}"),
            [],
        );
        assert!(
            result.is_err() || journal::foreign_keys(&transaction).is_err(),
            "must refuse {assignment}"
        );
        transaction.rollback().expect("preserve original row");
        assert_eq!(
            verify(&connection, options()).expect("original remains exact"),
            Some(receipt.registry)
        );
    }
    for target in ["project:one", "project:missing"] {
        let invalid = project(
            "project:one",
            Some(ProjectOrigin::ConvenienceMirror {
                upstream: target.parse().expect("key"),
            }),
        );
        let mut invalid_model = data.model();
        invalid_model.projects =
            BTreeMap::from([(&invalid.id, &invalid), (&upstream.id, &upstream)]);
        assert!(
            Desired::prepare(input(&invalid_model, &documents), options()).is_err(),
            "qualification must refuse {target}"
        );
    }
}
