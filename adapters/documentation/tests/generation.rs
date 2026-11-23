//! Actual subprocess, retained-tree and target seams; these fixtures do not qualify `TypeDoc`.

mod support;

use gazette_adapter_documentation::NativeGenerator;
use gazette_adapter_fs::FileSystem;
use gazette_adapter_process::Runner;
use gazette_core::documentation::{
    Format, Selection,
    generator::{Argument, Recipe},
};
use gazette_core::{
    documentation::{
        Artifact, ProjectItem, ProjectModule, Reference, Request, Target,
        generator::{Cause, Phase},
    },
    process::{Cleanup, Failure},
    source::Limits,
};
use gazette_ports::documentation::{DocumentationTarget as _, Generator as _};
use std::collections::BTreeMap;
use std::{fs, time::Duration};
use support::{Job, pin};

fn shell(script: &str, payload: &[u8]) -> Job {
    selected_shell(
        script,
        payload,
        Selection::File {
            path: "reference.json".parse().expect("fixture invariant"),
        },
    )
}

fn selected_shell(script: &str, payload: &[u8], output: Selection) -> Job {
    let program = fs::canonicalize("/bin/sh").expect("explicit fixture interpreter");
    let recipe = Recipe {
        program: program.to_str().expect("fixture interpreter path").into(),
        executable: pin(&program),
        arguments: vec![
            Argument::Input {
                path: "generator.sh".parse().expect("fixture invariant"),
            },
            Argument::Input {
                path: "payload.json".parse().expect("fixture invariant"),
            },
            Argument::Output,
            Argument::OutputRoot,
        ],
        environment: BTreeMap::new(),
        version_arguments: vec!["-c".into(), "printf 'owned-fixture-generator/1\\n'".into()],
        expected_version_sha256: gazette_core::source::Sha256Digest::of(
            b"owned-fixture-generator/1\n",
        ),
        validation_arguments: None,
        output,
        format: Format::TypeDocJson,
    };
    Job::new(
        &[
            ("generator.sh", script.as_bytes()),
            ("payload.json", payload),
        ],
        recipe,
    )
}

const COPY: &str = "/bin/cat \"$1\" > \"$2\"\n";
const ARTIFACT: &[u8] =
    br#"{"id":0,"name":"Fixture","kind":1,"variant":"project","schemaVersion":"2.0"}"#;

fn validation_job(script: &str) -> Job {
    let mut job = shell(COPY, ARTIFACT);
    job.recipe
        .environment
        .insert("GAZETTE_FIXTURE_CHECK".into(), "selected".into());
    job.recipe.validation_arguments = Some(vec![
        Argument::Literal { value: "-c".into() },
        Argument::Literal {
            value: script.into(),
        },
        Argument::Literal {
            value: "gazette-validation".into(),
        },
        Argument::Input {
            path: "payload.json".parse().expect("payload path"),
        },
        Argument::Output,
        Argument::OutputRoot,
    ]);
    job
}

fn generated_identity() -> gazette_core::source::TreeIdentity {
    use gazette_core::source::{Content, InventoryBuilder, Sha256Digest};
    let mut inventory = InventoryBuilder::new(Limits::default());
    inventory
        .insert(
            "reference.json".parse().expect("artifact path"),
            Content::File {
                bytes: ARTIFACT.len() as u64,
                sha256: Sha256Digest::of(ARTIFACT),
            },
        )
        .expect("expected complete generated inventory");
    inventory.finish().identity()
}

