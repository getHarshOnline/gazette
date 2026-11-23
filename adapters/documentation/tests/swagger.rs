//! Explicit installed go-swagger qualification, not Go library or hosting support.
//! Set `GAZETTE_TEST_SWAGGER` and `GAZETTE_TEST_GO_ROOT` to exact absolute native paths.

mod support;

use gazette_adapter_documentation::NativeGenerator;
use gazette_adapter_fs::FileSystem;
use gazette_adapter_process::Runner;
use gazette_core::{
    documentation::{
        Format, Selection,
        generator::{Argument, Cause, Phase, Recipe},
    },
    source::{Limits, Sha256Digest},
};
use gazette_ports::{documentation::Generator as _, source::SourceCapture as _};
use serde_json::Value;
use std::{collections::BTreeMap, fs, path::PathBuf};
use support::{Job, pin};

fn job(source: &[u8], selected: &str) -> (Job, tempfile::TempDir) {
    let program = PathBuf::from(
        std::env::var_os("GAZETTE_TEST_SWAGGER").expect("explicit installed Swagger executable"),
    );
    let go = PathBuf::from(
        std::env::var_os("GAZETTE_TEST_GO_ROOT").expect("explicit installed Go root"),
    );
    assert!(program.is_absolute() && go.is_absolute());
    assert!(
        go.join("bin/go").is_file(),
        "no toolchain download or PATH discovery"
    );
    let cache = tempfile::tempdir().expect("disposable tool state");
    let mut environment = BTreeMap::from([
        (
            "PATH".into(),
            go.join("bin").to_str().expect("Go binary directory").into(),
        ),
        ("GOROOT".into(), go.to_str().expect("Go root").into()),
        ("GOTOOLCHAIN".into(), "local".into()),
        ("GOENV".into(), "off".into()),
        ("GOWORK".into(), "off".into()),
        ("GOPROXY".into(), "off".into()),
        ("GOSUMDB".into(), "off".into()),
        ("GOFLAGS".into(), "-mod=readonly".into()),
        ("CGO_ENABLED".into(), "0".into()),
    ]);
    for (variable, name) in [
        ("GOCACHE", "build"),
        ("GOMODCACHE", "modules"),
        ("GOPATH", "gopath"),
        ("TMPDIR", "tmp"),
    ] {
        let path = cache.path().join(name);
        fs::create_dir(&path).expect("owned isolated cache directory");
        environment.insert(variable.into(), path.to_str().expect("cache path").into());
    }
    let version_arguments = vec!["version".into()];
    let version = std::process::Command::new(&program)
        .args(&version_arguments)
        .env_clear()
        .envs(&environment)
        .output()
        .expect("observe selected Swagger version");
    assert!(version.status.success(), "selected Swagger version failed");
    let recipe = Recipe {
        executable: pin(&program),
        program: program.to_str().expect("exact program").into(),
        arguments: vec![
            Argument::Literal {
                value: "generate".into(),
            },
            Argument::Literal {
                value: "spec".into(),
            },
            Argument::Literal {
                value: "--scan-models".into(),
            },
            Argument::Literal {
                value: "--output".into(),
            },
            Argument::Output,
            Argument::Input {
                path: selected.parse().expect("exact required source"),
            },
        ],
        environment,
        version_arguments,
        expected_version_sha256: Sha256Digest::of(&version.stdout),
        validation_arguments: Some(vec![
            Argument::Literal {
                value: "validate".into(),
            },
            Argument::Output,
        ]),
        output: Selection::File {
            path: "swagger.json".parse().expect("selected output"),
        },
        format: Format::SwaggerJson,
    };
    (
        Job::new(
            &[
                ("api.go", source),
                ("go.mod", include_bytes!("fixtures/swagger/go.mod")),
            ],
            recipe,
        ),
        cache,
    )
}

