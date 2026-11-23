//! Real configured command custody through the CLI; the owned shell fixture is not `TypeDoc` qualification.

use gazette_sdk::source;
use git2::{Repository, Signature};
use serde_json::Value;
use std::{
    fs,
    io::Write,
    os::unix::ffi::OsStrExt,
    path::PathBuf,
    process::{Command, Output, Stdio},
};
use tempfile::TempDir;

#[path = "support/mcp.rs"]
mod carrier;

const ARTIFACT: &[u8] =
    br#"{"id":0,"name":"Owned CLI fixture","kind":1,"variant":"project","schemaVersion":"2.0"}"#;
const COPY: &[u8] = b"/bin/cat \"$1\" > \"$2\"\nprintf companion > \"${2}.companion\"\n";

struct Fixture {
    source: TempDir,
    jobs: TempDir,
    config: String,
}

impl Fixture {
    fn new(script: &[u8], count: usize) -> Self {
        let source = tempfile::tempdir().expect("disposable exact-pin source");
        let jobs = tempfile::tempdir().expect("separate retained-job parent");
        let repository = Repository::init(source.path()).expect("native Git fixture");
        fs::create_dir(source.path().join("inputs")).expect("selected source subtree");
        let mut subtree = repository.treebuilder(None).expect("source tree builder");
        for (name, bytes) in [("generator.sh", script), ("payload.json", ARTIFACT)] {
            fs::write(source.path().join("inputs").join(name), bytes).expect("exact source bytes");
            subtree
                .insert(name, repository.blob(bytes).expect("source blob"), 0o100644)
                .expect("source tree entry");
        }
        let mut tree = repository.treebuilder(None).expect("root tree builder");
        tree.insert(
            "inputs",
            subtree.write().expect("source identity"),
            0o040000,
        )
        .expect("selected subtree");
        let tree = repository
            .find_tree(tree.write().expect("root identity"))
            .expect("native tree");
        let signature =
            Signature::new("Fixture", "fixture@example.invalid", &git2::Time::new(0, 0))
                .expect("fixed fixture signature");
        let commit = repository
            .commit(Some("HEAD"), &signature, &signature, "fixture", &tree, &[])
            .expect("exact native commit");
        let program = fs::canonicalize("/bin/sh").expect("explicit fixture interpreter");
        let program = program.to_str().expect("fixture interpreter path");
        let executable = fs::read(program).expect("fixture interpreter bytes");
        let mut config = format!(
            "[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n[[sources]]\nid='source:one'\nlocation={{kind='repository',root={:?},commit='{commit}',subdirectory='inputs'}}\n",
            source.path().to_str().expect("source path")
        );
        for index in 1..=count {
            config.push_str(&format!("[[projects]]\nid='project:p{index}'\nlifecycle='active'\nvisibility='public'\navailability='available'\n"));
        }
        config.push_str(&format!("[[brands]]\nid='brand:one'\nname='Configured publication'\n[[brands.publications]]\nsite='site:one'\n[brands.publications.documentation_repository]\nname='owner-selected-docs'\npath={:?}\n[brands.publications.documentation_repository.build]\ntarget={{kind='mintlify',theme='mint',primary_color='#123abc'}}\n", jobs.path().join("brand-selected-destination").to_str().expect("configured path")));
        for index in 1..=count {
            let version = format!("owned-cli-job-{index}/1\n");
            config.push_str(&format!("[[brands.publications.documentation_repository.build.modules]]\nproject='project:p{index}'\nslug='module-{index}'\ntitle='Module {index}'\n[[brands.publications.documentation_repository.build.modules.navigation]]\nkind='reference'\nslug='library'\ntitle='Library'\ninput={{kind='generated',source='source:one',recipe={{program={program:?},executable={{bytes={},sha256='{}'}},arguments=[{{kind='input',path='generator.sh'}},{{kind='input',path='payload.json'}},{{kind='output'}},{{kind='literal',value='{index}'}}],version_arguments=['-c',\"printf 'owned-cli-job-{index}/1\\n'\"],expected_version_sha256='{}',output={{kind='file',path='reference.json'}},format='typedoc-json'}}}}\n", executable.len(), source::Sha256Digest::of(&executable), source::Sha256Digest::of(version.as_bytes())));
        }
        Self {
            source,
            jobs,
            config,
        }
    }

