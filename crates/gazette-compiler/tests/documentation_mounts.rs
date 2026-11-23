//! Theme documentation mounts resolve independently from projects and Mintlify.

use gazette_compiler::{toml, workspace};

const BASE: &str = "[[principals]]\nid='person:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='person:one'\ndeployable=false\n[[sources]]\nid='source:manual'\nlocation={kind='filesystem',root='/unread/manual'}\n";
const MOUNT: &str = "[[documentation.mounts]]\nsite='site:one'\nlanding='content:landing'\nname='Manual'\nsource='source:manual'\nroute_prefix='/manual/'\nrender=false\ndiscoverable=false\n";

#[test]
fn disabled_mount_is_inspectable_without_native_binding_or_invented_state() {
    let source = format!("{BASE}{MOUNT}");
    let compiled =
        workspace::compile(&source, Default::default()).expect("structurally bound disabled mount");
    let mount = &compiled.workspace.documentation_mounts()[0].declaration;
    assert_eq!(mount.name, "Manual");
    assert_eq!(mount.landing.as_str(), "content:landing");
    assert!(mount.lowering.is_none());
    assert_eq!(compiled.workspace.content_bindings().count(), 0);
    assert_eq!(compiled.workspace.project_declarations().len(), 0);
    assert!(
        compiled
            .workspace
            .documentation_repository(&"site:one".parse().expect("site key"))
            .is_none()
    );
    let parsed = workspace::ParsedWorkspace::parse(&source, Default::default())
        .expect("same typed editor source");
    for (anchor, offset, expected) in [
        ("site='site:one'", 6, workspace::DeclarationKind::Site),
        (
            "source='source:manual'",
            8,
            workspace::DeclarationKind::Source,
        ),
    ] {
        let position = source.rfind(anchor).expect("explicit mount reference") + offset;
        assert_eq!(
            parsed
                .navigation()
                .at(position)
                .expect("native association")
                .symbol
                .declaration()
                .expect("Key declaration")
                .kind,
            expected
        );
    }
}

#[test]
fn enabled_mount_reuses_native_binding_and_requires_explicit_lowering() {
    let enabled = MOUNT.replace("render=false", "render=true");
    let failure = workspace::compile(&format!("{BASE}{enabled}"), Default::default())
        .expect_err("no guessed published state");
    let workspace::Error::Invalid(errors) = failure else {
        panic!("semantic lowering failure");
    };
    assert!(
        errors
            .iter()
            .any(|error| error.code == "workspace_documentation_mount_lowering")
    );
    let enabled = format!("{enabled}lowering={{lifecycle='archived',language='en-US'}}\n");
    let compiled = workspace::compile(&format!("{BASE}{enabled}"), Default::default())
        .expect("explicit lowering");
    let binding = compiled
        .workspace
        .content_bindings()
        .next()
        .expect("one rendered source");
    assert!(binding.project.is_none());
    assert!(binding.is_documentation());
    assert_eq!(binding.source_key().as_str(), "source:manual");
    assert_eq!(
        binding.native_markdown.expect("native profile").lifecycle,
        gazette_core::publication::ContentLifecycle::Archived
    );
    assert!(
        binding
            .tree_binding()
            .expect("shared binding")
            .target
            .as_str()
            .starts_with("__documentation/")
    );
}

#[test]
fn disabled_mounts_still_validate_reference_order_and_selection_constraints() {
    for (mount, code) in [
        (
            MOUNT.replace("site:one", "site:missing"),
            "workspace_reference",
        ),
        (
            MOUNT.replace("source:manual", "source:missing"),
            "workspace_reference",
        ),
        (
            MOUNT.replace("name='Manual'", "name=''"),
            "workspace_documentation_mount_name",
        ),
        (
            MOUNT.replace("'/manual/'", "'/manual'"),
            "workspace_native_route",
        ),
        (
            MOUNT.replace("discoverable=false", "discoverable=true"),
            "workspace_documentation_mount_discovery",
        ),
        (
            format!("{MOUNT}{MOUNT}"),
            "workspace_documentation_mount_duplicate",
        ),
    ] {
        let error = workspace::compile(&format!("{BASE}{mount}"), toml::Limits::default())
            .expect_err("no unused unchecked config");
        let workspace::Error::Invalid(errors) = error else {
            panic!("expected exact semantic constraint");
        };
        assert!(errors.iter().any(|error| error.code == code), "{errors:?}");
    }
}
