//! Literal tooltip data keeps signatures and links without executable output.

use super::*;

const LINK: &str = "<a href=\"#\" class=\"tooltip\" data-notable-ty=\"&amp;[u8]\">ⓘ</a>";
const FRAGMENT: &str = "<h3>Notable traits for <code>&amp;[u8]</code></h3><pre><code><div class=\"where\">impl <a href=\"trait.Read.html\">Read</a> for &amp;[u8]</div>";

fn data(literal: &str) -> String {
    format!("<script type=\"text/json\" id=\"notable-traits-data\">{literal}</script>")
}
fn body(fragment: &str) -> String {
    format!(
        "{LINK}{LINK}{}",
        data(&serde_json::json!({"&[u8]": fragment}).to_string())
    )
}

#[test]
fn repeated_tooltips_share_one_static_section_and_preserve_trait_links() {
    let source = document(&body(FRAGMENT), false);
    let page = parse(source.as_bytes(), "fixture", 64 << 10).unwrap();
    assert!(page.anchors.contains("rustdoc-notable-traits-0"));
    assert_eq!(
        page.anchors
            .iter()
            .filter(|id| id.starts_with("rustdoc-notable-traits-"))
            .count(),
        1
    );
    let links = BTreeMap::from([
        (
            "#rustdoc-notable-traits-0".into(),
            "/api#rustdoc-notable-traits-0".into(),
        ),
        ("trait.Read.html".into(), "/api/trait.Read".into()),
    ]);
    let output = String::from_utf8(
        page.render("fixture", 64 << 10, &links, &BTreeMap::new())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        output
            .matches("href=\"/api&#35;rustdoc-notable-traits-0\"")
            .count(),
        2
    );
    assert!(output.contains("Notable traits for"));
    assert!(output.contains("impl <a href=\"/api/trait.Read\">Read</a> for &amp;&#91;u8&#93;"));
    assert!(!output.contains("<script"));
    assert!(!output.contains("data-notable-ty"));
    assert!(page.render("fixture", 8, &links, &BTreeMap::new()).is_err());
    assert!(parse(source.as_bytes(), "small", source.len() - 1).is_err());
}

#[test]
fn missing_ambiguous_active_or_unbalanced_notable_data_is_refused() {
    let valid = body(FRAGMENT);
    let unknown = serde_json::json!({"other":FRAGMENT}).to_string();
    let duplicate = format!(
        "{{\"&[u8]\":{},\"&[u8]\":{}}}",
        serde_json::to_string(FRAGMENT).unwrap(),
        serde_json::to_string(FRAGMENT).unwrap()
    );
    let mut cases = vec![
        LINK.to_owned(),
        format!("{LINK}{}", data(&unknown)),
        format!("{LINK}{}", data(&duplicate)),
        format!("{valid}{}", data("{}")),
        valid.replace("text/json", "text/javascript"),
        valid.replace("ⓘ", "Extra API meaning"),
        valid.replace("href=\"#\"", "href=\"other.html\""),
        format!("<p id=\"rustdoc-notable-traits-0\">Conflict</p>{valid}"),
        format!("{valid}{LINK}"),
    ];
    for html in [
        "<script>alert(1)</script>",
        "<script type=\"text/json\" id=\"notable-traits-data\">{}</script>",
        "<p onclick=\"act()\">Changed</p>",
        "<p data-self-path=\"crate::Type\">Changed</p>",
        "<pre><code><div>Unclosed",
        "<h1>Replacement page</h1>",
        "<p id=\"rustdoc-notable-traits-0\">Duplicate</p>",
    ] {
        cases.push(body(html));
    }
    for changed in cases {
        assert!(
            parse(document(&changed, false).as_bytes(), "fixture", 64 << 10).is_err(),
            "accepted: {changed}"
        );
    }
}