    fn run(&self, close_stdout: bool, allow_execute: bool) -> Output {
        self.run_bounded(close_stdout, allow_execute, None)
    }

    fn run_bounded(
        &self,
        close_stdout: bool,
        allow_execute: bool,
        maximum: Option<usize>,
    ) -> Output {
        self.run_with_arguments(close_stdout, allow_execute, maximum, &[])
    }

    fn run_with_arguments(
        &self,
        close_stdout: bool,
        allow_execute: bool,
        maximum: Option<usize>,
        arguments: &[&str],
    ) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_gazette"));
        command
            .args([
                "assemble-docs",
                "--local",
                "--site",
                "site:one",
                "--generator-parent",
            ])
            .arg(self.jobs.path())
            .args(arguments);
        if allow_execute {
            command.arg("--allow-execute");
        }
        if let Some(maximum) = maximum {
            command.arg("--max-output-bytes").arg(maximum.to_string());
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("real Gazette executable");
        if close_stdout {
            drop(child.stdout.take().expect("close carrier before execution"));
        }
        child
            .stdin
            .take()
            .expect("configuration carrier")
            .write_all(self.config.as_bytes())
            .expect("complete configuration");
        child.wait_with_output().expect("native command completion")
    }

    fn stages(&self) -> Vec<PathBuf> {
        assert!(!self.jobs.path().join("brand-selected-destination").exists());
        fs::read_dir(self.jobs.path())
            .expect("retained job parent")
            .map(|entry| {
                entry
                    .expect("retained stage")
                    .path()
                    .canonicalize()
                    .expect("independent canonical retained locator")
            })
            .collect()
    }

    fn assert_source_unchanged(&self) {
        assert_eq!(
            fs::read(self.source.path().join("inputs/payload.json")).expect("source retained"),
            ARTIFACT
        );
    }
}

#[test]
fn generated_target_failure_keeps_its_invariant_and_runs_once_per_cli_or_mcp_request() {
    let mut fixture = Fixture::new(
        b"printf 'generation\\n' >> \"$4\"\nprintf '{}' > \"$2\"\n",
        1,
    );
    let counter = tempfile::tempdir().expect("independent explicit fixture counter");
    let counter_file = counter.path().join("invocations");
    let original = "{kind='literal',value='1'}]";
    assert_eq!(fixture.config.matches(original).count(), 1);
    fixture.config = fixture.config.replace(
        original,
        &format!(
            "{{kind='literal',value='1'}},{{kind='literal',value={:?}}}]",
            counter_file.to_str().expect("exact fixture argument")
        ),
    );
    let output = fixture.run(false, true);
    assert!(!output.status.success());
    let cli: Value = serde_json::from_slice(&output.stdout).expect("complete native custody");
    let expected = serde_json::json!({
        "context":"artifacts/module-1/library.json", "context_omitted":false,
        "reason":"expected a versioned TypeDoc project JSON artifact", "reason_omitted":false
    });
    assert_eq!(cli["failure"]["code"], "documentation_target_input");
    assert_eq!(cli["failure"]["target_diagnostic"], expected);
    assert_eq!(
        cli["completed"]
            .as_array()
            .expect("completed generation")
            .len(),
        1
    );
    assert_eq!(cli["completed"][0]["generation"]["exit"]["code"], 0);
    assert_eq!(fixture.stages().len(), 2);
    assert_eq!(
        fs::read_to_string(&counter_file).expect("counter"),
        "generation\n"
    );
    carrier::initialized_interact("2026-07-28", |_, send| {
        let result = send(serde_json::json!({
            "jsonrpc":"2.0", "id":1, "method":"tools/call",
            "params":{"name":"assemble_docs", "arguments":{
                "workspace":{"kind":"supplied", "source":fixture.config},
                "site":"site:one", "local":true, "allow_read":true, "allow_stage":true,
                "directory_bytes":1024,
                "generation":{"parent":fixture.jobs.path(), "allow_execute":true,
                    "max_jobs":1, "entry_bytes":128}
            }}
        }));
        assert_eq!(result["result"]["isError"], true, "{result}");
        let mcp = &result["result"]["structuredContent"];
        assert_eq!(mcp["failure"]["code"], cli["failure"]["code"]);
        assert_eq!(mcp["failure"]["target_diagnostic"], expected);
        assert_eq!(
            mcp["completed"]
                .as_array()
                .expect("complete native prefix")
                .len(),
            1
        );
        assert_eq!(
            mcp["completed"][0]["content"],
            cli["completed"][0]["content"]
        );
        assert_eq!(mcp["completed"][0]["generation"]["exit"]["code"], 0);
    });
    assert_eq!(
        fixture.stages().len(),
        4,
        "one stage pair per explicit request"
    );
    assert_eq!(
        fs::read_to_string(&counter_file).expect("counter"),
        "generation\ngeneration\n"
    );
    fixture.assert_source_unchanged();
}

