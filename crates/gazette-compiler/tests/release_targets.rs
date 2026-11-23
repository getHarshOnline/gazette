//! Complete target-array validation is independent of active authorization or native access.

use gazette_compiler::release::{self, Error, Limits, Target};
use gazette_core::source::Sha256Digest;
use serde_json::{Value, json};

fn targets() -> Vec<Target> {
    let value: Value =
        serde_json::from_str(include_str!("release_authorization/authorization.json"))
            .expect("existing exact native target fixture");
    let template: Target = serde_json::from_value(value["ordered_targets"][0].clone())
        .expect("one native target shape, not another signed authorization");
    (0..3)
        .map(|ordinal| {
            let mut target = template.clone();
            target.ordinal = ordinal;
            target.domain = format!("{ordinal}.readback.example");
            target.remote = format!("https://example.invalid/readback-{ordinal}.git");
            target
        })
        .collect()
}

fn failure(bytes: &[u8]) -> Error {
    release::parse_targets(bytes, Limits::default())
        .err()
        .expect("whole selection must be refused")
}

#[test]
fn checked_targets_bind_domain_separated_jcs_without_signature_or_time_prerequisites() {
    let mut targets = targets();
    targets[0].domain = "é😀.example".into();
    let bytes = serde_json::to_vec(&targets).expect("typed target array");
    let checked =
        release::parse_targets(&bytes, Limits::default()).expect("complete native selection");
    assert_eq!(checked.targets(), targets);
    let mut expected = b"gazette.release-targets/1\0".to_vec();
    expected
        .extend_from_slice(&serde_jcs::to_vec(&targets).expect("independent pinned JCS tooling"));
    assert_eq!(checked.identity(), Sha256Digest::of(&expected));
    assert_eq!(
        release::check_targets(&targets, Limits::default())
            .expect("typed native path")
            .identity(),
        checked.identity()
    );
    let value: Value = serde_json::from_slice(&bytes).expect("same target meanings");
    // Emit members explicitly: rebuilding a Map would sort them again when
    // this test runs without serde_json's preserve_order feature.
    let reversed_targets: Vec<_> = value
        .as_array()
        .expect("ordered targets")
        .iter()
        .map(|target| {
            let members: Vec<_> = target
                .as_object()
                .expect("target object")
                .iter()
                .rev()
                .map(|(key, value)| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(key).expect("escaped key"),
                        serde_json::to_string(value).expect("native field value")
                    )
                })
                .collect();
            format!("{{{}}}", members.join(","))
        })
        .collect();
    let changed_transport = format!("[{}]", reversed_targets.join(",")).into_bytes();
    assert_ne!(
        changed_transport, bytes,
        "key order must change independently of pretty-print whitespace"
    );
    assert_eq!(
        release::parse_targets(&changed_transport, Limits::default())
            .expect("equivalent JCS selection")
            .identity(),
        checked.identity()
    );
    targets[0].output_digest = Sha256Digest::of(b"another output");
    assert_ne!(
        release::check_targets(&targets, Limits::default())
            .expect("different valid selection")
            .identity(),
        checked.identity()
    );
}

#[test]
fn target_shape_and_exact_action_rules_fail_the_complete_array_not_a_prefix() {
    let original = targets();
    release::check_targets(&original, Limits::default()).expect("positive native control");
    for case in [
        "empty",
        "order",
        "duplicate-domain",
        "duplicate-destination",
        "force",
        "moving-source",
        "wrong-ref",
        "zero-oid",
    ] {
        let mut targets = original.clone();
        match case {
            "empty" => targets.clear(),
            "order" => targets.swap(0, 1),
            "duplicate-domain" => targets[1].domain = targets[0].domain.clone(),
            "duplicate-destination" => {
                targets[1].remote = targets[0].remote.clone();
                targets[1].branch = targets[0].branch.clone();
            }
            "force" => targets[1].refspec.insert(0, '+'),
            "moving-source" => {
                targets[1].refspec = format!("HEAD:refs/heads/{}", targets[1].branch)
            }
            "wrong-ref" => {
                targets[1].refspec = format!("{}:refs/tags/tag", targets[1].local_commit_oid)
            }
            "zero-oid" => {
                targets[1].local_commit_oid =
                    "0".repeat(40).parse().expect("well-shaped invalid zero")
            }
            _ => unreachable!("closed native cases"),
        }
        assert_eq!(
            release::check_targets(&targets, Limits::default())
                .err()
                .expect("invalid complete set"),
            Error::Target,
            "{case}"
        );
    }
    for bytes in [
        b"null".as_slice(),
        b"{}",
        b"[[]]",
        b"[null]",
        b"[] trailing",
    ] {
        assert!(matches!(failure(bytes), Error::Shape | Error::Json));
    }
    let value = serde_json::to_value(&original).expect("native targets");
    let mut unknown = value.clone();
    unknown[0]["authorization"] = json!(true);
    assert_eq!(
        failure(&serde_json::to_vec(&unknown).expect("unknown field")),
        Error::Shape
    );
    let bytes = serde_json::to_string(&value).expect("target wire");
    let duplicate = bytes.replacen("\"ordinal\":0", "\"ordinal\":0,\"ord\\u0069nal\":0", 1);
    assert_ne!(duplicate, bytes);
    assert_eq!(failure(duplicate.as_bytes()), Error::Json);
}

#[test]
fn target_wire_work_limits_apply_equally_to_typed_and_parsed_inputs() {
    let targets = targets();
    let bytes = serde_json::to_vec(&targets).expect("native target bytes");
    release::parse_targets(&bytes, Limits::default()).expect("same positive input under defaults");
    for limits in [
        Limits {
            bytes: 0,
            ..Default::default()
        },
        Limits {
            nodes: 1,
            ..Default::default()
        },
        Limits {
            depth: 1,
            ..Default::default()
        },
        Limits {
            targets: 1,
            ..Default::default()
        },
    ] {
        assert_eq!(
            release::parse_targets(&bytes, limits)
                .err()
                .expect("native parse bound"),
            Error::Limit
        );
        assert_eq!(
            release::check_targets(&targets, limits)
                .err()
                .expect("same typed bound"),
            Error::Limit
        );
    }
}
