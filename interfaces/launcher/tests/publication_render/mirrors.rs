//! Cross-site redirects retain one canonical object through real Tera and native staging.

use serde_json::json;

use super::*;

const MIRROR_TEMPLATE: &str = "<link rel=\"canonical\" href=\"{{ route.canonical_url }}\"><meta http-equiv=\"refresh\" content=\"0; url={{ route.canonical_url }}\">{% if canonical_site is defined %}<p data-content=\"{{ content }}\" data-home=\"{{ canonical_site }}\" data-site=\"{{ site }}\" data-address=\"{{ url }}\" data-theme=\"{{ presentation.theme }}\">{% endif %}<a href=\"{{ route.canonical_url }}\" data-kind=\"{{ presentation.redirect_destination.kind }}\" data-target=\"{{ presentation.redirect_destination.content }}\" data-target-site=\"{{ presentation.redirect_destination.site }}\">Moved</a>";

fn fixture(path: &str) -> Fixture {
    let page = PAGE.replace(
        "aliases=['/old.html']",
        &format!("aliases=['/old.html'],mirrors=[{{site='site:mirror',path='{path}'}}]"),
    );
    Fixture::new(&[
        ("one/page.md", page.as_bytes()),
        ("theme/templates/alias.html", MIRROR_TEMPLATE.as_bytes()),
    ])
}

fn fixture_without_alias(path: &str) -> Fixture {
    let page = PAGE.replace(
        "aliases=['/old.html']",
        &format!("aliases=['/old.html'],mirrors=[{{site='site:mirror',path='{path}'}}]"),
    );
    let manifest = MANIFEST.replace(",alias='alias.html'", "");
    Fixture::new(&[
        ("one/page.md", page.as_bytes()),
        ("theme/theme.toml", manifest.as_bytes()),
        ("theme/templates/alias.html", MIRROR_TEMPLATE.as_bytes()),
    ])
}

fn configuration(fixture: &Fixture) -> String {
    let mut configuration = fixture.config()
        .replace("[[brands]]", "[[sites]]\nid='site:mirror'\npublisher='owner:one'\ndeployable=false\n[[brands]]")
        .replace("[[sources]]\nid='source:theme'", "[[brands.publications]]\nsite='site:mirror'\npublic_base_url='https://mirror.example/selected/'\n[[sources]]\nid='source:theme'")
        .replace("public_base_url=", "machine={search='lexical',sitemap=true,json_ld=true}\npublic_base_url=");
    for (site, description, contents) in [
        (
            "one",
            "collection:home",
            &["collection:home", "content:one"][..],
        ),
        ("other", "content:other", &["content:other"][..]),
    ] {
        configuration = configuration.replace(
            &format!("machine={{search='lexical',sitemap=true,json_ld=true}}\npublic_base_url='https://{site}.example/"),
            &format!("machine={{search='lexical',sitemap=true,json_ld=true,{}}}\npublic_base_url='https://{site}.example/", curated_llms(description, &[("Canonical reading", contents)])),
        );
    }
    configuration
}

#[test]
fn mirror_only_site_cannot_curate_another_sites_canonical_content() {
    let fixture = fixture("/copy/");
    let configuration = configuration(&fixture).replace(
        "machine={search='lexical',sitemap=true,json_ld=true}\npublic_base_url='https://mirror.example/selected/'",
        &format!(
            "machine={{search='lexical',sitemap=true,json_ld=true,{}}}\npublic_base_url='https://mirror.example/selected/'",
            curated_llms("content:one", &[("Wrong-owner entry", &["content:one"])])
        ),
    );
    let inputs = build::acquire_publication(
        &configuration,
        &"site:mirror".parse().expect("mirror-only site"),
        toml::Limits::default(),
        build::PublicationSetAcquisitionLimits::default().sources,
    )
    .expect("complete explicitly selected mirror inputs");
    let failure = render(&inputs)
        .expect_err("mirror redirects do not supply a local canonical curation source");
    assert_eq!(failure.code(), "site_llms_reference");
}

