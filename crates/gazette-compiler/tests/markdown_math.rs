//! Native parse, retained formula binding and one body/excerpt/text projection.

use gazette_adapter_markdown::Comrak;
use gazette_compiler::markdown::{Error, Limits, ParsedMarkdown};
use gazette_core::markdown::{MathBinding, Profile};
use gazette_core::source::Sha256Digest;

// Captured from the Gazette KaTeX 0.18.4 generator, not invented HTML templates.
const INLINE: &str = r#"<math xmlns="http://www.w3.org/1998/Math/MathML"><semantics><mrow><mi>x</mi></mrow><annotation encoding="application/x-tex">x</annotation></semantics></math>"#;
const DISPLAY: &str = r#"<math xmlns="http://www.w3.org/1998/Math/MathML" display="block"><semantics><mrow><msup><mi>y</mi><mn>2</mn></msup></mrow><annotation encoding="application/x-tex">y^2</annotation></semantics></math>"#;

fn parse(source: &str) -> ParsedMarkdown<'_> {
    ParsedMarkdown::parse_with_profile(source, Profile::PublicationMath, Limits::default(), &Comrak)
        .unwrap()
}

fn bindings(body: &ParsedMarkdown<'_>) -> Vec<MathBinding> {
    body.math()
        .iter()
        .filter(|m| m.emitted)
        .zip([INLINE, DISPLAY])
        .map(|(occurrence, mathml)| MathBinding {
            occurrence: occurrence.clone(),
            mathml: mathml.to_owned(),
        })
        .collect()
}

#[test]
fn retained_typesetting_flows_through_complete_body_balanced_excerpt_and_same_native_text() {
    let source = "Before $x$ and `$literal$`.\n\n<!-- more -->\n\nAfter $$y^2$$.\n";
    let body = parse(source);
    let generated = bindings(&body);
    let identity = body.identity();
    let body = body.with_math(identity, generated).unwrap();
    let output = body
        .render_resolving_references(|_, _, _| Ok(None))
        .unwrap();
    assert!(output.body_html.contains(INLINE));
    assert!(output.body_html.contains(DISPLAY));
    assert!(output.body_html.contains("<code>$literal$</code>"));
    assert!(output.body_text.contains("Before x and $literal$."));
    assert!(output.body_text.contains("After y^2."));
    assert!(!output.body_text.contains("MathML"));
    let excerpt = output.summary.as_ref().unwrap();
    assert!(excerpt.html.contains(INLINE));
    assert!(!excerpt.html.contains(DISPLAY));
    assert!(!excerpt.text.contains("After"));
    assert_eq!(
        output,
        body.render_resolving_references(|_, _, _| Ok(None))
            .unwrap()
    );
}

#[test]
fn wrong_source_profile_formula_and_missing_or_unsafe_results_never_produce_partial_output() {
    let source = "$x$ and $$y^2$$";
    let body = parse(source);
    let original = bindings(&body);
    let mut identity = body.identity();
    identity.source_sha256 = Sha256Digest::of(b"different source");
    assert!(body.with_math(identity, original.clone()).is_err());
    let body = parse(source);
    let mut identity = body.identity();
    identity.profile = Profile::Publication;
    assert!(body.with_math(identity, original.clone()).is_err());
    let body = parse(source);
    let identity = body.identity();
    assert!(body.with_math(identity, vec![original[0].clone()]).is_err());
    let body = parse(source);
    let identity = body.identity();
    let mut changed = original.clone();
    changed[1].occurrence.display = false;
    assert!(body.with_math(identity, changed).is_err());
    assert!(parse(source).render_html().is_err());
    let body = parse(source);
    let identity = body.identity();
    let mut changed = original;
    changed[1].mathml = "<script>bad()</script>".into();
    let body = body.with_math(identity, changed).unwrap();
    assert!(matches!(body.render_html(), Err(Error::Adapter(_))));
}

#[test]
fn native_math_fence_uses_the_same_retained_generation_and_preserves_literal_tex() {
    let source = "```math\ny^2\n```\n";
    let body = parse(source);
    assert_eq!(body.math().len(), 1);
    let occurrence = body.math()[0].clone();
    assert_eq!(occurrence.tex, "y^2\n");
    assert!(occurrence.display && occurrence.emitted);
    assert!(
        occurrence
            .location
            .span
            .slice(source)
            .unwrap()
            .starts_with("```math")
    );
    // Exact captured KaTeX output for the fenced literal retains its terminal LF.
    let generated = DISPLAY.replace("y^2</annotation>", "y^2\n</annotation>");
    let identity = body.identity();
    let output = body
        .with_math(
            identity,
            vec![MathBinding {
                occurrence,
                mathml: generated.clone(),
            }],
        )
        .unwrap()
        .render_resolving_references(|_, _, _| Ok(None))
        .unwrap();
    assert!(output.body_html.contains(&generated));
    assert!(!output.body_html.contains("language-math"));
    assert_eq!(output.body_text.trim(), "y^2");
}
