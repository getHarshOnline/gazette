//! Installed go-swagger through configured generation, conversion and private staging.
//! Requires `GAZETTE_TEST_SWAGGER` and `GAZETTE_TEST_GO_ROOT`; never installs a tool or hosts docs.

use std::{collections::BTreeMap, fs, path::PathBuf, process::Command, time::Duration};

use gazette_core::documentation;
use gazette_ports::documentation::DocumentationTarget;
use gazette_sdk::{build, process, source};
use git2::{Repository, Signature};
use serde_json::{Value, json};
use tempfile::TempDir;

struct Fixture {
    source: TempDir,
    jobs: TempDir,
    cache: TempDir,
    destination: PathBuf,
    config: String,
}

impl Fixture {
    fn new() -> Self {
        let source = tempfile::tempdir().expect("isolated native fixture source");
        let jobs = tempfile::tempdir().expect("private native generation parent");
        let cache = tempfile::tempdir().expect("isolated Go state");
        let program = PathBuf::from(
            std::env::var_os("GAZETTE_TEST_SWAGGER")
                .expect("explicit installed Swagger executable"),
        );
        let go = PathBuf::from(
            std::env::var_os("GAZETTE_TEST_GO_ROOT").expect("explicit installed Go root"),
        );
        assert!(program.is_absolute() && go.is_absolute());
        assert!(
            go.join("bin/go").is_file(),
            "no toolchain download or PATH discovery"
        );
        let executable = fs::read(&program).expect("exact admitted native executable bytes");
        let mut environment = BTreeMap::from([
            (
                "PATH",
                go.join("bin")
                    .to_str()
                    .expect("Go binary directory")
                    .to_owned(),
            ),
            ("GOROOT", go.to_str().expect("Go root").to_owned()),
            ("GOTOOLCHAIN", "local".into()),
            ("GOENV", "off".into()),
            ("GOWORK", "off".into()),
            ("GOPROXY", "off".into()),
            ("GOSUMDB", "off".into()),
            ("GOFLAGS", "-mod=readonly".into()),
            ("CGO_ENABLED", "0".into()),
        ]);
        for (variable, name) in [
            ("GOCACHE", "build"),
            ("GOMODCACHE", "modules"),
            ("GOPATH", "gopath"),
            ("TMPDIR", "tmp"),
        ] {
            let path = cache.path().join(name);
            fs::create_dir(&path).expect("owned native cache directory");
            environment.insert(variable, path.to_str().expect("cache path").to_owned());
        }
        let version = Command::new(&program)
            .arg("version")
            .env_clear()
            .envs(environment.iter().map(|(key, value)| (*key, value)))
            .output()
            .expect("observe selected Swagger version");
        assert!(version.status.success(), "selected Swagger version failed");
        let expected_version_sha256 = source::Sha256Digest::of(&version.stdout);
        let environment = environment
            .iter()
            .map(|(key, value)| {
                format!(
                    "{key}={}",
                    serde_json::to_string(value).expect("TOML-compatible string")
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let repository = Repository::init(source.path()).expect("native fixture Git");
        fs::create_dir(source.path().join("inputs")).expect("explicit source subtree");
        let mut index = repository.index().expect("native index");
        for (name, bytes) in [
            (
                "api.go",
                include_bytes!("../../../adapters/documentation/tests/fixtures/swagger/api.go")
                    .as_slice(),
            ),
            (
                "go.mod",
                include_bytes!("../../../adapters/documentation/tests/fixtures/swagger/go.mod")
                    .as_slice(),
            ),
        ] {
            let path = PathBuf::from("inputs").join(name);
            fs::write(source.path().join(&path), bytes)
                .expect("existing owned Swagger source fixture");
            index.add_path(&path).expect("exact source entry");
        }
        let tree = repository
            .find_tree(index.write_tree().expect("source tree identity"))
            .expect("native tree");
        let signature =
            Signature::new("Fixture", "fixture@example.invalid", &git2::Time::new(0, 0))
                .expect("fixed native identity");
        let commit = repository
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                "Swagger fixture",
                &tree,
                &[],
            )
            .expect("exact native source pin");
        let destination = jobs.path().join("brand-selected-domain-doc");
        let config = format!(
            "[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n\
            [[projects]]\nid='project:widgets'\nlifecycle='active'\nvisibility='public'\navailability='available'\n\
            [[sources]]\nid='source:widgets'\nlocation={{kind='repository',root={:?},commit='{commit}',subdirectory='inputs'}}\n\
            [[brands]]\nid='brand:one'\nname='Configured Swagger publication'\n[[brands.publications]]\nsite='site:one'\n\
            [brands.publications.documentation_repository]\nname='owner-selected-docs'\npath={:?}\n\
            [brands.publications.documentation_repository.build]\ntarget={{kind='mintlify',theme='mint',primary_color='#123abc'}}\n\
            [[brands.publications.documentation_repository.build.modules]]\nproject='project:widgets'\nslug='widgets'\ntitle='Widget API'\n\
            [[brands.publications.documentation_repository.build.modules.navigation]]\nkind='reference'\nslug='http'\ntitle='HTTP reference'\n\
            input={{kind='generated',source='source:widgets',recipe={{program={:?},executable={{bytes={},sha256='{}'}},\
            arguments=[{{kind='literal',value='generate'}},{{kind='literal',value='spec'}},{{kind='literal',value='--scan-models'}},{{kind='literal',value='--output'}},{{kind='output'}},{{kind='input',path='api.go'}}],\
            environment={{{environment}}},version_arguments=['version'],expected_version_sha256='{expected_version_sha256}',validation_arguments=[{{kind='literal',value='validate'}},{{kind='output'}}],output={{kind='file',path='swagger.json'}},format='swagger-json'}}}}\n",
            source.path().to_str().expect("source path"),
            destination.to_str().expect("configured destination"),
            program.to_str().expect("absolute program"),
            executable.len(),
            source::Sha256Digest::of(&executable),
        );
        Self {
            source,
            jobs,
            cache,
            destination,
            config,
        }
    }
}

fn file<'a>(tree: &'a source::CapturedSource, path: &str) -> &'a [u8] {
    tree.file(&path.parse().expect("selected output path"))
        .expect("complete retained target bytes")
}

