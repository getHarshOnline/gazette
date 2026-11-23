//! Native commands use only retained workspace and machine coordinates.

use gazette_adapter_fs::FileSystem;
use gazette_application::{
    build::{CandidatePolicyRequirement, OutputPolicyRequirement, ReviewPolicy},
    configuration,
};
use gazette_core::{process, source::Sha256Digest};
use std::{fs, path::Path};

const TOPOLOGY: &str = "principals=[{id='owner:one',kind='person'}]\nsites=[{id='site:one',publisher='owner:one',deployable=false}]\n";
const PIN: &str =
    "{bytes=1,sha256='0000000000000000000000000000000000000000000000000000000000000000'}";

fn declaration(program: &str, directory: &str, arguments: &str) -> String {
    format!(
        "{TOPOLOGY}[policy_check]\nprogram={program:?}\ndirectory={directory:?}\narguments={arguments}\nexecutable={PIN}\nenvironment={{VALUE='machine:runtime/no-expansion'}}\n"
    )
}

fn load(root: &Path, declaration: &str, binding: &str) -> configuration::LoadedWorkspace {
    fs::create_dir_all(root.join("tools")).unwrap();
    fs::write(
        root.join("gazette.toml"),
        format!("machine_binding='tools/studio.local.toml'\n{declaration}"),
    )
    .unwrap();
    fs::write(root.join("tools/studio.local.toml"), binding).unwrap();
    configuration::load(
        &FileSystem,
        root,
        &"gazette.toml".parse().unwrap(),
        Default::default(),
    )
    .unwrap()
}

fn digest(command: &impl serde::Serialize) -> Sha256Digest {
    Sha256Digest::of(&serde_json::to_vec(command).unwrap())
}

#[test]
fn relocation_binds_program_directory_and_typed_arguments_together() {
    let source = declaration(
        "machine:runtime/bin/node",
        "machine:checker",
        "['-B',{path='machine:checker/check.py'},'--project',{path='project'},'machine:literal/file','$HOME; $(ignored)']",
    );
    let binding =
        "[roots]\nruntime='../runtime'\nchecker='../checker'\n[unrelated]\nsetting='kept'\n";
    let mut previous = None;
    for root in [tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap()] {
        let loaded = load(root.path(), &source, binding);
        let logical = loaded.documents().compilation.source_sha256;
        let resolved = loaded.into_resolved();
        let command = resolved.policy_command().unwrap().unwrap();
        // Binding coordinates retain their original native spelling; no canonicalization reads.
        let runtime = root.path().join("tools/../runtime");
        let checker = root.path().join("tools/../checker");
        assert_eq!(command.program, runtime.join("bin/node").to_str().unwrap());
        assert_eq!(command.directory, checker.to_str().unwrap());
        assert_eq!(
            command.arguments,
            [
                "-B".to_owned(),
                checker.join("check.py").to_str().unwrap().to_owned(),
                "--project".into(),
                root.path().join("project").to_str().unwrap().to_owned(),
                "machine:literal/file".into(),
                "$HOME; $(ignored)".into()
            ]
        );
        assert_eq!(command.environment["VALUE"], "machine:runtime/no-expansion");
        command.validate().unwrap();
        if let Some((old_logical, old_command)) = previous {
            assert_eq!(logical, old_logical);
            assert_ne!(digest(command), old_command);
        }
        previous = Some((logical, digest(command)));
    }
}

