//! Native URL-first wikilinks retain source evidence and require complete output bindings.

use gazette_adapter_markdown::{Comrak, PROFILE};
use gazette_core::{
    markdown::{
        Error, FootnoteBinding, HeadingBinding, Limits, LinkAnnotation, LinkBinding,
        LinkDestinationKind, LinkForm, LinkKind, LinkRelationship, Plan, Profile, Syntax,
    },
    source::{Precision, Sha256Digest},
};
use gazette_ports::markdown::{Document, Markdown};

fn parse(source: &str) -> Box<dyn Document> {
    Comrak
        .parse(source, Profile::Publication, Limits::default())
        .expect("native retained document")
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

fn plan(syntax: &Syntax) -> Plan {
    Plan {
        identity: syntax.identity,
        headings: syntax
            .headings
            .iter()
            .filter(|heading| heading.emitted)
            .enumerate()
            .map(|(index, heading)| HeadingBinding {
                occurrence: heading.clone(),
                id: format!("heading-{index}"),
                level: heading.level,
            })
            .collect(),
        links: bindings(syntax)
            .into_iter()
            .filter(|binding| binding.occurrence.emitted)
            .collect(),
        footnotes: syntax
            .footnotes
            .iter()
            .enumerate()
            .map(|(index, note)| FootnoteBinding {
                occurrence: note.clone(),
                alias: format!("note-{index}"),
            })
            .collect(),
        excerpt: None,
        math: Vec::new(),
        components: Vec::new(),
    }
}

#[test]
fn native_url_first_wikilinks_retain_labels_forms_and_unicode_crlf_constructs() {
    let source = "α prelude\r\n\r\nBefore [[site:example.test/one/#part|R&amp;D \\| β]] after [ordinary](site:example.test/one/).\r\n\r\n`[[site:example.test/code/|Code]]`\r\n\r\n```text\r\n[[site:example.test/fence/|Fence]]\r\n```\r\n";
    let document = parse(source);
    let syntax = document.syntax();
    assert_eq!(
        PROFILE,
        "comrak@0.54.0/syntect@5.3.0/two-face@0.5.2+bat-0.26.1/gazette-publication-markdown-v13"
    );
    assert_eq!(Comrak.profile(), PROFILE);
    assert_eq!(
        syntax.identity.source_sha256,
        Sha256Digest::of(source.as_bytes())
    );
    assert_eq!(syntax.links.len(), 2, "code is not a link occurrence");
    let wiki = &syntax.links[0];
    assert_eq!(wiki.form, LinkForm::WikiLink);
    assert_eq!(wiki.kind, LinkKind::Link);
    assert_eq!(wiki.destination, "site:example.test/one/#part");
    assert!(wiki.title.is_empty());
    assert!(wiki.emitted);
    assert_eq!(wiki.location.precision, Precision::Construct);
    assert_eq!(
        wiki.location.span.slice(source),
        Some("[[site:example.test/one/#part|R&amp;D \\| β]]")
    );
    assert_eq!(wiki.location.span.start, source.find("[[site:").unwrap());
    assert_eq!(syntax.links[1].form, LinkForm::Markdown);
    assert_eq!(syntax.links[1].destination, "site:example.test/one/");
    let mut selected = plan(syntax);
    selected.links[0].destination = "https://example.test/one/#part".into();
    selected.links[1].destination = "https://example.test/one/".into();
    let rendered = document
        .render(&selected, None)
        .expect("safe bound outputs");
    assert!(rendered.body.html.contains("R&amp;D | β</a>"));
    assert!(rendered.body.text.contains("R&D | β"));
    assert!(!rendered.body.text.contains("site:example.test/one/#part"));
    assert_eq!(document.syntax(), syntax);
}

#[test]
fn native_wikilink_bindings_emit_existing_public_annotations_without_changing_source() {
    let source = "[[site:example.test/one/|MONAD]] and [external](https://outside.test/).\n";
    let document = parse(source);
    let mut selected = plan(document.syntax());
    selected.links[0].destination = "http://127.0.0.1:4312/prefix/one/?a=1&b=2#part".into();
    selected.links[0].annotation = Some(LinkAnnotation {
        address: "content:source#link-1".into(),
        kind: LinkDestinationKind::CrossSite,
        content: Some("content:target".parse().unwrap()),
        site: Some("site:target".parse().unwrap()),
        relationships: vec![LinkRelationship::Related],
        theme: Some("theme:target".parse().unwrap()),
        palette: Some("palette:target".parse().unwrap()),
    });
    let rendered = document
        .render(&selected, None)
        .expect("proved binding facts");
    for attribute in [
        "href=\"http://127.0.0.1:4312/prefix/one/?a=1&amp;b=2#part\"",
        "data-gazette-link=\"content:source#link-1\"",
        "data-gazette-destination-kind=\"cross-site\"",
        "data-gazette-destination-content=\"content:target\"",
        "data-gazette-destination-site=\"site:target\"",
        "data-gazette-destination-relationships=\"related\"",
        "data-gazette-destination-theme=\"theme:target\"",
        "data-gazette-destination-palette=\"palette:target\"",
    ] {
        assert!(rendered.body.html.contains(attribute), "{attribute}");
    }
    assert!(!rendered.body.html.contains("data-wikilink"));
    assert!(rendered.body.html.contains(">MONAD</a>"));
    assert_eq!(
        document.syntax().links[0].destination,
        "site:example.test/one/"
    );
    assert_eq!(
        document.syntax().identity.source_sha256,
        Sha256Digest::of(source.as_bytes())
    );
    assert_eq!(document.render(&selected, None).unwrap(), rendered);
}

#[test]
fn native_wikilinks_require_all_exact_bindings_and_refuse_unresolved_or_unsafe_outputs() {
    let document = parse("[[site:example.test/one/|One]] [ordinary](./two)\n");
    let mut selected = plan(document.syntax());
    assert_eq!(document.render(&selected, None), Err(Error::InvalidPlan));
    for destination in [
        "site:example.test/one/",
        "javascript:alert(1)",
        "data:text/html,bad",
        "custom:target",
        "/safe\nspoof",
        "/safe\u{7f}spoof",
        "/unsafe\\path",
    ] {
        selected.links[0].destination = destination.into();
        assert_eq!(
            document.render(&selected, None),
            Err(Error::InvalidPlan),
            "{destination:?}"
        );
        assert_eq!(
            document.rewrite_links(document.syntax().identity, &selected.links),
            Err(Error::InvalidPlan),
            "{destination:?}"
        );
    }
    selected.links[0].destination = "/one/".into();
    let valid = selected.clone();
    let mut duplicate = valid.clone();
    duplicate.links[1] = duplicate.links[0].clone();
    assert_eq!(document.render(&duplicate, None), Err(Error::InvalidPlan));
    assert_eq!(
        document.rewrite_links(duplicate.identity, &duplicate.links),
        Err(Error::InvalidPlan)
    );
    selected.links.pop();
    assert_eq!(document.render(&selected, None), Err(Error::InvalidPlan));
    assert_eq!(
        document.rewrite_links(document.syntax().identity, &selected.links),
        Err(Error::InvalidPlan)
    );
    for field in [
        "form",
        "kind",
        "title",
        "destination",
        "location",
        "identity",
    ] {
        let mut forged = valid.clone();
        match field {
            "form" => forged.links[0].occurrence.form = LinkForm::Markdown,
            "kind" => forged.links[0].occurrence.kind = LinkKind::Image,
            "title" => forged.links[0].occurrence.title = "forged".into(),
            "destination" => forged.links[0].occurrence.destination = "/forged".into(),
            "location" => forged.links[0].occurrence.location.span.start += 1,
            "identity" => forged.identity.source_sha256 = Sha256Digest::of(b"other"),
            _ => unreachable!(),
        }
        assert_eq!(
            document.render(&forged, None),
            Err(Error::InvalidPlan),
            "{field}"
        );
        assert_eq!(
            document.rewrite_links(forged.identity, &forged.links),
            Err(Error::InvalidPlan),
            "{field}"
        );
    }
    assert!(document.render(&valid, None).is_ok());
}

#[test]
fn native_wikilink_occurrences_bind_by_source_not_traversal_or_shared_target() {
    let source = "[[site:example.test/one/|First]] then [[site:example.test/one/|Second]].\n";
    for profile in [Profile::Publication, Profile::PublicationMath] {
        let document = Comrak.parse(source, profile, Limits::default()).unwrap();
        assert_eq!(document.syntax().links.len(), 2);
        assert_ne!(
            document.syntax().links[0].location,
            document.syntax().links[1].location
        );
        let mut selected = plan(document.syntax());
        selected.links[0].destination = "/first/".into();
        selected.links[1].destination = "/second/".into();
        let rendered = document.render(&selected, None).unwrap();
        selected.links.reverse();
        assert_eq!(document.render(&selected, None).unwrap(), rendered);
        assert!(
            rendered
                .body
                .html
                .contains("href=\"/first/\" data-wikilink=\"true\">First</a>")
        );
        assert!(
            rendered
                .body
                .html
                .contains("href=\"/second/\" data-wikilink=\"true\">Second</a>")
        );
        assert_eq!(
            document.syntax().identity.source_sha256,
            Sha256Digest::of(source.as_bytes())
        );
    }
}

#[test]
fn malformed_brackets_are_native_text_and_wikilink_labels_are_not_a_second_inline_parser() {
    let source = "[[site:example.test/broken/|Broken]\n\n[[https://example.test/|**literal**]]\n";
    let document = parse(source);
    assert_eq!(document.syntax().links.len(), 1);
    assert_eq!(document.syntax().links[0].form, LinkForm::WikiLink);
    let rendered = document.render(&plan(document.syntax()), None).unwrap();
    assert!(rendered.body.html.contains(">**literal**</a>"));
    assert!(!rendered.body.html.contains("<strong>literal</strong>"));
    assert!(
        rendered
            .body
            .text
            .contains("[[site:example.test/broken/|Broken]")
    );
}

#[test]
fn unchanged_wikilink_projection_preserves_exact_source_bytes() {
    let source = "# Héllo {#authored .tone}\r\n\r\n[[https://example.test/one/|R&amp;D β]]\r\n\r\n[^unused]: [[https://example.test/hidden/|Hidden]]\r\n";
    let document = parse(source);
    assert_eq!(document.syntax().links.len(), 2);
    assert!(!document.syntax().links[1].emitted);
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
}

#[test]
fn changed_wikilinks_use_native_link_formatter_for_all_occurrences_and_keep_labels() {
    let source = "[[site:example.test/one/|R&amp;D \\| β]] and [ordinary](./two \"Title\").\r\n\r\n[^unused]: [[site:example.test/hidden/|Hidden]]\r\n";
    let document = parse(source);
    let mut selected = bindings(document.syntax());
    assert_eq!(selected.len(), 3);
    for binding in &mut selected {
        binding.destination = match binding.occurrence.destination.as_str() {
            "site:example.test/one/" => "https://example.test/one/#part",
            "./two" => "/two/",
            "site:example.test/hidden/" => "/hidden/",
            other => panic!("unexpected occurrence {other}"),
        }
        .into();
    }
    selected.reverse();
    let rewritten = document
        .rewrite_links(document.syntax().identity, &selected)
        .unwrap();
    assert_eq!(
        document
            .rewrite_links(document.syntax().identity, &selected)
            .unwrap(),
        rewritten
    );
    let projected = parse(&rewritten);
    assert_eq!(projected.syntax().links.len(), 3);
    assert_eq!(projected.syntax().links[0].form, LinkForm::Markdown);
    assert_eq!(
        projected.syntax().links[0].destination,
        "https://example.test/one/#part"
    );
    assert_eq!(projected.syntax().links[1].form, LinkForm::Markdown);
    assert_eq!(projected.syntax().links[1].title, "Title");
    assert_eq!(projected.syntax().links[2].form, LinkForm::Markdown);
    assert_eq!(projected.syntax().links[2].destination, "/hidden/");
    let rendered = projected.render(&plan(projected.syntax()), None).unwrap();
    assert!(rendered.body.text.contains("R&D | β"));
    assert!(!rendered.body.html.contains("Hidden"));
    assert_eq!(
        document.syntax().links[0].destination,
        "site:example.test/one/"
    );
    assert_eq!(
        document.syntax().identity.source_sha256,
        Sha256Digest::of(source.as_bytes())
    );
}

#[test]
fn native_wikilinks_obey_existing_syntax_fact_and_output_bounds() {
    let source = "[[site:example.test/one/|Label]]\n";
    for limits in [
        Limits {
            nodes: 2,
            ..Limits::default()
        },
        Limits {
            fact_bytes: 1,
            ..Limits::default()
        },
    ] {
        assert!(matches!(
            Comrak.parse(source, Profile::Publication, limits),
            Err(Error::Limit(_))
        ));
    }
    let document = Comrak
        .parse(
            source,
            Profile::Publication,
            Limits {
                output_bytes: 32,
                ..Limits::default()
            },
        )
        .unwrap();
    let mut selected = plan(document.syntax());
    selected.links[0].destination = format!("/{}", "a".repeat(64));
    assert!(matches!(
        document.render(&selected, None),
        Err(Error::Limit(_))
    ));
    assert!(matches!(
        document.rewrite_links(document.syntax().identity, &selected.links),
        Err(Error::Limit(_))
    ));
}
