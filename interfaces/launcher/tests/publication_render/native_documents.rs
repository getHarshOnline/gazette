//! Native authored Markdown and project documentation.

use super::*;

#[test]
fn direct_cli_composes_project_docs_with_authored_content_without_inventing_subjects() {
    let unrelated = PROJECT_DOC
        .replace("projects=[{project='project:subject',kind='about'}]\n", "")
        .replace("id='doc:native'", "id='doc:unrelated'")
        .replace("path='/docs/native/'", "path='/docs/unrelated/'");
    let template = format!(
        "{TEMPLATE}{{% if provenance.project is defined %}}<small>{{{{ provenance.project }}}}</small>{{% endif %}}{{% for item in projects %}}<b>{{{{ item.project.id }}}}:{{{{ item.relationship.relation }}}}</b>{{% endfor %}}"
    );
    let fixture = Fixture::new(&[
        ("project/guide.md", PROJECT_DOC.as_bytes()),
        ("project/unrelated.md", unrelated.as_bytes()),
        ("theme/templates/page.html", template.as_bytes()),
    ]);
    let configuration = fixture.project_config(true);
    let inputs = acquire(&configuration);
    assert_eq!(inputs.sources().len(), 4);
    let expected = render(&inputs).expect("combined authored/project publication");
    let parent = tempdir().expect("explicit output parent");
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments.extend([
        "--output-parent",
        parent.path().to_str().expect("parent path"),
    ]);
    let output = cli(&configuration, &arguments);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value = serde_json::from_slice(&output.stdout).expect("retention receipt");
    assert_eq!(receipt["status"], "local_retained");
    assert_eq!(
        receipt["inventory"],
        serde_json::to_value(expected.site().output().inventory()).expect("expected inventory")
    );
    let directory = std::path::Path::new(receipt["directory"].as_str().expect("retained path"));
    let actual =
        source::inspect(directory, None, source::Limits::default()).expect("native read-back");
    assert_eq!(
        actual.identity(),
        expected.site().output().inventory().identity()
    );
    let model_bytes = fs::read(directory.join("model.json")).expect("emitted machine output");
    let model: Value = serde_json::from_slice(&model_bytes).expect("shared publication model");
    let pages = model["pages"].as_object().expect("page collection");
    assert_eq!(pages.len(), 4);
    assert_eq!(
        pages["content:one"]["provenance"],
        serde_json::json!({"site":"site:one"})
    );
    let doc = &pages["doc:native"];
    assert_eq!(
        doc["provenance"],
        serde_json::json!({"site":"site:one","project":"project:native"})
    );
    assert_eq!(
        doc["content"]["structure"]["family"]["kind"],
        "documentation"
    );
    assert_eq!(doc["collections"][0]["id"], "collection:home");
    assert_eq!(doc["related"][0]["id"], "content:other");
    assert_eq!(
        doc["projects"]
            .as_array()
            .expect("exact project relations")
            .len(),
        1
    );
    assert_eq!(doc["projects"][0]["project"]["id"], "project:subject");
    assert_eq!(doc["projects"][0]["relationship"]["relation"], "about");
    assert_eq!(
        doc["project_declarations"][0]["declaration"]["kind"],
        "about"
    );
    assert_eq!(pages["doc:unrelated"]["projects"], serde_json::json!([]));
    assert_eq!(
        pages["doc:unrelated"]["project_declarations"],
        serde_json::json!([])
    );
    assert_eq!(
        pages["doc:unrelated"]["provenance"]["project"],
        "project:native"
    );
    let html =
        fs::read_to_string(directory.join("docs/native/index.html")).expect("native-doc HTML");
    assert!(html.contains("<small>project:native</small>"));
    assert!(html.contains("<b>project:subject:about</b>"));
    assert!(html.contains("Retained native documentation"));
    assert!(!html.contains("<b>project:native:"));
    let model_text = std::str::from_utf8(&model_bytes).expect("model UTF-8");
    assert!(!model_text.contains("source:native"));
    assert!(!model_text.contains("native-docs/"));
    assert!(!model_text.contains(fixture.directory.path().to_str().expect("native path")));
    assert_eq!(
        receipt["evidence"]["pages"]["doc:native"]["source"]["path"],
        "native-docs/guide.md"
    );

    fs::write(
        fixture.directory.path().join("project/guide.md"),
        b"changed native source after capture",
    )
    .expect("disposable mutation");
    let repeated = render(&inputs).expect("retained project bytes, no native reread");
    assert_eq!(
        repeated.site().output().inventory().identity(),
        actual.identity()
    );
    assert_eq!(bytes(repeated.site().output(), "model.json"), model_bytes);
    assert_cli_failure_preserves_parent(&configuration, "source_content_mismatch");
}

