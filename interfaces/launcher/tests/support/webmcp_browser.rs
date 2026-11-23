//! Bounded native readiness observation for the experimental `WebMCP` fixture only.

use std::{
    fs,
    io::Read,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const SCRIPT: &str = include_str!("webmcp_browser.mjs");
const MAXIMUM_BYTES: u64 = 2 << 20;

fn captured_text(path: &std::path::Path) -> String {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .expect("owned native evidence")
        .take(MAXIMUM_BYTES + 1)
        .read_to_end(&mut bytes)
        .expect("bounded native evidence read");
    assert!(
        bytes.len() as u64 <= MAXIMUM_BYTES,
        "bounded completed native evidence"
    );
    String::from_utf8(bytes).expect("native UTF-8 evidence")
}

pub(super) fn observe(chrome: &std::path::Path, url: &str) -> String {
    assert!(
        url.starts_with("http://127.0.0.1:"),
        "owned loopback preview only"
    );
    let node = gazette_test_support::node();
    let profile = tempfile::tempdir().expect("new owned Chrome profile");
    let browser_diagnostics = profile.path().join("browser.log");
    let output = profile.path().join("dom.html");
    let observer_diagnostics = profile.path().join("observer.log");
    let mut browser = crate::mcp_carrier::Process(
        Command::new(chrome)
            .args([
                "--headless",
                "--enable-features=WebMCP",
                "--remote-debugging-port=0",
                "--remote-debugging-address=127.0.0.1",
                "--no-first-run",
                "--no-default-browser-check",
                "--disable-background-networking",
                "--disable-component-update",
                "--disable-sync",
                "--disable-extensions",
                "--disable-features=DialMediaRouteProvider",
                "--metrics-recording-only",
                "--password-store=basic",
                "--use-mock-keychain",
            ])
            .arg(format!("--user-data-dir={}", profile.path().display()))
            .arg(url)
            .current_dir(profile.path())
            .env_clear()
            .env("TMPDIR", profile.path())
            .env("PATH", "/usr/bin:/bin")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(fs::File::create(&browser_diagnostics).expect("owned browser diagnostics"))
            .spawn()
            .expect("explicit native Chrome"),
    );
    let mut observer = crate::mcp_carrier::Process(
        Command::new(node)
            .args(["--input-type=module", "--eval", SCRIPT, "--"])
            .arg(url)
            .arg(profile.path())
            .current_dir(profile.path())
            .env_clear()
            .stdin(Stdio::null())
            .stdout(fs::File::create(&output).expect("owned native DOM evidence"))
            .stderr(fs::File::create(&observer_diagnostics).expect("owned observer diagnostics"))
            .spawn()
            .expect("qualified native Node WebSocket observer"),
    );
    let deadline = Instant::now() + Duration::from_secs(45);
    let status = loop {
        for path in [&output, &browser_diagnostics, &observer_diagnostics] {
            assert!(
                fs::metadata(path).unwrap().len() <= MAXIMUM_BYTES,
                "bounded native evidence at {path:?}"
            );
        }
        if let Some(status) = observer.0.try_wait().expect("observer status") {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "native WebMCP observation exceeded its deadline"
        );
        assert!(
            browser.0.try_wait().expect("browser status").is_none(),
            "native browser exited before observation"
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(
        status.success(),
        "native WebMCP readiness failed: {}",
        captured_text(&observer_diagnostics)
    );
    assert!(
        fs::metadata(&browser_diagnostics).unwrap().len() <= MAXIMUM_BYTES,
        "bounded completed browser diagnostics"
    );
    let dom = captured_text(&output);
    assert!(dom.len() as u64 <= MAXIMUM_BYTES, "bounded completed DOM");
    assert!(
        dom.starts_with("<!DOCTYPE html>") && dom.ends_with("</html>"),
        "complete native document, not a fabricated result"
    );
    // The guards terminate/reap this exact observer and browser before TempDir removes its profile.
    drop(observer);
    drop(browser);
    dom
}
