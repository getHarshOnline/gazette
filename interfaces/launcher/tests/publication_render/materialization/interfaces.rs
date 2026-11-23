//! Actual initialized MCP configured outputs, native histories and held writer lifetime.

use std::collections::BTreeMap;

use gazette_sdk::{source, store};
use serde_json::json;

use super::super::{fs, sites, tempdir};
use super::{Fixture, configuration, initialize, session};
use crate::mcp_carrier;

fn call(
    send: &mut dyn FnMut(serde_json::Value) -> serde_json::Value,
    id: u64,
    name: &str,
    arguments: serde_json::Value,
) -> serde_json::Value {
    let response = send(json!({"jsonrpc":"2.0","id":id,"method":"tools/call",
        "params":{"name":name,"arguments":arguments}}));
    assert!(response.get("error").is_none(), "{response}");
    let text: serde_json::Value = serde_json::from_str(
        response["result"]["content"][0]["text"]
            .as_str()
            .expect("complete native JSON evidence"),
    )
    .expect("readable evidence JSON");
    assert_eq!(text, response["result"]["structuredContent"]);
    response
}

fn ids(label: &str) -> BTreeMap<gazette_sdk::workspace::Key, source::Sha256Digest> {
    ["site:one", "site:other"]
        .into_iter()
        .map(|site| {
            (
                site.parse().expect("native site identity"),
                source::Sha256Digest::of(format!("{label}:{site}").as_bytes()),
            )
        })
        .collect()
}

fn snapshot(root: &std::path::Path) -> source::TreeIdentity {
    source::inspect(root, None, Default::default())
        .expect("complete native baseline")
        .identity()
}

