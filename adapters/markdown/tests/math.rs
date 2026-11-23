//! Explicit native syntax selection precedes effectful typesetting and retained output.

use gazette_adapter_markdown::Comrak;
use gazette_core::markdown::{Error, Limits, Plan, Profile};
use gazette_core::source::Sha256Digest;
use gazette_ports::markdown::Markdown;

#[test]
fn selected_math_preserves_native_formula_locations_modes_and_literal_code() {
    let source = "α $x^2$ and $$y^2$$ and $`z_1`$.\n\n```math\na+b\n```\n\n`$not_math$` and $20,000 in $USD.\n";
    let document = Comrak
        .parse(source, Profile::PublicationMath, Limits::default())
        .expect("native math syntax");
    let syntax = document.syntax();
    assert_eq!(
        syntax.identity.source_sha256,
        Sha256Digest::of(source.as_bytes())
    );
    assert_eq!(syntax.identity.profile, Profile::PublicationMath);
    let formulas: Vec<_> = syntax
        .math
        .iter()
        .map(|m| (m.tex.as_str(), m.display, m.emitted))
        .collect();
    assert_eq!(
        formulas,
        [
            ("x^2", false, true),
            ("y^2", true, true),
            ("z_1", false, true),
            ("a+b\n", true, true)
        ]
    );
    for formula in &syntax.math {
        let span = formula.location.span;
        let original = &source[span.start..span.end];
        assert!(original.contains(formula.tex.trim()), "{original:?}");
    }
    assert!(
        syntax
            .math
            .windows(2)
            .all(|pair| pair[0].location.span < pair[1].location.span)
    );
    assert!(syntax.text.iter().any(|piece| piece.value == "x^2"));
    assert!(syntax.text.iter().any(|piece| piece.value == "$not_math$"));
    assert_eq!(
        document.syntax(),
        Comrak
            .parse(source, Profile::PublicationMath, Limits::default())
            .unwrap()
            .syntax()
    );
}

#[test]
fn ordinary_profile_does_not_silently_select_math_parsing() {
    let source = "$x^2$\n\n```math\ny^2\n```\n";
    let document = Comrak
        .parse(source, Profile::Publication, Limits::default())
        .unwrap();
    assert!(document.syntax().math.is_empty());
    let plan = Plan {
        identity: document.syntax().identity,
        headings: vec![],
        links: vec![],
        footnotes: vec![],
        excerpt: None,
        math: vec![],
        components: vec![],
    };
    let output = document.render(&plan, None).unwrap();
    assert!(output.body.html.contains("$x^2$"));
    assert!(output.body.html.contains("class=\"language-math\""));
    assert!(!output.body.html.contains("<math"));
}

#[test]
fn recognized_math_cannot_masquerade_as_completed_typesetting() {
    let document = Comrak
        .parse("$x$", Profile::PublicationMath, Limits::default())
        .unwrap();
    assert_eq!(document.check_rendering(), Ok(()));
    let plan = Plan {
        identity: document.syntax().identity,
        headings: vec![],
        links: vec![],
        footnotes: vec![],
        excerpt: None,
        math: vec![],
        components: vec![],
    };
    assert_eq!(document.render(&plan, None), Err(Error::InvalidPlan));
}

#[test]
fn math_analysis_preserves_non_emitted_evidence_and_obeys_shared_limits() {
    let source = "![label $x$](image.png)\n\n[^unused]: $y$\n";
    let document = Comrak
        .parse(source, Profile::PublicationMath, Limits::default())
        .unwrap();
    assert_eq!(document.syntax().math.len(), 2);
    assert!(
        document
            .syntax()
            .math
            .iter()
            .all(|formula| !formula.emitted)
    );
    assert_eq!(document.check_rendering(), Ok(()));
    let result = Comrak.parse(
        "$abcdef$",
        Profile::PublicationMath,
        Limits {
            fact_bytes: 5,
            ..Limits::default()
        },
    );
    assert!(matches!(result, Err(Error::Limit("fact string bytes"))));
}
