//! Native executable carriers only; recipes and qualification remain SDK-owned.

use super::*;

pub(super) fn run(
    request: &Value,
    mcp: bool,
    allow_read: bool,
    allow_policy: bool,
    complete: bool,
) -> Value {
    if !mcp {
        let mut arguments = vec!["qualify-sites"];
        if allow_read {
            arguments.push("--allow-read");
        }
        if allow_policy {
            arguments.push("--allow-policy-execution");
        }
        let output = crate::cli(&request.to_string(), &arguments);
        assert_eq!(output.status.success(), complete, "{output:?}");
        assert!(
            output.stderr.is_empty(),
            "native incomplete reports stay complete on stdout"
        );
        return serde_json::from_slice(&output.stdout)
            .expect("complete native qualification report");
    }
    let arguments = json!({"recipe_json":request.to_string(),"allow_read":allow_read,"allow_policy_execution":allow_policy});
    // This fixture selects one candidate and one output checker per site, with
    // no generators. The carrier must allow their existing native deadlines;
    // discovery and EOF retain the shared ten-second transport bound.
    let checks = u32::try_from(request["sites"].as_array().unwrap().len()).unwrap() * 2;
    let call_timeout =
        super::qualification_options().policy.timeout * checks + std::time::Duration::from_secs(10);
    let responses = crate::mcp_carrier::exchange_with_call_timeout(
        vec![
            json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}),
            json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"qualify_sites","arguments":arguments}}),
        ],
        call_timeout,
    );
    let descriptor = responses[0]["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "qualify_sites")
        .expect("actual discoverable operation");
    jsonschema::validator_for(&descriptor["inputSchema"])
        .unwrap()
        .validate(&arguments)
        .expect("actual carrier matches native discovery");
    assert_eq!(
        descriptor["inputSchema"]["properties"]["recipe_json"]["contentSchema"],
        serde_json::to_value(build::QualificationRecipe::input_schema()).unwrap()
    );
    assert_eq!(descriptor["annotations"]["readOnlyHint"], false);
    assert_eq!(descriptor["annotations"]["idempotentHint"], false);
    let response = &responses[1];
    assert!(response.get("error").is_none(), "{response}");
    assert_eq!(response["result"]["isError"], !complete, "{response}");
    let report = &response["result"]["structuredContent"];
    jsonschema::validator_for(&descriptor["outputSchema"])
        .unwrap()
        .validate(report)
        .expect("complete native report matches discovery");
    let text: Value =
        serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(&text, report);
    report.clone()
}
