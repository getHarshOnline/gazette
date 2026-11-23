//! Workspace capture programs remain explicit, independently pinned and singular across documents.

use super::{PRINCIPAL, SITE, compile, toml, workspace};
use gazette_core::source::{EntryPath, Sha256Digest};

fn selection() -> String {
    let node = format!(
        "{{program='/opt/gazette/node',executable={{bytes=10,sha256='{}'}}}}",
        Sha256Digest::of(b"node bytes")
    );
    let browser = format!(
        "{{program='/opt/gazette/browser',executable={{bytes=13,sha256='{}'}}}}",
        Sha256Digest::of(b"browser bytes")
    );
    format!("social_card_renderer={{node={node},browser={browser}}}\n")
}

#[test]
fn social_card_renderer_retains_both_pins_without_implicit_sources_or_commands() {
    let selected = selection();
    let compiled = compile(&format!("{selected}{PRINCIPAL}{SITE}")).unwrap();
    let renderer = compiled.workspace.social_card_renderer().unwrap();
    assert_eq!(renderer.node.program, "/opt/gazette/node");
    assert_eq!(renderer.node.executable.bytes, 10);
    assert_eq!(
        renderer.node.executable.sha256,
        Sha256Digest::of(b"node bytes")
    );
    assert_eq!(renderer.browser.program, "/opt/gazette/browser");
    assert_eq!(renderer.browser.executable.bytes, 13);
    assert_eq!(
        renderer.browser.executable.sha256,
        Sha256Digest::of(b"browser bytes")
    );
    assert_eq!(compiled.workspace.sources().count(), 0);
    let absent = compile(&format!("{PRINCIPAL}{SITE}")).unwrap();
    assert!(absent.workspace.social_card_renderer().is_none());
    assert!(
        serde_json::to_value(&absent.workspace)
            .unwrap()
            .get("social_card_renderer")
            .is_none()
    );
}

#[test]
fn social_card_renderer_checks_each_path_and_pin_through_the_native_contract() {
    for (from, to, field) in [
        ("/opt/gazette/node", "node", "node"),
        ("/opt/gazette/browser", "browser", "browser"),
        ("bytes=10", "bytes=0", "node"),
        ("bytes=13", "bytes=268435457", "browser"),
    ] {
        let selected = selection().replace(from, to);
        match compile(&format!("{selected}{PRINCIPAL}{SITE}")) {
            Err(workspace::Error::Invalid(errors)) => {
                assert_eq!(errors.len(), 1);
                assert_eq!(errors[0].code, "workspace_social_card_command");
                assert_eq!(errors[0].field, format!("social_card_renderer.{field}"));
            }
            _ => panic!("expected native pin/path refusal for {field}"),
        }
    }
}

#[test]
fn social_card_renderer_rejects_missing_pins_and_caller_execution_settings() {
    let selected = selection();
    for invalid in [
        selected.replace("node={program=", "node={arguments=['--eval'],program="),
        selected.replace(
            "browser={program=",
            "browser={environment={HOME='/ambient'},program=",
        ),
        selected.replace(
            "browser={program=",
            "browser={directory='/ambient',program=",
        ),
        selected.replace(
            "social_card_renderer={",
            "social_card_renderer={source='source:helper',",
        ),
        selected.replace(
            "social_card_renderer={",
            "social_card_renderer={entry='helper.mjs',",
        ),
        selected.replace(
            "social_card_renderer={",
            "social_card_renderer={arguments=[],",
        ),
        "social_card_renderer={node={program='/node'},browser={program='/browser'}}\n".to_owned(),
        "social_card_renderer={node={program='/node',executable={bytes=1}}}\n".to_owned(),
        "social_card_renderer=[]\n".to_owned(),
        selected.replace(
            &format!(
                "node={{program='/opt/gazette/node',executable={{bytes=10,sha256='{}'}}}}",
                Sha256Digest::of(b"node bytes")
            ),
            &format!(
                "node=['/opt/gazette/node',{{bytes=10,sha256='{}'}}]",
                Sha256Digest::of(b"node bytes")
            ),
        ),
    ] {
        assert!(
            compile(&format!("{invalid}{PRINCIPAL}{SITE}")).is_err(),
            "{invalid}"
        );
    }
}

#[test]
fn social_card_renderer_merges_once_and_duplicate_declarations_fail_even_if_equal() {
    let root: EntryPath = "gazette.toml".parse().unwrap();
    let machine: EntryPath = "capture.toml".parse().unwrap();
    let selected = selection();
    let root_source = format!("includes=['capture.toml']\n{PRINCIPAL}{SITE}");
    let compiled = workspace::compile_documents(
        &root,
        [
            (&root, root_source.as_bytes()),
            (&machine, selected.as_bytes()),
        ],
        toml::SetLimits::default(),
    )
    .unwrap();
    assert_eq!(
        compiled
            .compilation
            .workspace
            .social_card_renderer()
            .unwrap()
            .browser
            .program,
        "/opt/gazette/browser"
    );
    let duplicate = format!("{selected}{root_source}");
    match workspace::compile_documents(
        &root,
        [
            (&root, duplicate.as_bytes()),
            (&machine, selected.as_bytes()),
        ],
        toml::SetLimits::default(),
    ) {
        Err(workspace::Error::Invalid(errors)) => {
            assert_eq!(errors.len(), 1);
            assert_eq!(errors[0].code, "workspace_document_singleton_duplicate");
            assert_eq!(errors[0].field, "social_card_renderer");
        }
        _ => panic!("expected singleton capture selection refusal"),
    }
}
