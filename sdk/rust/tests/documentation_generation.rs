//! Actual configured subprocess/capture/target wiring. The owned shell fixture transports
//! TypeDoc-shaped bytes; it does not qualify `TypeDoc`, Rustdoc or a hosted publication.

#[path = "documentation_generation/manual_capture.rs"]
mod manual_capture;

use gazette_core::documentation;
use gazette_ports::documentation::{DocumentationTarget, Generator};
use gazette_sdk::{build, process, source};
use git2::{Repository, Signature};
use serde_json::Value;
use std::{fs, path::PathBuf, time::Duration};
use tempfile::TempDir;

const ARTIFACT: &[u8] =
    br#"{"id":0,"name":"Owned fixture","kind":1,"variant":"project","schemaVersion":"2.0"}"#;
const COMPANION: &[u8] = b"retained imported companion";
const COPY: &[u8] = b"/bin/cat \"$1\" > \"$2\"\nprintf companion > \"${2}.companion\"\n";

struct Fixture {
    root: TempDir,
    jobs: TempDir,
    destination: PathBuf,
    config: String,
}

impl Fixture {
    fn new(script: &[u8], generated: usize, format: &str) -> Self {
        let root = tempfile::tempdir().expect("disposable exact-pin source");
        let jobs = tempfile::tempdir().expect("separate private jobs parent");
        let repository = Repository::init(root.path()).expect("native Git source");
        let source_root = root.path().join("inputs");
        fs::create_dir(&source_root).expect("source subtree");
        let mut subtree = repository.treebuilder(None).expect("source tree");
        for (name, bytes) in [
            ("generator.sh", script),
            ("payload.json", ARTIFACT),
            ("payload.json.companion", COMPANION),
        ] {
            fs::write(source_root.join(name), bytes).expect("source bytes");
            subtree
                .insert(name, repository.blob(bytes).expect("source blob"), 0o100644)
                .expect("source tree entry");
        }
        let mut tree = repository.treebuilder(None).expect("repository tree");
        tree.insert(
            "inputs",
            subtree.write().expect("source tree identity"),
            0o040000,
        )
        .expect("subtree entry");
        let tree = repository
            .find_tree(tree.write().expect("root identity"))
            .expect("native tree");
        let signature =
            Signature::new("Fixture", "fixture@example.invalid", &git2::Time::new(0, 0))
                .expect("fixed local signature");
        let commit = repository
            .commit(Some("HEAD"), &signature, &signature, "fixture", &tree, &[])
            .expect("exact captured revision");
        let destination = jobs.path().join("brand-selected-domain-doc");
        let program = fs::canonicalize("/bin/sh").expect("explicit fixture interpreter");
        let program = program.to_str().expect("fixture interpreter path");
        let executable = fs::read(program).expect("fixture interpreter bytes");
        let mut config = format!(
            "[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n[[sources]]\nid='source:one'\nlocation={{kind='repository',root={:?},commit='{commit}',subdirectory='inputs'}}\n",
            root.path().to_str().expect("fixture path")
        );
        for index in 0..=generated {
            config.push_str(&format!("[[projects]]\nid='project:p{index}'\nlifecycle='active'\nvisibility='public'\navailability='available'\n"));
        }
        config.push_str(&format!("[[brands]]\nid='brand:one'\nname='Configured publication'\n[[brands.publications]]\nsite='site:one'\n[brands.publications.documentation_repository]\nname='owner-selected-docs'\npath={:?}\n[brands.publications.documentation_repository.build]\ntarget={{kind='mintlify',theme='mint',primary_color='#123abc'}}\n", destination.to_str().expect("configured destination")));
        for index in 0..=generated {
            config.push_str(&format!("[[brands.publications.documentation_repository.build.modules]]\nproject='project:p{index}'\nslug='module-{index}'\ntitle='Module {index}'\n[[brands.publications.documentation_repository.build.modules.navigation]]\nkind='reference'\nslug='library'\ntitle='Library'\n"));
            if index == 0 {
                config.push_str("input={kind='artifact',source='source:one',selection={kind='file',path='payload.json'},format='typedoc-json'}\n");
            } else {
                config.push_str(&format!("input={{kind='generated',source='source:one',recipe={{program={program:?},executable={{bytes={},sha256='{}'}},arguments=[{{kind='input',path='generator.sh'}},{{kind='input',path='payload.json'}},{{kind='output'}},{{kind='literal',value='{index}'}}],version_arguments=['-c',\"printf 'owned-transport/1\\n'\"],expected_version_sha256='{}',output={{kind='file',path='reference.json'}},format='{format}'}}}}\n", executable.len(), source::Sha256Digest::of(&executable), source::Sha256Digest::of(b"owned-transport/1\n")));
            }
        }
        Self {
            root,
            jobs,
            destination,
            config,
        }
    }