#[test]
fn native_workspace_selection_is_shared_by_cli_and_initialized_mcp_from_unrelated_directories() {
    let fixture = Fixture::new(
        b"[ \"$PACKAGES\" = \"$EXPECTED\" ] || exit 61\n[ \"$LITERAL\" = 'machine:missing/not-expanded' ] || exit 62\n/bin/cat \"$1\" > \"$2\"\nprintf companion > \"${2}.companion\"\n",
        1,
    );
    let owner = tempfile::tempdir().expect("explicit native workspace owner");
    let elsewhere = tempfile::tempdir().expect("unrelated process directory");
    let original = format!(
        "path={:?}",
        fixture
            .jobs
            .path()
            .join("brand-selected-destination")
            .to_str()
            .unwrap()
    );
    assert_eq!(fixture.config.matches(&original).count(), 1);
    let program = fs::canonicalize("/bin/sh").unwrap();
    let native_program = format!("program={:?}", program.to_str().unwrap());
    let portable_program = format!(
        "program='machine:runtime/{}'",
        program.file_name().unwrap().to_str().unwrap()
    );
    let environment = format!(
        "environment={{PACKAGES={{path='packages'}},EXPECTED={:?},LITERAL='machine:missing/not-expanded'}},",
        owner.path().join("packages").to_str().unwrap()
    );
    let mut portable = fixture
        .config
        .replace(&original, "path='domain-doc'")
        .replace(&native_program, &portable_program)
        .replace("arguments=[{kind='input',path='generator.sh'}", "arguments=[{kind='path',path='machine:provider/generator.sh'}")
        .replace("recipe={", &format!("recipe={{{environment}validation_arguments=[{{kind='path',path='machine:provider/validate.sh'}},{{kind='output'}}],"));
    // Replace this field without coupling the test to shell text escaped inside TOML.
    let version_start = portable.find("version_arguments=").unwrap();
    let version_end = version_start
        + portable[version_start..]
            .find(",expected_version_sha256=")
            .unwrap();
    portable.replace_range(
        version_start..version_end,
        "version_arguments=[{path='machine:provider/version.sh'}]",
    );
    fs::write(
        owner.path().join("generator.sh"),
        fs::read(fixture.source.path().join("inputs/generator.sh")).unwrap(),
    )
    .unwrap();
    fs::write(
        owner.path().join("version.sh"),
        "printf 'owned-cli-job-1/1\\n'\n",
    )
    .unwrap();
    fs::write(owner.path().join("validate.sh"), "test -s \"$1\"\n").unwrap();
    assert!(portable.contains("version_arguments=[{path='machine:provider/version.sh'}]"));
    fs::write(
        owner.path().join("machine.local.toml"),
        format!(
            "[roots]\nruntime={:?}\nprovider={:?}\n",
            program.parent().unwrap().to_str().unwrap(),
            owner.path().to_str().unwrap()
        ),
    )
    .unwrap();
    fs::write(
        owner.path().join("gazette.toml"),
        format!("machine_binding='machine.local.toml'\n{portable}"),
    )
    .expect("relative destination and generator paths remain workspace-owned");
    let selection = serde_json::json!({
        "kind":"local", "root":owner.path(), "workspace":"gazette.toml"
    });
    let mut child = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args([
            "assemble-docs",
            "--local",
            "--workspace-json",
            "--site",
            "site:one",
            "--allow-execute",
            "--generator-parent",
        ])
        .arg(fixture.jobs.path())
        .current_dir(elsewhere.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("actual CLI process");
    serde_json::to_writer(child.stdin.take().unwrap(), &selection).unwrap();
    let output = child.wait_with_output().expect("actual CLI completion");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let direct: Value = serde_json::from_slice(&output.stdout).expect("complete native custody");
    carrier::initialized_interact("2026-07-28", |_, send| {
        let result = send(serde_json::json!({
            "jsonrpc":"2.0", "id":1, "method":"tools/call",
            "params":{"name":"assemble_docs", "arguments":{
                "workspace":selection, "site":"site:one", "local":true,
                "allow_read":true, "allow_stage":true, "directory_bytes":1024,
                "generation":{"parent":fixture.jobs.path(), "allow_execute":true, "max_jobs":1, "entry_bytes":128}
            }}
        }));
        assert_eq!(result["result"]["isError"], false, "{result}");
        let report = &result["result"]["structuredContent"];
        for field in ["status", "brand", "manual", "inventory"] {
            assert_eq!(report[field], direct[field], "same native owner: {field}");
        }
        assert_eq!(
            report["jobs"][0]["output_identity"],
            direct["jobs"][0]["output_identity"]
        );
    });
    assert!(
        !owner.path().join("domain-doc").exists(),
        "assembly is not configured output delivery"
    );
    assert!(
        !elsewhere.path().join("domain-doc").exists(),
        "no process-directory fallback"
    );
    fixture.assert_source_unchanged();
}

#[test]
fn workspace_selector_json_refuses_non_objects_and_duplicate_fields_before_generation() {
    let fixture = Fixture::new(COPY, 1);
    for selection in [
        "[]",
        "{\"kind\":\"supplied\",\"source\":\"a\",\"source\":\"b\"}",
    ] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_gazette"))
            .args([
                "assemble-docs",
                "--local",
                "--workspace-json",
                "--site",
                "site:one",
                "--allow-execute",
                "--generator-parent",
            ])
            .arg(fixture.jobs.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("actual CLI process");
        child
            .stdin
            .take()
            .unwrap()
            .write_all(selection.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("workspace_selection"));
        assert!(
            fixture.stages().is_empty(),
            "invalid selector cannot start native generation"
        );
    }
}

#[test]
fn actual_mcp_and_cli_share_generated_documentation_outputs_and_discoverable_evidence() {
    let fixture = Fixture::new(COPY, 1);
    let cli = fixture.run(false, true);
    assert!(
        cli.status.success(),
        "{}",
        String::from_utf8_lossy(&cli.stderr)
    );
    let direct: Value = serde_json::from_slice(&cli.stdout).expect("native CLI report");
    let responses = carrier::exchange(vec![
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}),
        serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"assemble_docs","arguments":{
            "workspace":{"kind":"supplied","source":fixture.config},"site":"site:one","local":true,"allow_read":true,"allow_stage":true,"directory_bytes":1024,
            "generation":{"parent":fixture.jobs.path(),"allow_execute":true,"max_jobs":1,"entry_bytes":128}
        }}}),
    ]);
    let tool = responses[0]["result"]["tools"]
        .as_array()
        .expect("actual discovery")
        .iter()
        .find(|tool| tool["name"] == "assemble_docs")
        .expect("available native operation");
    let result = &responses[1]["result"];
    assert_eq!(result["isError"], false, "{responses:?}");
    let report = &result["structuredContent"];
    jsonschema::validator_for(&tool["outputSchema"])
        .expect("native output schema")
        .validate(report)
        .expect("actual protocol output matches discovery");
    assert_eq!(
        serde_json::from_str::<Value>(
            result["content"][0]["text"]
                .as_str()
                .expect("readable projection")
        )
        .expect("same complete JSON"),
        *report
    );
    for field in ["status", "inputs", "repository", "inventory"] {
        assert_eq!(report[field], direct[field], "same SDK owner: {field}");
    }
    assert_eq!(
        report["artifacts"][0]["content"]["sha256"],
        direct["artifacts"][0]["content"]["sha256"]
    );
    assert_eq!(
        report["jobs"][0]["output_identity"],
        direct["jobs"][0]["output_identity"]
    );
    let output = PathBuf::from(
        report["jobs"][0]["output"]
            .as_str()
            .expect("actual MCP stage"),
    );
    assert_eq!(
        fs::read(output.join("reference.json")).expect("retained exact artifact"),
        ARTIFACT
    );
    assert_eq!(
        fs::read(output.join("reference.json.companion")).expect("complete companion tree"),
        b"companion"
    );
    assert_eq!(
        fixture.stages().len(),
        4,
        "two explicit calls, no silent retry"
    );
    fixture.assert_source_unchanged();
}

