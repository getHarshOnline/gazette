//! The Gazette command exposes real framed configuration diagnostics, not another engine.

use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Cursor, Read, Write},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn frame(value: Value) -> Vec<u8> {
    let body = serde_json::to_vec(&value).expect("JSON");
    let mut result = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
    result.extend(body);
    result
}

fn messages(bytes: Vec<u8>) -> Vec<Value> {
    let mut input = BufReader::new(Cursor::new(bytes));
    let mut results = Vec::new();
    loop {
        let mut line = String::new();
        if input.read_line(&mut line).expect("header") == 0 {
            return results;
        }
        let length: usize = line
            .strip_prefix("Content-Length: ")
            .expect("protocol-only stdout")
            .trim()
            .parse()
            .expect("length");
        line.clear();
        input.read_line(&mut line).expect("separator");
        assert_eq!(line, "\r\n");
        let mut body = vec![0; length];
        input.read_exact(&mut body).expect("complete body");
        results.push(serde_json::from_slice(&body).expect("JSON frame"));
    }
}

fn run(requests: Vec<Value>, failed_stdout: bool) -> (bool, Vec<Value>) {
    let cwd = tempfile::tempdir().expect("isolated cwd");
    let mut command = Command::new(env!("CARGO_BIN_EXE_gazette"));
    command
        .arg("lsp")
        .current_dir(cwd.path())
        .stdin(Stdio::piped())
        .stderr(Stdio::piped());
    if failed_stdout {
        command.stdout(std::fs::File::open("/dev/null").expect("nonwritable descriptor"));
    } else {
        command.stdout(Stdio::piped());
    }
    let mut process = Process(command.spawn().expect("Gazette LSP"));
    let mut input = process.0.stdin.take().expect("stdin");
    for request in requests {
        input.write_all(&frame(request)).expect("request frame");
    }
    drop(input);
    // These synthetic responses fit the OS pipe; bounded waiting exposes lifecycle hangs.
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = process.0.try_wait().expect("status") {
            break status;
        }
        assert!(Instant::now() < deadline, "LSP did not terminate");
        std::thread::sleep(Duration::from_millis(10));
    };
    let mut output = Vec::new();
    if let Some(mut stdout) = process.0.stdout.take() {
        stdout.read_to_end(&mut output).expect("stdout");
    }
    let mut errors = String::new();
    process
        .0
        .stderr
        .take()
        .expect("stderr")
        .read_to_string(&mut errors)
        .expect("stderr text");
    if status.success() {
        assert!(errors.is_empty(), "{errors}");
    }
    assert!(std::fs::read_dir(cwd.path()).expect("cwd").next().is_none());
    (status.success(), messages(output))
}

fn initialize() -> Value {
    json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{},
        "initializationOptions":{"workspace":{"uri":"file:///not-opened/gazette/workspace.toml","path":"workspace.toml","text":"[[principals]]\nid='owner:one'\nkind='organization'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n"},"documents":[]}
    }})
}

#[test]
fn command_hovers_current_selected_workspace_identity_without_native_reads() {
    let uri = "file:///not-opened/gazette/workspace.toml";
    let request =
        |id, method, params| json!({"jsonrpc":"2.0","id":id,"method":method,"params":params});
    let (success, output) = run(
        vec![
            initialize(),
            json!({"jsonrpc":"2.0","method":"initialized","params":{}}),
            request(
                2,
                "textDocument/hover",
                json!({"textDocument":{"uri":uri},"position":{"line":5,"character":13}}),
            ),
            request(3, "shutdown", Value::Null),
            json!({"jsonrpc":"2.0","method":"exit","params":null}),
        ],
        false,
    );
    assert!(success);
    let response = output
        .iter()
        .find(|v| v["id"] == 2)
        .expect("real hover response");
    assert_eq!(
        response["result"]["contents"],
        json!({"kind":"plaintext","value":"Resolved principal: owner:one"})
    );
    assert_eq!(
        response["result"]["range"],
        json!({"start":{"line":5,"character":10},"end":{"line":5,"character":21}})
    );
    assert_eq!(output[0]["result"]["capabilities"]["hoverProvider"], true);
}

#[test]
fn command_navigates_selected_native_footnotes_without_opening_uri_paths() {
    let uri = "file:///not-opened/gazette/article.md";
    let source = "+++\r\n[content]\r\nid='content:one'\r\nsite='site:one'\r\ntitle='Article'\r\nlifecycle='draft'\r\n[content.structure]\r\nkind='page'\r\nfamily={kind='article'}\r\n+++\r\n😀 [^note]\r\n\r\n[^note]: Local evidence.\r\n";
    let mut initial = initialize();
    initial["params"]["initializationOptions"]["documents"] =
        json!([{"uri":uri,"path":"article.md","text":source}]);
    let (success, output) = run(
        vec![
            initial,
            json!({"jsonrpc":"2.0","method":"initialized","params":{}}),
            json!({"jsonrpc":"2.0","id":2,"method":"textDocument/definition","params":{"textDocument":{"uri":uri},"position":{"line":10,"character":4}}}),
            json!({"jsonrpc":"2.0","id":3,"method":"textDocument/references","params":{"textDocument":{"uri":uri},"position":{"line":12,"character":2},"context":{"includeDeclaration":false}}}),
            json!({"jsonrpc":"2.0","id":4,"method":"shutdown","params":null}),
            json!({"jsonrpc":"2.0","method":"exit","params":null}),
        ],
        false,
    );
    assert!(success);
    let response = |id| {
        output
            .iter()
            .find(|message| message["id"] == id)
            .expect("requested executable navigation response")
    };
    assert_eq!(
        response(1)["result"]["capabilities"]["definitionProvider"],
        true
    );
    assert_eq!(
        response(1)["result"]["capabilities"]["referencesProvider"],
        true
    );
    assert_eq!(
        response(2)["result"],
        json!({"uri":uri,"range":{"start":{"line":12,"character":0},"end":{"line":12,"character":24}}})
    );
    assert_eq!(
        response(3)["result"],
        json!([{"uri":uri,"range":{"start":{"line":10,"character":3},"end":{"line":10,"character":10}}}])
    );
}