    fn acquire(&self) -> build::AcquiredInputs {
        build::acquire_documentation(
            &self.config,
            &"site:one".parse().expect("selected site"),
            Default::default(),
            source::PreparationLimits {
                sources: 1,
                entries: 8,
                bytes: 65536,
                per_source: Default::default(),
            },
        )
        .expect("configuration selects one shared exact source")
    }
}

fn artifact_content() -> source::Content {
    source::Content::File {
        bytes: ARTIFACT.len() as u64,
        sha256: source::Sha256Digest::of(ARTIFACT),
    }
}

fn options() -> build::DocumentationGenerationOptions {
    build::DocumentationGenerationOptions {
        input: source::Limits::default(),
        output: source::Limits::default(),
        execution: process::Options {
            allow_execute: true,
            timeout: Duration::from_secs(30),
            ..Default::default()
        },
        max_capture_bytes: 65536,
        max_jobs: 8,
        directory_bytes: 16 << 10,
        entry_bytes: 4 << 10,
    }
}

fn file<'a>(tree: &'a source::CapturedSource, path: &str) -> &'a [u8] {
    tree.file(&path.parse().expect("fixture output entry"))
        .expect("complete target bytes")
}

/// Assert the actual application-to-target seam, then require real Mintlify output.
/// No native file reads or replacement success implementation occur inside this target.
struct CheckingTarget<'a> {
    imported: &'a source::AcquiredInput,
    jobs: &'a [build::DocumentationJob],
}

impl DocumentationTarget for CheckingTarget<'_> {
    fn preflight(&self, plan: documentation::Plan<'_>) -> Result<(), documentation::Error> {
        assert_eq!(plan.projects.len(), self.jobs.len() + 1);
        Ok(())
    }

    fn assemble(
        &self,
        request: documentation::Request<'_>,
        limits: source::Limits,
    ) -> Result<source::CapturedSource, documentation::Error> {
        assert_eq!(request.projects.len(), self.jobs.len() + 1);
        for (index, module) in request.projects.iter().enumerate() {
            let [documentation::ProjectItem::Reference(reference)] = module.navigation.as_slice()
            else {
                panic!("fixture module must retain its one configured reference")
            };
            let artifact = &reference.artifact;
            assert_eq!(artifact.format, documentation::Format::TypeDocJson);
            assert_eq!(artifact.bytes()?, ARTIFACT);
            if index == 0 {
                assert_eq!(
                    artifact.selection.path().expect("file selection").as_str(),
                    "payload.json"
                );
                assert_eq!(artifact.tree.inventory(), self.imported.inventory());
                for entry in self.imported.inventory().entries() {
                    if let source::Content::File { .. } = entry.content {
                        let expected = self.imported.file(&entry.path).expect("captured source");
                        let actual = artifact
                            .tree
                            .file(&entry.path)
                            .expect("complete target input");
                        assert_eq!(actual, expected);
                        assert!(
                            std::ptr::eq(actual, expected),
                            "target borrows retained bytes"
                        );
                    }
                }
                assert_eq!(
                    artifact
                        .tree
                        .file(&"payload.json.companion".parse().expect("companion")),
                    Some(COMPANION),
                );
            } else {
                let job = &self.jobs[index - 1];
                assert_eq!(artifact.selection, &job.selection);
                assert_eq!(artifact.tree.inventory().identity(), job.output_identity);
                assert_eq!(artifact.tree.inventory().entries().len(), 2);
                assert_eq!(
                    artifact
                        .tree
                        .file(&"reference.json.companion".parse().expect("companion")),
                    Some(b"companion".as_slice()),
                );
                assert_ne!(
                    artifact.tree.inventory().identity().sha256(),
                    match job.content {
                        source::Content::File { sha256, .. } => sha256,
                        source::Content::Directory => panic!("file fixture"),
                    }
                );
            }
        }
        gazette_adapter_mintlify::Mintlify::new(&gazette_adapter_markdown::Comrak)
            .assemble(request, limits)
    }
}

