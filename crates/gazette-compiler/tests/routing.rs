//! Explicit local routing through retained authored input, not publication admission.

use gazette_compiler::{
    authored::{self, Error},
    corpus::{self, Input, ParsedCorpus},
    toml::{self, PathSegment},
    workspace,
};
use gazette_core::publication::{ContentLifecycle, Key, content};

#[path = "routing/mirrors.rs"]
mod mirrors;

fn key(value: &str) -> Key {
    value.parse().expect("fixture identity")
}

fn configuration(alpha: Option<&str>, beta: Option<&str>) -> String {
    let mut source = String::from(
        "[[principals]]\nid='owner:publisher'\nkind='person'\n\
         [[sites]]\nid='site:alpha'\npublisher='owner:publisher'\ndeployable=false\n\
         [[sites]]\nid='site:beta'\npublisher='owner:publisher'\ndeployable=false\n\
         [[brands]]\nid='brand:one'\nname='Explicit publication settings'\n",
    );
    for (site, base) in [("site:alpha", alpha), ("site:beta", beta)] {
        source.push_str(&format!("[[brands.publications]]\nsite='{site}'\n"));
        if let Some(base) = base {
            source.push_str(&format!("public_base_url='{base}'\n"));
        }
    }
    source
}

fn document(id: &str, site: &str, route: &str) -> String {
    format!(
        "+++\n[content]\nid='{id}'\nsite='{site}'\ntitle='Not a route résumé'\n\
         lifecycle='draft'\n{route}\n\
         structure={{kind='page',family={{kind='documentation'}}}}\n+++\n# Retained body\n"
    )
}

fn corpus<'a>(files: &[(&str, &'a str)]) -> ParsedCorpus<'a> {
    ParsedCorpus::parse(
        files.iter().map(|(path, source)| Input {
            path: path.parse().expect("fixture logical path"),
            bytes: source.as_bytes(),
        }),
        corpus::Limits::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("actual fenced Markdown and TOML")
}

fn error_for(source: &str, files: &[(&str, &str)], limits: content::Limits) -> Error {
    let workspace = workspace::compile(source, toml::Limits::default()).expect("workspace");
    let corpus = corpus(files);
    authored::resolve(&workspace, &corpus, limits).expect_err("complete routing must fail")
}

fn assert_diagnostic(error: Error, code: &str) {
    match error {
        Error::Structure {
            error: content::Error::Invalid(diagnostics),
            declarations,
        } => {
            assert!(diagnostics.iter().any(|diagnostic| diagnostic.code == code));
            assert!(!declarations.is_empty(), "retain original declarations");
        }
        other => panic!("expected {code}, got {other:?}"),
    }
}

#[test]
fn external_redirects_bind_released_directory_routes_to_independent_https_sites() {
    let mut source = configuration(Some("https://alpha.example/"), None);
    source.push_str("public_base_url='https://beta.example/'\n");
    let with_redirect = |path, target| {
        format!("{source}[brands.publications.external_redirects]\n'{path}'='{target}'\n")
    };
    let compiled = workspace::compile(
        &with_redirect("/old/", "https://community.example/"),
        toml::Limits::default(),
    )
    .expect("external publication redirect");
    let redirects = &compiled
        .workspace
        .publication_for(&key("site:beta"))
        .unwrap()
        .external_redirects;
    let (route, target) = redirects.first_key_value().unwrap();
    assert_eq!(route.output().as_str(), "old/index.html");
    assert_eq!(target.as_str(), "https://community.example/");

    for (path, target) in [
        ("/", "https://community.example/"),
        ("/old", "https://community.example/"),
        ("/old/", "http://community.example/"),
        ("/old/", "https://beta.example/another/"),
    ] {
        assert!(
            workspace::compile(&with_redirect(path, target), toml::Limits::default()).is_err(),
            "{path} → {target}",
        );
    }
}

