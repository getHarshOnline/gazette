//! Real tool dispatch refusals and discovery; native rendering fixtures cover the connected path.

use super::*;
use serde_json::json;

fn request() -> Value {
    json!({"recipe_json":"{}","local":true,"allow_read":true})
}

fn decode_value(value: Value) -> Result<Input, ErrorData> {
    decode(value.as_object().expect("arguments").clone())
}

fn checked(value: Value) -> Value {
    let result = decode_value(value)
        .expect("argument shape")
        .execute(8192)
        .expect("tool result");
    assert_eq!(result.is_error, Some(true));
    let value = result
        .structured_content
        .expect("complete structured error");
    let descriptor = serde_json::to_value(descriptor()).expect("tool discovery");
    jsonschema::validator_for(&descriptor["outputSchema"])
        .expect("schema")
        .validate(&value)
        .expect("actual native result");
    value
}

#[test]
fn local_and_read_permissions_are_independent_and_precede_acquisition() {
    for local in [false, true] {
        for read in [false, true] {
            if local && read {
                continue;
            }
            let mut value = request();
            value["local"] = json!(local);
            value["allow_read"] = json!(read);
            assert_eq!(checked(value)["code"], "publication_permission");
        }
    }
    let mut value = request();
    value.as_object_mut().expect("object").remove("allow_read");
    assert_eq!(checked(value)["code"], "publication_permission");
    assert_eq!(checked(request())["code"], "publication_set_input");
}

#[test]
fn discovery_describes_native_recipe_and_no_hidden_effect_permissions() {
    let tool = serde_json::to_value(descriptor()).expect("discovery");
    assert_eq!(tool["name"], "render_sites");
    assert_eq!(tool["annotations"]["readOnlyHint"], false);
    assert_eq!(tool["annotations"]["destructiveHint"], true);
    assert_eq!(tool["annotations"]["idempotentHint"], false);
    assert_eq!(tool["annotations"]["openWorldHint"], true);
    assert_eq!(
        tool["inputSchema"]["properties"]["recipe_json"]["contentSchema"],
        json!(build::LocalPublicationRecipe::input_schema())
    );
    let schema = jsonschema::validator_for(&tool["inputSchema"]).expect("input schema");
    for extra in [
        "allow_policy_execution",
        "create_stage",
        "initialize_worktree",
        "allow_network",
        "listen",
    ] {
        let mut value = request();
        value[extra] = json!(true);
        assert!(!schema.is_valid(&value));
        assert!(decode_value(value).is_err());
    }
}

#[test]
fn nested_native_option_arrays_and_nulls_are_not_accepted_as_structs() {
    for pointer in [
        "/syntax",
        "/output",
        "/acquisition",
        "/acquisition/sources",
        "/acquisition/sources/per_source",
        "/acquisition/sources/per_source/content",
        "/corpus",
        "/corpus/document",
        "/corpus/document/metadata",
        "/corpus/document/markdown",
        "/corpus/notebook",
    ] {
        for invalid in [json!([]), Value::Null] {
            let mut value = request();
            value["acquisition"] = json!(build::PublicationSetAcquisitionLimits::default());
            value["corpus"] = json!(gazette_cli::sdk::corpus::Limits::default());
            let (parent, field) = pointer.rsplit_once('/').expect("field");
            value.pointer_mut(parent).expect("parent")[field] = invalid;
            assert!(decode_value(value).is_err(), "invalid {pointer}");
        }
    }
}

#[test]
fn explicit_corpus_limits_reach_the_shared_render_command() {
    let mut value = request();
    value["corpus"] = json!({"source_bytes": 536870912});
    let command = decode_value(value)
        .expect("native corpus selection")
        .into_command()
        .expect("invocation calendar");
    assert!(command.default_render_year.is_some());
    assert_eq!(
        command.limits.publication.corpus.source_bytes,
        Some(536870912)
    );
    assert_eq!(command.limits.bytes, default_total_bytes());
    assert_eq!(command.limits.entries, default_total_entries());
}

#[test]
fn complete_recipe_ceiling_applies_before_workspace_parsing() {
    let mut value = request();
    value["max_input_bytes"] = json!(0);
    assert_eq!(checked(value)["code"], "publication_set_input");
}

