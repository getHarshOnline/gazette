//! Real process-local ordinary store lifetimes over the same native SDK providers.

use gazette_sdk::store;
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    os::unix::fs::PermissionsExt,
    process::{ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

#[path = "support/mcp.rs"]
mod carrier;
use carrier::{Process, exchange};

fn call(
    input: &mut ChildStdin,
    output: &Receiver<std::io::Result<String>>,
    id: u64,
    name: &str,
    arguments: Value,
) -> Value {
    let request = json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":arguments,"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}});
    serde_json::to_writer(&mut *input, &request).expect("request encoding");
    input.write_all(b"\n").expect("request frame");
    input.flush().expect("request delivery");
    let line = output
        .recv_timeout(Duration::from_secs(10))
        .expect("bounded MCP response")
        .expect("response bytes");
    let response: Value = serde_json::from_str(&line).expect("JSON-only stdout");
    assert_eq!(response["id"], id);
    response
}

fn success(response: &Value) -> &Value {
    assert!(response.get("error").is_none(), "{response}");
    assert_ne!(response["result"]["isError"], true, "{response}");
    &response["result"]["structuredContent"]
}

#[test]
fn ordinary_connection_and_writer_lifetimes_match_sdk_without_release_authority() {
    let root = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .expect("private disposable journal");
    let cwd = tempfile::tempdir().expect("isolated process cwd");
    let read = store::ReadOptions {
        allow_coordination_files: true,
        ..Default::default()
    };
    let session = store::SessionOptions {
        allow_local_reservation: true,
        read,
    };
    let writer = store::WriterOptions {
        allow_generation_advance: true,
        session,
    };
    let initialized = store::initialize(
        root.path(),
        store::InitializeOptions {
            allow_create: true,
            session,
            ..Default::default()
        },
    )
    .expect("native journal initialization");
    let mut sdk_connection =
        store::connect(&initialized.database, root.path(), session).expect("SDK connection");
    let connection_observation = json!(sdk_connection.observe().expect("SDK connection readiness"));
    sdk_connection
        .close()
        .expect("SDK connection explicit close");

    let mut child = Process(
        Command::new(env!("CARGO_BIN_EXE_gazette"))
            .arg("mcp")
            .current_dir(cwd.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("real Gazette MCP"),
    );
    let mut input = child.0.stdin.take().expect("stdin");
    let stdout = child.0.stdout.take().expect("stdout");
    let (sender, output) = mpsc::sync_channel(1);
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if sender.send(line).is_err() {
                break;
            }
        }
    });
    let connection_request = json!({"request":{"kind":"connection","path":initialized.database,"coordination_root":root.path(),"allow_native_reads":true,"options":session}});
    let connected = call(
        &mut input,
        &output,
        1,
        "open_store_session",
        connection_request,
    );
    let connected = success(&connected);
    assert_eq!(connected["status"], "connected");
    assert_eq!(connected["active"], true);
    assert_eq!(connected["observation"], connection_observation);
    let connection_handle = connected["session"]
        .as_str()
        .expect("connection handle")
        .to_owned();
    let contended = store::connect(&initialized.database, root.path(), session)
        .err()
        .expect("held native reservation");
    let observed = call(
        &mut input,
        &output,
        2,
        "inspect_store_session",
        json!({"session":connection_handle}),
    );
    assert_eq!(success(&observed)["observation"], connection_observation);
    assert_eq!(success(&observed)["status"], "observed");
    let competing_connection = call(
        &mut input,
        &output,
        3,
        "open_store_session",
        json!({"request":{"kind":"connection","path":initialized.database,"coordination_root":root.path(),"allow_native_reads":true,"options":session}}),
    );
    assert_eq!(competing_connection["result"]["isError"], true);
    assert_eq!(
        competing_connection["result"]["structuredContent"]["failure"]["code"],
        contended.code()
    );
    let closed = call(
        &mut input,
        &output,
        4,
        "close_store_session",
        json!({"session":connection_handle}),
    );
    assert_eq!(success(&closed)["status"], "closed");
    assert_eq!(success(&closed)["active"], false);
    store::connect(&initialized.database, root.path(), session)
        .expect("close released reservation")
        .close()
        .expect("close independent probe");
    assert_eq!(
        store::inspect_journal(&initialized.database, None, read)
            .expect("read header")
            .header,
        initialized.header,
        "connection lifetimes never advance generation"
    );

    let mut sdk_writer = store::acquire_writer(root.path(), initialized.header, writer)
        .expect("independent SDK writer baseline");
    let reference = sdk_writer.inspect().expect("SDK writer readiness");
    let baseline = reference.header;
    assert_eq!(baseline.writer_fence, initialized.header.writer_fence + 1);
    sdk_writer.close().expect("SDK writer explicit close");
    let request = json!({"request":{"kind":"writer","root":root.path(),"expected_header":baseline,"allow_native_reads":true,"options":writer}});
    let mut wrong = baseline;
    wrong.writer_fence += 10;
    let expected_failure = store::acquire_writer(root.path(), wrong, writer)
        .err()
        .expect("SDK refuses wrong expectation");
    let mut wrong_request = request.clone();
    wrong_request["request"]["expected_header"] = json!(wrong);
    let refused = call(&mut input, &output, 5, "open_store_session", wrong_request);
    assert_eq!(refused["result"]["isError"], true);
    assert_eq!(
        refused["result"]["structuredContent"]["failure"]["code"],
        expected_failure.cause.code()
    );
    assert_eq!(
        refused["result"]["structuredContent"]["failure"]["generation"],
        json!(store::GenerationEffect::NotAttempted)
    );
    assert_eq!(refused["result"]["structuredContent"]["active"], false);
    for (id, pointer) in [
        (6, "/request/allow_native_reads"),
        (7, "/request/options/allow_generation_advance"),
        (8, "/request/options/session/allow_local_reservation"),
        (9, "/request/options/session/read/allow_coordination_files"),
    ] {
        let mut denied = request.clone();
        *denied.pointer_mut(pointer).expect("permission field") = json!(false);
        assert_eq!(
            call(&mut input, &output, id, "open_store_session", denied)["error"]["code"],
            -32602
        );
        assert_eq!(
            store::inspect_journal(&initialized.database, None, read)
                .expect("denial preserves generation")
                .header,
            baseline
        );
    }
    let opened = call(&mut input, &output, 10, "open_store_session", request);
    let opened = success(&opened);
    assert_eq!(opened["status"], "acquired");
    assert_eq!(opened["active"], true);
    let handle = opened["session"]
        .as_str()
        .expect("writer handle")
        .to_owned();
    let acquired: store::JournalHeader =
        serde_json::from_value(opened["inspection"]["header"].clone())
            .expect("actual native acquired coordinates");
    assert_eq!(
        acquired,
        store::JournalHeader {
            writer_fence: baseline.writer_fence + 1,
            ..baseline
        }
    );
    let mut expected_inspection = json!(reference);
    expected_inspection["header"] = json!(acquired);
    assert_eq!(opened["inspection"], expected_inspection);
    for id in [11, 12, 13] {
        let inspected = call(
            &mut input,
            &output,
            id,
            "inspect_store_session",
            json!({"session":handle}),
        );
        assert_eq!(success(&inspected)["inspection"], expected_inspection);
        assert_eq!(success(&inspected)["status"], "inspected");
    }
    let blocked = store::acquire_writer(root.path(), acquired, writer)
        .err()
        .expect("same held writer prevents reacquisition");
    assert_eq!(blocked.generation, store::GenerationEffect::NotAttempted);
    let blocked_mcp = call(
        &mut input,
        &output,
        14,
        "open_store_session",
        json!({"request":{"kind":"writer","root":root.path(),"expected_header":acquired,"allow_native_reads":true,"options":writer}}),
    );
    assert_eq!(blocked_mcp["result"]["isError"], true);
    assert_eq!(
        blocked_mcp["result"]["structuredContent"]["failure"]["code"],
        blocked.cause.code()
    );
    let release = call(
        &mut input,
        &output,
        15,
        "close_release_session",
        json!({"session":handle}),
    );
    assert_eq!(release["result"]["isError"], true);
    assert_eq!(
        release["result"]["structuredContent"]["code"],
        "release_session_unknown"
    );
    let still_held = call(
        &mut input,
        &output,
        16,
        "inspect_store_session",
        json!({"session":handle}),
    );
    assert_eq!(success(&still_held)["inspection"], expected_inspection);
    let closed = call(
        &mut input,
        &output,
        17,
        "close_store_session",
        json!({"session":handle}),
    );
    assert_eq!(success(&closed)["status"], "closed");
    assert_eq!(success(&closed)["active"], false);
    let gone = call(
        &mut input,
        &output,
        18,
        "inspect_store_session",
        json!({"session":handle}),
    );
    assert_eq!(
        gone["result"]["structuredContent"]["code"],
        "store_session_unknown"
    );
    assert_eq!(
        store::inspect_journal(&initialized.database, None, read)
            .expect("fresh read-back")
            .header,
        acquired,
        "inspection, contention, wrong-registry and close do not reacquire generation"
    );
    let mut next = store::acquire_writer(root.path(), acquired, writer)
        .expect("explicit close released native writer");
    let next_header = next.inspect().expect("new independent readiness").header;
    assert_eq!(next_header.writer_fence, acquired.writer_fence + 1);
    next.close().expect("independent writer close");

    let held_at_eof = call(
        &mut input,
        &output,
        19,
        "open_store_session",
        json!({"request":{"kind":"writer","root":root.path(),"expected_header":next_header,"allow_native_reads":true,"options":writer}}),
    );
    let held_at_eof = success(&held_at_eof);
    assert_eq!(held_at_eof["active"], true);
    let dropped_handle = held_at_eof["session"]
        .as_str()
        .expect("held lifetime")
        .to_owned();
    let eof_header: store::JournalHeader =
        serde_json::from_value(held_at_eof["inspection"]["header"].clone())
            .expect("exact delivered generation before EOF");
    assert_eq!(eof_header.writer_fence, next_header.writer_fence + 1);
    assert!(
        store::acquire_writer(root.path(), eof_header, writer).is_err(),
        "writer remains held immediately before EOF"
    );
    // Deliberately no close request: EOF releases resources, not a successful-close receipt.
    drop(input);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.0.try_wait().expect("process status") {
            assert!(status.success(), "{status}");
            break;
        }
        assert!(Instant::now() < deadline, "MCP did not exit after EOF");
        std::thread::sleep(Duration::from_millis(10));
    }
    reader.join().expect("response reader");
    let mut stderr = String::new();
    child
        .0
        .stderr
        .take()
        .expect("stderr")
        .read_to_string(&mut stderr)
        .expect("stderr bytes");
    assert!(stderr.is_empty(), "{stderr}");
    assert!(fs::read_dir(cwd.path()).expect("cwd").next().is_none());
    assert!(
        output.try_recv().is_err(),
        "EOF fabricated no close response"
    );
    assert_eq!(
        store::inspect_journal(&initialized.database, Some(&eof_header), read)
            .expect("EOF preserves advanced native coordinates")
            .header,
        eof_header
    );
    let mut after_eof = store::acquire_writer(root.path(), eof_header, writer)
        .expect("service drop released ordinary native reservation");
    assert_eq!(
        after_eof
            .inspect()
            .expect("independent post-EOF acquisition")
            .header
            .writer_fence,
        eof_header.writer_fence + 1
    );
    after_eof
        .close()
        .expect("explicit close of independent post-EOF writer");
    let fresh_process = exchange(vec![
        json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"inspect_store_session","arguments":{"session":dropped_handle}}}),
    ]);
    assert_eq!(
        fresh_process[0]["result"]["structuredContent"]["code"],
        "store_session_unknown"
    );
}