#[test]
fn direct_cli_rejects_project_doc_home_identity_and_unframed_failures_without_staging() {
    let wrong_home = PROJECT_DOC
        .replace("collections=['collection:home']\n", "")
        .replace("site='site:one'", "site='site:other'");
    let duplicate = PROJECT_DOC.replace("id='doc:native'", "id='content:one'");
    for (document, code) in [
        (wrong_home.as_str(), "publication_home"),
        (duplicate.as_str(), "publication_authored"),
        (
            "# Native text without the supported metadata profile",
            "publication_authored",
        ),
    ] {
        let fixture = Fixture::new(&[("project/guide.md", document.as_bytes())]);
        assert_cli_failure_preserves_parent(&fixture.project_config(true), code);
    }
    let fixture = Fixture::new(&[
        ("project/guide.md", PROJECT_DOC.as_bytes()),
        ("project/notebook.ipynb", NOTEBOOK),
    ]);
    assert_cli_failure_preserves_parent(&fixture.project_config(true), "site_uncompiled_asset");
}

#[test]
fn disabled_project_documents_are_not_acquired_or_silently_promoted_into_output() {
    for fixture in [
        Fixture::new(&[]),
        Fixture::new(&[
            ("project/guide.md", b"invalid native metadata"),
            ("project/asset.ipynb", b"{}"),
        ]),
    ] {
        let configuration = fixture.project_config(false);
        let inputs = acquire(&configuration);
        assert!(
            !inputs
                .sources()
                .contains_key(&"source:native".parse().expect("source key"))
        );
        let rendered = render(&inputs).expect("disabled docs impose no capture/render obligation");
        let model: Value = serde_json::from_slice(bytes(rendered.site().output(), "model.json"))
            .expect("authored-only model");
        assert_eq!(model["pages"].as_object().expect("pages").len(), 2);
        assert!(
            rendered
                .site()
                .output()
                .file(&"docs/native/index.html".parse().expect("doc path"))
                .is_none()
        );
        let output = cli(&configuration, &RENDER_ARGUMENTS);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: Value = serde_json::from_slice(&output.stdout).expect("CLI model receipt");
        assert!(report["inputs"]["sources"].get("source:native").is_none());
        assert_eq!(
            report["evidence"]["pages"]
                .as_object()
                .expect("rendered pages")
                .len(),
            2
        );
    }
}

