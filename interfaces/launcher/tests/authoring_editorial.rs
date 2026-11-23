//! Initialized executable MCP and SDK share native editorial source custody.
#[allow(dead_code)]
#[path = "support/mcp.rs"]
mod carrier;
use gazette_sdk::{
    authoring::session::{Session, Source},
    source::Sha256Digest,
};
use serde_json::{Value, json};

fn call(send: &mut dyn FnMut(Value) -> Value, id: usize, name: &str, arguments: Value) -> Value {
    let response = send(
        json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":arguments}}),
    );
    assert!(response.get("error").is_none(), "{response}");
    assert_eq!(response["result"]["isError"], false, "{response}");
    response["result"]["structuredContent"].clone()
}
fn source(value: &Value) -> Source {
    Source {
        id: value["id"].as_str().unwrap().into(),
        path: value["path"].as_str().unwrap().parse().unwrap(),
        text: value["text"].as_str().unwrap().into(),
    }
}
fn framed(id: &str, relations: &str, structure: &str) -> String {
    format!(
        "+++\r\n# é😀 native metadata\r\n{relations}[content]\r\nid='{id}'\r\nsite='site:one'\r\ntitle='Title'\r\nlifecycle='draft'\r\nstructure={structure}\r\n+++\r\n# Body\r\n"
    )
}

