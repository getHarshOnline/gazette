//! Exercise the actual build-time producer without Git, clocks or runtime state.

#[path = "../build_support/repository_source.rs"]
mod repository_source;

use std::{fs, path::Path};

const MEMBERS: [&str; 7] = [
    "crates/gazette-application",
    "crates/gazette-compiler",
    "crates/gazette-core",
    "crates/gazette-ports",
    "adapters/raster",
    "sdk/rust",
    "interfaces/cli",
];

fn put(root: &Path, path: &str, bytes: &[u8]) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().expect("parent")).expect("fixture directories");
    fs::write(path, bytes).expect("fixture bytes");
}

fn fixture(reverse: bool) -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("isolated source workspace");
    let mut paths = vec![
        "Cargo.toml".to_owned(),
        "Cargo.lock".to_owned(),
        "rust-toolchain.toml".to_owned(),
        "crates/gazette-application/build.rs".to_owned(),
        "crates/gazette-application/build_support/repository_source.rs".to_owned(),
    ];
    for member in MEMBERS {
        paths.push(format!("{member}/Cargo.toml"));
        paths.push(format!("{member}/src/lib.rs"));
    }
    paths.push("crates/gazette-compiler/src/notebook/schema.json".into());
    if reverse {
        paths.reverse();
    }
    for path in paths {
        let contents = if path.ends_with("/Cargo.toml") {
            "[package]\nname = 'fixture'\n"
        } else {
            &path
        };
        put(root.path(), &path, contents.as_bytes());
    }
    put(
        root.path(),
        "Cargo.toml",
        format!("[workspace]\nmembers = {MEMBERS:?}\n").as_bytes(),
    );
    root
}

#[test]
fn root_location_creation_order_and_excluded_runtime_state_do_not_change_source_identity() {
    let first = fixture(false);
    let second = fixture(true);
    let baseline = repository_source::snapshot(first.path()).expect("source observation");
    assert_eq!(
        baseline.digest,
        repository_source::snapshot(second.path())
            .expect("relocated source")
            .digest
    );
    for path in [
        "target/generated.rs",
        ".git/HEAD",
        ".codegraph/state",
        "docs/note.md",
        "crates/gazette-core/tests/test.rs",
    ] {
        put(first.path(), path, b"unrelated runtime or tests");
    }
    let after = repository_source::snapshot(first.path()).expect("same selected source");
    assert_eq!(baseline.digest, after.digest);
    assert_eq!(baseline.bytes, after.bytes);
    assert_eq!(baseline.files, after.files);
    assert!(
        after
            .watched
            .contains(&first.path().join("crates/gazette-core/src")),
        "Cargo must watch source discovery, not only old files"
    );
    assert!(!after.watched.contains(&first.path().join("target")));
}

#[test]
fn payloads_manifests_lock_and_generator_inputs_are_bound_without_rust_only_filtering() {
    for path in [
        "Cargo.toml",
        "Cargo.lock",
        "rust-toolchain.toml",
        "crates/gazette-core/Cargo.toml",
        "crates/gazette-core/src/lib.rs",
        "crates/gazette-compiler/src/notebook/schema.json",
        "crates/gazette-application/build.rs",
        "crates/gazette-application/build_support/repository_source.rs",
        "adapters/raster/src/lib.rs",
        "sdk/rust/src/lib.rs",
        "interfaces/cli/src/lib.rs",
    ] {
        let root = fixture(false);
        let before = repository_source::snapshot(root.path()).expect("baseline");
        let mut changed = fs::read(root.path().join(path)).expect("fixture input");
        changed.extend_from_slice(b"\n# changed exact input\n");
        put(root.path(), path, &changed);
        assert_ne!(
            before.digest,
            repository_source::snapshot(root.path())
                .expect("changed input")
                .digest,
            "{path}"
        );
    }
}

