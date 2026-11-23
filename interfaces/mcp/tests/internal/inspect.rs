//! Direct tool execution reaches the real SDK; protocol framing is tested by the catalog owner.

use gazette_cli::sdk::{document, markdown, toml};
use serde_json::{Value, json};

use super::*;

#[test]
fn every_markdown_numeric_limit_advertises_its_exact_native_range() {
    let schema = serde_json::to_value(descriptor()).expect("descriptor")["inputSchema"].clone();
    let validator = jsonschema::validator_for(&schema).expect("discovery schema");
    let beyond: Value = serde_json::from_str(&((usize::MAX as u128) + 1).to_string())
        .expect("out-of-range JSON integer");
    for field in [
        "source_bytes",
        "nodes",
        "depth",
        "fact_bytes",
        "output_bytes",
        "metadata_bytes",
        "metadata_nodes",
        "metadata_depth",
    ] {
        for (value, accepted) in [(json!(usize::MAX), true), (beyond.clone(), false)] {
            let input = json!({"source":"# A", "limits":{field:value}});
            assert_eq!(validator.is_valid(&input), accepted, "{input}");
            assert_eq!(
                decode(input.as_object().expect("arguments").clone()).is_ok(),
                accepted
            );
        }
    }
}

#[test]
fn limit_object_cannot_be_replaced_by_a_positional_serde_sequence() {
    let arguments = json!({"source":"# A","limits":[100,100,10,100,100,100,100,10]});
    assert!(decode(arguments.as_object().expect("object").clone()).is_err());
}

fn execute(arguments: Value, maximum: usize) -> CallToolResult {
    let result = decode(
        arguments
            .as_object()
            .expect("test arguments are an object")
            .clone(),
    )
    .expect("typed Markdown arguments")
    .execute(maximum)
    .expect("tool result, not a protocol error");
    let descriptor = serde_json::to_value(descriptor()).expect("descriptor");
    jsonschema::validator_for(&descriptor["outputSchema"])
        .expect("native result schema")
        .validate(
            result
                .structured_content
                .as_ref()
                .expect("structured result"),
        )
        .expect("native emitted result conforms to discovery");
    result
}

fn result(result: &CallToolResult) -> &Value {
    assert_eq!(result.is_error, Some(false), "{result:?}");
    result
        .structured_content
        .as_ref()
        .expect("structured native report")
}

fn failure_code(result: &CallToolResult) -> &str {
    assert_eq!(result.is_error, Some(true), "{result:?}");
    let value = result
        .structured_content
        .as_ref()
        .expect("structured failure");
    assert!(value.get("inspection").is_none());
    assert!(value.get("document").is_none());
    assert!(value.get("html").is_none());
    value["code"].as_str().expect("stable native failure class")
}

fn document_limits(limits: &Limits) -> document::Limits {
    document::Limits {
        source_bytes: limits.source_bytes,
        markdown: limits.markdown(),
        metadata: toml::Limits {
            source_bytes: limits.metadata_bytes,
            nodes: limits.metadata_nodes,
            depth: limits.metadata_depth,
        },
    }
}

#[test]
fn supplied_markdown_and_html_match_the_sdk_without_normalizing_source() {
    let source = "# Héllo 😀\r\n\r\n- [x] done\r\n- [ ] pending\r\n\r\n[link](https://example.test) and **bold**.\r\n";
    let parsed = markdown::parse(source, markdown::Limits::default()).expect("real Markdown parse");
    let actual = execute(json!({"source":source,"html":true}), 1 << 20);
    assert_eq!(
        result(&actual),
        &json!({
            "inspection":parsed.inspect().expect("real inspection"),
            "html":parsed.render_html().expect("real safe HTML"),
        })
    );
    assert!(
        result(&actual).get("input").is_none(),
        "supplied text is not pinned native acquisition"
    );
}

#[test]
fn explicit_toml_framing_matches_native_metadata_body_coordinates_and_html() {
    let source = "+++\r\ntitle = 'Héllo'\r\n+++\r\n# Body 😀\r\n";
    let request = decode(
        json!({"source":source,"front_matter":"toml","html":true})
            .as_object()
            .expect("arguments")
            .clone(),
    )
    .expect("TOML mode");
    let parsed =
        document::parse(source, document_limits(&request.limits)).expect("real framed source");
    let expected = json!({
        "document":parsed.inspect().expect("native framed inspection"),
        "html":parsed.render_html().expect("native framed HTML"),
    });
    assert_eq!(
        result(&request.execute(1 << 20).expect("tool result")),
        &expected
    );
}

