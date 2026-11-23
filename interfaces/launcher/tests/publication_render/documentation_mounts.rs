//! Real native Theme documentation capture, links, provenance and initialized interfaces.

use super::*;
use serde_json::json;

fn fixture() -> Fixture {
    Fixture::new(&[
        ("project/README.md", b"# Manual\n\n[Guide](guide.md#details)\n"),
        ("project/guide.md", b"# Guide\n\n## Details\n\n[Home](README.md)\n"),
        ("theme/templates/page.html", b"<h1>{{ content.title }}</h1>{{ body_html | safe }}{% if documentation_mount is defined %}<aside>{{ documentation_mount.name }}:{{ documentation_mount.landing.route.canonical_url }}</aside>{% endif %}"),
    ])
}

fn configured(fixture: &Fixture, enabled: bool) -> String {
    format!(
        "{}\n[[sources]]\nid='source:manual'\nlocation={{kind='repository',root={:?},commit='{}',subdirectory='{}'}}\n[[documentation.mounts]]\nsite='site:one'\nlanding='content:one'\nname='Explicit product manual'\nsource='source:manual'\nroute_prefix='/manual/'\nrender={enabled}\ndiscoverable={enabled}\n{}",
        fixture.config(),
        fixture
            .directory
            .path()
            .to_str()
            .expect("fixture source root"),
        fixture.commit,
        if enabled {
            "project"
        } else {
            "unavailable-manual"
        },
        if enabled {
            "lowering={lifecycle='published',language='en-US'}\n"
        } else {
            ""
        }
    )
}

#[test]
fn disabled_documentation_mount_never_reads_missing_source_or_builds_routes() {
    let fixture = fixture();
    let config = configured(&fixture, false);
    let inputs = acquire(&config);
    assert!(
        !inputs
            .sources()
            .contains_key(&"source:manual".parse().expect("manual key"))
    );
    let output = render(&inputs).expect("disabled native mount does not fail ordinary publication");
    assert!(
        output
            .site()
            .output()
            .file(&"manual/index.html".parse().expect("manual output"))
            .is_none()
    );
    let cli = cli(
        &config,
        &["check", "--publication", "site:one", "--registry"],
    );
    assert!(
        cli.status.success(),
        "{}",
        String::from_utf8_lossy(&cli.stderr)
    );
}

#[test]
fn native_documentation_mount_uses_shared_routes_links_theme_and_exact_provenance() {
    let fixture = fixture();
    let config = configured(&fixture, true);
    let inputs = acquire(&config);
    let output = render(&inputs).expect("native Theme documentation");
    let tree = output.site().output();
    let html = std::str::from_utf8(bytes(tree, "manual/index.html"))
        .expect("native manual HTML")
        .replace("&#x2F;", "/");
    assert!(
        html.contains("https://one.example/mount/manual/guide/#details"),
        "{html}"
    );
    assert!(
        html.contains("Explicit product manual:https://one.example/mount/guide/"),
        "{html}"
    );
    let guide = std::str::from_utf8(bytes(tree, "manual/guide/index.html"))
        .expect("guide HTML")
        .replace("&#x2F;", "/");
    assert!(
        guide.contains("https://one.example/mount/manual/"),
        "{guide}"
    );
    let model: Value =
        serde_json::from_slice(bytes(tree, "model.json")).expect("same publication model");
    let manual = model["pages"]
        .as_object()
        .expect("page map")
        .iter()
        .find(|(_, page)| page["content"]["title"] == "Manual")
        .expect("native manual page");
    assert_eq!(manual.1["content"]["language"], "en-US");
    assert_eq!(manual.1["content"]["lifecycle"], "published");
    assert_eq!(
        manual.1["documentation_mount"]["landing"]["content"]["id"],
        "content:one"
    );
    assert_eq!(manual.1["provenance"]["site"], "site:one");
    assert!(manual.1["provenance"].get("project").is_none());
    assert_eq!(manual.1["projects"], json!([]));
    let parsed = inputs
        .parse_publication(Default::default(), &gazette_adapter_markdown::Comrak)
        .expect("exact native corpus");
    let resolved = parsed
        .resolve(Default::default())
        .expect("source-bound graph");
    let registry = serde_json::to_value(
        resolved
            .registry(Default::default())
            .expect("same registry"),
    )
    .expect("private registry evidence");
    assert_eq!(
        registry["model"]["provenance"][manual.0],
        json!({"source":"source:manual","entry":"README.md","project":null})
    );
    assert_eq!(
        tree.inventory().identity(),
        render(&inputs)
            .expect("retained replay")
            .site()
            .output()
            .inventory()
            .identity()
    );
    let parent = tempdir().expect("CLI output parent");
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments.extend([
        "--output-parent",
        parent.path().to_str().expect("output parent"),
    ]);
    let cli = cli(&config, &arguments);
    assert!(
        cli.status.success(),
        "{}",
        String::from_utf8_lossy(&cli.stderr)
    );
    let receipt: Value = serde_json::from_slice(&cli.stdout).expect("actual staged receipt");
    let directory = std::path::Path::new(receipt["directory"].as_str().expect("native stage"));
    assert_eq!(
        fs::read(directory.join("manual/index.html")).expect("actual CLI generated bytes"),
        bytes(tree, "manual/index.html")
    );
}

