//! Capability-first direct CLI discovery over the unchanged command surface.

use std::{collections::BTreeSet, process::Command};

use gazette_cli::command::metadata::{
    self, Closure, Effect, Intent, PotentialEffect, RelationKind,
};

fn executable() -> &'static str {
    env!("CARGO_BIN_EXE_gazette")
}

#[test]
fn complete_cli_surface_has_canonical_metadata_and_grouped_help() {
    let output = Command::new(executable())
        .arg("--help")
        .output()
        .expect("run Gazette help");
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let help = String::from_utf8(output.stdout).expect("UTF-8 help");
    let guide_index = help.find("Capability guide").expect("capability guide");
    let commands_index = help.find("Commands:\n").expect("complete command list");
    assert!(
        guide_index < commands_index,
        "the concise capability guide must precede the exhaustive command list"
    );
    let guide = help
        .split_once("Capability guide")
        .map(|(_, guide)| guide)
        .expect("capability-first guide");
    for capability in [
        "Authoring",
        "Build",
        "Documentation",
        "Observation",
        "Preview",
        "Recovery",
        "Release",
        "Store",
        "Workspace",
    ] {
        assert!(guide.contains(capability), "missing {capability}: {guide}");
    }

    let command_block = help
        .split_once("Commands:\n")
        .and_then(|(_, tail)| tail.split_once("\n\nOptions:"))
        .map(|(commands, _)| commands)
        .expect("complete Clap command list");
    for name in command_block.lines().filter_map(|line| {
        line.strip_prefix("  ")
            .filter(|entry| !entry.starts_with(' '))
            .and_then(|entry| entry.split_whitespace().next())
            // Local preference edits and protocol launchers do not route an engine operation.
            .filter(|name| !matches!(*name, "config" | "mcp" | "lsp" | "help"))
    }) {
        assert!(
            metadata::cli_family(name).is_some(),
            "CLI command {name} escaped family discovery"
        );
    }
    assert!(
        guide.contains("render-sites"),
        "common build entry is hidden"
    );
    assert!(
        guide.contains("advanced"),
        "advanced operations are not disclosed"
    );
    assert!(
        !guide.contains("1 advanced operations"),
        "singular advanced-operation counts must use singular grammar: {guide}"
    );
    let recovery = guide
        .lines()
        .find(|line| line.trim_start().starts_with("Recovery"))
        .expect("Recovery guide line");
    assert!(
        recovery.contains("1 advanced operation"),
        "the one-operation Recovery group must use singular grammar: {recovery}"
    );
    let preview = guide
        .lines()
        .find(|line| line.trim_start().starts_with("Preview"))
        .expect("Preview guide line");
    assert!(
        preview.contains("preview-sites"),
        "common preview entry is hidden: {preview}"
    );
    assert!(
        !preview.contains("production-preview"),
        "specialist production preview must remain in the advanced count: {preview}"
    );
    for capability in ["Build", "Workspace"] {
        let line = guide
            .lines()
            .find(|line| line.trim_start().starts_with(capability))
            .unwrap_or_else(|| panic!("missing {capability} guide line"));
        assert!(
            line.contains("check"),
            "aggregate check must be projected into its {capability} neighborhood: {line}"
        );
    }
}

