//! Actual CLI/MCP source staging, retained output and independent native read-back.

use gazette_sdk::source::{self, input};
use git2::{Repository, Signature};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};
use tempfile::{TempDir, tempdir};

#[path = "support/mcp.rs"]
mod carrier;

struct Fixture {
    repository: TempDir,
    authored: TempDir,
    placement: Value,
}

impl Fixture {
    fn new() -> Self {
        let repository = tempdir().expect("project fixture");
        let repo = Repository::init(repository.path()).expect("native Git");
        fs::create_dir(repository.path().join("docs")).expect("project docs");
        fs::write(
            repository.path().join("docs/readme.md"),
            b"# Pinned project\n",
        )
        .expect("project bytes");
        let mut index = repo.index().expect("index");
        index
            .add_path(Path::new("docs/readme.md"))
            .expect("selected native file");
        index.write().expect("fixture index");
        let tree = repo
            .find_tree(index.write_tree().expect("tree OID"))
            .expect("native tree");
        let signature =
            Signature::new("Fixture", "fixture@example.invalid", &git2::Time::new(0, 0))
                .expect("fixed fixture signature");
        let commit = repo
            .commit(Some("HEAD"), &signature, &signature, "fixture", &tree, &[])
            .expect("local fixture commit");
        let authored = tempdir().expect("authored fixture");
        fs::write(authored.path().join("index.md"), b"# Authored page\n").expect("authored bytes");
        Self {
            placement: json!({"target":"projects/tool","project":"fixture-project","deployment":"fixture-deployment",
                "source":{"kind":"repository","root":repository.path(),"commit":commit.to_string(),"subdirectory":"docs"}}),
            repository,
            authored,
        }
    }

    fn request(&self, parent: &Path, mixed: bool) -> Value {
        json!({"allow_native_reads":true,"create_stage":true,"authored":self.authored.path(),
            "parent":parent,"workspace":"fixture-workspace","site":"fixture-site",
            "placements":if mixed {vec![self.placement.clone()]} else {vec![]},
            "preparation":{"sources":128,"entries":100_000,"bytes":1_073_741_824_u64,"per_source":{}},
            "limits":{},"directory_bytes":16384})
    }

    fn cli(&self, parent: &Path, mixed: bool) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_gazette"));
        command
            .arg("stage")
            .arg(self.authored.path())
            .arg(parent)
            .args(["--workspace", "fixture-workspace", "--site", "fixture-site"]);
        if mixed {
            command.arg("--placement").arg(self.placement.to_string());
        } else {
            command.arg("--authored-only");
        }
        command
    }
}

fn call(arguments: Value) -> Value {
    carrier::exchange(vec![json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"stage","arguments":arguments}})]).remove(0)
}

fn report(output: Output) -> Value {
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    serde_json::from_slice(&output.stdout).expect("complete CLI receipt")
}

fn readback(receipt: &Value) {
    assert_eq!(receipt["status"], "retained");
    let directory = Path::new(
        receipt["directory"]
            .as_str()
            .expect("native retained locator"),
    );
    let observed = report(
        Command::new(env!("CARGO_BIN_EXE_gazette"))
            .arg("inspect-source")
            .arg(directory)
            .output()
            .expect("independent native executable"),
    );
    assert_eq!(observed, receipt["inventory"]);
    assert_eq!(
        fs::read(directory.join("index.md")).expect("retained authored bytes"),
        b"# Authored page\n"
    );
}

