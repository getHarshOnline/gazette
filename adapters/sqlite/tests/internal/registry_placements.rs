//! Project publication placement roles persist independently from routes and lineage.

use super::*;
use gazette_core::publication::{
    Project, ProjectPlacementRole,
    registry::{Placement, Site},
};

fn project() -> Project {
    serde_json::from_value(serde_json::json!({
        "id": "project:one", "lifecycle": "active",
        "visibility": "public", "availability": "available"
    }))
    .expect("project")
}

fn site<'a>(publisher: &'a Key) -> Site<'a> {
    Site {
        publisher,
        rights_holder: None,
        default_author: None,
        roles: &[],
        deployable: false,
        brand: None,
        public_base_url: None,
        repository: None,
        discussion: None,
    }
}

#[test]
fn primary_and_mirror_roles_replace_replay_remove_and_fail_native_tampering() {
    let (root, mut writer) = fixture();
    let path = root.path().join(journal::DATABASE);
    let data = Data::new(false);
    let project = project();
    let mirror_site: Key = "site:mirror".parse().expect("site");
    let documents = data.documents();
    let mut model = data.model();
    model.projects.insert(&project.id, &project);
    model.sites.insert(&mirror_site, site(&data.principal.id));
    model.placements = vec![
        Placement {
            site: &data.site,
            project: &project.id,
            role: ProjectPlacementRole::Primary,
            render: true,
            discoverable: true,
        },
        Placement {
            site: &mirror_site,
            project: &project.id,
            role: ProjectPlacementRole::Mirror,
            render: false,
            discoverable: false,
        },
    ];

    let first = writer
        .replace_registry(
            input(&model, &documents),
            RegistryWriteOptions {
                allow_replace: true,
            },
        )
        .expect("exact placement roles");
    let connection = Connection::open(&path).expect("native read");
    let mut statement = connection
        .prepare(
            "SELECT site, role, render, discoverable FROM gazette_registry_placements ORDER BY site",
        )
        .expect("placement rows");
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })
        .expect("rows")
        .collect::<Result<Vec<_>, _>>()
        .expect("values");
    assert_eq!(
        rows,
        [
            ("site:mirror".into(), "mirror".into(), 0, 0),
            ("site:one".into(), "primary".into(), 1, 1),
        ]
    );
    drop(statement);
    connection.close().expect("close native read");

    let replay = writer
        .replace_registry(
            input(&model, &documents),
            RegistryWriteOptions {
                allow_replace: true,
            },
        )
        .expect("exact replay");
    assert_eq!(first.registry, replay.registry);
    assert_eq!(first.journal, replay.journal);

    model.placements.pop();
    let removed = writer
        .replace_registry(
            input(&model, &documents),
            RegistryWriteOptions {
                allow_replace: true,
            },
        )
        .expect("complete-set mirror removal");
    assert_ne!(first.registry.rows_sha256, removed.registry.rows_sha256);
    writer.close().expect("verified close");

    let connection = Connection::open(&path).expect("tamper seam");
    assert!(
        connection
            .execute(
                "UPDATE gazette_registry_placements SET role='canonical'",
                [],
            )
            .is_err(),
        "closed native role vocabulary"
    );
    connection
        .execute("UPDATE gazette_registry_placements SET role='mirror'", [])
        .expect("valid-shape semantic corruption");
    assert!(matches!(
        verify(&connection, options()),
        Err(Error::RegistryVerification)
    ));
}
