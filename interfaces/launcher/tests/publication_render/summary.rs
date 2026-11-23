//! Explicit Markdown excerpts cross real local outputs without becoming descriptions.

use std::convert::Infallible;

use gazette_sdk::html::{self, TextKind};
use serde_json::Value;

use super::{Fixture, PAGE, RENDER_ARGUMENTS, acquire, bytes, cli, fs, render, tempdir};

const BODY: &str = "# Heading {#continue-reading}\n\nLead **strong** [next](next/?mode=1) and [here](#continue-reading). ![Picture](picture.png)[^note]\n\n<!-- more -->\n\nRemaining body.\n\n[^note]: Footnote excluded from excerpt.\n";
const TEMPLATE: &[u8] = b"<!doctype html><html><head><title>Summary fixture</title></head><body>{% if summary is defined %}<section id=\"excerpt\">{{ summary.html | safe }}</section><a id=\"continue\" href=\"{{ summary.continuation }}\">Continue</a><script type=\"application/json\">{{ summary | json_script }}</script>{% endif %}<script type=\"application/ld+json\">{{ json_ld | json_script }}</script><main>{{ body_html | safe }}</main></body></html>";

fn enable(config: &str) -> String {
    config.replace(
        "public_base_url=",
        "machine={json_ld=true,search='lexical'}\npublic_base_url=",
    )
}

fn scripts(bytes: &[u8]) -> Vec<Value> {
    let mut values = Vec::new();
    html::inspect(bytes, html::Limits::default(), |kind, value| {
        match kind {
            TextKind::JsonScript => {
                values.push(serde_json::from_str(value).expect("whole JSON script"))
            }
            TextKind::Script | TextKind::ModuleScript | TextKind::DataScript => {
                panic!("excerpt generated unexpected script")
            }
            _ => (),
        }
        Ok::<_, Infallible>(())
    })
    .expect("complete HTML tokenization");
    values
}