#[test]
fn generated_only_success_reports_actual_jobs_without_creating_brand_destination() {
    let fixture = Fixture::new(COPY, 1);
    let output = fixture.run(false, true);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("native CLI receipt");
    assert!(report["directory"].is_null(), "no final stage requested");
    assert_eq!(report["jobs"].as_array().expect("actual jobs").len(), 1);
    assert_eq!(
        report["jobs"][0]["evidence"]["version_text"],
        "owned-cli-job-1/1\n"
    );
    assert_eq!(
        report["artifacts"][0]["generation"],
        report["jobs"][0]["evidence"]
    );
    assert_eq!(
        report["artifacts"][0]["content"]["sha256"],
        serde_json::to_value(source::Sha256Digest::of(ARTIFACT)).expect("artifact digest")
    );
    let path = PathBuf::from(
        report["jobs"][0]["output"]
            .as_str()
            .expect("native output locator"),
    );
    assert_eq!(
        fs::read(path.join("reference.json")).expect("actual output"),
        ARTIFACT
    );
    assert_eq!(
        fs::read(path.join("reference.json.companion")).expect("companion retained"),
        b"companion"
    );
    assert_eq!(
        fixture.stages().len(),
        2,
        "one input and one output, no implicit retry"
    );
    fixture.assert_source_unchanged();
}

