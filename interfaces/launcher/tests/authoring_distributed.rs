//! Initialized executable MCP uses the same distributed authoring corpus and custody as SDK.

#[allow(dead_code)]
#[path = "support/mcp.rs"]
mod carrier;

use gazette_sdk::{
    authoring::{
        self,
        session::{Session, Source},
    },
    source::Sha256Digest,
};
use serde_json::{Value, json};
use std::fs;
use tempfile::tempdir;

fn source(value: &Value) -> Source {
    Source {
        id: value["id"].as_str().unwrap().into(),
        path: value["path"].as_str().unwrap().parse().unwrap(),
        text: value["text"].as_str().unwrap().into(),
    }
}

fn call(send: &mut dyn FnMut(Value) -> Value, id: usize, name: &str, arguments: Value) -> Value {
    let response = send(
        json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":arguments}}),
    );
    assert!(response.get("error").is_none(), "{response}");
    assert_eq!(response["result"]["isError"], false, "{response}");
    response["result"]["structuredContent"].clone()
}

#[test]
fn initialized_mcp_preserves_corpus_scale_logical_paths_and_current_sdk_diagnostics() {
    let workspace = tempdir().unwrap();
    let owners = [tempdir().unwrap(), tempdir().unwrap(), tempdir().unwrap()];
    fs::create_dir(workspace.path().join("config")).unwrap();
    let root = "includes=['config/sites.toml']\n";
    let sites = "[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n[[brands]]\nid='brand:one'\nname='Distributed'\n[[brands.publications]]\nsite='site:one'\npublic_base_url='https://one.example/'\n";
    fs::write(workspace.path().join("gazette.toml"), root).unwrap();
    fs::write(workspace.path().join("config/sites.toml"), sites).unwrap();
    let workspace_source = json!({"id":format!("file://{}", workspace.path().join("gazette.toml").display()),"path":"gazette.toml","text":fs::read_to_string(workspace.path().join("gazette.toml")).unwrap()});
    let module = json!({"id":format!("file://{}", workspace.path().join("config/sites.toml").display()),"path":"config/sites.toml","text":fs::read_to_string(workspace.path().join("config/sites.toml")).unwrap()});
    let documents = (0..220).map(|number| {
        let owner = number % owners.len();
        let path = owners[owner].path().join(format!("item-{number}.md"));
        fs::write(&path, format!("+++\n[content]\nid='content:item-{number}'\nsite='site:one'\ntitle='Item {number}'\nlifecycle='draft'\nroute={{path='/blog/item-{number}/'}}\nstructure={{kind='page',family={{kind='article'}}}}\n+++\n# Item {number}\n")).unwrap();
        json!({"id":format!("file://{}",path.display()),"path":format!("owner-{owner}/content/item-{number}.md"),"text":fs::read_to_string(path).unwrap()})
    }).collect::<Vec<_>>();
    let mut native = Session::new_with_workspace_documents(
        source(&workspace_source),
        [source(&module)],
        documents.iter().map(source),
        [],
        Default::default(),
    )
    .unwrap();
    assert!(native.analysis().as_ref().unwrap().structurally_valid());
    let arguments =
        json!({"workspace":workspace_source,"workspace_documents":[module],"documents":documents});
    assert!(
        serde_json::to_vec(&arguments).unwrap().len() < 1 << 20,
        "finite supplied-buffer mode, not expanded framing limits"
    );
    carrier::initialized_interact("2026-07-28", |initialized, send| {
        assert_eq!(initialized["result"]["protocolVersion"], "2026-07-28");
        let created = call(send, 1, "authoring_start", arguments.clone());
        let handle = created["session"].as_str().unwrap();
        assert_eq!(created["generation"], 1);
        let status = call(send, 2, "authoring_status", json!({"session":handle}));
        assert_eq!(status["structurally_valid"], true);
        assert_eq!(
            status["files"].as_array().unwrap().len(),
            native.files().len()
        );
        let document = arguments["documents"][0]["id"].as_str().unwrap();
        let logical = arguments["documents"][0]["path"].as_str().unwrap();
        let baseline = arguments["documents"][0]["text"].as_str().unwrap();
        let conflict = baseline.replace("id='content:item-0'", "id='content:item-1'");
        assert!(native.open(document, 7, conflict.clone()).unwrap());
        let opened = call(
            send,
            3,
            "authoring_open_document",
            json!({"session":handle,"generation":1,"document":document,"version":7,"text":conflict}),
        );
        assert_eq!(opened["generation"], native.generation());
        let current = call(
            send,
            4,
            "authoring_diagnostics",
            json!({"session":handle,"generation":2,"document":document}),
        );
        let expected = authoring::diagnostics(
            native.analysis().as_ref().unwrap(),
            native.files()[document].path(),
            native.files()[document].text(),
            1 << 20,
        )
        .unwrap();
        assert_eq!(current["value"], json!(expected));
        assert_eq!(current["path"], logical);
        assert_eq!(current["version"], 7);
        assert_eq!(
            current["source_sha256"],
            json!(Sha256Digest::of(conflict.as_bytes()))
        );
        let stale = send(
            json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"authoring_change_document","arguments":{"session":handle,"generation":1,"document":document,"version":8,"text":baseline}}}),
        );
        assert_eq!(
            stale["result"]["structuredContent"]["code"],
            "authoring_stale_generation"
        );
        let invalid = call(send, 6, "authoring_status", json!({"session":handle}));
        assert_eq!(invalid["structurally_valid"], false);
        assert_eq!(invalid["generation"], 2);
        fs::write(
            owners[0].path().join("item-0.md"),
            "later unselected disk state",
        )
        .unwrap();
        assert!(native.close(document).unwrap());
        let closed = call(
            send,
            7,
            "authoring_close_document",
            json!({"session":handle,"generation":2,"document":document}),
        );
        assert_eq!(closed["generation"], native.generation());
        let restored = call(send, 8, "authoring_status", json!({"session":handle}));
        assert_eq!(restored["structurally_valid"], true);
        let selected = restored["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|file| file["document"] == document)
            .unwrap();
        assert_eq!(
            selected["source_sha256"],
            json!(Sha256Digest::of(baseline.as_bytes()))
        );
        assert_eq!(selected["version"], Value::Null);
        assert!(native.analysis().as_ref().unwrap().structurally_valid());
        call(
            send,
            9,
            "authoring_close_session",
            json!({"session":handle,"generation":3}),
        );
    });
}