#[test]
fn inspection_does_not_silently_enable_html_or_bypass_native_html_gates() {
    for source in ["<script>secret()</script>\n", "[bad](javascript:secret)\n"] {
        let inspected = execute(json!({"source":source}), 1 << 20);
        assert!(result(&inspected).get("html").is_none());
        let parsed =
            markdown::parse(source, markdown::Limits::default()).expect("source inspection");
        let expected = parsed.render_html().expect_err("native HTML refusal");
        let rejected = execute(json!({"source":source,"html":true}), 1 << 20);
        assert_eq!(failure_code(&rejected), expected.code());
        assert!(
            !format!("{rejected:?}").contains("secret"),
            "failure must not echo private source"
        );
    }
}

#[test]
fn every_markdown_limit_reaches_the_native_owner() {
    let source = "# A heading\n\nA paragraph.\n";
    for field in [
        "source_bytes",
        "nodes",
        "depth",
        "fact_bytes",
        "output_bytes",
    ] {
        let arguments = json!({"source":source,"limits":{field:0}});
        let request = decode(arguments.as_object().expect("arguments").clone())
            .expect("zero is an exact ceiling");
        let expected = markdown::parse(source, request.limits.markdown())
            .and_then(|parsed| parsed.inspect())
            .expect_err("zero ceiling rejects nonempty native inspection");
        let actual = request.execute(1 << 20).expect("bounded tool error");
        assert_eq!(failure_code(&actual), expected.code(), "{field}");
    }
}

#[test]
fn complete_document_and_each_metadata_limit_retain_native_failure_classes() {
    let source = "+++\ntitle='hello'\n+++\n# Body\n";
    for field in [
        "source_bytes",
        "metadata_bytes",
        "metadata_nodes",
        "metadata_depth",
    ] {
        let request = decode(
            json!({"source":source,"front_matter":"toml","limits":{field:0}})
                .as_object()
                .expect("arguments")
                .clone(),
        )
        .expect("typed limits");
        let expected = document::parse(source, document_limits(&request.limits))
            .and_then(|parsed| parsed.inspect())
            .expect_err("zero complete/metadata ceiling rejects framed source");
        let actual = request.execute(1 << 20).expect("tool error");
        assert_eq!(failure_code(&actual), expected.code(), "{field}");
    }
    for source in ["# missing framing", "+++\nprivate='unterminated\n+++\nbody"] {
        let actual = execute(json!({"source":source,"front_matter":"toml"}), 1 << 20);
        assert!(actual.is_error.expect("tool failure disposition"));
        assert!(!format!("{actual:?}").contains("unterminated"));
    }
}

#[test]
fn native_source_ceiling_is_exact_and_wire_output_limit_never_returns_partial_success() {
    let source = "# Héllo\n";
    assert!(
        result(&execute(
            json!({"source":source,"limits":{"source_bytes":source.len()}}),
            1 << 20
        ))
        .get("inspection")
        .is_some()
    );
    let rejected = execute(
        json!({"source":source,"limits":{"source_bytes":source.len()-1}}),
        1 << 20,
    );
    assert!(rejected.is_error.expect("source failure"));
    let overflow = execute(json!({"source":source,"html":true}), 1);
    assert_eq!(failure_code(&overflow), "mcp_output_limit");
}