#[test]
fn closed_stdout_after_generation_is_failure_with_all_retained_job_evidence() {
    let fixture = Fixture::new(COPY, 1);
    let output = fixture.run(true, true);
    assert!(
        !output.status.success(),
        "generation is effectful even without --output-parent"
    );
    let stderr = String::from_utf8(output.stderr).expect("failure carrier");
    let (diagnostic, payload) = stderr
        .split_once('\n')
        .expect("diagnostic and original JSON payload");
    assert!(diagnostic.contains("output_io"), "{stderr}");
    let report: Value =
        serde_json::from_str(payload).expect("same complete native receipt on stderr");
    assert_eq!(report["status"], "documentation-assembled-local");
    assert!(report["directory"].is_null());
    assert_eq!(report["jobs"].as_array().expect("completed jobs").len(), 1);
    assert_eq!(
        report["jobs"][0]["evidence"]["version"]["stdout_sha256"],
        serde_json::to_value(source::Sha256Digest::of(b"owned-cli-job-1/1\n"))
            .expect("version identity")
    );
    let stages = fixture.stages();
    assert_eq!(stages.len(), 2, "native custody survives carrier failure");
    for path in &stages {
        assert!(
            ["input", "output"]
                .iter()
                .any(|field| report["jobs"][0][field]
                    == path.to_str().expect("exact retained locator")),
            "missing exact stage: {report}"
        );
    }
    let generated = stages
        .iter()
        .find(|path| path.join("reference.json").is_file())
        .expect("complete generated tree");
    assert_eq!(
        fs::read(generated.join("reference.json")).expect("retained output"),
        ARTIFACT
    );
    assert_eq!(
        fs::read(generated.join("reference.json.companion")).expect("retained companion"),
        b"companion"
    );
    fixture.assert_source_unchanged();
}