#[test]
fn brand_base_subpath_and_explicit_route_feed_the_same_borrowed_page_record() {
    let source = configuration(Some("https://alpha.example/publication/"), None);
    let workspace = workspace::compile(&source, toml::Limits::default()).expect("workspace");
    assert!(
        workspace
            .workspace
            .public_base_url(&key("site:alpha"))
            .is_some()
    );
    assert!(
        workspace
            .workspace
            .public_base_url(&key("site:beta"))
            .is_none()
    );
    assert!(
        workspace
            .workspace
            .public_base_url(&key("site:missing"))
            .is_none()
    );
    let document = document(
        "content:guide",
        "site:alpha",
        "route={path='/guide/',aliases=['/z-old/','/a-old.html']}",
    );
    let corpus = corpus(&[("unrelated/source-name.md", &document)]);
    let resolved =
        authored::resolve(&workspace, &corpus, content::Limits::default()).expect("routes");
    let id = key("content:guide");
    let route = resolved.content().route(&id).expect("explicit route");
    assert_eq!(
        route.canonical_url,
        "https://alpha.example/publication/guide/"
    );
    assert_eq!(route.output.as_str(), "guide/index.html");
    assert_eq!(
        route
            .aliases
            .iter()
            .map(|alias| (alias.url.as_str(), alias.output.as_str()))
            .collect::<Vec<_>>(),
        [
            ("https://alpha.example/publication/a-old.html", "a-old.html"),
            (
                "https://alpha.example/publication/z-old/",
                "z-old/index.html"
            ),
        ]
    );
    let page = resolved
        .project_page(
            &id,
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("real body and graph projection");
    assert!(std::ptr::eq(page.data.route.expect("same route"), route));
    assert_eq!(page.data.content.lifecycle, ContentLifecycle::Draft);
    assert_eq!(
        page.body_html,
        "<h1 id=\"retained-body\">Retained body</h1>\n"
    );
    let machine = serde_json::to_value(&page).expect("shared machine projection");
    assert_eq!(
        machine["route"],
        serde_json::to_value(route).expect("resolved route")
    );
    assert_eq!(machine["content"]["lifecycle"], "draft");
    let origin = resolved
        .origin(
            &id,
            &[
                PathSegment::Key("content".into()),
                PathSegment::Key("route".into()),
                PathSegment::Key("path".into()),
            ],
        )
        .expect("exact authored route origin");
    let span = origin.span.expect("explicit token span");
    assert_eq!(&document[span.start..span.end], "'/guide/'");
    assert_eq!(origin.path.as_str(), "unrelated/source-name.md");
}

#[test]
fn no_route_is_invented_from_title_source_name_or_site_base() {
    let source = configuration(Some("https://alpha.example/"), None);
    let workspace = workspace::compile(&source, toml::Limits::default()).expect("workspace");
    let plain = document("content:plain", "site:alpha", "");
    let corpus = corpus(&[("looks-like-a-route/index.md", &plain)]);
    let resolved = authored::resolve(&workspace, &corpus, content::Limits::default())
        .expect("unrouted content");
    assert!(resolved.content().route(&key("content:plain")).is_none());
    assert!(resolved.content().route(&key("content:missing")).is_none());
    assert!(
        resolved
            .project_page(
                &key("content:plain"),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak
            )
            .expect("local page")
            .data
            .route
            .is_none()
    );
}

#[test]
fn missing_base_and_unknown_site_fail_with_native_declaration_evidence() {
    let configuration = configuration(None, None);
    for (site, code) in [
        ("site:alpha", "content_route_base"),
        ("site:missing", "content_site_reference"),
    ] {
        let document = document("content:guide", site, "route={path='/guide/'}");
        assert_diagnostic(
            error_for(
                &configuration,
                &[("guide.md", &document)],
                content::Limits::default(),
            ),
            code,
        );
    }
}

#[test]
fn aliases_and_input_order_resolve_deterministically_without_changing_identity() {
    let source = configuration(
        Some("https://alpha.example/"),
        Some("https://beta.example/"),
    );
    let workspace = workspace::compile(&source, toml::Limits::default()).expect("workspace");
    let first = document(
        "content:one",
        "site:alpha",
        "route={path='/one/',aliases=['/z/','/a.html']}",
    );
    let reordered = first.replace("['/z/','/a.html']", "['/a.html','/z/']");
    let second = document("content:two", "site:beta", "route={path='/one/'}");
    let a = corpus(&[("one.md", &first), ("two.md", &second)]);
    let b = corpus(&[("two.md", &second), ("one.md", &reordered)]);
    let a = authored::resolve(&workspace, &a, content::Limits::default()).expect("first order");
    let b = authored::resolve(&workspace, &b, content::Limits::default()).expect("second order");
    for id in [key("content:one"), key("content:two")] {
        assert_eq!(
            serde_json::to_value(a.content().route(&id)).expect("first route projection"),
            serde_json::to_value(b.content().route(&id)).expect("reordered route projection")
        );
        assert_eq!(
            a.project_page(
                &id,
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak
            )
            .expect("retained page")
            .data
            .content
            .id,
            id
        );
    }
    assert_eq!(
        a.content()
            .route(&key("content:two"))
            .expect("second site route")
            .canonical_url,
        "https://beta.example/one/"
    );
    let moved = first.replace("path='/one/'", "path='/moved/'");
    let moved_corpus = corpus(&[("different-file.md", &moved)]);
    let moved = authored::resolve(&workspace, &moved_corpus, content::Limits::default())
        .expect("explicit move");
    assert_eq!(
        moved
            .project_page(
                &key("content:one"),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak
            )
            .expect("moved page")
            .data
            .content
            .id,
        key("content:one")
    );
    assert_eq!(
        moved
            .content()
            .route(&key("content:one"))
            .expect("moved route")
            .canonical_url,
        "https://alpha.example/moved/"
    );
}

#[test]
fn all_primary_alias_and_file_directory_collisions_fail_including_drafts() {
    let source = configuration(
        Some("https://alpha.example/"),
        Some("https://beta.example/"),
    );
    for (first_route, second_route) in [
        ("route={path='/a/'}", "route={path='/a/index.html'}"),
        ("route={path='/a.html'}", "route={path='/a.html/child/'}"),
        (
            "route={path='/a/',aliases=['/old/']}",
            "route={path='/old/'}",
        ),
        (
            "route={path='/a/',aliases=['/old/']}",
            "route={path='/b/',aliases=['/old/index.html']}",
        ),
        (
            "route={path='/a/',aliases=['/old/','/old/']}",
            "route={path='/b/'}",
        ),
        ("route={path='/a/',aliases=['/a/']}", "route={path='/b/'}"),
    ] {
        let first = document("content:first", "site:alpha", first_route);
        let second = document("content:second", "site:alpha", second_route);
        assert_diagnostic(
            error_for(
                &source,
                &[("first.md", &first), ("second.md", &second)],
                content::Limits::default(),
            ),
            "content_route_collision",
        );
    }
}

#[test]
fn different_site_keys_cannot_claim_the_same_canonical_address() {
    let first = document("content:first", "site:alpha", "route={path='/a/'}");
    let second = document("content:second", "site:beta", "route={path='/a/'}");
    let source = configuration(
        Some("https://shared.example/base/"),
        Some("https://shared.example/base/"),
    );
    assert_diagnostic(
        error_for(
            &source,
            &[("first.md", &first), ("second.md", &second)],
            content::Limits::default(),
        ),
        "content_route_collision",
    );
}

#[test]
fn resolved_url_expansion_is_bounded_separately_from_small_authored_text() {
    let long_base = format!("https://alpha.example/{}/", "mount".repeat(256));
    let source = configuration(Some(&long_base), None);
    let document = document(
        "content:one",
        "site:alpha",
        "route={path='/a/',aliases=['/old/']}",
    );
    let error = error_for(
        &source,
        &[("one.md", &document)],
        content::Limits {
            text_bytes: 512,
            ..content::Limits::default()
        },
    );
    match error {
        Error::Structure {
            error: content::Error::Limit(resource),
            declarations,
        } => {
            assert_eq!(resource, "route text bytes");
            assert_eq!(declarations[&key("content:one")].len(), 1);
        }
        other => panic!("expected bounded URL expansion, got {other:?}"),
    }
}

#[test]
fn decoded_equivalent_routes_collide_and_unsafe_paths_fail_at_authored_schema() {
    let source = configuration(Some("https://alpha.example/"), None);
    let first = document("content:first", "site:alpha", "route={path='/caf%C3%A9/'}");
    let second = document("content:second", "site:alpha", "route={path='/café/'}");
    assert_diagnostic(
        error_for(
            &source,
            &[("first.md", &first), ("second.md", &second)],
            content::Limits::default(),
        ),
        "content_route_collision",
    );
    for path in [
        "relative/",
        "/../escape/",
        "/%2e%2e/escape/",
        "/a%2fb/",
        "/a?query/",
        "/a#fragment/",
        "/%252e/",
        "//other.example/",
    ] {
        let document = document(
            "content:invalid",
            "site:alpha",
            &format!("route={{path='{path}'}}"),
        );
        match error_for(
            &source,
            &[("invalid.md", &document)],
            content::Limits::default(),
        ) {
            Error::Files(errors) => {
                assert_eq!(errors.len(), 1, "{path}");
                assert_eq!(errors[0].origin.path.as_str(), "invalid.md");
                assert!(
                    matches!(errors[0].error, authored::FileError::Metadata(_)),
                    "{path}"
                );
            }
            other => panic!("unsafe route {path} must fail its typed source: {other:?}"),
        }
    }
}

#[test]
fn root_and_container_routes_are_explicit_not_limited_to_leaf_articles() {
    let source = configuration(Some("https://alpha.example/prefix/"), None);
    let workspace = workspace::compile(&source, toml::Limits::default()).expect("workspace");
    let collection = document("collection:home", "site:alpha", "route={path='/'}").replace(
        "structure={kind='page',family={kind='documentation'}}",
        "structure={kind='collection'}",
    );
    let journey = document("journey:learn", "site:alpha", "route={path='/learn/'}").replace(
        "structure={kind='page',family={kind='documentation'}}",
        "structure={kind='journey',collection='collection:home',members=[]}",
    );
    let corpus = corpus(&[("collection.md", &collection), ("journey.md", &journey)]);
    let resolved = authored::resolve(&workspace, &corpus, content::Limits::default())
        .expect("container routes");
    for (id, url, output) in [
        (
            "collection:home",
            "https://alpha.example/prefix/",
            "index.html",
        ),
        (
            "journey:learn",
            "https://alpha.example/prefix/learn/",
            "learn/index.html",
        ),
    ] {
        let page = resolved
            .project_page(
                &key(id),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .expect("authored container body");
        let route = page.data.route.expect("container address");
        assert_eq!(route.canonical_url, url);
        assert_eq!(route.output.as_str(), output);
        assert_eq!(
            page.body_html,
            "<h1 id=\"retained-body\">Retained body</h1>\n"
        );
    }
}

#[test]
fn repeated_alias_diagnostics_deduplicate_before_copying_long_identities() {
    let source = configuration(Some("https://alpha.example/"), None);
    let id = format!("content:{}", "x".repeat(8192));
    let aliases = vec!["'/old/'"; 1024].join(",");
    let document = document(
        &id,
        "site:alpha",
        &format!("route={{path='/primary/',aliases=[{aliases}]}}"),
    );
    match error_for(
        &source,
        &[("long.md", &document)],
        content::Limits {
            text_bytes: 131_072,
            ..content::Limits::default()
        },
    ) {
        Error::Structure {
            error: content::Error::Invalid(diagnostics),
            ..
        } => {
            assert_eq!(diagnostics.len(), 1);
            assert_eq!(diagnostics[0].code, "content_route_collision");
            assert_eq!(diagnostics[0].source.as_str(), id);
        }
        other => panic!("expected one deduplicated collision, got {other:?}"),
    }
}

#[test]
fn distinct_collision_diagnostics_cannot_amplify_a_long_parent_identity() {
    let source = configuration(Some("https://alpha.example/"), None);
    let id = format!("content:{}", "x".repeat(8192));
    let mut files = vec![(
        "parent.md".to_owned(),
        document(&id, "site:alpha", "route={path='/a'}"),
    )];
    for index in 0..10 {
        files.push((
            format!("child{index}.md"),
            document(
                &format!("content:child{index}"),
                "site:alpha",
                &format!("route={{path='/a/{index}/'}}"),
            ),
        ));
    }
    let refs: Vec<_> = files
        .iter()
        .map(|(path, text)| (path.as_str(), text.as_str()))
        .collect();
    match error_for(
        &source,
        &refs,
        content::Limits {
            text_bytes: 10_000,
            ..content::Limits::default()
        },
    ) {
        Error::Structure {
            error: content::Error::Limit(resource),
            ..
        } => assert_eq!(resource, "route diagnostic bytes"),
        other => panic!("expected bounded diagnostics, got {other:?}"),
    }
}