#[test]
fn native_adapter_bytes_and_new_manifest_members_share_the_existing_identity() {
    let root = fixture(false);
    let native = include_bytes!("../../../adapters/raster/src/jpeg_metadata.rs");
    put(root.path(), "adapters/raster/src/jpeg_metadata.rs", native);
    let before = repository_source::snapshot(root.path()).expect("native adapter source");
    let mut changed = native.to_vec();
    changed.extend_from_slice(b"\n// changed adapter input\n");
    put(
        root.path(),
        "adapters/raster/src/jpeg_metadata.rs",
        &changed,
    );
    assert_ne!(
        before.digest,
        repository_source::snapshot(root.path())
            .expect("changed adapter")
            .digest
    );

    let manifest = format!(
        "[workspace]\nmembers = {:?}\n",
        [&MEMBERS[..], &["adapters/new-provider"]].concat()
    );
    put(root.path(), "Cargo.toml", manifest.as_bytes());
    assert!(
        repository_source::snapshot(root.path()).is_err(),
        "a declared missing provider cannot disappear from the identity"
    );
    put(
        root.path(),
        "adapters/new-provider/Cargo.toml",
        b"[package]\nname = 'new-provider'\n",
    );
    put(
        root.path(),
        "adapters/new-provider/src/lib.rs",
        b"pub fn provider() {}\n",
    );
    let added = repository_source::snapshot(root.path()).expect("declared complete provider");
    assert!(
        added
            .watched
            .contains(&root.path().join("adapters/new-provider/src/lib.rs"))
    );
    put(
        root.path(),
        "adapters/new-provider/build.rs",
        b"fn main() {}\n",
    );
    put(
        root.path(),
        "adapters/new-provider/build_support/input.txt",
        b"native build input",
    );
    assert_ne!(
        added.digest,
        repository_source::snapshot(root.path())
            .expect("new build inputs")
            .digest
    );
}

#[test]
fn unsupported_or_ambiguous_workspace_membership_is_not_an_incomplete_success() {
    for members in [
        "[]",
        "[1]",
        "['../outside']",
        "['/outside']",
        "['adapters/*']",
        "['adapters/raster', 'adapters/raster']",
        "['adapters/./raster']",
    ] {
        let root = fixture(false);
        put(
            root.path(),
            "Cargo.toml",
            format!("[workspace]\nmembers = {members}\n").as_bytes(),
        );
        assert!(
            repository_source::snapshot(root.path()).is_err(),
            "{members}"
        );
    }
}

#[test]
fn component_declared_schema_is_required_and_changes_the_same_source_identity() {
    let root = fixture(false);
    let manifest =
        "[package]\nname = 'fixture'\n[package.metadata.gazette]\nsource-inputs = ['schema']\n";
    put(
        root.path(),
        "adapters/raster/Cargo.toml",
        manifest.as_bytes(),
    );
    assert!(
        repository_source::snapshot(root.path()).is_err(),
        "missing declared resource"
    );
    let sql = include_bytes!("../../../adapters/sqlite/schema/0001-journal.sql");
    put(root.path(), "adapters/raster/schema/initial.sql", sql);
    let before = repository_source::snapshot(root.path()).expect("exact native schema bytes");
    let mut changed = sql.to_vec();
    changed.extend_from_slice(b"\n-- changed schema\n");
    put(root.path(), "adapters/raster/schema/initial.sql", &changed);
    assert_ne!(
        before.digest,
        repository_source::snapshot(root.path())
            .expect("changed schema")
            .digest
    );
    for inputs in [
        "1",
        "[1]",
        "['../outside']",
        "['/outside']",
        "['schema', 'schema']",
    ] {
        put(
            root.path(),
            "adapters/raster/Cargo.toml",
            format!("[package.metadata.gazette]\nsource-inputs = {inputs}\n").as_bytes(),
        );
        assert!(
            repository_source::snapshot(root.path()).is_err(),
            "{inputs}"
        );
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("schema", root.path().join("adapters/raster/linked"))
            .expect("fixture link");
        put(
            root.path(),
            "adapters/raster/Cargo.toml",
            b"[package.metadata.gazette]\nsource-inputs = ['linked/initial.sql']\n",
        );
        assert!(
            repository_source::snapshot(root.path()).is_err(),
            "resource ancestors cannot be links"
        );
    }
}

