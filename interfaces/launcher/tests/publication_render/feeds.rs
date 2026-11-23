//! Feed outputs cross the same real CLI/SDK capture, rendering and staging boundary.

use super::{
    Fixture, HOME, PAGE, RENDER_ARGUMENTS, Value, acquire, bytes, cli, fs, render, tempdir,
};

fn configured(fixture: &Fixture) -> String {
    let channels = ["atom", "rss", "json"].map(|format| {
        format!("{{id='feed:{format}',channel='collection:home',path='/feeds/{format}',format='{format}',scope={{kind='site'}}}}")
    });
    let machine = format!(
        "machine={{search='lexical',json_ld=true,feeds={{identity_base='https://identity.example/publication/',channels=[{}]}}}}\n",
        channels.join(",")
    );
    fixture
        .config()
        .replacen("public_base_url=", &format!("{machine}public_base_url="), 1)
        .replace(
            "id='owner:one'\nkind='person'",
            "id='owner:one'\nkind='person'\nname='Writer & reader'",
        )
}

fn fixture() -> Fixture {
    let metadata = "[content]\ndescription='An exact description'\nmodified='2026-09-09T12:00:00+05:30'\npublished='2026-09-08'\nattributions=[{principal='owner:one',role='author'}]\n";
    let home = HOME
        .replace(
            "description='The explicit fixture collection and its retained articles.'\n",
            "",
        )
        .replace("[content]\n", metadata);
    let page = PAGE
        .replace("description='The retained local article.'\n", "")
        .replace("[content]\n", metadata)
        + "\n\n<!-- more -->\n\nAfter the cut.\n";
    Fixture::new(&[
        ("one/home.md", home.as_bytes()),
        ("one/page.md", page.as_bytes()),
    ])
}

fn filtered(fixture: &Fixture) -> String {
    configured(fixture)
        .replace(",scope={kind='site'}", "")
        .replace("feeds={identity_base=", "feeds={selection={kind='articles',residence='all',lifecycle=['published'],require_published_date=true},omit_when_empty=true,identity_base=")
}

#[test]
fn configured_all_draft_feeds_are_absent_not_zero_byte_outputs_and_keep_article_pages() {
    let fixture = fixture();
    let config = filtered(&fixture);
    let inputs = acquire(&config);
    let rendered = render(&inputs).expect("explicit filtered empty publication");
    let tree = rendered.site().output();
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).expect("shared model");
    assert_eq!(
        model["pages"]["content:one"]["content"]["lifecycle"],
        "draft"
    );
    assert!(
        std::str::from_utf8(bytes(tree, "guide/index.html"))
            .expect("retained article HTML")
            .contains("Retained")
    );
    for page in model["pages"].as_object().expect("selected pages").values() {
        assert!(
            page["presentation"].get("feeds").is_none(),
            "omitted files must not be advertised"
        );
    }
    for format in ["atom", "rss", "json"] {
        let path = format!("feeds/{format}");
        assert!(
            !tree
                .inventory()
                .entries()
                .iter()
                .any(|entry| entry.path.as_str() == path)
        );
    }
    let parent = tempdir().expect("disposable output parent");
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments.extend([
        "--output-parent",
        parent.path().to_str().expect("native parent"),
    ]);
    let result = cli(&config, &arguments);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let receipt: Value = serde_json::from_slice(&result.stdout).expect("staging receipt");
    assert_eq!(
        receipt["inventory"],
        serde_json::to_value(tree.inventory()).expect("exact SDK inventory")
    );
    let directory = std::path::Path::new(receipt["directory"].as_str().expect("fresh output"));
    for format in ["atom", "rss", "json"] {
        assert!(
            !directory.join(format!("feeds/{format}")).exists(),
            "omitted feed never becomes a placeholder"
        );
    }
    assert!(directory.join("guide/index.html").is_file());
}