#[test]
fn binding_is_captured_once_and_independent_expectations_use_bound_command() {
    let root = tempfile::tempdir().unwrap();
    let source = declaration(
        "machine:runtime/python",
        "machine:checker",
        "[{path='machine:checker/check.py'}]",
    );
    let loaded = load(
        root.path(),
        &source,
        "[roots]\nruntime='/selected/runtime'\nchecker='/selected/checker'\n",
    );
    let binding_bytes = loaded
        .source(&"tools/studio.local.toml".parse().unwrap())
        .unwrap()
        .to_vec();
    fs::write(
        root.path().join("tools/studio.local.toml"),
        "[roots]\nruntime='/replacement'\nchecker='/replacement'\n",
    )
    .unwrap();
    assert_eq!(
        loaded
            .source(&"tools/studio.local.toml".parse().unwrap())
            .unwrap(),
        binding_bytes
    );
    let resolved = loaded.into_resolved();
    let command = resolved.policy_command().unwrap().unwrap();
    let expected = process::Command {
        program: "/selected/runtime/python".into(),
        directory: "/selected/checker".into(),
        executable: command.executable,
        arguments: vec!["/selected/checker/check.py".into()],
        environment: command.environment.clone(),
    };
    assert_eq!(command, &expected);
    let policy = ReviewPolicy {
        id: "policy:one".parse().unwrap(),
        revision: "revision:one".parse().unwrap(),
        source_sha256: Sha256Digest::of(b"policy"),
    };
    let mut output = OutputPolicyRequirement {
        policy: policy.clone(),
        command_sha256: digest(&expected),
        response_bytes: 1024,
    };
    output.check_command(command).unwrap();
    CandidatePolicyRequirement {
        policy,
        command_sha256: output.command_sha256,
    }
    .check_command(command)
    .unwrap();
    output.command_sha256 = digest(resolved.compilation().workspace.policy_check().unwrap());
    assert!(
        output.check_command(command).is_err(),
        "portable declarations are not native command identity"
    );
    let fresh = configuration::load(
        &FileSystem,
        root.path(),
        &"gazette.toml".parse().unwrap(),
        Default::default(),
    )
    .unwrap()
    .into_resolved();
    assert_ne!(
        digest(fresh.policy_command().unwrap().unwrap()),
        digest(command)
    );
}

#[test]
fn missing_roots_refuse_each_command_part_without_blocking_inspection() {
    for (program, directory, arguments, field) in [
        ("machine:missing/checker", "/", "[]", "program"),
        ("/checker", "machine:missing", "[]", "directory"),
        (
            "/checker",
            "/",
            "[{path='machine:missing/file'}]",
            "argument",
        ),
        ("/checker", ".", "[]", "directory"),
    ] {
        let resolved = configuration::resolve(
            &declaration(program, directory, arguments),
            Default::default(),
        )
        .unwrap();
        assert!(resolved.compilation().workspace.policy_check().is_some());
        let error = resolved.policy_command().unwrap_err();
        assert_eq!(error.code(), "runtime_command_binding");
        assert!(error.to_string().contains(field));
        assert!(resolved.math_command().unwrap().is_none());
    }
    let root = tempfile::tempdir().unwrap();
    let loaded = load(
        root.path(),
        &declaration("machine:missing/checker", "/", "[]"),
        "[roots]\nother='/unused'\n",
    );
    assert!(loaded.into_resolved().policy_command().is_err());
}

#[test]
fn expanded_command_is_bounded_and_absolute_legacy_command_stays_exact() {
    let root = tempfile::tempdir().unwrap();
    let arguments = format!("[{}]", vec!["{path='machine:runtime/file'}"; 128].join(","));
    let source = declaration("machine:runtime/checker", "/", &arguments);
    let loaded = load(
        root.path(),
        &source,
        &format!("[roots]\nruntime='/{}'\n", "x".repeat(512)),
    );
    assert!(matches!(
        loaded.into_resolved().policy_command(),
        Err(configuration::RuntimeBindingError::Command("process_limit"))
    ));
    let source = declaration("/checker", "/", "['literal']");
    let resolved = configuration::resolve(&source, Default::default()).unwrap();
    assert_eq!(
        digest(resolved.policy_command().unwrap().unwrap()),
        digest(resolved.compilation().workspace.policy_check().unwrap())
    );
}

#[test]
fn math_and_checker_bind_independently_from_the_same_machine_file() {
    let root = tempfile::tempdir().unwrap();
    let source = format!(
        "{}\n[math_typesetter]\nprogram='machine:runtime/node'\ndirectory='.'\narguments=[{{path='machine:runtime/options'}}]\nexecutable={PIN}\nsource='source:math'\nentrypoint='render.mjs'\n[[sources]]\nid='source:math'\nlocation={{kind='filesystem',root='math'}}\n",
        declaration("machine:missing/python", "/", "[]")
    );
    let resolved = load(root.path(), &source, "[roots]\nruntime='/runtime'\n").into_resolved();
    let command = resolved.math_command().unwrap().unwrap();
    assert_eq!(command.program, "/runtime/node");
    assert_eq!(command.directory, root.path().to_str().unwrap());
    assert_eq!(command.arguments, ["/runtime/options"]);
    assert!(resolved.policy_command().is_err());
}