#[test]
fn command_follows_compiler_heading_anchors_and_reverse_references_without_native_reads() {
    let uri = "file:///not-opened/gazette/article.md";
    let source = "+++\r\n[content]\r\nid='content:one'\r\nsite='site:one'\r\ntitle='Article'\r\nlifecycle='draft'\r\n[content.structure]\r\nkind='page'\r\nfamily={kind='article'}\r\n+++\r\n# Target\r\n\r\n😀 [jump](#target)\r\n";
    let mut initial = initialize();
    initial["params"]["initializationOptions"]["documents"] =
        json!([{"uri":uri,"path":"article.md","text":source}]);
    let (success, output) = run(
        vec![
            initial,
            json!({"jsonrpc":"2.0","method":"initialized","params":{}}),
            json!({"jsonrpc":"2.0","id":2,"method":"textDocument/definition","params":{"textDocument":{"uri":uri},"position":{"line":12,"character":4}}}),
            json!({"jsonrpc":"2.0","id":3,"method":"textDocument/references","params":{"textDocument":{"uri":uri},"position":{"line":10,"character":2},"context":{"includeDeclaration":false}}}),
            json!({"jsonrpc":"2.0","id":4,"method":"shutdown","params":null}),
            json!({"jsonrpc":"2.0","method":"exit","params":null}),
        ],
        false,
    );
    assert!(success);
    let response = |id| {
        output
            .iter()
            .find(|message| message["id"] == id)
            .expect("requested executable heading navigation response")
    };
    assert_eq!(
        response(2)["result"],
        json!({"uri":uri,"range":{"start":{"line":10,"character":0},"end":{"line":10,"character":8}}})
    );
    assert_eq!(
        response(3)["result"],
        json!([{"uri":uri,"range":{"start":{"line":12,"character":3},"end":{"line":12,"character":18}}}])
    );
}

#[test]
fn command_checks_editor_buffers_and_clears_diagnostics_without_opening_their_uri() {
    let uri = "file:///not-opened/gazette/workspace.toml";
    let valid = "[[principals]]\nid='owner:one'\nkind='organization'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n";
    let (success, output) = run(
        vec![
            initialize(),
            json!({"jsonrpc":"2.0","method":"initialized","params":{}}),
            json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":uri,"languageId":"toml","version":1,"text":"# 🦀\r\n[bad"}}}),
            json!({"jsonrpc":"2.0","method":"textDocument/didChange","params":{"textDocument":{"uri":uri,"version":2},"contentChanges":[{"text":valid}]}}),
            json!({"jsonrpc":"2.0","method":"textDocument/didClose","params":{"textDocument":{"uri":uri}}}),
            json!({"jsonrpc":"2.0","id":2,"method":"shutdown","params":null}),
            json!({"jsonrpc":"2.0","method":"exit","params":null}),
        ],
        false,
    );
    assert!(success);
    assert_eq!(output.len(), 10);
    assert_eq!(
        output[0]["result"]["capabilities"]["positionEncoding"],
        "utf-16"
    );
    assert_eq!(
        output[0]["result"]["capabilities"]["textDocumentSync"]["change"],
        1
    );
    let diagnostics: Vec<_> = output
        .iter()
        .filter(|m| m["method"] == "textDocument/publishDiagnostics")
        .map(|m| &m["params"])
        .collect();
    assert_eq!(diagnostics.len(), 4);
    assert_eq!(diagnostics[0]["diagnostics"], json!([]));
    assert_eq!(diagnostics[1]["version"], 1);
    assert_eq!(diagnostics[1]["diagnostics"][0]["code"], "toml_invalid");
    assert_eq!(
        diagnostics[1]["diagnostics"][0]["range"]["start"]["line"],
        1
    );
    assert_eq!(diagnostics[2]["version"], 2);
    assert_eq!(diagnostics[2]["diagnostics"], json!([]));
    assert_eq!(diagnostics[3]["diagnostics"], json!([]));
    assert_eq!(output[9]["id"], 2);
    assert!(output[9].get("error").is_none());
}

#[test]
fn missing_shutdown_and_failed_output_are_failures() {
    assert!(!run(vec![initialize()], false).0);
    assert!(!run(vec![initialize()], true).0);
}

#[test]
fn help_names_the_readonly_workspace_and_content_lsp_entry() {
    let output = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args(["lsp", "--help"])
        .output()
        .expect("help");
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).expect("help UTF-8");
    assert!(help.contains("gazette lsp"));
    assert!(help.contains("workspace/content diagnostics"));
    assert!(help.contains("no files are changed"));
    assert!(!help.contains("no files are opened"));
}
