//! Explicit authored roles through native publication interfaces.

use super::*;

fn attribution_config(fixture: &Fixture) -> String {
    let mut config = fixture.config();
    for (id, kind) in [
        ("person:zed", "person"),
        ("organization:rights", "organization"),
        ("person:ada", "person"),
        ("person:container", "person"),
        ("person:foreign-unlinked", "person"),
        ("person:unused", "person"),
    ] {
        config.push_str(&format!("[[principals]]\nid='{id}'\nkind='{kind}'\n"));
    }
    config
}

#[test]
fn direct_cli_and_sdk_preserve_authored_roles_without_publisher_or_container_inheritance() {
    let declarations = "[{principal='person:zed',role='author'},{principal='organization:rights',role='rights-holder'},{principal='person:ada',role='author'}]";
    let page = PAGE.replace(
        "[content]\n",
        &format!("[content]\nattributions={declarations}\n"),
    );
    let home = HOME.replace(
        "[content]\n",
        "[content]\nattributions=[{principal='person:container',role='author'}]\n",
    );
    let unattributed = PAGE
        .replace("id='content:one'", "id='content:unattributed'")
        .replace(
            "route={path='/guide/',aliases=['/old.html']}",
            "route={path='/unattributed/'}",
        );
    let foreign = OTHER
        .replace("id='content:other'", "id='content:foreign-unlinked'")
        .replace("path='/elsewhere/'", "path='/foreign/'")
        .replace(
            "[content]\n",
            "[content]\nattributions=[{principal='person:foreign-unlinked',role='author'}]\n",
        );
    let template = format!(
        "{TEMPLATE}<ul>{{% for item in attributions %}}<li>{{{{ item.role }}}}:{{{{ item.principal.id }}}}:{{{{ item.principal.kind }}}}</li>{{% endfor %}}</ul><footer>publisher:{{{{ site.publisher }}}}</footer>"
    );
    let fixture = Fixture::new(&[
        ("one/page.md", page.as_bytes()),
        ("one/home.md", home.as_bytes()),
        ("one/unattributed.md", unattributed.as_bytes()),
        ("other/foreign.md", foreign.as_bytes()),
        ("theme/templates/page.html", template.as_bytes()),
    ]);
    let config = attribution_config(&fixture);
    let inputs = acquire(&config);
    let expected = render(&inputs).expect("SDK attribution render");
    let parent = tempdir().expect("local output parent");
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments.extend(["--output-parent", parent.path().to_str().expect("path")]);
    let output = cli(&config, &arguments);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value = serde_json::from_slice(&output.stdout).expect("CLI receipt");
    let directory =
        std::path::Path::new(receipt["directory"].as_str().expect("retained directory"));
    assert_eq!(
        source::inspect(directory, None, source::Limits::default())
            .expect("independent read-back")
            .identity(),
        expected.site().output().inventory().identity()
    );
    let model_bytes = fs::read(directory.join("model.json")).expect("machine output");
    assert_eq!(model_bytes, bytes(expected.site().output(), "model.json"));
    let model: Value = serde_json::from_slice(&model_bytes).expect("same semantic model");
    let roles = serde_json::json!([
        {"principal":{"id":"person:zed","kind":"person"},"role":"author"},
        {"principal":{"id":"organization:rights","kind":"organization"},"role":"rights-holder"},
        {"principal":{"id":"person:ada","kind":"person"},"role":"author"}
    ]);
    let attributed = &model["pages"]["content:one"];
    assert_eq!(
        attributed["attributions"], roles,
        "authored order is not principal-key order"
    );
    assert_eq!(attributed["site"]["publisher"], "owner:one");
    assert_eq!(
        attributed["content"]["attributions"][0]["principal"],
        "person:zed"
    );
    assert_eq!(
        attributed["content"]["attributions"][1]["role"],
        "rights-holder"
    );
    assert_eq!(
        model["pages"]["content:unattributed"]["attributions"],
        serde_json::json!([])
    );
    assert_eq!(
        model["pages"]["content:unattributed"]["collections"][0]["attributions"][0]["principal"],
        "person:container"
    );
    assert_eq!(
        model["pages"]["collection:home"]["attributions"][0]["principal"]["id"],
        "person:container"
    );
    let html = fs::read_to_string(directory.join("guide/index.html")).expect("Tera output");
    assert!(html.contains("<ul><li>author:person:zed:person</li><li>rights-holder:organization:rights:organization</li><li>author:person:ada:person</li></ul>"));
    assert!(html.contains("<footer>publisher:owner:one</footer>"));
    let empty =
        fs::read_to_string(directory.join("unattributed/index.html")).expect("unattributed HTML");
    assert!(empty.contains("<ul></ul><footer>publisher:owner:one</footer>"));
    assert!(!empty.contains("<li>author:"));
    let machine = std::str::from_utf8(&model_bytes).expect("UTF-8 model");
    for unrelated in ["person:foreign-unlinked", "person:unused"] {
        assert!(
            !machine.contains(unrelated),
            "no catalog-wide attribution leakage"
        );
        assert!(!html.contains(unrelated));
        assert!(
            receipt["inputs"]["compilation"]["workspace"]["principals"]
                .get(unrelated)
                .is_some(),
            "private input evidence remains complete"
        );
    }
    let corpus = inputs
        .parse_publication(Default::default(), &gazette_adapter_markdown::Comrak)
        .expect("retained syntax");
    let publication = corpus
        .resolve(Default::default())
        .expect("same source binding");
    let resolved = publication.authored();
    let origin = resolved
        .origin(
            &"content:one".parse().expect("content key"),
            &[
                toml::PathSegment::Key("content".into()),
                toml::PathSegment::Key("attributions".into()),
                toml::PathSegment::Index(0),
                toml::PathSegment::Key("principal".into()),
            ],
        )
        .expect("exact authored attribution origin");
    assert_eq!(origin.path.as_str(), "one/page.md");
    assert_eq!(
        origin.source_sha256,
        source::Sha256Digest::of(page.as_bytes())
    );
    let span = origin.span.expect("literal field coordinates");
    assert_eq!(&page[span.start..span.end], "'person:zed'");
}

#[test]
fn direct_cli_rejects_missing_wrong_namespace_duplicate_and_unknown_attributions_before_staging() {
    for (declarations, diagnostic) in [
        (
            "[{principal='person:missing',role='author'}]",
            "content_principal_reference",
        ),
        (
            "[{principal='site:one',role='author'}]",
            "content_principal_reference",
        ),
        (
            "[{principal='person:zed',role='author'},{principal='person:zed',role='author'}]",
            "content_duplicate_attribution",
        ),
        (
            "[{principal='person:zed',role='publisher'}]",
            "unknown variant",
        ),
    ] {
        let page = PAGE.replace(
            "[content]\n",
            &format!("[content]\nattributions={declarations}\n"),
        );
        let fixture = Fixture::new(&[("one/page.md", page.as_bytes())]);
        let config = attribution_config(&fixture);
        let error = render(&acquire(&config)).expect_err("invalid attribution has no fallback");
        assert!(
            error.to_string().contains(diagnostic),
            "{diagnostic}: {error}"
        );
        assert_cli_failure_preserves_parent(&config, "publication_authored");
    }
}
