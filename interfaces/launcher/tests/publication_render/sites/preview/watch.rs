//! Actual watched child processes, bounded event waits and real HTTP generation reads.

use std::{io, path::Path, sync::mpsc, thread};

use super::*;

struct Watch {
    child: Child,
    events: Option<mpsc::Receiver<io::Result<Value>>>,
    reader: Option<thread::JoinHandle<()>>,
}

impl Watch {
    fn start(input: &Value, local: bool) -> Self {
        let mut arguments = vec!["--watch", "--watch-interval-ms", "100"];
        if local {
            arguments.push("--local-urls");
        }
        let (child, mut output) = Process::spawn(input, &arguments);
        let (sender, events) = mpsc::sync_channel(8);
        let reader = thread::spawn(move || {
            loop {
                let mut line = String::new();
                let event = match output.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => serde_json::from_str(&line).map_err(io::Error::other),
                    Err(error) => Err(error),
                };
                if sender.send(event).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            events: Some(events),
            reader: Some(reader),
        }
    }

    fn next(&self, expected: &str) -> Value {
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        loop {
            let event = self
                .events
                .as_ref()
                .expect("live events")
                .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                .expect("watch event arrives within bounded deadline")
                .expect("valid JSONL watch event");
            if expected != "rebuild_failed" && event["event"] == "rebuild_failed" {
                // Polling can observe the same already-reported failed candidate
                // again before a subsequent exact restoration is acquired.
                continue;
            }
            assert_eq!(
                event["event"], expected,
                "unexpected watch transition: {event}"
            );
            return event;
        }
    }

    fn close(&mut self) {
        drop(self.child.stdin.take().expect("EOF close input"));
        self.next("closed");
        assert!(
            self.child
                .wait()
                .expect("watch process exits after close")
                .success()
        );
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        drop(self.events.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

fn replace(root: &Path, target: &Path, bytes: &[u8]) {
    let staged = root.join("next-watch-input");
    fs::write(&staged, bytes).expect("complete test-owned atomic-save bytes");
    fs::rename(staged, target).expect("atomic replacement within disposable fixture");
}

fn model(socket: SocketAddr, mount: &str) -> Value {
    let response = response(socket, "GET", &format!("{mount}model.json"));
    serde_json::from_slice(body(&response)).expect("actual served machine model")
}

fn page(socket: SocketAddr, path: &str) -> Vec<u8> {
    body(&response(socket, "GET", path)).to_owned()
}

fn same_bindings(before: &Value, after: &Value) {
    for site in SITES {
        assert_eq!(
            before["sites"][site]["address"],
            after["sites"][site]["address"]
        );
        assert_eq!(before["sites"][site]["url"], after["sites"][site]["url"]);
    }
}

#[test]
fn configured_build_selection_survives_real_watch_generations_and_http_bytes() {
    let fixture = Fixture::new(&[]);
    let root = fixture.directory.path();
    let source = root.join("one/page.md");
    let original = fs::read_to_string(&source).expect("authored source");
    let configured = super::super::configured::configuration(&fixture, "include");
    let input = super::super::configured::recipe(&configured);
    let mut process = Watch::start(&input, true);
    let ready = process.next("ready");
    let selected = json!({
        "origin":"workspace","sites":["site:other","site:one"],"local_drafts":"include"
    });
    assert_eq!(ready["build_selection"], selected);
    let address = address(&ready, "site:one");
    assert!(
        std::str::from_utf8(&page(address, "/mount/guide/index.html"))
            .expect("first actual HTML")
            .contains("Retained")
    );
    let changed = original.replace("Retained", "Configured watch replacement");
    replace(root, &source, changed.as_bytes());
    let event = process.next("changed");
    assert_eq!(event["build_selection"], selected);
    assert_eq!(event["generation"], 2);
    same_bindings(&ready, &event);
    assert!(
        std::str::from_utf8(&page(address, "/mount/guide/index.html"))
            .expect("replacement actual HTML")
            .contains("Configured watch replacement")
    );
    assert_ne!(ready["input_identity"], event["input_identity"]);
    process.close();
}

#[test]
fn dirty_content_theme_and_failed_complete_generation_are_observable_over_http() {
    let fixture = Fixture::new(&[]);
    let root = fixture.directory.path();
    let source = root.join("one/page.md");
    let template_path = root.join("theme/templates/page.html");
    let original = fs::read_to_string(&source).expect("initial source");
    let template = fs::read_to_string(&template_path).expect("initial Theme");
    let dirty = original.replace("Retained", "Initial dirty");
    replace(root, &source, dirty.as_bytes());
    let config = fixture.config();
    let input = request(&config, &SITES);
    let mut process = Watch::start(&input, true);
    let ready = process.next("ready");
    assert_eq!(ready["input_mode"], "dirty-content-addressed");
    assert_eq!(ready["generation"], 1);
    assert_eq!(ready["stale"], false);
    assert_eq!(ready["status"], "local-cross-site-noncertified");
    let one = address(&ready, "site:one");
    let other = address(&ready, "site:other");
    assert!(
        std::str::from_utf8(&page(one, "/mount/guide/index.html"))
            .expect("dirty page")
            .contains("Initial dirty")
    );
    assert!(
        model(one, "/mount/")["pages"]["content:one"]["body_text"]
            .as_str()
            .expect("dirty machine text")
            .contains("Initial dirty")
    );
    let changed = original.replace("Retained", "Changed content");
    replace(root, &source, changed.as_bytes());
    let content_event = process.next("changed");
    assert_eq!(content_event["generation"], 2);
    same_bindings(&ready, &content_event);
    assert_ne!(ready["input_identity"], content_event["input_identity"]);
    assert!(
        std::str::from_utf8(&page(one, "/mount/guide/index.html"))
            .expect("changed page")
            .contains("Changed content")
    );
    assert!(
        model(one, "/mount/")["pages"]["content:one"]["body_text"]
            .as_str()
            .expect("changed model")
            .contains("Changed content")
    );
    let changed_template = format!("<aside>Changed Theme</aside>{template}");
    replace(root, &template_path, changed_template.as_bytes());
    let current = process.next("changed");
    assert_eq!(current["generation"], 3);
    same_bindings(&ready, &current);
    let one_before = page(one, "/mount/guide/index.html");
    let other_before = page(other, "/elsewhere/index.html");
    for output in [&one_before, &other_before] {
        assert!(
            std::str::from_utf8(output)
                .expect("both Theme outputs")
                .contains("Changed Theme")
        );
    }
    let one_model = model(one, "/mount/");
    let other_model = model(other, "/");
    assert_eq!(
        one_model["pages"]["content:one"]["route"]["canonical_url"],
        format!("http://{one}/mount/guide/")
    );
    assert_eq!(
        other_model["pages"]["content:other"]["route"]["canonical_url"],
        format!("http://{other}/elsewhere/")
    );
    // One candidate site can render, but the later site's missing variable must
    // prevent replacement of either previously complete served output.
    let broken = format!(
        "<b>Must not leak</b>{{% if site.id == 'site:other' %}}{{{{ missing_watch_value }}}}{{% endif %}}{changed_template}"
    );
    replace(root, &template_path, broken.as_bytes());
    let failed = process.next("rebuild_failed");
    assert_eq!(failed["generation"], 3);
    assert_eq!(failed["stale"], true);
    assert_eq!(failed["code"], "preview_incomplete_generation");
    assert_eq!(page(one, "/mount/guide/index.html"), one_before);
    assert_eq!(page(other, "/elsewhere/index.html"), other_before);
    assert_eq!(model(one, "/mount/"), one_model);
    assert_eq!(model(other, "/"), other_model);
    replace(root, &template_path, changed_template.as_bytes());
    let recovered = process.next("recovered");
    assert_eq!(recovered["generation"], 3);
    assert_eq!(recovered["stale"], false);
    assert_eq!(recovered["input_identity"], current["input_identity"]);
    assert_eq!(recovered["sites"], current["sites"]);
    assert_eq!(page(one, "/mount/guide/index.html"), one_before);
    assert_eq!(page(other, "/elsewhere/index.html"), other_before);
    assert_eq!(model(one, "/mount/"), one_model);
    assert_eq!(model(other, "/"), other_model);
    process.close();
    for socket in [one, other] {
        TcpListener::bind(socket).expect("watch closes held sockets");
    }
    assert_eq!(
        fixture.config(),
        config,
        "watch does not rewrite fixed bindings or pins"
    );
    assert_eq!(
        fs::read_to_string(source).expect("source remains user's latest edit"),
        changed
    );
    assert_eq!(
        fs::read_to_string(template_path).expect("Theme remains user's latest edit"),
        changed_template
    );
}

#[test]
fn rename_delete_and_recreate_rebuild_without_changing_the_initial_workspace() {
    let fixture = Fixture::new(&[]);
    let root = fixture.directory.path();
    let source = root.join("one/page.md");
    let moved = root.join("one/moved.md");
    let original = fs::read(&source).expect("original page bytes");
    let config = fixture.config();
    let config_path = root.join("workspace.toml");
    fs::write(&config_path, &config).expect("test's unselected workspace file");
    let mut process = Watch::start(&request(&config, &SITES), false);
    let ready = process.next("ready");
    assert_eq!(ready["status"], "local-production-linked-noncertified");
    let one = address(&ready, "site:one");
    let other = address(&ready, "site:other");
    fs::write(&config_path, b"not the selected stdin workspace").expect("unselected config edit");
    fs::rename(&source, &moved).expect("rename exact test-owned source");
    let renamed = process.next("changed");
    same_bindings(&ready, &renamed);
    assert_eq!(
        renamed["input_identity"]["workspace"],
        ready["input_identity"]["workspace"]
    );
    assert_eq!(
        model(one, "/mount/")["pages"]["content:one"]["route"]["canonical_url"],
        "https://one.example/mount/guide/"
    );
    fs::remove_file(&moved).expect("delete only the renamed disposable page");
    let deleted = process.next("changed");
    same_bindings(&ready, &deleted);
    assert!(model(one, "/mount/")["pages"].get("content:one").is_none());
    assert!(response(one, "GET", "/mount/guide/index.html").starts_with(b"HTTP/1.1 404 "));
    assert_eq!(
        model(other, "/")["pages"]["content:other"]["route"]["canonical_url"],
        "https://other.example/elsewhere/"
    );
    replace(root, &source, &original);
    let recreated = process.next("changed");
    same_bindings(&ready, &recreated);
    assert_eq!(recreated["input_identity"], ready["input_identity"]);
    assert_eq!(
        model(one, "/mount/")["pages"]["content:one"]["route"]["canonical_url"],
        "https://one.example/mount/guide/"
    );
    assert!(
        std::str::from_utf8(&page(one, "/mount/guide/index.html"))
            .expect("recreated page")
            .contains("Retained")
    );
    process.close();
    assert_eq!(
        fs::read_to_string(config_path).expect("unselected config retained"),
        "not the selected stdin workspace"
    );
    for socket in [one, other] {
        TcpListener::bind(socket).expect("fixed listeners released");
    }
}

#[test]
fn invalid_watch_intervals_fail_before_reading_the_initial_request() {
    for interval in ["0", "99", "3600001", "18446744073709551615"] {
        let mut args = ARGUMENTS.to_vec();
        args.extend(["--watch", "--watch-interval-ms", interval]);
        let result = cli("not valid JSON or workspace", &args);
        assert!(!result.status.success());
        assert!(result.stdout.is_empty());
        assert!(String::from_utf8_lossy(&result.stderr).contains("preview_watch_options"));
    }
    let mut args = ARGUMENTS.to_vec();
    args.extend(["--watch-interval-ms", "100"]);
    let result = cli("not valid JSON or workspace", &args);
    assert!(!result.status.success());
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains("--watch"));
}
