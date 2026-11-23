use super::*;
use gazette_core::source::{Precision, Span};

fn lower(html: &str) -> String {
    encode(
        html,
        Limits::default(),
        Location {
            span: Span {
                start: 0,
                end: html.len(),
            },
            precision: Precision::Construct,
        },
    )
    .unwrap()
}

#[test]
fn component_layout_becomes_readable_headings_links_lists_tables_and_code() {
    let html = "<section class=\"card\" data-gazette-props=\"{}\"><h2 id=\"decision\">A <em>decision</em></h2><p>Read <a href=\"https://example.test/guide/\">the guide</a>.</p><details><summary>Why?</summary><p>Because <strong>context</strong> matters.</p><ul><li><p>First</p><ol start=\"2\"><li><p>Nested</p></li></ol></li></ul></details><table><thead><tr><th>A</th><th>B</th></tr></thead><tbody><tr><td>one</td><td><code>x|y</code></td></tr></tbody></table><pre><code class=\"language-rust\">let x = 1;\n\n// second line\n</code></pre><svg aria-hidden=\"true\"><path d=\"M0 0\"/></svg></section>";
    let md = lower(html);
    for part in [
        "## A *decision*",
        "id=\"decision\"",
        "[the guide](https://example.test/guide/)",
        "**Why?**",
        "**context**",
        "First",
        "2.",
        "Nested",
        "| A",
        "one",
        "```rust",
        "let x = 1;\n\n// second line",
    ] {
        assert!(md.contains(part), "missing {part}: {md}");
    }
    for presentation in [
        "<section",
        "<details",
        "<table",
        "<svg",
        "data-gazette",
        "class=",
    ] {
        assert!(!md.contains(presentation), "{md}");
    }
}

#[test]
fn keeps_authored_literals_math_and_image_text() {
    let md = lower(
        "<p>Literal **stars** &amp; &lt;tag&gt; <img src=\"/figure.svg\" alt=\"A → B\" title=\"Comparison\"></p><math xmlns=\"http://www.w3.org/1998/Math/MathML\"><semantics><mi>x</mi><annotation encoding=\"application/x-tex\">x^2</annotation></semantics></math>",
    );
    assert!(md.contains("\\*\\*stars\\*\\*"), "{md}");
    assert!(md.contains("![A → B](/figure.svg \"Comparison\")"), "{md}");
    assert!(md.contains("$x^2$"), "{md}");
    assert!(!md.contains("<math"));
}

#[test]
fn unsupported_visual_meaning_keeps_lossless_transport() {
    let svg = "<svg viewBox=\"0 0 10 10\"><path d=\"M0 0 L10 10\"></path></svg>";
    assert!(lower(svg).contains("viewBox=\"0 0 10 10\""));
}

#[test]
fn keeps_separate_layout_text_and_notebook_execution_and_literal_outputs() {
    let md = lower(
        "<section><div>First thought.</div><div>Second thought.</div><div data-execution-count=\"3\"><textarea>\n\nretained &amp; text\n\nlast</textarea><pre><code>one\n\ntwo\n</code></pre></div></section>",
    );
    assert!(md.contains("First thought.\n\nSecond thought."), "{md}");
    assert!(md.contains("Execution count: 3"), "{md}");
    assert!(md.contains("retained & text\n\nlast"), "{md}");
    assert!(md.contains("one\n\ntwo\n"), "{md}");
}

#[test]
fn inline_svg_pair_uses_the_exact_authored_image_resource_once() {
    let md = lower(
        "<p><span data-gazette-image-ref=\"page#image-1\" role=\"img\" aria-label=\"A diagram\"><svg><text>Geometry</text></svg></span><img hidden data-gazette-image=\"page#image-1\" src=\"/diagram.svg\" alt=\"A diagram\" title=\"Figure\"></p>",
    );
    assert_eq!(md.trim(), "![A diagram](/diagram.svg \"Figure\")");
}

fn read(markdown: &str) -> String {
    let mut options = comrak::Options::default();
    options.render.r#unsafe = true;
    comrak::markdown_to_html(markdown, &options)
}

#[test]
fn merged_table_fallback_keeps_only_the_table_between_readable_sections() {
    let md = lower(
        "<section class=\"process\" data-gazette-component=\"process\"><h2 id=\"steps\">A description</h2><ol><li><h3>Choose</h3><p>Ask the question.</p></li></ol><table class=\"comparison\"><thead><tr><th>Available</th><th>Stock</th><th>Repair?</th></tr></thead><tbody><tr><th><code>(1,0)</code></th><td rowspan=\"2\"><code>a = 1</code></td><td>No</td></tr><tr><th><code>(1,1)</code></th><td>Yes</td></tr></tbody></table><p>Keep both answers.</p></section>",
    );
    for part in [
        "## A description",
        "### Choose",
        "Ask the question.",
        "<table class=\"comparison\">",
        "rowspan=\"2\"",
        "<code>a = 1</code>",
        "<td>No</td>",
        "<td>Yes</td>",
        "Keep both answers.",
    ] {
        assert!(md.contains(part), "{part}: {md}");
    }
    assert_eq!(md.matches("<table").count(), 1);
    assert_eq!(md.matches("a = 1").count(), 1);
    assert!(!md.contains("<section"), "{md}");
    assert!(!md.contains("data-gazette"), "{md}");
    assert!(
        !md.contains("| Available"),
        "discard the failed table prefix: {md}"
    );
}