#[test]
#[ignore = "requires explicit GAZETTE_TEST_SWAGGER and GAZETTE_TEST_GO_ROOT provider qualification"]
fn installed_swagger_generates_retained_http_contract_and_reuses_exact_capture() {
    let (job, _cache) = job(include_bytes!("fixtures/swagger/api.go"), "api.go");
    let generator = NativeGenerator::new(&FileSystem, &Runner);
    let generated = generator
        .generate(job.request())
        .expect("real installed Swagger generation");
    assert!(
        generated
            .evidence
            .version_text
            .as_deref()
            .expect("native version")
            .contains("v0.34.1")
    );
    assert_eq!(generated.evidence.input, job.input.inventory().identity());
    let path = generated.selection.path().expect("exact selected output");
    let bytes = generated.output.file(path).expect("retained native JSON");
    let model: Value = serde_json::from_slice(bytes).expect("actual Swagger JSON");
    assert_eq!(model["swagger"], "2.0");
    assert_eq!(model["host"], "api.example.invalid");
    assert_eq!(model["basePath"], "/v1");
    assert_eq!(model["info"]["version"], "0.1.0");
    let operation = &model["paths"]["/widgets/{id}"]["get"];
    assert_eq!(operation["operationId"], "getWidget");
    assert_eq!(operation["tags"], serde_json::json!(["widgets"]));
    let parameters = operation["parameters"]
        .as_array()
        .expect("native HTTP parameters");
    let id = parameters
        .iter()
        .find(|item| item["name"] == "id")
        .expect("path parameter");
    assert_eq!(id["in"], "path");
    assert_eq!(id["required"], true);
    assert_eq!(id["type"], "string");
    let verbose = parameters
        .iter()
        .find(|item| item["name"] == "verbose")
        .expect("query parameter");
    assert_eq!(verbose["in"], "query");
    assert_eq!(verbose["type"], "boolean");
    assert_eq!(
        operation["responses"]["200"]["$ref"],
        "#/responses/WidgetResponse"
    );
    assert_eq!(
        model["responses"]["WidgetResponse"]["schema"]["$ref"],
        "#/definitions/Widget"
    );
    assert_eq!(
        model["definitions"]["Widget"]["properties"]["id"]["type"],
        "string"
    );
    assert_eq!(
        model["definitions"]["Widget"]["required"],
        serde_json::json!(["id"])
    );
    assert_eq!(model["securityDefinitions"]["apiKey"]["type"], "apiKey");
    assert_eq!(model["securityDefinitions"]["apiKey"]["in"], "header");
    assert_eq!(
        model["securityDefinitions"]["apiKey"]["name"],
        "X-Fixture-Key"
    );
    assert_eq!(operation["security"], serde_json::json!([{"apiKey":[]}]));
    let identity = generated.output.inventory().identity();
    let validation = generated
        .evidence
        .validation
        .as_ref()
        .expect("native output validation is separately retained");
    assert_eq!(validation.tree, identity);
    assert_eq!(
        validation
            .invocation
            .exit
            .expect("native validation exit")
            .code,
        Some(0)
    );
    let native = FileSystem
        .capture(job.output.path(), None, Limits::default())
        .expect("independent complete native read-back");
    assert_eq!(native.inventory(), generated.output.inventory());
    assert_eq!(native.file(path), Some(bytes));
    let hash = Sha256Digest::of(bytes);
    let failure = generator
        .generate(job.request())
        .expect_err("occupied output cannot be regenerated in place");
    assert!(matches!(failure.phase, Phase::Preflight));
    assert!(failure.evidence.version.is_none());
    assert!(failure.evidence.generation.is_none());
    assert!(failure.evidence.validation.is_none());
    assert_eq!(generated.output.inventory().identity(), identity);
    let selected = path.clone();
    drop(job);
    assert_eq!(
        Sha256Digest::of(
            generated
                .output
                .file(&selected)
                .expect("retained artifact reused without native acquisition")
        ),
        hash
    );
}

#[test]
#[ignore = "requires explicit GAZETTE_TEST_SWAGGER and GAZETTE_TEST_GO_ROOT provider qualification"]
fn missing_and_invalid_required_go_sources_never_return_a_partial_artifact() {
    let generator = NativeGenerator::new(&FileSystem, &Runner);
    let (missing, _cache) = job(include_bytes!("fixtures/swagger/api.go"), "absent.go");
    let failure = generator
        .generate(missing.request())
        .expect_err("missing required source");
    assert!(matches!(failure.phase, Phase::Preflight));
    assert!(failure.evidence.version.is_none());
    assert!(matches!(
        failure.cause,
        Cause::Invalid("documentation_generator_input_missing")
    ));
    let (invalid, _cache) = job(b"package fixture\nfunc broken(\n", "api.go");
    let original = fs::read(invalid.input.path().join("api.go")).expect("exact malformed source");
    let failure = generator
        .generate(invalid.request())
        .expect_err("successful codescan must not admit invalid generated Swagger");
    assert!(matches!(failure.phase, Phase::Validation), "{failure}");
    assert!(failure.evidence.version.is_some());
    assert_eq!(
        failure
            .evidence
            .generation
            .as_ref()
            .expect("codescan returned success despite invalid Go syntax")
            .exit
            .expect("native generation exit")
            .code,
        Some(0)
    );
    let validation = failure
        .evidence
        .validation
        .as_ref()
        .expect("failed native validator retains its exact input tree and invocation");
    assert_ne!(
        validation
            .invocation
            .exit
            .expect("native validation exit")
            .code,
        Some(0)
    );
    let rejected = FileSystem
        .capture(invalid.output.path(), None, Limits::default())
        .expect("independent read-back of rejected output custody");
    assert_eq!(validation.tree, rejected.inventory().identity());
    assert_eq!(
        fs::read(invalid.input.path().join("api.go")).expect("preserved failure source"),
        original
    );
    assert!(
        invalid.output.path().is_dir(),
        "failed job custody is retained"
    );
}
