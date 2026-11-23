//! Retained static assets and complete output bounds.

use super::*;

#[test]
fn direct_cli_static_assets_preserve_exact_bytes_scopes_and_shared_page_models() {
    // Synthetic binary payloads prove transport only, not valid media, MIME,
    // security, rights or publication approval.
    const AUTHORED_IMAGE: &[u8] = b"\x89PNG\r\n\x1a\n\0authored";
    const NATIVE_IMAGE: &[u8] = b"\x89PNG\r\n\x1a\n\0native";
    const AUTHORED_DOWNLOAD: &[u8] = b"%PDF-1.7\n\0\xffauthored";
    const NATIVE_DOWNLOAD: &[u8] = b"%PDF-1.7\n\0\xfenative";
    let page = format!(
        "{PAGE}\n![Image](images/shared.PNG) [Download](files/r%C3%A9sum%C3%A9%20copy.PDF) [Repeated](files/r%C3%A9sum%C3%A9%20copy.PDF)\n\n![Alt [not a link](missing.pdf) ![nested](files/unreferenced.txt)](images/shared.PNG)\n\n`![example](absent.png)`\n\n```text\n[example](absent.pdf)\n```\n\n![External](https://example.invalid/absent.png) [Root relative](/owned-elsewhere.pdf) <downloads@example.pdf>\n"
    );
    let native = format!(
        "{}\n![Image](../images/shared.PNG) [Download](../files/r%C3%A9sum%C3%A9%20copy.PDF)\n",
        PROJECT_DOC.replace("path='/docs/native/'", "path='/docs/native.html'")
    );
    let template = format!(
        "{TEMPLATE}<aside>{{% for asset in assets %}}<a href=\"{{{{ asset.url }}}}\">{{{{ asset.bytes }}}}</a>{{% endfor %}}</aside>"
    );
    let fixture = Fixture::new(&[
        ("one/page.md", page.as_bytes()),
        ("one/images/shared.PNG", AUTHORED_IMAGE),
        ("one/files/résumé copy.PDF", AUTHORED_DOWNLOAD),
        (
            "one/files/unreferenced.txt",
            b"retained without an invented page relationship",
        ),
        ("project/nested/guide.md", native.as_bytes()),
        ("project/images/shared.PNG", NATIVE_IMAGE),
        ("project/files/résumé copy.PDF", NATIVE_DOWNLOAD),
        ("other/foreign.png", b"foreign selected-site exclusion"),
        ("other/notebook.ipynb", NOTEBOOK),
        ("theme/templates/page.html", template.as_bytes()),
    ]);
    let config = static_configuration(&fixture, true);
    let inputs = acquire(&config);
    let rendered = render(&inputs).expect("static assets and both documentation paths");
    let tree = rendered.site().output();
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).expect("shared model");
    let expected = [
        (
            "media/authored/images/shared.PNG",
            "https://one.example/mount/media/authored/images/shared.PNG",
            AUTHORED_IMAGE,
            "one/images/shared.PNG",
            serde_json::json!({"site":"site:one"}),
        ),
        (
            "media/authored/files/résumé copy.PDF",
            "https://one.example/mount/media/authored/files/r%C3%A9sum%C3%A9%20copy.PDF",
            AUTHORED_DOWNLOAD,
            "one/files/résumé copy.PDF",
            serde_json::json!({"site":"site:one"}),
        ),
        (
            "media/native/images/shared.PNG",
            "https://one.example/mount/media/native/images/shared.PNG",
            NATIVE_IMAGE,
            "native-docs/images/shared.PNG",
            serde_json::json!({"site":"site:one","project":"project:native"}),
        ),
        (
            "media/native/files/résumé copy.PDF",
            "https://one.example/mount/media/native/files/r%C3%A9sum%C3%A9%20copy.PDF",
            NATIVE_DOWNLOAD,
            "native-docs/files/résumé copy.PDF",
            serde_json::json!({"site":"site:one","project":"project:native"}),
        ),
    ];
    assert_eq!(
        model["assets"]
            .as_object()
            .expect("selected asset plan")
            .len(),
        5
    );
    for (output, url, payload, _, provenance) in &expected {
        assert_eq!(
            bytes(tree, output),
            *payload,
            "binary payload must not be decoded or transformed"
        );
        assert_eq!(
            model["assets"][url],
            serde_json::json!({
                "url":url,"output":output,"sha256":source::Sha256Digest::of(payload),
                "bytes":payload.len(),"provenance":provenance
            })
        );
    }
    assert_eq!(
        model["pages"]["collection:home"]["assets"],
        serde_json::json!([])
    );
    for (id, prefix, output) in [
        ("content:one", "authored", "guide/index.html"),
        ("doc:native", "native", "docs/native.html"),
    ] {
        let page = &model["pages"][id];
        let assets = page["assets"]
            .as_array()
            .expect("direct deduplicated assets");
        let urls = [
            format!("https://one.example/mount/media/{prefix}/files/r%C3%A9sum%C3%A9%20copy.PDF"),
            format!("https://one.example/mount/media/{prefix}/images/shared.PNG"),
        ];
        assert_eq!(
            assets.len(),
            2,
            "only direct references, repeated references deduplicated"
        );
        for (asset, url) in assets.iter().zip(&urls) {
            assert_eq!(asset, &model["assets"][url]);
        }
        let body = page["body_html"].as_str().expect("resolved body");
        assert!(body.contains(&format!("href=\"{}\"", urls[0])));
        assert!(body.contains(&format!("src=\"{}\"", urls[1])));
        let html = std::str::from_utf8(bytes(tree, output)).expect("Tera output");
        assert!(html.contains(body));
        let normalized = html.replace("&#x2F;", "/");
        for asset in assets {
            assert!(normalized.contains(&format!(
                "<a href=\"{}\">{}</a>",
                asset["url"].as_str().expect("URL"),
                asset["bytes"]
            )));
        }
    }
    let authored_body = model["pages"]["content:one"]["body_html"]
        .as_str()
        .expect("body");
    assert!(authored_body.contains("alt=\"Alt not a link nested\""));
    assert!(!authored_body.contains("missing.pdf"));
    assert!(!authored_body.contains("files/unreferenced.txt"));
    assert!(authored_body.contains("href=\"mailto:downloads@example.pdf\""));
    for unchanged in [
        "![example](absent.png)",
        "[example](absent.pdf)",
        "src=\"https://example.invalid/absent.png\"",
        "href=\"https://one.example/owned-elsewhere.pdf\"",
    ] {
        assert!(authored_body.contains(unchanged));
    }
    let public_model = std::str::from_utf8(bytes(tree, "model.json")).expect("JSON");
    for private in [
        fixture.directory.path().to_str().expect("root"),
        "source:native",
        "native-docs/",
        "foreign.png",
        "notebook.ipynb",
    ] {
        assert!(
            !public_model.contains(private),
            "not part of selected public asset model: {private}"
        );
    }
    let parent = tempdir().expect("explicit local output parent");
    fs::write(parent.path().join("unrelated.txt"), b"preserve").expect("unrelated bytes");
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments.extend(["--output-parent", parent.path().to_str().expect("parent")]);
    let output = cli(&config, &arguments);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value = serde_json::from_slice(&output.stdout).expect("CLI receipt");
    let retained = std::path::Path::new(receipt["directory"].as_str().expect("retained directory"));
    assert_eq!(
        source::inspect(retained, None, source::Limits::default())
            .expect("independent inventory")
            .identity(),
        tree.inventory().identity()
    );
    for entry in tree.inventory().entries() {
        if matches!(entry.content, source::Content::File { .. }) {
            assert_eq!(
                fs::read(retained.join(entry.path.as_str())).expect("read-back"),
                tree.file(&entry.path).expect("expected bytes")
            );
        }
    }
    for (output, _, payload, origin, _) in &expected {
        assert_eq!(receipt["evidence"]["assets"][output]["path"], *origin);
        assert_eq!(
            receipt["evidence"]["assets"][output]["source_sha256"],
            serde_json::to_value(source::Sha256Digest::of(payload)).expect("digest")
        );
    }
    assert_eq!(
        fs::read(parent.path().join("unrelated.txt")).expect("preserved bytes"),
        b"preserve"
    );
    let disabled_inputs = acquire(&config.replace("render=true", "render=false"));
    assert!(
        !disabled_inputs
            .sources()
            .contains_key(&"source:native".parse().expect("native source"))
    );
    let disabled = render(&disabled_inputs).expect("disabled project assets are not acquired");
    let disabled_model: Value =
        serde_json::from_slice(bytes(disabled.site().output(), "model.json"))
            .expect("isolated asset model");
    assert_eq!(
        disabled_model["assets"]
            .as_object()
            .expect("authored asset plan")
            .len(),
        3
    );
    assert!(
        !std::str::from_utf8(bytes(disabled.site().output(), "model.json"))
            .expect("JSON")
            .contains("/media/native/")
    );
    fs::write(
        fixture.directory.path().join("one/images/shared.PNG"),
        b"changed after capture",
    )
    .expect("isolated source edit");
    fs::write(
        fixture
            .directory
            .path()
            .join("project/files/résumé copy.PDF"),
        b"changed after capture",
    )
    .expect("isolated source edit");
    assert_eq!(
        render(&inputs)
            .expect("immutable retained assets")
            .site()
            .output()
            .inventory()
            .identity(),
        tree.inventory().identity()
    );
    assert_cli_failure_preserves_parent(&config, "source_content_mismatch");
}

