//! Workspace source associations through real framed requests and the shared SDK owner.

use super::{
    footnotes::{definition, references, response},
    *,
};
use gazette_sdk::{authoring::session, source};

fn workspace() -> String {
    "principals=[{name='é🦀 owner:one',id='owner:one',kind='person'}]\r\nsites=[{id='site:one',publisher=\"owner:\\u006fne\",deployable=false}]\r\n".into()
}

fn selected_workspace(text: &str, broken: bool) -> Value {
    let mut value = if broken {
        selected(&[("untitled:broken.md", "broken.md", "+++\n[broken\n+++\n")])
    } else {
        initialize()
    };
    value["params"]["initializationOptions"]["workspace"]["text"] = json!(text);
    value
}

fn native(text: &str) -> session::Session {
    session::Session::new(
        session::Source {
            id: URI.into(),
            path: "workspace.toml".parse().expect("path"),
            text: text.into(),
        },
        [],
        session::Limits::default(),
    )
    .expect("native selected workspace")
}

fn point(text: &str, offset: usize) -> Value {
    let prefix = &text[..offset];
    json!({"line":prefix.matches('\n').count(),"character":prefix.rsplit('\n').next().expect("line prefix").encode_utf16().count()})
}

fn located(text: &str, location: source::Location) -> Value {
    assert!(
        location.span.slice(text).is_some(),
        "native checked construct"
    );
    json!({"uri":URI,"range":{"start":point(text,location.span.start),"end":point(text,location.span.end)}})
}

fn located_in(uri: &str, text: &str, location: source::Location) -> Value {
    assert!(
        location.span.slice(text).is_some(),
        "native checked construct"
    );
    json!({"uri":uri,"range":{"start":point(text,location.span.start),"end":point(text,location.span.end)}})
}

fn symbols(id: i32) -> Value {
    request(
        id,
        "textDocument/documentSymbol",
        json!({"textDocument":{"uri":URI}}),
    )
}

#[test]
fn framed_resource_alias_definition_completion_hover_and_symbols_preserve_native_meaning() {
    let text = "# é🦀\r\n[design_inputs]\r\n\"tok\\u0065ns\"='source:tokens'\r\n\
[[principals]]\r\nid='person:one'\r\nkind='person'\r\n\
[[sites]]\r\nid='site:one'\r\npublisher='person:one'\r\ndeployable=false\r\n\
[[sources]]\r\nid='source:tokens'\r\nlocation={kind='filesystem',root='/not-opened/tokens'}\r\n\
[[theme_packages]]\r\npackage='theme:one'\r\nsource='source:tokens'\r\nmanifest='theme.toml'\r\nresource_inputs={'shared/tokens.css'={input='tokens',entry='dist/tokens.css'}}\r\n";
    let state = native(text);
    let analysis = state.analysis().as_ref().unwrap();
    let path = "workspace.toml".parse().unwrap();
    let offset = text.find("input='tokens'").unwrap() + "input=".len();
    let navigation = analysis
        .source_navigation(&path, text, offset)
        .unwrap()
        .unwrap();
    let (result, messages) = selected_session(
        selected_workspace(text, false),
        [
            definition(2, URI, point(text, offset)),
            references(3, URI, point(text, offset), true),
            request(
                4,
                "textDocument/completion",
                json!({"textDocument":{"uri":URI},"position":point(text, offset)}),
            ),
            request(
                5,
                "textDocument/hover",
                json!({"textDocument":{"uri":URI},"position":point(text, offset)}),
            ),
            symbols(6),
        ],
        Limits::default(),
    );
    result.unwrap();
    assert_eq!(
        response(&messages, 2)["result"],
        located(text, navigation.definition.location)
    );
    let references = response(&messages, 3)["result"].as_array().unwrap();
    assert_eq!(references.len(), 2);
    assert!(references.contains(&located(text, navigation.definition.location)));
    assert!(references.contains(&located(text, navigation.occurrence)));
    let items = response(&messages, 4)["result"]["items"]
        .as_array()
        .unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["label"], "tokens");
    assert_eq!(items[0]["textEdit"]["newText"], "\"tokens\"");
    assert_eq!(items[0]["data"]["symbol"]["kind"], "resource-input");
    assert_eq!(items[0]["data"]["preimage"], "'tokens'");
    assert_eq!(items[0]["data"]["definition"], json!(navigation.definition));
    assert_eq!(
        items[0]["data"]["source_sha256"],
        json!(source::Sha256Digest::of(text.as_bytes()))
    );
    assert_eq!(
        response(&messages, 5)["result"]["contents"]["value"],
        "Resolved resource-input: tokens"
    );
    let symbols = response(&messages, 6)["result"].as_array().unwrap();
    let alias = symbols
        .iter()
        .find(|symbol| symbol["name"] == "tokens")
        .unwrap();
    assert_eq!(
        alias["range"],
        located(text, navigation.definition.location)["range"]
    );
    assert_eq!(alias["selectionRange"], alias["range"]);
}

