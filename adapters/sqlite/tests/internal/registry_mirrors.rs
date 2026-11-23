//! Redirect presentations retain canonical identity through real registry transactions.

use super::*;
use gazette_core::{
    publication::registry::{Mirror, Route},
    source::EntryPath,
};

fn route<'a>(site: &'a Key, output: &'a EntryPath) -> Route<'a> {
    Route {
        url: "https://one.example/draft/",
        canonical: None,
        canonical_url: "https://one.example/draft/",
        output,
        aliases: vec![],
        mirrors: vec![Mirror {
            site,
            url: "https://two.example/shared/",
            output,
        }],
    }
}

fn add_site<'a>(model: &mut Projection<'a>, data: &'a Data, site: &'a Key) {
    model.sites.insert(
        site,
        Site {
            publisher: &data.principal.id,
            rights_holder: None,
            default_author: None,
            roles: &[],
            deployable: false,
            brand: None,
            public_base_url: None,
            repository: None,
            discussion: None,
        },
    );
}

fn read_mirrors(connection: &Connection) -> Vec<(String, String, String, String, String)> {
    connection.prepare("SELECT m.site,m.output,m.content,c.site,r.canonical_url FROM gazette_registry_mirrors m JOIN gazette_registry_routes r ON r.content=m.content JOIN gazette_registry_content c ON c.id=r.content ORDER BY m.site,m.output")
        .expect("native mirror canonical join")
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)))
        .expect("mirror rows")
        .collect::<rusqlite::Result<_>>()
        .expect("exact joined values")
}

#[test]
fn mirrors_reconcile_read_back_and_remove_without_copying_content() {
    let (root, mut writer) = fixture();
    let data = Data::new(true);
    let site: Key = "site:two".parse().expect("site");
    let content: Key = "content:one".parse().expect("content");
    let output: EntryPath = "draft/index.html".parse().expect("output");
    let mut model = data.model();
    add_site(&mut model, &data, &site);
    model.routes.insert(&content, route(&site, &output));
    let documents = data.documents();
    let receipt = writer
        .replace_registry(
            input(&model, &documents),
            RegistryWriteOptions {
                allow_replace: true,
            },
        )
        .expect("mirror reconciliation");
    let path = root.path().join(journal::DATABASE);
    assert_eq!(
        Sqlite
            .inspect_registry(&path, options())
            .expect("independent readback")
            .registry,
        Some(receipt.registry)
    );
    let connection = Connection::open(&path).expect("native rows");
    assert_eq!(
        read_mirrors(&connection),
        vec![(
            site.to_string(),
            output.to_string(),
            content.to_string(),
            data.site.to_string(),
            "https://one.example/draft/".into()
        )]
    );
    let content_count: i64 = connection
        .query_row("SELECT count(*) FROM gazette_registry_content", [], |row| {
            row.get(0)
        })
        .expect("native content count");
    assert_eq!(
        content_count, 2,
        "mirror does not create another authored object"
    );
    connection.close().expect("close read");
    let repeat = writer
        .replace_registry(
            input(&model, &documents),
            RegistryWriteOptions {
                allow_replace: true,
            },
        )
        .expect("idempotent mirror replay");
    assert_eq!(receipt.registry, repeat.registry);
    assert_eq!(receipt.journal, repeat.journal);
    model
        .routes
        .get_mut(&content)
        .expect("route")
        .mirrors
        .clear();
    let removed = writer
        .replace_registry(
            input(&model, &documents),
            RegistryWriteOptions {
                allow_replace: true,
            },
        )
        .expect("complete set removal");
    assert_eq!(removed.registry.rows + 1, receipt.registry.rows);
    assert_ne!(
        removed.registry.semantic_sha256,
        receipt.registry.semantic_sha256
    );
    assert_ne!(removed.registry.rows_sha256, receipt.registry.rows_sha256);
    assert_eq!(removed.journal, receipt.journal);
    writer.close().expect("native close");
    assert!(read_mirrors(&Connection::open(&path).expect("final read")).is_empty());
    assert_eq!(
        Sqlite
            .inspect_registry(&path, options())
            .expect("final independent read")
            .registry,
        Some(removed.registry)
    );
}

