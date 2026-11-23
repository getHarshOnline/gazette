//! An included manual source reaches the real CLI through workspace acquisition.

use std::{fs, os::unix::fs::symlink};

use gazette_sdk::{build, corpus, toml, workspace};
use tempfile::tempdir;

use super::{HEADER, PLACEMENT, PROJECT, limits, run};

#[test]
fn included_branch_check_preserves_dirty_bytes_and_refuses_another_branch_at_the_same_commit() {
    let fixture = super::Fixture::new();
    let repo = git2::Repository::open(fixture.root.path()).unwrap();
    let commit = repo.find_commit(fixture.commit).unwrap();
    repo.branch("manuals", &commit, false).unwrap();
    repo.branch("other", &commit, false).unwrap();
    repo.set_head("refs/heads/manuals").unwrap();
    fs::write(fixture.root.path().join("docs/readme.md"), "# Held edits\n").unwrap();
    let config = format!(
        "{HEADER}{PROJECT}{PLACEMENT}[[sources]]\nid='source:one'\nlocation={{kind='included-filesystem', root={:?}, include=['docs'], expected_branch='manuals'}}\n",
        fixture.root.path().to_str().unwrap()
    );
    let output = run(
        &config,
        &["--acquire-site", "site:one", "--parse-documents"],
    );
    assert!(output.status.success(), "{output:?}");
    for detached in [false, true] {
        if detached {
            repo.set_head_detached(fixture.commit).unwrap();
        } else {
            repo.set_head("refs/heads/other").unwrap();
        }
        let head = fs::read(repo.path().join("HEAD")).unwrap();
        let output = run(&config, &["--acquire-site", "site:one"]);
        assert!(!output.status.success(), "{output:?}");
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("expected \"manuals\""),
            "{output:?}"
        );
        assert_eq!(fs::read(repo.path().join("HEAD")).unwrap(), head);
        assert_eq!(
            fs::read(fixture.root.path().join("docs/readme.md")).unwrap(),
            b"# Held edits\n"
        );
    }
}

#[test]
fn included_manual_source_keeps_only_declared_files_through_sdk_and_cli() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("docs")).unwrap();
    fs::write(root.path().join("docs/guide.md"), "# Guide\n").unwrap();
    fs::write(root.path().join("README.md"), "# Overview\n").unwrap();
    symlink("unavailable", root.path().join("unselected-private-input")).unwrap();
    let config = format!(
        "{HEADER}{PROJECT}{PLACEMENT}[[sources]]\nid='source:one'\n[sources.location]\nkind='included-filesystem'\nroot={:?}\ninclude=['docs','README.md']\n",
        root.path().to_str().unwrap()
    );
    let acquired = build::acquire_site(
        &config,
        &"site:one".parse().unwrap(),
        toml::Limits::default(),
        limits(),
    )
    .unwrap();
    let parsed = acquired
        .parse_documents(corpus::Limits::default(), &gazette_adapter_markdown::Comrak)
        .unwrap();
    assert_eq!(
        parsed
            .corpus()
            .files()
            .keys()
            .map(|path| path.as_str())
            .collect::<Vec<_>>(),
        ["docs/tool/README.md", "docs/tool/docs/guide.md"]
    );
    let output = run(
        &config,
        &["--acquire-site", "site:one", "--parse-documents"],
    );
    assert!(output.status.success(), "{output:?}");
    let mut expected = serde_json::to_vec_pretty(&parsed.inspect().unwrap()).unwrap();
    expected.push(b'\n');
    assert_eq!(output.stdout, expected);
    fs::remove_file(root.path().join("README.md")).unwrap();
    let missing = run(&config, &["--acquire-site", "site:one"]);
    assert!(!missing.status.success());
    assert!(missing.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&missing.stderr).contains("required source selection is missing"),
        "{missing:?}"
    );
}

#[test]
fn included_source_declaration_rejects_empty_duplicate_and_unsafe_paths() {
    for include in [
        "[]",
        "['docs','docs']",
        "['../private']",
        "['/private']",
        "['docs/../private']",
    ] {
        let config = format!(
            "{HEADER}{PROJECT}{PLACEMENT}[[sources]]\nid='source:one'\nlocation={{kind='included-filesystem', root='.', include={include}}}\n"
        );
        assert!(
            workspace::compile(&config, toml::Limits::default()).is_err(),
            "{include}"
        );
    }
    let config = format!(
        "{HEADER}{PROJECT}{PLACEMENT}[[sources]]\nid='source:one'\nlocation={{kind='included-filesystem', root='.', include=['docs']}}\n"
    );
    workspace::compile(&config, toml::Limits::default()).unwrap();
}