#[test]
fn workspace_symbols_and_references_match_native_sdk_with_unicode_and_crlf() {
    let text = workspace();
    let state = native(&text);
    let analysis = state.analysis().as_ref().expect("current analysis");
    let path = "workspace.toml".parse().expect("path");
    let offset = text
        .find("owner:\\u006fne")
        .expect("escaped native reference");
    let navigation = analysis
        .source_navigation(&path, &text, offset)
        .expect("native lookup")
        .expect("reference");
    let declarations = analysis
        .workspace_symbols(&path, &text)
        .expect("native symbols")
        .expect("workspace role");
    let decoy = text.find("owner:one").expect("display-name decoy");
    let (result, messages) = selected_session(
        selected_workspace(&text, false),
        [
            definition(2, URI, point(&text, offset)),
            references(3, URI, point(&text, offset), false),
            references(4, URI, point(&text, offset), true),
            symbols(5),
            definition(6, URI, point(&text, decoy)),
        ],
        Limits::default(),
    );
    result.expect("complete framed workspace protocol");
    assert_eq!(
        response(&messages, 2)["result"],
        located(&text, navigation.definition.location)
    );
    let expected: Vec<_> = navigation
        .references
        .iter()
        .map(|location| located(&text, location.location))
        .collect();
    assert_eq!(response(&messages, 3)["result"], json!(expected));
    let all = response(&messages, 4)["result"]
        .as_array()
        .expect("references with declaration");
    assert_eq!(all.len(), expected.len() + 1);
    assert!(all.contains(&located(&text, navigation.definition.location)));
    for reference in expected {
        assert!(all.contains(&reference));
    }
    assert!(
        response(&messages, 6)["result"].is_null(),
        "display text does not become a reference"
    );
    let actual = response(&messages, 5)["result"]
        .as_array()
        .expect("declaration symbols");
    assert_eq!(actual.len(), declarations.len());
    for (actual, native) in actual.iter().zip(declarations) {
        assert_eq!(actual["name"], native.name());
        assert_eq!(actual["kind"], 19, "standard Object symbol kind");
        assert_eq!(actual["range"], located(&text, native.location())["range"]);
        assert_eq!(actual["selectionRange"], actual["range"]);
    }
    assert!(
        point(&text, navigation.definition.location.span.start)["character"]
            .as_u64()
            .expect("column")
            < navigation.definition.location.span.start as u64,
        "Unicode bytes are converted, not used as UTF-16 columns"
    );
}