#[test]
fn validation_success_binds_same_executable_environment_paths_and_exact_generated_tree() {
    use gazette_core::{process::Command, source::Sha256Digest};
    let script = "test \"$PWD/payload.json\" = \"$1\" && test \"$3/reference.json\" = \"$2\" && /usr/bin/cmp \"$1\" \"$2\" && printf 'validated:%s' \"$GAZETTE_FIXTURE_CHECK\"";
    let job = validation_job(script);
    let generated = NativeGenerator::new(&FileSystem, &Runner)
        .generate(job.request())
        .expect("real selected validation");
    let validation = generated
        .evidence
        .validation
        .as_ref()
        .expect("native validation evidence");
    assert_eq!(validation.tree, generated_identity());
    assert_eq!(validation.tree, generated.output.inventory().identity());
    assert_eq!(
        generated
            .output
            .file(&"reference.json".parse().expect("artifact")),
        Some(ARTIFACT)
    );
    assert_eq!(
        validation.invocation.exit.expect("reaped validator").code,
        Some(0)
    );
    assert_eq!(
        validation.invocation.stdout_bytes,
        Some(b"validated:selected".len())
    );
    assert_eq!(
        validation.invocation.stdout_sha256,
        Some(Sha256Digest::of(b"validated:selected"))
    );
    let command = Command {
        program: job.recipe.program.clone(),
        executable: Some(job.recipe.executable),
        arguments: vec![
            "-c".into(),
            script.into(),
            "gazette-validation".into(),
            job.input
                .path()
                .join("payload.json")
                .to_str()
                .unwrap()
                .into(),
            job.output
                .path()
                .join("reference.json")
                .to_str()
                .unwrap()
                .into(),
            job.output.path().to_str().unwrap().into(),
        ],
        directory: job.input.path().to_str().unwrap().into(),
        environment: job.recipe.environment.clone(),
    };
    assert_eq!(
        validation.invocation.command_sha256,
        Sha256Digest::of(&serde_json::to_vec(&command).expect("exact native command"))
    );
    assert_eq!(
        generated
            .evidence
            .generation
            .as_ref()
            .expect("generation")
            .exit
            .expect("reaped generator")
            .code,
        Some(0)
    );
    assert_eq!(
        fs::read(job.input.path().join("payload.json")).expect("retained input"),
        ARTIFACT
    );
    let replay = NativeGenerator::new(&FileSystem, &Runner)
        .generate(job.request())
        .expect_err("no implicit rerun of successful native validation");
    assert_eq!(replay.code(), "documentation_generator_output_not_empty");
    assert!(replay.evidence.validation.is_none());
}

#[test]
fn validation_nonzero_preserves_generated_bytes_and_both_invocation_receipts() {
    use gazette_core::source::Sha256Digest;
    let job = validation_job(
        "/usr/bin/cmp \"$1\" \"$2\" || exit 8; printf 'invalid specification'; exit 9",
    );
    let error = NativeGenerator::new(&FileSystem, &Runner)
        .generate(job.request())
        .expect_err("native validator rejects");
    assert!(matches!(error.phase, Phase::Validation));
    assert_eq!(error.code(), "documentation_generator_exit");
    assert_eq!(error.input, job.input.path());
    assert_eq!(error.output, job.output.path());
    assert_eq!(
        error
            .evidence
            .generation
            .as_ref()
            .expect("generation survives")
            .exit
            .expect("generation exit")
            .code,
        Some(0)
    );
    let validation = error
        .evidence
        .validation
        .as_ref()
        .expect("failed validation survives");
    assert_eq!(validation.tree, generated_identity());
    assert_eq!(
        validation.invocation.exit.expect("validator exit").code,
        Some(9)
    );
    assert_eq!(
        validation.invocation.stdout_bytes,
        Some(b"invalid specification".len())
    );
    assert_eq!(
        validation.invocation.stdout_sha256,
        Some(Sha256Digest::of(b"invalid specification"))
    );
    assert_eq!(
        fs::read(error.output.join("reference.json")).expect("output custody"),
        ARTIFACT
    );
    assert!(error.evidence.version.is_some());
}