#[test]
fn later_job_failure_delivers_completed_prefix_not_only_its_count() {
    let fixture = Fixture::new(
        b"if [ \"$3\" = 2 ]; then printf partial > \"$2\"; exit 7; fi\n/bin/cat \"$1\" > \"$2\"\n",
        2,
    );
    let output = fixture.run(false, true);
    assert!(!output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).expect("compact failure custody");
    assert_eq!(report["code"], "documentation_response_unavailable");
    assert_eq!(report["failure"]["code"], "documentation_generator_exit");
    assert_eq!(
        report["completed"]
            .as_array()
            .expect("complete prefix")
            .len(),
        1
    );
    let completed = &report["completed"][0];
    for (index, evidence) in [(1, completed), (2, &report["failure"])] {
        let version = format!("owned-cli-job-{index}/1\n");
        assert_eq!(
            evidence["version"]["stdout_sha256"],
            serde_json::to_value(source::Sha256Digest::of(version.as_bytes()))
                .expect("exact version digest")
        );
        assert_eq!(evidence["version"]["stdout_bytes"], version.len());
        assert!(
            evidence.get("version_text").is_none(),
            "compact evidence retains digests, not unbounded text"
        );
    }
    assert_eq!(completed["generation"]["exit"]["code"], 0);
    assert_eq!(report["failure"]["generation"]["exit"]["code"], 7);
    let stages = fixture.stages();
    assert_eq!(stages.len(), 4, "all retained stages, no retry or cleanup");
    for path in &stages {
        let expected =
            serde_json::to_value(path.as_os_str().as_bytes()).expect("exact Unix path bytes");
        assert!(
            report["retained_unix_bytes"]
                .as_array()
                .expect("retained locator set")
                .contains(&expected)
        );
    }
    assert_eq!(
        report["retained_unix_bytes"]
            .as_array()
            .expect("all locators")
            .len(),
        4
    );
    let complete_output = stages
        .iter()
        .find(|path| fs::read(path.join("reference.json")).is_ok_and(|bytes| bytes == ARTIFACT))
        .expect("completed output tree");
    assert_eq!(
        completed["output_unix_bytes"],
        serde_json::to_value(complete_output.as_os_str().as_bytes())
            .expect("native output locator")
    );
    assert_eq!(
        completed["output_identity"],
        serde_json::to_value(
            source::inspect(complete_output, None, Default::default())
                .expect("independent native read-back")
                .identity()
        )
        .expect("exact complete tree pin")
    );
    assert_eq!(
        completed["content"]["sha256"],
        serde_json::to_value(source::Sha256Digest::of(ARTIFACT)).expect("selected artifact pin")
    );
    let payloads: Vec<_> = stages
        .iter()
        .filter_map(|path| fs::read(path.join("reference.json")).ok())
        .collect();
    assert_eq!(payloads.len(), 2);
    assert!(payloads.iter().any(|bytes| bytes == ARTIFACT));
    assert!(payloads.iter().any(|bytes| bytes == b"partial"));
    fixture.assert_source_unchanged();
}

#[test]
fn denied_generation_creates_no_job_or_destination() {
    let fixture = Fixture::new(COPY, 1);
    let output = fixture.run(false, false);
    assert!(!output.status.success());
    assert!(
        output.stdout.is_empty(),
        "permission fails before native work"
    );
    let stderr = String::from_utf8(output.stderr).expect("typed pre-effect refusal");
    assert!(stderr.contains("documentation_permission"), "{stderr}");
    assert!(fixture.stages().is_empty());
    fixture.assert_source_unchanged();
}

#[test]
fn insufficient_response_budget_refuses_before_any_generator_stage_is_created() {
    let fixture = Fixture::new(COPY, 1);
    let output = fixture.run_bounded(false, true, Some(128));
    assert!(!output.status.success());
    assert!(
        output.stdout.is_empty(),
        "no native operation or successful receipt"
    );
    let stderr = String::from_utf8(output.stderr).expect("pre-effect refusal");
    assert!(stderr.contains("documentation_response_limit"), "{stderr}");
    assert!(
        fixture.stages().is_empty(),
        "capacity admission precedes stage creation and commands"
    );
    fixture.assert_source_unchanged();
}

#[test]
fn incompatible_explicit_navigation_refuses_before_cli_or_mcp_generator_effects() {
    let mut fixture = Fixture::new(COPY, 1);
    assert_eq!(fixture.config.matches("title='Library'\ninput=").count(), 1);
    fixture.config = fixture.config.replace(
        "title='Library'\ninput=",
        "title='Library'\npages=[{kind='page',path='index'}]\ninput=",
    );
    let output = fixture.run(false, true);
    assert!(!output.status.success());
    let cli: Value =
        serde_json::from_slice(&output.stdout).expect("complete pre-generation refusal");
    assert_eq!(cli["failure"]["code"], "documentation_target_input");
    assert_eq!(cli["completed"], serde_json::json!([]));
    assert_eq!(cli["retained_unix_bytes"], serde_json::json!([]));
    assert!(
        fixture.stages().is_empty(),
        "no input or output stage created"
    );
    carrier::initialized_interact("2026-07-28", |_, send| {
        let result = send(serde_json::json!({
            "jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":"assemble_docs","arguments":{
                "workspace":{"kind":"supplied","source":fixture.config},"site":"site:one",
                "local":true,"allow_read":true,"allow_stage":true,"directory_bytes":1024,
                "generation":{"parent":fixture.jobs.path(),"allow_execute":true,"max_jobs":1,"entry_bytes":128}
            }}
        }));
        assert_eq!(result["result"]["isError"], true, "{result}");
        let mcp = &result["result"]["structuredContent"];
        assert_eq!(mcp["failure"]["code"], cli["failure"]["code"]);
        assert_eq!(mcp["completed"], serde_json::json!([]));
        assert_eq!(mcp["retained_unix_bytes"], serde_json::json!([]));
    });
    assert!(
        fixture.stages().is_empty(),
        "neither request starts generation"
    );
    fixture.assert_source_unchanged();
}