fn generator_recipe()
-> gazette_core::documentation::generator::Recipe<gazette_compiler::workspace::RuntimeArgument> {
    serde_json::from_value(serde_json::json!({
        "program":"machine:tools/bin/python",
        "executable":{"bytes":1,"sha256":"0".repeat(64)},
        "arguments":[{"kind":"input-root"},{"kind":"output-root"}],
        "environment":{"PYTHONPATH":{"path":"machine:packages"},"LITERAL":"machine:missing/no-expansion"},
        "version_arguments":["--version"],"expected_version_sha256":"1".repeat(64),
        "output":{"kind":"directory"},"format":"sphinx-json"
    })).unwrap()
}

#[test]
fn generator_paths_use_captured_binding_and_preserve_native_recipe_requirements() {
    let root = tempfile::tempdir().unwrap();
    let loaded = load(
        root.path(),
        TOPOLOGY,
        "[roots]\ntools='/selected/tools'\npackages='../packages'\n",
    );
    fs::write(
        root.path().join("tools/studio.local.toml"),
        "[roots]\ntools='/replacement'\npackages='/replacement'\n",
    )
    .unwrap();
    let resolved = loaded.into_resolved();
    let selected = generator_recipe();
    let bound = resolved.documentation_recipe(&selected).unwrap();
    assert_eq!(bound.program, "/selected/tools/bin/python");
    assert_eq!(
        bound.environment["PYTHONPATH"],
        root.path().join("tools/../packages").to_str().unwrap()
    );
    assert_eq!(bound.environment["LITERAL"], "machine:missing/no-expansion");
    assert_eq!(bound.arguments, selected.arguments);
    assert_eq!(bound.executable, selected.executable);
    assert_eq!(
        bound.expected_version_sha256,
        selected.expected_version_sha256
    );
    bound.validate().unwrap();
    let mut invalid = selected.clone();
    invalid.arguments.remove(0);
    assert!(resolved.documentation_recipe(&invalid).is_err());
}

#[test]
fn generator_missing_roots_and_expanded_environment_limits_refuse_before_execution() {
    let detached = configuration::resolve(TOPOLOGY, Default::default()).unwrap();
    let mut selected = generator_recipe();
    assert!(matches!(
        detached.documentation_recipe(&selected),
        Err(configuration::RuntimeBindingError::Root {
            field: "program",
            ..
        })
    ));
    selected.program = "/selected/python".into();
    assert!(matches!(
        detached.documentation_recipe(&selected),
        Err(configuration::RuntimeBindingError::Root {
            field: "environment",
            ..
        })
    ));
    selected.environment.clear();
    detached.documentation_recipe(&selected).unwrap();
    selected.program = "tools/python".into();
    assert!(
        detached.documentation_recipe(&selected).is_err(),
        "supplied buffers never use process cwd"
    );
    let root = tempfile::tempdir().unwrap();
    let resolved = load(
        root.path(),
        TOPOLOGY,
        &format!(
            "[roots]\ntools='/tools'\npackages='/{}'\n",
            "x".repeat(2048)
        ),
    )
    .into_resolved();
    selected = generator_recipe();
    selected.environment = (0..64)
        .map(|i| {
            (
                format!("PATH_{i}"),
                gazette_compiler::workspace::RuntimeArgument::Path {
                    path: "machine:packages".into(),
                },
            )
        })
        .collect();
    assert!(matches!(
        resolved.documentation_recipe(&selected),
        Err(configuration::RuntimeBindingError::Command("process_limit"))
    ));
}

