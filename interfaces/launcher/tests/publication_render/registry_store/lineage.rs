//! One configured origin reaches rendered data and actual executable registry custody.

use rusqlite::OptionalExtension;

use super::*;

#[test]
fn configured_lineage_reconciles_through_cli_and_invalid_cycle_preserves_the_store() {
    let fixture = Fixture::new(&[("project/guide.md", super::super::PROJECT_DOC.as_bytes())]);
    let root = private_directory();
    let initial = initialize(root.path());
    let mut expected = initial.header;
    for (declaration, kind, upstream) in [
        ("origin={kind='original'}", Some("original"), None),
        (
            "origin={kind='successor-fork',upstream='project:native'}",
            Some("successor-fork"),
            Some("project:native"),
        ),
        (
            "origin={kind='convenience-mirror',upstream='project:native'}",
            Some("convenience-mirror"),
            Some("project:native"),
        ),
        ("", None, None),
    ] {
        let config = fixture.project_config(true).replace(
            "id='project:subject'",
            &format!("id='project:subject'\n{declaration}"),
        );
        let inputs = acquire(&config);
        let rendered = render(&inputs).expect("same configured semantic catalog");
        let model: Value = serde_json::from_slice(bytes(rendered.site().output(), "model.json"))
            .expect("rendered model");
        let project = &model["pages"]["doc:native"]["projects"][0]["project"];
        assert_eq!(
            project
                .get("origin")
                .and_then(|origin| origin.get("kind"))
                .and_then(Value::as_str),
            kind
        );
        assert_eq!(
            project
                .get("origin")
                .and_then(|origin| origin.get("upstream"))
                .and_then(Value::as_str),
            upstream
        );
        let result = reconcile_cli(&config, root.path(), &expected, None, &[]);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let receipt: Value = serde_json::from_slice(&result.stdout).expect("executable receipt");
        expected =
            serde_json::from_value(receipt["journal"].clone()).expect("exact next coordinates");
        let connection =
            Connection::open_with_flags(&initial.database, OpenFlags::SQLITE_OPEN_READ_ONLY)
                .expect("independent native read");
        let row: Option<(String, Option<String>)> = connection.query_row(
            "SELECT kind, upstream FROM gazette_registry_project_origins WHERE project='project:subject'", [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).optional().expect("stored lineage");
        assert_eq!(
            row,
            kind.map(|kind| (kind.to_owned(), upstream.map(str::to_owned)))
        );
        connection.close().expect("read closed");
        let inspected = store::inspect_registry(&initial.database, Some(&expected), read())
            .expect("native recomputed identity");
        assert_eq!(
            serde_json::to_value(inspected.registry).unwrap(),
            receipt["registry"]
        );
    }
    let before = fs::read(&initial.database).expect("retained database bytes");
    let invalid = fixture
        .project_config(true)
        .replace(
            "id='project:subject'",
            "id='project:subject'\norigin={kind='successor-fork',upstream='project:native'}",
        )
        .replace(
            "id='project:native'",
            "id='project:native'\norigin={kind='convenience-mirror',upstream='project:subject'}",
        );
    let output = reconcile_cli(&invalid, root.path(), &expected, None, &[]);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("workspace_project_lineage_cycle"));
    unchanged(&initial.database, &expected, &before);
}
