//! Live catalog schema, effects and paging checks.

use super::*;

#[test]
fn fixed_paged_catalog_declares_exact_effects_and_no_unimplemented_capability() {
    let response = runtime().block_on(exchange(
        request("tools/list", json!({})),
        server::Limits::default(),
    ));
    let tools = response["result"]["tools"]
        .as_array()
        .expect("tool catalog");
    assert_eq!(
        tools
            .iter()
            .map(|tool| tool["name"].as_str().expect("tool name"))
            .collect::<Vec<_>>(),
        discovery_names(),
        "the live catalog must use the canonical registry's progressive order"
    );
    let first_advanced = tools
        .iter()
        .position(|tool| tool["_meta"]["io.rtepl.gazette/prominence"] == "advanced")
        .expect("catalog includes specialist operations");
    assert!(first_advanced >= 8, "the first page is entirely common");
    assert!(
        tools[..first_advanced]
            .iter()
            .all(|tool| { tool["_meta"]["io.rtepl.gazette/prominence"] == "common" })
    );
    assert!(
        tools[first_advanced..]
            .iter()
            .all(|tool| { tool["_meta"]["io.rtepl.gazette/prominence"] == "advanced" })
    );
    let capabilities = [
        "authoring",
        "build",
        "documentation",
        "observation",
        "preview",
        "recovery",
        "release",
        "store",
        "workspace",
    ];
    let mut neighborhoods = std::collections::BTreeMap::<&str, usize>::new();
    let names = tools
        .iter()
        .map(|tool| tool["name"].as_str().expect("named tool"))
        .collect::<std::collections::BTreeSet<_>>();
    for tool in tools {
        let name = tool["name"].as_str().expect("named tool");
        let registered = metadata::operation(name).expect("registered MCP operation");
        let capability = tool["_meta"]["io.rtepl.gazette/capability"]
            .as_str()
            .expect("one canonical capability neighborhood");
        assert_eq!(capability, registered.capability.key());
        assert!(
            capabilities.contains(&capability),
            "unknown capability {capability}"
        );
        *neighborhoods.entry(capability).or_default() += 1;
        for key in ["intent", "effect", "prominence", "closure"] {
            assert!(
                tool["_meta"][format!("io.rtepl.gazette/{key}")]
                    .as_str()
                    .is_some_and(|value| !value.is_empty()),
                "{name} has no canonical {key}: {tool}"
            );
        }
        let intent = tool["_meta"]["io.rtepl.gazette/intent"]
            .as_str()
            .expect("canonical intent");
        let effect = tool["_meta"]["io.rtepl.gazette/effect"]
            .as_str()
            .expect("canonical effect");
        let closure = tool["_meta"]["io.rtepl.gazette/closure"]
            .as_str()
            .expect("canonical closure");
        assert_eq!(intent, registered.intent.key());
        assert_eq!(effect, registered.effect.key());
        assert_eq!(
            tool["_meta"]["io.rtepl.gazette/prominence"],
            registered.prominence.key()
        );
        assert_eq!(closure, registered.closure.key());
        let relations = tool["_meta"]["io.rtepl.gazette/relations"].as_array();
        assert_eq!(
            relations.map(Vec::len).unwrap_or_default(),
            registered.relations.len(),
            "{name} relation projection drifted from the canonical registry"
        );
        if let Some(relations) = relations {
            for (relation, registered_relation) in relations.iter().zip(registered.relations) {
                let target = relation["operation"]
                    .as_str()
                    .expect("related operation name");
                assert_eq!(target, registered_relation.operation);
                assert_eq!(
                    relation["kind"],
                    registered_relation.kind.key(),
                    "{name} relation kind drifted from the canonical registry"
                );
                assert!(names.contains(target), "{name} relates to absent {target}");
                assert!(
                    matches!(
                        relation["kind"].as_str(),
                        Some(
                            "inspect" | "verify" | "observe" | "reconcile" | "recover" | "contrast"
                        )
                    ),
                    "{name} has an unknown relation: {relation}"
                );
            }
        }
        let terminal_owner = tool["_meta"]["io.rtepl.gazette/terminalResultOwner"].as_str();
        assert_eq!(terminal_owner, registered.terminal_result_owner);
        if let Some(owner) = terminal_owner {
            assert!(
                names.contains(owner),
                "{name} closes through absent {owner}"
            );
        }
        if matches!(intent, "act" | "reconcile" | "recover") {
            match closure {
                "related" => assert!(
                    relations.is_some_and(|relations| !relations.is_empty()),
                    "{name} declares related closure without a proven edge"
                ),
                "terminal-owner" => assert!(
                    terminal_owner.is_some(),
                    "{name} declares a terminal owner without naming it"
                ),
                "self-terminal" => {
                    assert!(terminal_owner.is_none(), "{name} has two closure owners");
                    assert!(
                        relations.is_none_or(Vec::is_empty),
                        "{name} self-terminates but also prescribes a relation"
                    );
                }
                other => panic!("{name} action lacks a closure disposition: {other}"),
            }
        }
        if !tool["annotations"]["readOnlyHint"]
            .as_bool()
            .expect("read-only annotation")
        {
            assert_ne!(
                effect, "read",
                "{name} requests an effect but discovery calls it read-only"
            );
        }
        if tool["annotations"]["readOnlyHint"]
            .as_bool()
            .expect("read-only annotation")
        {
            assert_ne!(
                effect, "local",
                "{name} is read-only but discovery claims local state may change"
            );
        }
        if effect == "external" {
            assert_eq!(
                tool["annotations"]["openWorldHint"], true,
                "{name} can reach a provider but lacks the protocol hint"
            );
        }
        assert!(
            tool["title"]
                .as_str()
                .is_some_and(|title| title.contains(" · ")),
            "native discovery includes a grouped human title: {tool}"
        );
        let mutation = matches!(
            name,
            "adopt_store_restore"
                | "assemble_docs"
                | "render_sites"
                | "materialize_sites"
                | "materialize_docs"
                | "qualify_sites"
                | "authoring_start"
                | "authoring_import_editor_context"
                | "authoring_open_document"
                | "authoring_change_document"
                | "authoring_close_document"
                | "authoring_close_session"
                | "prepare_sites"
                | "prepare_release_commit"
                | "checkpoint_release_commit"
                | "recover_journal_worktree"
                | "retain_release_commit"
                | "stage"
                | "stage_rendered_site"
                | "start_preview"
                | "start_production_preview"
                | "stop_preview"
                | "inspect_store"
                | "inspect_journal"
                | "inspect_registry"
                | "inspect_materialization"
                | "inspect_journal_worktree"
                | "inspect_release_attempts"
                | "inspect_release_authorization"
                | "inspect_release_checkpoint"
                | "inspect_release_consumption"
                | "inspect_release_leases"
                | "inspect_release_preparations"
                | "inspect_release_reconciliations"
                | "verify_prepared_authorization"
                | "inspect_release_observation"
                | "record_release_observation"
                | "enroll_local_coordinator"
                | "inspect_local_coordinator"
                | "open_recovery_session"
                | "open_release_session"
                | "operate_recovery_session"
                | "operate_release_session"
                | "close_recovery_session"
                | "close_release_session"
                | "open_store_session"
                | "inspect_store_session"
                | "close_store_session"
                | "initialize_store"
                | "backup_store"
                | "verify_backup"
                | "prepare_store_restore"
                | "reconcile_registry"
        );
        assert_eq!(tool["annotations"]["readOnlyHint"], !mutation);
        assert_eq!(
            tool["annotations"]["destructiveHint"],
            matches!(
                name,
                "adopt_store_restore"
                    | "assemble_docs"
                    | "render_sites"
                    | "materialize_sites"
                    | "materialize_docs"
                    | "qualify_sites"
                    | "authoring_change_document"
                    | "authoring_close_document"
                    | "authoring_close_session"
                    | "prepare_sites"
                    | "checkpoint_release_commit"
                    | "recover_journal_worktree"
                    | "prepare_release_commit"
                    | "retain_release_commit"
                    | "stage"
                    | "stage_rendered_site"
                    | "start_preview"
                    | "start_production_preview"
                    | "stop_preview"
                    | "record_release_observation"
                    | "enroll_local_coordinator"
                    | "open_recovery_session"
                    | "open_release_session"
                    | "operate_recovery_session"
                    | "operate_release_session"
                    | "close_recovery_session"
                    | "close_release_session"
                    | "open_store_session"
                    | "close_store_session"
                    | "initialize_store"
                    | "backup_store"
                    | "prepare_store_restore"
                    | "reconcile_registry"
            )
        );
        assert_eq!(
            tool["annotations"]["openWorldHint"],
            matches!(
                name,
                "adopt_store_restore"
                    | "authoring_import_editor_context"
                    | "assemble_docs"
                    | "check_stage"
                    | "probe_rendered_site"
                    | "stage_rendered_site"
                    | "checkpoint_release_commit"
                    | "recover_journal_worktree"
                    | "prepare_release_commit"
                    | "retain_release_commit"
                    | "stage"
                    | "start_preview"
                    | "start_production_preview"
                    | "plan_preview"
                    | "plan_production_preview"
                    | "inspect_preview"
                    | "poll_preview"
                    | "stop_preview"
                    | "probe_output"
                    | "inspect_markdown"
                    | "inspect_source"
                    | "inspect_store"
                    | "inspect_journal"
                    | "inspect_registry"
                    | "inspect_materialization"
                    | "inspect_journal_worktree"
                    | "inspect_release_attempts"
                    | "inspect_release_authorization"
                    | "inspect_release_checkpoint"
                    | "inspect_release_consumption"
                    | "inspect_release_leases"
                    | "inspect_release_preparations"
                    | "inspect_release_reconciliations"
                    | "verify_prepared_authorization"
                    | "inspect_release_observation"
                    | "record_release_observation"
                    | "enroll_local_coordinator"
                    | "inspect_local_coordinator"
                    | "inspect_toml"
                    | "observe_delivery"
                    | "observe_github_pages_build"
                    | "observe_release_targets"
                    | "verify_generated_tree"
                    | "verify_release_commit"
                    | "inspect_release_custody"
                    | "prepare_sites"
                    | "render_sites"
                    | "materialize_sites"
                    | "materialize_docs"
                    | "qualify_sites"
                    | "inspect_publication"
                    | "review_publication"
                    | "project_registry"
                    | "reconcile_registry"
                    | "inspect_composition"
                    | "check_composition"
                    | "compile_templates"
                    | "inspect_site"
                    | "parse_site_documents"
                    | "open_recovery_session"
                    | "open_release_session"
                    | "operate_recovery_session"
                    | "operate_release_session"
                    | "close_recovery_session"
                    | "close_release_session"
                    | "open_store_session"
                    | "inspect_store_session"
                    | "close_store_session"
                    | "initialize_store"
                    | "backup_store"
                    | "verify_backup"
                    | "prepare_store_restore"
            )
        );
        let input = &tool["inputSchema"];
        let variants = input["anyOf"].as_array();
        for variant in variants.map_or_else(|| vec![input], |values| values.iter().collect()) {
            let object = if let Some(reference) = variant["$ref"].as_str() {
                input
                    .pointer(reference.strip_prefix('#').expect("local schema reference"))
                    .expect("defined input variant")
            } else {
                variant
            };
            assert_eq!(object["additionalProperties"], false, "{name}");
        }
        assert_eq!(tool["outputSchema"]["type"], "object", "{}", tool["name"]);
        jsonschema::validator_for(&tool["outputSchema"]).expect("resolvable native output schema");
    }
    for (operation, kind, related) in [
        ("inspect_store", "contrast", "inspect_journal"),
        ("inspect_publication", "contrast", "review_publication"),
        ("render_sites", "verify", "qualify_sites"),
        (
            "observe_release_targets",
            "contrast",
            "observe_github_pages_build",
        ),
        (
            "operate_release_session",
            "observe",
            "observe_release_targets",
        ),
        (
            "operate_release_session",
            "reconcile",
            "inspect_release_reconciliations",
        ),
    ] {
        let tool = tools
            .iter()
            .find(|tool| tool["name"] == operation)
            .expect("confusable operation");
        assert!(
            tool["_meta"]["io.rtepl.gazette/relations"]
                .as_array()
                .expect("relation guidance")
                .iter()
                .any(|relation| { relation["kind"] == kind && relation["operation"] == related }),
            "{operation} does not distinguish {related}"
        );
    }
    for (name, capability, effect) in [
        ("review_publication", "build", "read"),
        ("verify_generated_tree", "documentation", "read"),
        ("reconcile_registry", "store", "local"),
        ("observe_release_targets", "observation", "external"),
        ("prepare_sites", "release", "local"),
        ("qualify_sites", "build", "local"),
        ("verify_backup", "store", "local"),
    ] {
        let tool = tools
            .iter()
            .find(|tool| tool["name"] == name)
            .expect("operation with an exact semantic override");
        assert_eq!(
            tool["_meta"]["io.rtepl.gazette/capability"], capability,
            "{name} drifted into the wrong capability neighborhood"
        );
        assert_eq!(
            tool["_meta"]["io.rtepl.gazette/effect"], effect,
            "{name} no longer reports its widest native effect"
        );
    }
    let review = tools
        .iter()
        .find(|tool| tool["name"] == "review_publication")
        .expect("publication review operation");
    assert_eq!(review["_meta"]["io.rtepl.gazette/intent"], "inspect");
    assert_eq!(review["annotations"]["readOnlyHint"], true);
    for (start, terminal) in [
        ("authoring_start", "authoring_close_session"),
        ("authoring_import_editor_context", "authoring_close_session"),
        ("open_store_session", "close_store_session"),
        ("materialize_sites", "close_store_session"),
        ("materialize_docs", "close_store_session"),
        ("open_release_session", "close_release_session"),
        ("open_recovery_session", "close_recovery_session"),
        ("start_preview", "stop_preview"),
        ("start_production_preview", "stop_preview"),
    ] {
        let tool = tools
            .iter()
            .find(|tool| tool["name"] == start)
            .expect("lifecycle start");
        assert_eq!(
            tool["_meta"]["io.rtepl.gazette/terminalResultOwner"], terminal,
            "{start} lacks its terminal result owner"
        );
    }
    assert_eq!(
        neighborhoods.keys().copied().collect::<Vec<_>>(),
        capabilities,
        "every stable neighborhood remains represented"
    );
    assert!(
        neighborhoods.values().all(|count| *count <= 32),
        "one neighborhood has become another unbounded flat catalog: {neighborhoods:?}"
    );
    use rmcp::ServerHandler;
    let info = serde_json::to_value(server::Server::new(server::Limits::default()).get_info())
        .expect("server info");
    assert_eq!(info["protocolVersion"], "2026-07-28");
    let capabilities = info["capabilities"]
        .as_object()
        .expect("capabilities object");
    assert_eq!(
        capabilities.keys().map(String::as_str).collect::<Vec<_>>(),
        ["tools"]
    );
}

