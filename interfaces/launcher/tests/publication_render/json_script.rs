//! Script transport consumes real publication fields without reconstructing their meaning.

use std::{collections::BTreeMap, convert::Infallible};

use gazette_sdk::html::{self, TextKind};
use serde_json::{Value, json};

use super::{
    Fixture, PAGE, PROJECT_DOC, RENDER_ARGUMENTS, acquire, build, bytes, cli, fs, render, source,
    tempdir,
};

const FIELDS: [&str; 6] = [
    "content",
    "projects",
    "attributions",
    "collections",
    "related",
    "provenance",
];

fn template() -> String {
    let mut text = String::from(
        "<!doctype html><html><head><title>Transport fixture</title></head><body><h1>{{ content.title }}</h1>",
    );
    for field in FIELDS {
        // Separate JSON data blocks test each existing typed value; none claims
        // that these local inspection records are Schema.org JSON-LD.
        if field == "provenance" {
            text.push_str("{% if provenance is defined %}");
        }
        text.push_str(&format!(
            "<script type=\"application/json\">{{{{ {field} | json_script }}}}</script>"
        ));
        if field == "provenance" {
            text.push_str("{% endif %}");
        }
    }
    text.push_str("<p>after-data-sentinel</p></body></html>");
    text
}

fn decoded(bytes: &[u8]) -> (Vec<Value>, Vec<String>) {
    let mut data = Vec::new();
    let mut visible = Vec::new();
    html::inspect(bytes, html::Limits::default(), |kind, value| {
        match kind {
            TextKind::JsonScript => data.push(
                serde_json::from_str(value)
                    .expect("complete JSON in a single HTML script-data run"),
            ),
            TextKind::Text => visible.push(value.to_owned()),
            TextKind::Script
            | TextKind::ModuleScript
            | TextKind::DataScript
            | TextKind::ExternalScript
            | TextKind::EventAttribute => {
                panic!("data created an unexpected script")
            }
            TextKind::Style | TextKind::StyleAttribute => panic!("data created CSS"),
            TextKind::Attribute => assert_eq!(
                value, "application/json",
                "only the declared script type attributes exist"
            ),
        }
        Ok::<_, Infallible>(())
    })
    .expect("actual HTML tokenizer accepts the full output");
    (data, visible)
}

#[test]
fn script_data_preserves_shared_publication_values_through_html_cli_and_staging() {
    let title = "</ScRiPt ><script>injected()</script><!--<script>--> & ' \" \\ Ω\u{2028}\u{2029}";
    let doc = PROJECT_DOC
        .replace("title='Native documentation'", &format!("title={}", serde_json::to_string(title).expect("quoted TOML title")))
        .replace("kind='about'}]", "kind='about'},{project='project:native',kind='mentions'}]")
        .replace("[content]\n", "[content]\nattributions=[{principal='person:author',role='author'},{principal='organization:rights',role='rights-holder'}]\n");
    let template = template();
    let fixture = Fixture::new(&[
        ("project/guide.md", doc.as_bytes()),
        ("theme/templates/page.html", template.as_bytes()),
    ]);
    let config = format!(
        "{}[[principals]]\nid='person:author'\nkind='person'\n[[principals]]\nid='organization:rights'\nkind='organization'\n",
        fixture.project_config(true)
    );
    let inputs = acquire(&config);
    let rendered = render(&inputs).expect("connected native publication rendering");
    let tree = rendered.site().output();
    let model: Value =
        serde_json::from_slice(bytes(tree, "model.json")).expect("actual machine output");
    let page = &model["pages"]["doc:native"];
    let (data, visible) = decoded(bytes(tree, "docs/native/index.html"));
    assert_eq!(data, FIELDS.map(|field| page[field].clone()));
    assert_eq!(visible, ["Transport fixture", title, "after-data-sentinel"]);
    assert_eq!(data[0]["title"], title);
    assert_eq!(data[1][0]["relationship"]["relation"], "mentions");
    assert_eq!(data[1][1]["relationship"]["relation"], "about");
    assert_eq!(data[2][0]["role"], "author");
    assert_eq!(data[2][1]["role"], "rights-holder");
    assert_eq!(
        data[5],
        json!({"site":"site:one","project":"project:native"})
    );
    assert!(
        !std::str::from_utf8(bytes(tree, "docs/native/index.html"))
            .expect("HTML")
            .contains(fixture.directory.path().to_str().expect("native path"))
    );

    // Escaping must not hide the original text from the existing disclosure
    // consumer: it tokenizes HTML, then decodes the script's JSON strings.
    let rules = BTreeMap::from([(
        "rule:script".parse().expect("rule"),
        "</ScRiPt >".to_owned(),
    )]);
    let probe = rendered
        .site()
        .probe_disclosure(&rules, Default::default())
        .expect("real disclosure path");
    let report = serde_json::to_value(probe).expect("probe evidence");
    assert!(
        report["findings"]
            .as_array()
            .expect("findings")
            .iter()
            .any(|finding| finding["path"] == "docs/native/index.html"
                && finding["rule"] == "rule:script"
                && finding["representation"] == "json-string")
    );

    let parent = tempdir().expect("native stage parent");
    let sdk_stage = build::stage_publication(
        &rendered,
        parent.path(),
        source::Limits::default(),
        16 << 10,
    )
    .expect("SDK stage");
    assert_eq!(
        fs::read(sdk_stage.path().join("docs/native/index.html")).expect("staged HTML"),
        bytes(tree, "docs/native/index.html")
    );
    sdk_stage.close().expect("scoped stage cleanup");
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments.extend(["--output-parent", parent.path().to_str().expect("parent")]);
    let output = cli(&config, &arguments);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value = serde_json::from_slice(&output.stdout).expect("CLI stage receipt");
    assert_eq!(
        receipt["inventory"],
        serde_json::to_value(tree.inventory()).expect("same inventory")
    );
    let directory = std::path::Path::new(receipt["directory"].as_str().expect("native output"));
    assert_eq!(
        fs::read(directory.join("docs/native/index.html")).expect("CLI HTML"),
        bytes(tree, "docs/native/index.html")
    );

    fs::write(
        fixture.directory.path().join("theme/templates/page.html"),
        "changed after capture",
    )
    .expect("fixture change");
    let repeated = render(&inputs).expect("retained-only repeated render");
    assert_eq!(
        repeated.site().output().inventory().identity(),
        tree.inventory().identity()
    );
}

