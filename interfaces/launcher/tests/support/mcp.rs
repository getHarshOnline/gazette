//! One bounded native MCP executable carrier shared by interface and publication fixtures.

use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

pub(crate) struct Process(pub(crate) Child);

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[allow(dead_code)] // Some integration fixtures use only initialized sessions.
pub(crate) fn exchange(requests: Vec<Value>) -> Vec<Value> {
    exchange_with_call_timeout(requests, Duration::from_secs(10))
}

#[allow(dead_code)] // Shared by fixtures that select different session modes.
pub(crate) fn exchange_with_call_timeout(
    requests: Vec<Value>,
    call_timeout: Duration,
) -> Vec<Value> {
    interact_with_call_timeout(call_timeout, |send| {
        requests.into_iter().map(send).collect()
    })
}

/// Keep one executable MCP process alive while later requests consume opaque
/// handles returned by earlier calls.
#[allow(dead_code)] // This shared module is compiled separately by fixtures without sessions.
pub(crate) fn interact<T>(action: impl FnOnce(&mut dyn FnMut(Value) -> Value) -> T) -> T {
    interact_with_call_timeout(Duration::from_secs(10), action)
}

/// Exercise native protocol negotiation and notifications without substituting
/// stateless metadata for the initialized session on later requests.
#[allow(dead_code)] // Shared by separate executable fixtures.
pub(crate) fn initialized_interact<T>(
    version: &str,
    action: impl FnOnce(&Value, &mut dyn FnMut(Value) -> Value) -> T,
) -> T {
    initialized_interact_with_call_timeout(version, Duration::from_secs(10), action)
}

/// Keep protocol negotiation while allowing a fixture's explicit bounded provider budget.
#[allow(dead_code)] // Only provider-shaped fixtures need a longer tool-call budget.
pub(crate) fn initialized_interact_with_call_timeout<T>(
    version: &str,
    call_timeout: Duration,
    action: impl FnOnce(&Value, &mut dyn FnMut(Value) -> Value) -> T,
) -> T {
    assert!(
        !call_timeout.is_zero(),
        "tool-call deadline must be bounded and nonzero"
    );
    interact_with_lifecycle(call_timeout, Some(version), None, |initialized, send| {
        action(&initialized.expect("initialize response"), send)
    })
}

/// Exercise the real launcher with finite caller-selected transport budgets.
#[allow(dead_code)] // Only complete retained-corpus fixtures select larger frames.
pub(crate) fn initialized_interact_with_transport_limits<T>(
    version: &str,
    call_timeout: Duration,
    limits: gazette_mcp::TransportLimits,
    action: impl FnOnce(&Value, &mut dyn FnMut(Value) -> Value) -> T,
) -> T {
    assert!(!call_timeout.is_zero());
    interact_with_lifecycle(
        call_timeout,
        Some(version),
        Some(limits),
        |initialized, send| action(&initialized.expect("initialize response"), send),
    )
}

fn interact_with_call_timeout<T>(
    call_timeout: Duration,
    action: impl FnOnce(&mut dyn FnMut(Value) -> Value) -> T,
) -> T {
    interact_with_lifecycle(call_timeout, None, None, |_, send| action(send))
}