#[test]
fn canonical_registry_is_exhaustive_unique_and_closed() {
    let operation_names = metadata::operations()
        .iter()
        .map(|operation| operation.name)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        operation_names.len(),
        metadata::operations().len(),
        "canonical operation names must be unique"
    );

    for operation in metadata::operations() {
        assert!(
            std::ptr::eq(metadata::operation(operation.name).unwrap(), operation),
            "canonical lookup must retain registry identity"
        );
        let hyphenated = operation.name.replace('_', "-");
        assert!(
            std::ptr::eq(metadata::operation(&hyphenated).unwrap(), operation),
            "hyphen normalization must not create a second metadata owner"
        );
        for relation in operation.relations {
            assert!(
                operation_names.contains(relation.operation),
                "{} relates to absent {}",
                operation.name,
                relation.operation
            );
        }
        if let Some(owner) = operation.terminal_result_owner {
            assert!(
                operation_names.contains(owner),
                "{} names absent terminal owner {owner}",
                operation.name
            );
        }
        assert!(
            operation
                .potential_effects
                .windows(2)
                .all(|pair| pair[0] < pair[1]),
            "{} potential effects must be unique and canonically ordered",
            operation.name
        );
        let network = operation
            .potential_effects
            .contains(&PotentialEffect::Network);
        let local_effect = operation.potential_effects.iter().any(|effect| {
            !matches!(
                effect,
                PotentialEffect::FilesystemRead | PotentialEffect::Network
            )
        });
        let mutable_state = operation.potential_effects.iter().any(|effect| {
            matches!(
                effect,
                PotentialEffect::MemoryState
                    | PotentialEffect::FilesystemWrite
                    | PotentialEffect::Sqlite
                    | PotentialEffect::Git
                    | PotentialEffect::Listener
            )
        });
        match operation.effect {
            Effect::Read => assert!(
                !network && !mutable_state,
                "{} read profile cannot request mutable or remote effects",
                operation.name
            ),
            Effect::Local => assert!(
                local_effect && !network,
                "{} local profile requires a local effect and no network",
                operation.name
            ),
            Effect::External => assert!(
                network,
                "{} external profile must name actual network I/O",
                operation.name
            ),
        }
        match operation.closure {
            Closure::None => {
                assert!(operation.relations.is_empty());
                assert!(operation.terminal_result_owner.is_none());
                assert!(!matches!(
                    operation.intent,
                    Intent::Act | Intent::Reconcile | Intent::Recover
                ));
            }
            Closure::Related => {
                assert!(!operation.relations.is_empty());
                assert!(operation.terminal_result_owner.is_none());
            }
            Closure::TerminalOwner => {
                let owner = operation
                    .terminal_result_owner
                    .expect("held lifecycle names its closing owner");
                assert_ne!(owner, operation.name);
                assert_eq!(
                    metadata::operation(owner)
                        .expect("canonical closing owner")
                        .closure,
                    Closure::SelfTerminal
                );
                assert!(operation.relations.iter().all(|relation| matches!(
                    relation.kind,
                    RelationKind::Inspect | RelationKind::Contrast
                )));
            }
            Closure::SelfTerminal => {
                assert!(operation.relations.is_empty());
                assert!(operation.terminal_result_owner.is_none());
                assert!(matches!(
                    operation.intent,
                    Intent::Act | Intent::Reconcile | Intent::Recover
                ));
            }
        }
    }

    let mut commands = BTreeSet::new();
    for family in metadata::cli_families() {
        assert!(commands.insert(family.command), "duplicate CLI family");
        assert!(
            std::ptr::eq(metadata::cli_family(family.command).unwrap(), family),
            "CLI family lookup must retain registry identity"
        );
        assert!(!family.modes.is_empty(), "{} has no modes", family.command);
        for mode in family.modes {
            assert!(
                !mode.operations.is_empty(),
                "{} has an empty mode",
                family.command
            );
            for operation in mode.operations {
                assert!(
                    operation_names.contains(operation),
                    "{} maps to absent {operation}",
                    family.command
                );
            }
        }
    }
}

#[test]
fn canonical_effect_facets_distinguish_material_native_paths() {
    use PotentialEffect::{
        FilesystemRead, FilesystemWrite, Git, Listener, MemoryState, Network, Process, Sqlite,
    };

    let profile = |name| {
        let operation = metadata::operation(name).expect("registered operation");
        (operation.effect, operation.potential_effects)
    };
    assert_eq!(
        profile("assemble_docs"),
        (
            Effect::Local,
            [FilesystemRead, FilesystemWrite, Sqlite, Process].as_slice()
        )
    );
    assert_eq!(
        profile("load_workspace"),
        (Effect::Read, [FilesystemRead].as_slice())
    );
    assert_eq!(
        profile("start_preview"),
        (
            Effect::Local,
            [MemoryState, FilesystemRead, Process, Listener].as_slice()
        )
    );
    assert_eq!(
        profile("inspect_store"),
        (Effect::Local, [FilesystemRead, Sqlite].as_slice())
    );
    assert_eq!(
        profile("prepare_release_commit"),
        (
            Effect::Local,
            [FilesystemRead, FilesystemWrite, Git].as_slice()
        )
    );
    assert_eq!(
        profile("observe_github_pages_build"),
        (
            Effect::External,
            [FilesystemRead, Process, Network].as_slice()
        )
    );
    assert_eq!(
        profile("authoring_diagnostics"),
        (Effect::Read, &[] as &[PotentialEffect])
    );
    assert_eq!(
        profile("authoring_change_document"),
        (Effect::Local, [MemoryState].as_slice())
    );
}

#[test]
fn aggregate_check_is_one_typed_multi_capability_family() {
    let family = metadata::cli_family("check").expect("check family");
    let capabilities = family
        .operations()
        .map(|name| metadata::operation(name).unwrap().capability.key())
        .collect::<BTreeSet<_>>();
    assert_eq!(capabilities, BTreeSet::from(["build", "workspace"]));
    assert!(metadata::operation("check").is_none());
    assert_eq!(metadata::cli_family("check"), Some(family));
}
