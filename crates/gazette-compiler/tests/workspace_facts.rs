//! Explicit fact sources preserve local values and refuse unresolved publication meaning.

use gazette_compiler::{toml, workspace};
use gazette_core::publication::{
    content,
    facts::{Source, Target},
};

const LOCAL: &str = "[[principals]]\nid='owner:one'\nkind='person'\nname='Original name'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n";
const EXTERNAL: &str = "[[fact_sources]]\ntarget={kind='principal-name',principal='owner:one'}\nsource={kind='external',binding='input:weave'}\n";
const LOCAL_SELECTION: &str = "[[fact_sources]]\ntarget={kind='principal-name',principal='owner:one'}\nsource={kind='local'}\n";

fn compile(source: &str) -> workspace::Compilation {
    workspace::compile(source, toml::Limits::default()).expect("structural compilation")
}

fn empty_content() -> content::Declaration {
    content::Declaration {
        browsing: vec![],
        nodes: vec![],
        memberships: vec![],
        related: vec![],
        projects: vec![],
    }
}

#[test]
fn omitted_empty_and_explicit_local_selectors_use_the_existing_value() {
    let original = compile(LOCAL);
    let empty = compile(&format!("fact_sources=[]\n{LOCAL}"));
    let explicit = compile(&format!("{LOCAL}{LOCAL_SELECTION}"));
    assert_eq!(original.workspace, empty.workspace);
    assert_eq!(
        original.workspace.principals().next(),
        explicit.workspace.principals().next()
    );
    assert!(original.workspace.fact_sources().is_empty());
    assert_eq!(explicit.workspace.fact_sources()[0].source, Source::Local);
    for compilation in [&original, &empty, &explicit] {
        compilation
            .workspace
            .require_resolved_facts()
            .expect("local facts");
        compilation
            .resolve_content(empty_content(), content::Limits::default())
            .expect("local semantic consumer");
    }
}

#[test]
fn unresolved_selection_is_inspectable_but_cannot_become_publication_meaning() {
    let config = format!("{}{EXTERNAL}", LOCAL.replace("name='Original name'\n", ""));
    let parsed = workspace::ParsedWorkspace::parse(&config, toml::Limits::default())
        .expect("editor structural inspection");
    let compilation = parsed.compilation();
    let error = compilation
        .workspace
        .require_resolved_facts()
        .expect_err("no resolver implemented");
    assert_eq!(error.code(), "publication_facts_unavailable");
    assert_eq!(error.binding.as_str(), "input:weave");
    assert_eq!(
        error.target,
        Target::PrincipalName {
            principal: "owner:one".parse().expect("key")
        }
    );
    assert!(
        compilation
            .workspace
            .principals()
            .next()
            .expect("principal")
            .name
            .is_none()
    );
    let projected = serde_json::to_value(&compilation.workspace).expect("inspection");
    assert_eq!(
        projected["fact_sources"][0]["source"]["binding"],
        "input:weave"
    );
    let content::Error::Invalid(errors) = compilation
        .resolve_content(empty_content(), content::Limits::default())
        .expect_err("no unresolved registry/render input")
    else {
        panic!("typed semantic diagnostic")
    };
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, error.code());
    assert_eq!(errors[0].source, *error.target.principal());
    assert_eq!(errors[0].target, Some(error.binding));
}

#[test]
fn conflicts_missing_local_values_and_wrong_subjects_are_rejected() {
    for (config, expected) in [
        (
            format!("{LOCAL}{EXTERNAL}"),
            "workspace_fact_source_conflict",
        ),
        (
            format!(
                "{}{LOCAL_SELECTION}",
                LOCAL.replace("name='Original name'\n", "")
            ),
            "workspace_fact_source_local_missing",
        ),
        (
            format!(
                "{LOCAL}{}",
                LOCAL_SELECTION.replace("principal='owner:one'", "principal='site:one'")
            ),
            "workspace_fact_source_principal",
        ),
        (
            format!("{LOCAL}{LOCAL_SELECTION}{LOCAL_SELECTION}"),
            "workspace_fact_source_duplicate",
        ),
        (
            format!("{LOCAL}{LOCAL_SELECTION}{EXTERNAL}"),
            "workspace_fact_source_duplicate",
        ),
    ] {
        let workspace::Error::Invalid(errors) =
            workspace::compile(&config, toml::Limits::default()).expect_err("invalid selection")
        else {
            panic!("semantic diagnostics")
        };
        assert!(
            errors.iter().any(|error| error.code == expected),
            "{expected}: {errors:?}"
        );
    }
}

#[test]
fn unknown_fields_cannot_smuggle_literals_provider_objects_or_authority() {
    let unnamed = LOCAL.replace("name='Original name'\n", "");
    for selection in [
        EXTERNAL.replace("principal-name", "principal-kind"),
        EXTERNAL.replace(
            "binding='input:weave'",
            "binding='input:weave',name='hidden fallback'",
        ),
        EXTERNAL.replace(
            "binding='input:weave'",
            "binding='input:weave',snapshot={revision='1'}",
        ),
        EXTERNAL.replace("principal='owner:one'", "principal='owner:one',rights=true"),
    ] {
        assert!(matches!(
            workspace::compile(&format!("{unnamed}{selection}"), toml::Limits::default()),
            Err(workspace::Error::Toml(_))
        ));
    }
}
