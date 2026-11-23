//! Actual MCP preview lifetimes, HTTP parity and carrier-loss socket cleanup.

use gazette_sdk::{build, preview, source};
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    process::{ChildStdin, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    thread,
    time::{Duration, Instant},
};

#[path = "support/mcp.rs"]
mod carrier;
#[path = "preview_mcp/support.rs"]
mod support;
use support::*;

struct Client {
    process: carrier::Process,
    input: Option<ChildStdin>,
    responses: Receiver<std::io::Result<String>>,
    abandon: Arc<AtomicBool>,
    reader: Option<thread::JoinHandle<()>>,
    next: u64,
}

#[test]
fn production_preview_mcp_plans_and_serves_the_same_fresh_request() {
    let fixture = Fixture::new();
    let mut request = fixture.request(false);
    let object = request.as_object_mut().expect("request object");
    object.remove("local_urls");
    object.remove("mode");
    object.insert("registry".into(), json!("unconfigured"));
    object.insert("artifact".into(), json!("fresh_render"));
    let mut client = Client::start();
    let plan = client.call("plan_production_preview", request.clone());
    assert_eq!(plan["status"], "production_linked_no_push_noncertified");
    assert_eq!(plan["registry"], "unconfigured");
    assert_eq!(plan["artifact"], "fresh_render");
    assert_eq!(plan["effects"]["network"]["outbound"], false);
    assert_eq!(plan["effects"]["git"]["mutation"], false);
    assert_eq!(plan["effects"]["release_authority_consumption"], false);

    let started = client.call("start_production_preview", request);
    let handle = started["session"].as_str().expect("handle").to_owned();
    let ready = client.wait(&handle, |state| state["latest"].is_object());
    let state = &ready["snapshot"]["latest"];
    assert_eq!(state["status"], "local-production-linked-noncertified");
    assert_eq!(state["input_mode"], "pinned");
    let sockets = SITES.map(|site| address(state, site));
    for (site, socket) in SITES.into_iter().zip(sockets) {
        let mount = if site == "site:one" { "/mount/" } else { "/" };
        let file = if site == "site:one" {
            "guide/index.html"
        } else {
            "elsewhere/index.html"
        };
        assert!(!get(socket, &format!("{mount}{file}")).is_empty());
    }
    client.stop(&handle);
    released(&sockets);
    drop(client.input.take());
    client.finish(true);
}