#[test]
fn additions_removals_and_paths_change_identity_and_missing_mandatory_roots_fail() {
    let root = fixture(false);
    let before = repository_source::snapshot(root.path())
        .expect("baseline")
        .digest;
    let extra = "crates/gazette-core/src/new.rs";
    put(root.path(), extra, b"new code");
    let added = repository_source::snapshot(root.path())
        .expect("added file")
        .digest;
    assert_ne!(before, added);
    fs::rename(
        root.path().join(extra),
        root.path().join("crates/gazette-core/src/renamed.rs"),
    )
    .expect("fixture rename");
    assert_ne!(
        added,
        repository_source::snapshot(root.path())
            .expect("renamed input")
            .digest
    );
    fs::remove_file(root.path().join("crates/gazette-core/src/renamed.rs"))
        .expect("remove fixture addition");
    assert_eq!(
        before,
        repository_source::snapshot(root.path())
            .expect("restored source")
            .digest
    );
    fs::remove_file(root.path().join("Cargo.lock")).expect("remove required fixture lock");
    assert!(repository_source::snapshot(root.path()).is_err());

    let root = fixture(false);
    fs::remove_file(root.path().join("rust-toolchain.toml"))
        .expect("remove required fixture toolchain declaration");
    assert!(repository_source::snapshot(root.path()).is_err());
}

#[cfg(unix)]
#[test]
fn source_symlinks_are_not_followed() {
    let root = fixture(false);
    std::os::unix::fs::symlink(
        "lib.rs",
        root.path().join("crates/gazette-core/src/link.rs"),
    )
    .expect("fixture symlink");
    assert!(repository_source::snapshot(root.path()).is_err());

    let root = fixture(false);
    std::os::unix::fs::symlink("raster", root.path().join("adapters/linked"))
        .expect("fixture member symlink");
    put(
        root.path(),
        "Cargo.toml",
        b"[workspace]\nmembers = ['adapters/linked']\n",
    );
    assert!(
        repository_source::snapshot(root.path()).is_err(),
        "member ancestors cannot escape native custody"
    );
}

#[test]
fn mandatory_input_kinds_and_file_size_are_enforced() {
    let root = fixture(false);
    let lock = root.path().join("Cargo.lock");
    fs::remove_file(&lock).expect("remove fixture lock");
    fs::create_dir(&lock).expect("wrong-kind fixture lock");
    assert!(repository_source::snapshot(root.path()).is_err());

    let root = fixture(false);
    let toolchain = root.path().join("rust-toolchain.toml");
    fs::remove_file(&toolchain).expect("remove fixture toolchain declaration");
    fs::create_dir(&toolchain).expect("wrong-kind fixture toolchain declaration");
    assert!(repository_source::snapshot(root.path()).is_err());

    let root = fixture(false);
    let source = root.path().join("crates/gazette-core/src");
    fs::remove_file(source.join("lib.rs")).expect("remove fixture source");
    fs::remove_dir(&source).expect("remove empty fixture source directory");
    fs::write(source, b"not a source directory").expect("wrong-kind fixture source");
    assert!(repository_source::snapshot(root.path()).is_err());

    let root = fixture(false);
    fs::File::create(root.path().join("crates/gazette-core/src/oversized.rs"))
        .expect("sparse fixture")
        .set_len((64 << 20) + 1)
        .expect("over-limit sparse size");
    assert!(repository_source::snapshot(root.path()).is_err());
}

