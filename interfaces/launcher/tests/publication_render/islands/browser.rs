//! Real Chrome DOM evidence over Gazette's own local-URL preview, never a mock DOM.

use super::*;
use crate::browser_carrier::{chrome, dump, values};
use crate::sites::{self, preview};
use gazette_sdk::html::TextKind;

const MODULE: &str = r#"
let mounts = 0;
document.documentElement.dataset.fixtureEvaluations = String(Number(document.documentElement.dataset.fixtureEvaluations || 0) + 1);
export function mountCounter({element, props}) {
  mounts += 1;
  element.dataset.fixtureMount = String(mounts);
  element.dataset.fixtureProps = JSON.stringify(props);
  const output = document.createElement('output');
  output.textContent = props.label;
  element.append(output);
}
"#;

fn fixture(module: &str, selected_export: &str) -> Fixture {
    let body = format!(
        "::::counter {{label={}}}\nOuter fallback.\n\n:::counter {{label='nested'}}\nNested fallback.\n:::\n\n<!-- more -->\n\nRemainder.\n::::\n\n:::counter {{label='sibling'}}\nSibling fallback.\n:::\n",
        serde_json::to_string(DANGEROUS).unwrap()
    );
    let manifest = MANIFEST.replace(
        "export='mountCounter'",
        &format!("export='{selected_export}'"),
    );
    let page = PAGE.replace("**Retained** body\n", &body);
    // Duplicate inclusion still names the same actual compiled resource. No test
    // bootstrap, source rewriting, DOM emulation or external module delivery.
    let template = format!(
        "{TEMPLATE}{{% if presentation.interaction is defined %}}<script type=\"module\" src=\"{{{{ presentation.interaction.url }}}}\"></script>{{% endif %}}"
    );
    Fixture::new(&[
        ("one/page.md", page.as_bytes()),
        ("theme/theme.toml", manifest.as_bytes()),
        ("theme/plugin.toml", PLUGIN.as_bytes()),
        ("theme/templates/page.html", template.as_bytes()),
        (
            "theme/templates/components/counter.html",
            COMPONENT.as_bytes(),
        ),
        ("theme/counter.ts", module.as_bytes()),
        ("theme/increment.ts", b"export const step = 1;"),
        ("theme/unused.ts", b"export function mountUnused() {}"),
    ])
}

fn preview(fixture: &Fixture) -> (preview::Process, String) {
    let config = configured(fixture, SELECTION);
    let mut process = preview::Process::with_arguments(
        &sites::request(&config, &["site:one", "site:other"]),
        &["--local-urls"],
    );
    let ready = process.event();
    assert_eq!(ready["event"], "ready");
    assert_eq!(ready["status"], "local-cross-site-noncertified");
    let address = preview::address(&ready, "site:one");
    let base = ready["sites"]["site:one"]["url"].as_str().unwrap();
    assert_eq!(base, format!("http://{address}/mount/"));
    (process, format!("{base}guide/"))
}

fn assert_fallback(dom: &str) {
    assert_eq!(dom.matches("data-gazette-fallback").count(), 3);
    let text = values(dom, TextKind::Text).join(" ");
    for value in ["Outer fallback.", "Nested fallback.", "Sibling fallback."] {
        assert!(text.contains(value), "browser lost fallback {value}");
    }
    assert!(
        values(dom, TextKind::EventAttribute).is_empty(),
        "props must not create active HTML"
    );
}

#[test]
#[ignore = "requires explicit real Chrome provider qualification"]
fn chrome_mounts_nested_retained_islands_once_and_preserves_exact_props_and_no_js_fallback() {
    let chrome = chrome();
    let fixture = fixture(MODULE, "mountCounter");
    let (mut server, url) = preview(&fixture);
    let dom = dump(&chrome, &url, true);
    assert_fallback(&dom);
    assert_eq!(
        dom.matches("data-gazette-state=\"mounted\"").count(),
        3,
        "{dom}"
    );
    assert!(
        dom.contains("data-fixture-evaluations=\"1\""),
        "one native ESM evaluation"
    );
    for number in [1, 2, 3] {
        assert_eq!(
            dom.matches(&format!("data-fixture-mount=\"{number}\""))
                .count(),
            1
        );
    }
    assert!(!dom.contains("data-fixture-mount=\"4\""));
    let attributes = values(&dom, TextKind::Attribute);
    for label in [DANGEROUS, "nested", "sibling"] {
        let expected = json!({"label":label}).to_string();
        assert!(
            attributes
                .iter()
                .filter(|value| **value == expected)
                .count()
                >= 2,
            "both source props and actual mount receipt must preserve {label:?}"
        );
    }
    assert!(
        values(&dom, TextKind::Text)
            .iter()
            .any(|value| value == DANGEROUS)
    );
    let no_js = dump(&chrome, &url, false);
    assert_fallback(&no_js);
    assert_eq!(no_js.matches("data-gazette-state=\"pending\"").count(), 3);
    assert!(!no_js.contains("data-fixture-mount"));
    assert!(!no_js.contains("data-fixture-evaluations"));
    server.close();
}

#[test]
#[ignore = "requires explicit real Chrome provider qualification"]
fn chrome_reports_missing_export_and_throwing_mount_without_removing_static_fallback() {
    let chrome = chrome();
    for (module, export, reason) in [
        (MODULE, "absentExport", "export"),
        (
            "export function mountCounter({element}) { element.textContent='partial mount'; throw new Error('fixture failure'); }",
            "mountCounter",
            "mount",
        ),
    ] {
        let fixture = fixture(module, export);
        let (mut server, url) = preview(&fixture);
        let dom = dump(&chrome, &url, true);
        assert_fallback(&dom);
        assert_eq!(
            dom.matches("data-gazette-state=\"failed\"").count(),
            3,
            "{dom}"
        );
        assert_eq!(
            dom.matches(&format!("data-gazette-failure=\"{reason}\""))
                .count(),
            3
        );
        assert!(!dom.contains("data-gazette-state=\"mounted\""));
        assert!(
            !values(&dom, TextKind::Text)
                .iter()
                .any(|value| value.contains("partial mount"))
        );
        server.close();
    }
}

#[test]
#[ignore = "requires explicit real Chrome provider qualification"]
fn chrome_mounts_notes_only_island_without_removing_article_or_commentary_fallback() {
    let chrome = chrome();
    let fixture = super::notes_fixture();
    let (mut server, guide) = preview(&fixture);
    let url = format!("{}notes/", guide.strip_suffix("guide/").unwrap());
    let dom = dump(&chrome, &url, true);
    assert_eq!(
        dom.matches("data-gazette-state=\"mounted\"").count(),
        1,
        "{dom}"
    );
    assert_eq!(dom.matches("data-gazette-fallback").count(), 1);
    assert!(
        values(&dom, TextKind::Text)
            .iter()
            .any(|value| value == "notes:1")
    );
    for text in ["Continuous main meaning.", "Supplementary fallback."] {
        assert!(values(&dom, TextKind::Text).join(" ").contains(text));
    }
    let no_js = dump(&chrome, &url, false);
    assert_eq!(no_js.matches("data-gazette-state=\"pending\"").count(), 1);
    assert!(
        !values(&no_js, TextKind::Text)
            .iter()
            .any(|value| value == "notes:1")
    );
    for text in ["Continuous main meaning.", "Supplementary fallback."] {
        assert!(values(&no_js, TextKind::Text).join(" ").contains(text));
    }
    server.close();
}