#[test]
fn excerpt_theme_model_search_and_json_ld_share_retained_text_links_and_assets() {
    let page = PAGE.replace("**Retained** body\n", BODY).replace(
        "description='The retained local article.'\n",
        "description='An explicit plain description, not the excerpt.'\n",
    );
    let fixture = Fixture::new(&[
        ("one/page.md", page.as_bytes()),
        ("one/picture.png", b"retained opaque picture bytes"),
        ("theme/templates/page.html", TEMPLATE),
    ]);
    let config = enable(&fixture.config()).replace(
        "target='one'}",
        "target='one',static_assets={route_prefix='/media/'}}",
    );
    let inputs = acquire(&config);
    let rendered = render(&inputs).expect("complete retained excerpt publication");
    let tree = rendered.site().output();
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).expect("model");
    let page = &model["pages"]["content:one"];
    let summary = &page["summary"];
    let text = summary["text"].as_str().expect("plain summary text");
    let excerpt = summary["html"].as_str().expect("summary fragment");
    let body = page["body_html"].as_str().expect("body fragment");
    assert!(text.contains("Lead strong next and here."));
    assert!(!text.contains("Remaining body"));
    assert!(!text.contains("Footnote excluded"));
    assert!(!text.contains("<!-- more -->"));
    assert!(!excerpt.contains("footnote"));
    assert!(body.contains("Remaining body"));
    assert!(body.contains("Footnote excluded"));
    assert!(
        !page["body_text"]
            .as_str()
            .expect("body text")
            .contains("<!-- more -->")
    );
    for url in [
        "https://one.example/mount/guide/next/?mode=1",
        "https://one.example/mount/guide/#continue-reading",
        "https://one.example/mount/media/picture.png",
    ] {
        assert!(excerpt.contains(url), "excerpt URL {url}");
        assert!(body.contains(url), "body URL {url}");
    }
    assert_eq!(
        summary["continuation"],
        "https://one.example/mount/guide/#continue-reading-1"
    );
    assert!(body.contains("id=\"continue-reading-1\""));
    assert_eq!(
        page["headings"][0]["fragment"], "#continue-reading",
        "heading navigation stays page-relative"
    );
    let start = summary["marker"]["start"].as_u64().expect("marker start") as usize;
    let end = summary["marker"]["end"].as_u64().expect("marker end") as usize;
    assert_eq!(BODY[start..end].trim(), "<!-- more -->");
    assert_eq!(
        page["assets"].as_array().expect("referenced assets").len(),
        1,
        "one obligation for body and excerpt"
    );
    assert_eq!(
        bytes(tree, "media/picture.png"),
        b"retained opaque picture bytes"
    );
    let ld = &page["json_ld"];
    assert_eq!(ld["abstract"], summary["text"]);
    assert_eq!(
        ld["description"],
        "An explicit plain description, not the excerpt."
    );
    let search: Vec<Value> = serde_json::from_slice(bytes(tree, "search.json")).expect("search");
    let record = search
        .iter()
        .find(|record| record["content"]["id"] == "content:one")
        .expect("search document");
    assert_eq!(record["summary"], summary["text"]);
    assert!(
        record["summary"].is_string(),
        "search does not duplicate excerpt HTML or coordinates"
    );
    assert_eq!(record["body"], page["body_text"]);
    assert_eq!(record["content"], page["content"]);
    let site_ld: Value = serde_json::from_slice(bytes(tree, "site.jsonld")).expect("site LD");
    assert_eq!(
        site_ld["@graph"]
            .as_array()
            .expect("graph")
            .iter()
            .find(|node| node["identifier"] == "content:one")
            .expect("LD node"),
        ld
    );
    assert_eq!(
        scripts(bytes(tree, "guide/index.html")),
        [summary.clone(), ld.clone()]
    );
    let html = std::str::from_utf8(bytes(tree, "guide/index.html")).expect("HTML");
    assert!(html.contains(&format!("<section id=\"excerpt\">{excerpt}</section>")));
    assert!(html.contains(&format!("<main>{body}</main>")));
    let home = &model["pages"]["collection:home"];
    assert!(home.get("summary").is_none());
    assert!(home["json_ld"].get("abstract").is_none());
    assert!(
        search
            .iter()
            .find(|record| record["content"]["id"] == "collection:home")
            .expect("home search")
            .get("summary")
            .is_none()
    );

    let parent = tempdir().expect("stage parent");
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments.extend(["--output-parent", parent.path().to_str().expect("parent")]);
    let output = cli(&config, &arguments);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value = serde_json::from_slice(&output.stdout).expect("CLI receipt");
    assert_eq!(
        receipt["inventory"],
        serde_json::to_value(tree.inventory()).expect("SDK inventory")
    );
    let stage = std::path::Path::new(receipt["directory"].as_str().expect("stage"));
    for path in [
        "model.json",
        "search.json",
        "site.jsonld",
        "guide/index.html",
        "media/picture.png",
    ] {
        assert_eq!(
            fs::read(stage.join(path)).expect("staged output"),
            bytes(tree, path)
        );
    }
    fs::write(
        fixture.directory.path().join("one/page.md"),
        "changed native body",
    )
    .expect("fixture edit");
    fs::write(
        fixture.directory.path().join("one/picture.png"),
        b"changed bytes",
    )
    .expect("fixture asset edit");
    let repeated = render(&inputs).expect("retained repeat");
    assert_eq!(
        repeated.site().output().inventory().identity(),
        tree.inventory().identity()
    );
    assert_eq!(
        bytes(repeated.site().output(), "model.json"),
        bytes(tree, "model.json")
    );
}

#[test]
fn a_valid_excerpt_never_hides_late_errors_or_stages_a_successful_subset() {
    let parent = tempdir().expect("stage parent");
    fs::write(parent.path().join("keep"), b"unrelated output").expect("sentinel");
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments.extend(["--output-parent", parent.path().to_str().expect("parent")]);
    for (tail, code) in [
        (
            "<script>unavailable</script>\n",
            "markdown_raw_html_unavailable",
        ),
        ("[Bad](javascript:alert)\n", "markdown_unsafe_url"),
        ("[Missing](#absent)\n", "markdown_link_resolution"),
        ("[Encoded](%2e%2e/outside/)\n", "markdown_link_resolution"),
        ("<!-- more -->\n", "markdown_summary"),
    ] {
        let page = PAGE.replace(
            "**Retained** body\n",
            &format!("Good prefix.\n\n<!-- more -->\n\n{tail}"),
        );
        let fixture = Fixture::new(&[("one/page.md", page.as_bytes())]);
        let config = enable(&fixture.config());
        let inputs = acquire(&config);
        assert_eq!(render(&inputs).expect_err("no partial site").code(), code);
        let output = cli(&config, &arguments);
        assert!(!output.status.success(), "accepted late failure {tail}");
        assert!(output.stdout.is_empty(), "no successful partial receipt");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(code),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(fs::read_dir(parent.path()).expect("parent").count(), 1);
        assert_eq!(
            fs::read(parent.path().join("keep")).expect("sentinel"),
            b"unrelated output"
        );
    }
}
