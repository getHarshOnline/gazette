//! Retained policy input is a checked source relationship, not a copied command or approval.

use gazette_compiler::{toml, workspace};
use serde_json::json;

const CHECKER: &str =
    "policy_check={program='/not-installed/checker',directory='/not-opened/work'}\n";
const TOPOLOGY: &str = r#"
[[principals]]
id='owner:one'
kind='organization'
[[sites]]
id='site:one'
publisher='owner:one'
deployable=false
[[sites]]
id='site:unselected'
publisher='owner:one'
deployable=false
[[sources]]
id='source:policy'
location={kind='repository',root='/not-opened/policy',commit='1111111111111111111111111111111111111111',subdirectory='rules'}
"#;

fn compile(input: &str) -> Result<workspace::Compilation, workspace::Error> {
    workspace::compile(&format!("{input}\n{TOPOLOGY}"), toml::Limits::default())
}

#[test]
fn exact_policy_binding_is_optional_and_preserves_native_literal_path_and_source_owner() {
    let plain = compile("").expect("standalone workspace");
    assert!(plain.workspace.policy_source().is_none());
    assert!(
        serde_json::to_value(&plain.workspace)
            .expect("workspace projection")
            .get("policy_source")
            .is_none()
    );
    let configured = compile(&format!(
        "{CHECKER}policy_source={{source='source:policy',entry='nested/%2e%2e policy.json'}}"
    ))
    .expect("literal file declaration does not read or decode a URL");
    let selected = configured
        .workspace
        .policy_source()
        .expect("explicit binding");
    assert_eq!(selected.source.as_str(), "source:policy");
    assert_eq!(selected.entry.as_str(), "nested/%2e%2e policy.json");
    assert_eq!(
        serde_json::to_value(selected).expect("native schema"),
        json!({"source":"source:policy","entry":"nested/%2e%2e policy.json"})
    );
    assert_eq!(
        configured
            .workspace
            .source(&selected.source)
            .expect("sole native source")
            .location
            .root()
            .external(),
        Some("/not-opened/policy")
    );
    assert_eq!(
        configured.workspace.content_bindings().count(),
        0,
        "no synthetic authored placement"
    );
    assert_eq!(
        plain.workspace.sources().collect::<Vec<_>>(),
        configured.workspace.sources().collect::<Vec<_>>()
    );
}

#[test]
fn policy_source_requires_selected_checker_and_exact_declared_source() {
    for (input, code, field) in [
        (
            "policy_source={source='source:policy',entry='policy.json'}".to_owned(),
            "workspace_policy_source",
            "policy_source",
        ),
        (
            format!("{CHECKER}policy_source={{source='source:missing',entry='policy.json'}}"),
            "workspace_reference",
            "policy_source.source",
        ),
    ] {
        let workspace::Error::Invalid(errors) =
            compile(&input).expect_err("whole workspace refuses unresolved policy selection")
        else {
            panic!("semantic relationship diagnostic expected")
        };
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, code);
        assert_eq!(errors[0].field, field);
        assert!(errors[0].key.is_some());
    }
}

#[test]
fn policy_source_named_table_and_literal_entry_are_strict() {
    for selected in [
        "['source:policy','policy.json']",
        "{source='source:policy'}",
        "{entry='policy.json'}",
        "{source='source:policy',entry='policy.json',unknown=true}",
        "{source='source:policy',source='source:policy',entry='policy.json'}",
        "{source='source:policy',entry='../policy.json'}",
        "{source='source:policy',entry='/policy.json'}",
        "{source='source:policy',entry=''}",
        "{source='source:policy',entry=['policy.json']}",
        "{source='source:policy',entry=42}",
        "{source='not-a-key',entry='policy.json'}",
    ] {
        assert!(
            matches!(
                compile(&format!("{CHECKER}policy_source={selected}")),
                Err(workspace::Error::Toml(_))
            ),
            "closed source table: {selected}"
        );
    }
}
