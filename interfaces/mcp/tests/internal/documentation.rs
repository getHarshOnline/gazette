//! Real configured source/command/assembly paths through the named MCP route.

#[path = "documentation/preparation.rs"]
mod preparation;

use super::*;
use git2::{Repository, Signature};
use serde_json::json;
use std::{fs, os::unix::ffi::OsStrExt, path::Path};
use tempfile::TempDir;

const ARTIFACT: &[u8] =
    br#"{"id":0,"name":"Owned MCP fixture","kind":1,"variant":"project","schemaVersion":"2.0"}"#;
const COPY: &[u8] = b"/bin/cat \"$1\" > \"$2\"\nprintf companion > \"${2}.companion\"\n";
const MAXIMUM: usize = 256 << 10;

struct Fixture {
    _source: TempDir,
    jobs: TempDir,
    target: TempDir,
    config: String,
}

impl Fixture {
    fn new(script: &[u8], generated: usize) -> Self {
        let root = tempfile::tempdir().expect("native source");
        let jobs = tempfile::tempdir().expect("job parent");
        let target = tempfile::tempdir().expect("independent output parent");
        fs::create_dir(root.path().join("inputs")).expect("selected subtree");
        fs::write(root.path().join("inputs/generator.sh"), script)
            .expect("owned transport fixture");
        fs::write(root.path().join("inputs/payload.json"), ARTIFACT).expect("native artifact");
        let repo = Repository::init(root.path()).expect("native Git");
        let mut index = repo.index().expect("fixture index");
        index
            .add_all(["inputs"], git2::IndexAddOption::DEFAULT, None)
            .expect("fixture bytes");
        let tree = repo
            .find_tree(index.write_tree().expect("tree identity"))
            .expect("tree");
        let signature =
            Signature::new("Fixture", "fixture@example.invalid", &git2::Time::new(0, 0))
                .expect("fixed signature");
        let commit = repo
            .commit(Some("HEAD"), &signature, &signature, "fixture", &tree, &[])
            .expect("source commit");
        let program = fs::canonicalize("/bin/sh").expect("explicit fixture interpreter");
        let program = program.to_str().expect("fixture interpreter path");
        let executable = fs::read(program).expect("fixture interpreter bytes");
        let mut config = format!(
            "[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n[[sources]]\nid='source:one'\nlocation={{kind='repository',root={:?},commit='{commit}',subdirectory='inputs'}}\n",
            root.path().to_str().expect("source path")
        );
        for index in 0..=generated {
            config.push_str(&format!("[[projects]]\nid='project:p{index}'\nlifecycle='active'\nvisibility='public'\navailability='available'\n"));
        }
        config.push_str(&format!("[[brands]]\nid='brand:one'\nname='Configured publication'\n[[brands.publications]]\nsite='site:one'\n[brands.publications.documentation_repository]\nname='selected-domain-doc'\npath={:?}\n[brands.publications.documentation_repository.build]\ntarget={{kind='mintlify',theme='mint',primary_color='#123abc'}}\n", target.path().join("brand-destination").to_str().expect("configured path")));
        for index in 0..=generated {
            config.push_str(&format!("[[brands.publications.documentation_repository.build.modules]]\nproject='project:p{index}'\nslug='module-{index}'\ntitle='Module {index}'\n[[brands.publications.documentation_repository.build.modules.navigation]]\nkind='reference'\nslug='library'\ntitle='Library'\n"));
            if index == 0 {
                config.push_str("input={kind='artifact',source='source:one',selection={kind='file',path='payload.json'},format='typedoc-json'}\n");
            } else {
                let version = format!("owned-mcp-{index}/1\n");
                config.push_str(&format!("input={{kind='generated',source='source:one',recipe={{program={program:?},executable={{bytes={},sha256='{}'}},arguments=[{{kind='input',path='generator.sh'}},{{kind='input',path='payload.json'}},{{kind='output'}},{{kind='literal',value='{index}'}}],version_arguments=['-c',\"printf 'owned-mcp-{index}/1\\n'\"],expected_version_sha256='{}',output={{kind='file',path='reference.json'}},format='typedoc-json'}}}}\n", executable.len(), source::Sha256Digest::of(&executable), source::Sha256Digest::of(version.as_bytes())));
            }
        }
        Self {
            _source: root,
            jobs,
            target,
            config,
        }
    }

