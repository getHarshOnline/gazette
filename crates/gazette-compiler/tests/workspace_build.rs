//! Strict configured build intent and explicit override ownership.

use gazette_compiler::workspace::{self, BuildOrigin, LocalDrafts};
use gazette_core::source::EntryPath;

const OWNERS: &str = "[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n[[sites]]\nid='site:two'\npublisher='owner:one'\ndeployable=false\n";

#[test]
fn configured_defaults_preserve_order_and_explicit_override_is_not_a_fallback() {
    let input = format!("[build]\nsites=['site:two','site:one']\nlocal_drafts='include'\n{OWNERS}");
    let compilation = workspace::compile(&input, Default::default()).unwrap();
    let selected = compilation.workspace.build_selection(None).unwrap();
    assert_eq!(selected.origin, BuildOrigin::Workspace);
    assert_eq!(selected.local_drafts, Some(LocalDrafts::Include));
    assert_eq!(
        selected
            .sites
            .iter()
            .map(|site| site.as_str())
            .collect::<Vec<_>>(),
        ["site:two", "site:one"]
    );
    let override_sites = ["site:one".parse().unwrap()];
    let selected = compilation
        .workspace
        .build_selection(Some(&override_sites))
        .unwrap();
    assert_eq!(selected.origin, BuildOrigin::Invocation);
    assert_eq!(selected.sites, override_sites);
    assert!(compilation.workspace.build_selection(Some(&[])).is_err());
    assert!(
        compilation
            .workspace
            .build_selection(Some(&["site:missing".parse().unwrap()]))
            .is_err()
    );
    assert!(
        compilation
            .workspace
            .build_selection(Some(&[
                override_sites[0].clone(),
                override_sites[0].clone()
            ]))
            .is_err()
    );
    let no_defaults = workspace::compile(OWNERS, Default::default()).unwrap();
    assert!(no_defaults.workspace.build_selection(None).is_err());
    assert_eq!(
        no_defaults
            .workspace
            .build_selection(Some(&override_sites))
            .unwrap()
            .local_drafts,
        None
    );
}

#[test]
fn configured_defaults_refuse_empty_duplicate_unknown_and_malformed_state() {
    for build in [
        "sites=[]\nlocal_drafts='include'",
        "sites=['site:one','site:one']\nlocal_drafts='include'",
        "sites=['site:absent']\nlocal_drafts='include'",
        "sites=['site:one']\nlocal_drafts='publish'",
        "sites=['site:one']\nlocal_drafts='include'\nextra=true",
        "sites=['site:one']",
    ] {
        let source = format!("[build]\n{build}\n{OWNERS}");
        assert!(
            workspace::compile(&source, Default::default()).is_err(),
            "{build}"
        );
    }
}

#[test]
fn modular_defaults_have_one_owner_and_generated_schema_matches_native_contract() {
    let root: EntryPath = "gazette.toml".parse().unwrap();
    let include: EntryPath = "included.toml".parse().unwrap();
    let build = "[build]\nsites=['site:one']\nlocal_drafts='exclude'\n";
    let source = format!("includes=['included.toml']\n{build}");
    let parsed = workspace::ParsedWorkspaceDocuments::parse(
        &root,
        [(&root, source.as_bytes()), (&include, OWNERS.as_bytes())],
        Default::default(),
    )
    .unwrap();
    assert_eq!(
        parsed.compilation().workspace.build().unwrap().local_drafts,
        LocalDrafts::Exclude
    );
    let duplicate = format!("{build}{OWNERS}");
    assert!(
        workspace::ParsedWorkspaceDocuments::parse(
            &root,
            [(&root, source.as_bytes()), (&include, duplicate.as_bytes())],
            Default::default()
        )
        .is_err()
    );
    let schema = gazette_compiler::contracts::workspace();
    let validator = jsonschema::validator_for(schema.as_value()).unwrap();
    assert!(
        validator.is_valid(
            &serde_json::json!({"build":{"sites":["site:one"],"local_drafts":"exclude"}})
        )
    );
    assert!(
        !validator.is_valid(
            &serde_json::json!({"build":{"sites":["site:one"],"local_drafts":"publish"}})
        )
    );
}

#[test]
fn build_limits_and_math_are_typed_intent_without_execution_permission() {
    let source = format!(
        "[build]\nsites=['site:one']\nlocal_drafts='include'\ncorpus_bytes=536870912\nmath=true\n{OWNERS}"
    );
    let compiled = workspace::compile(&source, Default::default()).unwrap();
    let build = compiled.workspace.build().unwrap();
    assert_eq!(build.corpus_bytes, Some(536870912));
    assert_eq!(build.math, Some(true));
    for replacement in ["corpus_bytes=0", "corpus_bytes=-1", "corpus_bytes='large'"] {
        assert!(
            workspace::compile(
                &source.replace("corpus_bytes=536870912", replacement),
                Default::default()
            )
            .is_err()
        );
    }
    assert!(
        workspace::compile(
            &source.replace("math=true", "math='yes'"),
            Default::default()
        )
        .is_err()
    );
    assert!(
        workspace::compile(
            &source.replace("math=true", "math=true\nallow_execute=true"),
            Default::default()
        )
        .is_err()
    );
    let schema = gazette_compiler::contracts::workspace();
    let validator = jsonschema::validator_for(schema.as_value()).unwrap();
    assert!(validator.is_valid(&serde_json::json!({"build":{"sites":["site:one"],"local_drafts":"include","corpus_bytes":536870912,"math":false}})));
    assert!(!validator.is_valid(&serde_json::json!({"build":{"sites":["site:one"],"local_drafts":"include","corpus_bytes":0}})));
}
