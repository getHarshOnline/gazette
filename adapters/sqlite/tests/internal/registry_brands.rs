//! Explicit brand parentage survives private registry reconciliation without role inference.

use super::*;
use gazette_core::publication::{PublicationRole, PublicationRoleBinding, registry::Brand};

fn read_brands(connection: &Connection) -> Vec<(String, String, Option<String>)> {
    connection
        .prepare("SELECT id,name,parent FROM gazette_registry_brands ORDER BY id")
        .expect("normalized brand table")
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .expect("native brand rows")
        .collect::<rusqlite::Result<_>>()
        .expect("exact brand values")
}

#[test]
fn parents_reconcile_with_exact_identity_readback_and_no_inherited_roles() {
    let (root, mut writer) = fixture();
    let data = Data::new(false);
    // Sorted insertion deliberately encounters the child before its parent.
    let child: Key = "brand:a-child".parse().expect("child key");
    let parent: Key = "brand:z-parent".parse().expect("parent key");
    let roles = [PublicationRoleBinding {
        principal: data.principal.id.clone(),
        role: PublicationRole::SiteOperator,
    }];
    let mut model = data.model();
    model.brands.insert(
        &child,
        Brand {
            name: "Child",
            parent: Some(&parent),
        },
    );
    model.brands.insert(
        &parent,
        Brand {
            name: "Parent",
            parent: None,
        },
    );
    let site = model.sites.get_mut(&data.site).expect("explicit site");
    site.brand = Some(&child);
    site.roles = &roles;
    let documents = data.documents();
    let write = RegistryWriteOptions {
        allow_replace: true,
    };
    let receipt = writer
        .replace_registry(input(&model, &documents), write)
        .expect("deferred parent reference permits complete-set insertion");
    let path = root.path().join(journal::DATABASE);
    assert_eq!(
        Sqlite
            .inspect_registry(&path, options())
            .expect("independent readback")
            .registry,
        Some(receipt.registry)
    );
    let connection = Connection::open(&path).expect("native inspection");
    assert_eq!(
        read_brands(&connection),
        vec![
            (child.to_string(), "Child".into(), Some(parent.to_string())),
            (parent.to_string(), "Parent".into(), None),
        ]
    );
    let publisher: String = connection
        .query_row("SELECT publisher FROM gazette_registry_sites", [], |row| {
            row.get(0)
        })
        .expect("independent publisher");
    assert_eq!(publisher, data.principal.id.as_str());
    let roles_count: i64 = connection
        .query_row(
            "SELECT count(*) FROM gazette_registry_site_roles",
            [],
            |row| row.get(0),
        )
        .expect("independent roles");
    assert_eq!(roles_count, 1);
    connection.close().expect("finish inspection");
    let repeated = writer
        .replace_registry(input(&model, &documents), write)
        .expect("same complete set");
    assert_eq!(repeated.registry, receipt.registry);
    assert_eq!(repeated.journal, receipt.journal);

    model.brands.remove(&parent);
    let error = writer
        .replace_registry(input(&model, &documents), write)
        .expect_err("still-referenced parent cannot disappear from the complete set");
    assert_eq!(error.effect, RegistryEffect::NotAttempted);
    assert_eq!(
        Sqlite
            .inspect_registry(&path, options())
            .expect("refusal preserves prior complete set")
            .registry,
        Some(receipt.registry)
    );
    model.brands.insert(
        &parent,
        Brand {
            name: "Parent",
            parent: None,
        },
    );

    // Parentage is explicit: clearing it changes both identities, not row count.
    model.brands.get_mut(&child).expect("child").parent = None;
    let unparented = writer
        .replace_registry(input(&model, &documents), write)
        .expect("clear parent");
    assert_eq!(unparented.registry.rows, receipt.registry.rows);
    assert_ne!(
        unparented.registry.semantic_sha256,
        receipt.registry.semantic_sha256
    );
    assert_ne!(
        unparented.registry.rows_sha256,
        receipt.registry.rows_sha256
    );
    model.brands.remove(&parent);
    let reduced = writer
        .replace_registry(input(&model, &documents), write)
        .expect("remove no-longer-referenced parent");
    assert_eq!(reduced.registry.rows + 1, unparented.registry.rows);
    writer.close().expect("verify reduced complete set");
    let connection = Connection::open(&path).expect("reopen exact registry");
    assert_eq!(
        read_brands(&connection),
        vec![(child.to_string(), "Child".into(), None)]
    );
    connection.close().expect("finish final inspection");
    assert_eq!(
        Sqlite
            .inspect_registry(&path, options())
            .expect("retained reduced identity")
            .registry,
        Some(reduced.registry)
    );
}

#[test]
fn missing_and_self_parents_refuse_before_database_mutation() {
    let (root, mut writer) = fixture();
    let path = root.path().join(journal::DATABASE);
    let before = fs::read(&path).expect("initial bytes");
    let data = Data::new(false);
    let child: Key = "brand:child".parse().expect("child key");
    let missing: Key = "brand:missing".parse().expect("absent key");
    let documents = data.documents();
    for parent in [&missing, &child] {
        let mut model = data.model();
        model.brands.insert(
            &child,
            Brand {
                name: "Child",
                parent: Some(parent),
            },
        );
        let error = writer
            .replace_registry(
                input(&model, &documents),
                RegistryWriteOptions {
                    allow_replace: true,
                },
            )
            .expect_err("parent constraint refuses whole input before write");
        assert_eq!(error.effect, RegistryEffect::NotAttempted);
        assert_eq!(fs::read(&path).expect("unchanged database"), before);
    }
    writer.close().expect("no damaged writer");
}

#[test]
fn parent_tampering_changes_native_identity_and_refuses_inspection() {
    let (root, mut writer) = fixture();
    let data = Data::new(false);
    let child: Key = "brand:child".parse().expect("child key");
    let parent: Key = "brand:parent".parse().expect("parent key");
    let mut model = data.model();
    model.brands.insert(
        &child,
        Brand {
            name: "Child",
            parent: Some(&parent),
        },
    );
    model.brands.insert(
        &parent,
        Brand {
            name: "Parent",
            parent: None,
        },
    );
    let documents = data.documents();
    writer
        .replace_registry(
            input(&model, &documents),
            RegistryWriteOptions {
                allow_replace: true,
            },
        )
        .expect("initial exact parentage");
    writer.close().expect("release native writer before tamper");
    let path = root.path().join(journal::DATABASE);
    let connection = Connection::open(&path).expect("native fault seam");
    connection
        .execute(
            "UPDATE gazette_registry_brands SET parent=NULL WHERE id=?1",
            [child.as_str()],
        )
        .expect("constraint-valid but unrecorded parent change");
    connection.close().expect("retain altered bytes");
    assert!(matches!(
        Sqlite.inspect_registry(&path, options()),
        Err(Error::RegistryVerification)
    ));
}