#[test]
fn configured_mixed_modules_share_capture_and_existing_target_with_exact_job_evidence() {
    let fixture = Fixture::new(COPY, 2, "typedoc-json");
    let inputs = fixture.acquire();
    assert_eq!(inputs.sources().len(), 1);
    assert_eq!(
        build::assemble_documentation(&inputs, source::Limits::default())
            .expect_err("generated selection cannot fall back to imported bytes")
            .code(),
        "documentation_generation_required"
    );
    let generated = build::generate_documentation(&inputs, fixture.jobs.path(), options())
        .expect("two configured native jobs");
    assert_eq!(generated.jobs().len(), 2);
    let target = CheckingTarget {
        imported: inputs.sources().values().next().expect("shared source"),
        jobs: generated.jobs(),
    };
    let assembly = generated
        .assemble_documentation(&target, source::Limits::default())
        .expect("same Mintlify target for imported and generated references");
    assert!(std::ptr::eq(assembly.inputs(), &inputs));
    assert_eq!(assembly.repository().name, "owner-selected-docs");
    assert_eq!(
        PathBuf::from(&assembly.repository().path),
        fixture.destination
    );
    assert!(!fixture.destination.exists());
    assert_eq!(assembly.artifacts().len(), 3);
    assert!(assembly.artifacts()[0].generation.is_none());
    assert_eq!(
        assembly.artifacts()[0].tree,
        target.imported.inventory().identity()
    );
    for (offset, job) in generated.jobs().iter().enumerate() {
        let index = offset + 1;
        assert_eq!((job.module, job.reference), (index, 0));
        assert_eq!(
            job.evidence.version_text.as_deref(),
            Some("owned-transport/1\n")
        );
        assert_eq!(
            job.evidence.input,
            inputs
                .sources()
                .values()
                .next()
                .expect("shared source")
                .inventory()
                .identity()
        );
        assert_eq!(
            fs::read(job.output.join("reference.json")).expect("retained native bytes"),
            ARTIFACT
        );
        assert_eq!(
            fs::read(job.output.join("reference.json.companion")).expect("companion retained"),
            b"companion"
        );
        assert_eq!(assembly.artifacts()[index].content, artifact_content());
        assert_eq!(assembly.artifacts()[index].tree, job.output_identity);
        assert_eq!(
            serde_json::to_value(assembly.artifacts()[index].generation)
                .expect("artifact evidence"),
            serde_json::to_value(&job.evidence).expect("exact original job evidence")
        );
    }
    for index in 0..3 {
        assert_eq!(
            file(
                assembly.output(),
                &format!("artifacts/module-{index}/library.json")
            ),
            ARTIFACT
        );
    }
    let navigation: Value =
        serde_json::from_slice(file(assembly.output(), "docs.json")).expect("target navigation");
    assert_eq!(navigation["name"], "Configured publication");
    let dropdowns = navigation["navigation"]["dropdowns"]
        .as_array()
        .expect("multiple projects use the configured project dropdowns");
    assert_eq!(dropdowns.len(), 3);
    for (index, dropdown) in dropdowns.iter().enumerate() {
        assert_eq!(dropdown["dropdown"], format!("Module {index}"));
        assert!(
            dropdown["pages"]
                .as_array()
                .is_some_and(|pages| !pages.is_empty())
        );
    }
}

