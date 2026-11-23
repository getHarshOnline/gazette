//! Deterministic offline retrieval evaluation over the live MCP catalog.
//!
//! This deliberately small lexical baseline is an interface regression fixture,
//! not a claim about the behavior of an MCP host, language model or private tool
//! search extension. It scores query-token matches in canonical operation names
//! (30), titles (10), descriptions (3), capabilities (8), intents (5) and
//! prominence (2), then breaks ties by operation name. Exact contiguous name
//! phrases receive a 20-point bonus. The weights are fixed here so a descriptor
//! change has an observable, reproducible consequence.

#[path = "support/mcp.rs"]
mod mcp;

use std::collections::BTreeSet;

use serde::Deserialize;
use serde_json::{Value, json};

const CAPABILITY: &str = "io.rtepl.gazette/capability";
const INTENT: &str = "io.rtepl.gazette/intent";
const PROMINENCE: &str = "io.rtepl.gazette/prominence";
const CLOSURE: &str = "io.rtepl.gazette/closure";
const RELATIONS: &str = "io.rtepl.gazette/relations";
const TERMINAL_OWNER: &str = "io.rtepl.gazette/terminalResultOwner";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Scenario {
    id: String,
    query: String,
    expected: String,
    confusable: String,
    read_only: bool,
}

fn scenarios() -> Vec<Scenario> {
    serde_json::from_str(include_str!("fixtures/interface_intents.json"))
        .expect("valid interface intent fixture")
}

fn catalog() -> Vec<Value> {
    let response = mcp::exchange(vec![json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/list",
        "params": {}
    })])
    .remove(0);
    assert!(response.get("error").is_none(), "{response}");
    response["result"]["tools"]
        .as_array()
        .expect("live MCP tool catalog")
        .clone()
}

fn tokens(value: &str) -> BTreeSet<String> {
    value
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}

fn field(tool: &Value, key: &str) -> BTreeSet<String> {
    tokens(tool[key].as_str().unwrap_or_default())
}

fn metadata_field(tool: &Value, key: &str) -> BTreeSet<String> {
    tokens(tool["_meta"][key].as_str().unwrap_or_default())
}

fn normalized(value: &str) -> String {
    tokens(value).into_iter().collect::<Vec<_>>().join(" ")
}

fn score(query: &str, tool: &Value) -> u64 {
    let query_tokens = tokens(query);
    let weighted = [
        (field(tool, "name"), 30),
        (field(tool, "title"), 10),
        (field(tool, "description"), 3),
        (metadata_field(tool, CAPABILITY), 8),
        (metadata_field(tool, INTENT), 5),
        (metadata_field(tool, PROMINENCE), 2),
    ];
    let mut score = weighted
        .iter()
        .map(|(field, weight)| query_tokens.intersection(field).count() as u64 * weight)
        .sum();
    let query_words = query
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>()
        .join(" ");
    let name_words = tool["name"]
        .as_str()
        .map(|name| name.replace('_', " "))
        .unwrap_or_default();
    if query_words.contains(&name_words) {
        score += 20;
    }
    score
}

fn ranked<'a>(query: &str, tools: &'a [Value]) -> Vec<(u64, &'a Value)> {
    let mut ranked = tools
        .iter()
        .map(|tool| (score(query, tool), tool))
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| left.1["name"].as_str().cmp(&right.1["name"].as_str()))
    });
    ranked
}

fn named<'a>(tools: &'a [Value], name: &str) -> &'a Value {
    tools
        .iter()
        .find(|tool| tool["name"] == name)
        .unwrap_or_else(|| panic!("live catalog lacks {name}"))
}

fn is_read_only(tool: &Value) -> bool {
    tool["annotations"]["readOnlyHint"] == true
}

fn schema_bytes(tool: &Value) -> usize {
    ["inputSchema", "outputSchema"]
        .iter()
        .filter_map(|key| tool.get(*key))
        .map(|schema| {
            serde_json::to_vec(schema)
                .expect("serialize tool schema")
                .len()
        })
        .sum()
}

