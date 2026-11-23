//! Real JavaScript binding shares the native pinned-output and HTTP fixture.

use super::*;

#[test]
#[ignore = "requires explicit native addon provider qualification"]
fn native_addon_preserves_complete_delivery_reports_and_effect_boundaries() {
    let node = PathBuf::from(std::env::var_os("GAZETTE_TEST_NODE").expect(
        "supply the installed Node executable via absolute GAZETTE_TEST_NODE; no skip or install",
    ));
    let addon = PathBuf::from(std::env::var_os("GAZETTE_TEST_ADDON").expect(
        "supply the current built addon via absolute GAZETTE_TEST_ADDON; no skip or install",
    ));
    assert!(node.is_absolute() && node.is_file());
    assert!(addon.is_absolute() && addon.is_file());
    let server = Server::new();
    let (_directory, request) = request(
        &server,
        &["/", "/different.txt", "/missing.txt", "/unavailable.txt"],
    );
    let typed = delivery::Request::parse(request.to_string().as_bytes(), 1 << 20).unwrap();
    let expected = delivery::observe_retained(&typed).expect("complete native reference");
    assert!(!expected.matches_selected_routes());
    let mut successful = request.clone();
    successful["selection"]["routes"] = json!(["/"]);
    let success_expected = delivery::observe_retained(
        &delivery::Request::parse(successful.to_string().as_bytes(), 1 << 20).unwrap(),
    )
    .expect("matching native reference");
    assert!(success_expected.matches_selected_routes());
    assert_eq!(server.count(), 5, "native references have exact effects");
    let packet = json!({"request":request,"expected":expected,"success_expected":success_expected});
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../sdk/js/tests/delivery.mjs");
    let mut child = carrier::Process(
        Command::new(node)
            .arg(script)
            .arg(addon)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("explicit local addon host"),
    );
    child
        .0
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&packet).unwrap())
        .expect("complete native fixture packet");
    let mut stdout = String::new();
    let mut stderr = String::new();
    child
        .0
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut stdout)
        .unwrap();
    child
        .0
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    let status = child.0.wait().expect("reaped addon host");
    assert!(status.success(), "{status}: {stdout}\n{stderr}");
    assert!(stdout.contains("Gazette JavaScript delivery proof passed"));
    assert_eq!(
        server.count(),
        14,
        "four mixed, one matching and four overflow observations; refusals never request or retry"
    );
    assert_eq!(
        source::inspect(&typed.tree, None, typed.source_limits)
            .unwrap()
            .identity(),
        typed.tree_identity,
        "HTTP evidence and host failures cannot modify retained output"
    );
}