#[test]
fn figure_captions_and_transparent_wrappers_keep_block_children() {
    let md = lower(
        "<figure class=\"plate\"><img src=\"/world.webp\" alt=\"A green world\"><figcaption><h3 id=\"world\">Other worlds</h3><p>Names and rituals survive.</p></figcaption></figure><span><p>Another thought.</p></span>",
    );
    for part in [
        "![A green world](/world.webp)",
        "### Other worlds",
        "id=\"world\"",
        "Names and rituals survive.",
        "Another thought.",
    ] {
        assert!(md.contains(part), "{part}: {md}");
    }
    for tag in ["<figure", "<figcaption", "<span", "<p>"] {
        assert!(!md.contains(tag), "{md}");
    }
}

#[test]
fn empty_carousel_output_does_not_revert_faq_or_catalogue_links() {
    let md = lower(
        "<section data-gazette-component=\"inquiry\"><h2>Questions</h2><details><summary>Where?</summary><div><p>Read <a data-gazette-link=\"page#link-1\" href=\"/guide/\">the guide</a>.</p></div></details><nav hidden><button><svg aria-hidden=\"true\"><path d=\"M0 0\"/></svg></button><output aria-live=\"polite\"></output></nav><output>Three answers</output></section>",
    );
    for part in [
        "## Questions",
        "**Where?**",
        "[the guide](/guide/)",
        "Three answers",
    ] {
        assert!(md.contains(part), "{part}: {md}");
    }
    for tag in [
        "<section",
        "<details",
        "<output",
        "data-gazette",
        "aria-live",
    ] {
        assert!(!md.contains(tag), "{md}");
    }
}

#[test]
fn html_flow_whitespace_keeps_inline_separation_and_non_ascii_spaces() {
    let md = lower(
        "<ul>\n  <li>\n <a href=\"/first/\">First</a>\n\t <a href=\"/second/\">Second</a>\n </li>\n</ul><p>before <em>inside</em> after&nbsp;thin&#8201;space</p><p>&nbsp;</p>",
    );
    assert!(!md.contains("&#10;"), "{md}");
    assert!(md.contains("[First](/first/) [Second](/second/)"), "{md}");
    assert!(
        md.contains("before *inside* after\u{a0}thin\u{2009}space"),
        "{md}"
    );
    assert_eq!(md.matches('\u{a0}').count(), 2, "standalone NBSP survives");
}

#[test]
fn flow_normalization_does_not_decode_literal_entities_code_or_math() {
    let md = lower(
        "<p>Literal &amp;#10; and <code>&amp;#10;</code>.</p><pre><code>one\n\n&amp;#10;\n  two\n</code></pre><textarea>first\n\nlast</textarea><math xmlns=\"http://www.w3.org/1998/Math/MathML\"><semantics><mi>x</mi><annotation encoding=\"application/x-tex\">x\n + y</annotation></semantics></math>",
    );
    let html = read(&md);
    assert!(html.contains("Literal &amp;#10;"), "{md}: {html}");
    assert!(html.contains("<code>&amp;#10;</code>"), "{md}: {html}");
    assert!(html.contains("one\n\n&amp;#10;\n  two\n"), "{md}: {html}");
    assert!(md.contains("first\n\nlast"), "{md}");
    assert!(md.contains("$x\n + y$"), "{md}");
}

#[test]
fn foreign_subtree_fallback_preserves_its_geometry_without_ancestor_wrappers() {
    let html = "<section class=\"card\"><p>Before <svg viewBox=\"0 0 10 10\"><path d=\"M0 0 L10 10\"></path></svg> after.</p><p>Continue reading.</p></section>";
    let md = lower(html);
    assert!(md.contains("Before <svg viewBox=\"0 0 10 10\">"), "{md}");
    assert!(md.contains("d=\"M0 0 L10 10\""), "{md}");
    assert!(md.contains("</svg> after."), "{md}");
    assert!(md.contains("Continue reading."), "{md}");
    assert!(!md.contains("<section"), "{md}");
    let tree = html_tree::parse("<p id=\"long\">retained</p>", Limits::default()).unwrap();
    let children = tree.root.children.borrow();
    assert!(matches!(
        crate::html_transport::encode_node(
            &children[0],
            Limits {
                output_bytes: 8,
                ..Limits::default()
            },
            Location {
                span: Span {
                    start: 0,
                    end: html.len()
                },
                precision: Precision::Construct
            }
        ),
        Err(Error::Limit(_))
    ));
}

#[test]
fn inline_foreign_fallback_keeps_markdown_punctuation_as_literal_text() {
    let md = lower(
        "<p>Before <svg viewBox=\"0 0 10 10\"><text>**literal** [link](url) `code` &amp;#10;</text></svg> after.</p>",
    );
    let html = read(&md);
    assert!(
        html.contains("**literal** [link](url) `code` &amp;#10;"),
        "{md}: {html}"
    );
    assert!(!html.contains("<strong>"), "{html}");
    assert!(!html.contains("href="), "{html}");
    assert!(html.contains("viewBox=\"0 0 10 10\""), "{html}");
    assert!(html.contains("Before <svg"), "{html}");
    assert!(html.contains("</svg> after."), "{html}");
}
