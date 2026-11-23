//! WOFF2 delivery joins retained resources, stylesheet closure and output evidence.

use super::*;
use base64::Engine as _;
use gazette_sdk::build::{
    DisclosureClass, DisclosureDecoding, DisclosurePattern, DisclosurePolicy, DisclosureRule,
};

const FONT: &str = include_str!("../../../../adapters/fonts/tests/fixtures/valid-001.woff2.b64");

fn font() -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(FONT.split_whitespace().collect::<String>())
        .expect("provenance-documented WPT WOFF2 fixture")
}

fn font_resource(output: &str) -> String {
    format!(
        "[[resources]]\ninputs=['fixture.woff2']\nbuild={{kind='woff2-font',entry='fixture.woff2'}}\noutput='{output}'\n"
    )
}

fn policy() -> DisclosurePolicy {
    let classes = [
        DisclosureClass::LocalPath,
        DisclosureClass::PrivateEndpoint,
        DisclosureClass::Secret,
        DisclosureClass::GeneratedArtifact,
        DisclosureClass::FixtureClaim,
        DisclosureClass::InternalIdentifier,
    ];
    DisclosurePolicy {
        rules: classes
            .into_iter()
            .enumerate()
            .map(|(index, class)| {
                (
                    format!("rule:font-{index}").parse().expect("rule key"),
                    DisclosureRule {
                        class,
                        pattern: DisclosurePattern::Literal {
                            value: format!("not-present-private-{index}"),
                        },
                    },
                )
            })
            .collect(),
    }
}

fn render_selected(
    inputs: &build::AcquiredInputs,
) -> Result<build::PublicationRender<'_>, build::PublicationRenderError> {
    use gazette_sdk::{authored, publication::content::Disposition};

    let parsed =
        build::parse_publication(inputs, Default::default()).expect("retained font fixture syntax");
    let resolved = parsed
        .resolve(Default::default())
        .expect("complete font fixture graph");
    let selection = authored::Selection {
        content: resolved
            .authored()
            .content()
            .graph()
            .nodes()
            .map(|node| {
                let disposition =
                    if node.lifecycle == gazette_sdk::publication::ContentLifecycle::Published {
                        Disposition::Include
                    } else {
                        Disposition::Exclude
                    };
                (node.id.clone(), disposition)
            })
            .collect(),
        assets: parsed
            .corpus()
            .files()
            .iter()
            .filter(|(_, file)| {
                matches!(
                    file,
                    gazette_sdk::corpus::File::Asset(_) | gazette_sdk::corpus::File::Notebook(_)
                )
            })
            .map(|(path, _)| (path.clone(), Disposition::Include))
            .collect(),
    };
    let selected = resolved
        .into_selected(selection)
        .expect("explicit complete published font selection");
    build::render_selected_publication(
        &selected,
        build::SiteRenderOptions {
            render_year: None,
            graph: None,
            model_output: Some(&"model.json".parse().expect("model")),
            limits: source::Limits::default(),
        },
        Default::default(),
    )
}

