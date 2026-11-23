//! Actual Sphinx 8.2.3 → retained JSON directory → Mintlify packaging.
//! Requires `GAZETTE_TEST_SPHINX_PYTHON`: an explicit approved venv Python executable.
//! No installation, PATH fallback, skipped qualification or hosted-rendering claim.
//! Interpreter pinning does not independently qualify its installed package closure.

mod support;

use gazette_adapter_documentation::NativeGenerator;
use gazette_adapter_fs::FileSystem;
use gazette_adapter_process::Runner;
use gazette_core::{
    documentation::{
        Artifact, Format, ProjectItem, ProjectModule, Reference, Request, Selection, Target,
        generator::{Argument, Cause, Phase, Recipe},
    },
    process::Failure,
    source::{CapturedSource, Content, Limits, Sha256Digest},
};
use gazette_ports::{
    documentation::{DocumentationTarget as _, Generator as _},
    source::SourceCapture as _,
};
use serde_json::Value;
use std::{collections::BTreeMap, fs, path::PathBuf};
use support::{Job, pin};

const FILES: &[(&str, &[u8])] = &[
    ("conf.py", include_bytes!("fixtures/sphinx/conf.py")),
    ("model.py", include_bytes!("fixtures/sphinx/model.py")),
    ("api.py", include_bytes!("fixtures/sphinx/api.py")),
    ("index.rst", include_bytes!("fixtures/sphinx/index.rst")),
    ("model.rst", include_bytes!("fixtures/sphinx/model.rst")),
    ("api.rst", include_bytes!("fixtures/sphinx/api.rst")),
];

