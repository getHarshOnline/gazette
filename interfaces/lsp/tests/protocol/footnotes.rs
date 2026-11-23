//! Selected-source footnote associations projected through actual framed requests.

use super::*;

const PAGE: &str = "file:///selected/one.md";
const OTHER: &str = "file:///selected/two.md";

fn page(id: &str) -> String {
    format!(
        "{}\n😀 See [^same] then [^same].\n\n[^same]: Native definition 😀.\n",
        content(id, "", "kind='page'\nfamily={kind='article'}")
    )
    .replace('\n', "\r\n")
}

// Fixture coordinates are explicit whole source slices, not a Markdown parser.
pub(super) fn position(source: &str, needle: &str, occurrence: usize) -> Value {
    let offset = source
        .match_indices(needle)
        .nth(occurrence)
        .expect("fixture occurrence")
        .0;
    let prefix = &source[..offset];
    json!({"line":prefix.matches('\n').count(),
        "character":prefix.rsplit('\n').next().expect("fixture line prefix").encode_utf16().count()})
}

pub(super) fn location(uri: &str, source: &str, needle: &str, occurrence: usize) -> Value {
    let start = position(source, needle, occurrence);
    let mut end = start.clone();
    end["character"] = json!(
        start["character"].as_u64().expect("numeric fixture column")
            + needle.encode_utf16().count() as u64
    );
    json!({"uri":uri,"range":{"start":start,"end":end}})
}

pub(super) fn definition(id: i32, uri: &str, position: Value) -> Value {
    request(
        id,
        "textDocument/definition",
        json!({"textDocument":{"uri":uri},"position":position}),
    )
}

pub(super) fn references(id: i32, uri: &str, position: Value, include: bool) -> Value {
    request(
        id,
        "textDocument/references",
        json!({"textDocument":{"uri":uri},"position":position,
        "context":{"includeDeclaration":include}}),
    )
}

pub(super) fn response(messages: &[Value], id: i32) -> &Value {
    messages
        .iter()
        .find(|message| message["id"] == id)
        .expect("complete response")
}

#[test]
fn footnote_locations_preserve_full_source_utf16_and_never_join_equal_labels_across_files() {
    let source = page("content:one");
    let other = page("content:two").replace("Native definition", "Other definition");
    let definition_position = position(&source, "[^same]:", 0);
    let first = position(&source, "[^same]", 0);
    assert_eq!(
        first["character"], 7,
        "the non-BMP prefix occupies two UTF-16 units"
    );
    let (result, messages) = selected_session(
        selected(&[(PAGE, "one.md", &source), (OTHER, "two.md", &other)]),
        [
            definition(2, PAGE, first.clone()),
            definition(3, OTHER, position(&other, "[^same]", 0)),
            references(4, PAGE, definition_position.clone(), false),
            references(5, PAGE, definition_position, true),
            references(6, PAGE, first, false),
            definition(7, PAGE, position(&source, "Héllo", 0)),
            references(8, PAGE, position(&source, "Héllo", 0), true),
        ],
        Limits::default(),
    );
    result.expect("complete native footnote session");
    let declared = location(PAGE, &source, "[^same]: Native definition 😀.", 0);
    assert_eq!(response(&messages, 2)["result"], declared);
    assert_eq!(
        response(&messages, 3)["result"],
        location(OTHER, &other, "[^same]: Other definition 😀.", 0)
    );
    let expected = json!([
        location(PAGE, &source, "[^same]", 0),
        location(PAGE, &source, "[^same]", 1)
    ]);
    assert_eq!(response(&messages, 4)["result"], expected);
    assert_eq!(response(&messages, 6)["result"], expected);
    let with_declaration = response(&messages, 5)["result"]
        .as_array()
        .expect("location list");
    assert_eq!(with_declaration.len(), 3);
    assert!(with_declaration.contains(&declared));
    for reference in expected.as_array().expect("expected reference array") {
        assert!(with_declaration.contains(reference));
    }
    let heading = location(PAGE, &source, "# Héllo 😀", 0);
    assert_eq!(response(&messages, 7)["result"], heading);
    assert_eq!(response(&messages, 8)["result"], json!([heading]));
}

#[test]
fn unsaved_changes_replace_navigation_and_invalid_selected_corpus_cannot_reuse_it() {
    let source = page("content:one");
    let other = page("content:two");
    let changed = source
        .replace("😀 See", "😀 Updated see")
        .replace("Native definition", "Changed definition");
    let first = position(&source, "[^same]", 0);
    let new_first = position(&changed, "[^same]", 0);
    let (result, messages) = selected_session(
        selected(&[(PAGE, "one.md", &source), (OTHER, "two.md", &other)]),
        [
            definition(2, PAGE, first.clone()),
            open_content(PAGE, 1, &source),
            change(PAGE, 2, &changed),
            definition(3, PAGE, new_first.clone()),
            definition(4, PAGE, first.clone()),
            open_content(OTHER, 1, "+++\n[broken\n+++\nNo valid corpus"),
            definition(5, PAGE, new_first.clone()),
            references(6, PAGE, new_first, true),
            close(OTHER),
            close(PAGE),
            definition(7, PAGE, first),
        ],
        Limits::default(),
    );
    result.expect("overlay and baseline lifecycle");
    assert_eq!(
        response(&messages, 3)["result"],
        location(PAGE, &changed, "[^same]: Changed definition 😀.", 0)
    );
    assert!(
        response(&messages, 4)["result"].is_null(),
        "old cursor now points to ordinary text"
    );
    for id in [5, 6] {
        assert!(
            response(&messages, id).get("error").is_some(),
            "current invalid corpus cannot return stale navigation"
        );
        assert!(response(&messages, id).get("result").is_none());
    }
    assert_eq!(
        response(&messages, 7)["result"],
        response(&messages, 2)["result"]
    );
}