    fn arguments(&self, generated: usize, stage: bool) -> Value {
        let mut value = json!({"workspace":{"kind":"supplied","source": self.config}, "site":"site:one", "local":true,"allow_read":true,"allow_stage":true,"directory_bytes":1024});
        if generated > 0 {
            value["generation"] = json!({"parent":self.jobs.path(),"max_jobs":generated,"entry_bytes":1024,"allow_execute":true});
        }
        if stage {
            value["output_parent"] = json!(self.target.path());
        }
        value
    }

    fn stages(&self) -> Vec<PathBuf> {
        assert!(!self.target.path().join("brand-destination").exists());
        fs::read_dir(self.jobs.path())
            .expect("job parent")
            .map(|entry| {
                entry
                    .expect("retained job")
                    .path()
                    .canonicalize()
                    .expect("exact native path")
            })
            .collect()
    }
}

fn call(arguments: Value, maximum: usize) -> Result<CallToolResult, ErrorData> {
    crate::tools::Route::named("assemble_docs")
        .expect("discoverable route")
        .decode(arguments.as_object().expect("named arguments").clone())?
        .execute(
            &crate::authoring::Sessions::default(),
            &crate::release_session::Sessions::default(),
            &crate::recovery_session::Sessions::default(),
            &crate::store_session::Sessions::default(),
            &crate::preview::Sessions::default(),
            maximum,
        )
}

fn report(result: CallToolResult, failed: bool) -> Value {
    assert_eq!(result.is_error, Some(failed));
    let encoded = serde_json::to_value(result).expect("native MCP response");
    let value = encoded["structuredContent"].clone();
    let text: Value = serde_json::from_str(
        encoded["content"][0]["text"]
            .as_str()
            .expect("readable payload"),
    )
    .expect("same JSON payload");
    assert_eq!(value, text);
    let tool = json!(descriptor());
    jsonschema::validator_for(&tool["outputSchema"])
        .expect("native output schema")
        .validate(&value)
        .expect("actual output conforms");
    value
}

fn replace_first_generated_version(fixture: &mut Fixture, command: &str, stdout: &[u8]) {
    let original_command = "printf 'owned-mcp-1/1\\n'";
    let original_digest = source::Sha256Digest::of(b"owned-mcp-1/1\n").to_string();
    let replacement_digest = source::Sha256Digest::of(stdout).to_string();
    assert!(fixture.config.contains(original_command));
    assert!(fixture.config.contains(&original_digest));
    fixture.config = fixture.config.replacen(original_command, command, 1);
    fixture.config = fixture
        .config
        .replacen(&original_digest, &replacement_digest, 1);
    assert!(fixture.config.contains(command));
    assert!(fixture.config.contains(&replacement_digest));
}

#[test]
fn discovery_and_parser_match_closed_native_options_bounds_and_optional_nulls() {
    let fixture = Fixture::new(COPY, 1);
    let valid = fixture.arguments(1, true);
    let tool = json!(descriptor());
    let schema = jsonschema::validator_for(&tool["inputSchema"]).expect("input schema");
    assert!(schema.is_valid(&valid));
    assert_eq!(
        tool["annotations"],
        json!({"readOnlyHint":false,"destructiveHint":true,"idempotentHint":false,"openWorldHint":true})
    );
    for (pointer, invalid) in [
        ("/syntax", json!([])),
        ("/acquisition", Value::Null),
        ("/output", json!([])),
        ("/generation/input", json!([])),
        ("/generation/max_jobs", json!(129)),
        ("/generation/entry_bytes", json!(0)),
        ("/generation/timeout_ms", json!(0)),
        ("/generation/parent", json!("relative")),
        ("/output_parent", json!("relative")),
        ("/directory_bytes", json!(0)),
        ("/generation/unknown", Value::Null),
    ] {
        let mut value = valid.clone();
        let (parent, field) = pointer.rsplit_once('/').expect("field");
        value.pointer_mut(parent).expect("parent")[field] = invalid;
        assert!(!schema.is_valid(&value), "{pointer}: {value}");
        assert!(decode(value.as_object().expect("arguments").clone()).is_err());
    }
    let mut nulls = fixture.arguments(0, false);
    nulls["generation"] = Value::Null;
    nulls["output_parent"] = Value::Null;
    assert!(schema.is_valid(&nulls));
    assert!(decode(nulls.as_object().expect("arguments").clone()).is_ok());
    let mut unknown = valid;
    unknown["allow_network"] = json!(true);
    assert!(!schema.is_valid(&unknown));
    assert!(decode(unknown.as_object().expect("arguments").clone()).is_err());
}

