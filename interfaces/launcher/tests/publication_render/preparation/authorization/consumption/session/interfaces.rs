//! Interactive executable carriers meet at the same held native release contracts.

#[path = "interfaces/acceptance.rs"]
mod acceptance;
#[path = "interfaces/recovery.rs"]
mod recovery;

use super::{
    Operation, Selected, consume_options, fs, read, release, session, store, verification,
};
use crate::mcp_carrier::Process;
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    io::{BufRead, BufReader, Read, Write},
    process::{ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

struct Carrier {
    process: Process,
    input: Option<ChildStdin>,
    responses: Receiver<std::io::Result<String>>,
    mcp: bool,
    sequence: u64,
    tools: Vec<Value>,
    handle: String,
}

impl Carrier {
    fn new(mcp: bool, directory: &std::path::Path) -> Self {
        Self::start(mcp, directory, "release")
    }

    fn start(mcp: bool, directory: &std::path::Path, kind: &str) -> Self {
        let mut process = Process(
            Command::new(env!("CARGO_BIN_EXE_gazette"))
                .arg(if mcp {
                    "mcp".into()
                } else {
                    format!("{kind}-session")
                })
                .current_dir(directory)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("real Gazette interface"),
        );
        let input = process.0.stdin.take().expect("interactive input");
        let output = process.0.stdout.take().expect("interactive output");
        let (send, responses) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                if send.send(line).is_err() {
                    break;
                }
            }
        });
        let mut carrier = Self {
            process,
            input: Some(input),
            responses,
            mcp,
            sequence: 0,
            tools: vec![],
            handle: String::new(),
        };
        if mcp {
            let mut cursor = None;
            let mut seen = BTreeSet::new();
            for _ in 0..32 {
                let params = cursor
                    .as_ref()
                    .map_or_else(|| json!({}), |cursor| json!({"cursor":cursor}));
                let catalog = carrier.rpc("tools/list", params);
                let page = catalog["result"]["tools"]
                    .as_array()
                    .expect("actual discovery");
                assert!(!page.is_empty(), "nonempty paged discovery");
                for tool in page {
                    let name = tool["name"].as_str().expect("named discovered tool");
                    assert!(seen.insert(name.to_owned()), "duplicate discovered tool");
                    carrier.tools.push(tool.clone());
                }
                let Some(next) = catalog["result"]["nextCursor"].as_str() else {
                    cursor = None;
                    break;
                };
                assert_eq!(page.len(), 8, "complete nonterminal discovery page");
                assert_ne!(cursor.as_deref(), Some(next), "tool cursor loop");
                cursor = Some(next.to_owned());
            }
            assert!(
                cursor.is_none(),
                "tool discovery exceeded its bounded page count"
            );
            for name in [
                format!("open_{kind}_session"),
                format!("operate_{kind}_session"),
                format!("close_{kind}_session"),
            ] {
                let tool = carrier
                    .tools
                    .iter()
                    .find(|tool| tool["name"] == name)
                    .expect("discoverable session tool");
                assert!(tool["inputSchema"].is_object());
                assert!(tool["outputSchema"].is_object());
                assert_eq!(tool["annotations"]["readOnlyHint"], false);
            }
        }
        carrier
    }

    fn line(&mut self, value: &Value) -> Value {
        let input = self.input.as_mut().expect("open carrier");
        serde_json::to_writer(&mut *input, value).expect("complete request encoding");
        input.write_all(b"\n").expect("request frame");
        input.flush().expect("request delivery");
        let response = self
            .responses
            .recv_timeout(Duration::from_secs(120))
            .expect("bounded complete native operation response")
            .expect("response line");
        serde_json::from_str(&response).expect("only complete JSON on stdout")
    }

    fn rpc(&mut self, method: &str, mut params: Value) -> Value {
        self.sequence += 1;
        params["_meta"] = json!({
            "io.modelcontextprotocol/protocolVersion":"2026-07-28",
            "io.modelcontextprotocol/clientCapabilities":{}
        });
        let response =
            self.line(&json!({"jsonrpc":"2.0","id":self.sequence,"method":method,"params":params}));
        assert_eq!(response["id"], self.sequence);
        assert!(response.get("error").is_none(), "{response}");
        response
    }

    fn tool(&mut self, name: &str, arguments: Value) -> Value {
        let descriptor = self
            .tools
            .iter()
            .find(|tool| tool["name"] == name)
            .expect("discovered tool")
            .clone();
        jsonschema::validator_for(&descriptor["inputSchema"])
            .expect("input schema compiles")
            .validate(&arguments)
            .expect("actual request matches discovery");
        let response = self.rpc("tools/call", json!({"name":name,"arguments":arguments}));
        let result = &response["result"];
        assert_eq!(result["isError"], false, "{response}");
        let report = result["structuredContent"].clone();
        jsonschema::validator_for(&descriptor["outputSchema"])
            .expect("output schema compiles")
            .validate(&report)
            .expect("actual native output matches discovery");
        let text: Value = serde_json::from_str(
            result["content"][0]["text"]
                .as_str()
                .expect("readable output"),
        )
        .expect("readable complete JSON");
        assert_eq!(text, report);
        report
    }

    fn open(&mut self, selected: &session::OpenRequest) -> store::JournalHeader {
        let report = if self.mcp {
            let report = self.tool("open_release_session", json!({"request":selected}));
            assert_eq!(report["status"], "ready", "{report}");
            self.handle = report["session"]
                .as_str()
                .expect("opaque native handle")
                .to_owned();
            report
        } else {
            let report = self.line(&json!(selected));
            assert_eq!(report["event"], "ready", "{report}");
            report
        };
        serde_json::from_value(report["inspection"]["header"].clone())
            .expect("actual native readiness")
    }

    fn operate(&mut self, operation: Operation) -> Value {
        let report = if self.mcp {
            let report = if matches!(operation, Operation::Close) {
                self.tool("close_release_session", json!({"session":self.handle}))
            } else {
                self.tool(
                    "operate_release_session",
                    json!({"session":self.handle,"operation":operation}),
                )
            };
            assert_eq!(report["status"], "outcome", "{report}");
            assert_eq!(report["session"], self.handle);
            report["outcome"].clone()
        } else {
            self.line(&json!(operation))
        };
        assert_ne!(report["result"], "failed", "{report}");
        report
    }

    fn finish(mut self) {
        self.input.take();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.process.0.try_wait().expect("native process state") {
                assert!(status.success(), "{status}");
                break;
            }
            assert!(
                Instant::now() < deadline,
                "interface did not exit after explicit close and EOF"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut stderr = String::new();
        self.process
            .0
            .stderr
            .take()
            .expect("stderr")
            .read_to_string(&mut stderr)
            .expect("stderr read");
        assert!(stderr.is_empty(), "{stderr}");
    }
}

