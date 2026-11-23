//! Discussion custody follows its site or project, never a placement or remote.

use super::*;
use gazette_core::publication::{Discussion, Project};

fn discussion(repository: &str, category: &str) -> Discussion {
    Discussion {
        capability: "discussion.render/v1".parse().expect("capability key"),
        repository_id: repository.into(),
        category_id: category.into(),
        options: BTreeMap::from([("theme".into(), "preferred_color_scheme".into())]),
    }
}

fn project(discussion: Option<Discussion>) -> Project {
    serde_json::from_value(serde_json::json!({
        "id":"project:one", "repository":"https://example.test/project",
        "lifecycle":"active", "visibility":"public", "availability":"available",
        "discussion":discussion
    }))
    .expect("shared project declaration")
}

fn bind<'a>(data: &'a Data, project: &'a Project, atom: &'a Discussion) -> Projection<'a> {
    let mut model = data.model();
    model.projects.insert(&project.id, project);
    let site = model.sites.get_mut(&data.site).expect("owning site");
    site.repository = Some("https://example.test/site");
    site.discussion = Some(atom);
    model
}

fn stored(connection: &Connection, table: &str) -> Vec<(String, String, String, String)> {
    connection
        .prepare(&format!("SELECT * FROM {table} ORDER BY 1"))
        .expect("native table")
        .query_map([], |row| {
            let options: String = row.get(4)?;
            assert_eq!(
                serde_json::from_str::<BTreeMap<String, String>>(&options).expect("stored options"),
                BTreeMap::from([("theme".into(), "preferred_color_scheme".into())])
            );
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .expect("native query")
        .collect::<rusqlite::Result<_>>()
        .expect("native values")
}

#[test]
fn owner_atoms_reopen_replay_replace_and_remove_without_cross_owner_inheritance() {
    let (root, mut writer) = fixture();
    let path = root.path().join(journal::DATABASE);
    let data = Data::new(false);
    let documents = data.documents();
    let site_atom = discussion("site-repository-id", "site-category-id");
    let project = project(Some(discussion(
        "project-repository-id",
        "project-category-id",
    )));
    let model = bind(&data, &project, &site_atom);
    let receipt = writer
        .replace_registry(
            input(&model, &documents),
            RegistryWriteOptions {
                allow_replace: true,
            },
        )
        .expect("complete owner atoms");
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
    let connection = Connection::open(&path).expect("independent reopened connection");
    assert_eq!(
        stored(&connection, "gazette_registry_site_discussions"),
        vec![(
            "site:one".into(),
            "discussion.render/v1".into(),
            "site-repository-id".into(),
            "site-category-id".into()
        )]
    );
    assert_eq!(
        stored(&connection, "gazette_registry_project_discussions"),
        vec![(
            "project:one".into(),
            "discussion.render/v1".into(),
            "project-repository-id".into(),
            "project-category-id".into()
        )]
    );
    let repository: String = connection
        .query_row(
            "SELECT repository FROM gazette_registry_sites WHERE id='site:one'",
            [],
            |row| row.get(0),
        )
        .expect("explicit public repository");
    assert_eq!(repository, "https://example.test/site");
    assert_eq!(
        verify(&connection, options()).expect("complete readback"),
        Some(receipt.registry)
    );
    connection.close().expect("close reader");

    let mut without_project_atom = project.clone();
    without_project_atom.discussion = None;
    let model = bind(&data, &without_project_atom, &site_atom);
    let replaced = writer
        .replace_registry(
            input(&model, &documents),
            RegistryWriteOptions {
                allow_replace: true,
            },
        )
        .expect("remove project atom only");
    assert_ne!(receipt.registry.rows_sha256, replaced.registry.rows_sha256);
    let connection = Connection::open(&path).expect("fresh readback");
    assert!(stored(&connection, "gazette_registry_project_discussions").is_empty());
    assert_eq!(
        stored(&connection, "gazette_registry_site_discussions").len(),
        1
    );
    assert_eq!(
        verify(&connection, options()).expect("removed project atom readback"),
        Some(replaced.registry)
    );
    connection.close().expect("close reader");

    let model = data.model();
    let cleared = writer
        .replace_registry(
            input(&model, &documents),
            RegistryWriteOptions {
                allow_replace: true,
            },
        )
        .expect("complete catalog removal");
    writer.close().expect("close native writer");
    let connection = Connection::open(&path).expect("reopen after writer close");
    assert!(stored(&connection, "gazette_registry_project_discussions").is_empty());
    assert!(stored(&connection, "gazette_registry_site_discussions").is_empty());
    let repository: Option<String> = connection
        .query_row("SELECT repository FROM gazette_registry_sites", [], |row| {
            row.get(0)
        })
        .expect("removed repository");
    assert_eq!(repository, None);
    assert_eq!(
        verify(&connection, options()).expect("cleared set readback"),
        Some(cleared.registry)
    );
}

#[test]
fn invalid_atoms_missing_repositories_and_project_key_mismatch_refuse_before_replacement() {
    let (root, mut writer) = fixture();
    let path = root.path().join(journal::DATABASE);
    let data = Data::new(false);
    let documents = data.documents();
    let atom = discussion("repository-id", "category-id");
    let project = project(Some(atom.clone()));
    let model = bind(&data, &project, &atom);
    let receipt = writer
        .replace_registry(
            input(&model, &documents),
            RegistryWriteOptions {
                allow_replace: true,
            },
        )
        .expect("original complete set");
    for bad in [
        Discussion {
            repository_id: String::new(),
            ..atom.clone()
        },
        Discussion {
            category_id: "contains space".into(),
            ..atom.clone()
        },
        Discussion {
            options: BTreeMap::from([("bad key".into(), "value".into())]),
            ..atom.clone()
        },
    ] {
        for on_site in [true, false] {
            let mut candidate = project.clone();
            if !on_site {
                candidate.discussion = Some(bad.clone());
            }
            let model = bind(&data, &candidate, if on_site { &bad } else { &atom });
            assert!(matches!(
                writer.replace_registry(
                    input(&model, &documents),
                    RegistryWriteOptions {
                        allow_replace: true
                    }
                ),
                Err(gazette_core::store::RegistryWriteError {
                    cause: Error::RegistryVerification,
                    effect: RegistryEffect::NotAttempted,
                    ..
                })
            ));
        }
    }
    for on_site in [true, false] {
        let mut candidate = project.clone();
        if !on_site {
            candidate.repository = None;
        }
        let mut model = bind(&data, &candidate, &atom);
        if on_site {
            model.sites.get_mut(&data.site).unwrap().repository = None;
        }
        assert!(matches!(
            writer.replace_registry(
                input(&model, &documents),
                RegistryWriteOptions {
                    allow_replace: true
                }
            ),
            Err(gazette_core::store::RegistryWriteError {
                cause: Error::RegistryVerification,
                effect: RegistryEffect::NotAttempted,
                ..
            })
        ));
    }
    let wrong: Key = "project:wrong".parse().unwrap();
    let mut model = bind(&data, &project, &atom);
    model.projects = BTreeMap::from([(&wrong, &project)]);
    assert!(matches!(
        writer.replace_registry(
            input(&model, &documents),
            RegistryWriteOptions {
                allow_replace: true
            }
        ),
        Err(gazette_core::store::RegistryWriteError {
            cause: Error::RegistryVerification,
            effect: RegistryEffect::NotAttempted,
            ..
        })
    ));
    writer.close().expect("close writer");
    let connection = Connection::open(&path).expect("independent preserved set");
    assert_eq!(
        verify(&connection, options()).expect("no candidate damaged previous rows"),
        Some(receipt.registry)
    );
}

#[test]
fn native_discussion_constraints_rollback_and_tamper_detection_preserve_atomic_custody() {
    let (root, mut writer) = fixture();
    let path = root.path().join(journal::DATABASE);
    let data = Data::new(false);
    let documents = data.documents();
    let atom = discussion("repository-id", "category-id");
    let project = project(Some(atom.clone()));
    let model = bind(&data, &project, &atom);
    let receipt = writer
        .replace_registry(
            input(&model, &documents),
            RegistryWriteOptions {
                allow_replace: true,
            },
        )
        .expect("initial set");
    writer.close().expect("release native fault seam");
    let mut connection = Connection::open(&path).expect("independent native seam");
    crate::configure_connection(&connection, options()).expect("native policy");
    for (table, assignment) in [
        ("gazette_registry_site_discussions", "site='project:one'"),
        ("gazette_registry_project_discussions", "project='site:one'"),
        ("gazette_registry_site_discussions", "capability=''"),
        ("gazette_registry_project_discussions", "repository_id=''"),
        ("gazette_registry_site_discussions", "category_id=NULL"),
        ("gazette_registry_project_discussions", "options_json='[]'"),
        (
            "gazette_registry_site_discussions",
            "options_json='malformed'",
        ),
    ] {
        let transaction = connection
            .transaction()
            .expect("isolated invalid native write");
        let result = transaction.execute(&format!("UPDATE {table} SET {assignment}"), []);
        assert!(
            result.is_err() || journal::foreign_keys(&transaction).is_err(),
            "must refuse {table} {assignment}"
        );
        transaction.rollback().expect("preserve prior set");
        assert_eq!(
            verify(&connection, options()).expect("unchanged after constraint failure"),
            Some(receipt.registry)
        );
    }
    let changed = discussion("repository-id", "changed-category");
    let model = bind(&data, &project, &changed);
    let desired =
        Desired::prepare(input(&model, &documents), options()).expect("qualified replacement");
    connection.authorizer(Some(|context: AuthContext<'_>| {
        if matches!(context.action, AuthAction::Insert { table_name } if table_name == "gazette_registry_site_discussions") { Authorization::Deny } else { Authorization::Allow }
    })).expect("native late replacement fault");
    let transaction = connection.transaction().expect("atomic replacement");
    assert!(desired.replace(&transaction, options()).is_err());
    transaction
        .rollback()
        .expect("rollback complete deletions and partial writes");
    connection
        .authorizer(None::<fn(AuthContext<'_>) -> Authorization>)
        .expect("remove fault");
    assert_eq!(
        verify(&connection, options()).expect("prior set survives rollback"),
        Some(receipt.registry)
    );
    assert_eq!(
        stored(&connection, "gazette_registry_site_discussions")[0].3,
        "category-id"
    );
    connection
        .execute(
            "UPDATE gazette_registry_site_discussions SET category_id='changed-category'",
            [],
        )
        .expect("shape-valid tamper");
    assert!(matches!(
        verify(&connection, options()),
        Err(Error::RegistryVerification)
    ));
}
