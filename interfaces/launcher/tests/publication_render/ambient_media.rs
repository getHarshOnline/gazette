//! Retained MP4 transport is independent of unsupported video qualification.
use super::*;
use std::collections::BTreeMap;

#[test]
fn mp4_has_native_resolution_and_exact_output_without_claiming_policy_coverage() {
    let clip = b"opaque retained MP4 fixture";
    let page = format!("{PAGE}\n[Film](media/clip.mp4)\n");
    let fixture = Fixture::new(&[
        ("one/page.md", page.as_bytes()),
        ("one/media/clip.mp4", clip),
    ]);
    let config = fixture.config().replace(
        "target='one'}]",
        "target='one',static_assets={route_prefix='/media/'}}]",
    );
    let inputs = acquire(&config);
    let rendered = render(&inputs).expect("configured opaque transport");
    assert_eq!(
        bytes(rendered.site().output(), "media/media/clip.mp4"),
        clip
    );
    let model: Value =
        serde_json::from_slice(bytes(rendered.site().output(), "model.json")).unwrap();
    assert!(
        model
            .to_string()
            .contains("https://one.example/mount/media/media/clip.mp4")
    );
    let rules = BTreeMap::from([(
        "rule:absent".parse().unwrap(),
        "ABSENT-PRIVATE-CANARY".into(),
    )]);
    let error = rendered
        .site()
        .check_disclosure(&rules, Default::default())
        .expect_err("transport must not manufacture video metadata coverage");
    assert!(
        matches!(error, build::DisclosureProbeError::Unsupported { kind: "file", path } if path.as_str() == "media/media/clip.mp4")
    );
}