#[test]
fn initialized_mcp_materializes_the_complete_set_and_keeps_original_writer_custody() {
    let fixture = Fixture::new(&[]);
    let outputs = tempdir().expect("disjoint configured output roots");
    let roots = [outputs.path().join("one"), outputs.path().join("other")];
    for root in &roots {
        fs::create_dir(root).expect("actual existing destination");
        fs::write(root.join("user.txt"), b"independent user bytes").expect("preserved file");
    }
    let config = configuration(&fixture, &roots[0]).replacen(
        "site='site:other'\npublic_base_url",
        &format!(
            "site='site:other'\noutput={{directory={:?}}}\npublic_base_url",
            roots[1].to_str().expect("output coordinate")
        ),
        1,
    );
    let selected = ["site:one", "site:other"];
    let recipe = sites::request(&config, &selected);
    let ids = selected
        .into_iter()
        .map(|site| {
            (
                site.parse::<gazette_sdk::workspace::Key>().expect("site"),
                source::Sha256Digest::of(format!("mcp-materialization:{site}").as_bytes()),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let source_before = source::inspect(fixture.directory.path(), None, Default::default())
        .expect("source baseline")
        .identity();
    let acquired = sites::acquire_set(&config, &selected);
    let model = "model.json".parse().expect("model");
    let expected = gazette_sdk::build::render_publications(
        &acquired,
        sites::options(
            &selected,
            &"page.html".parse().unwrap(),
            &"alias.html".parse().unwrap(),
            &model,
        ),
        Default::default(),
    )
    .expect("independent retained rendering");
    assert!(expected.is_complete());
    let (journal, initialized) = initialize();
    let options = store::WriterOptions {
        allow_generation_advance: true,
        session: session(),
    };
    let header = mcp_carrier::initialized_interact("2026-07-28", |initialized_protocol, send| {
        assert_eq!(
            initialized_protocol["result"]["protocolVersion"],
            "2026-07-28"
        );
        let catalog = send(json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}));
        let tool = catalog["result"]["tools"]
            .as_array()
            .expect("actual catalogue")
            .iter()
            .find(|tool| tool["name"] == "materialize_sites")
            .expect("configured output operation");
        assert_eq!(tool["annotations"]["readOnlyHint"], false);
        assert_eq!(
            tool["_meta"]["io.rtepl.gazette/terminalResultOwner"],
            "close_store_session"
        );
        let mut call = |id, name, arguments| {
            let response = send(
                json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":arguments}}),
            );
            assert!(response.get("error").is_none(), "{response}");
            response
        };
        let opened = call(
            2,
            "open_store_session",
            json!({"request":{"kind":"writer","root":journal.path(),"expected_header":initialized.header,"allow_native_reads":true,"options":options}}),
        );
        assert_eq!(opened["result"]["isError"], false, "{opened}");
        let ready = &opened["result"]["structuredContent"];
        let handle = ready["session"].clone();
        let header: store::JournalHeader =
            serde_json::from_value(ready["inspection"]["header"].clone())
                .expect("native acquired header");
        let args = json!({"session":handle,"publication":{"recipe_json":recipe.to_string(),"local":true,"allow_read":true},"ids":ids,"allow_write":true,"allow_record":true});
        jsonschema::validator_for(&tool["inputSchema"])
            .expect("discovered input contract")
            .validate(&args)
            .expect("real configured request");
        let database_before = fs::read(&initialized.database).expect("held generation baseline");
        let mut denied = args.clone();
        denied["allow_record"] = json!(false);
        let refusal = call(3, "materialize_sites", denied);
        assert_eq!(refusal["result"]["isError"], true);
        assert!(roots.iter().all(|root| !root.join("model.json").exists()));
        assert_eq!(
            fs::read(&initialized.database).expect("no record or output dispatch"),
            database_before
        );
        let report = call(4, "materialize_sites", args);
        assert_eq!(report["result"]["isError"], false, "{report}");
        let actual = &report["result"]["structuredContent"];
        jsonschema::validator_for(&tool["outputSchema"])
            .expect("native evidence schema")
            .validate(actual)
            .expect("actual complete result");
        assert_eq!(actual["status"], "materialized");
        assert_eq!(actual["active"], true);
        assert_eq!(actual["acquired_header"], json!(header));
        assert_eq!(
            actual["sites"].as_object().expect("complete sites").len(),
            selected.len()
        );
        for (site, root) in selected.into_iter().zip(&roots) {
            let key = site.parse().expect("selected identity");
            let output = expected.sites()[&key]
                .as_ref()
                .expect("retained complete site")
                .site()
                .output();
            let result = &actual["sites"][site];
            assert_eq!(
                result["history"]["record"]["intent"]["id"],
                json!(ids[&key])
            );
            assert!(result["history"]["record"]["completion"].is_object());
            assert_eq!(result["output"]["generated"], json!(output.inventory()));
            for entry in output.inventory().entries() {
                if let Some(bytes) = output.file(&entry.path) {
                    assert_eq!(
                        fs::read(root.join(entry.path.as_str())).expect("actual output bytes"),
                        bytes
                    );
                }
            }
            assert_eq!(
                fs::read(root.join("user.txt")).expect("user preservation"),
                b"independent user bytes"
            );
        }
        let inspected = call(5, "inspect_store_session", json!({"session":handle}));
        assert_eq!(
            inspected["result"]["structuredContent"]["inspection"]["header"],
            json!(header)
        );
        let closed = call(6, "close_store_session", json!({"session":handle}));
        assert_eq!(closed["result"]["structuredContent"]["status"], "closed");
        let unknown = call(
            7,
            "materialize_sites",
            json!({"session":handle,"publication":{"recipe_json":"invalid","local":true,"allow_read":true},"ids":ids,"allow_write":true,"allow_record":true}),
        );
        assert_eq!(unknown["result"]["isError"], true);
        assert_eq!(
            unknown["result"]["structuredContent"]["code"],
            "store_session_unknown"
        );
        header
    });
    for (site, root) in selected.into_iter().zip(&roots) {
        let history = store::inspect_materialization(
            &initialized.database,
            &root.canonicalize().expect("native coordinate"),
            Some(&header),
            session().read,
        )
        .expect("independently reopened native history");
        assert!(history.frontier.active.is_none());
        assert_eq!(
            history
                .frontier
                .head
                .expect("completed ownership")
                .intent
                .id,
            ids[&site.parse().expect("site")]
        );
    }
    assert_eq!(
        source::inspect(fixture.directory.path(), None, Default::default())
            .expect("unchanged source bytes")
            .identity(),
        source_before
    );
}

#[test]
fn initialized_mcp_unknown_and_connection_handles_refuse_before_publication_parse() {
    let fixture = Fixture::new(&[]);
    let output = tempdir().expect("untouched destination");
    fs::write(output.path().join("user.txt"), b"independent user").unwrap();
    let output_before = snapshot(output.path());
    let source_before = snapshot(fixture.directory.path());
    let (journal, initialized) = initialize();
    let database_before = fs::read(&initialized.database).unwrap();
    let operation_ids = ids("refused-handles");
    mcp_carrier::initialized_interact("2026-07-28", |_, send| {
        // An invalid recipe is deliberate: handle admission must win before the
        // shared producer can parse a recipe or open any declared source.
        let arguments = |handle: serde_json::Value| {
            json!({"session":handle,
            "publication":{"recipe_json":"invalid","local":true,"allow_read":true},
            "ids":operation_ids,"allow_write":true,"allow_record":true})
        };
        let unknown = call(
            send,
            1,
            "materialize_sites",
            arguments(json!("0".repeat(64))),
        );
        assert_eq!(unknown["result"]["isError"], true);
        assert_eq!(
            unknown["result"]["structuredContent"]["code"],
            "store_session_unknown"
        );
        assert_eq!(fs::read(&initialized.database).unwrap(), database_before);
        let opened = call(
            send,
            2,
            "open_store_session",
            json!({"request":{
            "kind":"connection","path":initialized.database,"coordination_root":journal.path(),
            "allow_native_reads":true,"options":session()}}),
        );
        assert_eq!(opened["result"]["isError"], false, "{opened}");
        let handle = opened["result"]["structuredContent"]["session"].clone();
        let refused = call(send, 3, "materialize_sites", arguments(handle.clone()));
        assert_eq!(refused["result"]["isError"], true);
        assert_eq!(
            refused["result"]["structuredContent"]["code"],
            "store_session_not_writer"
        );
        let inspected = call(send, 4, "inspect_store_session", json!({"session":handle}));
        assert_eq!(inspected["result"]["isError"], false, "{inspected}");
        assert_eq!(
            inspected["result"]["structuredContent"]["status"],
            "observed"
        );
        assert_eq!(
            inspected["result"]["structuredContent"]["observation"],
            opened["result"]["structuredContent"]["observation"],
            "connection custody remains a raw provider observation, not writer inspection"
        );
        assert_eq!(
            inspected["result"]["structuredContent"]["observation"]["sql_read_only"],
            true
        );
        assert!(
            inspected["result"]["structuredContent"]
                .get("inspection")
                .is_none()
        );
        let closed = call(send, 5, "close_store_session", json!({"session":handle}));
        assert_eq!(closed["result"]["isError"], false, "{closed}");
        assert_eq!(closed["result"]["structuredContent"]["status"], "closed");
    });
    assert_eq!(fs::read(&initialized.database).unwrap(), database_before);
    assert_eq!(snapshot(output.path()), output_before);
    assert_eq!(snapshot(fixture.directory.path()), source_before);
    let history = store::inspect_materialization(
        &initialized.database,
        &output.path().canonicalize().unwrap(),
        Some(&initialized.header),
        session().read,
    )
    .unwrap();
    assert!(history.frontier.head.is_none() && history.frontier.active.is_none());
}

#[test]
fn initialized_mcp_independent_permissions_preserve_held_writer_and_refuse_missing_sources() {
    let fixture = Fixture::new(&[]);
    let output = tempdir().expect("untouched output");
    fs::write(output.path().join("user.txt"), b"owner bytes").unwrap();
    fs::write(
        output.path().join(".git"),
        b"gitdir: ../independent-administration\n",
    )
    .unwrap();
    let output_before = snapshot(output.path());
    let recipe = sites::request(&configuration(&fixture, output.path()), &["site:one"]);
    let source_root = fixture.directory.path().to_owned();
    drop(fixture);
    assert!(
        !source_root.exists(),
        "source read cannot accidentally make refusal pass"
    );
    let (journal, initialized) = initialize();
    let operation_ids = BTreeMap::from([(
        "site:one".parse::<gazette_sdk::workspace::Key>().unwrap(),
        source::Sha256Digest::of(b"denied-mcp-output"),
    )]);
    let header = mcp_carrier::initialized_interact("2026-07-28", |_, send| {
        let opened = call(
            send,
            1,
            "open_store_session",
            json!({"request":{
            "kind":"writer","root":journal.path(),"expected_header":initialized.header,
            "allow_native_reads":true,"options":store::WriterOptions{
                allow_generation_advance:true,session:session()}}}),
        );
        assert_eq!(opened["result"]["isError"], false, "{opened}");
        let ready = &opened["result"]["structuredContent"];
        let handle = ready["session"].clone();
        let header: store::JournalHeader =
            serde_json::from_value(ready["inspection"]["header"].clone()).unwrap();
        let database_before = fs::read(&initialized.database).unwrap();
        let original = json!({"session":handle,"publication":{
            "recipe_json":recipe.to_string(),"local":true,"allow_read":true},
            "ids":operation_ids,"allow_write":true,"allow_record":true});
        for (index, field) in ["allow_write", "allow_record", "local", "allow_read"]
            .into_iter()
            .enumerate()
        {
            let mut arguments = original.clone();
            if matches!(field, "local" | "allow_read") {
                arguments["publication"][field] = json!(false);
            } else {
                arguments[field] = json!(false);
            }
            let digest = source::Sha256Digest::of(&serde_json::to_vec(&arguments).unwrap());
            let response = call(send, 2 + index as u64, "materialize_sites", arguments);
            assert_eq!(response["result"]["isError"], true, "{field}: {response}");
            let report = &response["result"]["structuredContent"];
            if matches!(field, "local" | "allow_read") {
                assert_eq!(report["status"], "failed");
                assert_eq!(report["failure"]["phase"], "render");
                assert_eq!(report["failure"]["code"], "publication_permission");
                assert_eq!(report["session"], handle);
                assert_eq!(report["active"], true);
                assert_eq!(report["acquired_header"], json!(header));
                assert_eq!(report["request_sha256"], json!(digest));
            } else {
                assert_eq!(report["code"], "materialization_permission");
            }
            assert_eq!(fs::read(&initialized.database).unwrap(), database_before);
            assert_eq!(snapshot(output.path()), output_before);
            assert!(!source_root.exists());
        }
        let inspected = call(send, 6, "inspect_store_session", json!({"session":handle}));
        assert_eq!(inspected["result"]["isError"], false, "{inspected}");
        assert_eq!(
            inspected["result"]["structuredContent"]["inspection"]["header"],
            json!(header)
        );
        let closed = call(send, 7, "close_store_session", json!({"session":handle}));
        assert_eq!(closed["result"]["isError"], false, "{closed}");
        assert_eq!(closed["result"]["structuredContent"]["status"], "closed");
        header
    });
    let history = store::inspect_materialization(
        &initialized.database,
        &output.path().canonicalize().unwrap(),
        Some(&header),
        session().read,
    )
    .unwrap();
    assert!(history.frontier.head.is_none() && history.frontier.active.is_none());
    assert_eq!(snapshot(output.path()), output_before);
}

#[test]
fn initialized_mcp_default_cap_overflow_retains_custody_and_actual_completed_history() {
    // Two copies of generated inventory are unavoidable in the native result:
    // history.intent.generated and output.generated. Use the minimum count
    // whose fixed-width native entry encodings alone exceed the real 256 KiB cap.
    const MAXIMUM: usize = 256 * 1024;
    const ASSET: &[u8] = b"retained static bytes\n";
    let name = |index| format!("{index:04}-{}.txt", "resource".repeat(20));
    let entry = source::Entry {
        path: format!("assets/{}", name(0)).parse().unwrap(),
        content: source::Content::File {
            bytes: ASSET.len() as u64,
            sha256: source::Sha256Digest::of(ASSET),
        },
    };
    let count = MAXIMUM / (2 * serde_json::to_vec(&entry).unwrap().len()) + 1;
    assert!(count < 1024, "bounded ordinary static fixture");
    let paths = (0..count)
        .map(|index| format!("one/{}", name(index)))
        .collect::<Vec<_>>();
    let overrides = paths
        .iter()
        .map(|path| (path.as_str(), ASSET))
        .collect::<Vec<_>>();
    let fixture = Fixture::new(&overrides);
    let destination = tempdir().expect("actual output repository");
    let repository = git2::Repository::init(destination.path()).unwrap();
    fs::write(destination.path().join("user.txt"), b"untouched user bytes").unwrap();
    let git_before = snapshot(repository.path());
    let source_before = snapshot(fixture.directory.path());
    let config = configuration(&fixture, destination.path()).replace(
        "authored=[{source='source:one',target='one'}]",
        "authored=[{source='source:one',target='one',static_assets={route_prefix='/assets/'}}]",
    );
    let selected = ["site:one"];
    let acquired = sites::acquire_set(&config, &selected);
    let expected = gazette_sdk::build::render_publications(
        &acquired,
        sites::options(
            &selected,
            &"page.html".parse().unwrap(),
            &"alias.html".parse().unwrap(),
            &"model.json".parse().unwrap(),
        ),
        Default::default(),
    )
    .expect("ordinary static transport within unchanged build bounds");
    assert!(expected.is_complete());
    let key = "site:one".parse::<gazette_sdk::workspace::Key>().unwrap();
    let tree = expected.sites()[&key].as_ref().unwrap().site().output();
    assert!(
        2 * serde_json::to_vec(tree.inventory()).unwrap().len() > MAXIMUM,
        "actual native full-result lower bound, not a hypothetical DTO"
    );
    let operation_ids = BTreeMap::from([(
        key.clone(),
        source::Sha256Digest::of(b"mcp-overflow-original"),
    )]);
    let recipe = sites::request(&config, &selected);
    let (journal, initialized) = initialize();
    let header = mcp_carrier::initialized_interact_with_call_timeout(
        "2026-07-28",
        std::time::Duration::from_secs(30),
        |_, send| {
            let catalog = send(json!({"jsonrpc":"2.0","id":0,"method":"tools/list","params":{}}));
            let tool = catalog["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .find(|tool| tool["name"] == "materialize_sites")
                .unwrap();
            let opened = call(
                send,
                1,
                "open_store_session",
                json!({"request":{
            "kind":"writer","root":journal.path(),"expected_header":initialized.header,
            "allow_native_reads":true,"options":store::WriterOptions{
                allow_generation_advance:true,session:session()}}}),
            );
            assert_eq!(opened["result"]["isError"], false, "{opened}");
            let ready = &opened["result"]["structuredContent"];
            let handle = ready["session"].clone();
            let header: store::JournalHeader =
                serde_json::from_value(ready["inspection"]["header"].clone()).unwrap();
            let arguments = json!({"session":handle,"publication":{
            "recipe_json":recipe.to_string(),"local":true,"allow_read":true},
            "ids":operation_ids,"allow_write":true,"allow_record":true});
            jsonschema::validator_for(&tool["inputSchema"])
                .unwrap()
                .validate(&arguments)
                .unwrap();
            let digest = source::Sha256Digest::of(&serde_json::to_vec(&arguments).unwrap());
            let response = call(send, 2, "materialize_sites", arguments);
            assert_eq!(
                response["result"]["isError"], true,
                "missing evidence is not success: {response}"
            );
            let report = &response["result"]["structuredContent"];
            jsonschema::validator_for(&tool["outputSchema"])
                .unwrap()
                .validate(report)
                .unwrap();
            assert_eq!(report["status"], "delivery");
            assert_eq!(report["code"], "materialization_response_unavailable");
            assert_eq!(report["complete_evidence"], false);
            assert_eq!(report["may_have_effects"], true);
            assert_eq!(report["session"], handle);
            assert_eq!(report["active"], true);
            assert_eq!(report["acquired_header"], json!(header));
            assert_eq!(report["request_sha256"], json!(digest));
            assert_eq!(report["ids"], json!(operation_ids));
            assert!(report.get("sites").is_none(), "no invented full result");
            assert!(serde_json::to_vec(report).unwrap().len() < MAXIMUM);
            let inspected = call(send, 3, "inspect_store_session", json!({"session":handle}));
            assert_eq!(
                inspected["result"]["isError"], false,
                "custody survives overflow: {inspected}"
            );
            assert_eq!(
                inspected["result"]["structuredContent"]["inspection"]["header"],
                json!(header)
            );
            let closed = call(send, 4, "close_store_session", json!({"session":handle}));
            assert_eq!(closed["result"]["isError"], false, "{closed}");
            assert_eq!(closed["result"]["structuredContent"]["status"], "closed");
            header
        },
    );
    let history = store::inspect_materialization(
        &initialized.database,
        &destination.path().canonicalize().unwrap(),
        Some(&header),
        session().read,
    )
    .unwrap();
    assert!(history.frontier.active.is_none());
    let record = history
        .frontier
        .head
        .expect("independently verified durable completion");
    assert_eq!(record.intent.id, operation_ids[&key]);
    assert_eq!(record.intent.generated, *tree.inventory());
    record
        .completion
        .as_ref()
        .expect("actual completed history")
        .validate(&record.intent)
        .unwrap();
    for entry in tree.inventory().entries() {
        if let Some(bytes) = tree.file(&entry.path) {
            assert_eq!(
                fs::read(destination.path().join(entry.path.as_str())).unwrap(),
                bytes
            );
        }
    }
    assert_eq!(
        fs::read(destination.path().join("user.txt")).unwrap(),
        b"untouched user bytes"
    );
    assert_eq!(snapshot(repository.path()), git_before);
    assert_eq!(snapshot(fixture.directory.path()), source_before);
}