#[test]
fn frozen_capture_and_repeated_assembly_never_reread_native_sources_or_rerun_jobs() {
    let fixture = Fixture::new(COPY, 1, "typedoc-json");
    let inputs = fixture.acquire();
    fs::write(
        fixture.root.path().join("inputs/payload.json"),
        b"changed after acquisition",
    )
    .expect("native source diverges from retained input");
    fs::write(
        fixture.root.path().join("inputs/generator.sh"),
        b"exit 91\n",
    )
    .expect("original script cannot be silently reread");
    fs::write(
        fixture.root.path().join("inputs/payload.json.companion"),
        b"changed companion",
    )
    .expect("original companion diverges");
    let generated = build::generate_documentation(&inputs, fixture.jobs.path(), options())
        .expect("captured recipe source");
    let target = CheckingTarget {
        imported: inputs.sources().values().next().expect("retained source"),
        jobs: generated.jobs(),
    };
    let original = generated
        .assemble_documentation(&target, source::Limits::default())
        .expect("first pure assembly");
    for job in generated.jobs() {
        fs::write(job.input.join("generator.sh"), b"exit 92\n").expect("rerun would now fail");
        fs::write(
            job.output.join("reference.json"),
            b"changed after generated capture",
        )
        .expect("native output diverges");
        fs::write(
            job.output.join("reference.json.companion"),
            b"changed generated companion",
        )
        .expect("native companion diverges");
    }
    let repeated = generated
        .assemble_documentation(&target, source::Limits::default())
        .expect("no process or path rereads");
    assert_eq!(original.output().inventory(), repeated.output().inventory());
    assert_eq!(
        serde_json::to_value(original.artifacts()).expect("original evidence"),
        serde_json::to_value(repeated.artifacts()).expect("retained evidence"),
    );
    assert_eq!(
        file(repeated.output(), "artifacts/module-1/library.json"),
        ARTIFACT
    );
    assert_eq!(
        fs::read_dir(fixture.jobs.path())
            .expect("retained stages")
            .count(),
        2
    );
}

#[test]
fn imported_reference_binds_complete_tree_and_reassembly_ignores_changed_companion() {
    let fixture = Fixture::new(COPY, 0, "typedoc-json");
    let inputs = fixture.acquire();
    let target = CheckingTarget {
        imported: inputs.sources().values().next().expect("captured source"),
        jobs: &[],
    };
    let original = inputs
        .assemble_documentation(&target, Default::default())
        .expect("real imported-reference Mintlify assembly");
    let evidence = &original.artifacts()[0];
    assert_eq!(evidence.tree, target.imported.inventory().identity());
    assert_eq!(evidence.content, artifact_content());
    assert_ne!(evidence.tree.sha256(), source::Sha256Digest::of(ARTIFACT));
    assert!(evidence.generation.is_none());
    fs::write(
        fixture.root.path().join("inputs/payload.json.companion"),
        b"only the companion changed",
    )
    .expect("unretained native companion mutation");
    assert_eq!(
        source::Sha256Digest::of(
            &fs::read(fixture.root.path().join("inputs/payload.json"))
                .expect("unchanged native entry")
        ),
        source::Sha256Digest::of(ARTIFACT),
    );
    let changed = source::inspect(
        &fixture.root.path().join("inputs"),
        None,
        Default::default(),
    )
    .expect("independent changed native inventory");
    assert_ne!(changed.identity(), evidence.tree);
    let repeated = inputs
        .assemble_documentation(&target, Default::default())
        .expect("complete retained tree reaches actual target without native reread");
    assert_eq!(original.output().inventory(), repeated.output().inventory());
    assert_eq!(
        file(repeated.output(), "artifacts/module-0/library.json"),
        ARTIFACT
    );
    assert_eq!(
        serde_json::to_value(original.artifacts()).expect("original binding"),
        serde_json::to_value(repeated.artifacts()).expect("unchanged retained binding"),
    );
    assert!(
        fs::read_dir(fixture.jobs.path())
            .expect("no generator stages")
            .next()
            .is_none()
    );
    assert!(!fixture.destination.exists());
}