#[test]
fn invalid_script_filter_arguments_fail_the_real_cli_without_a_partial_stage() {
    let fixture = Fixture::new(&[(
        "theme/templates/page.html",
        b"<script type=\"application/json\">{{ content | json_script(indent=2) }}</script>",
    )]);
    let parent = tempdir().expect("stage parent");
    fs::write(parent.path().join("keep"), b"unrelated bytes").expect("sentinel");
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments.extend(["--output-parent", parent.path().to_str().expect("parent")]);
    let result = cli(&fixture.config(), &arguments);
    assert!(!result.status.success());
    assert!(result.stdout.is_empty(), "no successful partial receipt");
    assert!(String::from_utf8_lossy(&result.stderr).contains("render_template"));
    assert_eq!(
        fs::read(parent.path().join("keep")).expect("sentinel"),
        b"unrelated bytes"
    );
    assert_eq!(
        fs::read_dir(parent.path())
            .expect("no partial stage")
            .count(),
        1
    );
}

#[test]
fn expanded_script_bytes_obey_the_connected_page_limit_without_poisoning_reuse() {
    let title = "<".repeat(200);
    let page = PAGE.replace("title='One <escaped>'", &format!("title='{title}'"));
    let fixture = Fixture::new(&[
        ("one/page.md", page.as_bytes()),
        (
            "theme/templates/page.html",
            b"<script type=\"application/json\">{{ content.title | json_script }}</script>",
        ),
    ]);
    let inputs = acquire(&fixture.config());
    let successful = render(&inputs).expect("complete baseline");
    let expected = bytes(successful.site().output(), "guide/index.html");
    assert!(
        expected.len() > title.len() * 5,
        "limit must count actual JSON escaping expansion"
    );
    for (limit, passes) in [
        (expected.len(), true),
        (expected.len() - 1, false),
        (expected.len(), true),
    ] {
        let mut limits = build::PublicationRenderLimits::default();
        limits.output.render.output_bytes = limit;
        let result = build::render_publication(
            &inputs,
            build::SiteRenderOptions {
                render_year: Some(
                    gazette_sdk::authored::RenderYear::new(2040).expect("fixture year"),
                ),
                graph: None,
                model_output: Some(&"model.json".parse().expect("model")),
                limits: source::Limits::default(),
            },
            limits,
        );
        if passes {
            let result = result.expect("exact bound or repeat after failure");
            assert_eq!(bytes(result.site().output(), "guide/index.html"), expected);
            assert_eq!(
                result.site().output().inventory().identity(),
                successful.site().output().inventory().identity()
            );
        } else {
            let error = result.expect_err("one byte below complete output is not success");
            assert_eq!(error.code(), "render_limit");
        }
    }
}