#[test]
fn configured_feed_filter_preserves_calendar_date_and_theme_machine_page_parity() {
    let metadata = "[content]\nmodified='2026-09-09T12:00:00+05:30'\npublished='2026-09-08'\nattributions=[{principal='owner:one',role='author'}]\n";
    let page = PAGE
        .replace("[content]\n", metadata)
        .replace("lifecycle='draft'", "lifecycle='published'");
    let undated = PAGE
        .replace("content:one", "content:undated")
        .replace("/guide/", "/undated/")
        .replace("aliases=['/old.html']", "aliases=[]")
        .replace("lifecycle='draft'", "lifecycle='published'");
    let home = HOME.replace("[content]\n", metadata);
    let fixture = Fixture::new(&[
        ("one/home.md", home.as_bytes()),
        ("one/page.md", page.as_bytes()),
        ("one/undated.md", undated.as_bytes()),
    ]);
    let inputs = acquire(&filtered(&fixture));
    let rendered = render(&inputs).expect("filtered standards projections");
    let tree = rendered.site().output();
    let model: Value =
        serde_json::from_slice(bytes(tree, "model.json")).expect("same native model");
    let feed: Value = serde_json::from_slice(bytes(tree, "feeds/json")).expect("native JSON Feed");
    assert_eq!(feed["items"].as_array().expect("dated article").len(), 1);
    let item = &feed["items"][0];
    let page = &model["pages"]["content:one"];
    assert_eq!(item["_gazette"]["content"], page["content"]);
    assert_eq!(item["content_html"], page["body_html"]);
    assert_eq!(item["content_text"], page["body_text"]);
    assert_eq!(item["_gazette"]["content"]["published"], "2026-09-08");
    assert!(item.get("date_published").is_none());
    assert!(
        model["pages"].get("content:undated").is_some(),
        "selection does not delete an undated article"
    );
    assert!(!bytes(tree, "undated/index.html").is_empty());
    let identity = item["id"].as_str().expect("one authored identity");
    for format in ["atom", "rss"] {
        let wire =
            std::str::from_utf8(bytes(tree, &format!("feeds/{format}"))).expect("complete XML");
        assert!(wire.contains(identity));
        assert!(!wire.contains("content%3Aundated"));
        assert!(
            !wire.contains(
                fixture
                    .directory
                    .path()
                    .to_str()
                    .expect("private source root")
            )
        );
    }
}

#[test]
fn omitted_feed_paths_remain_reserved_against_authored_machine_and_prefix_outputs() {
    let fixture = fixture();
    let configured = filtered(&fixture);
    for config in [
        configured.replace("path='/feeds/atom'", "path='/guide/index.html'"),
        configured.replace("path='/feeds/atom'", "path='/model.json'"),
        configured.replace("path='/feeds/atom'", "path='/guide'"),
        configured
            .replace("path='/feeds/atom'", "path='/reserved'")
            .replace("path='/feeds/rss'", "path='/reserved/item.xml'"),
    ] {
        let parent = tempdir().expect("preserved stage parent");
        fs::write(parent.path().join("canary"), b"keep").expect("fixture canary");
        let mut arguments = RENDER_ARGUMENTS.to_vec();
        arguments.extend([
            "--output-parent",
            parent.path().to_str().expect("native parent"),
        ]);
        let result = cli(&config, &arguments);
        assert!(
            !result.status.success(),
            "configured omitted reservation conflict must fail"
        );
        assert!(result.stdout.is_empty());
        let diagnostic = String::from_utf8_lossy(&result.stderr);
        assert!(diagnostic.contains("source_inventory"), "{diagnostic}");
        assert_eq!(
            fs::read(parent.path().join("canary")).expect("preserved canary"),
            b"keep"
        );
        assert_eq!(
            fs::read_dir(parent.path())
                .expect("no partial native stage")
                .count(),
            1
        );
    }
}