#[test]
fn permission_and_complete_job_bounds_refuse_before_creating_any_stage() {
    let fixture = Fixture::new(COPY, 2, "typedoc-json");
    let inputs = fixture.acquire();
    let mut denied = options();
    denied.execution.allow_execute = false;
    let error = build::generate_documentation(&inputs, fixture.jobs.path(), denied)
        .expect_err("execution denied");
    assert!(
        matches!(error.cause, build::DocumentationGenerationCause::Process(ref cause) if cause.failure == process::Failure::Permission)
    );
    assert!(error.completed.is_empty() && error.retained.is_empty());
    let mut bounded = options();
    bounded.max_jobs = 1;
    let error = build::generate_documentation(&inputs, fixture.jobs.path(), bounded)
        .expect_err("whole configured set bound");
    assert_eq!(error.code(), "documentation_generation_limit");
    assert!(error.completed.is_empty() && error.retained.is_empty());
    assert!(
        fs::read_dir(fixture.jobs.path())
            .expect("unchanged job parent")
            .next()
            .is_none()
    );
}

#[test]
fn later_execution_failure_retains_completed_prefix_and_exact_failing_residue() {
    let fixture = Fixture::new(
        b"if [ \"$3\" = 2 ]; then printf partial > \"$2\"; exit 7; fi\n/bin/cat \"$1\" > \"$2\"\n",
        2,
        "typedoc-json",
    );
    let inputs = fixture.acquire();
    let error = build::generate_documentation(&inputs, fixture.jobs.path(), options())
        .expect_err("second job fails whole operation");
    assert_eq!(error.code(), "documentation_generator_exit");
    assert_eq!(error.completed.len(), 1);
    assert_eq!(error.retained.len(), 4);
    let completed = &error.completed[0];
    assert_eq!(
        completed.selection.path().expect("selected file").as_str(),
        "reference.json"
    );
    assert_eq!(completed.content, artifact_content());
    assert_eq!(
        completed.output_identity,
        source::inspect(&completed.output, None, Default::default())
            .expect("independent completed output read-back")
            .identity()
    );
    assert_eq!(
        fs::read(error.completed[0].output.join("reference.json"))
            .expect("completed prefix remains"),
        ARTIFACT
    );
    let build::DocumentationGenerationCause::Generator(native) = &error.cause else {
        panic!("expected exact native generator failure: {error:?}");
    };
    assert_eq!(
        native
            .evidence
            .generation
            .as_ref()
            .expect("actual invocation")
            .exit
            .expect("exit observed")
            .code,
        Some(7)
    );
    assert_eq!(
        fs::read(native.output.join("reference.json")).expect("partial output retained"),
        b"partial"
    );
    assert!(error.retained.contains(&native.input) && error.retained.contains(&native.output));
    let retained = error.retained.clone();
    drop(error);
    assert!(
        retained.iter().all(|path| path.is_dir()),
        "dropping evidence does not delete custody"
    );
    assert!(!fixture.destination.exists());
}

#[test]
fn unsupported_target_format_refuses_before_generator_staging_or_execution() {
    let fixture = Fixture::new(COPY, 1, "rustdoc-json");
    let inputs = fixture.acquire();
    let error = build::generate_documentation(&inputs, fixture.jobs.path(), options())
        .expect_err("statically unsupported target format must refuse before effects");
    assert!(
        error
            .to_string()
            .contains("unsupported native Rustdoc format")
    );
    assert!(error.completed.is_empty());
    assert!(error.retained.is_empty());
    assert_eq!(
        fs::read_dir(fixture.jobs.path())
            .expect("unchanged jobs parent")
            .count(),
        0
    );
    assert!(!fixture.destination.exists());
}

