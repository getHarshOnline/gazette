//! Whole-source destination rewriting through the real retained Comrak document.

use gazette_adapter_markdown::Comrak;
use gazette_core::{
    markdown::{Error, Limits, LinkBinding, LinkForm, LinkKind, Profile, Syntax},
    source::Sha256Digest,
};
use gazette_ports::markdown::{Document, Markdown};

fn parse(source: &str) -> Box<dyn Document> {
    Comrak
        .parse(source, Profile::Publication, Limits::default())
        .expect("retain actual source facts")
}

fn bindings(syntax: &Syntax) -> Vec<LinkBinding> {
    syntax
        .links
        .iter()
        .map(|link| LinkBinding {
            image_sources: None,
            svg_layout: None,
            image_width: None,
            image_dimensions: None,
            image_loading: None,
            image_fetch_priority: None,
            inline_svg: None,
            image_address: None,
            occurrence: link.clone(),
            destination: link.destination.clone(),
            annotation: None,
        })
        .collect()
}

fn destinations(syntax: &Syntax) -> Vec<&str> {
    let mut values: Vec<_> = syntax
        .links
        .iter()
        .map(|link| link.destination.as_str())
        .collect();
    values.sort_unstable();
    values
}

#[test]
fn unchanged_whole_source_bindings_preserve_exact_bytes_and_repeatability() {
    let source = "# Héllo {#explicit .tone}\r\n\r\n[inline](./old \"title\") and [ref][ID].\r\n\r\n[ID]: ./ref\r\n\r\n[^unused]: [hidden](./hidden)\r\n";
    let document = parse(source);
    assert_eq!(document.syntax().links.len(), 3);
    let mut selected = bindings(document.syntax());
    selected.reverse();
    for _ in 0..2 {
        assert_eq!(
            document
                .rewrite_links(document.syntax().identity, &selected)
                .unwrap(),
            source
        );
    }
    assert_eq!(
        document.syntax().identity.source_sha256,
        Sha256Digest::of(source.as_bytes())
    );
    assert_eq!(
        document.syntax().headings[0].id.as_deref(),
        Some("explicit")
    );
    let empty = parse("");
    assert_eq!(
        empty.rewrite_links(empty.syntax().identity, &[]).unwrap(),
        ""
    );
}

#[test]
fn changed_inline_reference_and_image_destinations_preserve_labels_titles_code_and_html() {
    let source = "# API\n\n[**Inline**](./one \"Inline title\"), [Reference][api] and ![image alt](./image.png \"Image title\").\n\n[api]: ./two \"Reference title\"\n\n`[not a link](./one)`\n\n```rust\npub fn query<T: Copy>(value: &T) -> Result<T, Error> { todo!() }\n// [not a link](./two)\n```\n\n<div data-destination=\"./one\">raw <a href=\"./two\">HTML link</a></div>\n";
    let document = parse(source);
    assert_eq!(
        document.syntax().links.len(),
        3,
        "code and HTML are not Markdown links"
    );
    let mut selected = bindings(document.syntax());
    for binding in &mut selected {
        binding.destination = match binding.occurrence.destination.as_str() {
            "./one" => "/module/one",
            "./two" => "/module/two#item",
            "./image.png" => "/assets/image.png",
            other => panic!("unexpected real occurrence {other}"),
        }
        .into();
    }
    selected.reverse();
    let rewritten = document
        .rewrite_links(document.syntax().identity, &selected)
        .unwrap();
    assert_eq!(
        rewritten,
        document
            .rewrite_links(document.syntax().identity, &selected)
            .unwrap()
    );
    let projected = parse(&rewritten);
    assert_eq!(
        destinations(projected.syntax()),
        ["/assets/image.png", "/module/one", "/module/two#item"]
    );
    let mut expected: Vec<_> = selected
        .iter()
        .map(|binding| {
            (
                binding.destination.as_str(),
                binding.occurrence.kind,
                binding.occurrence.title.as_str(),
            )
        })
        .collect();
    expected.sort_by_key(|row| row.0);
    let mut actual: Vec<_> = projected
        .syntax()
        .links
        .iter()
        .map(|link| (link.destination.as_str(), link.kind, link.title.as_str()))
        .collect();
    actual.sort_by_key(|row| row.0);
    assert_eq!(actual, expected);
    let text: String = projected
        .syntax()
        .text
        .iter()
        .map(|part| part.value.as_str())
        .collect();
    for literal in [
        "Inline",
        "Reference",
        "image alt",
        "[not a link](./one)",
        "pub fn query<T: Copy>(value: &T) -> Result<T, Error> { todo!() }",
        "// [not a link](./two)",
    ] {
        assert!(text.contains(literal), "missing {literal:?} from {text:?}");
    }
    assert_eq!(
        projected
            .syntax()
            .html
            .iter()
            .map(|html| &html.literal)
            .collect::<Vec<_>>(),
        document
            .syntax()
            .html
            .iter()
            .map(|html| &html.literal)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        destinations(document.syntax()),
        ["./image.png", "./one", "./two"],
        "retained source is immutable"
    );
}

