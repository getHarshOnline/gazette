//! Structured source preserves one page-wide Markdown scope and original evidence.

use gazette_compiler::{document, landing::ParsedLanding};
use gazette_core::source::{Location, Precision, Sha256Digest};

fn parse(source: &str) -> ParsedLanding<'_> {
    ParsedLanding::parse(
        source,
        document::Limits::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .unwrap()
}

#[test]
fn sections_share_headings_footnotes_and_reference_definitions() {
    let source = r#"format: gazette-landing/v1
content: {id: 'content:one', title: 'A question'}
sections:
  - heading: {level: 1, text: 'A *real* question'}
    body: 'An [earlier reference][proof] and a note.[^one]'
  - heading: {level: 2, text: 'Another view'}
    body: |
      [proof]: https://example.test/evidence
      [^one]: One page-wide note.
  - heading: {level: 2, text: 'Another view'}
    body: 'An image: ![Evidence](images/example.png)'
"#;
    let parsed = parse(source);
    assert_eq!(parsed.source(), source);
    assert_eq!(parsed.source_sha256(), Sha256Digest::of(source.as_bytes()));
    assert_ne!(parsed.source_sha256(), parsed.body().source_sha256());
    let headings = parsed.body().headings().unwrap();
    assert_eq!(headings[0].text, "A *real* question");
    assert_eq!(headings[1].id, "another-view");
    assert_eq!(headings[2].id, "another-view-1");
    let html = parsed.body().render_html().unwrap();
    assert!(html.contains("https://example.test/evidence"));
    assert!(html.contains("One page-wide note."));
    assert!(html.contains("images/example.png"));
    let location = parsed
        .original_location(Location {
            span: headings[2].span,
            precision: headings[2].precision,
        })
        .unwrap();
    assert_eq!(location.precision, Precision::Construct);
    assert!(source[location.span.start..location.span.end].contains("Another view"));
    assert!(location.span.start > source.find("One page-wide note.").unwrap());
}

#[test]
fn nested_components_remain_native_directives_with_complete_child_content() {
    let source = r#"format: gazette-landing/v1
content: {id: 'content:one'}
sections:
  - component: introduction
    heading: {level: 1, text: 'Example'}
    sections:
      - component: comparison
        arguments: {mode: 'paired'}
        body: |
          A meaningful [destination](../next/).

          :::note {}
          An authored nested component.
          :::
  - component: conclusion
    body: 'A conclusion belonging to the comparison.'
"#;
    let parsed = parse(source);
    assert_eq!(parsed.body().directives().len(), 4);
    let text = parsed.body().text().unwrap();
    assert!(text.contains("An authored nested component."));
    assert!(text.contains("A conclusion belonging to the comparison."));
    assert!(parsed.body().check_source_rendering().is_ok());
}

#[test]
fn unknown_empty_and_invalid_sections_fail_before_projection() {
    for section in [
        "{extra: true, body: text}",
        "{}",
        "{arguments: {mode: paired}, body: text}",
        "{heading: {level: 7, text: text}}",
        "{component: 'bad name', body: text}",
    ] {
        let source = format!("format: gazette-landing/v1\nsections: [{section}]\n");
        assert!(
            ParsedLanding::parse(
                &source,
                Default::default(),
                &gazette_adapter_markdown::Comrak
            )
            .is_err(),
            "{section}"
        );
    }
}

#[test]
fn normalized_output_has_its_own_ceiling() {
    let source = "format: gazette-landing/v1\nsections: [{component: introduction, body: text}]\n";
    let mut limits = document::Limits::default();
    limits.markdown.source_bytes = 8;
    assert!(ParsedLanding::parse(source, limits, &gazette_adapter_markdown::Comrak).is_err());
}

#[test]
fn bodies_cannot_consume_the_following_declared_section() {
    for body in ["```\nexample", "~~~rust\nexample", ":::note {}\nexample"] {
        let source = format!(
            "format: gazette-landing/v1\nsections:\n  - body: {}\n  - heading: {{level: 2, text: Next}}\n",
            serde_json::to_string(body).unwrap()
        );
        assert!(
            ParsedLanding::parse(
                &source,
                Default::default(),
                &gazette_adapter_markdown::Comrak
            )
            .is_err(),
            "{body}"
        );
    }
}

#[test]
fn metadata_bytes_have_a_separate_ceiling() {
    let source =
        "format: gazette-landing/v1\ncontent: {title: 'A long title'}\nsections: [{body: text}]\n";
    let mut limits = document::Limits::default();
    limits.metadata.source_bytes = 8;
    assert!(ParsedLanding::parse(source, limits, &gazette_adapter_markdown::Comrak).is_err());
    let escaped_key = "format: gazette-landing/v1\n\"\\u0063\\u006f\\u006e\\u0074\\u0065\\u006e\\u0074\": {title: text}\nsections: [{body: text}]\n";
    limits.metadata.source_bytes = 40;
    assert!(ParsedLanding::parse(escaped_key, limits, &gazette_adapter_markdown::Comrak).is_err());
}

#[test]
fn component_names_use_the_existing_native_grammar() {
    let parsed =
        parse("format: gazette-landing/v1\nsections: [{component: Example_Group, body: text}]\n");
    assert!(
        parsed.body().directives()[0]
            .info
            .starts_with("Example_Group")
    );
}

