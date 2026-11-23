//! Explicit gomarkdoc v1.1.0 native-library qualification, not hosted Mintlify proof.
//! Supply absolute `GAZETTE_TEST_GOMARKDOC` and `GAZETTE_TEST_GO_ROOT`; missing tools fail.
//! Native flags/Markdown profile: <https://github.com/princjef/gomarkdoc/tree/v1.1.0>

mod support;

use gazette_adapter_documentation::NativeGenerator;
use gazette_adapter_fs::FileSystem;
use gazette_adapter_process::Runner;
use gazette_core::{
    documentation::{
        Artifact, Format, ProjectItem, ProjectModule, Reference, Request, Selection, Target,
        generator::{Argument, Phase, Recipe},
    },
    source::{CapturedSource, Limits, Sha256Digest},
};
use gazette_ports::{
    documentation::{DocumentationTarget as _, Generator as _},
    source::SourceCapture as _,
};
use serde_json::Value;
use std::{collections::BTreeMap, fs, path::PathBuf};
use support::{Job, pin};

fn arguments(check: bool) -> Vec<Argument> {
    let mut arguments: Vec<_> = [
        "--format",
        "github",
        "--repository.url",
        "https://github.com/example/reference",
        "--repository.default-branch",
        "1111111111111111111111111111111111111111",
        "--repository.path",
        "/",
        "--output",
    ]
    .into_iter()
    .map(|value| Argument::Literal {
        value: value.into(),
    })
    .collect();
    arguments.push(Argument::Output);
    if check {
        arguments.push(Argument::Literal {
            value: "--check".into(),
        });
    }
    // Native package selection is the exact retained working directory, never a remote package.
    arguments.push(Argument::InputRoot);
    arguments
}

