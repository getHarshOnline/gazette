//! Captured reference artifacts reach real local assembly; no generator invocation is implied.

#[path = "documentation/preparation.rs"]
mod preparation;

#[path = "documentation/preparation_cli.rs"]
mod preparation_cli;

use gazette_test_support as native;

use gazette_sdk::{build, source};
use git2::{Oid, Repository, Signature};
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};
use tempfile::{TempDir, tempdir};

fn artifact() -> Vec<u8> {
    serde_json::to_vec_pretty(&json!({
        "schemaVersion":"2.0", "variant":"project", "id":0, "kind":1,
        "name":"Fixture", "flags":{}, "files":{}, "packageVersion":"1.2.3",
        "children":[{"variant":"declaration","id":1,"kind":64,"name":"run","flags":{},
            "comment":{"summary":[{"kind":"text","text":"Exact documentation and example."}]},
            "signatures":[{"variant":"signature","id":2,"kind":4096,"name":"run","flags":{},
                "type":{"type":"reference","name":"Result","target":3}}]},
            {"variant":"declaration","id":3,"kind":2097152,"name":"Result","flags":{},
             "type":{"type":"intrinsic","name":"string"}}]
    }))
    .expect("representative native artifact")
}

struct Fixture {
    root: TempDir,
    destination: TempDir,
    commit: Oid,
}
impl Fixture {
    fn new(bytes: &[u8]) -> Self {
        let valid = artifact();
        Self::with_files(&[("library.json", bytes), ("valid.json", &valid)])
    }

    fn with_files(files: &[(&str, &[u8])]) -> Self {
        let root = tempdir().expect("isolated Git source");
        let destination = tempdir().expect("intended destination owner");
        let repository = Repository::init(root.path()).expect("fixture Git");
        let mut index = repository.index().expect("fixture index");
        for (name, bytes) in files {
            let relative = Path::new("generated").join(name);
            let path = root.path().join(&relative);
            fs::create_dir_all(path.parent().expect("artifact parent"))
                .expect("fixture directories");
            fs::write(path, bytes).expect("native artifact");
            index.add_path(&relative).expect("exact fixture file");
        }
        let tree = repository
            .find_tree(index.write_tree().expect("tree identity"))
            .expect("tree");
        let signature =
            Signature::new("Fixture", "fixture@example.invalid", &git2::Time::new(0, 0))
                .expect("fixed signature");
        let commit = repository
            .commit(Some("HEAD"), &signature, &signature, "fixture", &tree, &[])
            .expect("disposable fixture pin");
        Self {
            root,
            destination,
            commit,
        }
    }

    fn intended(&self) -> PathBuf {
        self.destination.path().join("selected-local-directory")
    }

    fn configuration(&self) -> String {
        let mut config = String::from(
            "[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=true\n[[sites]]\nid='site:other'\npublisher='owner:one'\ndeployable=false\n",
        );
        for project in ["one", "two"] {
            config.push_str(&format!("[[projects]]\nid='project:{project}'\nlifecycle='active'\nvisibility='public'\navailability='available'\n"));
        }
        config.push_str(&format!("[[sources]]\nid='source:artifact'\nlocation={{kind='repository',root={:?},commit='{}',subdirectory='generated'}}\n[[sources]]\nid='source:unavailable'\nlocation={{kind='repository',root={:?},commit='0000000000000000000000000000000000000000',subdirectory='generated'}}\n[[brands]]\nid='brand:one'\nname='Configured publication'\n[[brands.publications]]\nsite='site:one'\n[brands.publications.documentation_repository]\nname='configured-domain-doc'\npath={:?}\ndestination={{kind='git',remote='https://example.invalid/configured-domain-doc.git',branch='published'}}\n[brands.publications.documentation_repository.build]\ntarget={{kind='mintlify',theme='mint',primary_color='#123abc'}}\n", self.root.path().to_str().expect("source path"), self.commit, self.destination.path().join("absent-source").to_str().expect("absent path"), self.intended().to_str().expect("intended path")));
        for (project, slug, references) in [
            ("two", "second-first", vec!["library", "server"]),
            ("one", "first-second", vec!["library"]),
        ] {
            config.push_str(&format!("[[brands.publications.documentation_repository.build.modules]]\nproject='project:{project}'\nslug='{slug}'\ntitle='Module {project}'\n"));
            for reference in references {
                config.push_str(&format!("[[brands.publications.documentation_repository.build.modules.navigation]]\nkind='reference'\nslug='{reference}'\ntitle='Reference {reference}'\ninput={{kind='artifact',source='source:artifact',selection={{kind='file',path='library.json'}},format='typedoc-json'}}\n"));
            }
        }
        config.push_str(&format!("[[brands.publications]]\nsite='site:other'\n[brands.publications.documentation_repository]\nname='other-domain-doc'\npath={:?}\n[brands.publications.documentation_repository.build]\ntarget={{kind='mintlify',theme='mint',primary_color='#123abc'}}\n[[brands.publications.documentation_repository.build.modules]]\nproject='project:one'\nslug='other'\ntitle='Other'\n[[brands.publications.documentation_repository.build.modules.navigation]]\nkind='reference'\nslug='library'\ntitle='Other library'\ninput={{kind='artifact',source='source:unavailable',selection={{kind='file',path='library.json'}},format='typedoc-json'}}\n", self.destination.path().join("other-target").to_str().expect("other target")));
        config
    }
}

