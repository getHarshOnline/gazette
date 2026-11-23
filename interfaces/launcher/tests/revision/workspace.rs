//! Full TOML/compiler/application/native-reader path using disposable real repositories.

#[path = "workspace/mcp.rs"]
mod mcp;

#[path = "workspace/includes.rs"]
mod includes;

use std::{
    fs,
    io::Write,
    process::{Command, Output, Stdio},
};

use gazette_sdk::{build, corpus, markdown, source, toml};
use git2::Repository;
use tempfile::tempdir;

use super::{Fixture, commit_docs, commit_tree, seed_repo};

const HEADER: &str = "[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n";
const PROJECT: &str = "[[projects]]\nid='project:one'\nlifecycle='active'\nvisibility='public'\navailability='unavailable'\n";
const PLACEMENT: &str = "[[placements]]\nrole='primary'\nsite='site:one'\nproject='project:one'\nrender=true\ndiscoverable=false\ndocuments={source='source:one',target='docs/tool'}\n";

fn config(fixture: &Fixture) -> String {
    format!(
        "{HEADER}{PROJECT}{PLACEMENT}[[sources]]\nid='source:one'\n[sources.location]\nkind='repository'\nroot={:?}\ncommit='{}'\nsubdirectory='docs'\n",
        fixture.root.path().to_str().expect("fixture path"),
        fixture.commit
    )
}

fn limits() -> source::PreparationLimits {
    source::PreparationLimits {
        sources: 256,
        entries: source::Limits::default().entries,
        bytes: source::Limits::default().total_bytes,
        per_source: source::RevisionLimits::default(),
    }
}