fn assert_complete_output_profile(selected: &Selected) {
    for (ordinal, record) in selected.local.prepared.records.iter().enumerate() {
        let root = record.intent.address.path();
        let (site, content, html_path, visible_title, body) = match ordinal {
            0 => (
                "site:one",
                "content:one",
                "guide/index.html",
                "One &lt;escaped&gt;",
                "Retained",
            ),
            1 => (
                "site:other",
                "content:other",
                "elsewhere/index.html",
                "Other",
                "Other body",
            ),
            _ => panic!("fixture has exactly two publication targets"),
        };
        let read_json = |path: &str| -> Value {
            serde_json::from_slice(&fs::read(root.join(path)).expect("retained machine output"))
                .expect("machine output is valid JSON")
        };
        let model = read_json("model.json");
        let page = &model["pages"][content];
        let canonical = page["route"]["canonical_url"]
            .as_str()
            .expect("one canonical URL from the shared page model");

        let html = String::from_utf8(fs::read(root.join(html_path)).expect("retained HTML"))
            .expect("fixture HTML");
        assert!(
            html.contains(visible_title),
            "HTML title differs from model"
        );
        assert!(html.contains(body), "HTML body differs from model");
        assert!(
            html.contains(&canonical.replace('/', "&#x2F;")),
            "HTML route differs from model"
        );

        let search = read_json("search.json");
        let search_page = search
            .as_array()
            .expect("search documents")
            .iter()
            .find(|item| item["content"]["id"] == content)
            .expect("admitted page in search output");
        for field in ["content", "route", "breadcrumbs", "journeys"] {
            assert_eq!(search_page[field], page[field], "search parity for {field}");
        }
        assert_eq!(search_page["body"], page["body_text"]);

        let breadcrumbs = page["breadcrumbs"]
            .as_array()
            .expect("structural navigation trails");
        assert!(!breadcrumbs.is_empty(), "explicit structural path retained");
        assert!(breadcrumbs.iter().any(|trail| {
            trail["items"]
                .as_array()
                .and_then(|items| items.last())
                .is_some_and(|item| item["node"]["id"] == content)
        }));

        let json_ld = read_json("site.jsonld");
        let graph_page = json_ld["@graph"]
            .as_array()
            .expect("JSON-LD graph")
            .iter()
            .find(|item| item["identifier"] == content)
            .expect("admitted page in JSON-LD output");
        assert_eq!(graph_page, &page["json_ld"]);
        assert!(graph_page.get("gazette:breadcrumbs").is_none());
        let article = page["content"]["structure"]["family"]["kind"] == "article";
        assert_eq!(
            graph_page["@type"],
            if article { "Article" } else { "WebPage" }
        );
        let public_page = if article {
            &graph_page["mainEntityOfPage"]
        } else {
            graph_page
        };
        assert_eq!(public_page["@type"], "WebPage");
        assert_eq!(public_page["url"], page["route"]["url"]);
        // A trail that holds only the page itself says nothing, so public JSON-LD omits it.
        let breadcrumbs: Vec<&Value> = breadcrumbs
            .iter()
            .filter(|trail| {
                trail["items"]
                    .as_array()
                    .is_some_and(|items| items.len() > 1)
            })
            .collect();
        let public_trails = public_page["breadcrumb"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default();
        assert_eq!(public_trails.len(), breadcrumbs.len());
        for (public, native) in public_trails.iter().zip(breadcrumbs) {
            assert_eq!(public["@type"], "BreadcrumbList");
            let items = native["items"].as_array().expect("native trail items");
            assert_eq!(public["numberOfItems"], items.len());
            let public_items = public["itemListElement"]
                .as_array()
                .expect("public trail items");
            assert_eq!(public_items.len(), items.len());
            for (position, (item, native)) in public_items.iter().zip(items).enumerate() {
                assert_eq!(item["@type"], "ListItem");
                assert_eq!(item["position"], position + 1);
                assert_eq!(item["name"], native["node"]["title"]);
                // A trail names each step by identity; the step's facts stay on its own page.
                assert_eq!(item["item"]["@id"], native["route"]["canonical_url"]);
            }
        }

        let themed_graph = read_json("graph.json");
        assert_eq!(themed_graph, model["graph"]);
        assert_eq!(themed_graph["site"], site);
        assert_eq!(themed_graph["nodes"][content]["id"], content);

        let feed = read_json("feed.json");
        assert_eq!(feed["_gazette"]["content"], page["content"]);
        assert_eq!(feed["_gazette"]["route"], page["route"]);
        assert_eq!(feed["_gazette"]["breadcrumbs"], page["breadcrumbs"]);

        assert!(
            String::from_utf8(fs::read(root.join("sitemap.xml")).expect("retained sitemap"))
                .expect("sitemap XML")
                .contains(canonical),
            "sitemap route differs from model"
        );
        assert!(
            String::from_utf8(fs::read(root.join("llms.txt")).expect("retained llms output"))
                .expect("llms text")
                .contains(canonical),
            "llms route differs from model"
        );
    }
}

fn connected(mcp: bool) {
    let mut selected = Selected::new(true);
    assert_complete_output_profile(&selected);
    let mut carrier = Carrier::new(mcp, selected.local.control.path());
    let header = carrier.open(&selected.open);
    let (input, binding) = selected.authorize(header);
    let registered = carrier.operate(Operation::Register {
        expected: header,
        input: input.clone(),
        options: release::RegisterAuthorizationOptions {
            verification: verification(),
            record: store::ReleaseAuthorizationWriteOptions { allow_record: true },
        },
    });
    assert_eq!(registered["result"], "registered");
    assert_eq!(registered["receipt"]["journal"], json!(header));
    assert_eq!(
        registered["receipt"]["record"]["authorization"],
        input.authorization
    );
    assert_eq!(registered["receipt"]["record"]["binding"], json!(binding));
    let consumed = carrier.operate(Operation::Consume {
        expected: header,
        input: input.clone(),
        options: consume_options(),
    });
    assert_eq!(consumed["result"], "consumed");
    assert_eq!(consumed["receipt"]["journal"], json!(header));
    let id = consumed["receipt"]["record"]["authorization_id"]
        .as_str()
        .expect("original consumed ID")
        .to_owned();
    assert_eq!(
        consumed["receipt"]["record"]["targets"]
            .as_array()
            .expect("complete set")
            .len(),
        2
    );
    let mut attempts = Vec::new();
    for ordinal in 0..2 {
        let response = carrier.operate(Operation::Dispatch {
            expected: header,
            input: input.clone(),
            ordinal,
            number: 1,
            options: release::DispatchAttemptOptions {
                verification: verification(),
                read: release::RemoteReadOptions {
                    allow_read: true,
                    ..Default::default()
                },
                record: store::ReleaseAttemptOptions { allow_record: true },
                dispatch: release::DispatchOptions {
                    allow_dispatch: true,
                    timeout: Duration::from_secs(10),
                    ..Default::default()
                },
            },
        });
        assert_eq!(response["result"], "dispatched");
        assert_eq!(response["attempt"]["start"]["created"], json!(header));
        assert_eq!(response["attempt"]["result"]["created"], json!(header));
        attempts.push(response["attempt"].clone());
    }
    let settled = carrier.operate(Operation::Reconcile {
        expected: header,
        authorization_id: id.clone(),
        number: 1,
        options: release::ReconcileReleaseOptions {
            read: release::RemoteReadOptions {
                allow_read: true,
                ..Default::default()
            },
            record: store::ReleaseReconciliationOptions {
                allow_record: true,
                ..Default::default()
            },
            ..Default::default()
        },
    });
    assert_eq!(settled["result"], "reconciled");
    assert_eq!(
        settled["receipt"]["record"]["settlement"]["state"],
        "succeeded"
    );
    assert_eq!(
        carrier.operate(Operation::Close),
        json!({"result":"closed"})
    );
    let database = &selected.local.prepared.database;
    let history = store::inspect_release_reconciliations(database, &id, Some(&header), read())
        .expect("independent reopened native history");
    assert_eq!(
        json!(history.history.consumption),
        consumed["receipt"]["record"]
    );
    assert_eq!(json!(history.history.attempts), json!(attempts));
    assert_eq!(
        json!(history.records),
        json!([settled["receipt"]["record"].clone()])
    );
    let original = store::inspect_release_authorization(database, &id, Some(&header), read())
        .expect("original pending custody")
        .record
        .expect("retained original");
    assert_eq!(json!(original), registered["receipt"]["record"]);
    assert_eq!(
        original.authorization, input.authorization,
        "inner raw signed string is never normalized through Value"
    );
    assert_eq!(
        fs::read(&selected.calls)
            .expect("fresh provider calls")
            .len(),
        7
    );
    for (path, target) in selected.local.paths.iter().zip(
        history
            .history
            .consumption
            .expect("consumed intent")
            .targets,
    ) {
        let repository = git2::Repository::open_bare(path).expect("actual private destination");
        let actual = repository
            .refname_to_id("refs/heads/publication")
            .expect("actual branch after dispatch");
        assert_eq!(actual.to_string(), target.target.local_commit_oid.as_str());
    }
    if mcp {
        let mut second = selected.open.clone();
        second.expected = header;
        second.authority = None;
        let next = carrier.open(&second);
        assert!(next.writer_fence > header.writer_fence);
        let inspected = carrier.operate(Operation::Inspect);
        assert_eq!(inspected["inspection"]["header"], json!(next));
        assert_eq!(
            carrier.operate(Operation::Close),
            json!({"result":"closed"})
        );
        assert_eq!(
            fs::read(&selected.calls)
                .expect("unchanged authority reads")
                .len(),
            7
        );
    }
    carrier.finish();
}

#[test]
fn executable_cli_holds_one_generation_through_complete_two_target_release() {
    connected(false);
}

#[test]
fn executable_mcp_discovers_and_validates_complete_release_then_opens_independent_session() {
    connected(true);
}