#[test]
fn mismatched_expected_version_digest_fails_before_generation_with_observed_evidence() {
    use gazette_core::source::Sha256Digest;

    let mut job = shell(COPY, ARTIFACT);
    let expected = Sha256Digest::of(b"different owner-approved version\n");
    job.recipe.expected_version_sha256 = expected;
    let error = NativeGenerator::new(&FileSystem, &Runner)
        .generate(job.request())
        .expect_err("observed provider version must match the configured identity");

    assert!(matches!(error.phase, Phase::Version));
    assert_eq!(error.code(), "documentation_generator_version_identity");
    assert_eq!(error.evidence.expected_version_sha256, expected);
    assert_eq!(
        error.evidence.version_text.as_deref(),
        Some("owned-fixture-generator/1\n")
    );
    let observed = error.evidence.version.expect("completed version evidence");
    assert_eq!(observed.exit.expect("version exit").code, Some(0));
    assert_eq!(
        observed.stdout_sha256,
        Some(Sha256Digest::of(b"owned-fixture-generator/1\n"))
    );
    assert_eq!(
        observed.stdout_bytes,
        Some(b"owned-fixture-generator/1\n".len())
    );
    assert!(error.evidence.generation.is_none());
    assert_eq!(
        fs::read_dir(job.output.path())
            .expect("generation output remains empty")
            .count(),
        0
    );
}

#[test]
fn validation_cannot_rewrite_generated_bytes_or_add_unvalidated_companions() {
    for (script, residue, bytes) in [
        (
            "printf changed > \"$2\"",
            "reference.json",
            b"changed".as_slice(),
        ),
        (
            "printf companion > \"$3/extra.txt\"",
            "extra.txt",
            b"companion".as_slice(),
        ),
    ] {
        let job = validation_job(script);
        let error = NativeGenerator::new(&FileSystem, &Runner)
            .generate(job.request())
            .expect_err("validator must not change its captured subject");
        assert!(matches!(error.phase, Phase::Validation));
        assert_eq!(error.code(), "documentation_generator_output_changed");
        assert_eq!(error.output, job.output.path());
        let validation = error
            .evidence
            .validation
            .as_ref()
            .expect("completed invocation retained");
        assert_eq!(
            validation.tree,
            generated_identity(),
            "bind original captured bytes, not validator's replacement"
        );
        assert_eq!(
            validation.invocation.exit.expect("reaped validator").code,
            Some(0)
        );
        assert_eq!(
            error
                .evidence
                .generation
                .as_ref()
                .expect("generation")
                .exit
                .expect("reaped generator")
                .code,
            Some(0)
        );
        assert_eq!(
            fs::read(error.output.join(residue)).expect("changed output retained, not restored"),
            bytes
        );
        assert_eq!(
            fs::read(error.input.join("payload.json")).expect("input unaffected"),
            ARTIFACT
        );
    }
}

#[test]
fn validation_cannot_modify_retained_generator_input() {
    let job = validation_job("printf changed-input > \"$1\"");
    let error = NativeGenerator::new(&FileSystem, &Runner)
        .generate(job.request())
        .expect_err("validator changed original input");
    assert!(matches!(error.phase, Phase::Validation));
    assert_eq!(error.code(), "documentation_generator_input_changed");
    let validation = error
        .evidence
        .validation
        .as_ref()
        .expect("exact original validation subject");
    assert_eq!(validation.tree, generated_identity());
    assert_eq!(
        validation.invocation.exit.expect("reaped validator").code,
        Some(0)
    );
    assert_eq!(
        fs::read(error.input.join("payload.json")).expect("mutated input preserved"),
        b"changed-input"
    );
    assert_eq!(
        fs::read(error.output.join("reference.json")).expect("original output custody"),
        ARTIFACT
    );
}