#[test]
fn embedded_repository_source_provenance_has_no_runtime_paths_or_unknown_identity() {
    let identity = gazette_application::build::repository_source_provenance();
    let value = serde_json::to_value(identity).expect("source evidence");
    assert_eq!(value["profile"], repository_source::PROFILE);
    assert_eq!(value["sha256"].as_str().expect("digest").len(), 64);
    assert!(value["files"].as_u64().expect("file count") > 4);
    assert!(value["bytes"].as_u64().expect("source size") > 0);
    assert_eq!(value.as_object().expect("closed evidence").len(), 4);
    assert_eq!(
        identity,
        gazette_application::build::repository_source_provenance()
    );
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("independent Gazette workspace");
    let observed = repository_source::snapshot(workspace).expect("current observed source closure");
    for payload in [
        "sdk/js/analytics/index.mjs",
        "sdk/js/analytics/index.d.ts",
        "sdk/js/webmcp/index.mjs",
        "sdk/js/webmcp/data.mjs",
        "sdk/js/webmcp/index.d.ts",
    ] {
        assert!(
            observed.watched.contains(&workspace.join(payload)),
            "distributable browser payload is part of the source identity: {payload}"
        );
    }
    let digest = gazette_core::source::Sha256Digest::from(observed.digest);
    assert_eq!(value["sha256"], serde_json::json!(digest));
    assert_eq!(value["files"], serde_json::json!(observed.files));
    assert_eq!(value["bytes"], serde_json::json!(observed.bytes));
}

#[test]
fn embedded_runtime_build_profile_is_portable_and_closed() {
    let coordinate = gazette_application::build::runtime_build_profile();
    let value = serde_json::to_value(&coordinate).expect("runtime build profile");
    assert_eq!(value["engine_version"], env!("CARGO_PKG_VERSION"));
    assert!(!value["build_profile"].as_str().unwrap().is_empty());
    assert!(!value["compilation_target"].as_str().unwrap().is_empty());
    assert_eq!(value.as_object().expect("closed coordinate").len(), 3);
    for field in ["engine_version", "build_profile", "compilation_target"] {
        let value = value[field].as_str().unwrap();
        assert!(value.len() <= 256 && value.bytes().all(|byte| byte.is_ascii_graphic()));
        assert!(!value.contains('/') && !value.contains('\\'));
    }
    assert_eq!(
        coordinate,
        gazette_application::build::runtime_build_profile()
    );
    let version = gazette_application::build::runtime_version_coordinate();
    assert!(version.starts_with(env!("CARGO_PKG_VERSION")));
    assert!(version.contains(&format!("source {}", value_of_source_digest())));
    assert!(version.contains(&format!("profile {}", coordinate.build_profile)));
    assert!(version.contains(&format!("target {}", coordinate.compilation_target)));
}

fn value_of_source_digest() -> String {
    serde_json::to_value(gazette_application::build::repository_source_provenance())
        .expect("source provenance")["sha256"]
        .as_str()
        .expect("source digest")
        .to_owned()
}

#[test]
fn runtime_build_coordinate_rejects_missing_or_non_portable_values() {
    for value in ["", "contains space", "contains/path", "contains\\path"] {
        assert!(repository_source::validate_runtime_coordinate(value).is_err());
    }
    assert!(repository_source::validate_runtime_coordinate("release-lto").is_ok());
}

#[test]
fn process_manifest_binds_embedded_browser_entrypoint_and_support_files() {
    let root = fixture(false);
    put(
        root.path(),
        "Cargo.toml",
        format!(
            "[workspace]\nmembers = {:?}\n",
            [&MEMBERS[..], &["adapters/process"]].concat()
        )
        .as_bytes(),
    );
    put(
        root.path(),
        "adapters/process/Cargo.toml",
        include_bytes!("../../../adapters/process/Cargo.toml"),
    );
    put(
        root.path(),
        "adapters/process/src/lib.rs",
        b"pub struct Runner;\n",
    );
    put(
        root.path(),
        "adapters/process/social-card.mjs",
        b"export function capture() {}\n",
    );
    put(
        root.path(),
        "adapters/process/social_card/launch.mjs",
        b"export function browserLaunch() {}\n",
    );
    for path in [
        "adapters/process/social-card.mjs",
        "adapters/process/social_card/launch.mjs",
    ] {
        let before = repository_source::snapshot(root.path()).expect("declared browser helper");
        put(root.path(), path, b"changed embedded browser behavior\n");
        let after = repository_source::snapshot(root.path()).expect("changed browser helper");
        assert_ne!(before.digest, after.digest, "{path}");
        assert!(after.watched.contains(&root.path().join(path)));
    }
}