#[test]
fn pinned_project_and_authored_sources_have_complete_cli_mcp_retention_parity() {
    let fixture = Fixture::new();
    let direct_parent = tempdir().expect("CLI destination");
    let protocol_parent = tempdir().expect("MCP destination");
    let project_before = source::inspect(fixture.repository.path(), None, Default::default())
        .expect("project before");
    let authored_before = source::inspect(fixture.authored.path(), None, Default::default())
        .expect("authored before");
    let direct = report(
        fixture
            .cli(direct_parent.path(), true)
            .args(["--create-stage", "--name", "selected"])
            .output()
            .expect("CLI stage"),
    );
    let mut arguments = fixture.request(protocol_parent.path(), true);
    arguments["name"] = json!("selected");
    let responses = carrier::exchange(vec![
        json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"stage","arguments":arguments}}),
    ]);
    let descriptor = responses[0]["result"]["tools"]
        .as_array()
        .expect("catalog")
        .iter()
        .find(|tool| tool["name"] == "stage")
        .expect("discoverable stage");
    jsonschema::validator_for(&descriptor["inputSchema"])
        .expect("input schema")
        .validate(&arguments)
        .expect("native input discoverable");
    let result = &responses[1]["result"];
    assert_ne!(result["isError"], true, "{}", responses[1]);
    let retained = &result["structuredContent"];
    jsonschema::validator_for(&descriptor["outputSchema"])
        .expect("output schema")
        .validate(retained)
        .expect("actual native receipt matches schema");
    assert_eq!(
        serde_json::from_str::<Value>(
            result["content"][0]["text"]
                .as_str()
                .expect("text projection")
        )
        .expect("complete JSON"),
        *retained
    );
    for field in ["status", "workspace", "site", "inventory"] {
        assert_eq!(retained[field], direct[field], "{field}");
    }
    let (context, requests) = input::bind(
        "fixture-workspace".into(),
        "fixture-site".into(),
        vec![input::parse(&fixture.placement.to_string()).expect("shared input")],
    )
    .expect("exact binding");
    let prepared = source::prepare(
        requests,
        source::PreparationLimits {
            sources: 128,
            entries: 100_000,
            bytes: 1 << 30,
            per_source: Default::default(),
        },
    )
    .expect("same native pin");
    let target = &prepared.placements()[0].request().target;
    let binding = context.binding(target).expect("original explicit binding");
    for (receipt, parent) in [
        (&direct, direct_parent.path()),
        (retained, protocol_parent.path()),
    ] {
        assert_eq!(
            Path::new(receipt["directory"].as_str().expect("directory")),
            parent
                .canonicalize()
                .expect("canonical parent")
                .join("selected")
        );
        readback(receipt);
        let directory = Path::new(receipt["directory"].as_str().expect("directory"));
        let manifest = format!("{target}/{}", source::GENERATED_TREE_MANIFEST);
        let pin = receipt["inventory"]["entries"]
            .as_array()
            .expect("complete inventory")
            .iter()
            .find(|entry| entry["path"] == manifest)
            .expect("generated manifest evidence")["sha256"]
            .as_str()
            .expect("manifest pin");
        let verified = report(
            Command::new(env!("CARGO_BIN_EXE_gazette"))
                .arg("verify-generated-tree")
                .arg(directory.join(target.as_str()))
                .arg("--expected-binding")
                .arg(serde_json::to_string(&binding).expect("exact binding JSON"))
                .args(["--manifest-sha256", pin])
                .output()
                .expect("independent generated-tree verifier"),
        );
        assert_eq!(
            verified["inventory"],
            json!(prepared.placements()[0].source().inventory())
        );
        assert_eq!(
            fs::read(directory.join("projects/tool/readme.md")).expect("pinned docs"),
            b"# Pinned project\n"
        );
    }
    assert_eq!(
        source::inspect(fixture.repository.path(), None, Default::default())
            .expect("project after"),
        project_before
    );
    assert_eq!(
        source::inspect(fixture.authored.path(), None, Default::default()).expect("authored after"),
        authored_before
    );
}

#[test]
fn permissions_collisions_and_changed_pins_do_not_create_replacement_or_partial_stages() {
    let fixture = Fixture::new();
    let parent = tempdir().expect("destination");
    let mut arguments = fixture.request(parent.path(), true);
    arguments["name"] = json!("selected");
    for field in ["allow_native_reads", "create_stage"] {
        let mut denied = arguments.clone();
        denied[field] = json!(false);
        let result = call(denied);
        assert_eq!(result["error"]["code"], -32602, "{result}");
        assert_eq!(fs::read_dir(parent.path()).expect("no effect").count(), 0);
    }
    let denied = fixture
        .cli(parent.path(), true)
        .output()
        .expect("CLI denied creation");
    assert!(!denied.status.success());
    assert!(denied.stdout.is_empty());
    assert_eq!(fs::read_dir(parent.path()).expect("no effect").count(), 0);
    let original = call(arguments.clone());
    assert_ne!(original["result"]["isError"], true, "{original}");
    let retained = &original["result"]["structuredContent"];
    let failed = call(arguments.clone());
    assert_eq!(failed["result"]["isError"], true, "{failed}");
    let collision = fixture
        .cli(parent.path(), true)
        .args(["--create-stage", "--name", "selected"])
        .output()
        .expect("CLI named collision");
    assert!(!collision.status.success());
    assert!(collision.stdout.is_empty());
    assert_eq!(fs::read_dir(parent.path()).expect("no fallback").count(), 1);
    readback(retained);
    fs::write(
        fixture.repository.path().join("docs/readme.md"),
        b"changed since configured pin",
    )
    .expect("intentional source drift");
    arguments["name"] = json!("must-not-exist");
    let failed = call(arguments);
    assert_eq!(failed["result"]["isError"], true, "{failed}");
    assert!(!parent.path().join("must-not-exist").exists());
    assert_eq!(fs::read_dir(parent.path()).expect("no partial").count(), 1);
    readback(retained);
}