#[test]
fn direct_cli_native_markdown_and_framed_docs_share_routes_models_and_retained_bytes() {
    const ROOT: &str = "# Native **Library**\n\n[Section](nested/README.md#target)\n\n[Repeated](#repeat-1) [Unicode](#caf%C3%A9) [Japanese](#ri-ben) [Percent](#literal%25id)\n\n## Repeat\n\n### héhé\n\n## Repeat\n\n## Named {#repeat .kept}\n\n## 日本\n\n## Unicode {#\"café\"}\n\n## !!!\n\n## Percent {#\"literal%id\"}\n\n![image](https://example.invalid/missing.md#absent)\n\n`[example](missing.md#absent)`\n\n```text\n[untouched](outside.md#absent)\n```\n";
    const NESTED: &str = "# Nested {#target}\n\n[Root](../README.md?mode=1#caf%C3%A9) and [Guide](guide.md) [Percent](../README.md#literal%25id)\n";
    const GUIDE: &str = "Filename title, no heading. [Framed](../framed.md)\n";
    let framed_document = format!(
        "{}\n[Self](#retained-native-documentation) [Dot](./#retained-native-documentation) [Query](.?mode=1#retained-native-documentation) [Repeated dot](././#retained-native-documentation)\n",
        PROJECT_DOC.replace("path='/docs/native/'", "path='/docs/native.html'")
    );
    let authored_document =
        format!("{PAGE}\n## Authored {{#authored-explicit}}\n\n[Self](#authored-explicit)\n");
    let template = format!(
        "{TEMPLATE}<nav>{{% for heading in headings %}}<a data-level=\"{{{{ heading.level }}}}\" href=\"{{{{ heading.fragment }}}}\">{{{{ heading.text }}}}</a>{{% endfor %}}</nav>"
    );
    let fixture = Fixture::new(&[
        ("project/README.md", ROOT.as_bytes()),
        ("project/nested/README.md", NESTED.as_bytes()),
        ("project/nested/guide.md", GUIDE.as_bytes()),
        ("project/framed.md", framed_document.as_bytes()),
        ("one/page.md", authored_document.as_bytes()),
        ("theme/templates/page.html", template.as_bytes()),
    ]);
    let config = native_configuration(&fixture);
    let inputs = acquire(&config);
    let rendered = render(&inputs).expect("plain and framed native documentation together");
    let tree = rendered.site().output();
    let model: Value =
        serde_json::from_slice(bytes(tree, "model.json")).expect("same machine model");
    assert_eq!(model["pages"].as_object().expect("pages").len(), 6);
    let root = page_at(&model, "https://one.example/mount/docs/library/");
    let corpus = inputs
        .parse_publication(Default::default(), &gazette_adapter_markdown::Comrak)
        .expect("retained corpus");
    let publication = corpus
        .resolve(Default::default())
        .expect("shared compiler model");
    let resolved = publication.authored();
    let root_id = root["content"]["id"]
        .as_str()
        .expect("root ID")
        .parse()
        .expect("content key");
    let projection = resolved
        .project_page(
            &root_id,
            &gazette_sdk::raster::inspect,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("same heading projection");
    let retained_body = resolved
        .file(&root_id)
        .expect("retained file")
        .body()
        .expect("retained AST");
    assert!(
        std::ptr::eq(
            projection.data.headings,
            retained_body.headings().expect("cached heading index")
        ),
        "projection borrows the retained AST heading records"
    );
    assert_eq!(
        serde_json::to_value(projection.data.headings).expect("heading projection"),
        root["headings"]
    );
    assert_eq!(root["content"]["title"], "Native Library");
    assert_eq!(root["content"]["lifecycle"], "published");
    assert!(
        root["content"]["id"]
            .as_str()
            .expect("derived key")
            .starts_with("native-document:")
    );
    assert_eq!(root["derivation"]["title"]["kind"], "heading");
    let span = &root["derivation"]["title"]["span"];
    let heading = &ROOT[span["start"].as_u64().expect("start") as usize
        ..span["end"].as_u64().expect("end") as usize];
    assert!(heading.contains("# Native **Library**"));
    assert_eq!(
        root["provenance"],
        serde_json::json!({"site":"site:one","project":"project:native"})
    );
    for field in [
        "projects",
        "project_declarations",
        "collections",
        "journeys",
    ] {
        assert_eq!(root[field], serde_json::json!([]), "no invented {field}");
    }
    assert!(root.get("authors").is_none());
    let headings = root["headings"].as_array().expect("shared heading records");
    let expected_headings = [
        ("native-library", 1, "Native Library", None),
        ("repeat-1", 2, "Repeat", Some(0)),
        ("hehe", 3, "héhé", Some(1)),
        ("repeat-2", 2, "Repeat", Some(0)),
        ("repeat", 2, "Named", Some(0)),
        ("ri-ben", 2, "日本", Some(0)),
        ("café", 2, "Unicode", Some(0)),
        ("section", 2, "!!!", Some(0)),
        ("literal%id", 2, "Percent", Some(0)),
    ];
    assert_eq!(headings.len(), expected_headings.len());
    for (heading, (id, level, text, parent)) in headings.iter().zip(expected_headings) {
        assert_eq!(heading["id"], id);
        assert_eq!(heading["level"], level);
        assert_eq!(heading["text"], text);
        assert_eq!(heading["parent"], serde_json::json!(parent));
        let fragment = match id {
            "café" => "#caf%C3%A9".to_owned(),
            "literal%id" => "#literal%25id".to_owned(),
            _ => format!("#{id}"),
        };
        assert_eq!(heading["fragment"], fragment);
        let start = heading["span"]["start"].as_u64().expect("heading start") as usize;
        let end = heading["span"]["end"].as_u64().expect("heading end") as usize;
        assert!(ROOT[start..end].starts_with('#'));
    }
    let guide = page_at(
        &model,
        "https://one.example/mount/docs/library/nested/guide/",
    );
    assert_eq!(guide["content"]["title"], "guide");
    assert_eq!(
        guide["derivation"],
        serde_json::json!({"title":{"kind":"filename"},"language_from_profile":false})
    );
    let framed = &model["pages"]["doc:native"];
    assert_eq!(framed["content"]["title"], "Native documentation");
    assert!(framed.get("derivation").is_none());
    assert_eq!(framed["projects"][0]["project"]["id"], "project:subject");
    assert_eq!(framed["headings"][0]["id"], "retained-native-documentation");
    assert!(framed["body_html"].as_str().expect("framed body").contains(
        "href=\"https://one.example/mount/docs/native.html#retained-native-documentation\""
    ));
    assert_eq!(
        framed["route"]["canonical_url"],
        "https://one.example/mount/docs/native.html"
    );
    assert_eq!(framed["route"]["output"], "docs/native.html");
    let framed_body = framed["body_html"].as_str().expect("file-route body");
    for link in [
        "<a href=\"https://one.example/mount/docs/native.html#retained-native-documentation\" data-gazette-link=\"doc:native#link-2\" data-gazette-destination-kind=\"internal-native\" data-gazette-destination-content=\"doc:native\" data-gazette-destination-site=\"site:one\" data-gazette-destination-theme=\"theme:one\">Dot</a>",
        "<a href=\"https://one.example/mount/docs/native.html?mode=1#retained-native-documentation\" data-gazette-link=\"doc:native#link-3\" data-gazette-destination-kind=\"internal-native\" data-gazette-destination-content=\"doc:native\" data-gazette-destination-site=\"site:one\" data-gazette-destination-theme=\"theme:one\">Query</a>",
        "<a href=\"https://one.example/mount/docs/native.html#retained-native-documentation\" data-gazette-link=\"doc:native#link-4\" data-gazette-destination-kind=\"internal-native\" data-gazette-destination-content=\"doc:native\" data-gazette-destination-site=\"site:one\" data-gazette-destination-theme=\"theme:one\">Repeated dot</a>",
    ] {
        assert!(
            framed_body.contains(link),
            "self-reference must stay on the exact file route: {framed_body}"
        );
    }
    let framed_html =
        std::str::from_utf8(bytes(tree, "docs/native.html")).expect("file-route HTML");
    assert!(framed_html.contains(framed_body));
    let authored = &model["pages"]["content:one"];
    assert_eq!(authored["headings"][0]["id"], "authored-explicit");
    assert!(
        authored["body_html"]
            .as_str()
            .expect("authored body")
            .contains("href=\"https://one.example/mount/guide/#authored-explicit\"")
    );
    for (path, page, expected_link) in [
        (
            "docs/library/index.html",
            root,
            "https://one.example/mount/docs/library/nested/#target",
        ),
        (
            "docs/library/nested/index.html",
            page_at(&model, "https://one.example/mount/docs/library/nested/"),
            "https://one.example/mount/docs/library/?mode=1#caf%C3%A9",
        ),
        (
            "docs/library/nested/guide/index.html",
            guide,
            "https://one.example/mount/docs/native.html",
        ),
    ] {
        let html = std::str::from_utf8(bytes(tree, path)).expect("HTML");
        let body = page["body_html"].as_str().expect("shared rendered body");
        assert!(
            html.contains(body),
            "Theme must consume machine model body at {path}"
        );
        assert!(
            body.contains(&format!("href=\"{expected_link}\"")),
            "resolved link missing: {body}"
        );
    }
    let nested = page_at(&model, "https://one.example/mount/docs/library/nested/");
    assert!(
        nested["body_html"]
            .as_str()
            .expect("body")
            .contains("href=\"https://one.example/mount/docs/library/nested/guide/\"")
    );
    let body = root["body_html"].as_str().expect("root body");
    assert!(body.contains("[example](missing.md#absent)"));
    assert!(body.contains("[untouched](outside.md#absent)"));
    assert!(body.contains("src=\"https://example.invalid/missing.md#absent\""));
    for id in [
        "repeat-1",
        "repeat-2",
        "repeat",
        "hehe",
        "ri-ben",
        "café",
        "section",
        "literal%id",
    ] {
        assert!(
            body.contains(&format!("id=\"{id}\"")),
            "missing retained heading {id}"
        );
    }
    for fragment in ["repeat-1", "caf%C3%A9", "ri-ben", "literal%25id"] {
        assert!(body.contains(&format!(
            "href=\"https://one.example/mount/docs/library/#{fragment}\""
        )));
    }
    assert!(body.contains("class=\"kept\""));
    let root_html = std::str::from_utf8(bytes(tree, "docs/library/index.html")).expect("root HTML");
    assert!(root_html.contains("<a data-level=\"3\" href=\"#hehe\">héhé</a>"));
    assert!(root_html.contains("<a data-level=\"2\" href=\"#caf%C3%A9\">Unicode</a>"));
    assert!(root_html.contains("<a data-level=\"2\" href=\"#literal%25id\">Percent</a>"));
    assert!(
        nested["body_html"]
            .as_str()
            .expect("nested body")
            .contains("href=\"https://one.example/mount/docs/library/#literal%25id\"")
    );
    let model_text = std::str::from_utf8(bytes(tree, "model.json")).expect("JSON bytes");
    assert!(!model_text.contains(fixture.directory.path().to_str().expect("native root")));
    assert!(!model_text.contains("source:native"));
    assert!(!model_text.contains("native-docs/"));

    let parent = tempdir().expect("explicit local stage parent");
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments.extend(["--output-parent", parent.path().to_str().expect("parent")]);
    let output = cli(&config, &arguments);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value = serde_json::from_slice(&output.stdout).expect("CLI retained receipt");
    let directory =
        std::path::Path::new(receipt["directory"].as_str().expect("retained directory"));
    assert_eq!(
        receipt["inventory"],
        serde_json::to_value(tree.inventory()).expect("SDK inventory")
    );
    assert_eq!(
        fs::read(directory.join("model.json")).expect("CLI machine bytes"),
        bytes(tree, "model.json")
    );
    for entry in tree.inventory().entries() {
        if matches!(entry.content, source::Content::File { .. }) {
            assert_eq!(
                fs::read(directory.join(entry.path.as_str())).expect("actual output"),
                tree.file(&entry.path).expect("SDK output")
            );
        }
    }
    fs::write(
        fixture.directory.path().join("project/README.md"),
        "# Replaced after capture\n",
    )
    .expect("disposable native edit");
    let retained = render(&inputs).expect("captured native AST retained");
    assert_eq!(
        retained.site().output().inventory().identity(),
        tree.inventory().identity()
    );
    assert_cli_failure_preserves_parent(&config, "source_content_mismatch");
}

#[test]
fn native_derived_identity_survives_body_pin_mount_and_logical_prefix_changes() {
    let before = Fixture::new(&[("project/README.md", b"# Before\nOriginal body.\n")]);
    let after = Fixture::new(&[("project/README.md", b"# After\nChanged body.\n")]);
    assert_ne!(before.commit, after.commit);
    let before_inputs = acquire(&native_configuration(&before));
    let before_render = render(&before_inputs).expect("original native document");
    let before_model: Value =
        serde_json::from_slice(bytes(before_render.site().output(), "model.json"))
            .expect("original model");
    let after_config = native_configuration(&after)
        .replace("target='native-docs'", "target='changed-logical-prefix'")
        .replace(
            "route_prefix='/docs/library/'",
            "route_prefix='/reference/'",
        )
        .replace(
            "https://one.example/mount/",
            "https://one.example/changed-mount/",
        );
    let after_inputs = acquire(&after_config);
    let after_render =
        render(&after_inputs).expect("same native coordinate, updated source/presentation");
    let after_model: Value =
        serde_json::from_slice(bytes(after_render.site().output(), "model.json"))
            .expect("updated model");
    let original = page_at(&before_model, "https://one.example/mount/docs/library/");
    let updated = page_at(&after_model, "https://one.example/changed-mount/reference/");
    assert_eq!(original["content"]["id"], updated["content"]["id"]);
    assert_eq!(original["content"]["title"], "Before");
    assert_eq!(updated["content"]["title"], "After");
    assert_ne!(
        before_render.site().output().inventory().identity(),
        after_render.site().output().inventory().identity()
    );
}

#[test]
fn native_profile_invalid_framing_routes_and_links_fail_before_stage_creation() {
    for document in [
        "---\ndraft: true\n---\n# Held\n",
        "---\n# Ambiguous opening\n",
        "\u{feff}---\ndraft: true\n---\n# Held\n",
        "--- # metadata\ndraft: true\n---\n# Held\n",
        "\n\n---\ndraft: true\n---\n# Held\n",
        "+++\ndraft=true\n+++\n# Wrong profile\n",
    ] {
        let fixture = Fixture::new(&[("project/README.md", document.as_bytes())]);
        assert_cli_failure_preserves_parent(
            &native_configuration(&fixture),
            "publication_authored",
        );
    }
    let collision = Fixture::new(&[
        ("project/README.md", b"# Readme\n"),
        ("project/index.md", b"# Index\n"),
    ]);
    assert_cli_failure_preserves_parent(&native_configuration(&collision), "publication_authored");
    for link in [
        "missing.md",
        "../../outside.md",
        "../one/page.md",
        "README.md#unknown",
        "#unknown",
        "?mode=1#unknown",
        "./#unknown",
        "#bad%",
        "#bad%2",
        "#bad%zz",
        "#%FF",
        "README.md#bad%",
        "README.md#%FF",
        "#%256Eative",
    ] {
        let document = format!("# Native\n\n[Invalid]({link})\n");
        let fixture = Fixture::new(&[("project/README.md", document.as_bytes())]);
        assert_cli_failure_preserves_parent(
            &native_configuration(&fixture),
            "markdown_link_resolution",
        );
    }
    for fragment in ["#unknown", "#bad%", "#%FF"] {
        let document = format!("{PAGE}\n[Invalid]({fragment})\n");
        let fixture = Fixture::new(&[("one/page.md", document.as_bytes())]);
        assert_cli_failure_preserves_parent(&fixture.config(), "markdown_link_resolution");
    }
    let missing_profile = Fixture::new(&[("project/README.md", b"# Native\n")]);
    assert_cli_failure_preserves_parent(
        &missing_profile.project_config(true),
        "publication_authored",
    );
}