#[test]
fn missing_generator_coordinates_in_a_later_job_refuse_before_any_stage() {
    let native = format!(
        "program={:?}",
        fs::canonicalize("/bin/sh").unwrap().to_str().unwrap()
    );
    for (anchor, replacement) in [
        (
            native.clone(),
            "program='machine:unbound/python'".to_owned(),
        ),
        (
            native.clone(),
            format!("{native},environment={{PYTHONPATH={{path='machine:unbound/packages'}}}}"),
        ),
        (native.clone(), "program='tools/python'".to_owned()),
        (
            "arguments=[{kind='input',path='generator.sh'}".into(),
            "arguments=[{kind='path',path='machine:unbound/generator.sh'}".into(),
        ),
        (
            "version_arguments=['-c'".into(),
            "version_arguments=[{path='machine:unbound/version.sh'}".into(),
        ),
        (
            native.clone(),
            format!(
                "{native},validation_arguments=[{{kind='output'}},{{kind='path',path='machine:unbound/validator'}}]"
            ),
        ),
    ] {
        let mut fixture = Fixture::new(COPY, 2);
        let offset = fixture.config.rfind(&anchor).unwrap();
        fixture
            .config
            .replace_range(offset..offset + anchor.len(), &replacement);
        let output = fixture.run(false, true);
        assert!(!output.status.success());
        let report: Value = serde_json::from_slice(&output.stdout).expect("binding failure report");
        assert_eq!(
            report["failure"]["code"], "runtime_command_binding",
            "{report}"
        );
        assert!(
            fixture.stages().is_empty(),
            "all bindings precede the first stage"
        );
        fixture.assert_source_unchanged();
    }
}

#[test]
fn generator_input_limits_bound_repeated_copies_independently_from_output() {
    let fixture = Fixture::new(COPY, 2);
    let input_bytes = 2 * (COPY.len() + ARTIFACT.len());
    let largest_file = COPY.len().max(ARTIFACT.len());
    for (flag, value, code) in [
        (
            "--max-generator-input-entries",
            3,
            "documentation_generation_limit",
        ),
        (
            "--max-generator-input-total-bytes",
            input_bytes - 1,
            "documentation_generation_limit",
        ),
        (
            "--max-generator-input-depth",
            0,
            "documentation_generation_stage",
        ),
        (
            "--max-generator-input-file-bytes",
            largest_file - 1,
            "documentation_generation_stage",
        ),
    ] {
        let output = fixture.run_with_arguments(false, true, None, &[flag, &value.to_string()]);
        assert!(
            !output.status.success(),
            "{flag} must constrain generator input"
        );
        let report: Value = serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|_| panic!("{flag}: {}", String::from_utf8_lossy(&output.stderr)));
        assert_eq!(report["failure"]["code"], code, "{flag}: {report}");
        assert!(
            fixture.stages().is_empty(),
            "{flag} refuses before staging or execution"
        );
        fixture.assert_source_unchanged();
    }
    let output = fixture.run_with_arguments(
        false,
        true,
        None,
        &[
            "--max-generator-input-entries",
            "4",
            "--max-generator-input-depth",
            "1",
            "--max-generator-input-file-bytes",
            &largest_file.to_string(),
            "--max-generator-input-total-bytes",
            &input_bytes.to_string(),
        ],
    );
    assert!(
        output.status.success(),
        "{} {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["jobs"].as_array().unwrap().len(), 2);
    assert_eq!(
        fixture.stages().len(),
        4,
        "exact input limits allow both jobs and their independent output trees"
    );
    fixture.assert_source_unchanged();
}
