//! Private native prerequisite cases included by the fixture support library.

use super::*;
use std::io::Write;

#[test]
fn documented_baseline_accepts_exact_and_newer_releases() {
    for version in ["v22.18.0", "v22.18.1", "v22.19.0", "v23.0.0", "v24.16.0"] {
        assert!(supported_node_version(version), "{version}");
    }
}

#[test]
fn old_partial_malformed_and_prerelease_versions_fail_closed() {
    for version in [
        "v18.20.8",
        "v20.20.2",
        "v22.17.99",
        "v22.18",
        "22.18.0",
        "v22.18.0-rc.1",
        "v22.18.0\nextra",
        "v22.18.0.1",
        "vgarbage.18.0",
        "",
    ] {
        assert!(!supported_node_version(version), "{version}");
    }
}

#[test]
fn exact_selected_real_node_reports_its_version_and_meets_suite_prerequisite() {
    let selected = resolve("GAZETTE_TEST_NODE", "node");
    let version = node_version(&selected).expect("real selected executable version");
    qualify_node_runtime(&selected)
        .unwrap_or_else(|reason| panic!("selected {selected:?} reported {version:?}: {reason}"));
    assert!(supported_node_version(&version));
}

#[test]
fn a_selected_non_node_executable_is_not_replaced_by_path_node() {
    let selected = resolve("GAZETTE_TEST_FALSE", "false");
    let error = qualify_node_runtime(&selected).expect_err("exact non-Node selection fails");
    assert!(error.contains("version probe failed"), "{error}");
}

#[test]
fn closed_peer_output_remains_readable_without_socket_timeout_configuration() {
    let (mut output, mut writer) = UnixStream::pair().expect("native socket pair");
    writer.write_all(b"v24.16.0\n").expect("version bytes");
    drop(writer);
    assert_eq!(
        read_node_version(&mut output, Instant::now() + Duration::from_secs(2))
            .expect("closed peer retains its completed output"),
        "v24.16.0"
    );
}

#[test]
fn an_open_peer_cannot_turn_partial_output_into_a_completed_version() {
    let (mut output, mut writer) = UnixStream::pair().expect("native socket pair");
    writer.write_all(b"v24.16.0\n").expect("partial output");
    let deadline = Instant::now() + Duration::from_millis(50);
    let error = read_node_version(&mut output, deadline).expect_err("open peer remains incomplete");
    assert!(error.contains("deadline"), "{error}");
    assert!(Instant::now() >= deadline);
    drop(writer);
}

#[test]
fn output_accepts_the_ceiling_and_refuses_the_first_excess_byte() {
    for length in [128, 129] {
        let (mut output, mut writer) = UnixStream::pair().expect("native socket pair");
        writer
            .write_all(&vec![b'x'; length])
            .expect("bounded output");
        drop(writer);
        let result = read_node_version(&mut output, Instant::now() + Duration::from_secs(2));
        if length == 128 {
            assert_eq!(result.expect("exact ceiling accepted").len(), length);
        } else {
            let error = result.expect_err("excess byte refused");
            assert!(error.contains("128-byte output ceiling"), "{error}");
        }
    }
}
