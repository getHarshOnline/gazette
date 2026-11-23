//! Executable caller-selected budgets are validated before MCP service startup.

use std::process::{Command, Stdio};

#[test]
fn mcp_help_names_explicit_budgets_and_unchanged_defaults() {
    let output = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args(["mcp", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).unwrap();
    for expected in [
        "--max-frame-bytes",
        "--max-response-bytes",
        "1048576",
        "262144",
        "16777216",
    ] {
        assert!(help.contains(expected), "{help}");
    }
}

#[test]
fn invalid_transport_budgets_refuse_before_reading_any_mcp_frame() {
    for (flag, value, reason) in [
        ("--max-frame-bytes", "4095", "max-frame-bytes"),
        ("--max-frame-bytes", "16777217", "max-frame-bytes"),
        ("--max-response-bytes", "1023", "max-response-bytes"),
        ("--max-response-bytes", "16777217", "max-response-bytes"),
    ] {
        // Keep stdin open and unused: validation must not enter the service and
        // wait for frames. Child custody is retained until the bounded exit.
        let mut child = Command::new(env!("CARGO_BIN_EXE_gazette"))
            .args(["mcp", flag, value])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            if std::time::Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("invalid MCP budget waited for ingress");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        drop(input);
        let output = child.wait_with_output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty(), "no protocol started");
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(
            error.contains(reason) && error.contains("between"),
            "{error}"
        );
    }
}

#[test]
fn default_and_explicit_bounded_transports_accept_ordinary_eof() {
    for options in [
        vec![],
        vec![
            "--max-frame-bytes",
            "2097152",
            "--max-response-bytes",
            "524288",
        ],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_gazette"))
            .arg("mcp")
            .args(options)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(output.status.success(), "{:?}", output.stderr);
        assert!(output.stdout.is_empty());
    }
}
