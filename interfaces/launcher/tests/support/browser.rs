//! Shared real Chrome capture over owned Gazette loopback previews.

use gazette_sdk::html::{self, TextKind};
use std::{
    convert::Infallible,
    fs,
    io::Read,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use tempfile::tempdir;

const MAXIMUM_CAPTURE_BYTES: u64 = 2 << 20;
const CAPTURE_SETTLE: Duration = Duration::from_millis(200);

fn complete_dom(bytes: &[u8]) -> bool {
    std::str::from_utf8(bytes).is_ok_and(|dom| dom.ends_with("</html>\n"))
}

pub(crate) fn chrome() -> PathBuf {
    let path = PathBuf::from(std::env::var_os("GAZETTE_TEST_CHROME").expect(
        "set GAZETTE_TEST_CHROME to the explicitly qualified native Chrome executable; browser proof cannot be skipped",
    ));
    assert!(
        path.is_absolute() && path.is_file(),
        "explicit native Chrome executable"
    );
    path
}

pub(crate) fn dump(chrome: &std::path::Path, url: &str, javascript: bool) -> String {
    dump_with_flags(chrome, url, javascript, &[])
}

/// Explicit per-invocation experimental features; never changes the installed browser.
pub(crate) fn dump_with_flags(
    chrome: &std::path::Path,
    url: &str,
    javascript: bool,
    flags: &[&str],
) -> String {
    assert!(
        url.starts_with("http://127.0.0.1:"),
        "only owned loopback preview"
    );
    let profile = tempdir().expect("new private Chrome profile");
    let stdout = profile.path().join("dom.html");
    let stderr = profile.path().join("chrome.log");
    if !javascript {
        let defaults = profile.path().join("Default");
        fs::create_dir(&defaults).expect("private Chrome profile defaults");
        fs::write(
            defaults.join("Preferences"),
            br#"{"profile":{"default_content_setting_values":{"javascript":2}}}"#,
        )
        .expect("private Chrome JavaScript preference");
    }
    let mut command = Command::new(chrome);
    // Native documented DOM serialization executes page scripts before capture:
    // https://developer.chrome.com/docs/automation-and-testing/headless-cli
    command
        .args([
            "--headless",
            "--dump-dom",
            "--timeout=10000",
            "--virtual-time-budget=5000",
            "--no-first-run",
            "--no-default-browser-check",
            "--disable-background-networking",
            "--disable-component-update",
            "--disable-sync",
            "--disable-extensions",
            "--metrics-recording-only",
            "--password-store=basic",
            "--use-mock-keychain",
        ])
        .arg(format!("--user-data-dir={}", profile.path().display()));
    command.args(flags);
    command
        .arg(url)
        .current_dir(profile.path())
        .env_clear()
        .env("HOME", profile.path())
        .env("TMPDIR", profile.path())
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::null())
        .stdout(fs::File::create(&stdout).expect("private DOM output"))
        .stderr(fs::File::create(&stderr).expect("private browser diagnostic"));
    let mut process = crate::mcp_carrier::Process(command.spawn().expect("explicit native Chrome"));
    let deadline = Instant::now() + Duration::from_secs(90);
    let mut candidate: Option<(Vec<u8>, Instant)> = None;
    let status = loop {
        if let Some(status) = process.0.try_wait().expect("browser status") {
            break Some(status);
        }
        let bytes = fs::read(&stdout).expect("private DOM capture");
        assert!(
            bytes.len() as u64 <= MAXIMUM_CAPTURE_BYTES,
            "bounded DOM evidence"
        );
        if complete_dom(&bytes) {
            match &candidate {
                Some((previous, since))
                    if previous == &bytes && since.elapsed() >= CAPTURE_SETTLE =>
                {
                    if let Some(status) = process.0.try_wait().expect("browser status") {
                        break Some(status);
                    }
                    process
                        .0
                        .kill()
                        .expect("stop Chrome after complete DOM capture");
                    process
                        .0
                        .wait()
                        .expect("reap Chrome after complete DOM capture");
                    assert_eq!(
                        fs::read(&stdout).expect("final private DOM capture"),
                        bytes,
                        "DOM capture changed while stopping completed Chrome"
                    );
                    break None;
                }
                Some((previous, _)) if previous == &bytes => {}
                _ => candidate = Some((bytes, Instant::now())),
            }
        } else {
            candidate = None;
        }
        if Instant::now() >= deadline {
            let mut diagnostics = Vec::new();
            fs::File::open(&stderr)
                .expect("owned browser diagnostics")
                .take(16 << 10)
                .read_to_end(&mut diagnostics)
                .expect("bounded browser diagnostic read");
            panic!(
                "native Chrome did not complete bounded DOM capture; pid={}, DOM bytes={:?}, stderr bytes={:?}, bounded stderr={}",
                process.0.id(),
                fs::metadata(&stdout).map(|value| value.len()),
                fs::metadata(&stderr).map(|value| value.len()),
                String::from_utf8_lossy(&diagnostics),
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(
        fs::metadata(&stderr).unwrap().len() <= MAXIMUM_CAPTURE_BYTES,
        "bounded browser diagnostics"
    );
    let externally_stopped = status.is_none();
    if let Some(status) = status {
        assert!(status.success(), "{}", fs::read_to_string(&stderr).unwrap());
    }
    assert!(
        fs::metadata(&stdout).unwrap().len() <= MAXIMUM_CAPTURE_BYTES,
        "bounded DOM evidence"
    );
    let dom = fs::read_to_string(stdout).expect("actual browser DOM serialization");
    assert!(
        !externally_stopped || complete_dom(dom.as_bytes()),
        "complete native Chrome DOM serialization"
    );
    dom
}

pub(crate) fn values(dom: &str, selected: TextKind) -> Vec<String> {
    let mut values = Vec::new();
    html::inspect(dom.as_bytes(), html::Limits::default(), |kind, value| {
        if kind == selected {
            values.push(value.to_owned());
        }
        Ok::<_, Infallible>(())
    })
    .expect("native HTML decoding of browser output");
    values
}
