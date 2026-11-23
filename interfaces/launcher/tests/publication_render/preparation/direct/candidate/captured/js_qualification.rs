//! Actual Node addon consumes the same live source fixture and native qualification report.

use super::*;
use std::io::Write;
use std::process::{Command, Stdio};

#[test]
#[ignore = "requires explicit native addon provider qualification"]
fn javascript_qualification_preserves_native_complete_set_and_effect_evidence() {
    let node = std::env::var_os("GAZETTE_TEST_NODE").expect(
        "supply an installed Node executable via absolute GAZETTE_TEST_NODE; no skip or install",
    );
    let addon = std::env::var_os("GAZETTE_TEST_ADDON").expect(
        "supply the current built addon via absolute GAZETTE_TEST_ADDON; no skip or install",
    );
    assert!(Path::new(&node).is_absolute());
    assert!(Path::new(&addon).is_absolute());
    let fixture = policy_fixture(&[]);
    let parent = private_directory();
    let log = parent.path().join("qualification.jsonl");
    let (config, required) = source_configuration(&fixture, &log, "none");
    let candidate = serde_json::from_value(required).unwrap();
    let request = qualification::request(&config, &candidate);
    let before = fs::read(fixture.directory.path().join(".git/HEAD")).unwrap();
    let report = qualification::recipe(&request).execute(qualification::qualification_options());
    assert!(report.is_complete(), "{report:?}");
    let packet = json!({"request": request, "expected": report, "log": log});
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sdk/js/tests/qualification.mjs");
    let mut child = Command::new(node)
        .arg(script)
        .arg(addon)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start explicit local qualification carrier");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&packet).unwrap())
        .expect("complete fixture packet; closing stdin ends capture");
    let output = child
        .wait_with_output()
        .expect("native addon fixture completion");
    assert!(output.status.success(), "{output:?}");
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("Gazette JavaScript qualification proof passed"),
        "process exit alone is not behavioral proof"
    );
    qualification::assert_no_stage(parent.path(), &fixture, &before);
}
