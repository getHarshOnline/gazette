//! Explicit public repository metadata shares existing registry custody and read-back.

use super::*;
use gazette_core::publication::Project;

fn project(repository: Option<&str>) -> Project {
    serde_json::from_value(serde_json::json!({
        "id": "project:one", "name": "Explicit project",
        "repository": repository, "lifecycle": "active",
        "visibility": "public", "availability": "available"
    }))
    .expect("shared project declaration")
}

fn repository(connection: &Connection) -> Option<String> {
    connection
        .query_row(
            "SELECT repository FROM gazette_registry_projects",
            [],
            |row| row.get(0),
        )
        .expect("native optional repository column")
}

#[test]
fn project_subject_kind_and_identity_round_trip_and_clear_independently() {
    let (root, mut writer) = fixture();
    let data = Data::new(false);
    let documents = data.documents();
    let path = root.path().join(journal::DATABASE);
    for (kind, identity) in [
        (
            Some(gazette_core::publication::ProjectKind::SoftwareApplication),
            Some("https://entities.test/one#app"),
        ),
        (
            Some(gazette_core::publication::ProjectKind::CreativeWork),
            None,
        ),
        (None, None),
    ] {
        let mut project = project(Some("https://code.test/one"));
        project.kind = kind;
        project.identity = identity.map(str::to_owned);
        let mut model = data.model();
        model.projects.insert(&project.id, &project);
        let receipt = writer
            .replace_registry(
                input(&model, &documents),
                RegistryWriteOptions {
                    allow_replace: true,
                },
            )
            .unwrap();
        let connection = Connection::open(&path).unwrap();
        let actual: (Option<String>, Option<String>, String) = connection.query_row("SELECT kind,identity,repository FROM gazette_registry_projects WHERE id='project:one'", [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap();
        assert_eq!(
            actual.0,
            kind.map(|kind| serde_json::to_value(kind)
                .unwrap()
                .as_str()
                .unwrap()
                .to_owned())
        );
        assert_eq!(actual.1.as_deref(), identity);
        assert_eq!(actual.2, "https://code.test/one");
        assert_eq!(
            Sqlite.inspect_registry(&path, options()).unwrap().registry,
            Some(receipt.registry)
        );
    }
    writer.close().unwrap();
}

#[test]
fn project_repository_preserves_exact_bytes_reconciles_and_removes_without_inference() {
    let (root, mut writer) = fixture();
    let data = Data::new(false);
    let documents = data.documents();
    let path = root.path().join(journal::DATABASE);
    let mut previous: Option<gazette_core::store::RegistryHead> = None;
    for declared in [
        None,
        Some("https://EXAMPLE.test/Owner/Repo.git?view=Source%2Fmain#Readme"),
        Some("https://example.test/another-project"),
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
        assert_eq!(repository(&connection).as_deref(), declared);
        let name: String = connection
            .query_row("SELECT name FROM gazette_registry_projects", [], |row| {
                row.get(0)
            })
            .expect("separate name retained");
        assert_eq!(name, "Explicit project");
        connection.close().expect("close independent read");
        assert_eq!(
            Sqlite
                .inspect_registry(&path, options())
                .expect("verified native rows")
                .registry,
            Some(receipt.registry)
        );
        if let Some(prior) = previous {
            assert_eq!(receipt.registry.rows, prior.rows);
            assert_ne!(receipt.registry.rows_sha256, prior.rows_sha256);
            assert_ne!(receipt.registry.semantic_sha256, prior.semantic_sha256);
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
    assert_eq!(
        Sqlite
            .inspect_registry(&path, options())
            .expect("reopen final registry")
            .registry,
        previous
    );
}

#[test]
fn project_repository_replacement_rolls_back_and_native_tampering_fails_readback() {
    let (root, mut writer) = fixture();
    let path = root.path().join(journal::DATABASE);
    let data = Data::new(false);
    let original = project(Some("https://example.test/original"));
    let changed = project(Some("https://example.test/changed"));
    let mut model = data.model();
    model.projects.insert(&original.id, &original);
    let documents = data.documents();
    let receipt = writer
        .replace_registry(
            input(&model, &documents),
            RegistryWriteOptions {
                allow_replace: true,
            },
        )
        .expect("initial project row");
    writer.close().expect("release before native fault");
    model.projects.insert(&changed.id, &changed);
    let desired = Desired::prepare(input(&model, &documents), options()).expect("qualified change");
    let mut connection = Connection::open(&path).expect("native fault seam");
    crate::configure_connection(&connection, options()).expect("native connection policy");
    connection.authorizer(Some(|context: AuthContext<'_>| {
        if matches!(context.action, AuthAction::Insert { table_name } if table_name == "gazette_registry_projects") {
            Authorization::Deny
        } else {
            Authorization::Allow
        }
    })).expect("refuse replacement after deletions");
    let transaction = connection.transaction().expect("native transaction");
    assert!(desired.replace(&transaction, options()).is_err());
    transaction.rollback().expect("preserve original rows");
    connection
        .authorizer(None::<fn(AuthContext<'_>) -> Authorization>)
        .expect("remove native fault");
    assert_eq!(repository(&connection), original.repository);
    assert_eq!(
        verify(&connection, options()).expect("original head remains exact"),
        Some(receipt.registry)
    );
    connection
        .execute(
            "UPDATE gazette_registry_projects SET repository='https://example.test/tampered'",
            [],
        )
        .expect("native value corruption");
    assert!(matches!(
        verify(&connection, options()),
        Err(Error::RegistryVerification)
    ));
}

#[test]
fn project_resources_round_trip_as_independent_exact_nullable_fields_and_reconcile() {
    let (root, mut writer) = fixture();
    let data = Data::new(false);
    let documents = data.documents();
    let path = root.path().join(journal::DATABASE);
    for resources in [
        Some(
            serde_json::json!({"learn":"content:guide","blogs":"content:articles","thoughts":"content:thoughts","mentions":"content:mentions","github":"https://code.example.test/Team","code_documentation":"https://docs.example.test/Specimen/"}),
        ),
        Some(serde_json::json!({"github":"https://code.example.test/NewTeam"})),
        Some(serde_json::json!({})),
        None,
    ] {
        let mut value = serde_json::to_value(project(None)).expect("typed project");
        if let Some(resources) = &resources {
            value["resources"] = resources.clone();
        }
        let project: Project = serde_json::from_value(value).expect("optional native resources");
        let mut model = data.model();
        model.projects.insert(&project.id, &project);
        let receipt = writer
            .replace_registry(
                input(&model, &documents),
                RegistryWriteOptions {
                    allow_replace: true,
                },
            )
            .expect("replace complete registry");
        let connection = Connection::open(&path).expect("independent native read");
        let count: i64 = connection
            .query_row(
                "SELECT count(*) FROM gazette_registry_project_resources",
                [],
                |row| row.get(0),
            )
            .expect("resource row count");
        assert_eq!(count, i64::from(resources.is_some()));
        if let Some(resources) = &resources {
            let actual: [Option<String>; 6] = connection
                .query_row(
                    "SELECT learn,github,blogs,thoughts,mentions,code_documentation FROM gazette_registry_project_resources",
                    [],
                    |row| Ok([row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?]),
                )
                .expect("six independently optional fields");
            let field = |name: &str| {
                resources
                    .get(name)
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            };
            assert_eq!(
                actual,
                [
                    field("learn"),
                    field("github"),
                    field("blogs"),
                    field("thoughts"),
                    field("mentions"),
                    field("code_documentation")
                ]
            );
        }
        assert_eq!(
            verify(&connection, options()).expect("independent complete row read-back"),
            Some(receipt.registry)
        );
    }
    writer.close().expect("final native generation");
}