#[test]
fn lowered_components_bind_to_the_existing_typed_contract() {
    use gazette_compiler::theme_package::ParsedManifest;
    let parsed = parse(
        "format: gazette-landing/v1\nsections:\n  - component: callout\n    arguments: {tone: note}\n    heading: {level: 2, text: Repeat}\n    body: First\n    sections:\n      - component: callout\n        arguments: {tone: warning}\n        heading: {level: 2, text: Repeat}\n        body: Second\n  - body: Closing\n",
    );
    let manifest = ParsedManifest::parse("package='theme:one'\nnamespace='one'\ntemplates='templates'\nrender={page='page.html'}\n[components.callout]\ntemplate='callout.html'\narguments={type='object',properties={tone={type='string'}},additionalProperties=false}\n", Default::default()).unwrap();
    let components = manifest.components();
    let bound = components.bind(parsed.body().directives(), Default::default());
    assert!(
        bound.is_ok(),
        "{bound:?}\n{}\n{:?}",
        parsed.body().source(),
        parsed.body().directives()
    );
}

#[test]
fn nested_argument_colons_and_block_scalars_round_trip_without_opening_empty_directives() {
    use gazette_compiler::theme_package::ParsedManifest;
    let source = r#"format: gazette-landing/v1
sections:
  - component: wrapper
    sections:
      - component: handoff
        arguments:
          inputs:
            - title: Observation
              body: |
                Check the site's instructions: another view may explain it.
            - title: 'Café 🦀: next'
              body: 'Literal \u003a stays literal; https://example.test/a:b stays a URL.'
          'key:with:colons':
            'nested:🦀': "Keep \"quotes\", a backslash \\, a tab\tand a colon: intact."
        body: The actual component body remains here.
"#;
    let parsed = parse(source);
    assert_eq!(parsed.source(), source);
    assert_eq!(parsed.source_sha256(), Sha256Digest::of(source.as_bytes()));
    let directives = parsed.body().directives();
    assert_eq!(
        directives.len(),
        2,
        "closing fences must not become empty-name directives"
    );
    assert!(
        directives
            .iter()
            .all(|directive| !directive.info.is_empty() && !directive.info.contains(':'))
    );
    let manifest = ParsedManifest::parse("package='theme:one'\nnamespace='one'\ntemplates='templates'\nrender={page='page.html'}\n[components.wrapper]\ntemplate='wrapper.html'\n[components.handoff]\ntemplate='handoff.html'\narguments={type='object',additionalProperties=true}\n", Default::default()).unwrap();
    let bound = manifest
        .components()
        .bind(directives, Default::default())
        .unwrap();
    assert_eq!(
        bound[1].arguments["inputs"][0]["body"],
        "Check the site's instructions: another view may explain it.\n"
    );
    assert_eq!(bound[1].arguments["inputs"][1]["title"], "Café 🦀: next");
    assert_eq!(
        bound[1].arguments["inputs"][1]["body"],
        r"Literal \u003a stays literal; https://example.test/a:b stays a URL."
    );
    assert_eq!(
        bound[1].arguments["key:with:colons"]["nested:🦀"],
        "Keep \"quotes\", a backslash \\, a tab\tand a colon: intact."
    );
    let origin = parsed.original_location(directives[1].header).unwrap();
    assert_eq!(origin.precision, Precision::Construct);
    let original = origin.span.slice(source).unwrap();
    assert!(original.contains("handoff\n"));
    assert!(origin.span.start > source.find("component: wrapper").unwrap());
    assert!(original.contains("instructions: another view"));
    assert!(original.contains("The actual component body remains here."));
}

#[test]
fn selected_math_profile_reaches_the_normalized_body() {
    use gazette_core::markdown::Profile;
    let source = "format: gazette-landing/v1\nsections: [{body: 'An authored relation $x+y$.'}]\n";
    let parsed = ParsedLanding::parse_with_profile(
        source,
        Profile::PublicationMath,
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .unwrap();
    assert_eq!(parsed.body().identity().profile, Profile::PublicationMath);
    assert_eq!(parsed.body().math().len(), 1);
    assert!(parse(source).body().math().is_empty());
}

#[test]
fn argument_driven_components_do_not_require_filler_prose() {
    let source = r#"format: gazette-landing/v1
sections:
  - component: relationship
    arguments:
      source: Ledger
      positions:
        - owner: Reviewer
          state: Pending
  - component: divider
"#;
    let parsed = parse(source);
    assert_eq!(parsed.body().directives().len(), 2);
    assert!(parsed.body().source().contains("Reviewer"));
    assert!(parsed.body().check_source_rendering().is_ok());
}

#[test]
fn component_names_keep_original_yaml_ranges_after_unicode_and_crlf() {
    let source = concat!(
        "format: gazette-landing/v1\r\n",
        "content: {id: 'content:one', title: 'Café 🦀'}\r\n",
        "sections:\r\n",
        "  - component: 'introduction'\r\n",
        "    sections:\r\n",
        "      - {component: comparison, body: paired}\r\n",
        "      - {component: \"n\\u006fte\", body: note}\r\n",
    );
    let slots = parse(source).component_slots();
    assert_eq!(slots.len(), 3);
    for (slot, (expected, token)) in slots.iter().zip([
        ("introduction", "'introduction'"),
        ("comparison", "comparison"),
    ]) {
        assert_eq!(slot.name.as_str(), expected);
        assert_eq!(slot.name_location.precision, Precision::Exact);
        assert_eq!(slot.name_location.span.slice(source), Some(token));
    }
    assert_eq!(slots[2].name.as_str(), "note");
    assert_eq!(slots[2].name_location.precision, Precision::Construct);
}
