//! Public address normalization and native output spelling remain distinct.

use gazette_core::publication::routing::{PublicBaseUrl, RoutePath};
use gazette_core::source::EntryPath;

#[test]
fn resource_references_resolve_from_emitted_file_within_the_public_mount() {
    let base: PublicBaseUrl = "https://example.test/mount/".parse().expect("base");
    let origin: RoutePath = "/styles/main.css".parse().expect("emitted file");
    for reference in [
        "../images/caf%C3%A9.png",
        "/mount/images/caf%C3%A9.png",
        "https://example.test/mount/images/caf%C3%A9.png",
    ] {
        let target = base
            .resource_target(&origin, reference)
            .expect("same output");
        assert_eq!(target.output().as_str(), "images/café.png");
        assert_eq!(
            base.resolve(&target),
            "https://example.test/mount/images/caf%C3%A9.png"
        );
    }
}

#[test]
fn resource_references_do_not_escape_or_ambiguously_address_output_files() {
    let base: PublicBaseUrl = "https://example.test/mount/".parse().expect("base");
    let origin: RoutePath = "/styles/main.css".parse().expect("file");
    for reference in [
        "",
        " ",
        "a\nb.png",
        "a\\b.png",
        "../images/",
        "../../outside.png",
        "/mount-other/a.png",
        "/a.png",
        "https://other.test/mount/a.png",
        "https://user@example.test/mount/a.png",
        "//other.test/a.png",
        "data:image/png;base64,AA==",
        "file:///mount/a.png",
        "#image",
        "a.png?version=1",
        "a.png#fragment",
        "a%2fb.png",
        "a%252e.png",
        "%ff.png",
        "%xx.png",
    ] {
        assert!(
            base.resource_target(&origin, reference).is_err(),
            "{reference:?}"
        );
    }
}

#[test]
fn file_join_uses_the_same_literal_codec_without_adding_a_directory_index() {
    for (prefix, name, url, output) in [
        ("/", "download.pdf", "/download.pdf", "download.pdf"),
        (
            "/media/",
            "nested/image.png",
            "/media/nested/image.png",
            "media/nested/image.png",
        ),
        (
            "/caf%C3%A9/",
            "使用/first image.PNG",
            "/caf%C3%A9/%E4%BD%BF%E7%94%A8/first%20image.PNG",
            "café/使用/first image.PNG",
        ),
    ] {
        let base: RoutePath = prefix.parse().expect("directory mount");
        let entry: EntryPath = name.parse().expect("literal retained file");
        let route = base.join_file(&entry).expect("file placement");
        assert_eq!(route.as_str(), url);
        assert_eq!(route.output().as_str(), output);
        assert_eq!(route, url.parse().expect("same route codec"));
    }
    let directory: RoutePath = "/media/".parse().expect("directory");
    for name in ["literal%61.png", "query?.pdf", "fragment#.png"] {
        assert!(
            directory
                .join_file(&name.parse().expect("literal name"))
                .is_err()
        );
    }
    let file: RoutePath = "/file.html".parse().expect("file route");
    assert!(
        file.join_file(&"image.png".parse().expect("entry"))
            .is_err()
    );
}

#[test]
fn directory_join_encodes_literal_components_and_preserves_mounts() {
    for (prefix, entry, url, output) in [
        ("/", "guide", "/guide/", "guide/index.html"),
        (
            "/docs/",
            "nested/setup",
            "/docs/nested/setup/",
            "docs/nested/setup/index.html",
        ),
        (
            "/caf%C3%A9/",
            "使用/first steps",
            "/caf%C3%A9/%E4%BD%BF%E7%94%A8/first%20steps/",
            "café/使用/first steps/index.html",
        ),
        (
            "/docs/",
            "a'\"<>/v1.0_~",
            "/docs/a%27%22%3C%3E/v1.0_~/",
            "docs/a'\"<>/v1.0_~/index.html",
        ),
    ] {
        let prefix: RoutePath = prefix.parse().expect("directory prefix");
        let entry: EntryPath = entry.parse().expect("literal entry");
        let joined = prefix.join_directory(Some(&entry)).expect("directory join");
        assert_eq!(joined.as_str(), url);
        assert_eq!(joined.output().as_str(), output);
        assert_eq!(joined, url.parse().expect("existing route codec"));
    }
}

#[test]
fn directory_join_without_entry_preserves_exact_normalized_route() {
    for value in ["/", "/docs/", "/caf%c3%a9/"] {
        let prefix: RoutePath = value.parse().expect("directory");
        assert_eq!(
            prefix.join_directory(None).expect("unchanged directory"),
            prefix
        );
    }
}