#[test]
fn discovery_schema_is_typed_closed_and_defaults_follow_the_native_limits() {
    let descriptor = serde_json::to_value(descriptor()).expect("serializable tool descriptor");
    assert_eq!(descriptor["name"], "inspect_markdown");
    assert_eq!(descriptor["inputSchema"]["additionalProperties"], false);
    assert_eq!(
        descriptor["inputSchema"]["properties"]["source"]["type"],
        "string"
    );
    assert_eq!(descriptor["annotations"]["readOnlyHint"], true);
    assert_eq!(descriptor["annotations"]["idempotentHint"], true);
    assert_eq!(descriptor["annotations"]["destructiveHint"], false);
    assert_eq!(descriptor["annotations"]["openWorldHint"], true);
    jsonschema::validator_for(&descriptor["outputSchema"])
        .expect("native serialized result schema");
    let definitions = &descriptor["inputSchema"]["$defs"];
    assert_eq!(definitions["FrontMatter"]["enum"], json!(["toml"]));
    for field in [
        "source_bytes",
        "nodes",
        "depth",
        "fact_bytes",
        "output_bytes",
        "metadata_bytes",
        "metadata_nodes",
        "metadata_depth",
    ] {
        assert_eq!(
            definitions["MarkdownInspectionLimits"]["properties"][field]["type"], "integer",
            "{field}"
        );
    }
    let defaults = serde_json::to_value(Limits::default()).expect("limit defaults");
    assert_eq!(
        defaults["source_bytes"],
        markdown::Limits::default().source_bytes
    );
    assert_eq!(
        defaults["metadata_bytes"],
        gazette_cli::sdk::toml::Limits::default().source_bytes
    );
    for arguments in [
        json!({}),
        json!({"source":1}),
        json!({"source":"","tree":"/private/secret"}),
        json!({"source":"","front_matter":"yaml"}),
        json!({"source":"","limits":{"unknown":1}}),
        json!({"source":"","limits":{"nodes":-1}}),
    ] {
        assert!(
            decode(
                arguments
                    .as_object()
                    .expect("malformed object fixture")
                    .clone()
            )
            .is_err()
        );
    }
}

#[test]
fn output_schema_preserves_nullable_heading_parent_and_rejects_wrong_native_shapes() {
    let response = execute(
        json!({"source":"# First\n\n## Child\n\n[link](#first)\n", "html":true}),
        1 << 20,
    );
    let value = result(&response);
    assert_eq!(value["inspection"]["headings"][0]["parent"], Value::Null);
    assert_eq!(value["inspection"]["headings"][1]["parent"], 0);
    let descriptor = serde_json::to_value(descriptor()).expect("descriptor");
    let schema = jsonschema::validator_for(&descriptor["outputSchema"]).expect("output schema");
    for (pointer, wrong) in [
        ("/inspection/source_sha256", json!("invalid")),
        ("/inspection/headings/0/level", json!(7)),
        ("/inspection/headings/0/parent", json!("0")),
        ("/inspection/headings/0/span/start", json!(-1)),
        ("/html", json!({"html":"not a string"})),
    ] {
        let mut changed = value.clone();
        *changed.pointer_mut(pointer).expect("real native field") = wrong;
        assert!(!schema.is_valid(&changed), "{pointer}");
    }
    let mut missing = value.clone();
    missing["inspection"]["headings"][0]
        .as_object_mut()
        .expect("heading")
        .remove("parent");
    assert!(
        !schema.is_valid(&missing),
        "emitted null fields remain required"
    );
    let document = execute(
        json!({"source":"+++\ntitle='Page'\n+++\n# Page\n", "front_matter":"toml"}),
        1 << 20,
    );
    assert!(result(&document).get("document").is_some());
    assert!(
        result(&document).get("html").is_none(),
        "omitted HTML stays optional"
    );
}

fn tree_arguments(root: &std::path::Path, document: &str) -> Value {
    let identity = gazette_cli::sdk::source::inspect(root, None, Default::default())
        .expect("independently retained complete inventory")
        .identity();
    json!({"tree":{"root":root,"identity":identity,"document":document},"allow_native_reads":true})
}

#[test]
fn pinned_tree_markdown_and_frontmatter_match_the_shared_native_capture() {
    let root = tempfile::tempdir().expect("native tree");
    for (text, framed) in [
        ("# Captured é\n", false),
        ("+++\ntitle='Page'\n+++\n# Captured é\n", true),
    ] {
        std::fs::write(root.path().join("page.md"), text).expect("document");
        std::fs::write(root.path().join(".asset"), "included").expect("hidden asset");
        let mut arguments = tree_arguments(root.path(), "page.md");
        arguments["html"] = json!(true);
        if framed {
            arguments["front_matter"] = json!("toml");
        }
        let descriptor = serde_json::to_value(descriptor()).expect("descriptor");
        assert!(
            jsonschema::validator_for(&descriptor["inputSchema"])
                .expect("input schema")
                .is_valid(&arguments)
        );
        let tree: markdown::input::Tree =
            serde_json::from_value(arguments["tree"].clone()).expect("native selector");
        let loaded = tree.load(true).expect("native capture");
        let expected = if framed {
            serde_json::to_value(markdown::input::Report {
                input: loaded.evidence(),
                projection: document::inspect(
                    loaded.text().expect("text"),
                    Default::default(),
                    true,
                )
                .expect("document"),
            })
            .expect("native report")
        } else {
            serde_json::to_value(markdown::input::Report {
                input: loaded.evidence(),
                projection: markdown::inspect(
                    loaded.text().expect("text"),
                    Default::default(),
                    true,
                )
                .expect("Markdown"),
            })
            .expect("native report")
        };
        assert_eq!(result(&execute(arguments, 1 << 20)), &expected);
    }
}