#[test]
fn validation_malformed_arguments_refuse_before_any_native_child() {
    for arguments in [
        vec![],
        vec![
            Argument::Literal { value: "-c".into() },
            Argument::Literal {
                value: "printf forbidden > marker".into(),
            },
        ],
        vec![
            Argument::Literal {
                value: "invalid\0argument".into(),
            },
            Argument::Output,
        ],
    ] {
        let mut job = validation_job("exit 0");
        job.recipe.validation_arguments = Some(arguments);
        let error = NativeGenerator::new(&FileSystem, &Runner)
            .generate(job.request())
            .expect_err("invalid selected validation command");
        assert!(matches!(error.phase, Phase::Preflight));
        assert!(
            matches!(&error.cause,Cause::Process(cause) if cause.failure==Failure::Command && cause.cleanup==Cleanup::NotStarted)
        );
        assert!(error.evidence.version.is_none());
        assert!(error.evidence.generation.is_none());
        assert!(error.evidence.validation.is_none());
        assert_eq!(
            fs::read_dir(job.output.path())
                .expect("original empty output")
                .count(),
            0
        );
        assert!(!job.input.path().join("marker").exists());
    }
}

#[test]
fn native_generation_retains_bytes_and_existing_target_assembles_two_modules_without_reexecution() {
    let first = shell(COPY, ARTIFACT);
    let second = shell(COPY, ARTIFACT);
    let native = NativeGenerator::new(&FileSystem, &Runner);
    let generated = native
        .generate(first.request())
        .expect("first actual execution");
    let other = native
        .generate(second.request())
        .expect("second actual execution");
    assert_eq!(generated.selection, first.recipe.output);
    assert_eq!(
        generated
            .output
            .file(generated.selection.path().expect("selected file path")),
        Some(ARTIFACT)
    );
    assert_eq!(
        generated.evidence.version_text.as_deref(),
        Some("owned-fixture-generator/1\n")
    );
    assert_eq!(
        generated
            .evidence
            .generation
            .as_ref()
            .expect("fixture invariant")
            .exit
            .expect("fixture invariant")
            .code,
        Some(0)
    );
    assert_eq!(generated.evidence.input, first.input.inventory().identity());
    let projects =
        [(&generated, "first"), (&other, "second")].map(|(artifact, slug)| ProjectModule {
            slug,
            title: slug,
            manual: None,
            navigation: vec![ProjectItem::Reference(Reference {
                pages: None,
                slug: "library",
                title: "Library",
                artifact: Artifact {
                    format: artifact.format,
                    tree: artifact.output.as_input(),
                    selection: &artifact.selection,
                },
            })],
        });
    let target = Target::Mintlify {
        theme: "mint".into(),
        primary_color: "#123abc".into(),
        light_color: None,
        dark_color: None,
        telemetry: None,
    };
    let request = || Request {
        name: "Publication",
        public_base_url: None,
        target: &target,
        projects: &projects,
        usage: None,
    };
    let assembly = gazette_adapter_mintlify::Mintlify::new(&gazette_adapter_markdown::Comrak)
        .assemble(request(), Limits::default())
        .expect("existing target");
    assert_eq!(
        assembly.file(
            &"artifacts/first/library.json"
                .parse()
                .expect("fixture invariant")
        ),
        Some(ARTIFACT)
    );
    assert_eq!(
        assembly.file(
            &"artifacts/second/library.json"
                .parse()
                .expect("fixture invariant")
        ),
        Some(ARTIFACT)
    );
    assert_eq!(
        assembly.inventory(),
        gazette_adapter_mintlify::Mintlify::new(&gazette_adapter_markdown::Comrak)
            .assemble(request(), Limits::default())
            .expect("fixture invariant")
            .inventory()
    );
    let refused = native
        .generate(first.request())
        .expect_err("output is not silently reused");
    assert_eq!(refused.code(), "documentation_generator_output_not_empty");
    assert!(refused.evidence.version.is_none());
}

