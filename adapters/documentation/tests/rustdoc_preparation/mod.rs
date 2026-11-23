//! Cargo's exact dependency artifacts pass through the existing native job lifecycle.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use gazette_adapter_documentation::NativeGenerator;
use gazette_adapter_fs::FileSystem;
use gazette_adapter_process::Runner;
use gazette_core::{
    documentation::{
        Format, Selection,
        generator::{Argument, Recipe},
    },
    source::Sha256Digest,
};
use gazette_ports::documentation::Generator;
use serde_json::{Value, json};

use crate::support::{Job, pin};

const SOURCE: &str = "//! A selected library.\n//!\n//! ```\n//! assert_eq!(reference_fixture::answer(), 42);\n//! ```\n/// Re-exported dependency types keep their original declarations.\npub use bridge::{Old, New};\n/// The answer.\npub const fn answer() -> usize { 42 }\n";

fn executable(name: &str) -> PathBuf {
    let path = PathBuf::from(std::env::var_os(name).expect("explicit qualification executable"));
    assert!(path.is_absolute());
    path.canonicalize().unwrap()
}

fn job(source: &str) -> Job {
    let python = executable("GAZETTE_TEST_PYTHON");
    let cargo = executable("GAZETTE_TEST_RUSTDOC_CARGO");
    let rustc = executable("GAZETTE_TEST_RUSTDOC_RUSTC");
    let rustdoc = executable("GAZETTE_TEST_RUSTDOC_NIGHTLY");
    let provider = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/documentation-reference/rustdoc.py")
        .canonicalize()
        .unwrap();
    let config = serde_json::to_vec(&json!({
        "manifest":"Cargo.toml",
        "dependency_roots": {"same@1.0.0":"https://docs.rs/same/1.0.0/", "same@2.0.0":"https://docs.rs/same/2.0.0/"}
    })).unwrap();
    let literal = |value: &str| Argument::Literal {
        value: value.into(),
    };
    let script = provider.to_str().unwrap();
    let recipe = Recipe {
        executable: pin(&python),
        program: python.to_str().unwrap().into(),
        arguments: vec![
            literal("-B"),
            Argument::Path {
                path: script.into(),
            },
            literal("--cargo"),
            Argument::Path {
                path: cargo.to_str().unwrap().into(),
            },
            literal("--rustc"),
            Argument::Path {
                path: rustc.to_str().unwrap().into(),
            },
            literal("--rustdoc"),
            Argument::Path {
                path: rustdoc.to_str().unwrap().into(),
            },
            literal("--input"),
            Argument::InputRoot,
            literal("--output"),
            Argument::OutputRoot,
            literal("--config"),
            Argument::Input {
                path: "rustdoc.json".parse().unwrap(),
            },
        ],
        environment: BTreeMap::from([("PATH".into(), "/usr/bin:/bin".into())]),
        version_arguments: vec!["-B".into(), script.into(), "--version".into()],
        expected_version_sha256: Sha256Digest::of(b"gazette-rustdoc-provider/2\n"),
        validation_arguments: None,
        output: Selection::Directory {
            path: Some("reference/reference_fixture".parse().unwrap()),
        },
        format: Format::RustdocHtml,
    };
    Job::new(&[
        ("rustdoc.json", &config),
        ("Cargo.toml", b"[package]\nname='reference-fixture'\nversion='0.1.0'\nedition='2024'\n[dependencies]\nbridge={path='bridge'}\n[workspace]\nmembers=['bridge']\nexclude=['old','new']\n"),
        ("Cargo.lock", b"version = 4\n[[package]]\nname = \"bridge\"\nversion = \"0.1.0\"\ndependencies = [\"same 1.0.0\", \"same 2.0.0\"]\n[[package]]\nname = \"reference-fixture\"\nversion = \"0.1.0\"\ndependencies = [\"bridge\"]\n[[package]]\nname = \"same\"\nversion = \"1.0.0\"\n[[package]]\nname = \"same\"\nversion = \"2.0.0\"\n"),
        ("src/lib.rs", source.as_bytes()),
        ("bridge/Cargo.toml", b"[package]\nname='bridge'\nversion='0.1.0'\nedition='2024'\n[dependencies]\nold_alias={package='same',path='../old'}\nnew_alias={package='same',path='../new'}\n"),
        ("bridge/src/lib.rs", b"pub use old_alias::Old;\npub use new_alias::New;\n"),
        ("old/Cargo.toml", b"[package]\nname='same'\nversion='1.0.0'\nedition='2024'\n"),
        ("old/src/lib.rs", b"/// An older value.\npub struct Old;\n/// An older trait.\npub trait First {}\nimpl First for Old {}\n"),
        ("new/Cargo.toml", b"[package]\nname='same'\nversion='2.0.0'\nedition='2024'\n"),
        ("new/src/lib.rs", b"/// A newer value.\npub struct New;\n/// A newer trait.\npub trait Second {}\nimpl Second for New {}\n"),
    ], recipe)
}

