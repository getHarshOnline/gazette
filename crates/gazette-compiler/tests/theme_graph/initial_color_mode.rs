//! The starting colour mode inherits from the nearest declaring package.

use gazette_compiler::theme_package::{Error as ManifestError, ParsedManifest};
use gazette_core::markdown::InitialColorMode;

use super::{EffectiveTheme, Error, Graph, id, parse};

fn source(package: &str, mode: Option<&str>, dependencies: &[&str]) -> String {
    let namespace = package.trim_start_matches("theme:");
    let mode = mode
        .map(|mode| format!("initial_color_mode='{mode}'\n"))
        .unwrap_or_default();
    let dependencies = dependencies
        .iter()
        .map(|package| format!("[[dependencies]]\npackage='{package}'\n"))
        .collect::<String>();
    format!(
        "package='{package}'\nnamespace='{namespace}'\ntemplates='templates'\nrender={{page='page.html'}}\n{mode}{dependencies}"
    )
}

#[test]
fn initial_color_mode_defaults_to_system_and_follows_the_nearest_declaration() {
    for (base, world, expected) in [
        (None, None, InitialColorMode::System),
        (Some("system"), None, InitialColorMode::System),
        (Some("light"), None, InitialColorMode::Light),
        (Some("system"), Some("dark"), InitialColorMode::Dark),
    ] {
        let base_source = source("theme:base", base, &[]);
        let base = parse(&base_source);
        let world_source = source("theme:world", world, &["theme:base"]);
        let world = parse(&world_source);
        let graph =
            Graph::resolve(&id("theme:world"), [base.manifest(), world.manifest()]).unwrap();
        let effective = EffectiveTheme::resolve(&graph).unwrap();
        assert_eq!(effective.initial_color_mode(), expected);
    }
}

#[test]
fn independent_declarations_need_the_dependent_package_to_choose() {
    let light_source = source("theme:light", Some("light"), &[]);
    let light = parse(&light_source);
    let dark_source = source("theme:dark", Some("dark"), &[]);
    let dark = parse(&dark_source);
    let root_source = source("theme:root", None, &["theme:light", "theme:dark"]);
    let root = parse(&root_source);
    let graph = Graph::resolve(
        &id("theme:root"),
        [light.manifest(), dark.manifest(), root.manifest()],
    )
    .unwrap();
    assert!(matches!(
        EffectiveTheme::resolve(&graph),
        Err(Error::RenderCollision { entry, .. }) if entry == "initial_color_mode"
    ));
    let chosen_source = source("theme:root", Some("light"), &["theme:light", "theme:dark"]);
    let chosen = parse(&chosen_source);
    let graph = Graph::resolve(
        &id("theme:root"),
        [light.manifest(), dark.manifest(), chosen.manifest()],
    )
    .unwrap();
    assert_eq!(
        EffectiveTheme::resolve(&graph)
            .unwrap()
            .initial_color_mode(),
        InitialColorMode::Light
    );
}

#[test]
fn only_the_three_named_modes_are_admitted() {
    for mode in ["System", "auto", "", "dark light"] {
        let manifest = source("theme:base", Some(mode), &[]);
        assert!(
            matches!(
                ParsedManifest::parse(&manifest, Default::default()),
                Err(ManifestError::Syntax(_))
            ),
            "{mode}"
        );
    }
}