#[test]
fn native_workspace_resolves_included_sources_from_its_selected_machine_binding() {
    let workspace = tempdir().unwrap();
    // The CLI resolves --workspace-root; select that same native address for each carrier.
    let workspace_root = workspace.path().canonicalize().unwrap();
    let content = tempdir().unwrap();
    let unrelated = tempdir().unwrap();
    fs::create_dir(workspace.path().join("tools")).unwrap();
    fs::create_dir(content.path().join("docs")).unwrap();
    fs::write(
        content.path().join("docs/guide.md"),
        "# Machine-bound guide\n",
    )
    .unwrap();
    let config = format!(
        "machine_binding='tools/studio.local.toml'\n{HEADER}{PROJECT}{PLACEMENT}[[sources]]\nid='source:one'\nlocation={{kind='included-filesystem',root='machine:documents',include=['docs']}}\n"
    );
    fs::write(workspace.path().join("gazette.toml"), config).unwrap();
    let binding_path = workspace.path().join("tools/studio.local.toml");
    fs::write(
        &binding_path,
        format!(
            "[roots]\ndocuments={:?}\n",
            content.path().to_str().unwrap()
        ),
    )
    .unwrap();
    let loaded = std::process::Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args(["check", "workspace"])
        .arg(&workspace_root)
        .args(["--workspace", "gazette.toml", "--allow-read"])
        .current_dir(unrelated.path())
        .output()
        .unwrap();
    assert!(loaded.status.success(), "{loaded:?}");
    let loaded: serde_json::Value = serde_json::from_slice(&loaded.stdout).unwrap();
    assert_eq!(
        loaded["source_roots"]["source:one"],
        serde_json::json!(content.path())
    );
    let load_arguments = serde_json::json!({
        "root": workspace_root, "workspace": "gazette.toml", "allow_native_reads": true
    });
    assert_eq!(
        super::mcp::call("load_workspace", load_arguments, false),
        loaded
    );
    let denied = super::mcp::call(
        "load_workspace",
        serde_json::json!({
            "root": workspace_root, "workspace": "gazette.toml", "allow_native_reads": false
        }),
        true,
    );
    assert!(denied.get("source_roots").is_none());
    let run = || {
        std::process::Command::new(env!("CARGO_BIN_EXE_gazette"))
            .args(["check", "--workspace-root"])
            .arg(&workspace_root)
            .args(["--acquire-site", "site:one", "--parse-documents"])
            .current_dir(unrelated.path())
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap()
    };
    let output = run();
    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("guide.md"));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let evidence = &report["acquisition"]["configuration"];
    assert_eq!(evidence["kind"], "documents");
    let binding = evidence["inventory"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["path"] == "tools/studio.local.toml")
        .unwrap();
    assert_eq!(
        binding["sha256"],
        serde_json::json!(gazette_sdk::source::Sha256Digest::of(
            &fs::read(&binding_path).unwrap()
        ))
    );
    let arguments = serde_json::json!({
        "source": {"kind":"local", "root":workspace_root, "workspace":"gazette.toml"},
        "site":"site:one", "allow_read":true
    });
    assert_eq!(
        super::mcp::call("parse_site_documents", arguments.clone(), false),
        report
    );
    assert_eq!(
        super::mcp::call("inspect_site", arguments, false),
        report["acquisition"]
    );
    // Changing machine bytes changes capture evidence, not portable configuration identity.
    fs::write(
        &binding_path,
        format!(
            "[roots]\ndocuments={:?}\nunused='/another/machine/root'\n",
            content.path().to_str().unwrap()
        ),
    )
    .unwrap();
    let changed = run();
    assert!(changed.status.success(), "{changed:?}");
    let changed: serde_json::Value = serde_json::from_slice(&changed.stdout).unwrap();
    assert_eq!(
        changed["acquisition"]["compilation"],
        report["acquisition"]["compilation"]
    );
    assert_ne!(
        changed["acquisition"]["configuration"],
        report["acquisition"]["configuration"]
    );
    fs::write(&binding_path, "[roots]\nother='/unavailable'\n").unwrap();
    let output = run();
    assert!(!output.status.success(), "{output:?}");
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("machine root \"documents\" is not bound"),
        "{output:?}"
    );
    assert!(fs::read_dir(unrelated.path()).unwrap().next().is_none());
}
