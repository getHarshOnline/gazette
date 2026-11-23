//! Hosting, discovery and themed errors share captured output and native preview.

use super::*;

fn fixture() -> Fixture {
    fixture_with(",terms={path='/terms/',tdm_reservation=true}")
}

fn fixture_with(terms: &str) -> Fixture {
    let mut fixture = Fixture::new();
    let root = fixture.directory.path();
    let old = git(root, &["rev-parse", "HEAD"]);
    let manifest = root.join("theme/theme.toml");
    fs::write(
        &manifest,
        fs::read_to_string(&manifest).unwrap().replace(
            "page='page.html'",
            "page='page.html',not_found='missing.html'",
        ),
    )
    .unwrap();
    fs::write(root.join("theme/templates/missing.html"), "<!doctype html><h1>{{ presentation.surface.kind }} {{ site }}</h1><a href=\"{{ not_found.home_url }}\">Home</a>{% for item in presentation.discovery %}<link rel=\"{{ item.kind }}\" href=\"{{ item.url }}\">{% endfor %}").unwrap();
    git(root, &["add", "theme"]);
    git(
        root,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "-m",
            "themed missing page",
        ],
    );
    let new = git(root, &["rev-parse", "HEAD"]);
    fixture.config = fixture.config.replace(old.trim(), new.trim());
    fixture.config = format!(
        "[defaults.publication]\nprocessing={{hosting={{provider='github-pages'}}}}\nmachine={{sitemap=true,robots={{enabled=true,rules=[{{user_agent='*',allow=['/']}},{{user_agent='TrainingBot',disallow=['/']}}]}},web_manifest={{enabled=true,short_name='Preview'}}{terms}}}\n{}",
        fixture.config
    );
    fixture
}

#[test]
fn deployment_outputs_and_themed_404_use_the_same_native_preview_bytes() {
    let fixture = fixture();
    let inputs = fixture.acquire(&SITES);
    let reservation = preview::reserve(&inputs, addresses(&SITES), permission()).unwrap();
    let urls = reservation.urls().clone();
    let rendered = render_using(&inputs, false, Some(&reservation));
    assert!(rendered.is_complete(), "{:?}", rendered.sites());
    let site = rendered.sites()[&key("site:one")].as_ref().unwrap().site();
    let tree = site.output();
    assert_eq!(
        tree.file(&"CNAME".parse().unwrap()).unwrap(),
        b"one.example.invalid\n"
    );
    assert_eq!(tree.file(&".nojekyll".parse().unwrap()).unwrap(), b"");
    let base = urls[&key("site:one")].as_str();
    let robots = tree.file(&"robots.txt".parse().unwrap()).unwrap();
    assert_eq!(
        std::str::from_utf8(robots).unwrap(),
        format!(
            "User-agent: *\nAllow: /\n\nUser-agent: TrainingBot\nDisallow: /\n\nSitemap: {base}sitemap.xml\n"
        )
    );
    let manifest_bytes = tree.file(&"manifest.webmanifest".parse().unwrap()).unwrap();
    let manifest: serde_json::Value = serde_json::from_slice(manifest_bytes).unwrap();
    assert_eq!(manifest["scope"], base);
    assert_eq!(manifest["start_url"], base);
    assert_eq!(manifest["short_name"], "Preview");
    // The policy names the production terms page, even when preview serves the file.
    let policy = "https://one.example.invalid/pr%C3%A9fix/terms/";
    let tdm_bytes = tree
        .file(&".well-known/tdmrep.json".parse().unwrap())
        .unwrap();
    assert_eq!(
        std::str::from_utf8(tdm_bytes).unwrap(),
        format!("[{{\"location\":\"/*\",\"tdm-reservation\":1,\"tdm-policy\":\"{policy}\"}}]\n")
    );
    let missing = tree.file(&"404.html".parse().unwrap()).unwrap();
    let html = std::str::from_utf8(missing).unwrap();
    assert!(html.contains("not-found site:one"));
    let manifest_href = format!("{base}manifest.webmanifest").replace('/', "&#x2F;");
    assert!(
        html.contains(&format!("href=\"{manifest_href}\"")),
        "{html}"
    );
    let policy_href = policy.replace('/', "&#x2F;");
    assert!(
        html.contains(&format!("<link rel=\"tdm-policy\" href=\"{policy_href}\">")),
        "{html}"
    );
    assert_eq!(
        site.evidence().not_found.as_ref().unwrap().output_sha256,
        Sha256Digest::of(missing)
    );
    // Source edits after acquisition cannot change either retained or served bytes.
    fs::write(
        fixture
            .directory
            .path()
            .join("theme/templates/missing.html"),
        "changed",
    )
    .unwrap();
    let mut session = preview::attach(reservation, &rendered).unwrap();
    let response = response(&mut session, "site:one", "/pr%C3%A9fix/unknown/deep/page/");
    assert_eq!(response.status, 404);
    assert_eq!(response.body, missing);
    assert!(response.content_type.starts_with("text/html"));
    let response = super::response(
        &mut session,
        "site:one",
        "/pr%C3%A9fix/manifest.webmanifest",
    );
    assert_eq!(response.status, 200);
    assert_eq!(response.content_type, "application/manifest+json");
    assert_eq!(response.body, manifest_bytes);
    let response = super::response(
        &mut session,
        "site:one",
        "/pr%C3%A9fix/.well-known/tdmrep.json",
    );
    assert_eq!(response.status, 200);
    assert_eq!(response.content_type, "application/json");
    assert_eq!(response.body, tdm_bytes);
    session.close();
}