impl Client {
    fn start() -> Self {
        let mut process = carrier::Process(
            Command::new(env!("CARGO_BIN_EXE_gazette"))
                .arg("mcp")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("real MCP process"),
        );
        let input = process.0.stdin.take();
        let output = process.0.stdout.take().expect("stdout");
        let (sender, responses) = mpsc::sync_channel(1);
        let abandon = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&abandon);
        let reader = thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                if flag.load(Ordering::Acquire) || sender.send(line).is_err() {
                    break;
                }
            }
        });
        Self {
            process,
            input,
            responses,
            abandon,
            reader: Some(reader),
            next: 0,
        }
    }

    fn send(&mut self, name: &str, arguments: Value) -> u64 {
        self.next += 1;
        let request = json!({"jsonrpc":"2.0","id":self.next,"method":"tools/call","params":{
            "name":name,"arguments":arguments,"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}});
        let input = self.input.as_mut().expect("live request pipe");
        serde_json::to_writer(&mut *input, &request).expect("request bytes");
        input.write_all(b"\n").expect("request frame");
        input.flush().expect("request delivery");
        self.next
    }

    fn call(&mut self, name: &str, arguments: Value) -> Value {
        let id = self.send(name, arguments);
        let line = self
            .responses
            .recv_timeout(Duration::from_secs(20))
            .expect("bounded response")
            .expect("native line");
        let value: Value = serde_json::from_str(&line).expect("JSON-only carrier");
        assert_eq!(value["id"], id);
        assert!(value.get("error").is_none(), "{value}");
        let result = &value["result"];
        assert_eq!(
            serde_json::from_str::<Value>(
                result["content"][0]["text"]
                    .as_str()
                    .expect("text projection")
            )
            .expect("text JSON"),
            result["structuredContent"]
        );
        result["structuredContent"].clone()
    }

    fn wait(&mut self, handle: &str, predicate: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(25);
        loop {
            let report = self.call("inspect_preview", json!({"session":handle}));
            if predicate(&report["snapshot"]) {
                return report;
            }
            assert!(
                Instant::now() < deadline,
                "preview transition timed out: {report}"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn stop(&mut self, handle: &str) {
        let request = self.call("stop_preview", json!({"session":handle}));
        assert_eq!(request["retained"], true);
        assert_eq!(request["snapshot"]["stop_requested"], true);
        let finished = self.wait(handle, |state| state["finished"] == true);
        assert_eq!(finished["snapshot"]["terminal"]["status"], "closed");
        let released = self.call(
            "stop_preview",
            json!({"session":handle,"release_handle":true}),
        );
        assert_eq!(released["status"], "released");
        assert_eq!(released["retained"], false);
    }

    fn finish(&mut self, success: bool) {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(status) = self.process.0.try_wait().expect("native process status") {
                assert_eq!(status.success(), success);
                break;
            }
            assert!(Instant::now() < deadline, "MCP did not terminate");
            thread::sleep(Duration::from_millis(10));
        }
        if let Some(reader) = self.reader.take() {
            reader.join().expect("output reader");
        }
        let mut stderr = String::new();
        self.process
            .0
            .stderr
            .take()
            .expect("stderr")
            .read_to_string(&mut stderr)
            .expect("diagnostic");
        if success {
            assert!(stderr.is_empty(), "{stderr}");
        } else {
            assert!(!stderr.is_empty(), "failed carrier must report failure");
        }
    }
}

#[test]
fn both_url_modes_serve_without_poll_and_match_sdk_and_direct_cli() {
    for local in [false, true] {
        let fixture = Fixture::new();
        let recipe = fixture.recipe();
        let inputs = build::acquire_publications(
            &fixture.config,
            &SITES.map(key),
            Default::default(),
            Default::default(),
        )
        .expect("independent SDK acquisition");
        let before = source::inspect(fixture.root.path(), None, Default::default())
            .expect("original sources");
        let mut client = Client::start();
        let started = client.call("start_preview", fixture.request(local));
        assert_eq!(started["retained"], true);
        let handle = started["session"].as_str().expect("handle").to_owned();
        let ready = client.wait(&handle, |state| state["latest"].is_object());
        let state = &ready["snapshot"]["latest"];
        assert_eq!(state["input_mode"], "pinned");
        assert_eq!(
            state["input_identity"],
            json!(preview::watch::InputIdentity::of(&inputs))
        );
        let sockets = SITES.map(|site| address(state, site));
        let resolved = recipe
            .resolve(Default::default())
            .expect("retained explicit workspace");
        let mut options = resolved.options(Default::default());
        if local {
            options.preview_urls = Some(
                SITES
                    .into_iter()
                    .map(|site| {
                        (
                            key(site),
                            state["sites"][site]["url"]
                                .as_str()
                                .expect("held URL")
                                .parse()
                                .expect("local base"),
                        )
                    })
                    .collect(),
            );
        }
        let expected = build::render_publications(&inputs, options, Default::default())
            .expect("same shared compiler");
        let mut served = Vec::new();
        // No MCP poll or inspect occurs while these real HTTP requests are served.
        for (site, socket) in SITES.into_iter().zip(sockets) {
            let native = expected.sites()[&key(site)]
                .as_ref()
                .expect("site")
                .site()
                .output();
            let mount = if site == "site:one" { "/mount/" } else { "/" };
            let file = if site == "site:one" {
                "guide/index.html"
            } else {
                "elsewhere/index.html"
            };
            let page = get(socket, &format!("{mount}{file}"));
            let model = get(socket, &format!("{mount}model.json"));
            assert_eq!(
                page,
                native.file(&file.parse().expect("path")).expect("page")
            );
            assert_eq!(
                model,
                native
                    .file(&"model.json".parse().expect("path"))
                    .expect("model")
            );
            assert_eq!(
                state["sites"][site]["output_identity"],
                json!(native.inventory().identity())
            );
            served.push((mount, file, page, model));
        }
        let other_link = if local {
            format!("http://{}/elsewhere/", sockets[1])
        } else {
            "https://other.example/elsewhere/".to_owned()
        };
        let mut attributes = Vec::new();
        gazette_sdk::html::inspect(&served[0].2, Default::default(), |kind, value| {
            if kind == gazette_sdk::html::TextKind::Attribute {
                attributes.push(value.to_owned());
            }
            Ok::<_, std::convert::Infallible>(())
        })
        .expect("decode actual served HTML attributes");
        assert!(
            attributes.contains(&other_link),
            "resolved cross-site symbolic reference"
        );
        assert_eq!(
            attributes
                .iter()
                .filter(|value| *value == &other_link)
                .count(),
            2,
            "symbolic and literal links to an owned publication use the same selected base"
        );
        assert!(
            attributes.contains(&"https://unowned.example/reference/".to_owned()),
            "links outside the owned publication map retain their public destination"
        );
        let model: Value = serde_json::from_slice(&served[0].3).expect("actual served model");
        let page = &model["pages"]["content:one"];
        assert_eq!(
            page["site"]["metadata"]["render_year"], 2040,
            "all hosts use the fixture year"
        );
        assert_eq!(
            page["collections"],
            json!([]),
            "foreign reference is not containment"
        );
        let trails = page["breadcrumbs"]
            .as_array()
            .expect("native self-only trail");
        assert_eq!(trails.len(), 1);
        assert_eq!(trails[0]["via"], "content:one");
        let items = trails[0]["items"].as_array().expect("native endpoints");
        assert_eq!(items.len(), 1, "standalone article has no parent endpoint");
        assert_eq!(items[0]["node"], page["content"]);
        assert_eq!(items[0]["route"], page["route"]);
        assert_eq!(page["journeys"], json!([]));
        assert_eq!(page["related"][0]["id"], "collection:other");
        let bound = page["links"]
            .as_array()
            .expect("retained link annotations")
            .iter()
            .find(|link| link["content"] == "collection:other")
            .expect("symbolic reference retains the foreign content identity");
        assert_eq!(bound["destination"], other_link);
        assert_eq!(bound["kind"], "cross-site");
        assert_eq!(bound["relationships"], json!(["related"]));
        // Another service cannot operate this process-local handle.
        let foreign = carrier::exchange(vec![
            json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"stop_preview","arguments":{"session":handle}}}),
        ]);
        assert_eq!(
            foreign[0]["result"]["structuredContent"]["code"],
            "preview_session_unknown"
        );
        let second = client.call("start_preview", fixture.request(local));
        let second_handle = second["session"]
            .as_str()
            .expect("independent handle")
            .to_owned();
        let second_ready = client.wait(&second_handle, |state| state["latest"].is_object());
        let second_sockets = SITES.map(|site| address(&second_ready["snapshot"]["latest"], site));
        client.stop(&handle);
        released(&sockets);
        assert!(!get(second_sockets[0], "/mount/model.json").is_empty());
        client.stop(&second_handle);
        released(&second_sockets);
        drop(client.input.take());
        client.finish(true);

        // Reuse the now-free exact endpoints so CLI local URLs and output bytes are comparable.
        let mut command = Command::new(env!("CARGO_BIN_EXE_gazette"));
        command.args(["preview-sites", "--local", "--allow-listeners"]);
        if local {
            command.arg("--local-urls");
        }
        for (site, socket) in SITES.into_iter().zip(sockets) {
            command.arg("--listen").arg(format!("{site}={socket}"));
        }
        let mut direct = carrier::Process(
            command
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("native CLI preview"),
        );
        let mut input = direct.0.stdin.take().expect("CLI input");
        let output = direct.0.stdout.take().expect("CLI events");
        let (sender, receiver) = mpsc::sync_channel(2);
        let reader = thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        writeln!(input, "{}", fixture.recipe_value()).expect("CLI same recipe");
        let cli_ready: Value = serde_json::from_str(
            &receiver
                .recv_timeout(Duration::from_secs(20))
                .expect("CLI ready")
                .expect("line"),
        )
        .expect("event");
        assert_eq!(cli_ready["event"], "ready");
        for field in [
            "status",
            "input_mode",
            "generation",
            "stale",
            "input_identity",
            "sites",
        ] {
            assert_eq!(cli_ready[field], state[field], "{field}");
        }
        for ((mount, file, page, model), socket) in served.into_iter().zip(sockets) {
            assert_eq!(get(socket, &format!("{mount}{file}")), page);
            assert_eq!(get(socket, &format!("{mount}model.json")), model);
        }
        drop(input);
        let closed: Value = serde_json::from_str(
            &receiver
                .recv_timeout(Duration::from_secs(20))
                .expect("CLI close")
                .expect("line"),
        )
        .expect("closed");
        assert_eq!(closed["event"], "closed");
        assert!(direct.0.wait().expect("CLI completion").success());
        reader.join().expect("CLI reader");
        released(&sockets);
        assert_eq!(
            source::inspect(fixture.root.path(), None, Default::default())
                .expect("unchanged sources"),
            before
        );
    }
}

#[test]
fn watch_retains_failed_complete_generation_and_recovers_with_replayable_gap() {
    let fixture = Fixture::new();
    let mut client = Client::start();
    let mut request = fixture.request(true);
    request["mode"] = json!({"kind":"development","interval_ms":100});
    request["history"] = json!({"events":2,"bytes":65536,"event_bytes":32768});
    let started = client.call("start_preview", request);
    let handle = started["session"].as_str().expect("handle").to_owned();
    let ready = client.wait(&handle, |state| state["latest"].is_object());
    let sockets = SITES.map(|site| address(&ready["snapshot"]["latest"], site));
    let original = fs::read_to_string(fixture.root.path().join("one/page.md")).expect("source");
    fixture.replace(
        "one/page.md",
        &original.replace("Retained one", "Changed one"),
    );
    let changed = client.wait(&handle, |state| state["latest"]["generation"] == 2);
    let page = get(sockets[0], "/mount/guide/index.html");
    assert!(String::from_utf8_lossy(&page).contains("Changed one"));
    let other = get(sockets[1], "/elsewhere/index.html");
    fixture.replace(
        "theme/templates/page.html",
        &format!(
            "{{% if site.id == 'site:other' %}}{{{{ missing_watch_value }}}}{{% endif %}}{TEMPLATE}"
        ),
    );
    let stale = client.wait(&handle, |state| state["latest"]["stale"] == true);
    assert_eq!(stale["snapshot"]["latest"]["generation"], 2);
    assert_eq!(get(sockets[0], "/mount/guide/index.html"), page);
    assert_eq!(get(sockets[1], "/elsewhere/index.html"), other);
    fixture.replace("theme/templates/page.html", TEMPLATE);
    let recovered = client.wait(&handle, |state| {
        state["last_sequence"].as_u64().is_some_and(|v| v >= 4) && state["latest"]["stale"] == false
    });
    assert_eq!(
        recovered["snapshot"]["latest"],
        changed["snapshot"]["latest"]
    );
    let arguments = json!({"session":handle,"after":0,"maximum":2});
    let first = client.call("poll_preview", arguments.clone());
    assert!(first["observations"]["gap"].is_object());
    assert_eq!(first, client.call("poll_preview", arguments));
    assert_eq!(
        first["observations"]["events"]
            .as_array()
            .expect("events")
            .last()
            .expect("recovery")["event"]["event"],
        "recovered"
    );
    assert_eq!(
        SITES.map(|site| address(&recovered["snapshot"]["latest"], site)),
        sockets
    );
    client.stop(&handle);
    released(&sockets);
    drop(client.input.take());
    client.finish(true);
}

#[test]
fn broken_mcp_output_while_stdin_remains_open_tears_down_actual_listeners() {
    let fixture = Fixture::new();
    let mut client = Client::start();
    let started = client.call("start_preview", fixture.request(true));
    let handle = started["session"].as_str().expect("handle").to_owned();
    let ready = client.wait(&handle, |state| state["latest"].is_object());
    let sockets = SITES.map(|site| address(&ready["snapshot"]["latest"], site));
    assert!(!get(sockets[0], "/mount/model.json").is_empty());
    // End the sole stdout reader after its current read, without closing stdin.
    client.abandon.store(true, Ordering::Release);
    client.send("inspect_preview", json!({"session":handle}));
    client
        .reader
        .take()
        .expect("reader")
        .join()
        .expect("stdout read end closed");
    client.send("inspect_preview", json!({"session":handle}));
    client.finish(false);
    assert!(
        client.input.is_some(),
        "carrier failure, not deliberate stdin EOF"
    );
    released(&sockets);
    assert_eq!(
        fs::read_to_string(fixture.root.path().join("theme/templates/page.html")).expect("source"),
        TEMPLATE
    );
}