#[test]
fn pinned_selection_requires_permission_exclusivity_named_shapes_and_confined_entry() {
    let root = tempfile::tempdir().expect("native tree");
    std::fs::write(root.path().join("page.md"), "# Page").expect("document");
    let valid = tree_arguments(root.path(), "page.md");
    let descriptor = serde_json::to_value(descriptor()).expect("descriptor");
    let schema = jsonschema::validator_for(&descriptor["inputSchema"]).expect("schema");
    for (pointer, replacement) in [
        ("/allow_native_reads", json!(false)),
        ("/tree", json!(null)),
        ("/tree/identity", json!([])),
        ("/tree/document", json!("../page.md")),
        ("/tree/limits", json!([])),
    ] {
        let mut invalid = valid.clone();
        if pointer == "/tree/limits" {
            invalid["tree"]["limits"] = replacement;
        } else {
            *invalid.pointer_mut(pointer).expect("field") = replacement;
        }
        assert!(
            decode(invalid.as_object().expect("object").clone()).is_err(),
            "{invalid}"
        );
        // Literal EntryPath semantic confinement is also enforced by its native decoder.
        if pointer != "/tree/document" {
            assert!(!schema.is_valid(&invalid), "{invalid}");
        }
    }
    let mut ambiguous = valid.clone();
    ambiguous["source"] = json!("# Do not fall back");
    assert!(!schema.is_valid(&ambiguous));
    assert!(decode(ambiguous.as_object().expect("object").clone()).is_err());
    let mut denied = valid;
    denied
        .as_object_mut()
        .expect("object")
        .remove("allow_native_reads");
    denied["tree"]["root"] = json!(root.path().join("absent"));
    assert!(decode(denied.as_object().expect("object").clone()).is_err());
}

#[test]
fn pinned_tree_assets_mismatch_and_independent_limits_never_yield_partial_success() {
    let root = tempfile::tempdir().expect("native tree");
    std::fs::write(root.path().join("page.md"), "# Page\n").expect("document");
    std::fs::write(root.path().join("asset"), "one").expect("asset");
    let original = tree_arguments(root.path(), "page.md");
    std::fs::write(root.path().join("asset"), "two").expect("asset-only mismatch");
    assert!(execute(original, 1 << 20).is_error.expect("pin refused"));
    let valid = tree_arguments(root.path(), "page.md");
    for limits in [
        json!({"entries":1}),
        json!({"file_bytes":1}),
        json!({"total_bytes":1}),
    ] {
        let mut bounded = valid.clone();
        bounded["tree"]["limits"] = limits;
        assert!(
            execute(bounded, 1 << 20)
                .is_error
                .expect("whole-tree bound")
        );
    }
    let mut parser = valid.clone();
    parser["limits"] = json!({"source_bytes":1});
    assert_eq!(failure_code(&execute(parser, 1 << 20)), "markdown_limit");
    assert_eq!(failure_code(&execute(valid, 1)), "mcp_output_limit");
    std::fs::write(root.path().join("page.md"), b"\xff").expect("invalid text bytes");
    assert_eq!(
        failure_code(&execute(tree_arguments(root.path(), "page.md"), 1 << 20)),
        "markdown_input"
    );
    assert_eq!(
        failure_code(&execute(tree_arguments(root.path(), "missing"), 1 << 20)),
        "markdown_document"
    );
    let before_link = tree_arguments(root.path(), "page.md");
    std::os::unix::fs::symlink("page.md", root.path().join("link"))
        .expect("untrusted link fixture");
    assert!(
        execute(before_link, 1 << 20)
            .is_error
            .expect("links refused before text parsing")
    );
}