#[test]
fn denial_changed_input_and_existing_output_refuse_before_execution() {
    let native = NativeGenerator::new(&FileSystem, &Runner);
    let job = shell(COPY, ARTIFACT);
    let mut denied = job.request();
    denied.execution.allow_execute = false;
    let error = native.generate(denied).expect_err("permission");
    assert!(
        matches!(error.cause, Cause::Process(ref error) if error.failure == Failure::Permission && error.cleanup == Cleanup::NotStarted)
    );
    assert!(error.evidence.version.is_none());
    fs::write(job.input.path().join("payload.json"), b"changed").expect("fixture invariant");
    let changed = native
        .generate(job.request())
        .expect_err("retained identity");
    assert_eq!(changed.code(), "documentation_generator_input_changed");
    assert!(changed.evidence.version.is_none());
    assert_eq!(
        fs::read_dir(job.output.path())
            .expect("fixture invariant")
            .count(),
        0
    );
    let occupied = shell(COPY, ARTIFACT);
    fs::write(occupied.output.path().join("foreign"), b"keep").expect("fixture invariant");
    let error = native
        .generate(occupied.request())
        .expect_err("no adoption");
    assert_eq!(error.code(), "documentation_generator_output_not_empty");
    assert_eq!(
        fs::read(occupied.output.path().join("foreign")).expect("fixture invariant"),
        b"keep"
    );
}

#[test]
fn configured_markdown_output_uses_existing_native_capture_and_target_without_rerunning() {
    let bytes = b"# Generated library\n\n```go\ntype Client struct {}\n```\n\n[Source](https://example.test/exact/source)\n";
    let mut job = shell(COPY, bytes);
    job.recipe.format = Format::Markdown;
    let generated = NativeGenerator::new(&FileSystem, &Runner)
        .generate(job.request())
        .expect("actual configured subprocess and retained capture");
    let modules = [ProjectModule {
        slug: "client",
        title: "Client",
        manual: None,
        navigation: vec![ProjectItem::Reference(Reference {
            pages: None,
            slug: "library",
            title: "Library API",
            artifact: Artifact {
                format: generated.format,
                tree: generated.output.as_input(),
                selection: &generated.selection,
            },
        })],
    }];
    let target = Target::Mintlify {
        theme: "mint".into(),
        primary_color: "#123abc".into(),
        light_color: None,
        dark_color: None,
        telemetry: None,
    };
    let request = || Request {
        name: "Reference",
        public_base_url: None,
        target: &target,
        projects: &modules,
        usage: None,
    };
    let adapter = gazette_adapter_mintlify::Mintlify::new(&gazette_adapter_markdown::Comrak);
    let first = adapter
        .assemble(request(), Limits::default())
        .expect("Markdown consumer connected");
    let second = adapter
        .assemble(request(), Limits::default())
        .expect("retained reassembly");
    assert_eq!(first.inventory(), second.inventory());
    assert_eq!(
        generated
            .output
            .file(generated.selection.path().expect("selected file path")),
        Some(bytes.as_slice())
    );
    assert_eq!(
        first.file(&"client/library.md".parse().expect("page path")),
        Some(
            b"---\ntitle: \"Generated library\"\n---\n\n<a id=\"generated&#45;library\"></a>\n\n```go\ntype Client struct {}\n```\n\n[Source](https://example.test/exact/source)\n"
                .as_slice()
        )
    );
    assert!(
        generated
            .evidence
            .generation
            .expect("one real invocation")
            .exit
            .is_some()
    );
    // The shell copies an owned artifact: this proves runner composition, not Go tooling.
}

