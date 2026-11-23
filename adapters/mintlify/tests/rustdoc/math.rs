//! Installed Rustdoc retains a formula through the existing target and Markdown parser.

use super::*;

#[test]
fn installed_rustdoc_preserves_native_math_and_its_annotation() {
    let directory = tempfile::tempdir().unwrap();
    let source = "//! Native formula reference.\n/// A preserved formula.\n/// <math xmlns=\"http://www.w3.org/1998/Math/MathML\" display=\"block\"><semantics><mrow><mi>x</mi><mo>+</mo><mn>1</mn></mrow><annotation encoding=\"application/x-tex\">x + 1</annotation></semantics></math>\npub struct Formula;\n";
    let program = std::env::var_os("RUSTDOC").unwrap_or_else(|| "rustdoc".into());
    generate_source(directory.path(), Path::new(&program), source);
    let input = capture(&directory.path().join("html"));
    let output =
        assemble(&input).expect("native formula is preserved through shared MathML validation");
    let observed = observe_markdown(page(&output, "fixture/rust/struct.Formula.md"));
    assert!(observed.tags.contains("math"));
    assert!(observed.tags.contains("annotation"));
    assert!(observed.text.contains("A preserved formula."));
    assert!(observed.text.contains("x + 1"));
    assert_eq!(
        output.inventory().identity(),
        assemble(&input).unwrap().inventory().identity()
    );
}
