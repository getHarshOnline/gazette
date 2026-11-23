//! Cross-site redirects preserve one canonical object; no body or home duplication.

use super::*;

#[test]
fn mirror_only_site_borrows_canonical_object_and_keeps_indexes_empty() {
    let config = configuration(
        Some("https://alpha.example/base/"),
        Some("https://beta.example/other/"),
    ).replace("site='site:beta'\npublic_base_url", "site='site:beta'\nmachine={markdown_alternates={enabled=true,directory_page='index.md',file_page='append-md',media_type='text/markdown',discovery_rel='alternate'},llms={enabled=true,description_from='content:guide',representation='markdown-alternate',include_human_link=true,sections=[{title='Chosen',contents=['content:guide']}]}}\npublic_base_url");
    let config = format!("{}\n[[sources]]\nid='source:mirror-proof'\nlocation={{kind='filesystem',root='/private/mirror-proof-not-read'}}\n", config.replace("site='site:alpha'\npublic_base_url", "site='site:alpha'\nauthored=[{source='source:mirror-proof',target='home'}]\npublic_base_url"));
    let compiled = workspace::compile(&config, Default::default()).unwrap();
    let source = document(
        "content:guide",
        "site:alpha",
        "route={path='/docs/start/',mirrors=[{site='site:beta',path='/library/intro/'}]}",
    );
    let parsed = corpus(&[("home/guide.md", &source)]);
    let resolved = authored::resolve(&compiled, &parsed, content::Limits::default()).unwrap();
    assert_eq!(resolved.content().graph().nodes().count(), 1);
    let id = key("content:guide");
    let original = resolved.content().graph().node(&id).unwrap();
    let canonical = resolved.content().route(&id).unwrap();
    assert_eq!(
        canonical.canonical_url,
        "https://alpha.example/base/docs/start/"
    );
    let mirror_site = resolved.project_site_pages(&key("site:beta")).unwrap();
    assert_eq!(mirror_site.pages().len(), 0);
    assert_eq!(mirror_site.mirrors().len(), 1);
    let mirror = &mirror_site.mirrors()[0];
    assert!(std::ptr::eq(mirror.node, original));
    assert!(std::ptr::eq(mirror.route, canonical));
    assert_eq!(
        mirror.mirror.url,
        "https://beta.example/other/library/intro/"
    );
    assert_eq!(mirror.mirror.output.as_str(), "library/intro/index.html");
    let sitemap = String::from_utf8(mirror_site.sitemap(4096).unwrap()).unwrap();
    assert!(!sitemap.contains("<loc>"));
    assert_eq!(
        mirror_site
            .llms(&resolved.discovery().unwrap(), 4096)
            .unwrap_err()
            .code(),
        "site_llms_reference",
        "a foreign redirect cannot become a curated local canonical object"
    );
    assert!(
        mirror_site
            .markdown_alternates(&[], &[], 4096)
            .unwrap()
            .is_empty()
    );
    let home = resolved.project_site_pages(&key("site:alpha")).unwrap();
    assert_eq!(home.pages().len(), 1);
    assert!(home.mirrors().is_empty());
    let route_json = serde_json::to_value(canonical).unwrap();
    assert_eq!(route_json["mirrors"][0]["site"], "site:beta");
    assert_eq!(route_json["mirrors"][0]["url"], mirror.mirror.url);
}

#[test]
fn mirror_destinations_require_distinct_known_sites_and_public_bases() {
    for (mirror_site, beta_base, code) in [
        (
            "site:missing",
            Some("https://beta.example/"),
            "content_mirror_site",
        ),
        (
            "site:alpha",
            Some("https://beta.example/"),
            "content_mirror_home",
        ),
        ("site:beta", None, "content_route_base"),
    ] {
        let config = configuration(Some("https://alpha.example/"), beta_base);
        let source = document(
            "content:guide",
            "site:alpha",
            &format!("route={{path='/guide/',mirrors=[{{site='{mirror_site}',path='/mirror/'}}]}}"),
        );
        assert_diagnostic(
            error_for(
                &config,
                &[("guide.md", &source)],
                content::Limits::default(),
            ),
            code,
        );
    }
}

#[test]
fn mirrors_collide_with_primary_alias_and_ancestor_output_paths() {
    let config = configuration(
        Some("https://alpha.example/"),
        Some("https://beta.example/"),
    );
    for (mirror_path, other_route) in [
        ("/same/", "route={path='/same/'}"),
        ("/same/", "route={path='/home/',aliases=['/same/']}"),
        ("/tree/child/", "route={path='/tree'}"),
        ("/tree", "route={path='/tree/child/'}"),
        ("/", "route={path='/index.html'}"),
    ] {
        let source = document(
            "content:guide",
            "site:alpha",
            &format!(
                "route={{path='/guide/',mirrors=[{{site='site:beta',path='{mirror_path}'}}]}}"
            ),
        );
        let other = document("content:other", "site:beta", other_route);
        assert_diagnostic(
            error_for(
                &config,
                &[("guide.md", &source), ("other.md", &other)],
                content::Limits::default(),
            ),
            "content_route_collision",
        );
    }
}

