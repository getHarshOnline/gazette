//! Native capture-relative resolution and exact output encoding, without acquisition.

use super::{Destination, resolve, url};

fn local(origin: &str, href: &str, expected: &str, anchor: Option<&str>) {
    match resolve(origin, href).expect("admitted retained reference") {
        Destination::Local { path, fragment } => {
            assert_eq!(path.as_str(), expected);
            assert_eq!(fragment.as_deref(), anchor);
        }
        Destination::External(_) => panic!("local reference became external"),
    }
}

#[test]
fn relative_module_and_source_links_resolve_inside_the_retained_tree() {
    let origin = "library/module/struct.Client.html";
    for (href, expected, fragment) in [
        (
            "trait.Transport.html",
            "library/module/trait.Transport.html",
            None,
        ),
        ("./index.html", "library/module/index.html", None),
        ("../index.html", "library/index.html", None),
        (
            "../../src/library/lib.rs.html#12-24",
            "src/library/lib.rs.html",
            Some("12-24"),
        ),
        (
            "../encoded%20name/struct.%CE%A9.html",
            "library/encoded name/struct.Ω.html",
            None,
        ),
    ] {
        local(origin, href, expected, fragment);
    }
}

#[test]
fn fragment_only_links_keep_the_exact_current_document_and_anchor() {
    let origin = "library/struct.Client.html";
    local(origin, "#method.request", origin, Some("method.request"));
    local(origin, "", origin, None);
    local(origin, "#", origin, None);
    let literal = "impl-From%3CClient%3E-for-Wrapper";
    local(origin, &format!("#{literal}"), origin, Some(literal));
}

#[test]
fn output_urls_encode_literal_impl_ids_without_changing_the_anchor_identity() {
    assert_eq!(
        url(
            "library/struct.Client",
            Some("impl-From%3CClient%3E-for-Wrapper")
        ),
        "/library/struct.Client#impl-From%253CClient%253E-for-Wrapper"
    );
    assert_eq!(
        url("library/encoded name/struct.Ω", Some("impl<T>/From<T>")),
        "/library/encoded%20name/struct.%CE%A9#impl%3CT%3E%2FFrom%3CT%3E"
    );
    assert_eq!(url("library/index", None), "/library/index");
}

#[test]
fn external_references_are_exact_destinations_not_fetches_or_capture_paths() {
    for href in [
        "https://unresolvable.invalid/Library/struct.Item.html#method.new",
        "http://unresolvable.invalid/reference",
    ] {
        match resolve("library/index.html", href).expect("pure external classification") {
            Destination::External(actual) => assert_eq!(actual, href),
            Destination::Local { .. } => panic!("external reference became captured input"),
        }
    }
}

#[test]
fn escaping_ambiguous_and_credential_bearing_references_are_refused() {
    for href in [
        "../../../outside.html",
        "../../%2e%2e/outside.html",
        "module%2Fitem.html",
        "module%2fitem.html",
        "module%5Citem.html",
        "module\\item.html",
        "module//item.html",
        "/outside.html",
        "//example.test/item.html",
        "item.html?search=Client",
        "javascript:alert(1)",
        "file:///tmp/item.html",
        "mailto:owner@example.test",
        "ftp://example.test/item.html",
        "https://owner@example.test/item.html",
        "https://owner:secret@example.test/item.html",
        "https://example.test/with space",
        "item%FF.html",
    ] {
        assert!(
            resolve("library/module/index.html", href).is_err(),
            "accepted {href:?}"
        );
    }
}