#[test]
fn mirror_only_site_renders_shared_redirect_metadata_without_duplicate_content() {
    let fixture = fixture("/copy/");
    let configuration = configuration(&fixture);
    let selected = ["site:one", "site:other", "site:mirror"];
    let inputs = super::sites::acquire_set(&configuration, &selected);
    let page = "page.html".parse().expect("page");
    let alias = "alias.html".parse().expect("alias");
    let model_path = "model.json".parse().expect("model");
    let rendered = build::render_publications(
        &inputs,
        super::sites::options(&selected, &page, &alias, &model_path),
        Default::default(),
    )
    .expect("shared resolved corpus");
    assert!(
        rendered.is_complete(),
        "{}",
        serde_json::to_string(&rendered.inspect()).expect("set evidence")
    );
    let key = "site:mirror".parse().expect("mirror site");
    let mirror = rendered.sites()[&key]
        .as_ref()
        .expect("mirror-only output")
        .site();
    let model: Value =
        serde_json::from_slice(bytes(mirror.output(), "model.json")).expect("machine model");
    let mut metadata = json!({
        "content":"content:one",
        "canonical_site":"site:one",
        "site":"site:mirror",
        "url":"https://mirror.example/selected/copy/",
        "route":{"canonical_url":"https://one.example/mount/guide/"},
    });
    metadata.sort_all_objects();
    let presentation = json!({
        "surface":{"kind":"mirror"},
        "redirect_destination":{"kind":"cross-site","content":"content:one","site":"site:one","relationships":[],"presentation":{"theme":"theme:one"}},
        "theme":"theme:one",
        "palette":null,
        "browser_theme_colors":{"light":"#ffffff","dark":"#000000"},
        "capabilities":{},
        "discovery":[{"kind":"sitemap","media_type":"application/xml","url":"https://mirror.example/selected/sitemap.xml"}],
        "theme_resources":[],
        "resources":[],
        "search":{
            "route":"/search.json",
            "output":"search.json",
            "profile":"lexical",
            "fields":["title","body","summary","projects","keywords"],
            "url":"https://mirror.example/selected/search.json"
        }
    });
    let mut presented = metadata.clone();
    presented["presentation"] = presentation;
    presented.sort_all_objects();
    assert_eq!(model["mirrors"], json!({"copy/index.html":presented}));
    assert_eq!(model["pages"], json!({}));
    let evidence = serde_json::to_value(mirror.evidence()).expect("private evidence");
    let receipt = &evidence["mirrors"]["copy/index.html"];
    assert_eq!(receipt["model"], metadata);
    assert_eq!(receipt["template"], "fixture/alias.html");
    assert_eq!(
        receipt["model_sha256"],
        json!(source::Sha256Digest::of(
            &serde_json::to_vec(&presented).expect("exact renderer Value encoding")
        ))
    );
    assert_eq!(
        receipt["html_sha256"],
        json!(source::Sha256Digest::of(bytes(
            mirror.output(),
            "copy/index.html"
        )))
    );
    let html = std::str::from_utf8(bytes(mirror.output(), "copy/index.html"))
        .expect("redirect HTML")
        .replace("&#x2F;", "/");
    for fragment in [
        "rel=\"canonical\" href=\"https://one.example/mount/guide/\"",
        "0; url=https://one.example/mount/guide/",
        "data-content=\"content:one\"",
        "data-home=\"site:one\"",
        "data-site=\"site:mirror\"",
        "data-address=\"https://mirror.example/selected/copy/\"",
        "data-theme=\"theme:one\"",
        "data-kind=\"cross-site\"",
        "data-target=\"content:one\"",
        "data-target-site=\"site:one\"",
    ] {
        assert!(html.contains(fragment), "{html}");
    }
    assert!(!html.contains("Retained"));
    assert!(!model.to_string().contains("body_html"));
    assert!(!model.to_string().contains("attributions"));
    assert_eq!(
        serde_json::from_slice::<Value>(bytes(mirror.output(), "search.json")).expect("search"),
        json!([])
    );
    assert_eq!(
        serde_json::from_slice::<Value>(bytes(mirror.output(), "site.jsonld")).expect("JSON-LD"),
        json!({"@graph":[]})
    );
    assert!(
        !std::str::from_utf8(bytes(mirror.output(), "sitemap.xml"))
            .expect("sitemap")
            .contains("<loc>")
    );
    assert!(
        mirror
            .output()
            .file(&"llms.txt".parse().expect("llms path"))
            .is_none(),
        "a mirror-only site has no canonical content from which to curate discovery"
    );
    assert!(mirror.evidence().pages.is_empty());
    assert!(mirror.evidence().page_resources.is_empty());
    let canonical = rendered.sites()[&"site:one".parse().expect("home")]
        .as_ref()
        .expect("canonical output")
        .site();
    assert!(
        std::str::from_utf8(bytes(canonical.output(), "guide/index.html"))
            .expect("canonical body")
            .contains("Retained")
    );
    assert!(
        std::str::from_utf8(bytes(canonical.output(), "old.html"))
            .expect("unchanged same-site alias")
            .replace("&#x2F;", "/")
            .contains("https://one.example/mount/guide/")
    );
    let own_alias = std::str::from_utf8(bytes(canonical.output(), "old.html")).unwrap();
    assert!(own_alias.contains("data-kind=\"internal-native\""));
    assert!(own_alias.contains("data-target=\"content:one\""));
    assert!(canonical.evidence().mirrors.is_empty());
    let parent = tempdir().expect("native stage");
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments[3] = "site:mirror";
    arguments.extend([
        "--output-parent",
        parent.path().to_str().expect("stage parent"),
    ]);
    let result = cli(&configuration, &arguments);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let receipt: Value = serde_json::from_slice(&result.stdout).expect("native CLI receipt");
    assert_eq!(
        receipt["inventory"],
        serde_json::to_value(mirror.output().inventory()).expect("same output inventory")
    );
    let directory = std::path::Path::new(receipt["directory"].as_str().expect("retained tree"));
    for path in [
        "copy/index.html",
        "model.json",
        "search.json",
        "site.jsonld",
        "sitemap.xml",
    ] {
        assert_eq!(
            fs::read(directory.join(path)).expect("native staged file"),
            bytes(mirror.output(), path)
        );
    }
}