#[test]
fn nonzero_timeout_and_capture_overflow_preserve_exact_job_and_completed_version() {
    let native = NativeGenerator::new(&FileSystem, &Runner);
    let partial = shell("printf partial > \"$2\"; exit 7", ARTIFACT);
    let error = native.generate(partial.request()).expect_err("nonzero");
    assert_eq!(error.code(), "documentation_generator_exit");
    assert_eq!(error.output, partial.output.path());
    assert_eq!(
        error
            .evidence
            .generation
            .as_ref()
            .expect("fixture invariant")
            .exit
            .expect("fixture invariant")
            .code,
        Some(7)
    );
    assert!(error.evidence.version_text.is_some());
    assert_eq!(
        fs::read(error.output.join("reference.json")).expect("fixture invariant"),
        b"partial"
    );
    let timed = shell("exec /bin/sleep 10", ARTIFACT);
    let mut request = timed.request();
    request.execution.timeout = Duration::from_millis(100);
    let error = native.generate(request).expect_err("bounded generation");
    assert!(matches!(error.phase, Phase::Generation));
    assert!(
        matches!(error.cause, Cause::Process(ref error) if error.failure == Failure::Timeout && error.cleanup == Cleanup::Reaped)
    );
    assert!(error.evidence.version_text.is_some());
    assert!(
        error
            .evidence
            .generation
            .as_ref()
            .expect("fixture invariant")
            .exit
            .is_none()
    );
    let overflow = shell(COPY, ARTIFACT);
    let mut request = overflow.request();
    request.max_capture_bytes = 1;
    let error = native
        .generate(request)
        .expect_err("version stdout overflow");
    assert!(matches!(error.phase, Phase::Version));
    assert!(error.evidence.generation.is_none());
    assert_eq!(
        fs::read_dir(overflow.output.path())
            .expect("fixture invariant")
            .count(),
        0
    );
}

#[test]
fn mutation_missing_output_links_and_tree_limits_never_return_a_partial_artifact() {
    let native = NativeGenerator::new(&FileSystem, &Runner);
    let changed = shell("printf x >> \"$1\"; /bin/cat \"$1\" > \"$2\"", ARTIFACT);
    let error = native
        .generate(changed.request())
        .expect_err("source mutation");
    assert_eq!(error.code(), "documentation_generator_input_changed");
    assert!(matches!(error.phase, Phase::Capture));
    assert!(error.evidence.generation.is_some());
    assert!(error.output.join("reference.json").is_file());
    let missing = shell("exit 0", ARTIFACT);
    assert_eq!(
        native
            .generate(missing.request())
            .expect_err("required output")
            .code(),
        "documentation_generator_output_selection"
    );
    let linked = shell("/bin/ln -s \"$1\" \"$2\"", ARTIFACT);
    assert!(matches!(
        native
            .generate(linked.request())
            .expect_err("no link capture")
            .cause,
        Cause::Source(_)
    ));
    let bounded = shell(COPY, ARTIFACT);
    let mut request = bounded.request();
    request.output_limits.file_bytes = 1;
    assert!(matches!(
        native
            .generate(request)
            .expect_err("whole file required")
            .cause,
        Cause::Source(_)
    ));
}

#[test]
fn directory_output_arguments_select_exact_root_or_subtree_and_retain_all_companions() {
    use gazette_core::source::Content;

    let native = NativeGenerator::new(&FileSystem, &Runner);
    let script = "/bin/mkdir -p \"$2\"\n/bin/cat \"$1\" > \"$2/reference.json\"\nprintf '%s' \"$2\" > \"$3/selected-path.txt\"\nprintf '%s' \"$3\" > \"$3/root-path.txt\"\nprintf companion > \"$3/companion.txt\"\n";
    for path in [
        None,
        Some("nested/reference".parse().expect("nested subtree")),
    ] {
        let selection = Selection::Directory { path };
        let job = selected_shell(script, ARTIFACT, selection.clone());
        let generated = native
            .generate(job.request())
            .expect("actual directory output");
        assert_eq!(generated.selection, selection);
        assert_eq!(
            generated
                .selection
                .content(&generated.output.as_input())
                .expect("selected captured directory"),
            Content::Directory
        );
        let expected = selection.path().map_or_else(
            || job.output.path().to_path_buf(),
            |path| job.output.path().join(path.as_str()),
        );
        for (file, value) in [
            ("selected-path.txt", expected.as_path()),
            ("root-path.txt", job.output.path()),
        ] {
            assert_eq!(
                generated
                    .output
                    .file(&file.parse().expect("captured argument path")),
                Some(value.to_str().expect("native path UTF-8").as_bytes()),
                "arguments are not inferred from an index marker"
            );
        }
        let reference = selection.path().map_or_else(
            || "reference.json".to_owned(),
            |path| format!("{}/reference.json", path.as_str()),
        );
        assert_eq!(
            generated
                .output
                .file(&reference.parse().expect("selected subtree payload")),
            Some(ARTIFACT)
        );
        assert_eq!(
            generated
                .output
                .file(&"companion.txt".parse().expect("companion")),
            Some(b"companion".as_slice())
        );
        assert_eq!(
            fs::read(job.input.path().join("payload.json")).expect("retained source"),
            ARTIFACT
        );
        let artifact = Artifact {
            format: generated.format,
            tree: generated.output.as_input(),
            selection: &generated.selection,
        };
        assert_eq!(
            artifact
                .bytes()
                .expect_err("directory never guesses its reference file")
                .code(),
            "documentation_target_input"
        );
    }
}

