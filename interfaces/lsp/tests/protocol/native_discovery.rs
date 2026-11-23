//! Automatic membership uses native configuration while the protocol remains live through typos.

use super::*;

fn discovery(root: &std::path::Path) -> Value {
    let uri = url::Url::from_directory_path(root).unwrap().to_string();
    request(
        1,
        "initialize",
        json!({"capabilities":{},"rootUri":uri,"workspaceFolders":[{"name":"test","uri":uri}],
        "initializationOptions":{"native_selection":{"allow_read":true,"discover":true,"workspace":"gazette.toml"}}}),
    )
}

#[test]
fn malformed_native_root_is_repaired_and_reselected_without_restarting_the_server() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("pages")).unwrap();
    let broken = "[[principals\n";
    std::fs::write(root.path().join("gazette.toml"), broken).unwrap();
    std::fs::write(root.path().join("pages/one.md"), "# One").unwrap();
    std::fs::File::create(root.path().join("pages/film.mp4"))
        .unwrap()
        .set_len(64_u64 << 30)
        .unwrap();
    let config = format!(
        "{WORKSPACE}[[sources]]\nid='source:pages'\nlocation={{kind='filesystem',root='pages'}}\n[[brands]]\nid='brand:one'\nname='Example'\n[[brands.publications]]\nsite='site:one'\nauthored=[{{source='source:pages',target='content'}}]\n"
    );
    let root_uri = url::Url::from_directory_path(root.path())
        .unwrap()
        .to_string();
    let uri = url::Url::from_file_path(root.path().join("gazette.toml"))
        .unwrap()
        .to_string();
    let page_uri = url::Url::from_file_path(root.path().join("pages/one.md"))
        .unwrap()
        .to_string();
    let init = request(
        1,
        "initialize",
        json!({"capabilities":{},"rootUri":root_uri,"workspaceFolders":[{"name":"test","uri":root_uri}],
        "initializationOptions":{"native_selection":{"allow_read":true,"discover":true,"workspace":"gazette.toml"}}}),
    );
    let (result, messages) = selected_session(
        init,
        [
            open(&uri, 1, broken),
            change(&uri, 2, &config),
            request(10, "gazette/editorSnapshot", json!({})),
            open(&page_uri, 7, "# Unsaved"),
            change(&uri, 3, broken),
            request(11, "gazette/editorSnapshot", json!({})),
            change(&uri, 4, WORKSPACE),
            request(12, "gazette/editorSnapshot", json!({})),
        ],
        Limits::default(),
    );
    assert!(result.is_ok(), "{result:?}");
    let reply = |id| &messages.iter().find(|message| message["id"] == id).unwrap()["result"];
    assert_eq!(reply(10)["sources"].as_array().unwrap().len(), 2);
    assert_eq!(
        reply(10)["asset_observations"].as_object().unwrap().len(),
        1
    );
    assert!(
        reply(11)["sources"]
            .as_array()
            .unwrap()
            .iter()
            .any(|source| source["id"] == page_uri && source["version"] == 7)
    );
    assert_eq!(reply(12)["sources"].as_array().unwrap().len(), 1);
    assert!(reply(10)["generation"].as_u64().unwrap() < reply(11)["generation"].as_u64().unwrap());
    assert!(messages.iter().any(
        |message| message["method"] == "textDocument/publishDiagnostics"
            && message["params"]["uri"] == page_uri
            && message["params"]["diagnostics"] == json!([])
    ));
    assert_eq!(
        std::fs::read_to_string(root.path().join("gazette.toml")).unwrap(),
        broken
    );
}

#[test]
fn unavailable_native_sources_report_once_without_exhausting_diagnostics_or_ending_the_session() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("pages")).unwrap();
    let config = format!(
        "{WORKSPACE}[[sources]]\nid='source:pages'\nlocation={{kind='filesystem',root='pages'}}\n[[brands]]\nid='brand:one'\nname='Example'\n[[brands.publications]]\nsite='site:one'\nauthored=[{{source='source:pages',target='content'}}]\n"
    );
    std::fs::write(root.path().join("gazette.toml"), &config).unwrap();
    for index in 0..3 {
        std::fs::write(
            root.path().join(format!("pages/{index}.md")),
            content(
                &format!("content:page{index}"),
                "",
                "kind='page'\nfamily={kind='article'}",
            ),
        )
        .unwrap();
    }
    let uri = url::Url::from_file_path(root.path().join("gazette.toml"))
        .unwrap()
        .to_string();
    let (result, messages) = selected_session(
        discovery(root.path()),
        [
            open(&uri, 1, &config),
            change(&uri, 2, &config.replace("root='pages'", "root='absent'")),
            request(20, "gazette/editorSnapshot", json!({})),
        ],
        Limits {
            diagnostics: 2,
            ..Limits::default()
        },
    );
    assert!(result.is_ok(), "{result:?}");
    let failures: Vec<_> = messages
        .iter()
        .filter(|message| message["method"] == "textDocument/publishDiagnostics")
        .filter(|message| {
            message["params"]["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["code"] == "authoring_sources_unavailable")
        })
        .collect();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0]["params"]["uri"], uri);
    assert_eq!(
        messages.iter().find(|message| message["id"] == 20).unwrap()["result"]["sources"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
}

#[test]
fn source_query_admits_a_new_file_by_path_before_the_client_sends_its_text() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("pages")).unwrap();
    let config = format!(
        "{WORKSPACE}[[sources]]\nid='source:pages'\nlocation={{kind='filesystem',root='pages'}}\n[[brands]]\nid='brand:one'\nname='Example'\n[[brands.publications]]\nsite='site:one'\nauthored=[{{source='source:pages',target='content'}}]\n"
    );
    std::fs::write(root.path().join("gazette.toml"), &config).unwrap();
    let path = root.path().join("pages/new.md");
    let uri = url::Url::from_file_path(&path).unwrap().to_string();
    struct Arrival {
        path: std::path::PathBuf,
        created: bool,
        bytes: Vec<u8>,
    }
    impl Write for Arrival {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if !self.created {
                std::fs::write(&self.path, "# Newly created")?;
                self.created = true;
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut output = Arrival {
        path,
        created: false,
        bytes: vec![],
    };
    let input = stream(&[
        discovery(root.path()),
        notification("initialized", json!({})),
        request(10, "gazette/selectSource", json!({"uri":uri})),
        open(&uri, 8, "# Unsaved new page"),
        request(11, "gazette/editorSnapshot", json!({})),
        request(90, "shutdown", Value::Null),
        notification("exit", Value::Null),
    ]);
    serve(Cursor::new(input), &mut output, Limits::default()).unwrap();
    let messages = decode(&output.bytes);
    assert_eq!(
        messages.iter().find(|message| message["id"] == 10).unwrap()["result"]["selected"],
        true
    );
    assert!(
        messages.iter().find(|message| message["id"] == 11).unwrap()["result"]["sources"]
            .as_array()
            .unwrap()
            .iter()
            .any(|source| source["id"] == uri && source["version"] == 8)
    );
}
