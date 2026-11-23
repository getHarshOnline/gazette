//! Text assets configured as `.txt` become hidden pages; other assets stay files.

use super::*;

const START: &[u8] = b"# Start\n\n[Licence](../LICENSE)\n\n[Agents](../AGENTS.md)\n\n[Bill of materials](../SBOM.spdx.json)\n\n[Readme](../notes/readme.txt)\n\n![Diagram](media/diagram.png)\n";
const AGENTS: &[u8] = b"# Agents\n\n```sh\nrun {x} <Tag/>\n```\n";

fn text_tree(start: &[u8], licence: &[u8]) -> CapturedSource {
    retained(&[
        ("docs/start.md", start),
        ("docs/guide.md", b"# Guide\n"),
        ("docs/media/diagram.png", b"retained-image"),
        ("LICENSE", licence),
        ("AGENTS.md", AGENTS),
        ("SBOM.spdx.json", b"{\"a\":1}"),
        ("notes/readme.txt", b"Plain notes.\n"),
    ])
}

fn text_project(tree: &CapturedSource) -> ProjectModule<'_> {
    let mut module = project(tree);
    let manual = module.manual.as_mut().unwrap();
    for (input, output) in [
        ("LICENSE", "gazette/source/LICENSE.txt"),
        ("AGENTS.md", "gazette/source/AGENTS.md.txt"),
        ("SBOM.spdx.json", "gazette/source/SBOM.spdx.json.txt"),
    ] {
        manual.assets.push(ManualAsset {
            selection: Box::leak(Box::new(selection(input))),
            path: Box::leak(Box::new(output.parse().unwrap())),
        });
    }
    manual.assets.push(ManualAsset {
        selection: Box::leak(Box::new(Selection::Directory {
            path: Some("notes".parse().unwrap()),
        })),
        path: Box::leak(Box::new("gazette/notes".parse().unwrap())),
    });
    module
}

fn text(output: &CapturedSource, path: &str) -> String {
    String::from_utf8(output.file(&entry(path)).expect(path).to_vec()).unwrap()
}

#[test]
fn text_assets_become_hidden_noindex_pages_and_links_follow_them() {
    let tree = text_tree(START, b"MIT\n");
    let output = assemble(&[text_project(&tree)]).unwrap();
    assert_eq!(
        text(&output, "gazette/source/LICENSE.md"),
        "---\ntitle: \"LICENSE\"\nnoindex: true\n---\n\n```text\nMIT\n```\n"
    );
    // The fence outgrows the file's own fence, so nothing inside is read as markup.
    assert_eq!(
        text(&output, "gazette/source/AGENTS-md.md"),
        "---\ntitle: \"AGENTS.md\"\nnoindex: true\n---\n\n````md\n# Agents\n\n```sh\nrun {x} <Tag/>\n```\n````\n"
    );
    assert_eq!(
        text(&output, "gazette/source/SBOM-spdx-json.md"),
        "---\ntitle: \"SBOM.spdx.json\"\nnoindex: true\n---\n\n```json\n{\"a\":1}\n```\n"
    );
    assert_eq!(
        text(&output, "gazette/notes/readme.md"),
        "---\ntitle: \"notes/readme.txt\"\nnoindex: true\n---\n\n```txt\nPlain notes.\n```\n"
    );
    assert!(
        output
            .inventory()
            .entries()
            .iter()
            .all(|entry| !entry.path.as_str().ends_with(".txt")),
        "no Enterprise-only static text file is emitted"
    );
    assert_eq!(
        output.file(&entry("assets/diagram.png")),
        Some(b"retained-image".as_slice())
    );
    let start = text(&output, "gazette/start.md");
    for route in [
        "/gazette/source/LICENSE)",
        "/gazette/source/AGENTS-md)",
        "/gazette/source/SBOM-spdx-json)",
        "/gazette/notes/readme)",
    ] {
        assert!(start.contains(route), "{route}: {start}");
    }
    let docs = text(&output, "docs.json");
    assert!(!docs.contains("gazette/source/") && !docs.contains("gazette/notes/"));
}

#[test]
fn hidden_text_pages_stay_out_of_the_catalog() {
    let tree = text_tree(START, b"MIT\n");
    let assembled = Mintlify::new(&gazette_adapter_markdown::Comrak)
        .assemble_with_catalog(
            Request {
                name: "Product documentation",
                public_base_url: None,
                target: &Target::Mintlify {
                    theme: "mint".into(),
                    primary_color: "#123abc".into(),
                    light_color: None,
                    dark_color: None,
                    telemetry: None,
                },
                projects: &[text_project(&tree)],
                usage: None,
            },
            Limits::default(),
            gazette_core::documentation::CatalogLimits::default(),
        )
        .unwrap();
    let routes: Vec<_> = assembled
        .catalog
        .unwrap()
        .entries()
        .iter()
        .map(|entry| entry.route.as_str().to_owned())
        .collect();
    assert!(routes.contains(&"gazette/start".to_owned()), "{routes:?}");
    assert!(
        routes.iter().all(
            |route| !route.starts_with("gazette/source") && !route.starts_with("gazette/notes")
        ),
        "{routes:?}"
    );
}

#[test]
fn text_pages_refuse_fragments_and_bytes_that_are_not_text() {
    let fragment = String::from_utf8(START.to_vec())
        .unwrap()
        .replace("../LICENSE)", "../LICENSE#terms)");
    let error = assemble(&[text_project(&text_tree(fragment.as_bytes(), b"MIT\n"))])
        .unwrap_err()
        .to_string();
    assert!(error.contains("accepts no fragment"), "{error}");
    for licence in [b"\xff\xfe".as_slice(), b"MIT\0".as_slice()] {
        let error = assemble(&[text_project(&text_tree(START, licence))])
            .unwrap_err()
            .to_string();
        assert!(error.contains("UTF-8 without NUL"), "{error}");
    }
}
