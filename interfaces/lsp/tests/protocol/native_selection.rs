//! Real rooted native capture through the framed public LSP, not editor doubles.
use super::{
    footnotes::{definition, position, response},
    *,
};

fn directory_uri(path: &std::path::Path) -> String {
    url::Url::from_directory_path(path)
        .expect("native root URI")
        .to_string()
}
fn file_uri(root: &std::path::Path, path: &str) -> String {
    url::Url::from_file_path(root.join(path))
        .expect("native file URI")
        .to_string()
}
fn locator(root: &str, entry: &str, path: &str) -> Value {
    json!({"root":root,"entry":entry,"path":path})
}
fn rooted(
    config: &std::path::Path,
    owners: &[(&str, &std::path::Path)],
    documents: Vec<Value>,
) -> Value {
    let mut roots = vec![json!({"id":"workspace","uri":directory_uri(config)})];
    roots.extend(
        owners
            .iter()
            .map(|(id, path)| json!({"id":id,"uri":directory_uri(path)})),
    );
    let folders: Vec<_> = roots
        .iter()
        .map(|root| json!({"uri":root["uri"],"name":root["id"]}))
        .collect();
    request(
        1,
        "initialize",
        json!({
            "processId":null,"rootUri":directory_uri(config),"workspaceFolders":folders,"capabilities":{},
            "initializationOptions":{"native_selection":{
                "allow_read":true,"roots":roots,"workspace":locator("workspace","gazette.toml","gazette.toml"),
                "workspace_documents":[],"documents":documents,"manuals":[],"page_inputs":[],"package_baselines":[]
            }}
        }),
    )
}
fn failure(input: Value, limits: Limits, reason: &str) {
    let (result, messages) = selected_session(input, [], limits);
    assert!(
        matches!(result, Err(Error::Protocol("exit before shutdown"))),
        "{result:?}"
    );
    assert_eq!(messages[0]["error"]["code"], -32602, "{messages:?}");
    assert!(
        messages[0]["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains(reason)),
        "{messages:?}"
    );
    assert!(
        !messages
            .iter()
            .any(|message| message["method"] == "textDocument/publishDiagnostics"),
        "no partial selected session"
    );
}

#[test]
fn distributed_native_files_preserve_cross_root_navigation_and_versioned_overlay_digests() {
    let config = tempfile::tempdir().expect("configuration root");
    let first = tempfile::tempdir().expect("first owner");
    let second = tempfile::tempdir().expect("second owner");
    std::fs::create_dir(config.path().join("sites")).expect("module directory");
    let root =
        "includes=['sites/one.toml']\r\nprincipals=[{id='owner:one',kind='person',name='é🦀'}]\r\n";
    let module = "sites=[{id='site:one',publisher='owner:one',deployable=false}]\r\n";
    std::fs::write(config.path().join("gazette.toml"), root).expect("root source");
    std::fs::write(config.path().join("sites/one.toml"), module).expect("module source");
    let collection = format!(
        "{}\n# Cross-root target\n",
        content("collection:guide", "", "kind='collection'")
    )
    .replace('\n', "\r\n");
    let page = format!(
        "{}\n[Guide](guide.md#cross-root-target)\n",
        content(
            "content:one",
            "collections=['collection:guide']\n",
            "kind='page'\nfamily={kind='article'}",
        )
    )
    .replace('\n', "\r\n");
    std::fs::write(first.path().join("guide.md"), &collection).expect("collection source");
    std::fs::write(second.path().join("page%20é.md"), &page).expect("literal page source");
    std::fs::write(first.path().join("unselected.md"), b"+++\n[broken\n+++")
        .expect("unselected decoy");
    let collection_uri = file_uri(first.path(), "guide.md");
    let page_uri = file_uri(second.path(), "page%20é.md");
    let mut input = rooted(
        config.path(),
        &[("collection", first.path()), ("article", second.path())],
        vec![
            locator("collection", "guide.md", "content/guide.md"),
            locator("article", "page%20é.md", "content/page.md"),
        ],
    );
    input["params"]["initializationOptions"]["native_selection"]["workspace_documents"] =
        json!([locator("workspace", "sites/one.toml", "sites/one.toml")]);
    let changed = collection
        .replace("collection:guide", "collection:changed")
        .replace("Cross-root target", "Changed target");
    let (result, messages) = selected_session(
        input,
        [
            definition(2, &page_uri, position(&page, "collection:guide", 0)),
            definition(6, &page_uri, position(&page, "[Guide]", 0)),
            footnotes::references(7, &page_uri, position(&page, "collection:guide", 0), false),
            open_content(&page_uri, 1, &page),
            open_content(&collection_uri, 7, &collection),
            change(&collection_uri, 8, &changed),
            definition(3, &page_uri, position(&page, "collection:guide", 0)),
            change(&collection_uri, 7, &collection),
            definition(4, &page_uri, position(&page, "collection:guide", 0)),
            close(&collection_uri),
            definition(5, &page_uri, position(&page, "collection:guide", 0)),
        ],
        Limits::default(),
    );
    result.expect("real distributed LSP session");
    assert_eq!(
        response(&messages, 2)["result"]["uri"],
        collection_uri,
        "{messages:?}"
    );
    assert_eq!(
        response(&messages, 6)["result"]["uri"],
        collection_uri,
        "separate native document-link proof"
    );
    assert_eq!(
        response(&messages, 7)["result"]
            .as_array()
            .expect("complete membership references")
            .len(),
        1
    );
    assert_eq!(response(&messages, 7)["result"][0]["uri"], page_uri);
    assert!(response(&messages, 3)["result"].is_null());
    assert_eq!(
        response(&messages, 3)["result"],
        response(&messages, 4)["result"],
        "stale foreign-root overlay cannot revive facts"
    );
    assert_eq!(
        response(&messages, 5)["result"],
        response(&messages, 2)["result"],
        "close restores exact retained baseline"
    );
    let diagnostics: Vec<_> = messages
        .iter()
        .filter(|message| {
            message["method"] == "textDocument/publishDiagnostics"
                && message["params"]["uri"] == page_uri
        })
        .map(|message| &message["params"])
        .collect();
    let broken = diagnostics
        .iter()
        .find(|value| {
            value["diagnostics"].as_array().is_some_and(|values| {
                values
                    .iter()
                    .any(|diagnostic| diagnostic["data"]["target"] == "collection:guide")
            })
        })
        .expect("current cross-root dependency failure");
    assert_eq!(broken["version"], 1);
    assert_eq!(
        broken["diagnostics"][0]["data"]["source_sha256"],
        json!(gazette_sdk::source::Sha256Digest::of(page.as_bytes()))
    );
    assert!(messages.iter().any(
        |message| message["method"] == "textDocument/publishDiagnostics"
            && message["params"]["uri"] == file_uri(config.path(), "sites/one.toml")
    ));
}

#[test]
fn editorial_parent_journey_and_member_references_use_exact_foreign_root_uris() {
    let config = tempfile::tempdir().expect("configuration root");
    let containers = tempfile::tempdir().expect("container owner");
    let articles = tempfile::tempdir().expect("article owner");
    std::fs::write(config.path().join("gazette.toml"), WORKSPACE).expect("workspace");
    let root = content("collection:root", "", "kind='collection'");
    let child = content(
        "collection:child",
        "",
        "kind='collection'\nparent='collection:root'",
    );
    let journey = content(
        "journey:one",
        "",
        "kind='journey'\ncollection='collection:child'\nmembers=[{content='content:second',primary=true},{content='content:first',primary=true}]",
    );
    let first = content("content:first", "", "kind='page'\nfamily={kind='article'}");
    let second = content("content:second", "", "kind='page'\nfamily={kind='article'}");
    let direct = content(
        "content:direct",
        "collections=['collection:child']\n",
        "kind='page'\nfamily={kind='article'}",
    );
    for (name, source) in [
        ("root.md", &root),
        ("child.md", &child),
        ("journey.md", &journey),
    ] {
        std::fs::write(containers.path().join(name), source).expect("container source");
    }
    for (name, source) in [
        ("first.md", &first),
        ("second.md", &second),
        ("direct.md", &direct),
    ] {
        std::fs::write(articles.path().join(name), source).expect("article source");
    }
    let input = rooted(
        config.path(),
        &[
            ("containers", containers.path()),
            ("articles", articles.path()),
        ],
        vec![
            locator("containers", "root.md", "owners/root.md"),
            locator("containers", "child.md", "owners/child.md"),
            locator("containers", "journey.md", "owners/journey.md"),
            locator("articles", "first.md", "articles/first.md"),
            locator("articles", "second.md", "articles/second.md"),
            locator("articles", "direct.md", "articles/direct.md"),
        ],
    );
    let root_uri = file_uri(containers.path(), "root.md");
    let child_uri = file_uri(containers.path(), "child.md");
    let journey_uri = file_uri(containers.path(), "journey.md");
    let first_uri = file_uri(articles.path(), "first.md");
    let second_uri = file_uri(articles.path(), "second.md");
    let direct_uri = file_uri(articles.path(), "direct.md");
    let changed = journey.replace("content:first", "content:missing");
    let complete = |id, source: &str, needle: &str| {
        request(
            id,
            "textDocument/completion",
            json!({"textDocument":{"uri":journey_uri},"position":position(source, needle, 0)}),
        )
    };
    let (result, messages) = selected_session(
        input,
        [
            definition(2, &child_uri, position(&child, "collection:root", 0)),
            definition(3, &journey_uri, position(&journey, "collection:child", 0)),
            definition(4, &journey_uri, position(&journey, "content:second", 0)),
            definition(5, &journey_uri, position(&journey, "content:first", 0)),
            footnotes::references(
                6,
                &child_uri,
                position(&child, "collection:child", 0),
                false,
            ),
            footnotes::references(7, &first_uri, position(&first, "content:first", 0), false),
            complete(10, &journey, "content:second"),
            open_content(&journey_uri, 1, &journey),
            complete(11, &journey, "collection:child"),
            change(&journey_uri, 2, &changed),
            definition(8, &journey_uri, position(&journey, "content:second", 0)),
            complete(12, &changed, "content:missing"),
            close(&journey_uri),
            definition(9, &journey_uri, position(&journey, "content:first", 0)),
            complete(13, &journey, "content:first"),
        ],
        Limits::default(),
    );
    result.expect("native rooted editorial session");
    for (id, target) in [
        (2, &root_uri),
        (3, &child_uri),
        (4, &second_uri),
        (5, &first_uri),
        (9, &first_uri),
    ] {
        assert_eq!(
            response(&messages, id)["result"]["uri"],
            *target,
            "{messages:?}"
        );
    }
    let child_references = response(&messages, 6)["result"]
        .as_array()
        .expect("complete child references");
    assert_eq!(child_references.len(), 2);
    for uri in [&journey_uri, &direct_uri] {
        assert!(
            child_references
                .iter()
                .any(|reference| reference["uri"] == *uri)
        );
    }
    assert_eq!(
        response(&messages, 7)["result"]
            .as_array()
            .expect("member references")
            .len(),
        1
    );
    assert_eq!(response(&messages, 7)["result"][0]["uri"], journey_uri);
    assert!(
        response(&messages, 8)["result"].is_null(),
        "invalid current whole graph cannot retain earlier member associations"
    );
    assert_eq!(
        response(&messages, 9)["result"],
        response(&messages, 5)["result"],
        "close restores the exact retained target scalar"
    );
    for id in [10, 12, 13] {
        let items = response(&messages, id)["result"]["items"]
            .as_array()
            .expect("complete native member replacements");
        assert_eq!(items.len(), 3, "{messages:?}");
        assert!(
            items
                .iter()
                .all(|item| item["data"]["field"] == "journey_member")
        );
        assert!(items.iter().any(|item| item["label"] == "content:first"));
    }
    let items = response(&messages, 11)["result"]["items"]
        .as_array()
        .expect("complete native containing collections");
    assert_eq!(items.len(), 2);
    assert!(
        items
            .iter()
            .all(|item| item["data"]["field"] == "journey_collection"
                && item["data"]["version"] == 1)
    );
    assert_eq!(
        response(&messages, 12)["result"]["items"][0]["data"]["preimage"],
        "'content:missing'"
    );
    assert_eq!(
        response(&messages, 12)["result"]["items"][0]["data"]["version"],
        2
    );
    assert_eq!(
        response(&messages, 12)["result"]["items"][0]["data"]["source_sha256"],
        json!(gazette_sdk::source::Sha256Digest::of(changed.as_bytes()))
    );
    assert!(response(&messages, 13)["result"]["items"][0]["data"]["version"].is_null());
}

#[test]
fn rooted_modules_are_complete_exact_and_confined_to_the_configuration_owner() {
    let config = tempfile::tempdir().expect("configuration root");
    let owner = tempfile::tempdir().expect("content root");
    std::fs::write(
        config.path().join("gazette.toml"),
        format!("includes=['site.toml']\n{WORKSPACE}"),
    )
    .expect("root");
    std::fs::write(config.path().join("site.toml"), "").expect("mandatory module");
    let input = rooted(config.path(), &[("owner", owner.path())], vec![]);
    let (result, messages) = selected_session(input.clone(), [], Limits::default());
    result.expect("retain the complete explicit but invalid module selection for repair");
    assert!(messages.iter().any(|message| {
        message["params"]["diagnostics"]
            .as_array()
            .is_some_and(|values| {
                values
                    .iter()
                    .any(|diagnostic| diagnostic["code"] == "workspace_document_set")
            })
    }));
    let mut incorrect = input.clone();
    incorrect["params"]["initializationOptions"]["native_selection"]["workspace_documents"] =
        json!([locator("owner", "site.toml", "site.toml")]);
    failure(incorrect, Limits::default(), "configuration modules");
    let mut duplicate = input;
    duplicate["params"]["initializationOptions"]["native_selection"]["workspace_documents"] =
        json!([
            locator("workspace", "site.toml", "site.toml"),
            locator("workspace", "site.toml", "site.toml")
        ]);
    failure(duplicate, Limits::default(), "duplicate selected native");
}

#[test]
fn rooted_editor_retains_invalid_semantic_and_malformed_baselines_for_native_repair() {
    let config = tempfile::tempdir().expect("configuration");
    let uri = file_uri(config.path(), "gazette.toml");
    for (baseline, code) in [
        (
            WORKSPACE.replace("publisher='owner:one'", "publisher='owner:missing'"),
            "workspace_reference",
        ),
        ("[broken\r\n".to_owned(), "toml_invalid"),
    ] {
        std::fs::write(config.path().join("gazette.toml"), &baseline)
            .expect("invalid editor baseline");
        let input = rooted(config.path(), &[], vec![]);
        let (result, messages) = selected_session(
            input,
            [
                open(&uri, 7, WORKSPACE),
                request(
                    2,
                    "textDocument/documentSymbol",
                    json!({"textDocument":{"uri":uri}}),
                ),
                change(&uri, 8, "[broken\r\n"),
                request(
                    3,
                    "textDocument/documentSymbol",
                    json!({"textDocument":{"uri":uri}}),
                ),
                close(&uri),
            ],
            Limits::default(),
        );
        result.expect("native editor repair session");
        let publications: Vec<_> = messages
            .iter()
            .filter(|message| {
                message["method"] == "textDocument/publishDiagnostics"
                    && message["params"]["uri"] == uri
            })
            .map(|message| &message["params"])
            .collect();
        assert_eq!(publications[0]["diagnostics"][0]["code"], code);
        assert_eq!(
            publications[0]["diagnostics"][0]["data"]["source_sha256"],
            json!(gazette_sdk::source::Sha256Digest::of(baseline.as_bytes()))
        );
        assert_eq!(publications[1]["version"], 7);
        assert_eq!(publications[1]["diagnostics"], json!([]));
        assert_eq!(
            response(&messages, 2)["result"]
                .as_array()
                .expect("native valid symbols")
                .len(),
            2
        );
        assert_ne!(
            response(&messages, 3)["result"],
            response(&messages, 2)["result"],
            "invalid current source cannot reuse earlier symbols"
        );
        assert_eq!(
            publications.last().expect("restored baseline")["diagnostics"][0]["code"],
            code
        );
        assert_eq!(
            std::fs::read(config.path().join("gazette.toml")).expect("original source"),
            baseline.as_bytes()
        );
    }
}

#[test]
fn native_selection_refuses_source_count_and_aggregate_identifier_exhaustion() {
    let config = tempfile::tempdir().expect("configuration");
    let owner = tempfile::tempdir().expect("owner");
    std::fs::write(config.path().join("gazette.toml"), WORKSPACE).expect("root");
    let input = rooted(
        config.path(),
        &[("owner", owner.path())],
        vec![
            locator("owner", "one.md", "content/one.md"),
            locator("owner", "two.md", "content/two.md"),
        ],
    );
    failure(
        input,
        Limits {
            documents: 2,
            ..Limits::default()
        },
        "selected documents ceiling",
    );
    let input = rooted(
        config.path(),
        &[("owner", owner.path())],
        vec![locator("owner", "page.md", "content/page.md")],
    );
    let bytes =
        file_uri(config.path(), "gazette.toml").len() + file_uri(owner.path(), "page.md").len();
    failure(
        input,
        Limits {
            uri_bytes: bytes - 1,
            ..Limits::default()
        },
        "selected identifier bytes",
    );
}

#[test]
fn large_rejected_foreign_root_overlay_keeps_the_next_request_and_retained_baseline_healthy() {
    let config = tempfile::tempdir().expect("configuration");
    let owner = tempfile::tempdir().expect("owner");
    let page = content("content:one", "", "kind='page'\nfamily={kind='article'}");
    std::fs::write(config.path().join("gazette.toml"), WORKSPACE).expect("root");
    std::fs::write(owner.path().join("page.md"), &page).expect("page");
    let uri = file_uri(owner.path(), "page.md");
    let input = rooted(
        config.path(),
        &[("owner", owner.path())],
        vec![locator("owner", "page.md", "content/page.md")],
    );
    let (result, messages) = selected_session(
        input,
        [
            open_content(&uri, 1, &page),
            change(&uri, 2, &format!("{page}{}", "x".repeat(100_000))),
            request(
                2,
                "textDocument/documentSymbol",
                json!({"textDocument":{"uri":uri}}),
            ),
            close(&uri),
            request(
                3,
                "textDocument/documentSymbol",
                json!({"textDocument":{"uri":uri}}),
            ),
        ],
        Limits {
            source_bytes: WORKSPACE.len() + page.len() * 2,
            ..Limits::default()
        },
    );
    result.expect("large refused overlay leaves selected session healthy");
    assert!(messages.iter().any(|message| {
        message["method"] == "window/logMessage"
            && message["params"]["message"]
                .as_str()
                .is_some_and(|message| message.starts_with("lsp_document_change_limit:"))
    }));
    assert_eq!(response(&messages, 2)["result"][0]["name"], "Héllo 😀");
    assert_eq!(
        response(&messages, 2)["result"],
        response(&messages, 3)["result"]
    );
    assert!(!messages.iter().any(
        |message| message["method"] == "textDocument/publishDiagnostics"
            && message["params"]["version"] == 2
    ));
    assert_eq!(
        std::fs::read(owner.path().join("page.md")).expect("unchanged source"),
        page.as_bytes()
    );
}

#[test]
fn rooted_selection_never_grants_unopened_roots_or_reinterprets_physical_and_logical_paths() {
    let config = tempfile::tempdir().expect("config");
    let owner = tempfile::tempdir().expect("owner");
    std::fs::write(config.path().join("gazette.toml"), WORKSPACE).expect("root");
    let input = rooted(
        config.path(),
        &[("owner", owner.path())],
        vec![locator("owner", "page.md", "content/page.md")],
    );
    let mut unopened = input.clone();
    unopened["params"]["workspaceFolders"]
        .as_array_mut()
        .expect("folders")
        .pop();
    failure(unopened, Limits::default(), "explicitly opened");
    let mut unknown = input.clone();
    unknown["params"]["initializationOptions"]["native_selection"]["documents"][0]["root"] =
        json!("undeclared");
    failure(unknown, Limits::default(), "unknown selected native root");
    let mut escape = input.clone();
    escape["params"]["initializationOptions"]["native_selection"]["documents"][0]["entry"] =
        json!("../outside.md");
    failure(
        escape,
        Limits::default(),
        "invalid native-source initialization",
    );
    let mut permission = input;
    permission["params"]["initializationOptions"]["native_selection"]["allow_read"] = json!(false);
    failure(permission, Limits::default(), "allow_read=true");
}

#[test]
fn roots_and_physical_roles_must_be_unambiguous_before_any_capture() {
    let config = tempfile::tempdir().expect("config");
    let owner = tempfile::tempdir().expect("owner");
    std::fs::write(config.path().join("gazette.toml"), WORKSPACE).expect("root");
    let input = rooted(
        config.path(),
        &[("owner", owner.path())],
        vec![locator("owner", "page.md", "content/page.md")],
    );
    let mut repeated = input.clone();
    let alias = json!({"id":"alias","uri":directory_uri(owner.path())});
    repeated["params"]["initializationOptions"]["native_selection"]["roots"]
        .as_array_mut()
        .expect("roots")
        .push(alias);
    failure(
        repeated,
        Limits::default(),
        "duplicate selected native root",
    );
    let mut roles = input;
    roles["params"]["initializationOptions"]["native_selection"]["page_inputs"] =
        json!([locator("owner", "page.md", "other/page.md")]);
    failure(roles, Limits::default(), "duplicate selected native file");
}

#[test]
fn native_capture_rejects_missing_non_utf8_and_symlink_sources_without_partial_admission() {
    use std::os::unix::fs::symlink;
    let config = tempfile::tempdir().expect("config");
    let owner = tempfile::tempdir().expect("owner");
    std::fs::write(config.path().join("gazette.toml"), WORKSPACE).expect("root");
    std::fs::write(
        owner.path().join("good.md"),
        content("content:one", "", "kind='page'\nfamily={kind='article'}"),
    )
    .expect("good");
    let input = rooted(
        config.path(),
        &[("owner", owner.path())],
        vec![
            locator("owner", "good.md", "content/good.md"),
            locator("owner", "bad.md", "content/bad.md"),
        ],
    );
    failure(input.clone(), Limits::default(), "could not be captured");
    std::fs::write(owner.path().join("bad.md"), [0xff_u8]).expect("non UTF8");
    failure(input.clone(), Limits::default(), "not UTF-8");
    let mut linked = input;
    linked["params"]["initializationOptions"]["native_selection"]["documents"][1]["entry"] =
        json!("link.md");
    symlink("good.md", owner.path().join("link.md")).expect("link");
    failure(linked, Limits::default(), "could not be captured");
}

#[test]
fn grouped_native_captures_share_whole_set_bytes_including_workspace_and_modules() {
    let config = tempfile::tempdir().expect("config");
    let first = tempfile::tempdir().expect("first");
    let second = tempfile::tempdir().expect("second");
    std::fs::write(config.path().join("gazette.toml"), WORKSPACE).expect("root");
    let a = content("content:one", "", "kind='page'\nfamily={kind='article'}");
    let b = content("content:two", "", "kind='page'\nfamily={kind='article'}");
    std::fs::write(first.path().join("one.md"), &a).expect("first");
    std::fs::write(second.path().join("two.md"), &b).expect("second");
    let input = rooted(
        config.path(),
        &[("first", first.path()), ("second", second.path())],
        vec![
            locator("first", "one.md", "content/one.md"),
            locator("second", "two.md", "content/two.md"),
        ],
    );
    let maximum = WORKSPACE.len() + a.len() + b.len();
    let exact = Limits {
        source_bytes: maximum,
        ..Limits::default()
    };
    selected_session(input.clone(), [], exact)
        .0
        .expect("exact aggregate ceiling");
    failure(
        input,
        Limits {
            source_bytes: maximum - 1,
            ..Limits::default()
        },
        "could not be captured",
    );
}

#[test]
fn native_authored_file_ceiling_is_not_the_workspace_toml_ceiling_and_assets_stay_opaque() {
    let config = tempfile::tempdir().expect("config");
    let owner = tempfile::tempdir().expect("owner");
    std::fs::write(config.path().join("gazette.toml"),format!("{WORKSPACE}\n[[brands]]\nid='brand:one'\nname='Example'\n[[brands.publications]]\nsite='site:one'\nauthored=[{{source='source:content',target='content'}}]\n[[sources]]\nid='source:content'\nlocation={{kind='filesystem',root='/not-opened/content'}}\n")).expect("root");
    std::fs::create_dir(owner.path().join("guide")).expect("article directory");
    let mut page = content("content:guide", "", "kind='page'\nfamily={kind='article'}");
    page.push_str(&"body ".repeat(15_000));
    assert!(page.len() > Limits::default().syntax.source_bytes);
    std::fs::write(owner.path().join("guide/index.md"), &page).expect("large authored source");
    std::fs::write(owner.path().join("guide/private.bin"), [0_u8, 255, 159]).expect("opaque");
    let mut input = rooted(
        config.path(),
        &[("owner", owner.path())],
        vec![locator("owner", "guide/index.md", "content/guide/index.md")],
    );
    input["params"]["initializationOptions"]["native_selection"]["page_inputs"] = json!([locator(
        "owner",
        "guide/private.bin",
        "content/guide/private.bin"
    )]);
    let (result, messages) = selected_session(
        input,
        [request(
            2,
            "gazette/pagePackage",
            json!({"generation":1,"document":file_uri(owner.path(),"guide/private.bin")}),
        )],
        Limits::default(),
    );
    result.expect("large document and opaque native input");
    assert_eq!(
        response(&messages, 2)["result"]["ownership"]["role"]["kind"],
        "companion"
    );
    assert_eq!(
        response(&messages, 2)["result"]["retained_source"]["uri"],
        file_uri(owner.path(), "guide/index.md")
    );
}

#[test]
fn rooted_manuals_keep_source_qualified_paths_and_exact_editor_uris() {
    let config = tempfile::tempdir().expect("config");
    let first = tempfile::tempdir().expect("first manual owner");
    let second = tempfile::tempdir().expect("second manual owner");
    std::fs::write(config.path().join("gazette.toml"), WORKSPACE).expect("root");
    std::fs::write(first.path().join("start.md"), "# First 🦀\r\n").expect("first");
    std::fs::write(second.path().join("start.md"), "# Second\r\n").expect("second");
    let mut input = rooted(
        config.path(),
        &[("first", first.path()), ("second", second.path())],
        vec![],
    );
    input["params"]["initializationOptions"]["native_selection"]["manuals"] = json!([
        {"source":"source:first","root":"first","entry":"start.md","path":"start.md"},
        {"source":"source:second","root":"second","entry":"start.md","path":"start.md"}
    ]);
    let (result, messages) = selected_session(
        input,
        [
            request(
                2,
                "textDocument/documentSymbol",
                json!({"textDocument":{"uri":file_uri(first.path(),"start.md")}}),
            ),
            request(
                3,
                "textDocument/documentSymbol",
                json!({"textDocument":{"uri":file_uri(second.path(),"start.md")}}),
            ),
        ],
        Limits::default(),
    );
    result.expect("real source-qualified native manuals");
    assert_eq!(response(&messages, 2)["result"][0]["name"], "First 🦀");
    assert_eq!(response(&messages, 3)["result"][0]["name"], "Second");
}

#[test]
fn hundreds_of_native_sources_use_bounded_locators_not_an_oversized_initialize_body() {
    let config = tempfile::tempdir().expect("configuration");
    let first = tempfile::tempdir().expect("first owner");
    let second = tempfile::tempdir().expect("second owner");
    std::fs::write(config.path().join("gazette.toml"), WORKSPACE).expect("root");
    let mut documents = Vec::new();
    for index in 0..200 {
        let (alias, owner) = if index % 2 == 0 {
            ("first", first.path())
        } else {
            ("second", second.path())
        };
        let entry = format!("{index}.md");
        std::fs::write(
            owner.join(&entry),
            content(
                &format!("content:{index}"),
                "",
                "kind='page'\nfamily={kind='article'}",
            ),
        )
        .expect("selected source");
        documents.push(locator(alias, &entry, &format!("content/{index}.md")));
    }
    let input = rooted(
        config.path(),
        &[("first", first.path()), ("second", second.path())],
        documents,
    );
    assert!(
        serde_json::to_vec(&input).expect("locator bytes").len() < Limits::default().message_bytes
    );
    let last = file_uri(second.path(), "199.md");
    let (result, messages) = selected_session(
        input,
        [request(
            2,
            "textDocument/documentSymbol",
            json!({"textDocument":{"uri":last}}),
        )],
        Limits::default(),
    );
    result.expect("complete 200-source native protocol");
    assert_eq!(response(&messages, 2)["result"][0]["name"], "Héllo 😀");
    let selected: std::collections::BTreeSet<_> = messages
        .iter()
        .filter(|message| message["method"] == "textDocument/publishDiagnostics")
        .map(|message| message["params"]["uri"].as_str().expect("source URI"))
        .collect();
    assert_eq!(selected.len(), 201, "no silent successful subset");
}