#[test]
fn mirror_requires_explicit_alias_template_and_cannot_overwrite_machine_output() {
    let fixture = fixture_without_alias("/copy/");
    let configuration = configuration(&fixture);
    let arguments = [
        "render-site",
        "--local",
        "--site",
        "site:mirror",
        "--model-output",
        "model.json",
    ];
    let result = cli(&configuration, &arguments);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("site_alias_template_required"));
    let collision = self::fixture("/search.json");
    let mut arguments = RENDER_ARGUMENTS;
    arguments[3] = "site:mirror";
    let result = cli(&self::configuration(&collision), &arguments);
    assert!(!result.status.success());
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("source_inventory"),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn canonical_copy_alias_preserves_foreign_target_destination() {
    let page = PAGE.replace(
        "aliases=['/old.html']",
        "aliases=['/old.html'],canonical='content:other'",
    );
    let fixture = Fixture::new(&[
        ("one/page.md", page.as_bytes()),
        ("theme/templates/alias.html", MIRROR_TEMPLATE.as_bytes()),
    ]);
    let configuration = fixture.config();
    let inputs = acquire(&configuration);
    let rendered = render(&inputs).expect("canonical-copy alias renders");
    let html = std::str::from_utf8(bytes(rendered.site().output(), "old.html"))
        .unwrap()
        .replace("&#x2F;", "/");
    for fragment in [
        "href=\"https://other.example/elsewhere/\"",
        "data-kind=\"cross-site\"",
        "data-target=\"content:other\"",
        "data-target-site=\"site:other\"",
    ] {
        assert!(html.contains(fragment), "{html}");
    }
}