#[test]
fn document_phase_uses_pinned_payloads_and_matches_cli_without_reopening() {
    let mut fixture = Fixture::new();
    let repo = Repository::open(fixture.root.path()).expect("repo");
    let files: [(&str, &[u8]); 4] = [
        ("readme.md", b"# Native README\n"),
        (
            "guide.md",
            b"+++\ntitle='Guide'\n+++\n[README](readme.md)\n",
        ),
        ("image.bin", &[0xff, 0x80, 0]),
        ("calculation.ipynb", br#"{"nbformat":4,"nbformat_minor":5,"metadata":{"title":"Not publication metadata"},"cells":[{"id":"one","cell_type":"code","metadata":{},"source":"raise RuntimeError('must never execute')","execution_count":null,"outputs":[]}]}"#),
    ];
    let entries: Vec<_> = files
        .iter()
        .map(|(name, bytes)| {
            fs::write(fixture.root.path().join("docs").join(name), bytes).expect("file");
            (*name, repo.blob(bytes).expect("blob"), 0o100644)
        })
        .collect();
    fixture.commit = commit_docs(&repo, &entries, "document corpus");
    let config = config(&fixture);
    let acquired = build::acquire_site(
        &config,
        &"site:one".parse().expect("site"),
        toml::Limits::default(),
        limits(),
    )
    .expect("pin");
    let parsed = acquired
        .parse_documents(corpus::Limits::default(), &gazette_adapter_markdown::Comrak)
        .expect("syntax phase");
    assert_eq!(parsed.corpus().files().len(), 4);
    let corpus::File::Notebook(notebook) = &parsed.corpus().files()["docs/tool/calculation.ipynb"]
    else {
        panic!("retained notebook syntax");
    };
    assert_eq!(notebook.source(), files[3].1);
    assert_eq!(notebook.inspect().cells, 1);
    assert!(
        parsed.corpus().files()["docs/tool/calculation.ipynb"]
            .metadata()
            .is_none()
    );
    assert!(
        parsed.corpus().files()["docs/tool/readme.md"]
            .metadata()
            .is_none()
    );
    assert_eq!(parsed.acquired().site(), acquired.site());
    let report = parsed.inspect().expect("report");
    let output = run(
        &config,
        &["--acquire-site", "site:one", "--parse-documents"],
    );
    assert!(output.status.success(), "{output:?}");
    let mut expected = serde_json::to_vec_pretty(&report).expect("JSON");
    expected.push(b'\n');
    assert_eq!(output.stdout, expected);
    for (name, bytes) in files {
        assert_eq!(
            fs::read(fixture.root.path().join("docs").join(name)).expect("preserved source"),
            bytes
        );
    }
    fs::write(
        fixture.root.path().join("docs/readme.md"),
        "changed after capture",
    )
    .expect("fixture mutation");
    let retained = acquired
        .parse_documents(corpus::Limits::default(), &gazette_adapter_markdown::Comrak)
        .expect("no reopening");
    assert_eq!(
        retained.corpus().files()["docs/tool/readme.md"]
            .body()
            .expect("AST")
            .render_html()
            .expect("fragment"),
        "<h1 id=\"native-readme\">Native README</h1>\n"
    );
    assert!(
        !run(
            &config,
            &["--acquire-site", "site:one", "--parse-documents"]
        )
        .status
        .success(),
        "fresh acquisition must observe changed source"
    );
}

#[test]
fn malformed_notebook_fails_real_check_without_partial_evidence_or_source_repair() {
    for (bytes, code) in [
        (br#"{"nbformat":4,"nbformat_minor":5,"metadata":{},"cells":[{"cell_type":"markdown","metadata":{},"source":"missing ID"}]}"#.as_slice(), "notebook_structure"),
        (br#"{"nbformat":4,"nbformat_minor":5,"metadata":{},"cells":[{"id":"same","cell_type":"markdown","metadata":{},"source":"first"},{"id":"same","cell_type":"markdown","metadata":{},"source":"second"}]}"#.as_slice(), "notebook_duplicate_id"),
        (br#"{"nbformat":4,"nbformat_minor":5,"metadata":{},"cells":[],"cells":[]}"#.as_slice(), "notebook_json"),
    ] {
        let mut fixture = Fixture::new();
        let repo = Repository::open(fixture.root.path()).expect("fixture repository");
        let path = fixture.root.path().join("docs/z.ipynb");
        fs::write(&path, bytes).expect("fixture notebook");
        fixture.commit = commit_docs(&repo, &[
            ("readme.md", repo.blob(b"abc").expect("Markdown blob"), 0o100644),
            ("z.ipynb", repo.blob(bytes).expect("notebook blob"), 0o100644),
        ], "invalid notebook fixture");
        let declaration = config(&fixture);
        assert!(run(&declaration, &["--acquire-site", "site:one"]).status.success());
        let output = run(&declaration, &["--acquire-site", "site:one", "--parse-documents"]);
        assert!(!output.status.success(), "{output:?}");
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains(code), "{output:?}");
        assert_eq!(fs::read(&path).expect("unchanged source"), bytes);
    }
}

#[test]
fn late_bad_document_and_corpus_limits_fail_check_without_partial_output() {
    let mut fixture = Fixture::new();
    let valid = config(&fixture);
    for flag in [
        "--max-corpus-files",
        "--max-corpus-bytes",
        "--max-corpus-nodes",
    ] {
        let output = run(
            &valid,
            &["--acquire-site", "site:one", "--parse-documents", flag, "0"],
        );
        assert!(!output.status.success(), "{flag}: {output:?}");
        assert!(output.stdout.is_empty());
    }
    assert!(
        !run(&valid, &["--parse-documents"]).status.success(),
        "explicit source-read opt-in"
    );
    let repo = Repository::open(fixture.root.path()).expect("repo");
    let invalid = b"+++\nbroken = [\n+++\nbody";
    fs::write(fixture.root.path().join("docs/z.md"), invalid).expect("file");
    fixture.commit = commit_docs(
        &repo,
        &[
            (
                "readme.md",
                repo.blob(b"abc").expect("valid blob"),
                0o100644,
            ),
            ("z.md", repo.blob(invalid).expect("bad blob"), 0o100644),
        ],
        "late invalid document",
    );
    let config = config(&fixture);
    assert!(
        run(&config, &["--acquire-site", "site:one"])
            .status
            .success(),
        "pin is not syntax validation"
    );
    let output = run(
        &config,
        &["--acquire-site", "site:one", "--parse-documents"],
    );
    assert!(!output.status.success());
    assert!(output.stdout.is_empty(), "no successful README-only subset");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("docs/tool/z.md"),
        "{output:?}"
    );
    assert_eq!(
        fs::read(fixture.root.path().join("docs/z.md")).expect("unchanged"),
        invalid
    );
}

fn run(config: &str, args: &[&str]) -> Output {
    let cwd = tempdir().expect("unrelated cwd");
    let mut child = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args(["check", "--stdin"])
        .args(args)
        .current_dir(cwd.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Gazette executable");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(config.as_bytes())
        .expect("config");
    let output = child.wait_with_output().expect("process completion");
    assert!(
        fs::read_dir(cwd.path()).expect("cwd").next().is_none(),
        "check has no output/store effects"
    );
    output
}

#[test]
fn workspace_pin_drives_sdk_cli_and_retained_document_parsing_without_reopening_source() {
    let fixture = Fixture::new();
    let config = config(&fixture);
    let before = fs::read(fixture.root.path().join("docs/readme.md")).expect("source");
    let acquired = build::acquire_site(
        &config,
        &"site:one".parse().expect("site"),
        toml::Limits::default(),
        limits(),
    )
    .expect("acquisition");
    let output = run(&config, &["--acquire-site", "site:one"]);
    assert!(output.status.success(), "{output:?}");
    let mut expected = serde_json::to_vec_pretty(&acquired.inspect()).expect("shared report");
    expected.push(b'\n');
    assert_eq!(output.stdout, expected);
    assert_eq!(
        fs::read(fixture.root.path().join("docs/readme.md")).expect("unchanged"),
        before
    );
    assert_eq!(acquired.sources().placements().len(), 1);
    assert_eq!(
        acquired.sources().placements()[0].request().target.as_str(),
        "docs/tool"
    );
    fs::write(fixture.root.path().join("docs/readme.md"), "changed").expect("fixture change");
    let bytes = acquired.sources().placements()[0]
        .source()
        .file(&"readme.md".parse().expect("path"))
        .expect("retained bytes");
    let parsed = markdown::ParsedMarkdown::parse(
        std::str::from_utf8(bytes).expect("UTF8"),
        markdown::Limits::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("shared AST");
    assert_eq!(parsed.render_html().expect("fragment"), "<p>abc</p>\n");
    let failed = run(&config, &["--acquire-site", "site:one"]);
    assert!(!failed.status.success());
    assert!(
        failed.stdout.is_empty(),
        "later capture cannot silently reuse prior bytes"
    );
}

#[test]
fn site_selection_and_disabled_placements_do_not_read_unselected_roots() {
    let fixture = Fixture::new();
    let config = config(&fixture);
    let inaccessible = config.replace(
        fixture.root.path().to_str().expect("root"),
        "/gazette-fixture-missing-repository",
    );
    assert!(
        run(&inaccessible, &[]).status.success(),
        "pure check does not read repositories"
    );
    let disabled = inaccessible.replace("render=true", "render=false");
    let output = run(&disabled, &["--acquire-site", "site:one"]);
    assert!(output.status.success(), "{output:?}");
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON");
    assert!(
        value["placements"]
            .as_array()
            .expect("placements")
            .is_empty()
    );
    let other_site = "[[sites]]\nid='site:two'\npublisher='owner:one'\ndeployable=false\n";
    let unselected = format!("{inaccessible}{other_site}");
    assert!(
        run(&unselected, &["--acquire-site", "site:two"])
            .status
            .success()
    );
    let missing = run(&config, &["--acquire-site", "site:missing"]);
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("workspace_site"));
}

#[test]
fn complete_reference_validation_and_target_preflight_precede_native_acquisition() {
    let fixture = Fixture::new();
    let valid = config(&fixture);
    let config = valid.replace(
        fixture.root.path().to_str().expect("root"),
        "/gazette-fixture-missing-repository",
    );
    let bad_reference = format!(
        "{config}[[sites]]\nid='site:other'\npublisher='owner:missing'\ndeployable=false\n"
    );
    let error = build::acquire_site(
        &bad_reference,
        &"site:one".parse().expect("site"),
        toml::Limits::default(),
        limits(),
    )
    .expect_err("global reference failure");
    assert!(
        matches!(error, build::Error::Configuration(_)),
        "no native failure may precede configuration: {error}"
    );
    let other_project = PROJECT.replace("project:one", "project:two");
    let overlap = PLACEMENT
        .replace("project:one", "project:two")
        .replace("docs/tool", "docs/tool/nested");
    let error = build::acquire_site(
        &format!("{config}{other_project}{overlap}"),
        &"site:one".parse().expect("site"),
        toml::Limits::default(),
        limits(),
    )
    .expect_err("overlap before reads");
    assert!(matches!(
        error,
        build::Error::Configuration(gazette_sdk::workspace::Error::Invalid(errors))
            if errors.iter().any(|error| error.code == "workspace_content_overlap")
    ));
    for args in [
        vec!["--max-placements", "0"],
        vec!["--max-acquired-entries", "0"],
        vec!["--max-acquired-bytes", "0"],
    ] {
        let mut selected = vec!["--acquire-site", "site:one"];
        selected.extend(args);
        let output = run(&valid, &selected);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn parent_gitlink_configuration_uses_the_child_pin_and_never_a_sibling_clone() {
    let parent = Fixture::new();
    let child = parent.root.path().join("child");
    fs::create_dir(&child).expect("child directory");
    let child_commit = seed_repo(&child);
    let repo = Repository::open(parent.root.path()).expect("parent");
    let mut tree = repo.treebuilder(None).expect("tree");
    tree.insert("child", child_commit, 0o160000)
        .expect("gitlink");
    let parent_commit = commit_tree(&repo, tree.write().expect("tree write"), "child binding");
    let config = format!(
        "{HEADER}{PROJECT}{PLACEMENT}[[sources]]\nid='source:one'\n[sources.location]\nkind='submodule'\nroot={:?}\nparent_commit='{parent_commit}'\nsubmodule_path='child'\nsubdirectory='docs'\n",
        parent.root.path().to_str().expect("root")
    );
    let output = run(&config, &["--acquire-site", "site:one"]);
    assert!(output.status.success(), "{output:?}");
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON");
    let evidence = &value["placements"][0]["evidence"];
    assert_eq!(evidence["kind"], "submodule");
    assert_eq!(
        evidence["verification"]["parent_commit"],
        parent_commit.to_string()
    );
    assert_eq!(
        evidence["verification"]["source"]["commit"],
        child_commit.to_string()
    );
    fs::rename(&child, parent.root.path().join("sibling")).expect("move disposable child");
    let failed = run(&config, &["--acquire-site", "site:one"]);
    assert!(!failed.status.success());
    assert!(failed.stdout.is_empty());
}
