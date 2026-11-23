#![cfg(feature = "native-configuration")]

//! The native SDK exposes the exact modular workspace loader without an interface fork.

use std::fs;

use gazette_sdk::{configuration, source::EntryPath};

#[test]
fn native_sdk_loads_the_explicit_workspace_document_set() {
    let fixture = tempfile::tempdir().expect("workspace root");
    fs::create_dir(fixture.path().join("sites")).expect("site module directory");
    fs::write(
        fixture.path().join("gazette.toml"),
        "includes=['sites/one.toml']\n[[principals]]\nid='owner:one'\nkind='person'\n",
    )
    .expect("root document");
    fs::write(
        fixture.path().join("sites/one.toml"),
        "[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n",
    )
    .expect("site document");
    let path: EntryPath = "gazette.toml".parse().expect("workspace path");

    let loaded = configuration::load_workspace(
        fixture.path(),
        &path,
        configuration::WorkspaceLoadLimits::default(),
    )
    .expect("SDK modular workspace load");
    assert_eq!(loaded.documents().documents.len(), 2);
    assert_eq!(loaded.documents().compilation.workspace.sites().count(), 1);
    assert_eq!(
        loaded.source(&path),
        Some(
            b"includes=['sites/one.toml']\n[[principals]]\nid='owner:one'\nkind='person'\n"
                .as_slice()
        )
    );
    assert_eq!(
        loaded.source(&"sites/one.toml".parse().expect("included path")),
        Some(b"[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n".as_slice())
    );
}
