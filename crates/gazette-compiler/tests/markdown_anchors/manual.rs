//! Manual admission is explicit and does not change publication rendering.

use gazette_compiler::markdown::{AnchorPolicy, Error, Limits, ParsedMarkdown};

#[test]
fn manual_and_publication_anchor_caches_keep_their_own_meaning() {
    let source = "<a id=\"ready\"></a>\n\n# Ready\n\n[Alias](#ready) [Heading](#ready-1)\n";
    let parsed =
        ParsedMarkdown::parse(source, Limits::default(), &gazette_adapter_markdown::Comrak)
            .unwrap();
    assert_eq!(parsed.headings().unwrap()[0].id, "ready");
    let manual = parsed.headings_with_policy(AnchorPolicy::Manual).unwrap();
    assert_eq!(manual[0].id, "ready-1");
    assert!(std::ptr::eq(
        manual,
        parsed.headings_with_policy(AnchorPolicy::Manual).unwrap()
    ));
    let references = parsed
        .heading_references_with_policy(AnchorPolicy::Manual)
        .unwrap();
    assert_eq!(references.len(), 1);
    assert_eq!(
        references[0].location.span.slice(source),
        Some("[Heading](#ready-1)")
    );
    assert_eq!(references[0].heading, 0);
    parsed
        .check_source_rendering_with_policy(AnchorPolicy::Manual)
        .unwrap();
    assert!(matches!(
        parsed.check_source_rendering(),
        Err(Error::RawHtml(_))
    ));
    assert!(matches!(parsed.render_html(), Err(Error::RawHtml(_))));
    assert!(parsed.heading_references().is_err());
    assert_eq!(parsed.headings().unwrap()[0].id, "ready");
    assert_eq!(parsed.source(), source);
}

#[test]
fn manual_anchor_retention_uses_the_existing_output_ceiling() {
    let source = "<a name=\"alias\"></a>\n\n# Heading\n";
    let parsed = ParsedMarkdown::parse(
        source,
        Limits {
            output_bytes: 1,
            ..Limits::default()
        },
        &gazette_adapter_markdown::Comrak,
    )
    .unwrap();
    assert!(matches!(
        parsed.headings_with_policy(AnchorPolicy::Manual),
        Err(Error::Limit("manual anchor bytes"))
    ));
}
