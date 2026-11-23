//! Real retained publication reconciliation into disposable private `SQLite` stores.

#[path = "registry_store/mcp.rs"]
mod mcp;

#[path = "registry_store/lineage.rs"]
mod lineage;

use std::{os::unix::fs::PermissionsExt, path::Path, process::Output};

use gazette_sdk::store::{self, JournalHeader, ReadOptions};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use tempfile::TempDir;

use super::{Fixture, PAGE, acquire, build, bytes, cli, fs, render};

fn read() -> ReadOptions {
    ReadOptions {
        allow_coordination_files: true,
        ..ReadOptions::default()
    }
}

fn protected(root: &Path) -> store::LifecycleProtectedRoots {
    let roots = vec![root.to_path_buf()];
    store::LifecycleProtectedRoots {
        authored: roots.clone(),
        staged: roots.clone(),
        published: roots.clone(),
        live_journals: roots,
    }
}

#[test]
fn project_names_reconcile_change_and_remove_without_changing_project_identity() {
    let fixture = Fixture::new(&[("project/guide.md", super::PROJECT_DOC.as_bytes())]);
    let root = private_directory();
    let initial = initialize(root.path());
    let mut expected = initial.header;
    for (name, repository) in [
        (
            Some("First Ω"),
            Some("https://code.example.test/Group/First"),
        ),
        (
            Some("Second <&>"),
            Some("http://code.example.test/Group/Second"),
        ),
        (None, None),
    ] {
        let repository_field = repository
            .map(|url| format!("\nrepository={url:?}"))
            .unwrap_or_default();
        let config = fixture.project_config(true).replace(
            "id='project:subject'",
            &match name {
                Some(name) => format!("id='project:subject'\nname={name:?}{repository_field}"),
                None => "id='project:subject'".into(),
            },
        );
        let output = reconcile_cli(&config, root.path(), &expected, None, &[]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let receipt: Value = serde_json::from_slice(&output.stdout).expect("native receipt");
        expected = serde_json::from_value(receipt["journal"].clone()).expect("native coordinates");
        let connection =
            Connection::open_with_flags(&initial.database, OpenFlags::SQLITE_OPEN_READ_ONLY)
                .expect("independent SQL read");
        let (id, stored, state, stored_repository): (String, Option<String>, String, Option<String>) = connection.query_row("SELECT id, name, lifecycle, repository FROM gazette_registry_projects WHERE id='project:subject'", [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))).expect("same native row");
        assert_eq!(id, "project:subject");
        assert_eq!(stored.as_deref(), name);
        assert_eq!(state, "active");
        assert_eq!(stored_repository.as_deref(), repository);
        let unnamed: Option<String> = connection
            .query_row(
                "SELECT name FROM gazette_registry_projects WHERE id='project:native'",
                [],
                |row| row.get(0),
            )
            .expect("unnamed source project");
        assert!(unnamed.is_none());
        let unconfigured: Option<String> = connection
            .query_row(
                "SELECT repository FROM gazette_registry_projects WHERE id='project:native'",
                [],
                |row| row.get(0),
            )
            .expect("source project without repository declaration");
        assert!(unconfigured.is_none());
        connection.close().expect("read closed");
        let inspected = store::inspect_registry(&initial.database, Some(&expected), read())
            .expect("full row identity verification");
        assert_eq!(
            serde_json::to_value(inspected.registry).expect("verified registry"),
            receipt["registry"]
        );
    }
}

fn private_directory() -> TempDir {
    tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .expect("disposable private directory")
}

fn initialize(path: &Path) -> store::InitializationReceipt {
    store::initialize(
        path,
        store::InitializeOptions {
            allow_create: true,
            session: store::SessionOptions {
                allow_local_reservation: true,
                read: read(),
            },
            ..Default::default()
        },
    )
    .expect("production empty-store initialization")
}

fn options() -> build::RegistryReconcileOptions {
    build::RegistryReconcileOptions {
        projection: build::RegistryLimits::default(),
        registry: store::RegistryWriteOptions {
            allow_replace: true,
        },
        writer: store::WriterOptions {
            allow_generation_advance: true,
            session: store::SessionOptions {
                allow_local_reservation: true,
                read: read(),
            },
        },
    }
}

fn reconcile(
    inputs: &build::AcquiredInputs,
    root: &Path,
    expected: JournalHeader,
    options: build::RegistryReconcileOptions,
) -> Result<store::RegistryReceipt, build::RegistryReconcileError> {
    let parsed = inputs
        .parse_publication(Default::default(), &gazette_adapter_markdown::Comrak)
        .expect("retained syntax");
    let resolved = parsed
        .resolve(Default::default())
        .expect("complete source-bound publication");
    build::reconcile_registry(&resolved, root, expected, options)
}