#[test]
fn invalid_target_presentation_and_namespaces_refuse_before_generator_effects() {
    for (from, to, expected) in [
        (
            "theme='mint'",
            "theme='unknown'",
            "unsupported Mintlify theme",
        ),
        (
            "primary_color='#123abc'",
            "primary_color='blue'",
            "expected #RGB",
        ),
        ("slug='module-1'", "slug='Upper'", "lowercase URL segment"),
    ] {
        let mut fixture = Fixture::new(COPY, 1, "typedoc-json");
        fixture.config = fixture.config.replacen(from, to, 1);
        let inputs = fixture.acquire();
        let error = build::generate_documentation(&inputs, fixture.jobs.path(), options())
            .expect_err("target profile must be checked before native generator effects");
        assert!(error.to_string().contains(expected), "{error}");
        assert!(error.completed.is_empty());
        assert!(error.retained.is_empty());
        assert_eq!(
            fs::read_dir(fixture.jobs.path())
                .expect("unchanged jobs parent")
                .count(),
            0
        );
        assert!(!fixture.destination.exists());
    }
}

#[test]
fn target_staging_preserves_every_generated_input_and_output_root() {
    let fixture = Fixture::new(COPY, 2, "typedoc-json");
    let inputs = fixture.acquire();
    let generated = build::generate_documentation(&inputs, fixture.jobs.path(), options())
        .expect("complete generated set");
    let assembly = build::assemble_generated_documentation(&generated, Default::default())
        .expect("complete target");
    for root in generated
        .jobs()
        .iter()
        .flat_map(|job| [&job.input, &job.output])
    {
        let before = source::inspect(root, None, Default::default())
            .expect("original retained job inventory");
        build::stage_documentation(&assembly, root, Default::default(), 16 << 10)
            .expect_err("assembled output cannot enter a retained generator stage");
        assert_eq!(
            source::inspect(root, None, Default::default())
                .expect("protected stage remains readable")
                .identity(),
            before.identity()
        );
    }
    assert_eq!(
        fs::read_dir(fixture.jobs.path())
            .expect("same retained children")
            .count(),
        4
    );
    assert!(!fixture.destination.exists());
}

#[test]
fn repeated_input_totals_are_checked_before_staging_not_only_per_distinct_source() {
    let fixture = Fixture::new(COPY, 2, "typedoc-json");
    let inputs = fixture.acquire();
    let inventory = inputs
        .sources()
        .values()
        .next()
        .expect("one shared source")
        .inventory();
    let mut byte_limit = options();
    byte_limit.input.total_bytes = inventory.total_bytes() * 2 - 1;
    let mut entry_limit = options();
    entry_limit.input.entries = inventory.entries().len() * 2 - 1;
    for limits in [byte_limit, entry_limit] {
        let error = build::generate_documentation(&inputs, fixture.jobs.path(), limits)
            .expect_err("shared capture is materialized twice and counts twice");
        assert_eq!(error.code(), "documentation_generation_limit");
        assert!(error.completed.is_empty() && error.retained.is_empty());
        assert!(
            fs::read_dir(fixture.jobs.path())
                .expect("no stages created")
                .next()
                .is_none()
        );
    }
}

#[test]
fn generated_output_totals_are_decremented_across_jobs_without_losing_first_identity() {
    for constrain_bytes in [true, false] {
        let fixture = Fixture::new(COPY, 2, "typedoc-json");
        let inputs = fixture.acquire();
        let mut limits = options();
        if constrain_bytes {
            limits.output.total_bytes = (ARTIFACT.len() as u64 + b"companion".len() as u64) * 2 - 1;
        } else {
            limits.output.entries = 3;
        }
        let error = build::generate_documentation(&inputs, fixture.jobs.path(), limits)
            .expect_err("each job fits independently, but the complete set exceeds its allowance");
        assert_eq!(error.completed.len(), 1);
        assert_eq!(error.retained.len(), 4);
        let first = &error.completed[0];
        assert_eq!(
            first.selection.path().expect("selected file").as_str(),
            "reference.json"
        );
        assert_eq!(first.content, artifact_content());
        assert_eq!(
            first.output_identity,
            source::inspect(&first.output, None, Default::default())
                .expect("exact completed tree")
                .identity()
        );
        let build::DocumentationGenerationCause::Generator(native) = &error.cause else {
            panic!("expected bounded native output acquisition failure: {error:?}");
        };
        assert!(matches!(
            &native.cause,
            gazette_core::documentation::generator::Cause::Source(error)
                if matches!(error.as_ref(), source::Error::Limit { .. })
        ));
        assert!(matches!(
            native.phase,
            gazette_core::documentation::generator::Phase::Capture
        ));
        assert_eq!(
            native
                .evidence
                .generation
                .as_ref()
                .expect("second native process ran")
                .exit
                .expect("actual exit")
                .code,
            Some(0)
        );
        assert_eq!(
            fs::read(native.output.join("reference.json")).expect("uncertified residue retained"),
            ARTIFACT
        );
        assert!(!fixture.destination.exists());
    }
}