#[test]
fn enabled_documentation_mount_refuses_absent_foreign_or_unrouted_landing() {
    let fixture = fixture();
    let good = configured(&fixture, true);
    for config in [
        good.replace("landing='content:one'", "landing='content:missing'"),
        good.replace("landing='content:one'", "landing='content:other'"),
    ] {
        let cli = cli(
            &config,
            &["check", "--publication", "site:one", "--registry"],
        );
        assert!(!cli.status.success());
        assert!(
            String::from_utf8_lossy(&cli.stderr)
                .contains("publication_documentation_mount_landing"),
            "{}",
            String::from_utf8_lossy(&cli.stderr)
        );
        assert!(cli.stdout.is_empty());
    }
    let unrouted = Fixture::new(&[
        (
            "one/page.md",
            PAGE.replace("route={path='/guide/',aliases=['/old.html']}\n", "")
                .as_bytes(),
        ),
        ("project/README.md", b"# Manual\n"),
    ]);
    let cli = cli(
        &configured(&unrouted, true),
        &["check", "--publication", "site:one", "--registry"],
    );
    assert!(!cli.status.success());
    assert!(
        String::from_utf8_lossy(&cli.stderr).contains("publication_documentation_mount_landing")
    );
}

#[test]
fn initialized_mcp_materializes_same_native_mount_bytes_as_cli_and_sdk() {
    let fixture = fixture();
    let config = configured(&fixture, true);
    let inputs = acquire(&config);
    let rendered = render(&inputs).expect("native retained render");
    let parent = tempdir().expect("MCP stage parent");
    let arguments = json!({"publication":{"recipe_json":json!({"format":"gazette-local-publication-set/1","workspace":{"kind":"supplied","source":config},"sites":[{"site":"site:one","model_output":"model.json"}]}).to_string(),"local":true,"allow_read":true},"create_stage":true,"output_parent":parent.path(),"directory_bytes":1024});
    mcp_carrier::initialized_interact("2025-11-25", |_, send| {
        let response = send(
            json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"stage_rendered_site","arguments":arguments}}),
        );
        assert!(response.get("error").is_none(), "{response}");
        assert_eq!(response["result"]["isError"], false, "{response}");
        let receipt = &response["result"]["structuredContent"];
        let directory =
            std::path::Path::new(receipt["directory"].as_str().expect("MCP staged bytes"));
        for path in ["manual/index.html", "manual/guide/index.html"] {
            assert_eq!(
                fs::read(directory.join(path)).expect("native MCP output"),
                bytes(rendered.site().output(), path)
            );
        }
    });
}