#[test]
fn nested_images_and_used_unused_and_nested_footnote_links_are_all_bound() {
    let source = "[![alt](./image)](./page) and note[^used].\n\n[^used]: [used](./used) and deeper[^nested]\n\n[^nested]: [nested](./nested)\n\n[^unused]: [hidden](./hidden)\n";
    let document = parse(source);
    assert_eq!(document.syntax().links.len(), 5);
    assert_eq!(document.syntax().footnotes.len(), 3);
    assert!(
        document
            .syntax()
            .links
            .iter()
            .any(|link| link.destination == "./hidden" && !link.emitted)
    );
    assert!(
        document
            .syntax()
            .footnote_references
            .iter()
            .any(|reference| reference.definition.is_some())
    );
    let selected: Vec<_> = bindings(document.syntax())
        .into_iter()
        .map(|mut binding| {
            binding.destination = format!(
                "/resolved/{}",
                binding.destination.strip_prefix("./").unwrap()
            );
            binding
        })
        .collect();
    let rewritten = document
        .rewrite_links(document.syntax().identity, &selected)
        .unwrap();
    let projected = parse(&rewritten);
    assert_eq!(
        destinations(projected.syntax()),
        [
            "/resolved/hidden",
            "/resolved/image",
            "/resolved/nested",
            "/resolved/page",
            "/resolved/used"
        ]
    );
    assert_eq!(projected.syntax().footnotes.len(), 3);
    assert!(
        projected
            .syntax()
            .links
            .iter()
            .any(|link| link.destination == "/resolved/hidden" && !link.emitted)
    );
    assert!(
        projected
            .syntax()
            .footnote_references
            .iter()
            .any(|reference| reference.definition.is_some())
    );
    let only_emitted: Vec<_> = selected
        .iter()
        .filter(|binding| binding.occurrence.emitted)
        .cloned()
        .collect();
    assert!(only_emitted.len() < selected.len());
    assert_eq!(
        document.rewrite_links(document.syntax().identity, &only_emitted),
        Err(Error::InvalidPlan)
    );
}