/// Deliberately violates only the consumer's limits; actual execution and capture remain native.
struct RelaxedOutput;

impl Generator for RelaxedOutput {
    fn generate(
        &self,
        mut request: documentation::generator::Request<'_>,
    ) -> Result<documentation::generator::Generated, documentation::generator::Error> {
        request.output_limits.file_bytes = 65536;
        request.output_limits.depth = 8;
        gazette_adapter_documentation::NativeGenerator::new(
            &gazette_adapter_fs::FileSystem,
            &gazette_adapter_process::Runner,
        )
        .generate(request)
    }
}

#[test]
fn application_rechecks_returned_companion_file_and_depth_limits_without_losing_stages() {
    for (script, excessive_path, file_bytes, depth) in [
        (
            b"printf api > \"$2/api.json\"\nprintf 123456789 > \"$2/companion\"\n".as_slice(),
            "companion", 8, 8,
        ),
        (
            b"printf api > \"$2/api.json\"\n/bin/mkdir \"$2/nested\"\nprintf companion > \"$2/nested/companion\"\n".as_slice(),
            "nested/companion", 65536, 1,
        ),
    ] {
        let mut fixture = Fixture::new(script, 1, "typedoc-json");
        fixture.config = fixture.config.replace(
            "output={kind='file',path='reference.json'}", "output={kind='directory'}",
        );
        let inputs = fixture.acquire();
        let mut selected = options();
        selected.output = source::Limits { file_bytes, depth, entries: 8, total_bytes: 65536 };
        let error = inputs.generate_documentation(
            &gazette_adapter_fs::FileSystem,
            &RelaxedOutput,
            &gazette_adapter_mintlify::Mintlify::new(&gazette_adapter_markdown::Comrak),
            fixture.jobs.path(),
            selected,
        ).expect_err("application must recheck every returned tree bound, not only totals/counts");
        assert!(matches!(error.cause, build::DocumentationGenerationCause::Limit));
        assert!(error.completed.is_empty(), "rejected output cannot become a completed job");
        assert_eq!(error.retained.len(), 2);
        let output = &error.retained[1];
        let observed = source::inspect(output, None, Default::default())
            .expect("real generator tree remains in native custody");
        assert!(observed.total_bytes() < selected.output.total_bytes);
        assert!(observed.entries().len() < selected.output.entries);
        assert_eq!(fs::read(output.join("api.json")).expect("native process completed"), b"api");
        assert!(output.join(excessive_path).is_file());
        if depth == 1 {
            assert!(observed.entries().iter().any(|entry| entry.path.as_str().split('/').count() > depth));
            assert!(observed.entries().iter().all(|entry| !matches!(entry.content,
                source::Content::File {bytes,..} if bytes > file_bytes)));
        } else {
            assert!(observed.entries().iter().any(|entry| matches!(entry.content,
                source::Content::File {bytes,..} if bytes > file_bytes)));
            assert!(observed.entries().iter().all(|entry| entry.path.as_str().split('/').count() <= depth));
        }
        assert!(error.retained.iter().all(|path| path.is_dir()));
        assert_eq!(fs::read_dir(fixture.jobs.path()).expect("only retained stages").count(), 2);
        assert!(!fixture.destination.exists());
    }
}