#[test]
fn deployment_files_reserve_paths_and_charge_the_complete_tree() {
    let fixture = fixture();
    let inputs = fixture.acquire(&SITES[..1]);
    fn options(
        model: Option<&gazette_sdk::source::EntryPath>,
        limits: gazette_sdk::source::Limits,
    ) -> build::PublicationSetRenderOptions<'_> {
        build::PublicationSetRenderOptions {
            derive_selection: false,
            preview_link_bases: None,
            selection: None,
            review_responses: None,
            preview_urls: None,
            sites: BTreeMap::from([(
                key("site:one"),
                build::SiteRenderOptions {
                    render_year: None,
                    graph: None,
                    model_output: model,
                    limits,
                },
            )]),
        }
    }
    // Keep retained model bytes in this fixture so the independent model budget
    // fits while the output inventory exercises its exact total-byte boundary.
    let model = "model.json".parse().unwrap();
    let rendered = build::render_publications(
        &inputs,
        options(Some(&model), Default::default()),
        Default::default(),
    )
    .unwrap();
    assert!(rendered.is_complete());
    let tree = rendered.sites()[&key("site:one")]
        .as_ref()
        .unwrap()
        .site()
        .output();
    let bytes: u64 = tree.inventory().total_bytes();
    let exact = gazette_sdk::source::Limits {
        total_bytes: bytes,
        ..Default::default()
    };
    assert!(
        build::render_publications(&inputs, options(Some(&model), exact), Default::default())
            .unwrap()
            .is_complete()
    );
    let limits = gazette_sdk::source::Limits {
        total_bytes: bytes - 1,
        ..Default::default()
    };
    assert!(
        !build::render_publications(&inputs, options(Some(&model), limits), Default::default())
            .unwrap()
            .is_complete()
    );
    for path in [
        "CNAME",
        ".nojekyll",
        "robots.txt",
        "manifest.webmanifest",
        ".well-known/tdmrep.json",
        "404.html",
    ] {
        let path = path.parse().unwrap();
        assert!(
            !build::render_publications(
                &inputs,
                options(Some(&path), Default::default()),
                Default::default()
            )
            .unwrap()
            .is_complete()
        );
    }
    let without_provider = inputs
        .render_local(
            &gazette_adapter_tera::Compiler,
            &gazette_adapter_styles::Compiler,
            &gazette_sdk::raster::inspect,
            options(None, Default::default()),
            Default::default(),
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    assert!(!without_provider.is_complete());
    assert!(format!("{:?}", without_provider.sites()).contains("HostFiles provider"));
}

#[test]
fn omitted_or_unreserved_terms_emit_no_reservation_or_policy_discovery() {
    for terms in ["", ",terms={path='/terms/'}"] {
        let fixture = fixture_with(terms);
        let inputs = fixture.acquire(&SITES[..1]);
        let rendered = render_using(&inputs, false, None);
        assert!(rendered.is_complete(), "{terms}: {:?}", rendered.sites());
        let tree = rendered.sites()[&key("site:one")]
            .as_ref()
            .unwrap()
            .site()
            .output();
        assert!(
            tree.file(&".well-known/tdmrep.json".parse().unwrap())
                .is_none(),
            "{terms}"
        );
        let missing = tree.file(&"404.html".parse().unwrap()).unwrap();
        let html = std::str::from_utf8(missing).unwrap();
        assert!(html.contains("rel=\"web-manifest\""), "{html}");
        assert!(!html.contains("tdm-policy"), "{terms}: {html}");
    }
}
