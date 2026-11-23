//! Native CLI/MCP checkout selection, bounded evidence and explicit writer close.

use super::*;

#[test]
fn configured_checkout_cli_retains_setup_and_complete_materialization_evidence() {
    let fixture = Setup::new();
    let (journal, initialized) = initialize();
    let selected = fixture._parent.path().join("checkouts.json");
    fs::write(&selected, serde_json::to_vec(&fixture.selections).unwrap()).unwrap();
    let mut args = vec![
        "materialize-sites".to_owned(),
        journal.path().to_str().unwrap().to_owned(),
        "--expected-header".into(),
        serde_json::to_string(&initialized.header).unwrap(),
        "--checkouts".into(),
        selected.to_str().unwrap().into(),
    ];
    args.extend(
        [
            "--local",
            "--allow-read",
            "--allow-write-output",
            "--allow-record-history",
            "--advance-writer-generation",
            "--reserve-local-coordination",
            "--allow-coordination-files",
        ]
        .map(str::to_owned),
    );
    for (site, id) in ids() {
        args.extend(["--operation".into(), format!("{site}={id}")]);
    }
    let output = crate::cli(
        &sites::request(&fixture.config, &SELECTED).to_string(),
        &args.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    assert!(output.status.success(), "{output:?}");
    let frames = std::str::from_utf8(&output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    let result = frames.last().unwrap();
    assert_eq!(result["native_status"], "materialization_completed");
    assert_eq!(result["close"]["status"], "closed");
    assert_eq!(result["detail"]["checkouts"].as_object().unwrap().len(), 2);
    assert_eq!(result["detail"]["output"].as_object().unwrap().len(), 2);
    assert!(
        fixture
            .roots
            .iter()
            .all(|root| root.join("model.json").is_file())
    );
}

#[test]
fn configured_checkout_mcp_matches_schema_and_refuses_null_before_native_setup() {
    use serde_json::json;
    let fixture = Setup::new();
    let (journal, initialized) = initialize();
    crate::mcp_carrier::initialized_interact("2026-07-28", |_, send| {
        let catalog = send(json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}));
        let descriptor = catalog["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == "materialize_sites")
            .unwrap()
            .clone();
        let mut call = |id, name: &str, arguments| {
            send(
                json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":arguments}}),
            )
        };
        let opened = call(
            2,
            "open_store_session",
            json!({"request":{"kind":"writer","root":journal.path(),"expected_header":initialized.header,"allow_native_reads":true,"options":{"allow_generation_advance":true,"session":session()}}}),
        );
        assert_eq!(opened["result"]["isError"], false, "{opened}");
        let handle = opened["result"]["structuredContent"]["session"].clone();
        let args = json!({"session":handle,"publication":{"recipe_json":sites::request(&fixture.config,&SELECTED).to_string(),"local":true,"allow_read":true},"ids":ids(),"allow_write":true,"allow_record":true,"checkouts":fixture.selections});
        let validator = jsonschema::validator_for(&descriptor["inputSchema"]).unwrap();
        validator.validate(&args).unwrap();
        let mut null = args.clone();
        null["checkouts"] = json!(null);
        assert!(validator.validate(&null).is_err());
        let refused = call(3, "materialize_sites", null);
        assert!(refused.get("error").is_some() || refused["result"]["isError"] == true);
        assert!(!fixture.roots[0].exists());
        let result = call(4, "materialize_sites", args);
        assert_eq!(result["result"]["isError"], false, "{result}");
        let evidence = &result["result"]["structuredContent"];
        jsonschema::validator_for(&descriptor["outputSchema"])
            .unwrap()
            .validate(evidence)
            .unwrap();
        assert_eq!(evidence["checkouts"].as_object().unwrap().len(), 2);
        assert_eq!(evidence["sites"].as_object().unwrap().len(), 2);
        let closed = call(5, "close_store_session", json!({"session":handle}));
        assert_eq!(closed["result"]["structuredContent"]["status"], "closed");
    });
}