#[test]
fn direct_cli_static_asset_links_fail_closed_in_both_source_scopes() {
    for native in [false, true] {
        for reference in [
            "missing.png",
            "missing.pdf",
            "../one/secret.pdf",
            "%2e%2e/secret.pdf",
            "images%2Fsecret.png",
            "images%5Csecret.png",
            "bad%.png",
            "%FF.png",
        ] {
            for image in [false, true] {
                let prefix = if image { "!" } else { "" };
                let source = if native { PROJECT_DOC } else { PAGE };
                let document = format!("{source}\n{prefix}[Invalid]({reference})\n");
                let path = if native {
                    "project/guide.md"
                } else {
                    "one/page.md"
                };
                let fixture = Fixture::new(&[
                    (path, document.as_bytes()),
                    ("one/secret.pdf", b"not a cross-binding fallback"),
                ]);
                let config = static_configuration(&fixture, native);
                assert_eq!(
                    render(&acquire(&config))
                        .expect_err("invalid asset reference")
                        .code(),
                    "markdown_link_resolution"
                );
                assert_cli_failure_preserves_parent(&config, "markdown_link_resolution");
            }
        }
    }
}

#[test]
fn static_asset_configuration_does_not_compile_executable_or_unsupported_inputs() {
    for native in [false, true] {
        for extension in ["js", "ts", "ipynb", "svg", "html", "scss", "json", "yaml"] {
            let path = format!(
                "{}/uncompiled.{extension}",
                if native { "project" } else { "one" }
            );
            let fixture = Fixture::new(&[
                ("project/guide.md", PROJECT_DOC.as_bytes()),
                (
                    &path,
                    if extension == "ipynb" {
                        NOTEBOOK
                    } else {
                        b"opaque input, not an executed capability"
                    },
                ),
            ]);
            let config = static_configuration(&fixture, native);
            assert_eq!(
                render(&acquire(&config))
                    .expect_err("static transport is not compilation")
                    .code(),
                "site_uncompiled_asset"
            );
            assert_cli_failure_preserves_parent(&config, "site_uncompiled_asset");
        }
    }
    let fixture = Fixture::new(&[("one/image.png", b"unconfigured supported suffix")]);
    assert_cli_failure_preserves_parent(&fixture.config(), "site_uncompiled_asset");
}