#[test]
fn native_workspace_and_partial_limits_preserve_omission_and_call_permission() {
    let value = json!({"workspace":{"kind":"supplied","source":""},"local":true,"allow_read":true,"allow_execute":true,"corpus":{"files":2}});
    let command = decode_value(value.clone()).unwrap().into_command().unwrap();
    assert!(command.limits.publication.corpus.source_bytes.is_none());
    assert_eq!(command.limits.publication.corpus.files, 2);
    assert!(command.allow_math_execution);
    let descriptor = serde_json::to_value(descriptor()).unwrap();
    let schema = jsonschema::validator_for(&descriptor["inputSchema"]).unwrap();
    assert!(schema.is_valid(&value));
    for invalid in [
        json!({"local":true,"allow_read":true}),
        json!({"local":true,"allow_read":true,"workspace":{"kind":"supplied","source":""},"recipe_json":"{}"}),
        json!({"local":true,"allow_read":true,"recipe_json":"{}","corpus":{"source_bytes":null}}),
    ] {
        assert!(decode_value(invalid).is_err());
    }
}

#[test]
fn reconciliation_keeps_escaped_native_receipts_when_full_result_exceeds_capacity() {
    let receipt = json!({"site":"site:a\\\"b", "response":{"retained":["/tmp/quote\"\n\\\t"]},"is_error":false});
    let report = json!({"documentation":[receipt],"large_inspection":"x".repeat(40_000)});
    let result = deliver_report(&report, false, 4096).unwrap();
    assert_eq!(result.is_error, Some(true));
    let retained = result.structured_content.as_ref().unwrap();
    assert_eq!(retained["documentation"], report["documentation"]);
    assert_eq!(retained["status"], "documentation-publication-output-limit");
    assert!(serde_json::to_vec(&result).unwrap().len() <= 4096);
    let schema = serde_json::to_value(descriptor()).unwrap();
    assert!(
        jsonschema::validator_for(&schema["outputSchema"])
            .unwrap()
            .is_valid(retained)
    );
    let maximum = 8192;
    let native = json!({"path":"\u{0001}".repeat(native_capacity(maximum) / 6 - 20)});
    assert!(serde_json::to_vec(&native).unwrap().len() <= native_capacity(maximum));
    assert!(encode_framed(&native, true, maximum).is_ok());
    let exceptional = json!({"documentation":[],"custody":{"original":"x".repeat(8192)}});
    let error = deliver_report(&exceptional, true, 1024).unwrap_err();
    assert_eq!(error.data.unwrap()["custody"], exceptional["custody"]);
}

