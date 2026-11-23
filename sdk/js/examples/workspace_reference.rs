//! Test-only Rust reference for real Node-addon parity, not a product command.

use gazette_sdk::{
    authoring::session::{PageInputSource, Session, Source},
    document, markdown, toml, workspace,
};
use serde_json::{Value, json};

fn main() {
    let source = include_str!("../tests/fixtures/workspace.toml").replace('\n', "\r\n");
    let queries: Value = serde_json::from_str(include_str!("../tests/fixtures/queries.json"))
        .expect("synthetic query fixture");
    let parsed = workspace::ParsedWorkspace::parse(&source, toml::Limits::default())
        .expect("synthetic complete workspace");
    let field: workspace::FieldQuery =
        serde_json::from_value(queries["field"].clone()).expect("shared field query");
    let mut result = json!({
        "check": parsed.compilation(),
        "field": parsed.origin(&field).expect("explicit field")
    });
    for name in ["inherited", "overridden", "disabled"] {
        let query =
            serde_json::from_value(queries[name].clone()).expect("shared composition query");
        result[name] = serde_json::to_value(
            parsed
                .composition_origin(&query)
                .expect("resolved composition origin"),
        )
        .expect("native projection");
    }
    let invalid = source.replace("publisher='owner:one'", "publisher='owner:missing'");
    let error = workspace::ParsedWorkspace::parse(&invalid, toml::Limits::default())
        .expect_err("invalid complete graph");
    result["invalid"] =
        serde_json::to_value(workspace::Failure::from(&error)).expect("shared native failure");
    let markdown = include_str!("../tests/fixtures/markdown.md").replace('\n', "\r\n");
    let document = include_str!("../tests/fixtures/document.md").replace('\n', "\r\n");
    result["markdown"] = serde_json::to_value(
        markdown::inspect(&markdown, markdown::Limits::default(), false)
            .expect("Markdown inspection"),
    )
    .expect("Markdown reference");
    result["markdownHtml"] = serde_json::to_value(
        markdown::inspect(&markdown, markdown::Limits::default(), true).expect("Markdown HTML"),
    )
    .expect("Markdown HTML reference");
    result["document"] = serde_json::to_value(
        document::inspect(&document, document::Limits::default(), false)
            .expect("document inspection"),
    )
    .expect("document reference");
    result["documentHtml"] = serde_json::to_value(
        document::inspect(&document, document::Limits::default(), true).expect("document HTML"),
    )
    .expect("document HTML reference");
    let authoring_source = |id: &str, path: &str, text: &str| Source {
        id: id.into(),
        path: path.parse().expect("literal authoring path"),
        text: text.into(),
    };
    let page_workspace = include_str!("../tests/fixtures/page-package-workspace.toml");
    let page = include_str!("../tests/fixtures/page-package.md");
    let standalone = include_str!("../tests/fixtures/page-package-standalone.md");
    let image = include_bytes!("../tests/fixtures/page-package-image.png");
    let session = Session::new_with_manuals_page_inputs_and_package_baselines(
        authoring_source("workspace", "workspace.toml", page_workspace),
        [
            authoring_source("page", "content/guide/index.md", page),
            authoring_source("standalone", "content/standalone.md", standalone),
        ],
        [],
        [PageInputSource {
            id: "image".into(),
            path: "content/guide/image.png"
                .parse()
                .expect("literal companion path"),
            bytes: image.to_vec(),
        }],
        [],
        Default::default(),
    )
    .expect("exact supplied authoring session");
    result["pagePackage"] = json!({
        "document": session.page_package("page").expect("document query"),
        "companion": session.page_package("image").expect("companion query"),
        "standalone": session.page_package("standalone").expect("standalone query"),
    });
    println!(
        "{}",
        serde_json::to_string(&result).expect("reference JSON")
    );
}
