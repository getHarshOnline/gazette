use std::{fs, path::Path};

use gazette_sdk::configuration::{LocationOrigin, WorkspaceLookup, locate_workspace};

#[test]
fn preferences_are_optional_and_preserve_other_settings() {
    use gazette_sdk::configuration::{default_workspace, set_default_workspace};
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("config.json");
    assert!(default_workspace(&path).unwrap().is_none());
    fs::write(&path, r#"{"another_setting":true}"#).unwrap();
    set_default_workspace(&path, temp.path()).unwrap();
    assert_eq!(
        default_workspace(&path).unwrap().as_deref(),
        Some(temp.path())
    );
    let settings: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(settings["another_setting"], true);
    fs::write(&path, r#"{"workspace":"relative"}"#).unwrap();
    assert!(default_workspace(&path).is_err());
    fs::write(&path, b"not JSON").unwrap();
    assert!(set_default_workspace(&path, temp.path()).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"not JSON");
    #[cfg(unix)]
    {
        let target = temp.path().join("linked.json");
        let link = temp.path().join("config-link.json");
        fs::write(&target, r#"{"another_setting":true}"#).unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();
        set_default_workspace(&link, temp.path()).unwrap();
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            default_workspace(&target).unwrap().as_deref(),
            Some(temp.path())
        );
    }
}

fn workspace(root: &Path) {
    fs::create_dir_all(root).unwrap();
    fs::write(root.join("gazette.toml"), "sites=[]").unwrap();
}

#[test]
fn request_client_start_and_default_keep_their_precedence_and_origin() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().canonicalize().unwrap();
    let start = base.join("start");
    let client = base.join("client");
    let requested = base.join("requested");
    let default = base.join("default");
    for root in [&start, &client, &requested, &default] {
        workspace(root);
    }
    let lookup = |request, client| {
        locate_workspace(WorkspaceLookup {
            request,
            client,
            start: &start,
            default: Some(&default),
        })
        .unwrap()
        .unwrap()
    };
    let result = lookup(Some(Path::new("../requested")), Some(client.as_path()));
    assert_eq!(result.root, requested);
    assert_eq!(result.found_by, LocationOrigin::Request);
    assert_eq!(
        lookup(None, Some(client.as_path())).found_by,
        LocationOrigin::Client
    );
    assert_eq!(
        lookup(None, None).found_by,
        LocationOrigin::WorkingDirectory
    );
    fs::remove_file(start.join("gazette.toml")).unwrap();
    assert_eq!(lookup(None, None).found_by, LocationOrigin::UserDefault);
}

#[test]
fn nested_folders_find_their_workspace_but_do_not_cross_a_repository() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    workspace(&root);
    let nested = root.join("project/docs");
    fs::create_dir_all(&nested).unwrap();
    let lookup = || {
        locate_workspace(WorkspaceLookup {
            request: None,
            client: None,
            start: &nested,
            default: None,
        })
        .unwrap()
    };
    assert_eq!(lookup().unwrap().root, root);
    fs::write(root.join("project/.git"), "gitdir: elsewhere").unwrap();
    assert!(lookup().is_none());
}

#[test]
fn missing_or_invalid_explicit_selections_never_fall_back() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    workspace(&root);
    for request in ["missing", "empty"] {
        fs::create_dir_all(root.join("empty")).unwrap();
        let error = locate_workspace(WorkspaceLookup {
            request: Some(Path::new(request)),
            client: None,
            start: &root,
            default: Some(&root),
        })
        .unwrap_err();
        assert!(error.path.starts_with(root.join(request)));
    }
    fs::write(root.join("other.toml"), "broken but explicitly selected").unwrap();
    let result = locate_workspace(WorkspaceLookup {
        request: Some(Path::new("other.toml")),
        client: None,
        start: &root,
        default: None,
    })
    .unwrap()
    .unwrap();
    assert_eq!(result.workspace.as_str(), "other.toml");
}