#[test]
fn navigation_rejects_unselected_and_invalid_coordinates_including_half_a_surrogate() {
    let source = page("content:one");
    let mut surrogate = position(&source, "😀 See", 0);
    surrogate["character"] = json!(1);
    let mut beyond_line = position(&source, "😀 See", 0);
    beyond_line["character"] = json!(999);
    let invalid = [surrogate, beyond_line, json!({"line":999,"character":0})];
    let mut requests = Vec::new();
    for (index, point) in invalid.into_iter().enumerate() {
        requests.push(definition(10 + index as i32, PAGE, point.clone()));
        requests.push(references(20 + index as i32, PAGE, point, true));
    }
    requests.push(definition(
        30,
        "file:///not-selected.md",
        json!({"line":0,"character":0}),
    ));
    requests.push(references(
        31,
        "file:///not-selected.md",
        json!({"line":0,"character":0}),
        false,
    ));
    let (result, messages) = selected_session(
        selected(&[(PAGE, "one.md", &source)]),
        requests,
        Limits::default(),
    );
    result.expect("invalid requests do not corrupt the session");
    for id in [10, 11, 12, 20, 21, 22, 30, 31] {
        assert_eq!(response(&messages, id)["error"]["code"], -32602);
        assert!(response(&messages, id).get("result").is_none());
    }
}

#[test]
fn malformed_navigation_params_refuse_without_changing_the_current_analysis() {
    let source = page("content:one");
    let point = position(&source, "[^same]", 0);
    let mut requests = vec![
        request(
            2,
            "textDocument/definition",
            json!({"textDocument":{"uri":PAGE}}),
        ),
        definition(3, PAGE, json!({"line":-1,"character":0})),
        definition(4, PAGE, json!({"line":0,"character":0.5})),
        request(
            5,
            "textDocument/references",
            json!({"textDocument":{"uri":PAGE},"position":point}),
        ),
        request(
            6,
            "textDocument/references",
            json!({"textDocument":{"uri":PAGE},"position":point,"context":{"includeDeclaration":"yes"}}),
        ),
    ];
    requests.extend([
        request(
            8,
            "textDocument/definition",
            json!({"textDocument":[PAGE],"position":point}),
        ),
        request(
            9,
            "textDocument/definition",
            json!({"textDocument":{"uri":PAGE},"position":[0,0]}),
        ),
        request(
            10,
            "textDocument/references",
            json!({"textDocument":{"uri":PAGE},"position":point,"context":[true]}),
        ),
    ]);
    requests.push(definition(7, PAGE, point));
    let (result, messages) = selected_session(
        selected(&[(PAGE, "one.md", &source)]),
        requests,
        Limits::default(),
    );
    result.expect("malformed methods leave selected source intact");
    for id in [2, 3, 4, 5, 6, 8, 9, 10] {
        assert_eq!(response(&messages, id)["error"]["code"], -32602);
    }
    assert_eq!(
        response(&messages, 7)["result"],
        location(PAGE, &source, "[^same]: Native definition 😀.", 0)
    );
}

