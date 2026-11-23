//! Explicit site responsibilities use the complete-set registry transaction.

use super::*;
use gazette_core::publication::{PublicationRole, PublicationRoleBinding};

fn bindings(principal: &Key) -> Vec<PublicationRoleBinding> {
    [
        PublicationRole::CanonAuthority,
        PublicationRole::DataController,
        PublicationRole::SiteOperator,
        PublicationRole::DataProcessor,
    ]
    .into_iter()
    .map(|role| PublicationRoleBinding {
        principal: principal.clone(),
        role,
    })
    .collect()
}

fn read_roles(connection: &Connection) -> Vec<(String, String, String)> {
    connection.prepare("SELECT site,principal,role FROM gazette_registry_site_roles ORDER BY site,principal,role")
        .expect("native role table")
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .expect("native role rows")
        .collect::<rusqlite::Result<_>>()
        .expect("exact role values")
}

fn read_site_selections(connection: &Connection) -> (String, Option<String>, Option<String>) {
    connection.query_row(
        "SELECT publisher,rights_holder,default_author FROM gazette_registry_sites WHERE id='site:one'",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ).expect("exact normalized site selections")
}

#[test]
fn site_rights_and_default_author_are_distinct_normalized_replayable_selections() {
    let (root, mut writer) = fixture();
    let data = Data::new(true);
    let rights = Principal {
        id: "organization:rights".parse().expect("rights principal"),
        kind: PrincipalKind::Organization,
        name: Some("Explicit rights holder".into()),
        website: None,
        same_as: Vec::new(),
    };
    let author = Principal {
        id: "person:default".parse().expect("default person"),
        kind: PrincipalKind::Person,
        name: Some("Explicit default author".into()),
        website: None,
        same_as: Vec::new(),
    };
    let mut model = data.model();
    model.principals.insert(&rights.id, &rights);
    model.principals.insert(&author.id, &author);
    let documents = data.documents();
    let baseline = writer
        .replace_registry(
            input(&model, &documents),
            RegistryWriteOptions {
                allow_replace: true,
            },
        )
        .expect("same catalog without site selections");
    let site = model.sites.get_mut(&data.site).expect("site");
    site.rights_holder = Some(&rights.id);
    site.default_author = Some(&author.id);
    let receipt = writer
        .replace_registry(
            input(&model, &documents),
            RegistryWriteOptions {
                allow_replace: true,
            },
        )
        .expect("explicit distinct site selections");
    assert_eq!(
        receipt.registry.rows, baseline.registry.rows,
        "selection columns do not invent attribution or responsibility rows"
    );
    assert_ne!(receipt.registry.rows_sha256, baseline.registry.rows_sha256);
    assert_ne!(
        receipt.registry.semantic_sha256,
        baseline.registry.semantic_sha256
    );
    let path = root.path().join(journal::DATABASE);
    let connection = Connection::open(&path).expect("native site row read");
    assert_eq!(
        read_site_selections(&connection),
        (
            data.principal.id.to_string(),
            Some(rights.id.to_string()),
            Some(author.id.to_string())
        )
    );
    let kinds: (String, String) = connection.query_row("SELECT rights.kind,author.kind FROM gazette_registry_sites site JOIN gazette_registry_principals rights ON rights.id=site.rights_holder JOIN gazette_registry_principals author ON author.id=site.default_author", [], |row| Ok((row.get(0)?, row.get(1)?))).expect("normalized principal joins");
    assert_eq!(kinds, ("organization".into(), "person".into()));
    assert!(
        read_roles(&connection).is_empty(),
        "no inherited site responsibility"
    );
    let mut statement = connection.prepare("SELECT principal,role FROM gazette_registry_attributions WHERE content='content:one' ORDER BY position").expect("retained authored attribution rows");
    let attributions = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .expect("native attribution read")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("exact authored roles");
    assert_eq!(
        attributions,
        [
            (data.principal.id.to_string(), "rights-holder".into()),
            (data.principal.id.to_string(), "author".into())
        ],
        "site selections neither overwrite explicit author/rights attribution nor append inferred roles"
    );
    drop(statement);
    connection.close().expect("close native row read");
    assert_eq!(
        Sqlite
            .inspect_registry(&path, options())
            .expect("independent row digest verification")
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
        .expect("exact replay");
    assert_eq!(replay.registry, receipt.registry);
    assert_eq!(replay.journal, receipt.journal);
    let site = model.sites.get_mut(&data.site).expect("site");
    site.rights_holder = None;
    site.default_author = None;
    let removed = writer
        .replace_registry(
            input(&model, &documents),
            RegistryWriteOptions {
                allow_replace: true,
            },
        )
        .expect("complete-set explicit removal");
    assert_eq!(
        removed.registry, baseline.registry,
        "absence restores the exact semantic and normalized row identity"
    );
    writer.close().expect("final generation readback");
    let connection = Connection::open(&path).expect("native absence read");
    assert_eq!(
        read_site_selections(&connection),
        (data.principal.id.to_string(), None, None)
    );
    assert_eq!(
        Sqlite
            .inspect_registry(&path, options())
            .expect("independent NULL selection verification")
            .registry,
        Some(removed.registry)
    );
}

#[test]
fn missing_site_rights_or_default_author_refuses_before_any_store_effect() {
    let (root, mut writer) = fixture();
    let path = root.path().join(journal::DATABASE);
    let before = fs::read(&path).expect("exact untouched database");
    let data = Data::new(false);
    let missing: Key = "principal:absent".parse().expect("unresolved selection");
    let documents = data.documents();
    for rights in [true, false] {
        let mut model = data.model();
        let site = model.sites.get_mut(&data.site).expect("site");
        if rights {
            site.rights_holder = Some(&missing);
        } else {
            site.default_author = Some(&missing);
        }
        let error = writer
            .replace_registry(
                input(&model, &documents),
                RegistryWriteOptions {
                    allow_replace: true,
                },
            )
            .expect_err("native foreign-key qualification rejects unresolved principal");
        assert_eq!(error.effect, RegistryEffect::NotAttempted);
        assert_eq!(fs::read(&path).expect("no database mutation"), before);
        assert!(
            Sqlite
                .inspect_registry(&path, options())
                .expect("still pristine registry")
                .registry
                .is_none()
        );
    }
    writer.close().expect("unmodified writer remains healthy");
}

#[test]
fn site_rights_and_default_author_tampering_rejects_independent_registry_readback() {
    for column in ["rights_holder", "default_author"] {
        let (root, mut writer) = fixture();
        let data = Data::new(false);
        let mut model = data.model();
        let site = model.sites.get_mut(&data.site).expect("site");
        site.rights_holder = Some(&data.principal.id);
        site.default_author = Some(&data.principal.id);
        let documents = data.documents();
        let receipt = writer
            .replace_registry(
                input(&model, &documents),
                RegistryWriteOptions {
                    allow_replace: true,
                },
            )
            .expect("qualified exact selections");
        writer
            .close()
            .expect("release exact generation before deliberate corruption");
        let path = root.path().join(journal::DATABASE);
        let connection = Connection::open(&path).expect("native corruption seam");
        assert_eq!(
            verify(&connection, options()).expect("uncorrupted rows"),
            Some(receipt.registry)
        );
        connection
            .execute(
                &format!("UPDATE gazette_registry_sites SET {column}=NULL WHERE id='site:one'"),
                [],
            )
            .expect("FK-valid exact-column tampering");
        assert!(
            matches!(
                verify(&connection, options()),
                Err(Error::RegistryVerification)
            ),
            "row commitment covers {column}"
        );
        connection.close().expect("close corruption seam");
        assert!(
            matches!(
                Sqlite.inspect_registry(&path, options()),
                Err(Error::RegistryVerification)
            ),
            "public native readback refuses changed {column}"
        );
    }
}

#[test]
fn distinct_roles_have_native_rows_identities_and_complete_set_removal() {
    let (root, mut writer) = fixture();
    let data = Data::new(false);
    let other = Principal {
        id: "organization:operator".parse().expect("principal key"),
        kind: PrincipalKind::Organization,
        name: None,
        website: None,
        same_as: Vec::new(),
    };
    let roles = bindings(&other.id);
    let mut model = data.model();
    model.principals.insert(&other.id, &other);
    model.sites.get_mut(&data.site).expect("site").roles = &roles;
    let documents = data.documents();
    let receipt = writer
        .replace_registry(
            input(&model, &documents),
            RegistryWriteOptions {
                allow_replace: true,
            },
        )
        .expect("native role replacement");
    let path = root.path().join(journal::DATABASE);
    assert_eq!(
        Sqlite
            .inspect_registry(&path, options())
            .expect("independent row verification")
            .registry,
        Some(receipt.registry)
    );
    let connection = Connection::open(&path).expect("fixture SQL read");
    assert_eq!(
        read_roles(&connection),
        [
            "canon-authority",
            "data-controller",
            "data-processor",
            "site-operator",
        ]
        .map(|role| (data.site.to_string(), other.id.to_string(), role.into()))
    );
    let publisher: String = connection
        .query_row("SELECT publisher FROM gazette_registry_sites", [], |row| {
            row.get(0)
        })
        .expect("separate publisher");
    assert_eq!(publisher, data.principal.id.as_str());
    connection.close().expect("close fixture read");
    let repeated = writer
        .replace_registry(
            input(&model, &documents),
            RegistryWriteOptions {
                allow_replace: true,
            },
        )
        .expect("exact replay");
    assert_eq!(receipt.journal, repeated.journal);
    assert_eq!(receipt.registry, repeated.registry);
    model.sites.get_mut(&data.site).expect("site").roles = &roles[..1];
    let reduced = writer
        .replace_registry(
            input(&model, &documents),
            RegistryWriteOptions {
                allow_replace: true,
            },
        )
        .expect("complete role replacement");
    assert_ne!(reduced.registry.rows_sha256, receipt.registry.rows_sha256);
    assert_ne!(
        reduced.registry.semantic_sha256,
        receipt.registry.semantic_sha256
    );
    assert_eq!(reduced.registry.rows + 3, receipt.registry.rows);
    model.sites.get_mut(&data.site).expect("site").roles = &[];
    let empty = writer
        .replace_registry(
            input(&model, &documents),
            RegistryWriteOptions {
                allow_replace: true,
            },
        )
        .expect("explicit absence removes all roles");
    assert_eq!(empty.registry.rows + 1, reduced.registry.rows);
    writer.close().expect("final generation verification");
    let connection = Connection::open(&path).expect("final native read");
    assert!(read_roles(&connection).is_empty());
    assert_eq!(
        Sqlite
            .inspect_registry(&path, options())
            .expect("retained empty role set")
            .registry,
        Some(empty.registry)
    );
}

#[test]
fn duplicate_or_unresolved_role_bindings_refuse_before_store_mutation() {
    let (root, mut writer) = fixture();
    let path = root.path().join(journal::DATABASE);
    let before = fs::read(&path).expect("exact original bytes");
    let data = Data::new(false);
    let valid = bindings(&data.principal.id);
    let duplicate = vec![valid[0].clone(), valid[0].clone()];
    let missing = bindings(&"principal:absent".parse().expect("missing key"));
    for roles in [&duplicate, &missing] {
        let mut model = data.model();
        model.sites.get_mut(&data.site).expect("site").roles = roles;
        let documents = data.documents();
        let error = writer
            .replace_registry(
                input(&model, &documents),
                RegistryWriteOptions {
                    allow_replace: true,
                },
            )
            .expect_err("whole role set rejected");
        assert_eq!(error.effect, RegistryEffect::NotAttempted);
        assert_eq!(fs::read(&path).expect("unchanged database"), before);
    }
    writer.close().expect("no damaged writer");
}

#[test]
fn role_insert_failure_rolls_back_previous_rows_and_readback_rejects_tampering() {
    let (root, mut writer) = fixture();
    let path = root.path().join(journal::DATABASE);
    let data = Data::new(false);
    let roles = bindings(&data.principal.id);
    let mut model = data.model();
    model.sites.get_mut(&data.site).expect("site").roles = &roles;
    let documents = data.documents();
    let receipt = writer
        .replace_registry(
            input(&model, &documents),
            RegistryWriteOptions {
                allow_replace: true,
            },
        )
        .expect("initial roles");
    writer.close().expect("release before native fault");
    model.sites.get_mut(&data.site).expect("site").roles = &roles[..1];
    let desired = Desired::prepare(input(&model, &documents), options()).expect("qualified change");
    let mut connection = Connection::open(&path).expect("native fault seam");
    crate::configure_connection(&connection, options()).expect("native connection policy");
    connection.authorizer(Some(|context: AuthContext<'_>| {
        if matches!(context.action, AuthAction::Insert { table_name } if table_name == "gazette_registry_site_roles") { Authorization::Deny } else { Authorization::Allow }
    })).expect("refuse role insert after complete-set deletions");
    let transaction = connection.transaction().expect("native transaction");
    assert!(desired.replace(&transaction, options()).is_err());
    transaction.rollback().expect("preserve original registry");
    connection
        .authorizer(None::<fn(AuthContext<'_>) -> Authorization>)
        .expect("remove native fault");
    assert_eq!(read_roles(&connection).len(), 4);
    assert_eq!(
        verify(&connection, options()).expect("old head remains exact"),
        Some(receipt.registry)
    );
    connection
        .execute(
            "DELETE FROM gazette_registry_site_roles WHERE role='site-operator'",
            [],
        )
        .expect("native corruption");
    assert!(matches!(
        verify(&connection, options()),
        Err(Error::RegistryVerification)
    ));
    assert!(connection.execute("INSERT INTO gazette_registry_site_roles VALUES('site:one','person:one','publisher')", []).is_err(), "publisher is not a responsibility alias");
}