fn acquire(config: &str) -> build::AcquiredInputs {
    build::acquire_documentation(
        config,
        &"site:one".parse().expect("site"),
        Default::default(),
        source::PreparationLimits {
            sources: 1,
            entries: 8,
            bytes: 65536,
            per_source: Default::default(),
        },
    )
    .expect("one deduplicated pinned source; no Theme or authored binding")
}

fn run(config: &str, parent: Option<&Path>) -> Output {
    let cwd = tempdir().expect("empty standalone cwd");
    let mut command = Command::new(env!("CARGO_BIN_EXE_gazette"));
    command.args(["assemble-docs", "--local", "--site", "site:one"]);
    if let Some(parent) = parent {
        command.arg("--output-parent").arg(parent);
    }
    let mut child = command
        .current_dir(cwd.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("direct Gazette CLI without Pale");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(config.as_bytes())
        .expect("config transport");
    let output = child.wait_with_output().expect("CLI completion");
    assert!(
        fs::read_dir(cwd.path()).expect("cwd").next().is_none(),
        "no implicit workspace creation"
    );
    output
}

fn file<'a>(tree: &'a source::CapturedSource, path: &str) -> &'a [u8] {
    tree.file(&path.parse().expect("output path"))
        .expect("complete output entry")
}

fn project_dropdowns(document: &Value) -> &[Value] {
    let navigation = &document["navigation"];
    assert!(navigation.get("groups").is_none());
    let dropdowns = navigation["dropdowns"]
        .as_array()
        .expect("project dropdowns");
    assert_eq!(
        dropdowns
            .iter()
            .map(|item| item["dropdown"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["Module two", "Module one"]
    );
    dropdowns
}

fn assert_typedoc_navigation(document: &Value) {
    let dropdowns = project_dropdowns(document);
    for (dropdown, module, references) in [
        (&dropdowns[0], "second-first", vec!["library", "server"]),
        (&dropdowns[1], "first-second", vec!["library"]),
    ] {
        assert_eq!(
            dropdown["pages"],
            Value::Array(references.into_iter().map(|reference| json!({
                "group":format!("Reference {reference}"),
                "sdk":{"directory":format!("{module}/{reference}"),"format":"typedoc","source":format!("artifacts/{module}/{reference}.json")}
            })).collect())
        );
    }
}

#[test]
fn authored_manual_only_reaches_the_connected_cli_with_distinct_evidence() {
    let fixture = Fixture::with_files(&[
        ("docs/start.md", b"# Start\n\n[Guide](guide.md#guide)\n"),
        ("docs/guide.md", b"# Guide\n\n[Start](start.md#start)\n"),
    ]);
    let config = format!(
        "[[principals]]\nid='owner:one'\nkind='person'\n\
         [[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n\
         [[projects]]\nid='project:manual'\nlifecycle='active'\nvisibility='public'\navailability='available'\n\
         [[sources]]\nid='source:manual'\nlocation={{kind='repository',root={:?},commit='{}',subdirectory='generated'}}\n\
         [[brands]]\nid='brand:manual'\nname='Gazette manual'\n\
         [[brands.publications]]\nsite='site:one'\n\
         [brands.publications.documentation_repository]\nname='gazette-domain-doc'\npath={:?}\n\
         [brands.publications.documentation_repository.build]\ntarget={{kind='mintlify',theme='mint',primary_color='#123abc'}}\n\
         [[brands.publications.documentation_repository.build.modules]]\nproject='project:manual'\nslug='manual'\ntitle='Gazette manual'\nmanual={{source='source:manual'}}\n\
         [[brands.publications.documentation_repository.build.modules.navigation]]\nkind='manual'\ntitle='Get started'\npages=[{{title='Start',route='manual/start',input={{kind='file',path='docs/start.md'}}}},{{title='Guide',route='manual/guide',input={{kind='file',path='docs/guide.md'}}}}]\n",
        fixture.root.path().to_str().expect("source path"),
        fixture.commit,
        fixture.intended().to_str().expect("destination path"),
    );
    let parent = tempdir().expect("private output parent");
    let output = run(&config, Some(parent.path()));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("CLI report");
    assert_eq!(report["status"], "documentation-assembled-local");
    assert_eq!(report["artifacts"], json!([]));
    let manual = report["manual"].as_array().expect("manual evidence");
    assert_eq!(manual.len(), 2);
    assert!(manual.iter().all(|item| item["kind"] == "page"));
    assert!(manual.iter().all(|item| item["source"] == "source:manual"));

    let directory = Path::new(report["directory"].as_str().expect("retained output"));
    let docs: Value = serde_json::from_slice(&fs::read(directory.join("docs.json")).unwrap())
        .expect("Mintlify navigation");
    assert_eq!(
        docs["navigation"],
        json!({"groups":[{"group":"Gazette manual","pages":[{"group":"Get started","pages":["manual/start","manual/guide"]}]}]})
    );
    assert!(
        fs::read_to_string(directory.join("manual/start.md"))
            .unwrap()
            .contains("[Guide](/manual/guide#guide)")
    );
    assert!(
        fs::read_to_string(directory.join("manual/guide.md"))
            .unwrap()
            .contains("[Start](/manual/start#start)")
    );
}

#[test]
fn configured_markdown_directory_reaches_sdk_cli_and_staging_without_source_rereads() {
    let typedoc = artifact();
    let overview = b"# Library reference\n\nVersion 1.2.3. See [Client](./nested/client.md#client).\n\n[Client API][api]\n\n[api]: ./nested/client.md#client \"Public API\"\n\nLiteral `./nested/client.md#client` is not a destination.\n\n<a data-source=\"./nested/client.md#client\">Native markup</a>\n";
    let client = b"# Client\n\n```go\ntype Client struct {}\n// [Overview](../README.md) stays example text.\n```\n\n[Overview](../README.md#library-reference)\n";
    let notes =
        b"# Notes  \n\nAlready compatible: [Client](./nested/client#client).\n\nVersion 1.2.3.\n";
    let unchanged = b"Unchanged prose.\n";
    let fixture = Fixture::with_files(&[
        ("library.json", &typedoc),
        ("bundle/README.md", overview),
        ("bundle/nested/client.md", client),
        ("bundle/notes.md", notes),
        ("bundle/unchanged.md", unchanged),
        ("outside.md", b"not selected for publication"),
    ]);
    let config = fixture.configuration().replacen(
        "selection={kind='file',path='library.json'},format='typedoc-json'",
        "selection={kind='directory',path='bundle'},format='markdown'",
        1,
    );
    let inputs = acquire(&config);
    let assembly = build::assemble_documentation(&inputs, Default::default())
        .expect("one selected directory and independent TypeDoc modules");
    let prefix = "second-first/library";
    let converted = file(assembly.output(), &format!("{prefix}/README.md"));
    let converted_client = file(assembly.output(), &format!("{prefix}/nested/client.md"));
    for (original, emitted, expected, destinations) in [
        (
            overview.as_slice(),
            converted,
            r#"---
title: "Library reference"
---

<a id="library&#45;reference"></a>

Version 1\.2\.3\. See [Client](/second-first/library/nested/client#client)\.

[Client API](/second-first/library/nested/client#client "Public API")

Literal `./nested/client.md#client` is not a destination\.

<a data-source="&#46;&#47;nested&#47;client&#46;md&#35;client">Native markup</a>
"#,
            vec![
                "/second-first/library/nested/client#client",
                "/second-first/library/nested/client#client",
            ],
        ),
        (
            client.as_slice(),
            converted_client,
            r#"---
title: "Client"
---

<a id="client"></a>

```go
type Client struct {}
// [Overview](../README.md) stays example text.
```

[Overview](/second-first/library/README#library-reference)
"#,
            vec!["/second-first/library/README#library-reference"],
        ),
    ] {
        assert_ne!(
            source::Sha256Digest::of(original),
            source::Sha256Digest::of(emitted)
        );
        assert_eq!(
            emitted,
            expected.as_bytes(),
            "complete target projection retains title, anchors, examples, prose and escaped markup"
        );
        let original = gazette_sdk::markdown::inspect(
            std::str::from_utf8(original).expect("original UTF-8"),
            Default::default(),
            false,
        )
        .expect("original syntax");
        let emitted = gazette_sdk::markdown::inspect(
            std::str::from_utf8(emitted).expect("converted UTF-8"),
            Default::default(),
            false,
        )
        .expect("converted syntax");
        assert_eq!(
            emitted
                .inspection
                .links
                .iter()
                .map(|link| link.destination.as_str())
                .collect::<Vec<_>>(),
            destinations
        );
        assert_eq!(
            original
                .inspection
                .links
                .iter()
                .map(|link| &link.title)
                .collect::<Vec<_>>(),
            emitted
                .inspection
                .links
                .iter()
                .map(|link| &link.title)
                .collect::<Vec<_>>()
        );
    }
    assert!(
        std::str::from_utf8(converted)
            .expect("converted UTF-8")
            .contains(
                "<a data-source=\"&#46;&#47;nested&#47;client&#46;md&#35;client\">Native markup</a>"
            )
    );
    assert!(
        std::str::from_utf8(converted_client)
            .expect("converted UTF-8")
            .contains("type Client struct {}\n// [Overview](../README.md) stays example text.")
    );
    assert_eq!(
        file(assembly.output(), &format!("{prefix}/notes.md")),
        b"---\ntitle: \"Notes\"\n---\n\n<a id=\"notes\"></a>\n\nAlready compatible: [Client](/second-first/library/nested/client#client)\\.\n\nVersion 1\\.2\\.3\\.\n",
        "extensionless links use provider routes while retaining the promoted title and body"
    );
    assert_eq!(
        file(assembly.output(), &format!("{prefix}/unchanged.md")),
        unchanged,
        "pages without conversion retain exact original bytes"
    );
    assert_eq!(
        file(assembly.output(), "artifacts/second-first/server.json"),
        typedoc
    );
    let navigation: Value =
        serde_json::from_slice(file(assembly.output(), "docs.json")).expect("target navigation");
    assert_eq!(
        project_dropdowns(&navigation)[0]["pages"][0],
        json!({"group":"Reference library","pages":[
            "second-first/library/README",
            "second-first/library/nested/client",
            "second-first/library/notes",
            "second-first/library/unchanged"
        ]})
    );
    assert!(
        assembly
            .output()
            .inventory()
            .entries()
            .iter()
            .all(|entry| !entry.path.as_str().contains("outside"))
    );
    let parent = tempdir().expect("private staging parent");
    let result = run(&config, Some(parent.path()));
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value = serde_json::from_slice(&result.stdout).expect("CLI evidence");
    assert_eq!(report["artifacts"][0]["format"], "markdown");
    assert_eq!(
        report["artifacts"][0]["tree"],
        serde_json::to_value(assembly.artifacts()[0].tree).expect("original capture identity")
    );
    assert_eq!(
        report["artifacts"][0]["selection"],
        json!({"kind":"directory","path":"bundle"})
    );
    let retained = Path::new(report["directory"].as_str().expect("actual retained stage"));
    for entry in assembly.output().inventory().entries() {
        if let source::Content::File { .. } = entry.content {
            assert_eq!(
                fs::read(retained.join(entry.path.as_str())).expect("native staged bytes"),
                file(assembly.output(), entry.path.as_str())
            );
        }
    }
    assert!(
        !fixture.intended().exists(),
        "configured destination is not staging authority"
    );
    assert!(std::ptr::eq(assembly.inputs(), &inputs));
    for (path, original) in [
        ("README.md", overview.as_slice()),
        ("nested/client.md", client.as_slice()),
        ("notes.md", notes.as_slice()),
        ("unchanged.md", unchanged.as_slice()),
    ] {
        assert_eq!(
            inputs.sources()[&"source:artifact".parse().unwrap()]
                .file(&format!("bundle/{path}").parse().unwrap()),
            Some(original),
            "retained source bytes remain independent from target conversion"
        );
        assert_eq!(
            fs::read(fixture.root.path().join("generated/bundle").join(path))
                .expect("unchanged original source"),
            original
        );
    }
    fs::write(
        fixture.root.path().join("generated/bundle/README.md"),
        b"later native mutation",
    )
    .expect("disposable source drift");
    let repeated = build::assemble_documentation(&inputs, Default::default())
        .expect("no source reread or generation");
    assert_eq!(assembly.output().inventory(), repeated.output().inventory());
    assert_eq!(
        file(repeated.output(), &format!("{prefix}/README.md")),
        converted
    );
    assert_eq!(assembly.artifacts()[0].tree, repeated.artifacts()[0].tree);
}

#[test]
fn configured_markdown_directory_missing_page_refuses_a_successful_partial_stage() {
    for destination in ["./missing.md", "../outside.md", "./nested/Client.md"] {
        let typedoc = artifact();
        let page = format!("# Reference\n\n[Invalid]({destination})\n");
        let fixture = Fixture::with_files(&[
            ("library.json", &typedoc),
            ("bundle/README.md", page.as_bytes()),
            ("bundle/nested/client.md", b"# Client\n"),
            ("outside.md", b"# Outside the selected reference root\n"),
        ]);
        let config = fixture.configuration().replacen(
            "selection={kind='file',path='library.json'},format='typedoc-json'",
            "selection={kind='directory',path='bundle'},format='markdown'",
            1,
        );
        let inputs = acquire(&config);
        assert!(build::assemble_documentation(&inputs, Default::default()).is_err());
        let parent = tempdir().expect("private staging parent");
        let result = run(&config, Some(parent.path()));
        assert!(
            !result.status.success(),
            "invalid destination {destination} must not publish a subset"
        );
        assert!(
            fs::read_dir(parent.path())
                .expect("stage parent")
                .next()
                .is_none()
        );
        assert!(!fixture.intended().exists());
    }
}

#[test]
fn configured_sphinx_directory_composes_with_typedoc_through_sdk_and_cli() {
    let typedoc = artifact();
    let root = br#"{"current_page_name":"overview","title":"Python API","body":"<dl><dt id=\"client.run\">run(value: str) -&gt; bool</dt></dl>","toc":""}"#;
    let fixture = Fixture::with_files(&[
        ("library.json", &typedoc),
        ("valid.json", &typedoc),
        ("python/globalcontext.json", br#"{"root_doc":"overview"}"#),
        ("python/searchindex.json", br#"{"docnames":["overview"]}"#),
        ("python/overview.fjson", root),
        ("python/environment.pickle", b"private retained runtime"),
    ]);
    let config = fixture.configuration().replacen(
        "selection={kind='file',path='library.json'},format='typedoc-json'",
        "selection={kind='directory',path='python'},format='sphinx-json'",
        1,
    );
    let inputs = acquire(&config);
    let assembly =
        build::assemble_documentation(&inputs, Default::default()).expect("mixed native formats");
    let parent = tempdir().expect("private stages");
    let result = run(&config, Some(parent.path()));
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value = serde_json::from_slice(&result.stdout).expect("native CLI receipt");
    assert_eq!(report["artifacts"][0]["format"], "sphinx-json");
    assert_eq!(
        report["artifacts"][0]["selection"],
        json!({"kind":"directory","path":"python"})
    );
    let retained = Path::new(report["directory"].as_str().expect("actual stage"));
    let prefix = "artifacts/second-first/library";
    assert_eq!(
        file(assembly.output(), &format!("{prefix}/overview.fjson")),
        root
    );
    assert!(
        assembly
            .output()
            .file(
                &format!("{prefix}/environment.pickle")
                    .parse()
                    .expect("path")
            )
            .is_none()
    );
    assert_eq!(
        file(assembly.output(), "artifacts/second-first/server.json"),
        typedoc
    );
    for entry in assembly.output().inventory().entries() {
        if let source::Content::File { .. } = entry.content {
            assert_eq!(
                fs::read(retained.join(entry.path.as_str())).expect("staged bytes"),
                file(assembly.output(), entry.path.as_str())
            );
        }
    }
    assert!(
        !fixture.intended().exists(),
        "configuration does not authorize destination creation"
    );
    fs::write(
        fixture.root.path().join("generated/python/overview.fjson"),
        b"changed native source",
    )
    .expect("disposable source drift");
    let repeat =
        build::assemble_documentation(&inputs, Default::default()).expect("retained compilation");
    assert_eq!(assembly.output().inventory(), repeat.output().inventory());
}

#[test]
fn configured_openapi_uses_retained_inputs_and_the_same_cli_staging_path() {
    let bytes = br##"{"openapi":"3.1.2","info":{"title":"Exact HTTP API","version":"1.2.3"},"paths":{"/items":{"get":{"operationId":"items","responses":{"200":{"description":"Exact response","content":{"application/json":{"schema":{"$ref":"#/components/schemas/Item"}}}}}}}},"components":{"schemas":{"Item":{"type":"object","properties":{"next":{"$ref":"#/components/schemas/Item"}},"examples":[{"$ref":"literal example, not a reference"}]}}}}"##;
    assert_openapi_staging(bytes, "openapi-json", "json");
}

#[test]
fn configured_openapi_30_keeps_its_schema_semantics_through_capture_and_cli_staging() {
    let bytes = br##"{"openapi":"3.0.4","info":{"title":"Exact nullable HTTP API","version":"1.2.3"},"paths":{"/items":{"get":{"operationId":"items","responses":{"200":{"description":"Exact response","content":{"application/json":{"schema":{"$ref":"#/components/schemas/Item"}}}}}}}},"components":{"schemas":{"Item":{"type":"object","properties":{"next":{"$ref":"#/components/schemas/Item"},"amount":{"type":"number","nullable":true,"minimum":0,"exclusiveMinimum":true}},"example":{"$ref":"literal example, not a reference"}}}}}"##;
    assert_openapi_staging(bytes, "openapi-json", "json");
}

#[test]
fn configured_openapi_yaml_preserves_native_bytes_through_the_same_staging_path() {
    let bytes = br##"# Retain this native comment, not a reserialized JSON replacement.
openapi: 3.1.2
info:
  title: Exact HTTP API
  version: '1.2.3'
  description: |
    First line.
    Second line.
paths:
  /items:
    get:
      responses:
        '200':
          description: Exact response
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/Item'
components:
  schemas:
    Item:
      type: object
      examples:
        - $ref: literal example, not a reference
"##;
    assert_openapi_staging(bytes, "openapi-yaml", "yaml");
}

fn assert_openapi_staging(bytes: &[u8], format: &str, extension: &str) {
    let fixture = Fixture::new(bytes);
    let config = fixture
        .configuration()
        .replace("format='typedoc-json'", &format!("format='{format}'"));
    let inputs = acquire(&config);
    let assembly =
        build::assemble_documentation(&inputs, Default::default()).expect("validated API assembly");
    let parent = tempdir().expect("isolated output parent");
    let output = run(&config, Some(parent.path()));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("CLI receipt");
    assert_eq!(report["artifacts"][0]["format"], format);
    assert_eq!(
        report["artifacts"][0]["content"]["sha256"],
        serde_json::to_value(source::Sha256Digest::of(bytes)).expect("digest")
    );
    let retained = Path::new(
        report["directory"]
            .as_str()
            .expect("actual retained directory"),
    );
    assert!(!fixture.intended().exists());
    let docs: Value =
        serde_json::from_slice(file(assembly.output(), "docs.json")).expect("target configuration");
    let reference = &project_dropdowns(&docs)[0]["pages"][0];
    assert_eq!(
        reference,
        &json!({
            "group":"Reference library",
            "openapi":{"directory":"second-first/library","source":format!("artifacts/second-first/library.{extension}")}
        })
    );
    for entry in assembly.output().inventory().entries() {
        if let source::Content::File { .. } = entry.content {
            assert_eq!(
                fs::read(retained.join(entry.path.as_str())).expect("staged file"),
                assembly
                    .output()
                    .file(&entry.path)
                    .expect("retained output")
            );
        }
    }
    assert_eq!(
        file(
            assembly.output(),
            &format!("artifacts/second-first/library.{extension}")
        ),
        bytes
    );
    fs::remove_file(fixture.root.path().join("generated/library.json"))
        .expect("change disposable native source after capture");
    let repeat = build::assemble_documentation(&inputs, Default::default())
        .expect("retained rebuild without native reread");
    assert_eq!(repeat.output().inventory(), assembly.output().inventory());
}

#[test]
fn selected_invalid_openapi_fails_sdk_and_cli_without_partial_output() {
    let bytes = br##"{"openapi":"3.1.2","info":{"title":"API","version":"1"},"paths":{"/items":{"get":{"responses":{"200":{"$ref":"#/components/responses/missing"}}}}}}"##;
    assert_invalid_openapi(bytes, "openapi-json");
}

#[test]
fn selected_invalid_openapi_30_fails_sdk_and_cli_without_partial_output() {
    let bytes = br##"{"openapi":"3.0.4","info":{"title":"API","version":"1"},"paths":{"/items":{"get":{"responses":{"200":{"$ref":"#/components/responses/missing"}}}}}}"##;
    assert_invalid_openapi(bytes, "openapi-json");
}

#[test]
fn selected_invalid_yaml_uses_the_same_reference_failure_without_partial_output() {
    let bytes = br##"openapi: 3.1.2
info: {title: API, version: '1'}
paths:
  /items:
    get:
      responses:
        '200': {$ref: '#/components/responses/missing'}
"##;
    assert_invalid_openapi(bytes, "openapi-yaml");
}

fn assert_invalid_openapi(bytes: &[u8], format: &str) {
    let fixture = Fixture::new(bytes);
    let config = fixture
        .configuration()
        .replacen(
            "selection={kind='file',path='library.json'},format='typedoc-json'",
            "selection={kind='file',path='valid.json'},format='typedoc-json'",
            2,
        )
        .replacen(
            "selection={kind='file',path='library.json'},format='typedoc-json'",
            &format!("selection={{kind='file',path='library.json'}},format='{format}'"),
            1,
        );
    let inputs = acquire(&config);
    let error = build::assemble_documentation(&inputs, Default::default())
        .expect_err("late API reference invalid after two valid library references");
    assert!(
        error.to_string().contains("reference target is absent"),
        "{error}"
    );
    let parent = tempdir().expect("isolated output parent");
    let output = run(&config, Some(parent.path()));
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("reference target is absent"));
    assert!(
        fs::read_dir(parent.path())
            .expect("stages")
            .next()
            .is_none()
    );
    assert!(!fixture.intended().exists());
}

#[test]
fn configured_modules_share_one_capture_and_match_cli_staged_bytes() {
    let bytes = artifact();
    let fixture = Fixture::new(&bytes);
    let config = fixture.configuration().replacen(
        "[brands.publications.documentation_repository]\n",
        "[brands.publications.documentation_repository]\npublic_base_url='https://docs.example.test/reference/'\n",
        1,
    );
    let inputs = acquire(&config);
    assert_eq!(inputs.sources().len(), 1);
    let selected = &inputs.sources()[&"source:artifact".parse().expect("source")];
    assert_eq!(
        selected.file(&"library.json".parse().expect("entry")),
        Some(bytes.as_slice())
    );
    let assembled = build::assemble_documentation(&inputs, Default::default())
        .expect("complete documentation target");
    assert!(std::ptr::eq(assembled.inputs(), &inputs));
    assert_eq!(assembled.repository().name, "configured-domain-doc");
    assert_eq!(Path::new(&assembled.repository().path), fixture.intended());
    assert!(!fixture.intended().exists());
    let evidence = serde_json::to_value(assembled.artifacts()).expect("artifact evidence");
    assert_eq!(evidence.as_array().expect("ordered evidence").len(), 3);
    for (i, project, module, reference) in [
        (0, "project:two", "second-first", "library"),
        (1, "project:two", "second-first", "server"),
        (2, "project:one", "first-second", "library"),
    ] {
        assert_eq!(
            evidence[i],
            json!({"project":project,"module":module,"reference":reference,"source":"source:artifact","selection":{"kind":"file","path":"library.json"},"format":"typedoc-json","content":{"kind":"file","bytes":bytes.len(),"sha256":source::Sha256Digest::of(&bytes)},"tree":selected.inventory().identity()})
        );
        assert_eq!(
            file(
                assembled.output(),
                &format!("artifacts/{module}/{reference}.json")
            ),
            bytes
        );
    }
    let docs: Value = serde_json::from_slice(file(assembled.output(), "docs.json"))
        .expect("real target configuration");
    assert_eq!(docs["name"], "Configured publication");
    assert_eq!(
        docs["seo"]["metatags"]["canonical"],
        "https://docs.example.test/reference"
    );
    assert_typedoc_navigation(&docs);
    for group in project_dropdowns(&docs) {
        for reference in group["pages"].as_array().expect("references") {
            let sdk = &reference["sdk"];
            assert_eq!(sdk["format"], "typedoc");
            assert_eq!(
                file(
                    assembled.output(),
                    sdk["source"].as_str().expect("retained artifact path")
                ),
                bytes
            );
        }
    }
    let report = run(&config, None);
    assert!(
        report.status.success(),
        "{}",
        String::from_utf8_lossy(&report.stderr)
    );
    let report: Value = serde_json::from_slice(&report.stdout).expect("non-staging report");
    assert_eq!(report["status"], "documentation-assembled-local");
    assert!(report.get("directory").is_none_or(Value::is_null));
    assert_eq!(report["artifacts"], evidence);
    assert_eq!(
        report["inputs"],
        serde_json::to_value(inputs.inspect()).expect("exact acquisition evidence")
    );
    let parent = tempdir().expect("explicit staging parent");
    let staged =
        build::stage_documentation(&assembled, parent.path(), Default::default(), 16 << 10)
            .expect("SDK stage");
    assert_eq!(
        source::inspect(staged.path(), None, Default::default())
            .expect("read-back")
            .identity(),
        assembled.output().inventory().identity()
    );
    let output = run(&config, Some(parent.path()));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value = serde_json::from_slice(&output.stdout).expect("actual CLI receipt");
    assert_eq!(
        receipt["repository"],
        serde_json::to_value(assembled.repository()).expect("brand destination")
    );
    assert_eq!(receipt["artifacts"], evidence);
    let directory = Path::new(receipt["directory"].as_str().expect("actual stage"));
    assert_ne!(directory, fixture.intended());
    assert_eq!(
        source::inspect(directory, None, Default::default())
            .expect("CLI readback")
            .identity(),
        assembled.output().inventory().identity()
    );
    for entry in assembled.output().inventory().entries() {
        if matches!(entry.content, source::Content::File { .. }) {
            assert_eq!(
                fs::read(directory.join(entry.path.as_str())).expect("staged bytes"),
                assembled
                    .output()
                    .file(&entry.path)
                    .expect("retained bytes")
            );
        }
    }
    fs::remove_file(fixture.root.path().join("generated/library.json"))
        .expect("remove fixture source after capture");
    let repeated =
        build::assemble_documentation(&inputs, Default::default()).expect("no source reread");
    assert_eq!(
        repeated.output().inventory(),
        assembled.output().inventory()
    );
    assert_eq!(
        serde_json::to_value(repeated.artifacts()).expect("repeat evidence"),
        evidence
    );
    assert!(
        !fixture.intended().exists(),
        "assembly/staging never creates configured repository"
    );
}

#[test]
fn missing_or_malformed_selected_artifact_has_no_partial_report_or_stage() {
    for missing in [true, false] {
        let bytes = if missing { artifact() } else { b"{}".to_vec() };
        let fixture = Fixture::new(&bytes);
        // Two valid references precede the failing final module; never return
        // those earlier modules as a successful partial repository.
        let config = fixture.configuration().replacen(
            "selection={kind='file',path='library.json'}",
            "selection={kind='file',path='valid.json'}",
            2,
        );
        let config = if missing {
            config.replace(
                "selection={kind='file',path='library.json'}",
                "selection={kind='file',path='missing.json'}",
            )
        } else {
            config
        };
        let inputs = acquire(&config);
        let error = build::assemble_documentation(&inputs, Default::default())
            .expect_err("complete target fails");
        let expected = if missing {
            "documentation_artifact"
        } else {
            "documentation_target_input"
        };
        assert_eq!(error.code(), expected, "{error}");
        let parent = tempdir().expect("failure stage parent");
        fs::write(parent.path().join("sentinel"), b"untouched").expect("unrelated file");
        let before = source::inspect(parent.path(), None, Default::default()).expect("before");
        let output = run(&config, Some(parent.path()));
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains(expected));
        assert_eq!(
            source::inspect(parent.path(), None, Default::default())
                .expect("after")
                .identity(),
            before.identity()
        );
        assert!(!fixture.intended().exists());
    }
}

#[test]
fn documentation_stage_cannot_write_into_its_captured_input_repository() {
    let fixture = Fixture::new(&artifact());
    let config = fixture.configuration();
    let inputs = acquire(&config);
    let assembled = build::assemble_documentation(&inputs, Default::default()).expect("assembled");
    let before =
        source::inspect(fixture.root.path(), None, Default::default()).expect("native before");
    assert!(
        build::stage_documentation(
            &assembled,
            fixture.root.path(),
            Default::default(),
            16 << 10
        )
        .is_err()
    );
    let output = run(&config, Some(fixture.root.path()));
    assert!(!output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).expect("original stage failure");
    assert_eq!(report["failure"]["code"], "documentation_stage");
    assert!(report["target_unix_bytes"].is_null());
    assert_eq!(
        source::inspect(fixture.root.path(), None, Default::default())
            .expect("native after")
            .identity(),
        before.identity()
    );
    assert!(!fixture.intended().exists());
}

#[test]
fn imported_documentation_enforces_exact_native_stage_path_bounds_before_creation() {
    let fixture = Fixture::new(&artifact());
    let inputs = acquire(&fixture.configuration());
    let assembled =
        build::assemble_documentation(&inputs, Default::default()).expect("retained imports");
    let parent = tempdir().expect("owned output parent");
    let first = build::stage_documentation(&assembled, parent.path(), Default::default(), 16 << 10)
        .expect("bounded native stage");
    let exact = first.path().as_os_str().as_encoded_bytes().len();
    first.close().expect("remove only owned disposable stage");
    for bound in [0, exact - 1, (1 << 20) + 1] {
        assert!(
            build::stage_documentation(&assembled, parent.path(), Default::default(), bound)
                .is_err()
        );
        assert!(
            fs::read_dir(parent.path())
                .expect("independent no-effects readback")
                .next()
                .is_none()
        );
    }
    let exact_stage =
        build::stage_documentation(&assembled, parent.path(), Default::default(), exact)
            .expect("exact complete native path bound");
    assert_eq!(
        exact_stage.path().as_os_str().as_encoded_bytes().len(),
        exact
    );
    assert_eq!(
        source::inspect(exact_stage.path(), None, Default::default())
            .expect("actual bytes")
            .identity(),
        assembled.output().inventory().identity()
    );
    exact_stage
        .close()
        .expect("remove only owned disposable stage");
}

#[test]
fn lost_receipt_pipe_preserves_the_exact_stage_and_reports_its_recovery_path() {
    let fixture = Fixture::new(&artifact());
    let config = fixture.configuration();
    let inputs = acquire(&config);
    let expected = build::assemble_documentation(&inputs, Default::default()).expect("SDK output");
    let parent = tempdir().expect("fresh stage parent");
    let mut child = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args([
            "assemble-docs",
            "--local",
            "--site",
            "site:one",
            "--output-parent",
        ])
        .arg(parent.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("real CLI");
    drop(
        child
            .stdout
            .take()
            .expect("close receipt reader before dispatch"),
    );
    child
        .stdin
        .take()
        .expect("input")
        .write_all(config.as_bytes())
        .expect("workspace input");
    let result = child.wait_with_output().expect("terminal result");
    assert!(
        !result.status.success(),
        "a completed write with no receipt is not read-only success"
    );
    let entries = fs::read_dir(parent.path())
        .expect("stage parent")
        .map(|entry| entry.expect("entry").path())
        .collect::<Vec<_>>();
    assert_eq!(
        entries.len(),
        1,
        "one retained stage, no retry or silent cleanup"
    );
    let stderr = String::from_utf8(result.stderr).expect("diagnostic");
    let (diagnostic, payload) = stderr
        .split_once('\n')
        .expect("diagnostic and exact receipt");
    assert!(diagnostic.contains("output_io"), "{stderr}");
    let report: Value = serde_json::from_str(payload).expect("same complete receipt on stderr");
    assert_eq!(
        report["directory"],
        entries[0]
            .canonicalize()
            .expect("independent native locator")
            .to_str()
            .expect("fixture UTF-8")
    );
    assert_eq!(
        source::inspect(&entries[0], None, Default::default())
            .expect("native read-back")
            .identity(),
        expected.output().inventory().identity()
    );
    assert!(!fixture.intended().exists());
}