#[test]
fn static_asset_outputs_share_page_alias_model_and_parent_collision_checks() {
    for route in [
        "route={path='/media/authored/asset.txt'}",
        "route={path='/guide/',aliases=['/media/authored/asset.txt']}",
        "route={path='/media/authored'}",
    ] {
        let page = PAGE.replace("route={path='/guide/',aliases=['/old.html']}", route);
        let fixture = Fixture::new(&[
            ("one/page.md", page.as_bytes()),
            ("one/asset.txt", b"exact static bytes"),
        ]);
        let config = static_configuration(&fixture, false);
        assert_eq!(
            render(&acquire(&config))
                .expect_err("one output path owner")
                .code(),
            "source_inventory"
        );
        assert_cli_failure_preserves_parent(&config, "source_inventory");
    }
    let fixture = Fixture::new(&[
        ("one/asset.txt", b"authored bytes"),
        ("project/guide.md", PROJECT_DOC.as_bytes()),
        ("project/asset.txt", b"distinct native bytes"),
    ]);
    let config = static_configuration(&fixture, true).replace("/media/native/", "/media/authored/");
    assert_eq!(
        render(&acquire(&config))
            .expect_err("same URL is not last-writer wins")
            .code(),
        "source_inventory"
    );
    assert_cli_failure_preserves_parent(&config, "source_inventory");

    let config = static_configuration(&fixture, false);
    let inputs = acquire(&config);
    let model_output = "media/authored/asset.txt"
        .parse()
        .expect("colliding model output");
    let error = build::render_publication(
        &inputs,
        build::SiteRenderOptions {
            render_year: None,
            graph: None,
            model_output: Some(&model_output),
            limits: source::Limits::default(),
        },
        Default::default(),
    )
    .expect_err("model cannot replace exact asset bytes");
    assert_eq!(error.code(), "source_inventory");
    let mut arguments = RENDER_ARGUMENTS;
    arguments[5] = "media/authored/asset.txt";
    assert_cli_failure_with_arguments_preserves_parent(&config, &arguments, "source_inventory");
}

