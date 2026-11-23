//! `MathML` uses the shared invariant while retaining visible terms and source TeX.

use super::*;

const FORMULA: &str = "<math xmlns=\"http://www.w3.org/1998/Math/MathML\" display=\"block\"><semantics><mrow><mi>x</mi><mo>+</mo><mspace width=\"1em\"/><mn>1</mn></mrow><annotation encoding=\"application/x-tex\">x + 1</annotation></semantics></math>";

#[test]
fn native_math_keeps_presentation_and_tex_without_a_second_math_profile() {
    let source = document(&format!("<p>Before.</p>{FORMULA}<p>After.</p>"), false);
    let page = parse(source.as_bytes(), "formula", 64 << 10).unwrap();
    let rendered = String::from_utf8(
        page.render("formula", 64 << 10, &BTreeMap::new(), &BTreeMap::new())
            .unwrap(),
    )
    .unwrap();
    for preserved in [
        "Before.",
        "After.",
        "<mi>x</mi>",
        "<mo>+</mo>",
        "<mn>1</mn>",
        "<annotation encoding=\"application/x-tex\">x + 1</annotation>",
        "<mspace width=\"1em\"></mspace>",
    ] {
        assert!(
            rendered.contains(preserved),
            "missing {preserved}: {rendered}"
        );
    }
    assert!(
        page.render("formula", 8, &BTreeMap::new(), &BTreeMap::new())
            .is_err()
    );
}

#[test]
fn malformed_active_or_namespace_changing_math_is_rejected() {
    for bad in [
        FORMULA.replace(
            "http://www.w3.org/1998/Math/MathML",
            "https://invalid.test/math",
        ),
        FORMULA.replace(
            "<mrow>",
            "<mrow xmlns=\"http://www.w3.org/1998/Math/MathML\">",
        ),
        FORMULA.replace("<mi>", "<mi onclick=\"act()\">"),
        FORMULA.replace("<mi>", "<mi href=\"https://invalid.test/\">"),
        FORMULA.replace(
            "<mi>",
            "<mi style=\"background:url(https://invalid.test/)\">",
        ),
        FORMULA.replace("application/x-tex", "text/html"),
        FORMULA.replace("x + 1</annotation>", "</annotation>"),
        FORMULA.replace("</mrow>", "</msup>"),
        FORMULA.replace("</math>", ""),
        FORMULA.replace("<mn>1</mn>", "<script>act()</script>"),
        FORMULA.replace("<mn>1</mn>", "<m:mn>1</m:mn>"),
        FORMULA.replace("width=\"1em\"", "width=\"1em\" width=\"2em\""),
    ] {
        assert!(
            parse(document(&bad, false).as_bytes(), "formula", 64 << 10).is_err(),
            "accepted: {bad}"
        );
    }
}