fn reconcile_cli(
    config: &str,
    root: &Path,
    expected: &JournalHeader,
    omit: Option<&str>,
    extra: &[&str],
) -> Output {
    let header = serde_json::to_string(expected).expect("exact expected header");
    let mut args = vec![
        "reconcile-registry",
        root.to_str().expect("root"),
        "--site",
        "site:one",
        "--expected-header",
        &header,
        "--replace-registry",
        "--advance-writer-generation",
        "--reserve-local-coordination",
        "--allow-coordination-files",
    ];
    if let Some(omit) = omit {
        args.retain(|arg| *arg != omit);
    }
    args.extend_from_slice(extra);
    cli(config, &args)
}

fn inspect_cli(path: &Path, expected: &JournalHeader) -> Output {
    let header = serde_json::to_string(expected).expect("expected header");
    cli(
        "",
        &[
            "inspect-registry",
            path.to_str().expect("path"),
            "--expected-header",
            &header,
            "--allow-coordination-files",
        ],
    )
}

fn rows(database: &Path) -> Vec<(String, String, String, String)> {
    let connection = Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .expect("independent read-only native consumer");
    let mut query = connection
        .prepare("SELECT id, site, title, lifecycle FROM gazette_registry_content ORDER BY id")
        .expect("actual registry relation");
    query
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .expect("query")
        .collect::<Result<_, _>>()
        .expect("complete rows")
}

fn unchanged(database: &Path, header: &JournalHeader, expected: &[u8]) {
    let inspection = store::inspect_registry(database, Some(header), read())
        .expect("same complete snapshot after refused effect");
    assert_eq!(inspection.journal.header, *header);
    assert_eq!(fs::read(database).expect("database read-back"), expected);
}

#[test]
fn sdk_and_cli_reconcile_the_complete_model_without_changing_rendered_output() {
    let fixture = Fixture::new(&[]);
    let config = fixture.config();
    let inputs = acquire(&config);
    let root = private_directory();
    let initial = initialize(root.path());
    let empty = store::inspect_registry(&initial.database, Some(&initial.header), read())
        .expect("fresh empty registry");
    assert!(empty.registry.is_none());
    let before = render(&inputs).expect("ordinary retained publication render");
    let first = reconcile(&inputs, root.path(), initial.header, options())
        .expect("complete actual registry replacement");
    assert_eq!(first.journal.journal_id, initial.header.journal_id);
    assert_eq!(first.journal.journal_epoch, initial.header.journal_epoch);
    assert_eq!(first.journal.writer_fence, initial.header.writer_fence + 1);
    assert_eq!(
        first.registry.configuration_sha256,
        inputs.compilation().source_sha256
    );
    assert!(first.registry.rows > 0);
    let parsed = inputs
        .parse_publication(Default::default(), &gazette_adapter_markdown::Comrak)
        .expect("same retained parser");
    let resolved = parsed
        .resolve(Default::default())
        .expect("same shared graph");
    assert_eq!(
        first.registry.semantic_sha256,
        resolved
            .registry(Default::default())
            .expect("shared registry projection")
            .semantic_sha256()
    );
    let inspected = store::inspect_registry(&initial.database, Some(&first.journal), read())
        .expect("independent native row verification");
    assert_eq!(inspected.registry, Some(first.registry));
    let output = inspect_cli(&initial.database, &first.journal);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).expect("CLI inspection"),
        serde_json::to_value(&inspected).expect("SDK inspection")
    );

    let tree = before.site().output();
    let model: Value =
        serde_json::from_slice(bytes(tree, "model.json")).expect("rendered shared model");
    let records = rows(&initial.database);
    assert_eq!(
        records.len(),
        3,
        "private registry includes foreign content rather than only rendered site"
    );
    assert!(
        records
            .iter()
            .any(|row| row.0 == "content:other" && row.1 == "site:other" && row.3 == "archived")
    );
    for (id, page) in model["pages"].as_object().expect("local pages") {
        let row = records
            .iter()
            .find(|row| &row.0 == id)
            .expect("same typed content row");
        assert_eq!(page["content"]["title"], row.2);
        assert_eq!(page["content"]["lifecycle"], row.3);
    }
    assert_eq!(
        render(&inputs)
            .expect("registry cannot mutate retained rendering")
            .site()
            .output()
            .inventory()
            .identity(),
        tree.inventory().identity()
    );

    let output = reconcile_cli(&config, root.path(), &first.journal, None, &[]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value = serde_json::from_slice(&output.stdout).expect("CLI replacement receipt");
    assert_eq!(
        receipt["registry"],
        serde_json::to_value(first.registry).expect("same complete row identity")
    );
    let second: JournalHeader =
        serde_json::from_value(receipt["journal"].clone()).expect("exact returned coordinates");
    assert_eq!(second.writer_fence, first.journal.writer_fence + 1);
    assert_eq!(second.journal_id, first.journal.journal_id);
    assert_eq!(
        store::inspect_registry(&initial.database, Some(&second), read())
            .expect("repeat read-back")
            .registry,
        Some(first.registry)
    );

    let backup_parent = private_directory();
    let backup = store::backup(store::BackupRequest {
        source: &initial.database,
        output_parent: backup_parent.path(),
        protected: protected(root.path()),
        options: store::BackupOptions {
            allow_create: true,
            read: read(),
            ..Default::default()
        },
    })
    .expect("native backup of complete registry");
    store::verify_backup(
        &backup.directory,
        backup.manifest_sha256,
        store::BackupVerifyOptions {
            read: read(),
            ..Default::default()
        },
    )
    .expect("independently pinned backup verification");
    let copy = store::inspect_registry(
        &backup.directory.join(&backup.snapshot.database),
        Some(&second),
        read(),
    )
    .expect("copied rows and head independently agree");
    assert_eq!(copy.registry, Some(first.registry));
}