#[test]
#[ignore = "requires explicit Python and nightly Cargo/rustc/rustdoc qualification paths"]
fn configured_rustdoc_prepares_renamed_and_multiple_version_dependencies() {
    let job = job(SOURCE);
    let mut request = job.request();
    request.execution.timeout = Duration::from_secs(120);
    request.output_limits.total_bytes = 64 << 20;
    let generated = NativeGenerator::new(&FileSystem, &Runner)
        .generate(request)
        .unwrap();
    assert_eq!(
        fs::read(job.input.path().join("src/lib.rs")).unwrap(),
        SOURCE.as_bytes()
    );
    let get = |name: &str| generated.output.file(&name.parse().unwrap()).unwrap();
    let evidence: Value = serde_json::from_slice(get("preparation.json")).unwrap();
    assert_eq!(evidence["status"], "generated");
    let dependencies = evidence["preparation"]["dependencies"].as_array().unwrap();
    assert_eq!(dependencies.len(), 2);
    let mut versions = dependencies
        .iter()
        .map(|item| item["version"].as_str().unwrap())
        .collect::<Vec<_>>();
    versions.sort_unstable();
    assert_eq!(versions, ["1.0.0", "2.0.0"]);
    assert_ne!(dependencies[0]["alias"], dependencies[1]["alias"]);
    let prepared = get("prepared/src/lib.rs");
    assert!(prepared.starts_with(SOURCE.as_bytes()));
    let index = String::from_utf8_lossy(get("reference/reference_fixture/index.html"));
    for name in ["struct.Old.html", "struct.New.html", "fn.answer.html"] {
        assert!(index.contains(name), "{name}");
    }
    assert!(!index.contains("gazette_doc_"));
    assert!(
        String::from_utf8_lossy(get("reference/reference_fixture/struct.Old.html"))
            .contains("https://docs.rs/same/1.0.0/same/trait.First.html")
    );
    assert!(
        String::from_utf8_lossy(get("reference/reference_fixture/struct.New.html"))
            .contains("https://docs.rs/same/2.0.0/same/trait.Second.html")
    );
    assert!(index.contains("assert_eq!"));
    assert!(!job.output.path().join(".cargo-work").exists());
    assert!(job.output.path().join("prepared/Cargo.lock").is_file());
}

#[test]
#[ignore = "requires explicit Python and nightly Cargo/rustc/rustdoc qualification paths"]
fn unresolved_comment_links_fail_without_changing_original_source() {
    let source = format!("{SOURCE}\n/// See [`DefinitelyMissing`].\npub fn broken() {{}}\n");
    let job = job(&source);
    let mut request = job.request();
    request.execution.timeout = Duration::from_secs(120);
    let failure = NativeGenerator::new(&FileSystem, &Runner)
        .generate(request)
        .unwrap_err();
    assert_eq!(failure.code(), "documentation_generator_exit");
    assert_eq!(
        fs::read_to_string(job.input.path().join("src/lib.rs")).unwrap(),
        source
    );
    let evidence: Value =
        serde_json::from_slice(&fs::read(job.output.path().join("preparation.json")).unwrap())
            .unwrap();
    assert_eq!(evidence["status"], "incomplete");
    assert!(!job.output.path().join("reference").exists());
}