#[test]
fn all_feed_formats_share_pages_and_survive_retained_rebuild_and_cli_staging() {
    let fixture = fixture();
    let config = configured(&fixture);
    let inputs = acquire(&config);
    let rendered = render(&inputs).expect("all standard feed outputs");
    let tree = rendered.site().output();
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).expect("page model");
    let feed: Value = serde_json::from_slice(bytes(tree, "feeds/json")).expect("JSON Feed");
    let page = &model["pages"]["content:one"];
    assert_eq!(feed["items"].as_array().expect("complete entries").len(), 1);
    let item = &feed["items"][0];
    assert_eq!(item["content_html"], page["body_html"]);
    assert_eq!(item["content_text"], page["body_text"]);
    assert_eq!(item["summary"], page["summary"]["text"]);
    assert_eq!(item["_gazette"]["content"], page["content"]);
    assert_eq!(item["_gazette"]["attributions"], page["attributions"]);
    assert_eq!(item["_gazette"]["content"]["lifecycle"], "draft");
    assert!(
        item.get("date_published").is_none(),
        "calendar dates are not timestamps"
    );
    assert_eq!(item["date_modified"], "2026-09-09T12:00:00+05:30");
    assert!(
        feed.get("authors").is_none(),
        "no implicit channel-author inheritance"
    );
    assert_eq!(item["authors"][0]["name"], "Writer & reader");
    let identity = item["id"]
        .as_str()
        .expect("stable explicit-namespace identity");
    for format in ["atom", "rss"] {
        let wire = std::str::from_utf8(bytes(tree, &format!("feeds/{format}"))).expect("XML feed");
        assert!(wire.contains(identity));
        assert!(wire.contains("Writer &amp; reader"));
        assert!(!wire.contains(fixture.directory.path().to_str().expect("native root")));
    }
    let parent = tempdir().expect("disposable stage");
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments.extend(["--output-parent", parent.path().to_str().expect("parent")]);
    let result = cli(&config, &arguments);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let receipt: Value = serde_json::from_slice(&result.stdout).expect("CLI stage receipt");
    assert_eq!(
        receipt["inventory"],
        serde_json::to_value(tree.inventory()).expect("SDK inventory")
    );
    let directory = std::path::Path::new(receipt["directory"].as_str().expect("staged directory"));
    for format in ["atom", "rss", "json"] {
        let path = format!("feeds/{format}");
        assert_eq!(
            fs::read(directory.join(&path)).expect("exact staged feed"),
            bytes(tree, &path)
        );
    }
    fs::write(
        fixture.directory.path().join("one/page.md"),
        "changed after capture",
    )
    .expect("disposable source edit");
    let repeated = render(&inputs).expect("no mutable reread");
    assert_eq!(
        repeated.site().output().inventory().identity(),
        tree.inventory().identity()
    );
}

#[test]
fn feed_format_and_output_conflicts_fail_before_any_native_stage() {
    let fixture = fixture();
    let configured = configured(&fixture);
    for (config, code) in [
        (
            configured.replace("path='/feeds/atom'", "path='/guide/index.html'"),
            "source_inventory",
        ),
        (
            configured.replace("channel='collection:home'", "channel='content:other'"),
            "feed_binding",
        ),
        (
            configured.replace("name='Writer & reader'", ""),
            "feed_field_required",
        ),
    ] {
        let parent = tempdir().expect("preserved stage parent");
        fs::write(parent.path().join("canary"), b"keep").expect("canary");
        let mut arguments = RENDER_ARGUMENTS.to_vec();
        arguments.extend(["--output-parent", parent.path().to_str().expect("parent")]);
        let result = cli(&config, &arguments);
        assert!(!result.status.success());
        assert!(result.stdout.is_empty());
        let diagnostic = String::from_utf8_lossy(&result.stderr);
        assert!(diagnostic.contains(code), "{diagnostic}");
        assert_eq!(
            fs::read(parent.path().join("canary")).expect("preserved"),
            b"keep"
        );
        assert_eq!(
            fs::read_dir(parent.path())
                .expect("no staged subset")
                .count(),
            1
        );
    }
}

