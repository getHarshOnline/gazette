//! Actual initialized MCP documentation build inputs, native output and durable history.
//! CLI and MCP reuse one retained native documentation and configured-output sequence.

#[path = "support/mcp.rs"]
#[allow(dead_code)] // Shared carrier also has stateless helpers used by other targets.
mod mcp_carrier;

use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Command, Output, Stdio},
};

use gazette_sdk::{build, configuration, source, store};
use serde_json::{Value, json};
use tempfile::TempDir;

const MANUAL: &[u8] = b"# Start\n\nThe retained authored manual.\n";
const REFERENCE: &[u8] =
    br#"{"id":0,"name":"Retained API","kind":1,"variant":"project","schemaVersion":"2.0"}"#;
const RUSTDOC_LIBRARY: &[u8] =
    include_bytes!("../../../adapters/documentation/tests/fixtures/rustdoc/library.rs");

struct Fixture {
    source: TempDir,
    owner: TempDir,
}

impl Fixture {
    fn new(bulk: usize) -> Self {
        let source = tempfile::tempdir().unwrap();
        let owner = tempfile::tempdir().unwrap();
        fs::write(source.path().join("start.md"), MANUAL).unwrap();
        fs::write(source.path().join("api.json"), REFERENCE).unwrap();
        fs::create_dir(owner.path().join("domain-doc")).unwrap();
        let mut config = format!(
            "[[principals]]\nid='person:publisher'\nkind='person'\n\
             [[sites]]\nid='site:docs'\npublisher='person:publisher'\ndeployable=false\n\
             [[projects]]\nid='project:one'\nlifecycle='active'\nvisibility='public'\navailability='available'\n\
             [[sources]]\nid='source:manual'\nlocation={{kind='filesystem',root={:?}}}\n\
             [[brands]]\nid='brand:docs'\nname='Independent documentation brand'\n\
             [[brands.publications]]\nsite='site:docs'\n\
             [brands.publications.documentation_repository]\nname='native-domain-doc'\npath='domain-doc'\n\
             [brands.publications.documentation_repository.build]\n\
             target={{kind='mintlify',theme='mint',primary_color='#123abc'}}\n\
             [[brands.publications.documentation_repository.build.modules]]\n\
             project='project:one'\nslug='module'\ntitle='Module'\nmanual={{source='source:manual'}}\n\
             navigation=[{{kind='manual',title='Manual',pages=[{{title='Start',route='manual/start',input={{kind='file',path='start.md'}}}}]}},\
             {{kind='reference',slug='reference',title='API',input={{kind='artifact',source='source:manual',selection={{kind='file',path='api.json'}},format='typedoc-json'}}}}",
            source.path().to_str().unwrap(),
        );
        if bulk != 0 {
            fs::create_dir(source.path().join("bulk")).unwrap();
            for index in 0..bulk {
                fs::write(
                    source.path().join("bulk").join(bulk_name(index)),
                    b"# Retained reference\n\nNative imported Markdown.\n",
                )
                .unwrap();
            }
            config.push_str(",{kind='reference',slug='bulk',title='Retained directory',input={kind='artifact',source='source:manual',selection={kind='directory',path='bulk'},format='markdown'}}");
        }
        config.push_str("]\n");
        fs::write(owner.path().join("gazette.toml"), config).unwrap();
        Self { source, owner }
    }

    fn root(&self) -> std::path::PathBuf {
        self.owner.path().join("domain-doc")
    }

    fn documentation(&self) -> Value {
        json!({"workspace":{"kind":"local","root":self.owner.path(),"workspace":"gazette.toml"},
            "site":"site:docs","local":true,"allow_read":true,"directory_bytes":1024})
    }

    fn inputs(&self) -> build::AcquiredInputs {
        let loaded = configuration::load_workspace(
            self.owner.path(),
            &"gazette.toml".parse().unwrap(),
            Default::default(),
        )
        .unwrap();
        build::acquire_documentation_workspace(
            loaded,
            &"site:docs".parse().unwrap(),
            build::PublicationSetAcquisitionLimits::default().sources,
        )
        .unwrap()
    }
}

fn bulk_name(index: usize) -> String {
    format!("{index:04}-{}.md", "reference".repeat(16))
}

fn overflowing_count(maximum: usize) -> usize {
    let sample = source::Entry {
        path: format!("bulk/{}", bulk_name(0)).parse().unwrap(),
        content: source::Content::File {
            bytes: 48,
            sha256: source::Sha256Digest::of(b"native reference"),
        },
    };
    let count = maximum / serde_json::to_vec(&sample).unwrap().len() + 1;
    assert!(count < 2048, "bounded imported reference directory");
    count
}

fn session() -> store::SessionOptions {
    store::SessionOptions {
        allow_local_reservation: true,
        read: store::ReadOptions {
            allow_coordination_files: true,
            ..Default::default()
        },
    }
}

fn initialize() -> (TempDir, store::InitializationReceipt) {
    let root = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let receipt = store::initialize(
        root.path(),
        store::InitializeOptions {
            allow_create: true,
            session: session(),
            ..Default::default()
        },
    )
    .unwrap();
    (root, receipt)
}

fn options() -> build::MaterializationOptions {
    build::MaterializationOptions {
        allow_write: true,
        history: store::MaterializationWriteOptions { allow_record: true },
        ..Default::default()
    }
}

fn call(send: &mut dyn FnMut(Value) -> Value, id: u64, name: &str, arguments: Value) -> Value {
    let response = send(
        json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":arguments}}),
    );
    assert!(response.get("error").is_none(), "{response}");
    let result = response["result"].clone();
    let text: Value = serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(text, result["structuredContent"]);
    result
}

fn open(
    send: &mut dyn FnMut(Value) -> Value,
    id: u64,
    root: &Path,
    expected: store::JournalHeader,
) -> (Value, store::JournalHeader) {
    let result = call(
        send,
        id,
        "open_store_session",
        json!({"request":{"kind":"writer","root":root,
        "expected_header":expected,"allow_native_reads":true,
        "options":store::WriterOptions {allow_generation_advance:true,session:session()}}}),
    );
    assert_eq!(result["isError"], false, "{result}");
    let ready = &result["structuredContent"];
    let header: store::JournalHeader =
        serde_json::from_value(ready["inspection"]["header"].clone()).unwrap();
    assert_eq!(header.writer_fence, expected.writer_fence + 1);
    (ready["session"].clone(), header)
}

fn close(send: &mut dyn FnMut(Value) -> Value, id: u64, handle: &Value) {
    let result = call(send, id, "close_store_session", json!({"session":handle}));
    assert_eq!(result["isError"], false, "{result}");
    assert_eq!(result["structuredContent"]["status"], "closed");
    assert_eq!(result["structuredContent"]["active"], false);
}