fn interact_with_lifecycle<T>(
    call_timeout: Duration,
    version: Option<&str>,
    limits: Option<gazette_mcp::TransportLimits>,
    action: impl FnOnce(Option<Value>, &mut dyn FnMut(Value) -> Value) -> T,
) -> T {
    let cwd = tempfile::tempdir().expect("isolated cwd");
    let mut command = Command::new(env!("CARGO_BIN_EXE_gazette"));
    command.arg("mcp");
    if let Some(limits) = limits {
        command.args([
            "--max-frame-bytes",
            &limits.max_frame_bytes().to_string(),
            "--max-response-bytes",
            &limits.max_response_bytes().to_string(),
        ]);
    }
    let mut child = Process(
        command
            .current_dir(cwd.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("Gazette MCP"),
    );
    let mut input = child.0.stdin.take().expect("stdin");
    let output = child.0.stdout.take().expect("stdout");
    let (sender, receiver) = mpsc::sync_channel(1);
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(output).lines() {
            if sender.send(line).is_err() {
                break;
            }
        }
    });
    let initialized = version.map(|version| {
        let request = json!({
            "jsonrpc":"2.0", "id":"launcher-initialize", "method":"initialize",
            "params":{
                "protocolVersion":version, "capabilities":{},
                "clientInfo":{"name":"gazette-launcher-test","version":"1"}
            }
        });
        serde_json::to_writer(&mut input, &request).expect("initialize request");
        input.write_all(b"\n").expect("initialize frame");
        input.flush().expect("initialize flush");
        let line = receiver
            .recv_timeout(Duration::from_secs(10))
            .expect("bounded initialize response")
            .expect("initialize response line");
        let response: Value = serde_json::from_str(&line).expect("initialize JSON");
        assert_eq!(response["id"], request["id"]);
        assert!(response.get("error").is_none(), "{response}");
        assert_eq!(response["result"]["protocolVersion"], version);
        serde_json::to_writer(
            &mut input,
            &json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        )
        .expect("initialized notification");
        input.write_all(b"\n").expect("notification frame");
        input.flush().expect("notification flush");
        // Notifications have no response; the next response belongs to the
        // caller's request and is checked against that request's exact ID.
        response
    });
    let mut send = |mut request: Value| {
        if version.is_none() {
            request["params"]["_meta"] = json!({
                "io.modelcontextprotocol/protocolVersion":"2026-07-28",
                "io.modelcontextprotocol/clientCapabilities":{}
            });
        } else {
            assert!(
                request["params"].get("_meta").is_none(),
                "session, not inline negotiation"
            );
        }
        serde_json::to_writer(&mut input, &request).expect("request");
        input.write_all(b"\n").expect("frame");
        input.flush().expect("flush");
        let timeout = if request["method"] == "tools/call" {
            call_timeout
        } else {
            Duration::from_secs(10)
        };
        let line = receiver
            .recv_timeout(timeout)
            .unwrap_or_else(|error| panic!("bounded {} response: {error}", request["method"]))
            .expect("response line");
        let mut aggregate_bytes = line.len();
        let mut response: Value = serde_json::from_str(&line).expect("only JSON on stdout");
        assert_eq!(response["id"], request["id"]);
        if request["method"] == "tools/list"
            && request["params"].get("cursor").is_none()
            && response.get("error").is_none()
        {
            let mut tools = response["result"]["tools"]
                .as_array()
                .expect("tool page")
                .clone();
            let mut cursor = response["result"]["nextCursor"].as_str().map(str::to_owned);
            let mut seen = std::collections::BTreeSet::new();
            while let Some(next) = cursor {
                assert!(seen.insert(next.clone()), "tool cursor loop");
                assert!(seen.len() <= 128, "tool page ceiling");
                request["params"]["cursor"] = json!(next);
                serde_json::to_writer(&mut input, &request).expect("paged request");
                input.write_all(b"\n").expect("paged frame");
                input.flush().expect("paged flush");
                let line = receiver
                    .recv_timeout(Duration::from_secs(10))
                    .expect("bounded paged tools/list response")
                    .expect("paged response line");
                aggregate_bytes += line.len();
                assert!(
                    aggregate_bytes <= 16 * 1024 * 1024,
                    "catalog aggregate ceiling"
                );
                let page: Value = serde_json::from_str(&line).expect("paged JSON response");
                assert_eq!(page["id"], request["id"]);
                assert!(page.get("error").is_none(), "{page}");
                assert_eq!(page["result"]["resultType"], "complete");
                assert_eq!(page["result"]["ttlMs"], 0);
                assert_eq!(page["result"]["cacheScope"], "private");
                let page_tools = page["result"]["tools"].as_array().expect("tool page");
                assert!(!page_tools.is_empty());
                assert!(page_tools.len() <= 8);
                tools.extend(page_tools.clone());
                assert!(tools.len() <= 1024, "catalog tool ceiling");
                cursor = page["result"]["nextCursor"].as_str().map(str::to_owned);
            }
            response["result"]["tools"] = Value::Array(tools);
            response["result"]
                .as_object_mut()
                .expect("result")
                .remove("nextCursor");
        }
        response
    };
    let result = action(initialized, &mut send);
    drop(input);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.0.try_wait().expect("process status") {
            assert!(status.success(), "MCP failed: {status}");
            break;
        }
        assert!(Instant::now() < deadline, "MCP did not exit after EOF");
        std::thread::sleep(Duration::from_millis(10));
    }
    reader.join().expect("stdout reader");
    let mut stderr = String::new();
    child
        .0
        .stderr
        .take()
        .expect("stderr")
        .read_to_string(&mut stderr)
        .expect("stderr bytes");
    assert!(stderr.is_empty(), "{stderr}");
    assert!(std::fs::read_dir(cwd.path()).expect("cwd").next().is_none());
    result
}
