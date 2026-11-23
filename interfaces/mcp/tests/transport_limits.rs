//! Public finite transport budgets exercise the real service and unchanged SDK.

use gazette_mcp::{TransportLimits, TransportLimitsError, serve, serve_with_transport_limits};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
}
fn request(id: u64, name: &str, arguments: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{
        "name":name,"arguments":arguments,
        "_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28",
            "io.modelcontextprotocol/clientCapabilities":{}}
    }})
}
async fn response<R: tokio::io::AsyncRead + Unpin>(reader: &mut BufReader<R>) -> Value {
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut line))
        .await
        .expect("bounded native response")
        .expect("carrier response");
    assert!(!line.is_empty(), "expected native JSON-RPC result");
    serde_json::from_str(&line).unwrap()
}

#[test]
fn public_budgets_preserve_defaults_and_validate_both_finite_ranges() {
    let defaults = TransportLimits::default();
    assert_eq!(defaults.max_frame_bytes(), 1 << 20);
    assert_eq!(defaults.max_response_bytes(), 1 << 18);
    for frame in [0, 4095, (16 << 20) + 1, usize::MAX] {
        assert_eq!(
            TransportLimits::new(frame, 1 << 18),
            Err(TransportLimitsError::FrameBytes)
        );
    }
    for output in [0, 1023, (16 << 20) + 1, usize::MAX] {
        assert_eq!(
            TransportLimits::new(1 << 20, output),
            Err(TransportLimitsError::ResponseBytes)
        );
    }
    for (frame, output) in [(4096, 1024), (16 << 20, 16 << 20)] {
        let limits = TransportLimits::new(frame, output).unwrap();
        assert_eq!(limits.max_frame_bytes(), frame);
        assert_eq!(limits.max_response_bytes(), output);
    }
}

#[test]
fn default_refuses_complete_oversize_selection_and_explicit_budget_admits_same_native_set() {
    let workspace = "principals=[{id='person:one',kind='person'}]\nsites=[{id='site:one',publisher='person:one',deployable=false}]\n";
    let text = format!("# Native selected heading\n\n{}\n", "word ".repeat(6400));
    let documents = (0..36)
        .map(|index| {
            json!({"id":format!("document-{index}"),"path":format!("owner/{index}.md"),"text":text})
        })
        .collect::<Vec<_>>();
    // Prove this is an admitted complete native selection, not a large padding
    // field that merely survives JSON parsing or a raised analysis ceiling.
    let native = gazette_cli::sdk::authoring::session::Session::new(
        gazette_cli::sdk::authoring::session::Source {
            id: "workspace".into(),
            path: "workspace.toml".parse().unwrap(),
            text: workspace.into(),
        },
        (0..36).map(|index| gazette_cli::sdk::authoring::session::Source {
            id: format!("document-{index}"),
            path: format!("owner/{index}.md").parse().unwrap(),
            text: text.clone(),
        }),
        Default::default(),
    )
    .expect("unchanged native analysis budgets admit the intact selection");
    assert!(native.analysis().is_ok());
    let start = request(
        2,
        "authoring_start",
        json!({"workspace":{"id":"workspace","path":"workspace.toml","text":workspace},"documents":documents}),
    );
    let bytes = format!("{start}\n");
    assert!(bytes.len() - 1 > TransportLimits::default().max_frame_bytes());
    assert!(bytes.len() < 2 << 20);
    runtime().block_on(async {
        let (mut client, carrier) = tokio::io::duplex(2 << 20);
        let (reader, writer) = tokio::io::split(carrier);
        let service = tokio::spawn(serve(reader, writer));
        let _ = client.write_all(bytes.as_bytes()).await;
        client.shutdown().await.unwrap();
        assert!(
            tokio::time::timeout(Duration::from_secs(10), service)
                .await
                .unwrap()
                .unwrap()
                .is_err(),
            "default rejects the complete oversized frame, never partially selects"
        );

        let (client, carrier) = tokio::io::duplex(2 << 20);
        let (reader, writer) = tokio::io::split(carrier);
        let service = tokio::spawn(serve_with_transport_limits(
            reader,
            writer,
            TransportLimits::new(2 << 20, 1 << 18).unwrap(),
        ));
        let (reader, mut writer) = tokio::io::split(client);
        let mut reader = BufReader::new(reader);
        writer.write_all(bytes.as_bytes()).await.unwrap();
        let started = response(&mut reader).await;
        assert_eq!(started["result"]["isError"], false, "{started}");
        let handle = started["result"]["structuredContent"]["session"]
            .as_str()
            .unwrap();
        writer
            .write_all(
                format!(
                    "{}\n",
                    request(3, "authoring_status", json!({"session":handle}))
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        let status = response(&mut reader).await;
        let files = status["result"]["structuredContent"]["files"]
            .as_array()
            .unwrap();
        assert_eq!(
            files.len(),
            37,
            "complete workspace and all 36 sources retained"
        );
        let digest = json!(gazette_cli::sdk::source::Sha256Digest::of(text.as_bytes()));
        for index in 0..36 {
            assert!(
                files
                    .iter()
                    .any(|file| file["document"] == format!("document-{index}")
                        && file["source_sha256"] == digest)
            );
        }
        writer
            .write_all(
                format!(
                    "{}\n",
                    request(
                        4,
                        "authoring_close_session",
                        json!({"session":handle,"generation":1})
                    )
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        let closed = response(&mut reader).await;
        assert_eq!(closed["result"]["isError"], false, "{closed}");
        writer.shutdown().await.unwrap();
        drop(writer);
        drop(reader);
        tokio::time::timeout(Duration::from_secs(10), service)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    });
}

#[test]
fn explicit_response_budget_changes_encoding_only_and_next_request_remains_healthy() {
    let principals = (0..32)
        .map(|index| format!("{{id='person:{index}',kind='person'}}"))
        .collect::<Vec<_>>()
        .join(",");
    let source = format!(
        "principals=[{principals}]\nsites=[{{id='site:one',publisher='person:0',deployable=false}}]\n"
    );
    let native = gazette_cli::sdk::workspace::compile(&source, Default::default()).unwrap();
    assert!(serde_json::to_vec(&native).unwrap().len() > 1024);
    runtime().block_on(async {
        for maximum in [1024, 1 << 18] {
            let (client, carrier) = tokio::io::duplex(1 << 20);
            let (reader, writer) = tokio::io::split(carrier);
            let service = tokio::spawn(serve_with_transport_limits(
                reader,
                writer,
                TransportLimits::new(1 << 20, maximum).unwrap(),
            ));
            let (reader, mut writer) = tokio::io::split(client);
            let mut reader = BufReader::new(reader);
            writer
                .write_all(
                    format!(
                        "{}\n",
                        request(1, "check_workspace", json!({"source":source}))
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            let checked = response(&mut reader).await;
            if maximum == 1024 {
                assert_eq!(
                    checked["result"]["structuredContent"]["code"],
                    "mcp_output_limit"
                );
                assert_eq!(checked["result"]["isError"], true);
            } else {
                assert_eq!(checked["result"]["structuredContent"], json!(native));
                assert_eq!(checked["result"]["isError"], false);
            }
            writer
                .write_all(
                    format!(
                        "{}\n",
                        request(2, "check_workspace", json!({"source":"sites=[]"}))
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            let next = response(&mut reader).await;
            assert_eq!(next["id"], 2);
            assert!(
                next["result"]["structuredContent"]["code"].is_string(),
                "{next}"
            );
            writer.shutdown().await.unwrap();
            drop(writer);
            drop(reader);
            tokio::time::timeout(Duration::from_secs(10), service)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
        }
    });
}