#[test]
fn navigation_projection_limits_apply_to_the_complete_selected_source_facts() {
    let source = page("content:one");
    let call = definition(2, PAGE, position(&source, "[^same]", 0));
    let mut exact = Limits::default();
    // Every namespace shares the complete selected-source budget. Three footnote
    // associations retain the four-byte label "same"; authored site/principal,
    // editorial and related-content facts retain their native owned records too.
    let workspace = gazette_sdk::workspace::ParsedWorkspace::parse(WORKSPACE, Default::default())
        .expect("native selected workspace index");
    let compilation = gazette_sdk::workspace::compile(WORKSPACE, Default::default())
        .expect("native selected workspace compilation");
    let corpus = gazette_sdk::corpus::parse(
        [gazette_sdk::corpus::Input {
            path: "one.md".parse().expect("selected logical source path"),
            bytes: source.as_bytes(),
        }],
        Default::default(),
    )
    .expect("native selected corpus");
    let editorial = gazette_sdk::authored::editorial_authoring(
        &compilation,
        &corpus,
        exact.authoring.navigation_occurrences,
        exact.authoring.navigation_bytes,
    )
    .expect("complete native editorial source facts");
    assert_eq!(editorial.declarations().len(), 1);
    assert_eq!(editorial.occurrence_count(), 1);
    let workspace_path = "workspace.toml".parse().expect("selected workspace path");
    let mut workspace_targets = Vec::new();
    for symbol in workspace.navigation().symbols() {
        if let Some(target) = gazette_sdk::authored::AuthoredReferenceDeclaration::workspace(
            symbol,
            &workspace_path,
            WORKSPACE,
        )
        .expect("native workspace candidate")
        {
            workspace_targets.push(target);
        }
    }
    assert_eq!(workspace_targets.len(), 2);
    for field in [
        gazette_sdk::authored::AuthoredReferenceField::Site,
        gazette_sdk::authored::AuthoredReferenceField::AttributionPrincipal,
    ] {
        assert_eq!(
            workspace_targets
                .iter()
                .filter(|target| target.field == field)
                .count(),
            1,
            "one native candidate in each workspace reference namespace"
        );
    }
    let resolved = gazette_sdk::authored::resolve(&compilation, &corpus, exact.authoring.content)
        .expect("complete native selected graph");
    let related_targets = resolved
        .authored_reference_targets(
            exact.authoring.navigation_occurrences,
            exact.authoring.navigation_bytes,
        )
        .expect("complete native related-content candidates");
    assert_eq!(related_targets.len(), 1);
    assert_eq!(related_targets[0].identity.as_str(), "content:one");
    assert_eq!(
        related_targets[0].field,
        gazette_sdk::authored::AuthoredReferenceField::RelatedContent
    );
    assert!(
        resolved
            .symbolic_completion_targets(
                exact.authoring.navigation_occurrences,
                exact.authoring.navigation_bytes,
            )
            .expect("native symbolic candidates")
            .is_empty(),
        "unrouted fixture has no symbolic publication candidates"
    );
    exact.authoring.navigation_occurrences = workspace.navigation().occurrence_count()
        + workspace_targets.len()
        + 3
        + editorial.occurrence_count()
        + related_targets.len();
    exact.authoring.navigation_bytes = workspace.navigation().key_bytes()
        + workspace_targets
            .iter()
            .chain(&related_targets)
            .map(gazette_sdk::authored::AuthoredReferenceDeclaration::fact_bytes)
            .sum::<usize>()
        + 12
        + editorial.fact_bytes();
    let (result, messages) = selected_session(
        selected(&[(PAGE, "one.md", &source)]),
        [call.clone()],
        exact,
    );
    result.expect("complete exact native projection bounds");
    assert_eq!(
        response(&messages, 2)["result"],
        location(PAGE, &source, "[^same]: Native definition 😀.", 0)
    );
    for count_bound in [true, false] {
        let mut insufficient = exact;
        if count_bound {
            insufficient.authoring.navigation_occurrences -= 1;
        } else {
            insufficient.authoring.navigation_bytes -= 1;
        }
        let (result, messages) = selected_session(
            selected(&[(PAGE, "one.md", &source)]),
            [call.clone()],
            insufficient,
        );
        result.expect("analysis limit is reported without corrupting protocol lifecycle");
        assert!(messages.iter().any(|message| {
            message["method"] == "textDocument/publishDiagnostics"
                && message["params"]["diagnostics"]
                    .as_array()
                    .expect("published diagnostic array")
                    .iter()
                    .any(|diagnostic| diagnostic["code"] == "authoring_limit")
        }));
        assert_eq!(response(&messages, 2)["error"]["code"], -32600);
        assert!(response(&messages, 2).get("result").is_none());
    }
}

#[test]
fn references_response_budget_is_complete_not_a_truncated_successful_list() {
    let source = format!(
        "{}\n{}\n\n[^same]: Native.\n",
        content("content:one", "", "kind='page'\nfamily={kind='article'}"),
        "[^same] ".repeat(80)
    );
    let call = references(2, PAGE, position(&source, "[^same]:", 0), false);
    let initial = selected(&[(PAGE, "one.md", &source)]);
    let (result, messages) = selected_session(initial.clone(), [call.clone()], Limits::default());
    result.expect("complete reference baseline");
    assert_eq!(
        response(&messages, 2)["result"]
            .as_array()
            .expect("complete references array")
            .len(),
        80
    );
    let exact = serde_json::to_vec(response(&messages, 2))
        .expect("exact response encoding")
        .len();
    let (result, exact_messages) = selected_session(
        initial.clone(),
        [call.clone()],
        Limits {
            output_bytes: exact,
            ..Limits::default()
        },
    );
    result.expect("exact response budget admits all locations");
    assert_eq!(response(&exact_messages, 2), response(&messages, 2));
    let (result, bounded) = selected_session(
        initial,
        [call],
        Limits {
            output_bytes: exact - 1,
            ..Limits::default()
        },
    );
    assert!(result.is_err() || response(&bounded, 2).get("error").is_some());
    assert!(
        !bounded
            .iter()
            .any(|message| message["id"] == 2 && message.get("result").is_some()),
        "no partial locations are returned as success"
    );
}
