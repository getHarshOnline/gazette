//! Real response-dependent tools/call lifecycle over the Gazette executable.

use super::{Command, Duration, Instant, Process, Stdio, Value, Write, json};
use gazette_sdk::{source::Sha256Digest, store};
use std::{
    fs,
    io::{BufRead, BufReader, Read},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::ChildStdin,
    sync::mpsc::{self, Receiver},
};

struct Client {
    process: Process,
    input: Option<ChildStdin>,
    output: Receiver<std::io::Result<String>>,
    sequence: u64,
    tools: Vec<Value>,
}

impl Client {
    fn new(directory: &Path) -> Self {
        let mut process = Process(
            Command::new(env!("CARGO_BIN_EXE_gazette"))
                .arg("mcp")
                .current_dir(directory)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("actual Gazette MCP process"),
        );
        let input = process.0.stdin.take().expect("request pipe");
        let reader = process.0.stdout.take().expect("response pipe");
        let (send, output) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            for line in BufReader::new(reader).lines() {
                if send.send(line).is_err() {
                    break;
                }
            }
        });
        let mut client = Self {
            process,
            input: Some(input),
            output,
            sequence: 0,
            tools: vec![],
        };
        let mut cursor: Option<String> = None;
        for page in 0..128 {
            let params = cursor
                .as_ref()
                .map_or_else(|| json!({}), |cursor| json!({"cursor":cursor}));
            let listed = client.request("tools/list", params);
            assert_eq!(listed["resultType"], "complete");
            assert_eq!(listed["ttlMs"], 0);
            assert_eq!(listed["cacheScope"], "private");
            let tools = listed["tools"].as_array().expect("actual discovered page");
            assert!(!tools.is_empty());
            assert!(tools.len() <= 8);
            client.tools.extend(tools.clone());
            let Some(next) = listed["nextCursor"].as_str() else {
                break;
            };
            assert_ne!(cursor.as_deref(), Some(next));
            cursor = Some(next.to_owned());
            assert!(page < 127, "catalog page ceiling");
        }
        client
    }

    fn request(&mut self, method: &str, mut params: Value) -> Value {
        self.sequence += 1;
        params["_meta"] = json!({
            "io.modelcontextprotocol/protocolVersion":"2026-07-28",
            "io.modelcontextprotocol/clientCapabilities":{}
        });
        if method == "tools/list" {
            params["_meta"]["io.rtepl.gazette/discovery"] = json!({"capabilities":["store"]});
        }
        let request = json!({"jsonrpc":"2.0","id":self.sequence,"method":method,"params":params});
        let input = self.input.as_mut().expect("open process input");
        serde_json::to_writer(&mut *input, &request).expect("complete request");
        input.write_all(b"\n").expect("frame");
        input.flush().expect("deliver request");
        let line = self
            .output
            .recv_timeout(Duration::from_secs(30))
            .expect("bounded native operation response")
            .expect("response read");
        let response: Value = serde_json::from_str(&line).expect("complete JSON response");
        assert_eq!(response["id"], self.sequence);
        assert!(response.get("error").is_none(), "{response}");
        response["result"].clone()
    }

    fn call(&mut self, name: &str, arguments: Value) -> Value {
        let tool = self
            .tools
            .iter()
            .find(|tool| tool["name"] == name)
            .expect("discoverable native operation")
            .clone();
        jsonschema::validator_for(&tool["inputSchema"])
            .expect("input schema")
            .validate(&arguments)
            .expect("actual request matches discovery");
        let result = self.request("tools/call", json!({"name":name,"arguments":arguments}));
        assert_eq!(result["isError"], false, "{result}");
        let report = result["structuredContent"].clone();
        jsonschema::validator_for(&tool["outputSchema"])
            .expect("output schema")
            .validate(&report)
            .expect("actual native result matches discovery");
        let readable: Value = serde_json::from_str(
            result["content"][0]["text"]
                .as_str()
                .expect("readable projection"),
        )
        .expect("complete readable JSON");
        assert_eq!(readable, report);
        report
    }

    fn finish(mut self) {
        self.input.take();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.process.0.try_wait().expect("process status") {
                assert!(status.success(), "{status}");
                break;
            }
            assert!(Instant::now() < deadline, "MCP did not finish after EOF");
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

fn directory(parent: &Path, name: &str) -> PathBuf {
    let path = parent.join(name);
    fs::create_dir(&path).expect("private test directory");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).expect("private mode");
    path
}