#[test]
fn changed_documents_and_complete_empty_content_replace_rows_but_keep_journal_coordinates() {
    let fixture = Fixture::new(&[]);
    let inputs = acquire(&fixture.config());
    let root = private_directory();
    let initial = initialize(root.path());
    let first =
        reconcile(&inputs, root.path(), initial.header, options()).expect("initial complete set");
    let changed_page = format!("{PAGE}\nDifferent retained body, unchanged content identity.\n");
    let changed = Fixture::new(&[("one/page.md", changed_page.as_bytes())]);
    let changed_inputs = acquire(&changed.config());
    let second = reconcile(&changed_inputs, root.path(), first.journal, options())
        .expect("exact new configured inputs");
    assert_ne!(
        second.registry.configuration_sha256,
        first.registry.configuration_sha256
    );
    assert_ne!(
        second.registry.rows_sha256, first.registry.rows_sha256,
        "full document identity participates in durable row identity"
    );
    assert_eq!(second.registry.rows, first.registry.rows);
    assert_eq!(rows(&initial.database).len(), 3);
    let empty = Fixture::new(&[
        ("one/assets/input.bin", b"one"),
        ("other/assets/input.bin", b"other"),
    ]);
    let empty_config = empty
        .config()
        .replace("subdirectory='one'", "subdirectory='one/assets'")
        .replace("subdirectory='other'", "subdirectory='other/assets'");
    let empty_inputs = acquire(&empty_config);
    let third = reconcile(&empty_inputs, root.path(), second.journal, options())
        .expect("empty content is a complete replacement, not a skipped operation");
    assert!(rows(&initial.database).is_empty());
    assert!(
        third.registry.rows > 0,
        "configured principals/sites/brands still have rows"
    );
    assert!(third.registry.rows < second.registry.rows);
    assert_eq!(third.journal.journal_id, initial.header.journal_id);
    assert_eq!(third.journal.journal_epoch, initial.header.journal_epoch);
    assert_eq!(third.journal.writer_fence, initial.header.writer_fence + 3);
    assert_eq!(
        store::inspect_registry(&initial.database, Some(&third.journal), read())
            .expect("empty-content head remains explicit")
            .registry,
        Some(third.registry)
    );
}

