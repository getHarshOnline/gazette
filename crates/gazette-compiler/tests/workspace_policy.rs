//! Optional command selection is compiled configuration, not execution or authority.

use gazette_compiler::workspace::RuntimeCommand as Command;
use gazette_compiler::{toml, workspace};
use std::collections::BTreeMap;

const TOPOLOGY: &str = r#"
[[principals]]
id = 'owner:publisher'
kind = 'person'
[[sites]]
id = 'site:one'
publisher = 'owner:publisher'
deployable = false
[[sites]]
id = 'site:two'
publisher = 'owner:publisher'
deployable = false
[[projects]]
id = 'project:one'
lifecycle = 'done'
visibility = 'unlisted'
availability = 'gated'
[[placements]]
role = 'primary'
site = 'site:one'
project = 'project:one'
render = true
discoverable = false
documents = { source = 'source:one', target = 'docs' }
[[sources]]
id = 'source:one'
[sources.location]
kind = 'repository'
root = '/not-read-project'
commit = '0000000000000000000000000000000000000000'
subdirectory = 'docs'
"#;

fn compile(policy: &str) -> Result<workspace::Compilation, workspace::Error> {
    workspace::compile(
        &format!("{policy}\n{TOPOLOGY}"),
        toml::Limits {
            // Reach the independent 64 KiB command ceiling rather than stopping
            // at the whole TOML source ceiling in the command-boundary fixture.
            source_bytes: 128 << 10,
            ..toml::Limits::default()
        },
    )
}

#[test]
fn omitted_checker_preserves_standalone_workspace_and_explicit_empty_defaults() {
    let plain = compile("").expect("standalone workspace needs no checker");
    assert!(plain.workspace.policy_check().is_none());
    let configured = compile("[policy_check]\nprogram='/not-installed'\ndirectory='/not-existing'")
        .expect("compilation does not inspect or execute the native command");
    let command = configured
        .workspace
        .policy_check()
        .expect("explicit checker");
    assert!(command.arguments.is_empty());
    assert!(command.environment.is_empty());
    assert_ne!(plain.source_sha256, configured.source_sha256);
}

#[test]
fn command_is_retained_exactly_without_role_state_or_site_selection_inheritance() {
    let source = r#"
[policy_check]
program = '/private/policy checker'
directory = '/private/instance'
arguments = ['', '$HOME; $(ignored)', 'é']
environment = { _MODE = 'review', NAME2 = '$VALUE=literal' }
"#;
    let baseline = compile("").expect("baseline");
    let configured = compile(source).expect("exact native command declaration");
    assert_eq!(
        configured.workspace.policy_check(),
        Some(&Command {
            program: "/private/policy checker".into(),
            executable: None,
            directory: "/private/instance".into(),
            arguments: vec!["".into(), "$HOME; $(ignored)".into(), "é".into()],
            environment: BTreeMap::from([
                ("_MODE".into(), "review".into()),
                ("NAME2".into(), "$VALUE=literal".into())
            ]),
        })
    );
    assert_eq!(
        baseline.workspace.sites().collect::<Vec<_>>(),
        configured.workspace.sites().collect::<Vec<_>>()
    );
    assert_eq!(
        baseline.workspace.principals().collect::<Vec<_>>(),
        configured.workspace.principals().collect::<Vec<_>>()
    );
    assert_eq!(
        baseline.workspace.projects().collect::<Vec<_>>(),
        configured.workspace.projects().collect::<Vec<_>>()
    );
    assert_eq!(
        baseline.workspace.placements(),
        configured.workspace.placements()
    );
    for site in configured.workspace.sites() {
        assert_eq!(
            baseline.workspace.composition_for(&site.id),
            configured.workspace.composition_for(&site.id)
        );
    }
    assert_eq!(
        compile(source).expect("repeat resolution").workspace,
        configured.workspace
    );
}

#[test]
fn malformed_or_unknown_command_fields_reject_the_complete_workspace() {
    for policy in [
        "policy_check='checker'",
        "[policy_check]\nprogram='/checker'",
        "[policy_check]\nprogram='/checker'\ndirectory='/work'\nunknown=true",
        "[policy_check]\nprogram='/checker'\ndirectory='/work'\narguments='one string'",
        "[policy_check]\nprogram='/checker'\ndirectory='/work'\nenvironment={A=42}",
        "[policy_check]\nprogram='/one'\nprogram='/two'\ndirectory='/work'",
    ] {
        assert!(
            matches!(compile(policy), Err(workspace::Error::Toml(_))),
            "strict native TOML shape: {policy}"
        );
    }
}

#[test]
fn invalid_command_spelling_and_bounds_have_one_native_semantic_diagnostic() {
    let oversized = format!(
        "[policy_check]\nprogram='/checker'\ndirectory='/work'\narguments=['{}']",
        "x".repeat(64 << 10)
    );
    for policy in [
        "[policy_check]\nprogram='../checker'\ndirectory='/work'",
        "[policy_check]\nprogram='/checker'\ndirectory='../relative'",
        "[policy_check]\nprogram='/checker'\ndirectory='/work'\nenvironment={'1NAME'='value'}",
        "[policy_check]\nprogram='/checker'\ndirectory='/work'\narguments=[\"private\\u0000value\"]",
        oversized.as_str(),
    ] {
        let workspace::Error::Invalid(errors) =
            compile(policy).expect_err("invalid command rejects the whole compilation")
        else {
            panic!("expected native command semantic validation");
        };
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "workspace_policy_command");
        assert_eq!(errors[0].field, "policy_check");
        assert!(errors[0].key.is_none());
    }
}

#[test]
fn executable_pin_reaches_the_single_native_command_without_native_access() {
    let digest = gazette_core::source::Sha256Digest::of(b"declared bytes");
    let policy = format!(
        "[policy_check]\nprogram='/not-installed'\ndirectory='/not-existing'\nexecutable={{bytes=14,sha256='{digest}'}}"
    );
    let configured = compile(&policy).expect("pure declared pin");
    let pin = configured
        .workspace
        .policy_check()
        .expect("selected checker")
        .executable
        .expect("selected pin");
    assert_eq!(pin.bytes, 14);
    assert_eq!(pin.sha256, digest);
    for length in ["0", "268435457"] {
        assert!(matches!(
            compile(&policy.replace("bytes=14", &format!("bytes={length}"))),
            Err(workspace::Error::Invalid(_))
        ));
    }
    assert!(matches!(
        compile(&policy.replace("bytes=14", "bytes=14,unexpected=true")),
        Err(workspace::Error::Toml(_))
    ));
}

#[test]
fn output_transport_is_explicit_closed_and_requires_a_checker() {
    use workspace::PolicyOutputTransport;
    assert_eq!(
        compile("").unwrap().workspace.policy_output_transport(),
        PolicyOutputTransport::InlineV1
    );
    let checker = "policy_check={program='/checker',directory='/'}";
    for (value, expected) in [
        ("inline-v1", PolicyOutputTransport::InlineV1),
        ("framed-v2", PolicyOutputTransport::FramedV2),
    ] {
        let parsed = compile(&format!("policy_output_transport='{value}'\n{checker}")).unwrap();
        assert_eq!(parsed.workspace.policy_output_transport(), expected);
    }
    assert!(compile("policy_output_transport='framed-v2'").is_err());
    assert!(compile(&format!("policy_output_transport='automatic'\n{checker}")).is_err());
}