fn assert_action_closure(tool: &Value, tools: &[Value]) {
    let name = tool["name"].as_str().expect("operation name");
    match tool["_meta"][CLOSURE].as_str().expect("canonical closure") {
        "related" => {
            let relations = tool["_meta"][RELATIONS]
                .as_array()
                .filter(|relations| !relations.is_empty())
                .unwrap_or_else(|| panic!("{name} has no usable related-operation closure"));
            for relation in relations {
                let operation = relation["operation"]
                    .as_str()
                    .expect("related operation name");
                named(tools, operation);
            }
        }
        "terminal-owner" => {
            let owner = tool["_meta"][TERMINAL_OWNER]
                .as_str()
                .filter(|owner| !owner.is_empty())
                .unwrap_or_else(|| panic!("{name} has no usable terminal result owner"));
            named(tools, owner);
        }
        "self-terminal" => assert!(
            tool["_meta"][TERMINAL_OWNER].is_null()
                && tool["_meta"][RELATIONS]
                    .as_array()
                    .is_none_or(Vec::is_empty),
            "{name} contradicts its self-terminal closure"
        ),
        closure => panic!("effectful selection {name} has unusable closure {closure}"),
    }
}

#[test]
fn bounded_intents_retrieve_safe_canonical_operations_from_the_live_catalog() {
    let tools = catalog();
    let complete_schema_bytes = tools.iter().map(schema_bytes).sum::<usize>();
    assert!(complete_schema_bytes > 0);

    for scenario in scenarios() {
        let ranked = ranked(&scenario.query, &tools);
        let top = ranked
            .first()
            .map(|(_, tool)| *tool)
            .expect("nonempty catalog");
        let top_three = ranked
            .iter()
            .take(3)
            .map(|(_, tool)| *tool)
            .collect::<Vec<_>>();
        let top_three_names = top_three
            .iter()
            .map(|tool| tool["name"].as_str().expect("operation name"))
            .collect::<Vec<_>>();
        assert!(
            top_three_names.contains(&scenario.expected.as_str()),
            "{}: expected {} in top three for {:?}, observed {top_three_names:?}",
            scenario.id,
            scenario.expected,
            scenario.query
        );

        let expected = named(&tools, &scenario.expected);
        let confusable = named(&tools, &scenario.confusable);
        assert!(
            score(&scenario.query, expected) > score(&scenario.query, confusable),
            "{}: {:?} does not separate {} from {}",
            scenario.id,
            scenario.query,
            scenario.expected,
            scenario.confusable
        );

        if scenario.read_only {
            assert!(
                is_read_only(top),
                "{}: read-only intent ranked effectful {} first",
                scenario.id,
                top["name"]
            );
        }
        if !is_read_only(expected)
            && matches!(
                expected["_meta"][INTENT].as_str(),
                Some("act" | "reconcile" | "recover")
            )
        {
            assert_action_closure(expected, &tools);
        }

        let selected_schema_bytes = top_three
            .iter()
            .map(|tool| schema_bytes(tool))
            .sum::<usize>();
        assert!(
            selected_schema_bytes.saturating_mul(4) < complete_schema_bytes,
            "{}: bounded top-three selection is not materially smaller: {selected_schema_bytes} of {complete_schema_bytes} schema bytes",
            scenario.id
        );
    }
}

#[test]
fn retrieval_fixture_is_unique_and_covers_the_required_interface_neighborhoods() {
    let scenarios = scenarios();
    let ids = scenarios
        .iter()
        .map(|scenario| scenario.id.as_str())
        .collect::<BTreeSet<_>>();
    let expected = scenarios
        .iter()
        .map(|scenario| scenario.expected.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(ids.len(), scenarios.len(), "scenario IDs must be unique");
    assert_eq!(
        expected.len(),
        scenarios.len(),
        "each fixture must test a distinct canonical operation"
    );
    assert_eq!(
        expected,
        BTreeSet::from([
            "authoring_page_package",
            "backup_store",
            "check_documentation",
            "check_workspace",
            "compile_templates",
            "inspect_composition",
            "inspect_markdown",
            "inspect_publication",
            "inspect_store",
            "observe_github_pages_build",
            "operate_recovery_session",
            "reconcile_registry",
            "render_sites",
            "review_publication",
            "start_preview",
            "verify_backup",
            "verify_prepared_authorization",
        ])
    );
    assert!(
        scenarios
            .iter()
            .all(|scenario| normalized(&scenario.query) != normalized(&scenario.expected)),
        "queries must remain user intents rather than copied operation identifiers"
    );
}