#[test]
fn preparation_permissions_and_source_overlap_fail_before_writer_generation_advances() {
    let fixture = Fixture::new(&[]);
    let config = fixture.config();
    let inputs = acquire(&config);
    let root = private_directory();
    let initial = initialize(root.path());
    let prior = fs::read(&initial.database).expect("before all refused effects");
    for (omit, code) in [
        ("--replace-registry", "store_registry_permission"),
        ("--advance-writer-generation", "store_writer_permission"),
        (
            "--reserve-local-coordination",
            "store_reservation_permission",
        ),
        (
            "--allow-coordination-files",
            "store_coordination_permission",
        ),
    ] {
        let output = reconcile_cli(&config, root.path(), &initial.header, Some(omit), &[]);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(code),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        unchanged(&initial.database, &initial.header, &prior);
    }
    let mut bounded = options();
    bounded.projection.bytes = 0;
    let error = reconcile(&inputs, root.path(), initial.header, bounded)
        .expect_err("projection fails before acquiring writer");
    assert_eq!(error.code(), "registry_encoding");
    unchanged(&initial.database, &initial.header, &prior);
    let output = reconcile_cli(
        &config,
        root.path(),
        &initial.header,
        None,
        &["--max-registry-bytes", "0"],
    );
    assert!(!output.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).expect("shared projection refusal"),
        serde_json::json!({"phase":"projection", "code":"registry_encoding"})
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("registry_encoding"));
    unchanged(&initial.database, &initial.header, &prior);
    for malformed in [
        "not valid TOML [".to_owned(),
        config.replacen("site='site:one'", "site='site:missing'", 1),
    ] {
        let output = reconcile_cli(&malformed, root.path(), &initial.header, None, &[]);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        unchanged(&initial.database, &initial.header, &prior);
    }
    let conflicting = Fixture::new(&[(
        "one/page.md",
        PAGE.replace("collections=['collection:home']\n", "")
            .replace("site='site:one'", "site='site:other'")
            .as_bytes(),
    )]);
    let output = reconcile_cli(
        &conflicting.config(),
        root.path(),
        &initial.header,
        None,
        &[],
    );
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("publication_home"));
    unchanged(&initial.database, &initial.header, &prior);

    // Source capture precedes this deliberately overlapping disposable store.
    let overlap = fixture.directory.path().join("private-registry");
    fs::create_dir(&overlap).expect("disposable overlapping directory");
    fs::set_permissions(&overlap, fs::Permissions::from_mode(0o700)).expect("private permissions");
    let overlap_initial = initialize(&overlap);
    let overlap_before = fs::read(&overlap_initial.database).expect("overlap baseline");
    let error = reconcile(&inputs, &overlap, overlap_initial.header, options())
        .expect_err("source root is not a registry destination");
    assert!(matches!(
        error,
        build::RegistryReconcileError::Separation {
            writer_acquired: false,
            ..
        }
    ));
    unchanged(
        &overlap_initial.database,
        &overlap_initial.header,
        &overlap_before,
    );

    for role in ["other", "theme"] {
        let primary = Fixture::new(&[]);
        let external = Fixture::new(&[]);
        let original = format!(
            "root={:?},commit='{}',subdirectory='{role}'",
            primary.directory.path().to_str().expect("primary root"),
            primary.commit
        );
        let replacement = format!(
            "root={:?},commit='{}',subdirectory='{role}'",
            external
                .directory
                .path()
                .to_str()
                .expect("separate source root"),
            external.commit
        );
        let config = primary.config();
        assert!(config.contains(&original), "exact {role} source is rebound");
        let inputs = acquire(&config.replace(&original, &replacement));
        let overlap = external.directory.path().join("private-registry");
        fs::create_dir(&overlap).expect("disposable protected-root overlap");
        fs::set_permissions(&overlap, fs::Permissions::from_mode(0o700)).expect("private root");
        let initialized = initialize(&overlap);
        let before = fs::read(&initialized.database).expect("separate protected-source baseline");
        let error = reconcile(&inputs, &overlap, initialized.header, options())
            .expect_err("every captured role protects its native root");
        assert!(
            matches!(
                error,
                build::RegistryReconcileError::Separation {
                    writer_acquired: false,
                    ..
                }
            ),
            "{role}: {error}"
        );
        unchanged(&initialized.database, &initialized.header, &before);
    }
}

#[test]
fn inspection_recomputes_registry_rows_and_refuses_corruption_or_exhausted_bounds() {
    let fixture = Fixture::new(&[]);
    let inputs = acquire(&fixture.config());
    let root = private_directory();
    let initial = initialize(root.path());
    let receipt =
        reconcile(&inputs, root.path(), initial.header, options()).expect("real stored rows");
    let before = fs::read(&initial.database).expect("complete original DB");
    let error = store::inspect_registry(
        &initial.database,
        Some(&receipt.journal),
        ReadOptions {
            storage_bytes: 1,
            ..read()
        },
    )
    .expect_err("explicit native storage cap");
    assert_eq!(error.code(), "store_storage_limit");
    unchanged(&initial.database, &receipt.journal, &before);
    let connection =
        Connection::open(&initial.database).expect("test-only corruption of disposable DB");
    assert_eq!(connection.execute("UPDATE gazette_registry_content SET title='Changed without registry receipt' WHERE id='content:one'", []).expect("one native corruption"), 1);
    connection.close().expect("close contaminating connection");
    let corrupt = fs::read(&initial.database).expect("corrupt baseline");
    let error = store::inspect_registry(&initial.database, Some(&receipt.journal), read())
        .expect_err("head alone is not row verification");
    assert_eq!(error.code(), "store_registry_verification");
    let output = inspect_cli(&initial.database, &receipt.journal);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    assert!(diagnostic.contains("store_registry_verification"));
    assert!(
        !diagnostic.contains("Changed without registry receipt"),
        "stored row contents do not enter diagnostics"
    );
    assert_eq!(
        fs::read(&initial.database).expect("no automatic repair"),
        corrupt
    );
}