#[test]
fn directory_join_never_interprets_native_names_as_url_syntax() {
    let prefix: RoutePath = "/docs/".parse().expect("directory");
    for value in [
        "percent%",
        "%61",
        "caf%C3%A9",
        "query?value",
        "fragment#name",
    ] {
        let entry: EntryPath = value.parse().expect("valid literal filesystem name");
        assert!(prefix.join_directory(Some(&entry)).is_err(), "{value:?}");
    }
    // The URL parser accepts encoded letters. Passing native `%61` above must not
    // accidentally take this distinct parsing route and become the file name `a`.
    assert_eq!(
        "/%61/".parse::<RoutePath>().expect("URL escape").as_str(),
        "/a/"
    );
    for value in ["../escape", "a/../escape", "./a", "/absolute", "a//b"] {
        assert!(value.parse::<EntryPath>().is_err(), "{value:?}");
    }
}

#[test]
fn directory_join_rejects_file_prefix_even_without_an_entry() {
    let entry: EntryPath = "child".parse().expect("entry");
    for value in ["/file", "/index.html", "/nested/file"] {
        let prefix: RoutePath = value.parse().expect("file route");
        assert!(prefix.join_directory(Some(&entry)).is_err());
        assert!(prefix.join_directory(None).is_err());
    }
}

#[test]
fn roots_files_unicode_and_safe_escapes_have_one_spelling() {
    for (input, url, output) in [
        ("/", "/", "index.html"),
        ("/guide/", "/guide/", "guide/index.html"),
        ("/guide", "/guide", "guide"),
        ("/feed.xml", "/feed.xml", "feed.xml"),
        ("/caf%C3%a9/", "/caf%C3%A9/", "café/index.html"),
        ("/café/", "/caf%C3%A9/", "café/index.html"),
        ("/%61 b/", "/a%20b/", "a b/index.html"),
        ("/a'\"<>/", "/a%27%22%3C%3E/", "a'\"<>/index.html"),
    ] {
        let path: RoutePath = input.parse().expect(input);
        assert_eq!(path.as_str(), url);
        assert_eq!(path.output().as_str(), output);
        assert_eq!(serde_json::to_value(&path).expect("serialize"), url);
        assert_eq!(
            serde_json::from_value::<RoutePath>(url.into()).expect("roundtrip"),
            path
        );
    }
}

#[test]
fn ambiguous_paths_are_rejected_before_url_normalization() {
    for value in [
        "", "a", "//host/", "/a//b", "/./", "/a/../b", "/%2e/", "/%2e%2e/", "/a%2fb", "/a%5cb",
        "/a\\b", "/%252e/", "/%", "/%0", "/%xx", "/%FF", "/%00", "/a\nb", "/a?b", "/a%3Fb", "/a#b",
        "/a%23b",
    ] {
        assert!(value.parse::<RoutePath>().is_err(), "{value:?}");
    }
}

#[test]
fn public_base_preserves_mount_and_normalizes_only_url_components() {
    for (input, expected) in [
        ("https://EXAMPLE.test:443", "https://example.test/"),
        ("http://[::1]:8080/project/", "http://[::1]:8080/project/"),
        (
            "https://example.test/caf%c3%a9/",
            "https://example.test/caf%C3%A9/",
        ),
    ] {
        let base: PublicBaseUrl = input.parse().expect(input);
        assert_eq!(base.as_str(), expected);
        assert_eq!(base.resolve(&"/".parse().expect("root")), expected);
        assert_eq!(
            base.resolve_output(&"/".parse().expect("root")),
            format!("{expected}index.html")
        );
        assert_eq!(
            base.resolve_output(&"/file.xml".parse().expect("file")),
            format!("{expected}file.xml")
        );
        assert_eq!(
            base.resolve(&"/file.xml".parse().expect("file")),
            format!("{expected}file.xml")
        );
        assert_eq!(serde_json::to_value(&base).expect("serialize"), expected);
    }
}

#[test]
fn invalid_bases_do_not_silently_drop_components() {
    for value in [
        "/",
        "//example.test/",
        "file:///tmp/",
        "ftp://example.test/",
        "https:example.test",
        "https:///example.test/",
        "https://user@example.test/",
        "https://@example.test/",
        "https://example.test/path",
        "https://example.test/?a",
        "https://example.test/#",
        "https://example.test/../",
        "https://example.test/%2e/",
        "https://example.test//",
        " https://example.test/",
        "https://example.test/\n",
        "https://example.test\\evil/",
    ] {
        assert!(value.parse::<PublicBaseUrl>().is_err(), "{value:?}");
    }
}