#[test]
fn generator_tool_arguments_bind_once_in_generation_version_and_validation() {
    use gazette_core::documentation::generator::Argument;
    let mut selected = generator_recipe();
    selected.arguments.extend([
        Argument::Path {
            path: "machine:tools/plugin.mjs".into(),
        },
        Argument::Literal {
            value: "machine:missing/literal $HOME".into(),
        },
        Argument::Path {
            path: "local/check.mjs".into(),
        },
    ]);
    selected.version_arguments.insert(
        0,
        gazette_compiler::workspace::RuntimeArgument::Path {
            path: "machine:tools/cli.mjs".into(),
        },
    );
    selected.validation_arguments = Some(vec![
        Argument::Path {
            path: "machine:tools/check.mjs".into(),
        },
        Argument::Output,
    ]);
    for root in [tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap()] {
        let loaded = load(
            root.path(),
            TOPOLOGY,
            "[roots]\ntools='../tools-selected'\npackages='/packages'\n",
        );
        fs::write(
            root.path().join("tools/studio.local.toml"),
            "[roots]\ntools='/replaced'\n",
        )
        .unwrap();
        let bound = loaded
            .into_resolved()
            .documentation_recipe(&selected)
            .unwrap();
        let tools = root.path().join("tools/../tools-selected");
        assert_eq!(
            bound.version_arguments,
            [tools.join("cli.mjs").to_str().unwrap(), "--version"]
        );
        assert_eq!(
            bound.arguments[2],
            Argument::Path {
                path: tools.join("plugin.mjs").to_str().unwrap().into()
            }
        );
        assert_eq!(bound.arguments[3], selected.arguments[3]);
        assert_eq!(
            bound.arguments[4],
            Argument::Path {
                path: root.path().join("local/check.mjs").to_str().unwrap().into()
            }
        );
        assert_eq!(
            bound.validation_arguments.as_ref().unwrap()[0],
            Argument::Path {
                path: tools.join("check.mjs").to_str().unwrap().into()
            }
        );
        assert_eq!(bound.executable, selected.executable);
        assert_eq!(
            bound.expected_version_sha256,
            selected.expected_version_sha256
        );
    }
}

#[test]
fn every_generator_argument_boundary_refuses_unbound_or_oversized_paths() {
    use gazette_compiler::workspace::RuntimeArgument;
    use gazette_core::documentation::generator::Argument;
    let detached = configuration::resolve(TOPOLOGY, Default::default()).unwrap();
    for coordinate in [
        "machine:missing/tool",
        "relative/tool",
        "machine:tools/../escape",
    ] {
        for boundary in 0..3 {
            let mut selected = generator_recipe();
            selected.program = "/python".into();
            selected.environment.clear();
            match boundary {
                0 => selected.arguments.push(Argument::Path {
                    path: coordinate.into(),
                }),
                1 => selected.version_arguments.push(RuntimeArgument::Path {
                    path: coordinate.into(),
                }),
                _ => {
                    selected.validation_arguments = Some(vec![
                        Argument::Output,
                        Argument::Path {
                            path: coordinate.into(),
                        },
                    ])
                }
            }
            assert!(
                detached.documentation_recipe(&selected).is_err(),
                "{boundary}: {coordinate}"
            );
        }
    }
    let root = tempfile::tempdir().unwrap();
    let resolved = load(
        root.path(),
        TOPOLOGY,
        &format!(
            "[roots]\ntools='/{}'\npackages='/packages'\n",
            "x".repeat(1024)
        ),
    )
    .into_resolved();
    for boundary in 0..3 {
        let mut selected = generator_recipe();
        match boundary {
            0 => selected.arguments.extend(vec![
                Argument::Path {
                    path: "machine:tools/plugin".into()
                };
                128
            ]),
            1 => selected.version_arguments.extend(vec![
                RuntimeArgument::Path {
                    path: "machine:tools/cli".into()
                };
                128
            ]),
            _ => {
                selected.validation_arguments = Some(
                    std::iter::once(Argument::Output)
                        .chain(std::iter::repeat_n(
                            Argument::Path {
                                path: "machine:tools/check".into(),
                            },
                            128,
                        ))
                        .collect(),
                )
            }
        }
        assert!(
            matches!(
                resolved.documentation_recipe(&selected),
                Err(configuration::RuntimeBindingError::Command("process_limit"))
            ),
            "{boundary}"
        );
    }
}