#[test]
fn read_stage_and_execution_permissions_precede_native_work_independently() {
    let fixture = Fixture::new(COPY, 1);
    for pointer in [
        "/local",
        "/allow_read",
        "/allow_stage",
        "/generation/allow_execute",
    ] {
        let mut value = fixture.arguments(1, true);
        *value.pointer_mut(pointer).expect("permission") = json!(false);
        let result = report(call(value, MAXIMUM).expect("typed refusal"), true);
        assert_eq!(result["code"], "documentation_permission");
        assert!(fixture.stages().is_empty());
        assert!(
            fs::read_dir(fixture.target.path())
                .expect("target parent")
                .next()
                .is_none()
        );
    }
}

#[test]
fn mixed_modules_generate_once_and_share_existing_assembly_with_exact_retention() {
    let fixture = Fixture::new(COPY, 2);
    let value = report(
        call(fixture.arguments(2, true), MAXIMUM).expect("native complete path"),
        false,
    );
    assert_eq!(value["repository"]["name"], "selected-domain-doc");
    assert_eq!(value["jobs"].as_array().expect("native jobs").len(), 2);
    assert_eq!(
        value["artifacts"].as_array().expect("mixed modules").len(),
        3
    );
    assert!(value["artifacts"][0].get("generation").is_none());
    let retained = Path::new(value["directory"].as_str().expect("retained target"));
    for index in 0..3 {
        assert_eq!(
            fs::read(retained.join(format!("artifacts/module-{index}/library.json")))
                .expect("target artifact"),
            ARTIFACT
        );
    }
    let navigation: Value =
        serde_json::from_slice(&fs::read(retained.join("docs.json")).expect("navigation"))
            .expect("target format");
    let expected_dropdowns: Vec<_> = (0..3)
        .map(|index| {
            json!({
                "dropdown": format!("Module {index}"),
                "pages": [{
                    "group": "Library",
                    "sdk": {
                        "format": "typedoc",
                        "source": format!("artifacts/module-{index}/library.json"),
                        "directory": format!("module-{index}/library"),
                    },
                }],
            })
        })
        .collect();
    assert_eq!(
        navigation["navigation"],
        json!({"dropdowns": expected_dropdowns})
    );
    for job in value["jobs"].as_array().expect("job receipts") {
        let path = Path::new(job["output"].as_str().expect("native output"));
        assert_eq!(
            job["output_identity"],
            json!(
                source::inspect(path, None, Default::default())
                    .expect("independent native read-back")
                    .identity()
            )
        );
        assert_eq!(
            fs::read(path.join("reference.json.companion")).expect("native companion"),
            b"companion"
        );
    }
    assert_eq!(fixture.stages().len(), 4);
}

#[test]
fn imported_pure_and_retained_paths_need_no_generator_permission() {
    let fixture = Fixture::new(COPY, 0);
    let mut pure = fixture.arguments(0, false);
    pure["allow_stage"] = json!(false);
    let value = report(call(pure, MAXIMUM).expect("pure imported assembly"), false);
    assert!(value["directory"].is_null());
    assert!(
        value["jobs"]
            .as_array()
            .expect("no synthetic jobs")
            .is_empty()
    );
    let retained = report(
        call(fixture.arguments(0, true), MAXIMUM).expect("bounded imported retention"),
        false,
    );
    assert!(
        Path::new(
            retained["directory"]
                .as_str()
                .expect("native retained path")
        )
        .is_dir()
    );
    assert!(fixture.stages().is_empty());
}