#[test]
fn initialized_editorial_definition_references_completion_and_overlay_repair_match_sdk() {
    let config = tempfile::tempdir().unwrap();
    let owners = [tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap()];
    let workspace_text = "principals=[{id='owner:one',kind='person'}]\nsites=[{id='site:one',publisher='owner:one',deployable=false}]\n";
    std::fs::write(config.path().join("gazette.toml"), workspace_text).unwrap();
    let workspace = json!({"id":format!("file://{}",config.path().join("gazette.toml").display()),"path":"gazette.toml","text":std::fs::read_to_string(config.path().join("gazette.toml")).unwrap()});
    let files = [
        (
            0,
            "root.md",
            "owners/root.md",
            framed("collection:root", "", "{kind='collection'}"),
        ),
        (
            0,
            "child.md",
            "owners/child.md",
            framed(
                "collection:child",
                "",
                "{kind='collection',parent='collection:root'}",
            ),
        ),
        (
            0,
            "journey.md",
            "owners/journey.md",
            framed(
                "journey:one",
                "",
                "{kind='journey',collection='collection:child',members=[{content='content:one',primary=true}]}",
            ),
        ),
        (
            1,
            "member.md",
            "articles/member.md",
            framed("content:one", "", "{kind='page',family={kind='article'}}"),
        ),
        (
            1,
            "direct.md",
            "articles/direct.md",
            framed(
                "content:direct",
                "collections=['collection:child']\r\n",
                "{kind='page',family={kind='article'}}",
            ),
        ),
    ];
    let documents = files.iter().map(|(owner, entry, logical, text)| {
        let native = owners[*owner].path().join(entry);
        std::fs::write(&native, text).unwrap();
        json!({"id":format!("file://{}", native.display()),"path":logical,"text":std::fs::read_to_string(native).unwrap()})
    }).collect::<Vec<_>>();
    let mut sdk = Session::new(
        source(&workspace),
        documents.iter().map(source),
        Default::default(),
    )
    .unwrap();
    assert!(sdk.analysis().as_ref().unwrap().structurally_valid());
    carrier::initialized_interact("2026-07-28", |initialized, send| {
        assert_eq!(initialized["result"]["protocolVersion"], "2026-07-28");
        let catalog =
            send(json!({"jsonrpc":"2.0","id":"catalog","method":"tools/list","params":{}}));
        let tool = catalog["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == "authoring_complete_reference")
            .unwrap();
        let schema = tool["outputSchema"].to_string();
        assert!(
            schema.contains("EditorialReferenceCompletion"),
            "native output union is discoverable"
        );
        let created = call(
            send,
            1,
            "authoring_start",
            json!({"workspace":workspace,"documents":documents}),
        );
        let handle = created["session"].as_str().unwrap();
        for (id, source_index, needle, target_index) in [
            (2, 1, "collection:root", 0),
            (3, 2, "collection:child", 1),
            (4, 2, "content:one", 3),
            (5, 4, "collection:child", 1),
        ] {
            let document = documents[source_index]["id"].as_str().unwrap();
            let text = documents[source_index]["text"].as_str().unwrap();
            let offset = text.find(needle).unwrap();
            let report = call(
                send,
                id,
                "authoring_definition",
                json!({"session":handle,"generation":1,"document":document,"offset":offset}),
            );
            let native = sdk
                .analysis()
                .as_ref()
                .unwrap()
                .source_navigation(sdk.files()[document].analysis_path(), text, offset)
                .unwrap()
                .unwrap();
            assert_eq!(report["value"]["document"], documents[target_index]["id"]);
            assert_eq!(
                report["value"]["location"],
                json!(native.definition.location)
            );
            assert_eq!(
                report["value"]["source_sha256"],
                json!(native.definition.source_sha256)
            );
            let proposal = call(
                send,
                id + 10,
                "authoring_complete_reference",
                json!({"session":handle,"generation":1,"document":document,"offset":offset}),
            );
            let native = sdk
                .analysis()
                .as_ref()
                .unwrap()
                .complete_editorial_reference(
                    sdk.files()[document].analysis_path(),
                    text,
                    offset,
                    1 << 20,
                )
                .unwrap()
                .unwrap();
            assert_eq!(proposal["value"], json!(native));
            assert_eq!(proposal["generation"], 1);
            assert_eq!(proposal["version"], Value::Null);
            assert_eq!(
                proposal["source_sha256"],
                json!(Sha256Digest::of(text.as_bytes()))
            );
        }
        let child = documents[1]["id"].as_str().unwrap();
        let references = call(
            send,
            20,
            "authoring_references",
            json!({"session":handle,"generation":1,"document":child,"offset":documents[1]["text"].as_str().unwrap().find("collection:child").unwrap(),"include_declaration":false}),
        );
        assert_eq!(references["value"].as_array().unwrap().len(), 2);
        for index in [2, 4] {
            assert!(
                references["value"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|reference| reference["document"] == documents[index]["id"])
            );
        }
        let document = documents[4]["id"].as_str().unwrap();
        let baseline = documents[4]["text"].as_str().unwrap();
        let invalid = baseline.replace("collection:child", "collection:missing");
        assert!(sdk.open(document, 7, invalid.clone()).unwrap());
        call(
            send,
            21,
            "authoring_open_document",
            json!({"session":handle,"generation":1,"document":document,"version":7,"text":invalid}),
        );
        let proposal = call(
            send,
            22,
            "authoring_complete_reference",
            json!({"session":handle,"generation":2,"document":document,"offset":invalid.find("collection:missing").unwrap()}),
        );
        let native = sdk
            .analysis()
            .as_ref()
            .unwrap()
            .complete_editorial_reference(
                sdk.files()[document].analysis_path(),
                &invalid,
                invalid.find("collection:missing").unwrap(),
                1 << 20,
            )
            .unwrap()
            .unwrap();
        assert_eq!(proposal["value"], json!(native));
        assert_eq!(proposal["version"], 7);
        assert_eq!(
            proposal["source_sha256"],
            json!(Sha256Digest::of(invalid.as_bytes()))
        );
        assert!(proposal["value"]["current"].is_null());
        let unresolved = call(
            send,
            23,
            "authoring_definition",
            json!({"session":handle,"generation":2,"document":document,"offset":invalid.find("collection:missing").unwrap()}),
        );
        assert!(
            unresolved["value"].is_null(),
            "no stale native graph definition"
        );
        let choice = proposal["value"]["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .find(|candidate| candidate["declaration"]["identity"] == "collection:child")
            .unwrap();
        let span = &proposal["value"]["location"]["span"];
        let start = span["start"].as_u64().unwrap() as usize;
        let end = span["end"].as_u64().unwrap() as usize;
        assert_eq!(
            &invalid[start..end],
            proposal["value"]["preimage"].as_str().unwrap()
        );
        let repaired = format!(
            "{}{}{}",
            &invalid[..start],
            choice["replacement"].as_str().unwrap(),
            &invalid[end..]
        );
        assert!(sdk.change(document, 8, repaired.clone()).unwrap());
        call(
            send,
            24,
            "authoring_change_document",
            json!({"session":handle,"generation":2,"document":document,"version":8,"text":repaired}),
        );
        assert!(sdk.analysis().as_ref().unwrap().structurally_valid());
        let recovered = call(
            send,
            25,
            "authoring_definition",
            json!({"session":handle,"generation":3,"document":document,"offset":repaired.find("collection:child").unwrap()}),
        );
        assert_eq!(recovered["value"]["document"], documents[1]["id"]);
        let stale = send(
            json!({"jsonrpc":"2.0","id":26,"method":"tools/call","params":{"name":"authoring_complete_reference","arguments":{"session":handle,"generation":2,"document":document,"offset":0}}}),
        );
        assert_eq!(
            stale["result"]["structuredContent"]["code"],
            "authoring_stale_generation"
        );
        assert!(sdk.close(document).unwrap());
        call(
            send,
            27,
            "authoring_close_document",
            json!({"session":handle,"generation":3,"document":document}),
        );
        let restored = call(
            send,
            28,
            "authoring_complete_reference",
            json!({"session":handle,"generation":4,"document":document,"offset":baseline.find("collection:child").unwrap()}),
        );
        assert_eq!(
            restored["source_sha256"],
            json!(Sha256Digest::of(baseline.as_bytes()))
        );
        assert_eq!(restored["version"], Value::Null);
        assert_eq!(
            std::fs::read_to_string(owners[1].path().join("direct.md")).unwrap(),
            baseline
        );
        call(
            send,
            29,
            "authoring_close_session",
            json!({"session":handle,"generation":4}),
        );
    });
}