fn job(replacement: Option<(&str, &[u8])>) -> Job {
    let program = PathBuf::from(
        std::env::var_os("GAZETTE_TEST_SPHINX_PYTHON")
            .expect("explicit approved Sphinx venv Python required"),
    );
    assert!(program.is_absolute(), "no ambient provider discovery");
    let mut arguments: Vec<_> = [
        "-B", "-s", "-m", "sphinx", "-b", "json", "-W", "-n", "-E", "-N", ".",
    ]
    .into_iter()
    .map(|value| Argument::Literal {
        value: value.into(),
    })
    .collect();
    arguments.push(Argument::OutputRoot);
    // Native Sphinx's selected filenames also bind exact required source entries.
    for name in ["index.rst", "model.rst", "api.rst"] {
        arguments.push(Argument::Input {
            path: name.parse().expect("selected native document"),
        });
    }
    let files: Vec<_> = FILES
        .iter()
        .map(|&(name, bytes)| {
            (
                name,
                replacement
                    .filter(|(selected, _)| *selected == name)
                    .map_or(bytes, |(_, bytes)| bytes),
            )
        })
        .collect();
    let environment = BTreeMap::from([
        ("PYTHONNOUSERSITE".into(), "1".into()),
        ("PYTHONDONTWRITEBYTECODE".into(), "1".into()),
    ]);
    let version_arguments: Vec<_> = ["-B", "-s", "-c", "import sphinx; assert sphinx.__version__ == '8.2.3', sphinx.__version__; print('Sphinx ' + sphinx.__version__)"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    let version = std::process::Command::new(&program)
        .args(&version_arguments)
        .env_clear()
        .envs(&environment)
        .output()
        .expect("observe selected Sphinx version");
    assert!(version.status.success(), "selected Sphinx version failed");
    Job::new(
        &files,
        Recipe {
            executable: pin(&program),
            program: program.to_str().expect("exact UTF-8 executable").into(),
            arguments,
            environment,
            version_arguments,
            expected_version_sha256: Sha256Digest::of(&version.stdout),
            validation_arguments: None,
            output: Selection::Directory { path: None },
            format: Format::SphinxJson,
        },
    )
}

fn assemble(tree: &CapturedSource, selection: &Selection) -> CapturedSource {
    gazette_adapter_mintlify::Mintlify::new(&gazette_adapter_markdown::Comrak)
        .assemble(
            Request {
                name: "Python qualification",
                public_base_url: None,
                target: &Target::Mintlify {
                    theme: "mint".into(),
                    primary_color: "#123abc".into(),
                    light_color: None,
                    dark_color: None,
                    telemetry: None,
                },
                projects: &[ProjectModule {
                    slug: "fixture",
                    title: "Fixture project",
                    manual: None,
                    navigation: vec![ProjectItem::Reference(Reference {
                        pages: None,
                        slug: "python",
                        title: "Python library",
                        artifact: Artifact {
                            format: Format::SphinxJson,
                            tree: tree.as_input(),
                            selection,
                        },
                    })],
                }],
                usage: None,
            },
            Limits::default(),
        )
        .expect("actual native contexts accepted by Mintlify target")
}

fn context(tree: &CapturedSource, path: &str) -> Value {
    serde_json::from_slice(
        tree.file(&path.parse().expect("context path"))
            .expect("native context exists"),
    )
    .expect("actual native JSON")
}

#[test]
#[ignore = "requires explicit GAZETTE_TEST_SPHINX_PYTHON provider qualification"]
fn real_python_modules_signatures_links_and_companions_reach_retained_target() {
    let job = job(None);
    let native = NativeGenerator::new(&FileSystem, &Runner);
    let generated = native
        .generate(job.request())
        .expect("real pinned Sphinx generation");
    assert_eq!(
        generated.evidence.version_text.as_deref(),
        Some("Sphinx 8.2.3\n")
    );
    assert_eq!(
        generated
            .evidence
            .generation
            .as_ref()
            .expect("native generation")
            .exit
            .expect("native exit")
            .code,
        Some(0)
    );
    assert_eq!(generated.evidence.input, job.input.inventory().identity());
    let model = context(&generated.output, "model.fjson");
    let api = context(&generated.output, "api.fjson");
    let model_body = model["body"].as_str().expect("model HTML context");
    let api_body = api["body"].as_str().expect("API HTML context");
    for token in [
        "id=\"module-model\"",
        "id=\"model.Record\"",
        "id=\"model.Record.scaled\"",
        "One retained value.",
        "multiplied by the selected factor",
        "record.value",
        "42",
        "sample",
        "value",
        "label",
        "factor",
        "int",
        "str",
    ] {
        assert!(
            model_body.contains(token),
            "native model lost {token}: {model_body}"
        );
    }
    for token in [
        "id=\"module-api\"",
        "id=\"api.describe\"",
        "prefix",
        "record",
        "str",
        "without changing it",
        "42",
        "#model.Record",
    ] {
        assert!(
            api_body.contains(token),
            "native API lost {token}: {api_body}"
        );
    }
    assert!(
        ["href=\"model/", "href=\"../model/", "href=\"model.html"]
            .iter()
            .any(|link| api_body.contains(link)),
        "resolved cross-module native link: {api_body}"
    );
    assert!(
        model_body.contains("#api.describe"),
        "reverse native cross-module link"
    );
    let global = context(&generated.output, "globalcontext.json");
    assert_eq!(global["release"], "0.1.0");
    assert_eq!(global["project"], "Gazette Python fixture");
    let index = context(&generated.output, "index.fjson");
    assert!(index["body"].as_str().unwrap().contains("Record model"));
    assert!(
        index["body"]
            .as_str()
            .unwrap()
            .contains("Library operations")
    );
    assert_eq!(model["current_page_name"], "model");
    assert_eq!(api["current_page_name"], "api");
    assert!(
        generated
            .output
            .file(&"objects.inv".parse().unwrap())
            .is_some()
    );
    let search = context(&generated.output, "searchindex.json");
    for name in ["index", "model", "api"] {
        assert!(
            search["docnames"]
                .as_array()
                .unwrap()
                .contains(&Value::String(name.into()))
        );
    }
    let target = assemble(&generated.output, &generated.selection);
    for entry in target.inventory().entries() {
        if let Some(name) = entry
            .path
            .as_str()
            .strip_prefix("artifacts/fixture/python/")
        {
            if matches!(entry.content, Content::File { .. }) {
                assert_eq!(
                    target.file(&entry.path),
                    generated.output.file(&name.parse().unwrap()),
                    "exact native companion {name}"
                );
            }
            assert!(
                !name.starts_with(".doctrees/")
                    && name != "environment.pickle"
                    && name != "last_build"
            );
        }
    }
    for name in [
        "globalcontext.json",
        "searchindex.json",
        "objects.inv",
        "index.fjson",
        "model.fjson",
        "api.fjson",
        "_sources/model.rst.txt",
    ] {
        assert_eq!(
            target.file(&format!("artifacts/fixture/python/{name}").parse().unwrap()),
            Some(
                generated
                    .output
                    .file(&name.parse().unwrap())
                    .expect("required native artifact")
            )
        );
    }
    assert_eq!(
        context(&target, "docs.json")["navigation"]["groups"][0]["pages"][0]["sdk"],
        serde_json::json!({"format":"sphinx", "source":"artifacts/fixture/python", "directory":"fixture/python"})
    );
    let before = generated.output.inventory().identity();
    fs::rename(
        job.output.path().join("model.fjson"),
        job.output.path().join("moved-model.fjson"),
    )
    .unwrap();
    let repeated = assemble(&generated.output, &generated.selection);
    assert_eq!(target.inventory(), repeated.inventory());
    for entry in target.inventory().entries() {
        assert_eq!(target.file(&entry.path), repeated.file(&entry.path));
    }
    assert_eq!(generated.output.inventory().identity(), before);
    for &(name, bytes) in FILES {
        assert_eq!(fs::read(job.input.path().join(name)).unwrap(), bytes);
    }
    assert!(
        !job.input.path().join("__pycache__").exists(),
        "autodoc must not mutate retained source"
    );
}

#[test]
#[ignore = "requires explicit GAZETTE_TEST_SPHINX_PYTHON provider qualification"]
fn missing_required_source_and_denied_execution_refuse_before_native_start() {
    let native = NativeGenerator::new(&FileSystem, &Runner);
    let mut missing = job(None);
    missing.recipe.arguments.push(Argument::Input {
        path: "absent.rst".parse().unwrap(),
    });
    let error = native
        .generate(missing.request())
        .expect_err("missing required document");
    assert!(matches!(error.phase, Phase::Preflight));
    assert!(matches!(
        error.cause,
        Cause::Invalid("documentation_generator_input_missing")
    ));
    assert!(error.evidence.version.is_none() && error.evidence.generation.is_none());
    let denied = job(None);
    let mut request = denied.request();
    request.execution.allow_execute = false;
    let error = native
        .generate(request)
        .expect_err("no implicit tool permission");
    assert!(matches!(error.phase, Phase::Preflight));
    assert!(
        matches!(error.cause, Cause::Process(ref error) if error.failure == Failure::Permission)
    );
    assert!(error.evidence.version.is_none() && error.evidence.generation.is_none());
    for job in [&missing, &denied] {
        assert!(fs::read_dir(job.output.path()).unwrap().next().is_none());
    }
}

#[test]
#[ignore = "requires explicit GAZETTE_TEST_SPHINX_PYTHON provider qualification"]
fn real_import_and_cross_reference_failures_preserve_residue_not_successful_artifacts() {
    let native = NativeGenerator::new(&FileSystem, &Runner);
    for replacement in [
        ("api.py", b"raise RuntimeError('owned fixture import refusal')\n".as_slice()),
        ("api.rst", b"Library operations\n==================\n\n.. automodule:: api\n   :members:\n\nUnresolved :class:`missing.NoSuchRecord`.\n".as_slice()),
    ] {
        let job = job(Some(replacement));
        let error = native.generate(job.request()).expect_err("real Sphinx warning is a failed job");
        assert!(matches!(error.phase, Phase::Generation), "{error}");
        assert!(matches!(error.cause, Cause::Invalid("documentation_generator_exit")));
        assert_eq!(error.evidence.version_text.as_deref(), Some("Sphinx 8.2.3\n"));
        assert_ne!(error.evidence.generation.as_ref().unwrap().exit.unwrap().code, Some(0));
        assert_eq!(error.input, job.input.path());
        assert_eq!(error.output, job.output.path());
        assert_eq!(fs::read(job.input.path().join(replacement.0)).unwrap(), replacement.1);
        let retained = FileSystem.capture(job.output.path(), None, Limits::default()).expect("bounded native failure residue remains inspectable");
        assert!(!retained.inventory().entries().is_empty(), "Sphinx may write pages before -W reports failure; never substitute these for a successful generated artifact");
        assert!(error.evidence.validation.is_none());
    }
}