#[test]
fn empty_directory_is_explicit_output_but_absent_or_wrong_kind_selection_fails_capture() {
    use gazette_core::source::Content;

    let native = NativeGenerator::new(&FileSystem, &Runner);
    for path in [None, Some("empty".parse().expect("empty directory"))] {
        let job = selected_shell(
            "/bin/mkdir -p \"$2\"",
            ARTIFACT,
            Selection::Directory { path },
        );
        let generated = native
            .generate(job.request())
            .expect("empty is not missing");
        assert_eq!(generated.selection, job.recipe.output);
        assert_eq!(
            generated
                .selection
                .content(&generated.output.as_input())
                .expect("exact directory"),
            Content::Directory
        );
        assert!(
            generated
                .output
                .inventory()
                .entries()
                .iter()
                .all(|entry| matches!(entry.content, Content::Directory))
        );
        assert_eq!(generated.output.inventory().total_bytes(), 0);
    }
    for (script, selection) in [
        (
            "printf retained > \"$3/companion.txt\"",
            Selection::Directory {
                path: Some("absent".parse().expect("absent directory")),
            },
        ),
        (
            "/bin/cat \"$1\" > \"$2\"",
            Selection::Directory {
                path: Some("file.json".parse().expect("file instead of directory")),
            },
        ),
        (
            "/bin/mkdir \"$2\"",
            Selection::File {
                path: "directory".parse().expect("directory instead of file"),
            },
        ),
    ] {
        let job = selected_shell(script, ARTIFACT, selection);
        let error = native
            .generate(job.request())
            .expect_err("no output-kind substitution");
        assert_eq!(error.code(), "documentation_generator_output_selection");
        assert!(matches!(error.phase, Phase::Capture));
        assert_eq!(error.output, job.output.path());
        assert_eq!(
            error
                .evidence
                .generation
                .as_ref()
                .expect("native generation completed")
                .exit
                .expect("reaped child")
                .code,
            Some(0)
        );
        assert!(
            fs::read_dir(&error.output)
                .expect("residue retained")
                .next()
                .is_some()
        );
    }
}

#[test]
fn directory_selection_does_not_limit_capture_to_its_subtree_or_exempt_companion_bounds() {
    let native = NativeGenerator::new(&FileSystem, &Runner);
    let job = selected_shell(
        "/bin/mkdir \"$2\"\n/bin/cat \"$1\" > \"$2/reference.json\"\n/bin/cat \"$1\" > \"$3/companion.json\"",
        ARTIFACT,
        Selection::Directory {
            path: Some("selected".parse().expect("selected subtree")),
        },
    );
    let mut request = job.request();
    request.output_limits.total_bytes = ARTIFACT.len() as u64;
    let error = native
        .generate(request)
        .expect_err("complete output bounds include unselected companions");
    assert!(matches!(error.phase, Phase::Capture));
    assert!(matches!(error.cause, Cause::Source(_)));
    assert_eq!(
        fs::read(error.output.join("selected/reference.json")).expect("selected residue"),
        ARTIFACT
    );
    assert_eq!(
        fs::read(error.output.join("companion.json")).expect("companion residue"),
        ARTIFACT
    );
}
