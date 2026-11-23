//! Closed generated-MDX decoding preserves literal text and refuses executable syntax.

use gazette_compiler::generated_mdx::{Error, Limits, decode};

fn limits() -> Limits {
    Limits {
        input_bytes: 65536,
        output_bytes: 65536,
        tokens: 1000,
        depth: 32,
    }
}

fn page(body: &str) -> Vec<u8> {
    format!("---\ntitle: \"Example<T>\"\n---\n\n{body}\n").into_bytes()
}

#[test]
fn generated_mdx_preserves_literal_code_and_link_anchor_identity() {
    let literal = "pub fn f<'a>() {\n\t\"<script>${value} & &#10; \\\"\"\r\n}\u{a0}λ";
    let body = format!(
        "<h1><span children={{\"Example<T>\"}} /></h1><pre id=\"source\"><code><span children={{{}}} /></code></pre><p>See <a href=\"/rust/other#member\">Other</a>.</p>",
        serde_json::to_string(literal).unwrap()
    );
    let decoded = decode(&page(&body), limits()).unwrap();
    assert_eq!(decoded.title, "Example<T>");
    let mut texts = Vec::new();
    let mut attrs = Vec::new();
    gazette_compiler::generated_mdx::inspect(&decoded, Default::default(), |kind, value| {
        match kind {
            gazette_compiler::html::TextKind::Text => texts.push(value.to_owned()),
            gazette_compiler::html::TextKind::Attribute => attrs.push(value.to_owned()),
            _ => panic!("generated grammar must stay inert"),
        }
        Ok::<_, std::convert::Infallible>(())
    })
    .unwrap();
    assert!(texts.iter().any(|text| text == literal), "{texts:?}");
    assert_eq!(attrs, ["source", "/rust/other#member"]);
}

#[test]
fn generated_mdx_refuses_general_mdx_and_every_nonliteral_carrier() {
    for body in [
        "import Thing from './x'",
        "export const data = 1",
        "# Heading",
        "<Component />",
        "<script>act()</script>",
        "<p onload=\"act()\">x</p>",
        "<p title={value}>x</p>",
        "<p {...value}>x</p>",
        "<p>{value}</p>",
        "<p><span children={\"text\"} /></p>",
        "<pre><span children={value} /></pre>",
        "<pre><span children={`text`} /></pre>",
        "<pre><span children={1} /></pre>",
        "<pre><span children={true} /></pre>",
        "<pre><span children={{}} /></pre>",
        "<pre><span children={\"a\" + \"b\"} /></pre>",
        "<pre><span children={\"a\"; act()} /></pre>",
        "<pre><span children={\"a\"} onclick=\"act()\" /></pre>",
        "<pre><span children={\"unterminated} /></pre>",
        "<pre><span children={\"a\"} /></code>",
        "<p id=\"a\" id=\"b\">text</p>",
        "<a href=\"javascript:act()\">text</a>",
        "<a href=\"&#106;avascript:act()\">text</a>",
        "<a href=\"//unselected.test/\">text</a>",
    ] {
        assert!(decode(&page(body), limits()).is_err(), "{body}");
    }
    assert!(decode(b"---\ntitle: \"x\"\nextra: true\n---\n\n<p>x</p>", limits()).is_err());
}

#[test]
fn generated_mdx_exact_resource_boundaries_return_no_partial_result() {
    let source = page("<pre><span children={\"123\"} /></pre>");
    let decoded = decode(&source, limits()).unwrap();
    let mut exact = limits();
    exact.input_bytes = source.len();
    exact.output_bytes = decoded.decoded_bytes();
    exact.tokens = 4;
    exact.depth = 2;
    decode(&source, exact).unwrap();
    for field in 0..4 {
        let mut bounded = exact;
        match field {
            0 => bounded.input_bytes -= 1,
            1 => bounded.output_bytes -= 1,
            2 => bounded.tokens -= 1,
            _ => bounded.depth -= 1,
        }
        assert!(
            matches!(decode(&source, bounded), Err(Error::Limit(_))),
            "{field}"
        );
    }
}