fn request(documentation: Value, handle: &Value, id: source::Sha256Digest) -> Value {
    json!({"session":handle,"documentation":documentation,"id":id,"allow_write":true,"allow_record":true})
}

fn tool(send: &mut dyn FnMut(Value) -> Value) -> Value {
    let catalog = send(json!({"jsonrpc":"2.0","id":0,"method":"tools/list","params":{}}));
    catalog["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "materialize_docs")
        .expect("native registered configured-documentation route")
        .clone()
}

fn validate(tool: &Value, arguments: &Value, result: &Value) {
    jsonschema::validator_for(&tool["inputSchema"])
        .unwrap()
        .validate(arguments)
        .unwrap();
    jsonschema::validator_for(&tool["outputSchema"])
        .unwrap()
        .validate(&result["structuredContent"])
        .unwrap();
}

fn snapshot(root: &Path) -> source::TreeIdentity {
    source::inspect(root, None, Default::default())
        .unwrap()
        .identity()
}

fn history(
    database: &Path,
    root: &Path,
    header: store::JournalHeader,
) -> store::MaterializationInspection {
    store::inspect_materialization(
        database,
        &root.canonicalize().unwrap(),
        Some(&header),
        session().read,
    )
    .unwrap()
}

const CLI_PERMISSIONS: [&str; 7] = [
    "--local",
    "--allow-read",
    "--allow-write-output",
    "--allow-record-history",
    "--advance-writer-generation",
    "--reserve-local-coordination",
    "--allow-coordination-files",
];

fn cli_arguments(
    root: &Path,
    header: store::JournalHeader,
    id: source::Sha256Digest,
) -> Vec<String> {
    let mut args = vec![
        "materialize-docs".into(),
        root.to_str().unwrap().into(),
        "--expected-header".into(),
        serde_json::to_string(&header).unwrap(),
        "--operation".into(),
        id.to_string(),
        "--site".into(),
        "site:docs".into(),
        "--workspace-json".into(),
    ];
    args.extend(CLI_PERMISSIONS.map(str::to_owned));
    args
}

fn cli(arguments: &[String], input: &Value) -> Output {
    let cwd = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args(arguments)
        .current_dir(cwd.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(input).unwrap())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn frames(output: &Output) -> Vec<Value> {
    std::str::from_utf8(&output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn cli_header(
    frames: &[Value],
    expected: store::JournalHeader,
    root: &Path,
    id: source::Sha256Digest,
) -> store::JournalHeader {
    assert_eq!(
        frames.len(),
        3,
        "reserved, acquired, complete or custody outcome"
    );
    assert_eq!(frames[0]["event"], "reserved");
    assert_eq!(frames[0]["close"]["status"], "not_acquired");
    assert_eq!(frames[1]["event"], "acquired");
    assert_eq!(frames[1]["close"]["status"], "held");
    assert_eq!(frames[2]["close"]["status"], "closed");
    let header: store::JournalHeader =
        serde_json::from_value(frames[1]["custody"]["acquired_header"].clone()).unwrap();
    assert_eq!(header.writer_fence, expected.writer_fence + 1);
    for frame in frames {
        assert_eq!(frame["custody"]["root"], json!(root));
        assert_eq!(frame["custody"]["expected_header"], json!(expected));
        assert_eq!(frame["custody"]["operations"], json!({"site:docs":id}));
        assert_eq!(
            frame["custody"]["request_sha256"],
            frames[0]["custody"]["request_sha256"]
        );
    }
    assert_eq!(frames[2]["custody"]["acquired_header"], json!(header));
    header
}

fn workspace_input(fixture: &Fixture) -> Value {
    fixture.documentation()["workspace"].clone()
}

fn native_report_size(
    inputs: &build::AcquiredInputs,
    assembly: &build::DocumentationAssembly<'_>,
) -> usize {
    serde_json::to_vec(&build::documentation_output::Report {
        preview_link_bases: assembly.preview_link_bases(),
        status: build::documentation_output::Status::AssembledLocal,
        inputs: inputs.inspect(),
        repository: assembly.repository(),
        manual: assembly.manual(),
        unselected_manual_pages: assembly.unselected_manual_pages(),
        unselected_manual_pages_incomplete: assembly.unselected_manual_pages_incomplete(),
        artifacts: assembly.artifacts(),
        inventory: assembly.output().inventory(),
        directory: None,
        jobs: &[],
    })
    .unwrap()
    .len()
}

#[test]
fn initialized_mcp_local_owner_materializes_exact_manual_reference_and_reopens_history() {
    let fixture = Fixture::new(0);
    let root = fixture.root();
    let git = git2::Repository::init(&root).unwrap();
    fs::write(root.join("user.txt"), b"independent owner bytes").unwrap();
    let git_before = snapshot(git.path());
    let source_before = snapshot(fixture.source.path());
    let config_before = fs::read(fixture.owner.path().join("gazette.toml")).unwrap();
    let inputs = fixture.inputs();
    let assembly = build::assemble_documentation(&inputs, Default::default()).unwrap();
    assert_eq!(assembly.manual().len(), 1);
    assert_eq!(assembly.artifacts().len(), 1);
    let (journal, initialized) = initialize();
    let first = source::Sha256Digest::of(b"mcp-original-documentation");
    let second = source::Sha256Digest::of(b"mcp-next-documentation");
    let header = mcp_carrier::initialized_interact("2026-07-28", |initialized_protocol, send| {
        assert_eq!(
            initialized_protocol["result"]["protocolVersion"],
            "2026-07-28"
        );
        let descriptor = tool(send);
        assert_eq!(descriptor["annotations"]["readOnlyHint"], false);
        assert_eq!(
            descriptor["_meta"]["io.rtepl.gazette/terminalResultOwner"],
            "close_store_session"
        );
        let (handle, header) = open(send, 1, journal.path(), initialized.header);
        let arguments = request(fixture.documentation(), &handle, first);
        let digest = source::Sha256Digest::of(&serde_json::to_vec(&arguments).unwrap());
        let result = call(send, 2, "materialize_docs", arguments.clone());
        validate(&descriptor, &arguments, &result);
        assert_eq!(result["isError"], false, "{result}");
        let report = &result["structuredContent"];
        assert_eq!(report["status"], "materialized");
        assert_eq!(report["session"], handle);
        assert_eq!(report["active"], true);
        assert_eq!(report["acquired_header"], json!(header));
        assert_eq!(report["request_sha256"], json!(digest));
        assert_eq!(report["id"], json!(first));
        assert_eq!(
            report["documentation"]["status"],
            "documentation-assembled-local"
        );
        assert_eq!(
            report["documentation"]["jobs"],
            json!([]),
            "captured artifacts invent no generator jobs"
        );
        assert_eq!(report["documentation"]["manual"], json!(assembly.manual()));
        assert_eq!(
            report["documentation"]["artifacts"],
            json!(assembly.artifacts())
        );
        assert_eq!(
            report["documentation"]["inventory"],
            json!(assembly.output().inventory())
        );
        assert_eq!(report["materialization"]["output"]["root"], json!(root));
        assert_eq!(
            report["materialization"]["output"]["generated"],
            json!(assembly.output().inventory())
        );
        close(send, 3, &handle);
        let first_history = history(&initialized.database, &root, header);
        assert_eq!(first_history.frontier.head.unwrap().intent.id, first);
        let (next_handle, next_header) = open(send, 4, journal.path(), header);
        assert_ne!(
            next_handle, handle,
            "new explicit lifetime, not silent reacquisition"
        );
        let next_arguments = request(fixture.documentation(), &next_handle, second);
        let repeated = call(send, 5, "materialize_docs", next_arguments.clone());
        validate(&descriptor, &next_arguments, &repeated);
        assert_eq!(repeated["isError"], false, "{repeated}");
        assert_eq!(
            repeated["structuredContent"]["materialization"]["history"]["record"]["intent"]["previous"],
            json!(first)
        );
        assert!(
            repeated["structuredContent"]["materialization"]["output"].is_object(),
            "new original no-op readback is not historical replay"
        );
        close(send, 6, &next_handle);
        next_header
    });
    let reopened = history(&initialized.database, &root, header);
    assert!(reopened.frontier.active.is_none());
    let record = reopened.frontier.head.unwrap();
    assert_eq!(record.intent.id, second);
    record
        .completion
        .as_ref()
        .unwrap()
        .validate(&record.intent)
        .unwrap();
    let evidence: Value = serde_json::from_str(&record.intent.evidence).unwrap();
    assert_eq!(
        evidence["format"],
        "gazette.documentation-materialization/1"
    );
    assert_eq!(evidence["artifact_class"], "documentation-build-input");
    assert_eq!(evidence["brand"], "brand:docs");
    assert_eq!(evidence["manual"], json!(assembly.manual()));
    assert_eq!(evidence["artifacts"], json!(assembly.artifacts()));
    for entry in assembly.output().inventory().entries() {
        if let Some(bytes) = assembly.output().file(&entry.path) {
            assert_eq!(fs::read(root.join(entry.path.as_str())).unwrap(), bytes);
        }
    }
    assert!(
        fs::read_to_string(root.join("manual/start.md"))
            .unwrap()
            .contains("retained authored manual")
    );
    assert_eq!(
        fs::read(root.join("user.txt")).unwrap(),
        b"independent owner bytes"
    );
    assert_eq!(snapshot(git.path()), git_before);
    assert_eq!(snapshot(fixture.source.path()), source_before);
    assert_eq!(
        fs::read(fixture.owner.path().join("gazette.toml")).unwrap(),
        config_before
    );
}

#[test]
fn initialized_mcp_unknown_and_independent_permissions_precede_missing_local_workspace() {
    let fixture = Fixture::new(0);
    let root = fixture.root();
    fs::write(root.join("user.txt"), b"untouched user").unwrap();
    fs::write(root.join(".git"), b"gitdir: ../separate-admin\n").unwrap();
    let before = snapshot(&root);
    let source_before = snapshot(fixture.source.path());
    let mut documentation = fixture.documentation();
    documentation["workspace"]["root"] = json!(fixture.owner.path().join("absent-owner"));
    let id = source::Sha256Digest::of(b"refused-docs");
    let (journal, initialized) = initialize();
    let header = mcp_carrier::initialized_interact("2026-07-28", |_, send| {
        let descriptor = tool(send);
        let arguments = request(documentation.clone(), &json!("0".repeat(64)), id);
        let unknown = call(send, 1, "materialize_docs", arguments.clone());
        validate(&descriptor, &arguments, &unknown);
        assert_eq!(unknown["isError"], true);
        assert_eq!(
            unknown["structuredContent"]["code"],
            "store_session_unknown"
        );
        let (handle, header) = open(send, 2, journal.path(), initialized.header);
        let database_before = fs::read(&initialized.database).unwrap();
        for (index, field) in ["allow_write", "allow_record", "local", "allow_read"]
            .into_iter()
            .enumerate()
        {
            let mut arguments = request(documentation.clone(), &handle, id);
            if matches!(field, "local" | "allow_read") {
                arguments["documentation"][field] = json!(false);
            } else {
                arguments[field] = json!(false);
            }
            let digest = source::Sha256Digest::of(&serde_json::to_vec(&arguments).unwrap());
            let result = call(
                send,
                3 + index as u64,
                "materialize_docs",
                arguments.clone(),
            );
            validate(&descriptor, &arguments, &result);
            assert_eq!(result["isError"], true, "{result}");
            let report = &result["structuredContent"];
            assert_eq!(report["status"], "not_attempted");
            assert_eq!(report["documentation"], Value::Null);
            assert_eq!(report["session"], handle);
            assert_eq!(report["active"], true);
            assert_eq!(report["id"], json!(id));
            assert_eq!(report["request_sha256"], json!(digest));
            if matches!(field, "local" | "allow_read") {
                assert_eq!(report["failure"]["phase"], "request");
                assert_eq!(report["failure"]["code"], "documentation_permission");
            } else {
                assert_eq!(report["failure"]["phase"], "permission");
                assert_eq!(
                    report["failure"]["failure"]["code"],
                    if field == "allow_write" {
                        "materialization_permission"
                    } else {
                        "store_materialization_permission"
                    }
                );
            }
            assert_eq!(fs::read(&initialized.database).unwrap(), database_before);
            assert_eq!(snapshot(&root), before);
        }
        close(send, 7, &handle);
        header
    });
    let inspection = history(&initialized.database, &root, header);
    assert!(inspection.frontier.head.is_none() && inspection.frontier.active.is_none());
    assert_eq!(snapshot(&root), before);
    assert_eq!(snapshot(fixture.source.path()), source_before);
}

#[test]
fn initialized_mcp_oversized_native_documentation_custody_never_dispatches_output() {
    const MAXIMUM: usize = 256 * 1024;
    // One actual native documentation report includes at least this many full
    // inventory entries; the fixture's independent report proves the bound below.
    let fixture = Fixture::new(overflowing_count(MAXIMUM));
    let root = fixture.root();
    fs::write(root.join("user.txt"), b"untouched while delivery fails").unwrap();
    let before = snapshot(&root);
    let inputs = fixture.inputs();
    let assembly = build::assemble_documentation(&inputs, Default::default()).unwrap();
    let native_report = build::documentation_output::Report {
        preview_link_bases: assembly.preview_link_bases(),
        status: build::documentation_output::Status::AssembledLocal,
        inputs: inputs.inspect(),
        repository: assembly.repository(),
        manual: assembly.manual(),
        unselected_manual_pages: assembly.unselected_manual_pages(),
        unselected_manual_pages_incomplete: assembly.unselected_manual_pages_incomplete(),
        artifacts: assembly.artifacts(),
        inventory: assembly.output().inventory(),
        directory: None,
        jobs: &[],
    };
    assert!(
        serde_json::to_vec(&native_report).unwrap().len() > MAXIMUM,
        "connected native full report exceeds the unchanged carrier cap"
    );
    let (journal, initialized) = initialize();
    let id = source::Sha256Digest::of(b"native-docs-delivery-overflow");
    let header = mcp_carrier::initialized_interact_with_call_timeout(
        "2026-07-28",
        std::time::Duration::from_secs(30),
        |_, send| {
            let descriptor = tool(send);
            let (handle, header) = open(send, 1, journal.path(), initialized.header);
            let database_before = fs::read(&initialized.database).unwrap();
            let arguments = request(fixture.documentation(), &handle, id);
            let digest = source::Sha256Digest::of(&serde_json::to_vec(&arguments).unwrap());
            let result = call(send, 2, "materialize_docs", arguments.clone());
            validate(&descriptor, &arguments, &result);
            assert_eq!(
                result["isError"], true,
                "native compact result is not assembly permission: {result}"
            );
            let report = &result["structuredContent"];
            assert_eq!(report["status"], "not_attempted");
            assert_eq!(report["failure"]["phase"], "documentation_delivery");
            assert_eq!(
                report["documentation"]["code"],
                "documentation_response_unavailable"
            );
            assert_eq!(
                report["documentation"]["completed"],
                json!([]),
                "imports invent no native jobs"
            );
            assert_eq!(
                report["documentation"]["output_identity"],
                json!(assembly.output().inventory().identity())
            );
            assert_eq!(report["session"], handle);
            assert_eq!(report["active"], true);
            assert_eq!(report["acquired_header"], json!(header));
            assert_eq!(report["request_sha256"], json!(digest));
            assert_eq!(report["id"], json!(id));
            assert_eq!(fs::read(&initialized.database).unwrap(), database_before);
            assert_eq!(snapshot(&root), before);
            close(send, 3, &handle);
            header
        },
    );
    let inspection = history(&initialized.database, &root, header);
    assert!(inspection.frontier.head.is_none() && inspection.frontier.active.is_none());
    assert_eq!(snapshot(&root), before);
}

#[test]
fn initialized_mcp_preserves_native_pending_intent_without_implicit_retry() {
    let fixture = Fixture::new(0);
    let root = fixture.root();
    fs::write(root.join(".git"), b"gitdir: ../separate-admin\n").unwrap();
    fs::write(root.join("user.txt"), b"independent user").unwrap();
    let inputs = fixture.inputs();
    let assembly = build::assemble_documentation(&inputs, Default::default()).unwrap();
    let (journal, initialized) = initialize();
    let mut writer = store::acquire_writer(
        journal.path(),
        initialized.header,
        store::WriterOptions {
            allow_generation_advance: true,
            session: session(),
        },
    )
    .unwrap();
    let plan = writer
        .plan_documentation_materialization(
            &assembly,
            &[],
            Default::default(),
            options().evidence_bytes,
        )
        .unwrap();
    let pending = source::Sha256Digest::of(b"original pending native documentation");
    // A real native change after observation retains intent but refuses output;
    // reopening that lifetime is not authority to replace the pending operation.
    fs::write(root.join("new-user.txt"), b"concurrent native bytes").unwrap();
    let failure = writer.materialize(plan, pending, options()).unwrap_err();
    let build::MaterializationError::Write { id, cause } = failure else {
        panic!("exact native output phase")
    };
    assert_eq!(id, pending);
    assert!(cause.attempted.is_empty() && cause.residue.is_none());
    let previous_header = writer.inspect().unwrap().header;
    writer.close().unwrap();
    let before = snapshot(&root);
    let next_id = source::Sha256Digest::of(b"not an implicit documentation retry");
    let header = mcp_carrier::initialized_interact("2026-07-28", |_, send| {
        let descriptor = tool(send);
        let (handle, header) = open(send, 1, journal.path(), previous_header);
        let arguments = request(fixture.documentation(), &handle, next_id);
        let digest = source::Sha256Digest::of(&serde_json::to_vec(&arguments).unwrap());
        let result = call(send, 2, "materialize_docs", arguments.clone());
        validate(&descriptor, &arguments, &result);
        assert_eq!(result["isError"], true, "{result}");
        let report = &result["structuredContent"];
        assert_eq!(report["status"], "plan_failed");
        assert_eq!(report["failure"]["code"], "store_materialization_pending");
        assert_eq!(
            report["documentation"]["status"],
            "documentation-assembled-local"
        );
        assert_eq!(
            report["documentation"]["inventory"],
            json!(assembly.output().inventory())
        );
        assert_eq!(report["session"], handle);
        assert_eq!(report["active"], true);
        assert_eq!(report["acquired_header"], json!(header));
        assert_eq!(report["request_sha256"], json!(digest));
        assert_eq!(report["id"], json!(next_id));
        assert_eq!(snapshot(&root), before);
        close(send, 3, &handle);
        header
    });
    let inspection = history(&initialized.database, &root, header);
    assert!(inspection.frontier.head.is_none());
    let active = inspection.frontier.active.unwrap();
    assert_eq!(active.intent.id, pending);
    assert!(active.completion.is_none());
    assert_eq!(snapshot(&root), before);
    assert!(!root.join("docs.json").exists());
}

#[test]
fn cli_local_owner_matches_native_sdk_and_initialized_mcp_with_explicit_close() {
    let fixture = Fixture::new(0);
    let root = fixture.root();
    let git = git2::Repository::init(&root).unwrap();
    fs::write(root.join("user.txt"), b"independent configured owner").unwrap();
    let git_before = snapshot(git.path());
    let source_before = snapshot(fixture.source.path());
    let inputs = fixture.inputs();
    let assembly = build::assemble_documentation(&inputs, Default::default()).unwrap();
    let (journal, initialized) = initialize();
    let id = source::Sha256Digest::of(b"actual configured documentation CLI");
    let output = cli(
        &cli_arguments(journal.path(), initialized.header, id),
        &workspace_input(&fixture),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let frames = frames(&output);
    let header = cli_header(&frames, initialized.header, journal.path(), id);
    let result = &frames[2];
    assert_eq!(result["event"], "result");
    assert_eq!(result["details_complete"], true);
    assert_eq!(result["detail"]["outcome"]["status"], "materialized");
    assert_eq!(
        result["detail"]["documentation"]["status"],
        "documentation-assembled-local"
    );
    assert_eq!(result["detail"]["documentation"]["jobs"], json!([]));
    assert_eq!(
        result["detail"]["documentation"]["manual"],
        json!(assembly.manual())
    );
    assert_eq!(
        result["detail"]["documentation"]["artifacts"],
        json!(assembly.artifacts())
    );
    assert_eq!(
        result["detail"]["documentation"]["inventory"],
        json!(assembly.output().inventory())
    );
    assert_eq!(
        result["detail"]["outcome"]["materialization"]["output"]["generated"],
        json!(assembly.output().inventory())
    );
    let original = history(&initialized.database, &root, header);
    assert!(original.frontier.active.is_none());
    assert_eq!(original.frontier.head.unwrap().intent.id, id);
    let next_id = source::Sha256Digest::of(b"same retained documentation through initialized MCP");
    let final_header = mcp_carrier::initialized_interact("2026-07-28", |_, send| {
        let descriptor = tool(send);
        let (handle, acquired) = open(send, 1, journal.path(), header);
        let arguments = request(fixture.documentation(), &handle, next_id);
        let mcp = call(send, 2, "materialize_docs", arguments.clone());
        validate(&descriptor, &arguments, &mcp);
        assert_eq!(mcp["isError"], false, "{mcp}");
        assert_eq!(
            mcp["structuredContent"]["documentation"],
            result["detail"]["documentation"]
        );
        assert_eq!(
            mcp["structuredContent"]["materialization"]["history"]["record"]["intent"]["previous"],
            json!(id)
        );
        close(send, 3, &handle);
        acquired
    });
    let head = history(&initialized.database, &root, final_header)
        .frontier
        .head
        .unwrap();
    assert_eq!(head.intent.id, next_id);
    head.completion
        .as_ref()
        .unwrap()
        .validate(&head.intent)
        .unwrap();
    for entry in assembly.output().inventory().entries() {
        if let Some(bytes) = assembly.output().file(&entry.path) {
            assert_eq!(fs::read(root.join(entry.path.as_str())).unwrap(), bytes);
        }
    }
    assert_eq!(
        fs::read(root.join("user.txt")).unwrap(),
        b"independent configured owner"
    );
    assert_eq!(snapshot(git.path()), git_before);
    assert_eq!(snapshot(fixture.source.path()), source_before);
}

#[test]
fn cli_preview_maps_reject_duplicate_decoded_keys_before_native_acquisition() {
    let fixture = Fixture::new(0);
    let input = workspace_input(&fixture);
    let before = snapshot(&fixture.root());
    let valid = r#"{"https://docs.example.test/":"http://127.0.0.1:4100/"}"#;
    let arguments = |map: &str| {
        vec![
            "assemble-docs".into(),
            "--local".into(),
            "--site".into(),
            "site:docs".into(),
            "--workspace-json".into(),
            "--preview-link-bases-json".into(),
            map.into(),
        ]
    };
    let accepted = cli(&arguments(valid), &input);
    assert!(
        accepted.status.success(),
        "{}",
        String::from_utf8_lossy(&accepted.stderr)
    );
    let report: Value = serde_json::from_slice(&accepted.stdout).unwrap();
    assert_eq!(
        report["preview_link_bases"],
        serde_json::from_str::<Value>(valid).unwrap()
    );
    let mut unavailable = input;
    unavailable["root"] = json!(fixture.owner.path().join("unavailable-owner"));
    for map in [
        r#"{"https://docs.example.test/":"http://192.0.2.1:4100/","https://docs.example.test/":"http://127.0.0.1:4100/"}"#,
        r#"{"https://docs.example.test/":"http://127.0.0.1:4100/","https://docs.example.test\u002f":"http://127.0.0.1:4101/"}"#,
    ] {
        let refused = cli(&arguments(map), &unavailable);
        assert!(!refused.status.success());
        assert!(refused.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&refused.stderr).contains("documentation_preview_link_bases")
        );
        assert_eq!(snapshot(&fixture.root()), before);
    }
}

#[test]
fn cli_permissions_and_combined_response_reservation_refuse_before_generation_or_configuration() {
    let fixture = Fixture::new(0);
    let root = fixture.root();
    fs::write(root.join("user.txt"), b"no output dispatch").unwrap();
    let before = snapshot(&root);
    let (journal, initialized) = initialize();
    let journal_before = snapshot(journal.path());
    let id = source::Sha256Digest::of(b"refused before CLI configured work");
    let mut unavailable = workspace_input(&fixture);
    unavailable["root"] = json!(fixture.owner.path().join("unavailable-owner"));
    let arguments = cli_arguments(journal.path(), initialized.header, id);
    for flag in CLI_PERMISSIONS {
        let denied = arguments
            .iter()
            .filter(|argument| argument.as_str() != flag)
            .cloned()
            .collect::<Vec<_>>();
        let output = cli(&denied, &unavailable);
        assert!(!output.status.success(), "independent permission {flag}");
        assert!(
            output.stdout.is_empty(),
            "no writer reservation frame for {flag}"
        );
        assert!(!String::from_utf8_lossy(&output.stderr).contains("workspace_configuration"));
        assert_eq!(snapshot(journal.path()), journal_before);
        assert_eq!(snapshot(&root), before);
    }
    let generator_parent = tempfile::tempdir().unwrap();
    for missing in ["--allow-stage", "--allow-execute"] {
        let mut denied = arguments.clone();
        denied.extend([
            "--generator-parent".into(),
            generator_parent.path().to_str().unwrap().into(),
        ]);
        denied.push(if missing == "--allow-stage" {
            "--allow-execute".into()
        } else {
            "--allow-stage".into()
        });
        let output = cli(&denied, &unavailable);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains("documentation_materialization_permission")
        );
        assert_eq!(fs::read_dir(generator_parent.path()).unwrap().count(), 0);
        assert_eq!(snapshot(journal.path()), journal_before);
    }
    let mut tiny = arguments;
    tiny.extend(["--max-response-bytes".into(), "1".into()]);
    let output = cli(&tiny, &unavailable);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("materialization_response_reservation")
    );
    assert_eq!(snapshot(journal.path()), journal_before);
    assert_eq!(snapshot(&root), before);
    let inspection = history(&initialized.database, &root, initialized.header);
    assert!(inspection.frontier.head.is_none() && inspection.frontier.active.is_none());
}

#[test]
fn cli_native_pending_history_preserves_documentation_custody_and_explicit_close() {
    let fixture = Fixture::new(0);
    let root = fixture.root();
    let inputs = fixture.inputs();
    let assembly = build::assemble_documentation(&inputs, Default::default()).unwrap();
    let (journal, initialized) = initialize();
    let mut writer = store::acquire_writer(
        journal.path(),
        initialized.header,
        store::WriterOptions {
            allow_generation_advance: true,
            session: session(),
        },
    )
    .unwrap();
    let plan = writer
        .plan_documentation_materialization(
            &assembly,
            &[],
            Default::default(),
            options().evidence_bytes,
        )
        .unwrap();
    fs::write(
        root.join("new-user.txt"),
        b"concurrent independently owned bytes",
    )
    .unwrap();
    let pending = source::Sha256Digest::of(b"original pending before CLI");
    let failed = writer.materialize(plan, pending, options()).unwrap_err();
    let build::MaterializationError::Write { id, cause } = failed else {
        panic!("exact native write phase")
    };
    assert_eq!(id, pending);
    assert!(cause.attempted.is_empty() && cause.residue.is_none());
    let previous = writer.inspect().unwrap().header;
    writer.close().unwrap();
    let before = snapshot(&root);
    let next = source::Sha256Digest::of(b"CLI refuses rather than replace original pending");
    let output = cli(
        &cli_arguments(journal.path(), previous, next),
        &workspace_input(&fixture),
    );
    assert!(!output.status.success());
    let frames = frames(&output);
    let header = cli_header(&frames, previous, journal.path(), next);
    assert_eq!(frames[2]["details_complete"], true);
    assert_eq!(frames[2]["detail"]["outcome"]["status"], "plan_failed");
    assert_eq!(
        frames[2]["detail"]["outcome"]["failure"]["code"],
        "store_materialization_pending"
    );
    assert_eq!(
        frames[2]["detail"]["documentation"]["status"],
        "documentation-assembled-local"
    );
    assert_eq!(
        frames[2]["detail"]["documentation"]["inventory"],
        json!(assembly.output().inventory())
    );
    let inspection = history(&initialized.database, &root, header);
    assert!(inspection.frontier.head.is_none());
    let active = inspection.frontier.active.unwrap();
    assert_eq!(active.intent.id, pending);
    assert!(active.completion.is_none());
    assert_eq!(snapshot(&root), before);
}

#[test]
fn cli_post_effect_response_overflow_retains_original_documentation_and_completed_history() {
    // The real full native inspection below proves this ordinary selected tree
    // amplifies private history evidence beyond its much smaller documentation report.
    let fixture = Fixture::new(32);
    let root = fixture.root();
    fs::write(
        root.join("user.txt"),
        b"preserved through delivery overflow",
    )
    .unwrap();
    fs::write(root.join(".git"), b"gitdir: ../retained-admin\n").unwrap();
    let inputs = fixture.inputs();
    let assembly = build::assemble_documentation(&inputs, Default::default()).unwrap();
    let maximum = native_report_size(&inputs, &assembly) + (8 << 10);
    assert!(maximum < 1 << 20, "no default response ceiling enlargement");
    let (journal, initialized) = initialize();
    let mut writer = store::acquire_writer(
        journal.path(),
        initialized.header,
        store::WriterOptions {
            allow_generation_advance: true,
            session: session(),
        },
    )
    .unwrap();
    let plan = writer
        .plan_documentation_materialization(
            &assembly,
            &[],
            Default::default(),
            options().evidence_bytes,
        )
        .unwrap();
    let original = source::Sha256Digest::of(b"actual native full materialization baseline");
    let result = writer.materialize(plan, original, options()).unwrap();
    assert!(
        serde_json::to_vec(&result.inspect()).unwrap().len() > maximum,
        "actual complete native history/output inspection proves amplification before CLI"
    );
    let previous = writer.inspect().unwrap().header;
    writer.close().unwrap();
    let before = snapshot(&root);
    let next = source::Sha256Digest::of(b"post-effect CLI documentation response overflow");
    let mut arguments = cli_arguments(journal.path(), previous, next);
    arguments.extend(["--max-response-bytes".into(), maximum.to_string()]);
    let output = cli(&arguments, &workspace_input(&fixture));
    assert!(
        !output.status.success(),
        "missing full materialization evidence never success"
    );
    let frames = frames(&output);
    let header = cli_header(&frames, previous, journal.path(), next);
    let delivered = &frames[2];
    assert_eq!(delivered["event"], "delivery_incomplete");
    assert_eq!(delivered["details_complete"], false);
    assert_eq!(
        delivered["detail"]["outcome"]["code"],
        "documentation_materialization_response_unavailable"
    );
    assert_eq!(delivered["detail"]["outcome"]["may_have_effects"], true);
    assert_eq!(
        delivered["detail"]["documentation"]["status"],
        "documentation-assembled-local"
    );
    assert_eq!(delivered["detail"]["documentation"]["jobs"], json!([]));
    assert_eq!(
        delivered["detail"]["documentation"]["manual"],
        json!(assembly.manual())
    );
    assert_eq!(
        delivered["detail"]["documentation"]["artifacts"],
        json!(assembly.artifacts())
    );
    assert_eq!(
        delivered["detail"]["documentation"]["inventory"],
        json!(assembly.output().inventory())
    );
    let inspection = history(&initialized.database, &root, header);
    assert!(inspection.frontier.active.is_none());
    let head = inspection.frontier.head.unwrap();
    assert_eq!(head.intent.id, next);
    assert_eq!(head.intent.previous, Some(original));
    head.completion
        .as_ref()
        .unwrap()
        .validate(&head.intent)
        .unwrap();
    assert_eq!(snapshot(&root), before);
    for entry in assembly.output().inventory().entries() {
        if let Some(bytes) = assembly.output().file(&entry.path) {
            assert_eq!(fs::read(root.join(entry.path.as_str())).unwrap(), bytes);
        }
    }
}

#[test]
fn cli_native_documentation_report_overflow_keeps_compact_custody_without_output_dispatch() {
    const MAXIMUM: usize = 256 * 1024;
    let fixture = Fixture::new(overflowing_count(MAXIMUM));
    let root = fixture.root();
    fs::write(root.join("user.txt"), b"not an assembled target").unwrap();
    let before = snapshot(&root);
    let inputs = fixture.inputs();
    let assembly = build::assemble_documentation(&inputs, Default::default()).unwrap();
    assert!(native_report_size(&inputs, &assembly) > MAXIMUM);
    let (journal, initialized) = initialize();
    let id = source::Sha256Digest::of(b"CLI documentation report overflow before output dispatch");
    let mut arguments = cli_arguments(journal.path(), initialized.header, id);
    arguments.extend(["--max-response-bytes".into(), MAXIMUM.to_string()]);
    let output = cli(&arguments, &workspace_input(&fixture));
    assert!(!output.status.success());
    let frames = frames(&output);
    let header = cli_header(&frames, initialized.header, journal.path(), id);
    let result = &frames[2];
    assert_eq!(result["event"], "result");
    assert_eq!(result["details_complete"], true);
    assert_eq!(result["detail"]["outcome"]["status"], "not_attempted");
    assert_eq!(
        result["detail"]["outcome"]["failure"]["phase"],
        "documentation_delivery"
    );
    assert_eq!(
        result["detail"]["documentation"]["code"],
        "documentation_response_unavailable"
    );
    assert_eq!(result["detail"]["documentation"]["completed"], json!([]));
    assert_eq!(
        result["detail"]["documentation"]["output_identity"],
        json!(assembly.output().inventory().identity())
    );
    let inspection = history(&initialized.database, &root, header);
    assert!(inspection.frontier.head.is_none() && inspection.frontier.active.is_none());
    assert_eq!(snapshot(&root), before);
}

#[test]
fn cli_generated_defaults_fit_native_documentation_custody_reservation() {
    let help = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args(["materialize-docs", "--help"])
        .output()
        .unwrap();
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout).unwrap();
    let response = help
        .split("--max-response-bytes <MAX_RESPONSE_BYTES>")
        .nth(1)
        .expect("native Clap response bound")
        .split("--")
        .next()
        .unwrap();
    assert!(response.contains("[default: 16777216]"), "{response}");
    let jobs = help
        .split("--max-generator-jobs <MAX_GENERATOR_JOBS>")
        .nth(1)
        .expect("native Clap job ceiling")
        .split("--")
        .next()
        .unwrap();
    assert!(jobs.contains("[default: 32]"), "{jobs}");
    let bounds = build::documentation_output::Bounds {
        jobs: 32,
        directory_bytes: 16 << 10,
        entry_bytes: 4 << 10,
    };
    assert!(
        matches!(
            build::documentation_output::Reservation::new(bounds, 1 << 20),
            Err("documentation_response_limit")
        ),
        "the old response default cannot retain the admitted default job custody"
    );
    build::documentation_output::Reservation::new(bounds, 16 << 20)
        .expect("native full worst-case custody fits the aligned documentation default");
    // The real-provider test below exercises the same bound after the CLI's
    // outer custody reservation, without overriding either default.
}

#[test]
#[ignore = "requires explicit GAZETTE_TEST_RUSTDOC_STABLE provider qualification"]
fn native_rustdoc_materialize_docs_defaults_preserve_generated_custody_and_configured_output() {
    let fixture = Fixture::new(0);
    let root = fixture.root();
    let output_git = git2::Repository::init(&root).unwrap();
    fs::write(root.join("user.txt"), b"independent configured owner bytes").unwrap();
    let git_before = snapshot(output_git.path());
    let manual_before = snapshot(fixture.source.path());
    let program = std::path::PathBuf::from(
        std::env::var_os("GAZETTE_TEST_RUSTDOC_STABLE")
            .expect("supply the installed stable Rustdoc absolute path; no install or skip"),
    );
    assert!(program.is_absolute(), "never search PATH for the provider");
    assert!(
        fs::symlink_metadata(&program).unwrap().is_file(),
        "select the real executable, not a rustup proxy symlink"
    );
    let executable = fs::read(&program).unwrap();
    let executable_sha256 = source::Sha256Digest::of(&executable);
    let version = Command::new(&program)
        .arg("--version")
        .env_clear()
        .output()
        .unwrap();
    assert!(version.status.success());
    let version_text = std::str::from_utf8(&version.stdout).unwrap();
    assert!(version_text.starts_with("rustdoc ") && !version_text.contains("nightly"));
    let version_sha256 = source::Sha256Digest::of(&version.stdout);

    let source_root = tempfile::tempdir().unwrap();
    let repository = git2::Repository::init(source_root.path()).unwrap();
    fs::create_dir(source_root.path().join("inputs")).unwrap();
    fs::write(
        source_root.path().join("inputs/library.rs"),
        RUSTDOC_LIBRARY,
    )
    .unwrap();
    let mut index = repository.index().unwrap();
    index.add_path(Path::new("inputs/library.rs")).unwrap();
    index.write().unwrap();
    let tree = repository.find_tree(index.write_tree().unwrap()).unwrap();
    let signature =
        git2::Signature::new("Fixture", "fixture@example.invalid", &git2::Time::new(0, 0)).unwrap();
    let commit = repository
        .commit(
            Some("HEAD"),
            &signature,
            &signature,
            "Rustdoc fixture",
            &tree,
            &[],
        )
        .unwrap();
    drop(tree);
    let source_before = snapshot(source_root.path());
    let mut arguments = vec!["{kind='input',path='library.rs'}".to_owned()];
    for value in [
        "--crate-name",
        "reference_fixture",
        "--crate-version",
        "0.1.0",
        "--edition",
        "2024",
        "-D",
        "warnings",
        "-o",
    ] {
        arguments.push(format!("{{kind='literal',value={value:?}}}"));
    }
    arguments.push("{kind='output-root'}".into());
    let generated = format!(
        "{{kind='generated',source='source:rust',recipe={{program={:?},executable={{bytes={},sha256='{executable_sha256}'}},arguments=[{}],environment={{}},version_arguments=['--version'],expected_version_sha256='{version_sha256}',output={{kind='directory',path='reference_fixture'}},format='rustdoc-html'}}}}",
        program.to_str().unwrap(),
        executable.len(),
        arguments.join(","),
    );
    let config_path = fixture.owner.path().join("gazette.toml");
    let original = fs::read_to_string(&config_path).unwrap();
    let captured = "{kind='artifact',source='source:manual',selection={kind='file',path='api.json'},format='typedoc-json'}";
    assert_eq!(original.matches(captured).count(), 1);
    let mut config = original.replace(captured, &generated);
    config.push_str(&format!(
        "\n[[sources]]\nid='source:rust'\nlocation={{kind='repository',root={:?},commit='{commit}',subdirectory='inputs'}}\n",
        source_root.path().to_str().unwrap(),
    ));
    fs::write(&config_path, &config).unwrap();
    let inputs = fixture.inputs();
    let jobs = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let jobs_root = jobs.path().canonicalize().unwrap();
    let (journal, initialized) = initialize();
    let id = source::Sha256Digest::of(b"real Rustdoc configured CLI materialization defaults");
    let mut args = cli_arguments(journal.path(), initialized.header, id);
    args.extend([
        "--generator-parent".into(),
        jobs_root.to_str().unwrap().into(),
        "--allow-stage".into(),
        "--allow-execute".into(),
    ]);
    assert!(
        !args
            .iter()
            .any(|arg| arg == "--max-response-bytes" || arg == "--max-generator-jobs")
    );
    let output = cli(&args, &workspace_input(&fixture));
    assert!(
        output.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let frames = frames(&output);
    let header = cli_header(&frames, initialized.header, journal.path(), id);
    let result = &frames[2];
    assert_eq!(result["event"], "result");
    assert_eq!(result["details_complete"], true);
    assert_eq!(result["detail"]["outcome"]["status"], "materialized");
    let report = &result["detail"]["documentation"];
    assert_eq!(report["status"], "documentation-assembled-local");
    assert_eq!(report["inputs"], json!(inputs.inspect()));
    assert_eq!(report["manual"].as_array().unwrap().len(), 1);
    assert_eq!(report["jobs"].as_array().unwrap().len(), 1);
    assert_eq!(report["artifacts"].as_array().unwrap().len(), 1);
    let job = &report["jobs"][0];
    let artifact = &report["artifacts"][0];
    assert_eq!(artifact["source"], "source:rust");
    assert_eq!(artifact["format"], "rustdoc-html");
    assert_eq!(artifact["generation"], job["evidence"]);
    assert_eq!(artifact["tree"], job["output_identity"]);
    assert_eq!(artifact["selection"], job["selection"]);
    assert_eq!(job["evidence"]["version_text"], version_text);
    assert_eq!(job["evidence"]["generation"]["exit"]["code"], 0);
    let retained_input = Path::new(job["input"].as_str().unwrap());
    let retained_output = Path::new(job["output"].as_str().unwrap());
    assert!(retained_input.starts_with(&jobs_root) && retained_output.starts_with(&jobs_root));
    assert_ne!(retained_input, retained_output);
    assert_eq!(
        fs::read(retained_input.join("library.rs")).unwrap(),
        RUSTDOC_LIBRARY
    );
    let native = source::inspect(retained_output, None, Default::default()).unwrap();
    assert_eq!(json!(native.identity()), job["output_identity"]);
    assert!(
        native
            .entries()
            .iter()
            .any(|entry| entry.path.as_str().starts_with("static.files/"))
    );
    assert!(
        fs::read_to_string(retained_output.join("trait.impl/reference_fixture/trait.Describe.js"))
            .unwrap()
            .contains("Object.fromEntries(")
    );
    assert_eq!(
        fs::read_dir(&jobs_root).unwrap().count(),
        2,
        "one retained pair; no retry"
    );

    let inspection = history(&initialized.database, &root, header);
    assert!(inspection.frontier.active.is_none());
    let head = inspection.frontier.head.unwrap();
    assert_eq!(head.intent.id, id);
    head.completion
        .as_ref()
        .unwrap()
        .validate(&head.intent)
        .unwrap();
    assert_eq!(json!(head.intent.generated), report["inventory"]);
    assert_eq!(
        result["detail"]["outcome"]["materialization"]["output"]["generated"],
        report["inventory"]
    );
    for entry in head.intent.generated.entries() {
        if let source::Content::File { bytes, sha256 } = entry.content {
            let actual = fs::read(root.join(entry.path.as_str())).unwrap();
            assert_eq!(actual.len() as u64, bytes, "{}", entry.path);
            assert_eq!(source::Sha256Digest::of(&actual), sha256, "{}", entry.path);
        }
    }
    assert_eq!(
        head.intent
            .generated
            .entries()
            .iter()
            .filter(|entry| entry.path.as_str().ends_with(".md")
                && matches!(entry.content, source::Content::File { .. }))
            .count(),
        16,
        "all fifteen controlled Rustdoc pages plus the retained manual"
    );
    let navigation = fs::read_to_string(root.join("docs.json")).unwrap();
    for page in [
        "manual/start",
        "module/reference/index",
        "module/reference/fn.describe",
        "module/reference/trait.Describe",
    ] {
        assert!(
            navigation.contains(page),
            "missing selected navigation {page}"
        );
        assert!(root.join(format!("{page}.md")).is_file());
    }
    assert!(
        fs::read_to_string(root.join("manual/start.md"))
            .unwrap()
            .contains("The retained authored manual.")
    );
    let record = fs::read_to_string(root.join("module/reference/model/struct.Record.md")).unwrap();
    for expected in [
        "Original payload, without conversion.",
        "structfield.value",
        "associatedtype.Output",
        "method.description",
        "assert&#95;eq",
        "href=\"/module/reference/fn.describe\"",
        "/module/reference/sources/src/reference&#95;fixture/library.rs&#35;",
    ] {
        assert!(
            record.contains(expected),
            "missing retained API meaning {expected}"
        );
    }
    assert!(
        fs::read_to_string(root.join("module/reference/trait.Describe.md"))
            .unwrap()
            .contains("href=\"/module/reference/model/struct.Record\"")
    );
    let function = fs::read_to_string(root.join("module/reference/fn.describe.md")).unwrap();
    assert!(
        function.contains("&amp;") && function.contains("Display") && function.contains("0.1.0")
    );
    assert!(function.contains("href=\"/module/reference/model/struct.Record\""));
    assert!(!root.join("static.files").exists() && !root.join("trait.impl").exists());
    assert_eq!(
        fs::read(root.join("user.txt")).unwrap(),
        b"independent configured owner bytes"
    );
    assert_eq!(snapshot(output_git.path()), git_before);
    assert_eq!(snapshot(fixture.source.path()), manual_before);
    assert_eq!(snapshot(source_root.path()), source_before);
    assert_eq!(fs::read(config_path).unwrap(), config.as_bytes());
    assert_eq!(
        source::Sha256Digest::of(&fs::read(program).unwrap()),
        executable_sha256
    );
    let mut reopened = store::acquire_writer(
        journal.path(),
        header,
        store::WriterOptions {
            allow_generation_advance: true,
            session: session(),
        },
    )
    .unwrap();
    let reopened_header = reopened.inspect().unwrap().header;
    assert_eq!(reopened_header.writer_fence, header.writer_fence + 1);
    reopened.close().unwrap();
    let reopened_history = history(&initialized.database, &root, reopened_header);
    assert_eq!(reopened_history.frontier.head.unwrap().intent.id, id);
}