#[test]
fn feed_selection_and_failure_stay_site_local_without_shrinking_an_explicit_set() {
    let fixture = fixture();
    let config = configured(&fixture);
    for (workspace, succeeds) in [
        (config.clone(), true),
        (config.replace("name='Writer & reader'", ""), false),
    ] {
        let request = serde_json::json!({
            "format": "gazette-local-publication-set/1", "workspace": {"kind":"supplied","source":workspace},
            "sites": (["site:one", "site:other"].map(|site| serde_json::json!({
                "site": site,   "model_output": "model.json",
            }))),
        });
        let output = cli(&request.to_string(), &["render-sites", "--local"]);
        assert_eq!(
            output.status.success(),
            succeeds,
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: Value =
            serde_json::from_slice(&output.stdout).expect("complete per-site report");
        assert_eq!(
            report["sites"]
                .as_object()
                .expect("exact requested set")
                .len(),
            2
        );
        assert_eq!(report["sites"]["site:other"]["status"], "rendered");
        assert!(
            !report["sites"]["site:other"]["inventory"]
                .to_string()
                .contains("feeds/"),
            "no foreign feed selection"
        );
        let first = &report["sites"]["site:one"];
        if succeeds {
            assert_eq!(first["status"], "rendered");
            for format in ["atom", "rss", "json"] {
                assert!(
                    first["inventory"]
                        .to_string()
                        .contains(&format!("feeds/{format}"))
                );
            }
        } else {
            assert_eq!(report["status"], "local_set_incomplete");
            assert_eq!(first["status"], "failed");
            assert!(
                first.get("inventory").is_none(),
                "no successful subset of the failed site"
            );
            assert!(first.to_string().contains("feed_field_required"));
        }
    }
}

#[test]
fn emitted_feed_discovery_uses_resolved_addresses_and_escaped_channel_titles() {
    let metadata = "[content]\ndescription='A feed channel'\nmodified='2026-09-09T12:00:00+05:30'\nattributions=[{principal='owner:one',role='author'}]\n";
    let home = HOME
        .replace("[content]\n", metadata)
        .replace(
            "description='The explicit fixture collection and its retained articles.'\n",
            "",
        )
        .replace("title='Home'", "title='Updates <&> \"quoted\"'");
    let page = PAGE
        .replace("[content]\n", metadata)
        .replace("description='The retained local article.'\n", "");
    let template = br#"{% if presentation.feeds is defined %}{% for feed in presentation.feeds %}<link rel="alternate" href="{{ feed.url }}" type="{{ feed.media_type }}" title="{{ feed.title }}">{% endfor %}{% endif %}{{ body_html | safe }}"#;
    let fixture = Fixture::new(&[
        ("one/home.md", home.as_bytes()),
        ("one/page.md", page.as_bytes()),
        ("theme/templates/page.html", template),
        ("theme/templates/alias.html", template),
    ]);
    for base in ["https://one.example/mount/", "http://127.0.0.1:4329/local/"] {
        let config = configured(&fixture).replace("https://one.example/mount/", base);
        let inputs = acquire(&config);
        let rendered = render(&inputs).expect("actual emitted feeds before Theme rendering");
        let tree = rendered.site().output();
        let model: Value = serde_json::from_slice(bytes(tree, "model.json")).expect("model");
        let feeds = model["pages"]["content:one"]["presentation"]["feeds"]
            .as_array()
            .expect("resolved feeds");
        assert_eq!(feeds.len(), 3);
        for (format, media_type) in [
            ("atom", "application/atom+xml"),
            ("rss", "application/rss+xml"),
            ("json", "application/feed+json"),
        ] {
            let feed = feeds
                .iter()
                .find(|feed| feed["format"] == format)
                .expect("format emitted");
            assert_eq!(feed["id"], format!("feed:{format}"));
            assert_eq!(feed["channel"], "collection:home");
            assert_eq!(feed["format"], format);
            assert_eq!(feed["media_type"], media_type);
            assert_eq!(feed["url"], format!("{base}feeds/{format}"));
            assert_eq!(feed["title"], "Updates <&> \"quoted\"");
            assert!(!bytes(tree, &format!("feeds/{format}")).is_empty());
        }
        for path in ["index.html", "guide/index.html", "old.html"] {
            let html = std::str::from_utf8(bytes(tree, path))
                .expect("Theme HTML")
                .replace("&#x2F;", "/");
            assert_eq!(
                html.matches("rel=\"alternate\"").count(),
                3,
                "one link per emitted feed"
            );
            assert!(
                html.contains("Updates &lt;&amp;&gt; &quot;quoted&quot;"),
                "{html}"
            );
            assert!(html.contains(&format!("{base}feeds/atom")));
        }
        let again = render(&inputs).expect("same retained capture");
        assert_eq!(
            tree.inventory().identity(),
            again.site().output().inventory().identity()
        );
    }
}

#[test]
fn retained_empty_feeds_are_advertised_but_disabled_feeds_are_not() {
    let fixture = fixture();
    let kept = filtered(&fixture).replace("omit_when_empty=true", "omit_when_empty=false");
    let inputs = acquire(&kept);
    let rendered = render(&inputs).expect("standards-compliant empty feeds");
    let tree = rendered.site().output();
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).expect("model");
    assert_eq!(
        model["pages"]["content:one"]["presentation"]["feeds"]
            .as_array()
            .expect("emitted")
            .len(),
        3
    );
    let feed: Value = serde_json::from_slice(bytes(tree, "feeds/json")).expect("empty JSON Feed");
    assert!(feed["items"].as_array().expect("items").is_empty());
    let inputs = acquire(&fixture.config());
    let rendered = render(&inputs).expect("no feed configuration");
    let model: Value =
        serde_json::from_slice(bytes(rendered.site().output(), "model.json")).expect("model");
    assert!(
        model["pages"]["content:one"]["presentation"]
            .get("feeds")
            .is_none()
    );
}