#[test]
fn invalid_mirror_targets_and_duplicate_outputs_preserve_existing_store() {
    let (root, mut writer) = fixture();
    let data = Data::new(true);
    let site: Key = "site:two".parse().expect("site");
    let absent: Key = "site:absent".parse().expect("missing site");
    let content: Key = "content:one".parse().expect("content");
    let output: EntryPath = "draft/index.html".parse().expect("output");
    let documents = data.documents();
    let path = root.path().join(journal::DATABASE);
    let before = fs::read(&path).expect("baseline bytes");
    for target in [&data.site, &absent] {
        let mut model = data.model();
        model.routes.insert(&content, route(target, &output));
        let failure = writer
            .replace_registry(
                input(&model, &documents),
                RegistryWriteOptions {
                    allow_replace: true,
                },
            )
            .expect_err("same-home or unresolved target rejected");
        assert_eq!(failure.effect, RegistryEffect::NotAttempted);
        assert_eq!(fs::read(&path).expect("unchanged bytes"), before);
    }
    let mut model = data.model();
    add_site(&mut model, &data, &site);
    let mut duplicate = route(&site, &output);
    duplicate.mirrors.push(Mirror {
        site: &site,
        output: &output,
        url: "https://two.example/other/",
    });
    model.routes.insert(&content, duplicate);
    let failure = writer
        .replace_registry(
            input(&model, &documents),
            RegistryWriteOptions {
                allow_replace: true,
            },
        )
        .expect_err("duplicate site/output rejected by native constraint");
    assert_eq!(failure.effect, RegistryEffect::NotAttempted);
    assert_eq!(fs::read(&path).expect("unchanged bytes"), before);
    writer.close().expect("healthy writer");
}

#[test]
fn mirror_insert_failure_rolls_back_and_readback_detects_tampering() {
    let (root, mut writer) = fixture();
    let data = Data::new(true);
    let site: Key = "site:two".parse().expect("site");
    let content: Key = "content:one".parse().expect("content");
    let output: EntryPath = "draft/index.html".parse().expect("output");
    let mut model = data.model();
    add_site(&mut model, &data, &site);
    model.routes.insert(&content, route(&site, &output));
    let documents = data.documents();
    let receipt = writer
        .replace_registry(
            input(&model, &documents),
            RegistryWriteOptions {
                allow_replace: true,
            },
        )
        .expect("initial mirrors");
    writer.close().expect("release native writer");
    let desired =
        Desired::prepare(input(&model, &documents), options()).expect("qualified desired rows");
    let path = root.path().join(journal::DATABASE);
    let mut connection = Connection::open(&path).expect("native fault seam");
    crate::configure_connection(&connection, options()).expect("connection constraints");
    connection.authorizer(Some(|context: AuthContext<'_>| {
        if matches!(context.action, AuthAction::Insert { table_name } if table_name == "gazette_registry_mirrors") { Authorization::Deny } else { Authorization::Allow }
    })).expect("fail insert after replacement deletion");
    let transaction = connection.transaction().expect("native transaction");
    assert!(desired.replace(&transaction, options()).is_err());
    transaction.rollback().expect("native rollback");
    connection
        .authorizer(None::<fn(AuthContext<'_>) -> Authorization>)
        .expect("remove failure");
    assert_eq!(read_mirrors(&connection).len(), 1);
    assert_eq!(
        crate::registry::verify(&connection, options()).expect("retained row verification"),
        Some(receipt.registry)
    );
    connection
        .execute(
            "UPDATE gazette_registry_mirrors SET url='https://two.example/tampered/'",
            [],
        )
        .expect("deliberate fixture corruption");
    assert!(crate::registry::verify(&connection, options()).is_err());
}