#[test]
fn font_face_resolves_to_one_validated_exact_byte_font_and_strict_evidence() {
    let font = font();
    let resources = format!("{RESOURCES}{}", font_resource("/fonts/fixture.woff2"));
    let first =
        page("one", "/one/", &resources).replace("lifecycle='draft'", "lifecycle='published'");
    let fixture = Fixture::new(
        false,
        &[
            ("content/one.md", first.as_bytes()),
            ("content/fixture.woff2", &font),
            (
                "content/main.scss",
                b"@use 'tokens'; @font-face { font-family: 'Fixture'; src: url('../fonts/fixture.woff2') format('woff2'); } .card { color: tokens.$ink; font-family: 'Fixture'; }",
            ),
        ],
    );
    let acquired = acquire(&fixture.configuration());
    let built = render_selected(&acquired).expect("connected selected WOFF2 publication");
    let site = built.site();
    let path: source::EntryPath = "fonts/fixture.woff2".parse().expect("font output");
    assert_eq!(site.output().file(&path), Some(font.as_slice()));
    assert_eq!(
        site.media_types().get(&path),
        Some(&gazette_sdk::render::MediaType::Woff2)
    );
    let html = std::str::from_utf8(bytes(site.output(), "one/index.html")).expect("page HTML");
    assert!(
        html.contains("data-local=\"yes\" rel=\"stylesheet\"") && html.contains("z-first.css"),
        "selected page did not retain its stylesheet projection: {html}"
    );
    assert!(
        !html.contains("fixture.woff2"),
        "the stylesheet owns font loading; Theme receives inventory without inventing a font tag"
    );
    let model: Value = serde_json::from_slice(bytes(site.output(), "model.json")).expect("model");
    let resources = model["pages"]["content:one"]["presentation"]["resources"]
        .as_array()
        .expect("page resources");
    let stylesheet = resources
        .iter()
        .find(|resource| resource["kind"] == "stylesheet")
        .expect("stylesheet projection");
    assert_eq!(
        stylesheet["references"],
        json!({"../fonts/fixture.woff2":"fonts/fixture.woff2"})
    );
    let projected = resources
        .iter()
        .find(|resource| resource["kind"] == "woff2-font")
        .expect("font projection");
    assert_eq!(projected["output"], "fonts/fixture.woff2");
    assert_eq!(projected["sha256"], json!(source::Sha256Digest::of(&font)));
    let evidence = serde_json::to_value(site.evidence()).expect("private build evidence");
    let private = evidence["page_resources"]["content:one"]
        .as_array()
        .expect("private page-resource evidence")
        .iter()
        .find(|resource| resource["recipe"]["kind"] == "woff2-font")
        .expect("private font evidence");
    assert_eq!(private["recipe"]["entry"], "fixture.woff2");
    assert_eq!(
        private["dependencies"],
        json!({"fixture.woff2":source::Sha256Digest::of(&font)})
    );
    assert_eq!(private["output_sha256"], projected["sha256"]);
    assert!(
        private["tool"].as_str().is_some_and(|tool| {
            tool.contains("wuff/0.2.8") && tool.contains("read-fonts/0.39.2")
        })
    );
    let disclosure = build::check_disclosure_policy(
        site.output(),
        site.media_types(),
        &policy(),
        Default::default(),
    )
    .expect("complete WOFF2 disclosure decode");
    assert!(disclosure.files().iter().any(|file| {
        file.path == path
            && matches!(
                file.decoding,
                DisclosureDecoding::Woff2Font {
                    tables: 9,
                    glyphs: 1..,
                    ..
                }
            )
    }));
    let preparation = build::check_preparation(
        &built,
        &policy(),
        Default::default(),
        Default::default(),
        None,
    )
    .expect("preparation scans the same retained WOFF2 bytes");
    assert!(preparation.disclosure().files().iter().any(|file| {
        file.path == path && matches!(file.decoding, DisclosureDecoding::Woff2Font { .. })
    }));
}

#[test]
fn font_references_never_escape_or_bypass_exact_output_closure() {
    for (reference, expected) in [
        ("../fonts/missing.woff2", "source_inventory"),
        ("../../outside.woff2", "resource_input"),
        ("../fonts/fixture.woff2?version=1", "resource_input"),
        ("../fonts/fixture.woff2#face", "resource_input"),
        ("https://other.example/font.woff2", "resource_input"),
        ("//other.example/font.woff2", "resource_input"),
    ] {
        let font = font();
        let resources = format!("{RESOURCES}{}", font_resource("/fonts/fixture.woff2"));
        let first = page("one", "/one/", &resources);
        let style = format!(
            "@use 'tokens'; @font-face {{ font-family: Fixture; src: url('{reference}') format('woff2'); }}"
        );
        let fixture = Fixture::new(
            false,
            &[
                ("content/one.md", first.as_bytes()),
                ("content/fixture.woff2", &font),
                ("content/main.scss", style.as_bytes()),
            ],
        );
        failure(&fixture.configuration(), expected);
    }
}

#[test]
fn font_output_collisions_are_refused_before_partial_publication() {
    let font = font();
    let resources = format!("{RESOURCES}{}", font_resource("/assets/z-first.css"));
    let first = page("one", "/one/", &resources);
    let fixture = Fixture::new(
        false,
        &[
            ("content/one.md", first.as_bytes()),
            ("content/fixture.woff2", &font),
        ],
    );
    failure(&fixture.configuration(), "publication_authored");
}