#[test]
fn duplicate_mirrors_fail_and_distinct_mirrors_are_order_independent() {
    let config = configuration(
        Some("https://alpha.example/"),
        Some("https://beta.example/"),
    );
    let one = "{site='site:beta',path='/z/'}";
    let two = "{site='site:beta',path='/a/'}";
    let duplicate = document(
        "content:guide",
        "site:alpha",
        &format!("route={{path='/guide/',mirrors=[{one},{one}]}}"),
    );
    assert_diagnostic(
        error_for(
            &config,
            &[("guide.md", &duplicate)],
            content::Limits::default(),
        ),
        "content_route_collision",
    );
    let compiled = workspace::compile(&config, Default::default()).unwrap();
    let mut outputs = Vec::new();
    for order in [format!("{one},{two}"), format!("{two},{one}")] {
        let source = document(
            "content:guide",
            "site:alpha",
            &format!("route={{path='/guide/',mirrors=[{order}]}}"),
        );
        let parsed = corpus(&[("guide.md", &source)]);
        let resolved = authored::resolve(&compiled, &parsed, content::Limits::default()).unwrap();
        outputs.push(
            serde_json::to_vec(resolved.content().route(&key("content:guide")).unwrap()).unwrap(),
        );
    }
    assert_eq!(outputs[0], outputs[1]);
}

#[test]
fn mirror_edges_are_bounded() {
    let config = configuration(
        Some("https://alpha.example/"),
        Some("https://beta.example/"),
    );
    let source = document(
        "content:guide",
        "site:alpha",
        "route={path='/guide/',mirrors=[{site='site:beta',path='/mirror/'}]}",
    );
    assert!(matches!(
        error_for(
            &config,
            &[("guide.md", &source)],
            content::Limits {
                edges: 0,
                ..Default::default()
            }
        ),
        Error::Structure {
            error: content::Error::Limit("edges"),
            ..
        }
    ));
}

#[test]
fn preview_rebinds_mirror_and_canonical_addresses_without_changing_identity() {
    let config = configuration(
        Some("https://alpha.example/base/"),
        Some("https://beta.example/"),
    );
    let compiled = workspace::compile(&config, Default::default()).unwrap();
    let source = document(
        "content:guide",
        "site:alpha",
        "route={path='/guide/',mirrors=[{site='site:beta',path='/'}]}",
    );
    let parsed = corpus(&[("guide.md", &source)]);
    let preview = authored::resolve(&compiled, &parsed, content::Limits::default())
        .unwrap()
        .with_preview_urls(
            [
                (
                    key("site:alpha"),
                    "http://127.0.0.1:32001/base/".parse().unwrap(),
                ),
                (key("site:beta"), "http://127.0.0.1:32002/".parse().unwrap()),
            ]
            .into(),
        )
        .unwrap();
    let route = preview.content().route(&key("content:guide")).unwrap();
    assert_eq!(route.canonical_url, "http://127.0.0.1:32001/base/guide/");
    assert_eq!(route.mirrors[0].url, "http://127.0.0.1:32002/");
    assert_eq!(route.mirrors[0].output.as_str(), "index.html");
    assert_eq!(preview.content().graph().nodes().count(), 1);
    assert_eq!(
        compiled
            .workspace
            .public_base_url(&key("site:alpha"))
            .unwrap()
            .as_str(),
        "https://alpha.example/base/"
    );
}

#[test]
fn redirect_destination_uses_canonical_identity_and_actual_serving_site() {
    let config = configuration(
        Some("https://alpha.example/base/"),
        Some("https://beta.example/other/"),
    );
    let compiled = workspace::compile(&config, Default::default()).unwrap();
    let original = document(
        "content:original",
        "site:alpha",
        "route={path='/guide/',aliases=['/old/'],mirrors=[{site='site:beta',path='/mirror/'}]}",
    );
    let copy = document(
        "content:copy",
        "site:beta",
        "route={path='/copy/',aliases=['/previous/'],canonical='content:original'}",
    );
    let plain = document("content:plain", "site:alpha", "");
    let parsed = corpus(&[
        ("original.md", &original),
        ("copy.md", &copy),
        ("plain.md", &plain),
    ]);
    let resolved = authored::resolve(&compiled, &parsed, Default::default()).unwrap();
    let own = serde_json::to_value(
        resolved
            .redirect_destination(&key("content:original"), &key("site:alpha"))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        own,
        serde_json::json!({"kind":"internal-native","content":"content:original","site":"site:alpha","relationships":[], "presentation":null})
    );
    let cross = serde_json::json!({"kind":"cross-site","content":"content:original","site":"site:alpha","relationships":[], "presentation":null});
    assert_eq!(
        serde_json::to_value(
            resolved
                .redirect_destination(&key("content:original"), &key("site:beta"))
                .unwrap()
        )
        .unwrap(),
        cross
    );
    assert_eq!(
        serde_json::to_value(
            resolved
                .redirect_destination(&key("content:copy"), &key("site:beta"))
                .unwrap()
        )
        .unwrap(),
        cross
    );
    for (id, site) in [
        ("content:plain", "site:alpha"),
        ("content:missing", "site:alpha"),
        ("content:original", "site:missing"),
    ] {
        assert!(
            resolved
                .redirect_destination(&key(id), &key(site))
                .is_none()
        );
    }
    let preview = resolved
        .with_preview_urls(
            [
                (
                    key("site:alpha"),
                    "http://127.0.0.1:32101/base/".parse().unwrap(),
                ),
                (
                    key("site:beta"),
                    "http://127.0.0.1:32102/other/".parse().unwrap(),
                ),
            ]
            .into_iter()
            .collect(),
        )
        .unwrap();
    assert_eq!(
        serde_json::to_value(
            preview
                .redirect_destination(&key("content:copy"), &key("site:beta"))
                .unwrap()
        )
        .unwrap(),
        cross
    );
}