fn generated_request() -> (tempfile::TempDir, tempfile::TempDir, Value) {
    use std::fs;
    let root = tempfile::tempdir().unwrap();
    let stages = tempfile::tempdir().unwrap();
    for directory in ["content", "docs", "theme/templates"] {
        fs::create_dir_all(root.path().join(directory)).unwrap();
    }
    fs::write(root.path().join("content/index.md"), "+++\n[content]\nid='content:one'\nsite='site:one'\ntitle='One'\nlifecycle='draft'\nroute={path='/'}\nstructure={kind='page',family={kind='article'}}\n+++\n# One\n").unwrap();
    fs::write(root.path().join("theme/theme.toml"), "package='theme:one'\nnamespace='one'\ntemplates='templates'\nbrowser_theme_colors={light='#ffffff',dark='#000000'}\nrender={page='page.html'}\n").unwrap();
    fs::write(
        root.path().join("theme/templates/page.html"),
        "<h1>{{ content.title }}</h1>{{ body_html | safe }}",
    )
    .unwrap();
    fs::write(root.path().join("docs/start.md"), "# Start\n").unwrap();
    fs::write(
        root.path().join("docs/guide.md"),
        "# API\nNative provider words.\n",
    )
    .unwrap();
    fs::write(
        root.path().join("docs/generate.sh"),
        "/bin/cat \"$1\" > \"$2\"\n",
    )
    .unwrap();
    let program = fs::canonicalize("/bin/sh").unwrap();
    let executable = fs::read(&program).unwrap();
    let generated = format!(
        "input={{kind='generated',source='source:docs',recipe={{program={:?},executable={{bytes={},sha256='{}'}},arguments=[{{kind='input',path='generate.sh'}},{{kind='input',path='guide.md'}},{{kind='output'}}],version_arguments=['-c',\"printf 'mcp-catalog/1\\n'\"],expected_version_sha256='{}',output={{kind='file',path='api.md'}},format='markdown'}}}}",
        program.to_str().unwrap(),
        executable.len(),
        source::Sha256Digest::of(&executable),
        source::Sha256Digest::of(b"mcp-catalog/1\n")
    );
    let mut workspace = format!(
        "[build]\nsites=['site:one']\nlocal_drafts='include'\n[composition]\ntheme={{package='theme:one'}}\n[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n[[brands]]\nid='brand:one'\nname='Example'\n[[brands.publications]]\nsite='site:one'\npublic_base_url='https://one.example/'\nauthored=[{{source='source:content',target='content'}}]\ndocumentation_repository={{name='docs',path='/not-opened/docs',public_base_url='https://docs.example/',build={{target={{kind='mintlify',theme='mint',primary_color='#123456'}},modules=[{{project='project:library',slug='library',title='Library',manual={{source='source:docs'}},navigation=[{{kind='manual',title='Start',pages=[{{title='Start',route='library/start',input={{kind='file',path='start.md'}}}}]}},{{kind='reference',slug='reference',title='API',{generated}}}]}}]}}}}\n[[projects]]\nid='project:library'\nlifecycle='active'\nvisibility='public'\navailability='available'\n"
    );
    for name in ["content", "docs", "theme"] {
        workspace.push_str(&format!(
            "[[sources]]\nid='source:{name}'\nlocation={{kind='filesystem',root={:?}}}\n",
            root.path().join(name).to_str().unwrap()
        ));
    }
    workspace.push_str(
        "[[theme_packages]]\npackage='theme:one'\nsource='source:theme'\nmanifest='theme.toml'\n",
    );
    let recipe = json!({"format":"gazette-local-publication-set/1","workspace":{"kind":"supplied","source":workspace},"documentation_generation":{"parent":stages.path(),"allow_execute":true}});
    let arguments = json!({"recipe_json":recipe.to_string(),"local":true,"allow_read":true,"allow_execute":true});
    (root, stages, arguments)
}

#[test]
fn native_generation_reserves_framed_delivery_and_respects_outer_permission() {
    let (_root, stages, args) = generated_request();
    let denied = {
        let mut value = args.clone();
        value["allow_execute"] = false.into();
        value
    };
    let refusal = decode_value(denied).unwrap().execute(16 << 20).unwrap();
    assert_eq!(refusal.is_error, Some(true));
    assert_eq!(std::fs::read_dir(stages.path()).unwrap().count(), 0);
    let small = decode_value(args.clone()).unwrap().execute(1024).unwrap();
    assert_eq!(small.is_error, Some(true));
    assert_eq!(std::fs::read_dir(stages.path()).unwrap().count(), 0);
    let complete = decode_value(args).unwrap().execute(16 << 20).unwrap();
    assert_eq!(
        complete.is_error,
        Some(false),
        "{:?}",
        complete.structured_content
    );
    assert_eq!(std::fs::read_dir(stages.path()).unwrap().count(), 2);
    let value = complete.structured_content.as_ref().unwrap();
    assert_eq!(value["documentation"].as_array().unwrap().len(), 1);
    let schema = serde_json::to_value(descriptor()).unwrap();
    jsonschema::validator_for(&schema["outputSchema"])
        .unwrap()
        .validate(value)
        .unwrap();
    assert!(serde_json::to_vec(&complete).unwrap().len() <= 16 << 20);
}

#[test]
fn resource_file_override_preserves_other_native_render_bounds() {
    for requested in [None, Some(4 << 20)] {
        let mut value = request();
        if let Some(bytes) = requested {
            value["max_resource_file_bytes"] = json!(bytes);
        }
        let command = decode_value(value).unwrap().into_command().unwrap();
        let mut expected = gazette_cli::sdk::resource::Limits::default();
        if let Some(bytes) = requested {
            expected.inputs.file_bytes = bytes;
        }
        assert_eq!(
            serde_json::to_value(command.limits.publication.output.resources).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
    }
}