#[test]
fn authored_only_fresh_staging_is_explicit_and_lost_cli_receipt_keeps_native_output() {
    let fixture = Fixture::new();
    let parent = tempdir().expect("MCP destination");
    let arguments = fixture.request(parent.path(), false);
    let mut missing = arguments.clone();
    missing
        .as_object_mut()
        .expect("request")
        .remove("placements");
    assert_eq!(call(missing)["error"]["code"], -32602);
    assert_eq!(
        fs::read_dir(parent.path())
            .expect("no implicit selection")
            .count(),
        0
    );
    let response = call(arguments);
    assert_ne!(response["result"]["isError"], true, "{response}");
    let retained = &response["result"]["structuredContent"];
    readback(retained);
    assert_eq!(
        retained["inventory"]["entries"]
            .as_array()
            .expect("authored only")
            .len(),
        1
    );
    let direct_parent = tempdir().expect("CLI destination");
    let mut child = fixture
        .cli(direct_parent.path(), false)
        .args(["--create-stage", "--name", "lost-receipt"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("CLI effectful operation");
    drop(child.stdout.take());
    let output = child.wait_with_output().expect("terminal operation");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("receipt delivery failed"));
    let directory = direct_parent.path().join("lost-receipt");
    assert!(
        directory.is_dir(),
        "lost delivery cannot erase retained effect"
    );
    assert_eq!(
        json!(
            source::inspect(&directory, None, Default::default())
                .expect("independent readback after delivery loss")
        ),
        retained["inventory"]
    );
}

#[test]
fn broken_mcp_output_with_open_input_preserves_the_once_retained_named_stage() {
    let fixture = Fixture::new();
    let parent = tempdir().expect("MCP destination");
    let expected = source::inspect(fixture.authored.path(), None, Default::default())
        .expect("independent authored-only inventory before dispatch");
    let mut arguments = fixture.request(parent.path(), false);
    arguments["name"] = json!("mcp-lost-receipt");
    let request = json!({
        "jsonrpc":"2.0","id":1,"method":"tools/call","params":{
            "name":"stage","arguments":arguments,
            "_meta":{
                "io.modelcontextprotocol/protocolVersion":"2026-07-28",
                "io.modelcontextprotocol/clientCapabilities":{}
            }
        }
    });
    let mut child = carrier::Process(
        Command::new(env!("CARGO_BIN_EXE_gazette"))
            .arg("mcp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("production stateless MCP process"),
    );
    let mut input = child.0.stdin.take().expect("input producer stays open");
    drop(child.0.stdout.take());
    // Exactly one valid stateless request: no handshake response can fail before
    // dispatch. Named output plus full independent read-back proves that dispatch
    // reached retention; process failure alone would not establish that boundary.
    serde_json::to_writer(&mut input, &request).expect("one exact request");
    input.write_all(b"\n").expect("request frame");
    input.flush().expect("request flushed");
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.0.try_wait().expect("process status") {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "broken MCP output must terminate despite open input"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(
        !status.success(),
        "lost carrier must not claim service success"
    );
    // EOF is deliberately supplied only after failure has already been observed.
    drop(input);
    let mut stderr = String::new();
    child
        .0
        .stderr
        .take()
        .expect("stderr")
        .read_to_string(&mut stderr)
        .expect("terminal diagnostic");
    assert!(stderr.contains("MCP transport failed"), "{stderr}");
    assert!(
        stderr.contains("do not infer rollback or retry"),
        "{stderr}"
    );
    let directory = parent.path().join("mcp-lost-receipt");
    assert!(
        directory.is_dir(),
        "a transport failure alone is not proof of retained effects"
    );
    assert_eq!(
        fs::read_dir(parent.path())
            .expect("one native effect")
            .count(),
        1
    );
    let readback = report(
        Command::new(env!("CARGO_BIN_EXE_gazette"))
            .arg("inspect-source")
            .arg(&directory)
            .output()
            .expect("independent native read-back after MCP loss"),
    );
    assert_eq!(readback, json!(expected));
    assert_eq!(
        fs::read(directory.join("index.md")).expect("retained authored bytes"),
        b"# Authored page\n"
    );
    assert_eq!(
        source::inspect(fixture.authored.path(), None, Default::default())
            .expect("unchanged source"),
        expected
    );
}