#[test]
fn actual_mcp_initialize_backup_verify_and_detached_restore_preserve_native_pins_and_bytes() {
    let root = tempfile::tempdir().expect("disposable native lifecycle");
    let base = root
        .path()
        .canonicalize()
        .expect("canonical private namespace");
    let journal = directory(&base, "journal");
    let backups = directory(&base, "backups");
    let restores = directory(&base, "restores");
    let cwd = directory(&base, "empty-cwd");
    let read = store::ReadOptions {
        allow_coordination_files: true,
        ..Default::default()
    };
    let mut client = Client::new(&cwd);
    let initialized = client.call(
        "initialize_store",
        json!({"directory":journal,
        "options":{"allow_create":true,"session":{"allow_local_reservation":true,"read":read}}}),
    );
    let database = PathBuf::from(
        initialized["database"]
            .as_str()
            .expect("actual new database"),
    );
    let original = fs::read(&database).expect("initial database bytes");
    let header: store::JournalHeader =
        serde_json::from_value(initialized["header"].clone()).expect("actual journal coordinates");
    assert_eq!(
        store::inspect_journal(&database, Some(&header), read)
            .expect("independent native readback")
            .header,
        header
    );
    let backup = client.call(
        "backup_store",
        json!({"source":database,"parent":backups,"allow_read":true,
        "authored_roots":[journal],"staged_roots":[journal],
        "published_roots":[journal],"live_journal_roots":[journal],
        "options":{"allow_create":true,"read":read}}),
    );
    let backup_directory =
        PathBuf::from(backup["directory"].as_str().expect("actual fresh backup"));
    let pin: Sha256Digest = serde_json::from_value(backup["manifest_sha256"].clone())
        .expect("original independent pin");
    assert_eq!(
        pin,
        Sha256Digest::of(&fs::read(backup_directory.join("backup.json")).expect("native manifest"))
    );
    let verified = client.call("verify_backup", json!({"directory":backup_directory,"manifest_sha256":pin,"allow_read":true,"options":{"read":read}}));
    assert_eq!(verified["snapshot"], backup["snapshot"]);
    assert_eq!(
        verified,
        json!(
            store::verify_backup(
                &backup_directory,
                pin,
                store::BackupVerifyOptions {
                    read,
                    ..Default::default()
                }
            )
            .expect("SDK parity")
        )
    );
    let restored = client.call(
        "prepare_store_restore",
        json!({"backup":backup_directory,"parent":restores,
        "manifest_sha256":pin,"allow_read":true,
        "authored_roots":[journal],"staged_roots":[journal],
        "published_roots":[journal],"live_journal_roots":[journal],
        "options":{"allow_prepare":true,"verification":{"read":read}}}),
    );
    let detached = PathBuf::from(
        restored["directory"]
            .as_str()
            .expect("detached recovery directory"),
    );
    assert_eq!(detached.parent(), Some(restores.as_path()));
    assert_ne!(detached, journal);
    assert_eq!(
        restored["preparation"]["source_manifest_sha256"],
        json!(pin)
    );
    assert_eq!(
        restored["receipt_sha256"],
        json!(Sha256Digest::of(
            &fs::read(detached.join("restore.json")).expect("exact restore evidence")
        ))
    );
    assert_eq!(
        fs::read(detached.join("store.sqlite")).expect("prepared copy"),
        fs::read(backup_directory.join("store.sqlite")).expect("pinned backup copy")
    );
    assert_eq!(
        fs::read(&database).expect("source still selected"),
        original
    );
    assert_eq!(
        store::inspect_journal(&database, Some(&header), read)
            .expect("no generation advance")
            .header,
        header
    );
    assert_eq!(
        fs::read_dir(backups).expect("single backup effect").count(),
        1
    );
    assert_eq!(
        fs::read_dir(restores)
            .expect("single detached effect")
            .count(),
        1
    );
    client.finish();
    assert!(
        fs::read_dir(cwd)
            .expect("no ambient workspace")
            .next()
            .is_none()
    );
}