fn job(source: &[u8]) -> (Job, tempfile::TempDir) {
    let program = PathBuf::from(
        std::env::var_os("GAZETTE_TEST_GOMARKDOC")
            .expect("explicit installed gomarkdoc v1.1.0 executable required"),
    );
    let go = PathBuf::from(
        std::env::var_os("GAZETTE_TEST_GO_ROOT").expect("explicit installed Go root required"),
    );
    assert!(
        program.is_absolute() && go.is_absolute(),
        "no PATH discovery"
    );
    assert!(
        go.join("bin/go").is_file(),
        "installed Go required; no toolchain download"
    );
    let cache = tempfile::tempdir().expect("isolated native tool state");
    let mut environment = BTreeMap::from([
        (
            "PATH".into(),
            go.join("bin").to_str().expect("Go bin path").into(),
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
    for (variable, directory) in [
        ("GOCACHE", "build"),
        ("GOMODCACHE", "modules"),
        ("GOPATH", "gopath"),
        ("TMPDIR", "tmp"),
    ] {
        let path = cache.path().join(directory);
        fs::create_dir(&path).expect("owned isolated tool directory");
        environment.insert(
            variable.into(),
            path.to_str().expect("tool directory").into(),
        );
    }
    let version_arguments = vec!["--version".into()];
    let version = std::process::Command::new(&program)
        .args(&version_arguments)
        .env_clear()
        .envs(&environment)
        .output()
        .expect("observe selected gomarkdoc version");
    assert!(
        version.status.success(),
        "selected gomarkdoc version failed"
    );
    let recipe = Recipe {
        executable: pin(&program),
        program: program.to_str().expect("exact executable").into(),
        arguments: arguments(false),
        environment,
        version_arguments,
        expected_version_sha256: Sha256Digest::of(&version.stdout),
        validation_arguments: Some(arguments(true)),
        output: Selection::File {
            path: "api.md".parse().expect("native Markdown output"),
        },
        format: Format::Markdown,
    };
    (
        Job::new(
            &[
                ("go.mod", include_bytes!("fixtures/gomarkdoc/go.mod")),
                ("library.go", source),
                (
                    "example_test.go",
                    include_bytes!("fixtures/gomarkdoc/example_test.go"),
                ),
            ],
            recipe,
        ),
        cache,
    )
}

fn assemble(artifact: Artifact<'_>) -> CapturedSource {
    let target = Target::Mintlify {
        theme: "mint".into(),
        primary_color: "#123abc".into(),
        light_color: None,
        dark_color: None,
        telemetry: None,
    };
    let projects = [ProjectModule {
        slug: "go",
        title: "Go reference",
        manual: None,
        navigation: vec![ProjectItem::Reference(Reference {
            pages: None,
            slug: "library",
            title: "Library",
            artifact,
        })],
    }];
    gazette_adapter_mintlify::Mintlify::new(&gazette_adapter_markdown::Comrak)
        .assemble(
            Request {
                name: "Native reference",
                public_base_url: None,
                target: &target,
                projects: &projects,
                usage: None,
            },
            Limits::default(),
        )
        .expect("actual retained Go library reaches Mintlify target")
}

#[test]
#[ignore = "requires explicit GAZETTE_TEST_GOMARKDOC and GAZETTE_TEST_GO_ROOT provider qualification"]
fn native_go_library_reaches_mintlify_with_exact_capture_and_retained_repeat() {
    let (job, _cache) = job(include_bytes!("fixtures/gomarkdoc/library.go"));
    let generated = NativeGenerator::new(&FileSystem, &Runner)
        .generate(job.request())
        .expect("real gomarkdoc generation and native --check");
    assert!(
        generated
            .evidence
            .version_text
            .as_deref()
            .expect("native version")
            .contains("1.1.0")
    );
    assert_eq!(generated.format, Format::Markdown);
    assert_eq!(generated.evidence.input, job.input.inventory().identity());
    let selected = generated.selection.path().expect("selected file");
    let bytes = generated
        .output
        .file(selected)
        .expect("native Markdown capture");
    let markdown = std::str::from_utf8(bytes).expect("native UTF-8");
    for required in [
        "import \"example.invalid/reference\"",
        "type Record struct",
        "Value string",
        "func New(value string) Record",
        "func (record Record) Label() string",
        "DefaultLabel",
        "original payload",
        "reference.New(reference.DefaultLabel)",
        "fmt.Println(record.Label())",
        "hello",
        "https://example.invalid/guide",
        "<#Record>",
        "<a name=\"Record\"></a>",
        "https://github.com/example/reference/blob/1111111111111111111111111111111111111111/library.go#L",
    ] {
        assert!(
            markdown.contains(required),
            "native library omission: {required}\n{markdown}"
        );
    }
    assert!(
        !markdown.contains("func hidden("),
        "unexported implementation stays private"
    );
    let readback = FileSystem
        .capture(job.output.path(), None, Limits::default())
        .expect("native output read-back");
    assert_eq!(readback.inventory(), generated.output.inventory());
    assert_eq!(readback.file(selected), Some(bytes));
    let validation = generated
        .evidence
        .validation
        .as_ref()
        .expect("native --check evidence");
    assert_eq!(validation.tree, generated.output.inventory().identity());
    assert_eq!(
        validation.invocation.exit.expect("validator exit").code,
        Some(0)
    );
    let original = FileSystem
        .capture(job.input.path(), None, Limits::default())
        .expect("preserved native inputs");
    assert_eq!(original.inventory(), job.input.inventory());
    let target = assemble(Artifact {
        tree: generated.output.as_input(),
        selection: &generated.selection,
        format: generated.format,
    });
    assert_eq!(
        target.file(&"go/library.md".parse().expect("target route")),
        Some(bytes),
        "signatures, examples, raw anchors and link destinations survive unchanged"
    );
    let navigation: Value = serde_json::from_slice(
        target
            .file(&"docs.json".parse().expect("configuration"))
            .expect("target navigation"),
    )
    .expect("JSON");
    assert_eq!(
        navigation["navigation"]["groups"][0]["pages"][0]["pages"],
        serde_json::json!(["go/library"])
    );
    drop(job);
    let repeated = assemble(Artifact {
        tree: generated.output.as_input(),
        selection: &generated.selection,
        format: generated.format,
    });
    assert_eq!(
        target.inventory(),
        repeated.inventory(),
        "target repeats without source or output directory reads"
    );
    assert_eq!(
        repeated.file(&"go/library.md".parse().unwrap()),
        Some(bytes)
    );
}

#[test]
#[ignore = "requires explicit GAZETTE_TEST_GOMARKDOC and GAZETTE_TEST_GO_ROOT provider qualification"]
fn malformed_go_library_refuses_without_a_partial_documentation_artifact() {
    let (job, _cache) = job(b"package reference\nfunc Broken(\n");
    let failure = NativeGenerator::new(&FileSystem, &Runner)
        .generate(job.request())
        .expect_err("invalid Go cannot become an admitted reference");
    assert!(matches!(failure.phase, Phase::Generation), "{failure}");
    assert!(failure.evidence.version.is_some());
    assert!(failure.evidence.validation.is_none());
    let input = FileSystem
        .capture(job.input.path(), None, Limits::default())
        .expect("preserved failed input");
    assert_eq!(input.inventory(), job.input.inventory());
}
