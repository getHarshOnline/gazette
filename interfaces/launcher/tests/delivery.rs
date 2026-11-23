//! Real pinned curl and loopback HTTP through the CLI, MCP and shared SDK.

use gazette_sdk::{delivery, source};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::PathBuf,
    process::{Command, Output, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

#[path = "support/mcp.rs"]
mod carrier;

use gazette_test_support as native;

#[path = "delivery/javascript.rs"]
mod javascript;

struct Server {
    base: String,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Server {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("owned loopback listener");
        let base = format!("http://{}/mount/", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let log = Arc::clone(&requests);
        let stopping = Arc::clone(&stop);
        let thread = thread::spawn(move || {
            while !stopping.load(Ordering::SeqCst) {
                let (mut socket, _) = match listener.accept() {
                    Ok(connection) => connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(error) => panic!("fixture accept failed: {error}"),
                };
                socket.set_nonblocking(false).unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                socket
                    .set_write_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut header = Vec::new();
                while !header.ends_with(b"\r\n\r\n") {
                    assert!(header.len() < 8192, "bounded native request");
                    let mut byte = [0];
                    if socket.read(&mut byte).unwrap() == 0 {
                        break;
                    }
                    header.push(byte[0]);
                }
                let request = String::from_utf8(header).unwrap();
                let line = request.lines().next().expect("HTTP request line");
                let mut words = line.split_whitespace();
                assert_eq!(words.next(), Some("GET"));
                let route = words.next().expect("exact requested route");
                log.lock().unwrap().push(route.to_owned());
                if route == "/mount/unavailable.txt" {
                    continue;
                }
                let (status, body) = match route {
                    "/mount/" => (200, "<p>retained output</p>"),
                    "/mount/different.txt" => (200, "PRIVATE_DIFFERENT_BODY"),
                    "/mount/missing.txt" => (404, "PRIVATE_NOT_FOUND_BODY"),
                    _ => panic!("unselected HTTP route: {route}"),
                };
                write!(socket, "HTTP/1.1 {status} Result\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        Self {
            base,
            requests,
            stop,
            thread: Some(thread),
        }
    }

    fn count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.thread
            .take()
            .unwrap()
            .join()
            .expect("reaped owned HTTP fixture");
    }
}

fn request(server: &Server, routes: &[&str]) -> (tempfile::TempDir, Value) {
    let directory = tempfile::tempdir().expect("private fixture");
    let output = directory.path().join("output");
    fs::create_dir(&output).unwrap();
    for (file, body) in [
        ("index.html", "<p>retained output</p>"),
        ("different.txt", "retained different"),
        ("missing.txt", "retained missing"),
        ("unavailable.txt", "retained unavailable"),
    ] {
        fs::write(output.join(file), body).unwrap();
    }
    let identity = source::inspect(&output, None, Default::default())
        .unwrap()
        .identity();
    let program = native::curl().to_path_buf();
    let native = fs::read(&program).expect("exact existing curl bytes");
    let request = json!({
        "tree":output,"tree_identity":identity,"source_limits":source::Limits::default(),
        "curl":{"program":program,"directory":directory.path(),"executable":{"bytes":native.len(),"sha256":source::Sha256Digest::of(&native)}},
        "selection":{"base_url":server.base,"routes":routes},
        "options":{"http":{"allow_read":true,"allow_execute":true,"allow_network":true,"response_bytes":4096,"timeout_ms":5000},"max_routes":8}
    });
    (directory, request)
}

fn cli(input: &str, extra: &[&str]) -> Output {
    let mut child = carrier::Process(
        Command::new(env!("CARGO_BIN_EXE_gazette"))
            .arg("observe-delivery")
            .args(extra)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("real Gazette CLI"),
    );
    child
        .0
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    child
        .0
        .stdout
        .take()
        .unwrap()
        .read_to_end(&mut stdout)
        .unwrap();
    child
        .0
        .stderr
        .take()
        .unwrap()
        .read_to_end(&mut stderr)
        .unwrap();
    Output {
        status: child.0.wait().unwrap(),
        stdout,
        stderr,
    }
}

fn mcp(input: &str) -> Value {
    carrier::exchange(vec![json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"observe_delivery","arguments":{"request_json":input}}})]).remove(0)
}

#[test]
fn native_served_route_reports_match_across_sdk_cli_and_mcp_without_silent_shrink() {
    let server = Server::new();
    let (_directory, request) = request(
        &server,
        &["/", "/different.txt", "/missing.txt", "/unavailable.txt"],
    );
    let input = request.to_string();
    let typed = delivery::Request::parse(input.as_bytes(), 1 << 20).unwrap();
    let expected = delivery::observe_retained(&typed).expect("complete native report");
    assert!(!expected.matches_selected_routes());
    let expected = json!(expected);
    let direct = cli(&input, &[]);
    assert_eq!(direct.status.code(), Some(1));
    assert!(
        direct.stderr.is_empty(),
        "route failures retain the full structured report"
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&direct.stdout).unwrap(),
        expected
    );
    let framed = mcp(&input);
    assert_eq!(framed["result"]["isError"], true);
    assert_eq!(framed["result"]["structuredContent"], expected);
    let observations = expected["observations"].as_array().unwrap();
    assert_eq!(observations.len(), 4);
    assert_eq!(
        observations
            .iter()
            .map(|row| row["outcome"]["status"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["matches", "differs", "http-failure", "unavailable"]
    );
    assert_eq!(observations[0]["output"], "index.html");
    assert_eq!(observations[2]["outcome"]["http_status"], 404);
    assert_eq!(expected["output"], request["tree_identity"]);
    assert!(expected.get("settlement").is_none());
    assert!(!expected.to_string().contains("PRIVATE_"));
    assert_eq!(
        server.count(),
        12,
        "one GET per selected route per caller, without retries"
    );
    assert_eq!(
        source::inspect(&typed.tree, None, typed.source_limits)
            .unwrap()
            .identity(),
        typed.tree_identity
    );

    let mut matches = request;
    matches["selection"]["routes"] = json!(["/"]);
    let direct = cli(&matches.to_string(), &[]);
    assert!(direct.status.success());
    let framed = mcp(&matches.to_string());
    assert_ne!(framed["result"]["isError"], true);
    assert_eq!(
        framed["result"]["structuredContent"],
        serde_json::from_slice::<Value>(&direct.stdout).unwrap()
    );
    assert_eq!(server.count(), 14);
}

#[test]
fn permission_duplicate_pin_and_bound_refusals_make_no_http_requests() {
    let server = Server::new();
    let (_directory, request) = request(&server, &["/"]);
    let mut refused = Vec::new();
    for permission in ["allow_read", "allow_execute", "allow_network"] {
        let mut denied = request.clone();
        denied["options"]["http"][permission] = json!(false);
        refused.push((denied.to_string(), "delivery_request"));
    }
    let mut stale = request.clone();
    stale["tree_identity"]["sha256"] = json!("0".repeat(64));
    refused.push((stale.to_string(), "delivery_capture"));
    let mut positional = request.clone();
    positional["selection"] = json!([server.base, ["/"]]);
    refused.push((positional.to_string(), "delivery_input"));
    refused.push((
        request.to_string().replace(
            "\"max_routes\":8",
            "\"max_routes\":8,\"max\\u005froutes\":7",
        ),
        "delivery_input",
    ));
    let mut no_bounds = request.clone();
    no_bounds["options"]["http"]["response_bytes"] = json!(0);
    refused.push((no_bounds.to_string(), "delivery_request"));
    for (input, code) in refused {
        let direct = cli(&input, &[]);
        assert_eq!(direct.status.code(), Some(1));
        assert!(direct.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&direct.stderr).contains(code),
            "{direct:?}"
        );
        let framed = mcp(&input);
        assert_eq!(framed["result"]["isError"], true);
        assert_eq!(framed["result"]["structuredContent"]["code"], code);
    }
    let tiny = cli(&request.to_string(), &["--max-input-bytes", "1"]);
    assert!(tiny.stdout.is_empty());
    assert!(String::from_utf8_lossy(&tiny.stderr).contains("delivery_input_limit"));
    assert_eq!(server.count(), 0);
}

#[test]
fn delivery_discovery_exposes_the_shared_request_and_complete_report() {
    let replies = carrier::exchange(vec![
        json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}),
    ]);
    let tool = replies[0]["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "observe_delivery")
        .expect("discoverable operation");
    let inner = &tool["inputSchema"]["properties"]["request_json"];
    assert_eq!(inner["contentMediaType"], "application/json");
    assert_eq!(
        inner["contentSchema"],
        serde_json::to_value(delivery::request_schema()).unwrap()
    );
    let retained = tempfile::tempdir().expect("native report identity fixture");
    let report = delivery::Report {
        output: source::inspect(retained.path(), None, Default::default())
            .unwrap()
            .identity(),
        base_url: "https://example.invalid/".parse().unwrap(),
        observations: Vec::new(),
    };
    let validator =
        jsonschema::validator_for(&tool["outputSchema"]).expect("native complete output schema");
    assert!(validator.is_valid(&json!(report)));
    assert_eq!(tool["annotations"]["readOnlyHint"], true);
    assert_eq!(tool["annotations"]["openWorldHint"], true);
    let help = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args(["observe-delivery", "--help"])
        .output()
        .unwrap();
    assert!(help.status.success());
    let text = String::from_utf8(help.stdout).unwrap();
    assert!(text.contains("options.http"));
    assert!(text.contains("--max-input-bytes") && text.contains("--max-output-bytes"));
    assert!(
        !text.contains("--allow-network"),
        "native permissions have one request owner"
    );
}