// Observe the actual borrowed application-to-target input, then call the real target.
// This checks custody only; it neither replaces conversion nor fabricates output.
struct RetainedSwagger<'a>(&'a [u8]);
impl DocumentationTarget for RetainedSwagger<'_> {
    fn preflight(
        &self,
        plan: gazette_core::documentation::Plan<'_>,
    ) -> Result<(), gazette_core::documentation::Error> {
        gazette_adapter_mintlify::Mintlify::new(&gazette_adapter_markdown::Comrak).preflight(plan)
    }

    fn assemble(
        &self,
        request: documentation::Request<'_>,
        limits: source::Limits,
    ) -> Result<source::CapturedSource, documentation::Error> {
        assert_eq!(request.projects.len(), 1);
        let [documentation::ProjectItem::Reference(reference)] =
            request.projects[0].navigation.as_slice()
        else {
            panic!("fixture module must retain its one configured reference")
        };
        let artifact = &reference.artifact;
        assert_eq!(artifact.format, documentation::Format::SwaggerJson);
        assert_eq!(
            artifact.bytes()?,
            self.0,
            "exact original Swagger retained, not replaced by converted OpenAPI"
        );
        gazette_adapter_mintlify::Mintlify::new(&gazette_adapter_markdown::Comrak)
            .assemble(request, limits)
    }
}