#[test]
fn late_generation_failure_returns_all_original_pins_and_residue_without_partial_success() {
    let fixture = Fixture::new(
        b"if [ \"$3\" = 2 ]; then printf partial > \"$2\"; exit 7; fi\n/bin/cat \"$1\" > \"$2\"\n",
        2,
    );
    let value = report(
        call(fixture.arguments(2, true), MAXIMUM).expect("native failure carrier"),
        true,
    );
    assert_eq!(value["failure"]["code"], "documentation_generator_exit");
    assert_eq!(
        value["completed"]
            .as_array()
            .expect("completed prefix")
            .len(),
        1
    );
    assert_eq!(value["failure"]["generation"]["exit"]["code"], 7);
    assert_eq!(
        value["completed"][0]["version"]["stdout_sha256"],
        json!(source::Sha256Digest::of(b"owned-mcp-1/1\n"))
    );
    assert_eq!(
        value["failure"]["version"]["stdout_sha256"],
        json!(source::Sha256Digest::of(b"owned-mcp-2/1\n"))
    );
    for path in fixture.stages() {
        assert!(
            value["retained_unix_bytes"]
                .as_array()
                .expect("all native locators")
                .contains(&json!(path.as_os_str().as_bytes()))
        );
    }
    assert_eq!(
        value["retained_unix_bytes"]
            .as_array()
            .expect("exact retained set")
            .len(),
        4
    );
    assert!(
        fs::read_dir(fixture.target.path())
            .expect("no final target")
            .next()
            .is_none()
    );
}

#[test]
fn insufficient_response_and_native_path_bounds_refuse_without_stage_creation() {
    let fixture = Fixture::new(COPY, 1);
    assert!(call(fixture.arguments(1, true), 128).is_err());
    assert!(fixture.stages().is_empty());
    let mut value = fixture.arguments(1, true);
    value["directory_bytes"] = json!(1);
    let result = report(call(value, MAXIMUM).expect("native bound refusal"), true);
    assert!(result["failure"]["code"].is_string());
    assert!(fixture.stages().is_empty());
    assert!(
        fs::read_dir(fixture.target.path())
            .expect("target unchanged")
            .next()
            .is_none()
    );
}

#[test]
fn obsolete_or_wrong_selection_shapes_fail_before_native_generation() {
    let fixture = Fixture::new(COPY, 1);
    for (original, replacement) in [
        (
            "selection={kind='file',path='payload.json'}",
            "entry='payload.json'",
        ),
        (
            "selection={kind='file',path='payload.json'}",
            "selection='payload.json'",
        ),
        (
            "selection={kind='file',path='payload.json'}",
            "selection={kind='file'}",
        ),
        (
            "selection={kind='file',path='payload.json'}",
            "selection={kind='directory',path='../escape'}",
        ),
        (
            "output={kind='file',path='reference.json'}",
            "output='reference.json'",
        ),
        (
            "output={kind='file',path='reference.json'}",
            "output={kind='file',path='reference.json',entry='old'}",
        ),
    ] {
        let mut arguments = fixture.arguments(1, true);
        assert!(fixture.config.contains(original));
        arguments["workspace"]["source"] = json!(fixture.config.replace(original, replacement));
        let result = call(arguments, MAXIMUM).expect("typed compiler refusal");
        assert_eq!(result.is_error, Some(true), "{replacement}");
        assert!(fixture.stages().is_empty(), "{replacement}");
        assert!(
            fs::read_dir(fixture.target.path())
                .expect("unchanged target")
                .next()
                .is_none()
        );
    }
}