#[test]
fn static_asset_bytes_and_directories_count_toward_complete_output_budgets() {
    let payload = vec![0xa5; 65_536];
    let fixture = Fixture::new(&[("one/download/large.pdf", &payload)]);
    let inputs = acquire(&static_configuration(&fixture, false));
    let baseline = render(&inputs).expect("complete output including asset bytes");
    let inventory = baseline.site().output().inventory();
    assert_eq!(
        bytes(
            baseline.site().output(),
            "media/authored/download/large.pdf"
        ),
        payload
    );
    let limits = [
        source::Limits {
            file_bytes: payload.len() as u64 - 1,
            ..source::Limits::default()
        },
        source::Limits {
            total_bytes: inventory.total_bytes() - 1,
            ..source::Limits::default()
        },
        source::Limits {
            entries: inventory.entries().len() - 1,
            ..source::Limits::default()
        },
    ];
    for limits in limits {
        let error = build::render_publication(
            &inputs,
            build::SiteRenderOptions {
                render_year: Some(
                    gazette_sdk::authored::RenderYear::new(2040).expect("fixture year"),
                ),
                graph: None,
                model_output: Some(&"model.json".parse().expect("model")),
                limits,
            },
            Default::default(),
        )
        .expect_err("no partial output under asset-inclusive limit");
        assert!(
            matches!(
                error,
                build::PublicationRenderError::Site(
                    build::SiteRenderError::Output(_) | build::SiteRenderError::Render(_)
                )
            ),
            "{error}"
        );
    }
}