#[test]
fn complete_catalog_reaches_broad_hosts_and_selected_pages_use_strict_current_cursors() {
    runtime().block_on(async {
        let complete =
            exchange_one(request("tools/list", json!({})), server::Limits::default()).await;
        assert!(complete.get("error").is_none(), "{complete}");
        assert_eq!(complete["result"]["resultType"], "complete");
        assert_eq!(complete["result"]["ttlMs"], 0);
        assert_eq!(complete["result"]["cacheScope"], "private");
        assert!(complete["result"].get("nextCursor").is_none());
        let complete_tools = complete["result"]["tools"]
            .as_array()
            .expect("complete host catalog");
        assert_eq!(
            complete_tools
                .iter()
                .map(|tool| tool["name"].as_str().expect("tool name"))
                .collect::<Vec<_>>(),
            discovery_names(),
            "a host that materializes only its first result must still reach every operation"
        );

        let selected_capabilities = ["authoring", "build", "workspace"];
        let mut cursor: Option<String> = None;
        let mut names = Vec::new();
        let mut pages = 0;
        let mut first_cursor = None;
        let mut snapshot_digest: Option<String> = None;
        loop {
            let mut selected = request(
                "tools/list",
                cursor
                    .as_ref()
                    .map_or_else(|| json!({}), |cursor| json!({"cursor":cursor})),
            );
            if cursor.is_none() {
                selected["params"]["_meta"]["io.rtepl.gazette/discovery"] =
                    json!({"capabilities":selected_capabilities});
            }
            let response = exchange_one(selected, server::Limits::default()).await;
            assert!(response.get("error").is_none(), "{response}");
            let result = &response["result"];
            assert_eq!(result["resultType"], "complete");
            assert_eq!(result["ttlMs"], 0);
            assert_eq!(result["cacheScope"], "private");
            let tools = result["tools"].as_array().expect("tool page");
            assert!(!tools.is_empty());
            assert!(tools.len() <= 8, "fixed page bound: {}", tools.len());
            assert!(tools.iter().all(|tool| {
                selected_capabilities.contains(
                    &tool["_meta"]["io.rtepl.gazette/capability"]
                        .as_str()
                        .expect("capability"),
                )
            }));
            let next = result["nextCursor"].as_str().map(str::to_owned);
            if next.is_some() {
                assert_eq!(tools.len(), 8, "only the terminal page may be partial");
            }
            if let Some(next) = &next {
                let fields = next.split('.').collect::<Vec<_>>();
                assert_eq!(fields.len(), 4, "canonical cursor shape");
                if let Some(snapshot_digest) = &snapshot_digest {
                    assert_eq!(
                        &fields[1], snapshot_digest,
                        "every page binds one catalog snapshot"
                    );
                } else {
                    snapshot_digest = Some(fields[1].to_owned());
                }
            }
            names.extend(
                tools
                    .iter()
                    .map(|tool| tool["name"].as_str().expect("tool name").to_owned()),
            );
            pages += 1;
            assert!(pages <= 128);
            if first_cursor.is_none() {
                first_cursor.clone_from(&next);
            }
            let Some(next) = next else { break };
            cursor = Some(next);
        }
        assert!(pages > 1, "the live catalog must exercise pagination");
        let expected = complete_tools
            .iter()
            .filter(|tool| {
                selected_capabilities.contains(
                    &tool["_meta"]["io.rtepl.gazette/capability"]
                        .as_str()
                        .expect("capability"),
                )
            })
            .map(|tool| tool["name"].as_str().expect("tool name").to_owned())
            .collect::<Vec<_>>();
        assert_eq!(names, expected);
        assert_eq!(
            names
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            names.len(),
            "progressive discovery still exposes every operation exactly once"
        );

        let valid = first_cursor.expect("nonterminal cursor");
        let fields = valid.split('.').collect::<Vec<_>>();
        assert_eq!(fields.len(), 4);
        let mut wrong_digest = fields[1].as_bytes().to_vec();
        wrong_digest[0] = if wrong_digest[0] == b'0' { b'1' } else { b'0' };
        let wrong_digest = String::from_utf8(wrong_digest).expect("ASCII digest");
        let uppercase_index = fields[1]
            .bytes()
            .position(|byte| matches!(byte, b'a'..=b'f'))
            .expect("SHA-256 fixture contains a hexadecimal letter");
        let mut uppercase = fields[1].to_owned();
        uppercase.replace_range(
            uppercase_index..uppercase_index + 1,
            &fields[1][uppercase_index..uppercase_index + 1].to_ascii_uppercase(),
        );
        for invalid in [
            format!("g3.{}.{}.{}", fields[1], fields[2], fields[3]),
            format!("g2.{wrong_digest}.{}.{}", fields[2], fields[3]),
            format!("g2.{uppercase}.{}.{}", fields[2], fields[3]),
            format!("g2.{}.000.{}", fields[1], fields[3]),
            format!("g2.{}.fff.{}", fields[1], fields[3]),
            format!("g2.{}.{}.0000000000000001", fields[1], fields[2]),
            format!("g2.{}.{}.fffffffffffffff8", fields[1], fields[2]),
            format!("{valid}.trailing"),
        ] {
            let response = exchange_one(
                request("tools/list", json!({"cursor":invalid})),
                server::Limits::default(),
            )
            .await;
            assert_eq!(response["error"]["code"], -32602, "{response}");
        }
    });
}