#[test]
fn workspace_navigation_survives_corpus_failure_but_rejects_invalid_current_workspace() {
    let text = workspace();
    let changed = format!("# changed prefix é🦀\r\n{text}");
    let offset = text.find("owner:\\u006fne").expect("reference");
    let changed_offset = changed.find("owner:\\u006fne").expect("moved reference");
    let (result, messages) = selected_session(
        selected_workspace(&text, true),
        [
            definition(2, URI, point(&text, offset)),
            symbols(3),
            open(URI, 1, &text),
            change(URI, 2, &changed),
            definition(4, URI, point(&changed, changed_offset)),
            change(URI, 1, &text),
            definition(5, URI, point(&changed, changed_offset)),
            change(URI, 3, "[broken\n"),
            definition(6, URI, json!({"line":0,"character":0})),
            symbols(7),
            close(URI),
            definition(8, URI, point(&text, offset)),
            symbols(9),
        ],
        Limits::default(),
    );
    result.expect("framed overlay lifecycle");
    for id in [2, 3, 4, 5, 8, 9] {
        assert!(
            response(&messages, id).get("error").is_none(),
            "workspace availability independent of invalid Markdown: {}",
            response(&messages, id)
        );
    }
    assert_ne!(
        response(&messages, 2)["result"],
        response(&messages, 4)["result"]
    );
    assert_eq!(
        response(&messages, 4)["result"],
        response(&messages, 5)["result"],
        "stale version is a no-op"
    );
    for id in [6, 7] {
        assert!(response(&messages, id).get("error").is_some());
        assert!(response(&messages, id).get("result").is_none());
    }
    assert_eq!(
        response(&messages, 2)["result"],
        response(&messages, 8)["result"],
        "close restores exact selected baseline"
    );
    assert_eq!(
        response(&messages, 3)["result"],
        response(&messages, 9)["result"]
    );
}

#[test]
fn modular_workspace_navigation_uses_cross_document_graph_and_current_overlay_versions() {
    const SITE_URI: &str = "file:///selected/sites/one.toml";
    let root = "includes=['sites/one.toml']\n[[principals]]\nid='owner:publisher'\nkind='person'\n";
    let site = "[[sites]]\nid='site:one'\npublisher='owner:publisher'\ndeployable=false\n";
    let changed = site.replace("owner:publisher", "owner:missing");
    let reference = site.find("owner:publisher").expect("reference");
    let changed_reference = changed.find("owner:missing").expect("changed reference");
    let definition_span = source::Span {
        start: root.find("'owner:publisher'").expect("definition"),
        end: root.find("'owner:publisher'").expect("definition") + "'owner:publisher'".len(),
    };
    let mut init = initialize();
    init["params"]["initializationOptions"]["workspace"]["path"] = json!("gazette.toml");
    init["params"]["initializationOptions"]["workspace"]["text"] = json!(root);
    init["params"]["initializationOptions"]["workspace_documents"] = json!([{
        "uri": SITE_URI,
        "path": "sites/one.toml",
        "text": site
    }]);
    let (result, messages) = selected_session(
        init,
        [
            definition(2, SITE_URI, point(site, reference)),
            request(
                6,
                "textDocument/completion",
                json!({"textDocument":{"uri":SITE_URI},"position":point(site,reference)}),
            ),
            open(SITE_URI, 7, site),
            change(SITE_URI, 8, &changed),
            definition(3, SITE_URI, point(&changed, changed_reference)),
            change(SITE_URI, 7, site),
            definition(4, SITE_URI, point(&changed, changed_reference)),
            close(SITE_URI),
            definition(5, SITE_URI, point(site, reference)),
        ],
        Limits::default(),
    );
    result.expect("modular workspace protocol");
    let items = response(&messages, 6)["result"]["items"]
        .as_array()
        .expect("complete cross-document candidates");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["data"]["path"], "sites/one.toml");
    assert_eq!(
        items[0]["data"]["source_sha256"],
        json!(source::Sha256Digest::of(site.as_bytes()))
    );
    assert_eq!(items[0]["data"]["definition"]["path"], "gazette.toml");
    assert_eq!(
        items[0]["data"]["definition"]["source_sha256"],
        json!(source::Sha256Digest::of(root.as_bytes()))
    );
    assert_eq!(
        items[0]["data"]["definition"]["location"],
        json!(source::Location {
            span: definition_span,
            precision: source::Precision::Construct,
        })
    );
    assert_eq!(
        response(&messages, 2)["result"],
        located_in(
            URI,
            root,
            source::Location {
                span: definition_span,
                precision: source::Precision::Construct,
            }
        )
    );
    assert!(response(&messages, 3)["result"].is_null());
    assert_eq!(
        response(&messages, 3)["result"],
        response(&messages, 4)["result"],
        "a stale document version cannot revive an earlier graph"
    );
    assert_eq!(
        response(&messages, 5)["result"],
        response(&messages, 2)["result"]
    );
}