#[test]
fn root_directory_generation_preserves_directory_custody_without_file_digest_or_compatibility_claim()
 {
    let mut fixture = Fixture::new(
        b"/bin/cat \"$1\" > \"$2/reference.json\"\nprintf companion > \"$2/reference.json.companion\"\n", 1,
    );
    fixture.config = fixture.config.replace(
        "output={kind='file',path='reference.json'}",
        "output={kind='directory'}",
    );
    let value = report(
        call(fixture.arguments(1, true), MAXIMUM)
            .expect("native directory output and target refusal"),
        true,
    );
    assert_eq!(value["failure"]["code"], "documentation_target_input");
    assert_eq!(
        value["completed"][0]["selection"],
        json!({"kind":"directory"})
    );
    assert_eq!(
        value["completed"][0]["content"],
        json!({"kind":"directory"})
    );
    assert!(value["completed"][0].get("sha256").is_none());
    assert!(value["completed"][0].get("entry").is_none());
    let output: Vec<u8> =
        serde_json::from_value(value["completed"][0]["output_unix_bytes"].clone())
            .expect("exact retained directory");
    use std::os::unix::ffi::OsStringExt;
    let output = PathBuf::from(std::ffi::OsString::from_vec(output));
    assert_eq!(
        fs::read(output.join("reference.json")).expect("complete output file"),
        ARTIFACT
    );
    assert_eq!(
        fs::read(output.join("reference.json.companion")).expect("companion"),
        b"companion"
    );
    assert_eq!(
        value["completed"][0]["output_identity"],
        json!(
            source::inspect(&output, None, Default::default())
                .expect("independent complete tree")
                .identity()
        )
    );
    assert_eq!(fixture.stages().len(), 2);
    assert!(
        fs::read_dir(fixture.target.path())
            .expect("target not staged")
            .next()
            .is_none()
    );
}

#[test]
fn native_rustdoc_format_is_refused_before_generation_without_claiming_conversion() {
    let mut fixture = Fixture::new(COPY, 1);
    fixture.config = fixture.config.replacen(
        "output={kind='file',path='reference.json'},format='typedoc-json'",
        "output={kind='file',path='reference.json'},format='rustdoc-json'",
        1,
    );
    let value = report(
        call(fixture.arguments(1, true), MAXIMUM).expect("target refusal with custody"),
        true,
    );
    assert_eq!(value["failure"]["code"], "documentation_target_input");
    assert_eq!(
        value["completed"]
            .as_array()
            .expect("no native generation evidence")
            .len(),
        0
    );
    assert!(value["output_identity"].is_null());
    assert!(
        fixture.stages().is_empty(),
        "unsupported target format must fail before the generator starts"
    );
    assert!(
        fs::read_dir(fixture.target.path())
            .expect("no target accepted")
            .next()
            .is_none()
    );
}

#[test]
fn missing_generation_selection_never_falls_back_to_imported_artifacts() {
    let fixture = Fixture::new(COPY, 1);
    let value = report(
        call(fixture.arguments(0, false), MAXIMUM).expect("missing selection"),
        true,
    );
    assert_eq!(value["code"], "documentation_generation_required");
    assert!(fixture.stages().is_empty());
}

#[test]
fn oversized_complete_report_returns_compact_original_custody_not_a_generic_limit() {
    let mut fixture = Fixture::new(COPY, 1);
    let version = [vec![b' '; 149_999], vec![b'x']].concat();
    replace_first_generated_version(&mut fixture, "printf '%150000s' x", &version);
    let value = report(
        call(fixture.arguments(1, false), MAXIMUM).expect("reserved custody"),
        true,
    );
    assert_eq!(value["code"], "documentation_response_unavailable");
    assert!(
        value["failure"]["code"].is_null(),
        "generation and assembly completed; delivery fell back"
    );
    assert_eq!(
        value["completed"].as_array().expect("complete jobs").len(),
        1
    );
    assert_eq!(value["completed"][0]["version"]["stdout_bytes"], 150000);
    assert!(value["completed"][0].get("version_text").is_none());
    assert!(value["output_identity"]["sha256"].is_string());
    assert_eq!(fixture.stages().len(), 2, "no retry or cleanup");
}
