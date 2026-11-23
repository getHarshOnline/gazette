use super::*;
#[test]
fn target_is_a_canonical_public_host_not_a_preview_or_article_url() {
    assert_eq!(
        destination(&"https://news.example.com/".parse().unwrap()).as_deref(),
        Some("https://www.google.com/preferences/source?q=news.example.com")
    );
    for base in [
        "http://127.0.0.1:4310/",
        "http://localhost/",
        "https://example.com/articles/",
        "https://example.com:444/",
    ] {
        assert!(destination(&base.parse().unwrap()).is_none(), "{base}");
    }
}