#[test]
#[ignore = "requires explicit GAZETTE_TEST_SWAGGER and GAZETTE_TEST_GO_ROOT provider qualification"]
fn installed_swagger_reaches_configured_mintlify_module_stage_and_retained_reassembly() {
    let fixture = Fixture::new();
    let inputs = build::acquire_documentation(
        &fixture.config,
        &"site:one".parse().expect("site"),
        Default::default(),
        source::PreparationLimits {
            sources: 1,
            entries: 8,
            bytes: 65536,
            per_source: Default::default(),
        },
    )
    .expect("configured exact project source without Theme or authored admission");
    let generated = build::generate_documentation(
        &inputs,
        fixture.jobs.path(),
        build::DocumentationGenerationOptions {
            input: Default::default(),
            output: Default::default(),
            execution: process::Options {
                allow_execute: true,
                timeout: Duration::from_secs(30),
                ..Default::default()
            },
            max_capture_bytes: 1 << 20,
            max_jobs: 1,
            directory_bytes: 16 << 10,
            entry_bytes: 4 << 10,
        },
    )
    .expect("actual installed go-swagger generation and native validation");
    assert_eq!(generated.jobs().len(), 1);
    let job = &generated.jobs()[0];
    assert!(
        job.evidence
            .version_text
            .as_deref()
            .expect("native version")
            .contains("v0.34.1")
    );
    let original = fs::read(job.output.join("swagger.json")).expect("native output read-back");
    let original_model: Value = serde_json::from_slice(&original).expect("native Swagger JSON");
    assert_eq!(original_model["swagger"], "2.0");
    let validation = job
        .evidence
        .validation
        .as_ref()
        .expect("separate native validation");
    assert_eq!(validation.tree, job.output_identity);
    assert_eq!(
        validation.invocation.exit.expect("native exit").code,
        Some(0)
    );
    let assembly = build::assemble_generated_documentation(&generated, Default::default())
        .expect("production SDK target assembly");
    assert_eq!(assembly.repository().name, "owner-selected-docs");
    assert_eq!(
        PathBuf::from(&assembly.repository().path),
        fixture.destination
    );
    assert!(
        !fixture.destination.exists(),
        "assembly does not create the brand destination"
    );
    assert_eq!(
        assembly.artifacts()[0].format,
        documentation::Format::SwaggerJson
    );
    assert_eq!(
        assembly.artifacts()[0].content,
        source::Content::File {
            bytes: original.len() as u64,
            sha256: source::Sha256Digest::of(&original),
        }
    );
    let converted: Value =
        serde_json::from_slice(file(assembly.output(), "artifacts/widgets/http.json"))
            .expect("validated converted OpenAPI");
    assert_eq!(converted["openapi"], "3.0.3");
    assert_eq!(
        converted["servers"],
        json!([{"url":"https://api.example.invalid/v1"}])
    );
    let operation = &converted["paths"]["/widgets/{id}"]["get"];
    assert_eq!(operation["operationId"], "getWidget");
    assert_eq!(operation["tags"], json!(["widgets"]));
    let parameters = operation["parameters"]
        .as_array()
        .expect("preserved native parameters");
    let id = parameters
        .iter()
        .find(|value| value["name"] == "id")
        .expect("path parameter");
    assert_eq!(id["in"], "path");
    assert_eq!(id["required"], true);
    assert_eq!(id["schema"]["type"], "string");
    let verbose = parameters
        .iter()
        .find(|value| value["name"] == "verbose")
        .expect("query parameter");
    assert_eq!(verbose["in"], "query");
    assert_eq!(verbose["schema"]["type"], "boolean");
    assert_eq!(
        operation["responses"]["200"]["$ref"],
        "#/components/responses/WidgetResponse"
    );
    assert_eq!(
        converted["components"]["responses"]["WidgetResponse"]["content"]["application/json"]["schema"]
            ["$ref"],
        "#/components/schemas/Widget"
    );
    assert_eq!(
        converted["components"]["schemas"]["Widget"]["properties"]["id"]["type"],
        "string"
    );
    assert_eq!(
        converted["components"]["schemas"]["Widget"]["required"],
        json!(["id"])
    );
    assert_eq!(
        converted["components"]["securitySchemes"]["apiKey"],
        json!({"type":"apiKey","name":"X-Fixture-Key","in":"header"})
    );
    assert_eq!(operation["security"], json!([{"apiKey":[]}]));
    let navigation: Value =
        serde_json::from_slice(file(assembly.output(), "docs.json")).expect("target navigation");
    assert_eq!(navigation["name"], "Configured Swagger publication");
    let groups = navigation["navigation"]["groups"]
        .as_array()
        .expect("project modules");
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0]["group"], "Widget API");
    assert_eq!(groups[0]["pages"][0]["group"], "HTTP reference");
    assert_eq!(
        groups[0]["pages"][0]["openapi"],
        json!({"source":"artifacts/widgets/http.json","directory":"widgets/http"})
    );

    let parent = tempfile::tempdir().expect("independent final stage parent");
    let stage = build::stage_documentation(&assembly, parent.path(), Default::default(), 16 << 10)
        .expect("fresh private target stage");
    let observed =
        source::inspect(stage.path(), None, Default::default()).expect("native staged read-back");
    assert_eq!(observed, *assembly.output().inventory());
    for entry in assembly.output().inventory().entries() {
        if matches!(entry.content, source::Content::File { .. }) {
            assert_eq!(
                fs::read(stage.path().join(entry.path.as_str())).expect("actual staged file"),
                file(assembly.output(), entry.path.as_str())
            );
        }
    }
    fixture
        .source
        .close()
        .expect("remove only disposable native source");
    fixture
        .jobs
        .close()
        .expect("remove only disposable generated job directories");
    fixture
        .cache
        .close()
        .expect("remove only disposable Go caches");
    let repeated = generated
        .assemble_documentation(&RetainedSwagger(&original), Default::default())
        .expect(
            "exact original survives source/job/cache removal and uses the same real converter",
        );
    assert_eq!(assembly.output().inventory(), repeated.output().inventory());
    assert_eq!(
        serde_json::to_value(assembly.artifacts()).expect("original custody"),
        serde_json::to_value(repeated.artifacts()).expect("retained custody")
    );
    for entry in assembly.output().inventory().entries() {
        if matches!(entry.content, source::Content::File { .. }) {
            assert_eq!(
                file(assembly.output(), entry.path.as_str()),
                file(repeated.output(), entry.path.as_str())
            );
        }
    }
}