#[test]
fn wrong_identity_missing_duplicate_and_forged_facts_never_format_a_partial_document() {
    let document = parse("[first](./one \"title\") ![second](./two)\n");
    let identity = document.syntax().identity;
    let valid = bindings(document.syntax());
    assert_eq!(valid.len(), 2);
    let mut wrong_identity = identity;
    wrong_identity.source_sha256 = Sha256Digest::of(b"another source");
    assert_eq!(
        document.rewrite_links(wrong_identity, &valid),
        Err(Error::InvalidPlan)
    );
    assert_eq!(
        document.rewrite_links(identity, &valid[..1]),
        Err(Error::InvalidPlan)
    );
    let mut extra = valid.clone();
    extra.push(valid[0].clone());
    assert_eq!(
        document.rewrite_links(identity, &extra),
        Err(Error::InvalidPlan)
    );
    let duplicate = [valid[0].clone(), valid[0].clone()];
    assert_eq!(
        document.rewrite_links(identity, &duplicate),
        Err(Error::InvalidPlan)
    );
    for field in ["title", "kind", "form", "destination", "emitted", "span"] {
        let mut forged = valid.clone();
        let occurrence = &mut forged[0].occurrence;
        match field {
            "title" => occurrence.title = "forged".into(),
            "kind" => occurrence.kind = LinkKind::Image,
            "form" => occurrence.form = LinkForm::WikiLink,
            "destination" => occurrence.destination = "/forged".into(),
            "emitted" => occurrence.emitted = !occurrence.emitted,
            "span" => occurrence.location.span.start += 1,
            _ => unreachable!(),
        }
        assert_eq!(
            document.rewrite_links(identity, &forged),
            Err(Error::InvalidPlan),
            "{field}"
        );
    }
    assert_eq!(
        document.rewrite_links(identity, &valid).unwrap(),
        "[first](./one \"title\") ![second](./two)\n"
    );
}

#[test]
fn unsafe_or_control_destinations_are_refused_even_when_other_bindings_are_valid() {
    let document = parse("[first](./one) ![second](./two)\n");
    for destination in [
        "javascript:alert(1)",
        "vbscript:bad",
        "data:text/html,bad",
        "/safe\nspoof",
        "/safe\0tail",
        "/safe\u{7f}tail",
    ] {
        let mut selected = bindings(document.syntax());
        selected[1].destination = destination.into();
        assert_eq!(
            document.rewrite_links(document.syntax().identity, &selected),
            Err(Error::InvalidPlan),
            "{destination:?}"
        );
    }
}

#[test]
fn output_and_aggregate_binding_limits_refuse_without_truncation() {
    let source = "[label](./a)\n";
    let unlimited = parse(source);
    let mut selected = bindings(unlimited.syntax());
    selected[0].destination = format!("/{}", "a".repeat(200));
    let complete = unlimited
        .rewrite_links(unlimited.syntax().identity, &selected)
        .unwrap();
    for limit in [complete.len(), complete.len() - 1] {
        let document = Comrak
            .parse(
                source,
                Profile::Publication,
                Limits {
                    output_bytes: limit,
                    ..Limits::default()
                },
            )
            .unwrap();
        let result = document.rewrite_links(document.syntax().identity, &selected);
        if limit == complete.len() {
            assert_eq!(result.unwrap(), complete);
        } else {
            assert_eq!(result, Err(Error::Limit("rendered output bytes")));
        }
    }
    let document = Comrak
        .parse(
            source,
            Profile::Publication,
            Limits {
                output_bytes: 100,
                ..Limits::default()
            },
        )
        .unwrap();
    assert_eq!(
        document.rewrite_links(document.syntax().identity, &selected),
        Err(Error::Limit("fact string bytes"))
    );
    let document = Comrak
        .parse(
            source,
            Profile::Publication,
            Limits {
                output_bytes: source.len() - 1,
                ..Limits::default()
            },
        )
        .unwrap();
    assert_eq!(
        document.rewrite_links(document.syntax().identity, &bindings(document.syntax())),
        Err(Error::Limit("rendered output bytes"))
    );
}

#[test]
fn changed_link_with_heading_attributes_fails_instead_of_erasing_the_anchor() {
    let source = "# API {#stable .reference}\n\n[code](./old)\n";
    let document = parse(source);
    assert_eq!(document.syntax().headings[0].id.as_deref(), Some("stable"));
    assert_eq!(document.syntax().headings[0].classes, ["reference"]);
    let mut selected = bindings(document.syntax());
    assert_eq!(
        document
            .rewrite_links(document.syntax().identity, &selected)
            .unwrap(),
        source
    );
    selected[0].destination = "/new".into();
    assert_eq!(
        document.rewrite_links(document.syntax().identity, &selected),
        Err(Error::Unsupported {
            location: document.syntax().headings[0].location,
            reason: "native Markdown link formatting cannot preserve heading attributes",
        })
    );
}
