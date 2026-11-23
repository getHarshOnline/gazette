//! Native disclosure dispatch for retained documentation representations.

use super::*;
use crate::build::{
    DisclosureClass, DisclosureDecoding, DisclosurePattern, DisclosureProbeError, DisclosureRule,
    check_disclosure_policy_with_inspectors,
};
use gazette_adapter_markdown::Comrak;
use gazette_core::source::{CapturedSource, InventoryBuilder};

fn output(path: &str, bytes: &[u8]) -> CapturedSource {
    let path: EntryPath = path.parse().expect("documentation output path");
    let mut inventory = InventoryBuilder::new(Default::default());
    inventory
        .insert(
            path.clone(),
            Content::File {
                bytes: bytes.len() as u64,
                sha256: Sha256Digest::of(bytes),
            },
        )
        .expect("documentation output inventory");
    CapturedSource::from_parts(inventory.finish(), BTreeMap::from([(path, bytes.to_vec())]))
        .expect("retained documentation bytes")
}

fn policy() -> DisclosurePolicy {
    DisclosurePolicy {
        rules: [
            DisclosureClass::LocalPath,
            DisclosureClass::PrivateEndpoint,
            DisclosureClass::Secret,
            DisclosureClass::GeneratedArtifact,
            DisclosureClass::FixtureClaim,
            DisclosureClass::InternalIdentifier,
        ]
        .into_iter()
        .enumerate()
        .map(|(index, class)| {
            (
                format!("rule:documentation-{index}").parse().unwrap(),
                DisclosureRule {
                    class,
                    pattern: DisclosurePattern::Literal {
                        value: "SECRET".into(),
                    },
                },
            )
        })
        .collect(),
    }
}

#[test]
fn documentation_markdown_dispatch_requires_native_inspection_and_detects_encoded_links() {
    let inspectors = DisclosureInspectors {
        markdown: Some(&Comrak),
        ..Default::default()
    };
    let clean = output("reference.MD", b"# Public\n");
    let media = media_types(&clean);
    assert_eq!(
        media.values().copied().collect::<Vec<_>>(),
        [MediaType::Markdown]
    );
    let checked = check_disclosure_policy_with_inspectors(
        &clean,
        &media,
        &policy(),
        inspectors,
        Default::default(),
    )
    .expect("native documentation Markdown coverage");
    assert!(matches!(
        checked.files()[0].decoding,
        DisclosureDecoding::MarkdownSyntax { .. }
    ));
    assert!(matches!(
        check_disclosure_policy_with_inspectors(
            &clean,
            &media,
            &policy(),
            DisclosureInspectors::default(),
            Default::default(),
        ),
        Err(DisclosureProbeError::Unsupported { .. })
    ));
    let hidden = output("reference.md", b"[public](https://public.test/%53ECRET)\n");
    let identity = hidden.inventory().identity();
    assert!(matches!(
        check_disclosure_policy_with_inspectors(
            &hidden,
            &media_types(&hidden),
            &policy(),
            inspectors,
            Default::default(),
        ),
        Err(DisclosureProbeError::Findings { .. })
    ));
    assert_eq!(hidden.inventory().identity(), identity);
}

#[test]
fn documentation_mdx_never_receives_markdown_or_plain_text_coverage() {
    // This is target-side MDX, not the native Markdown profile's HTML syntax.
    let mdx = output(
        "reference.mdx",
        b"export const value = '\\u0053ECRET';\n\n<Component value={value} />\n",
    );
    assert!(media_types(&mdx).is_empty());
    let error = check_disclosure_policy_with_inspectors(
        &mdx,
        &media_types(&mdx),
        &policy(),
        DisclosureInspectors {
            markdown: Some(&Comrak),
            ..Default::default()
        },
        Default::default(),
    )
    .expect_err("MDX requires its own qualified decoder, not permissive text coverage");
    assert!(
        matches!(error, DisclosureProbeError::Unsupported { path, .. } if path.as_str() == "reference.mdx")
    );
}

#[test]
fn documentation_generated_literals_have_explicit_coverage_without_reclassifying_mdx() {
    let clean = output(
        "reference.mdx",
        b"---\ntitle: \"Public\"\n---\n\n<pre><span children={\"pub fn f() {}\\n\"} /></pre>\n",
    );
    assert!(media_types(&clean).is_empty());
    let report = check_disclosure_policy_with_inspectors(
        &clean,
        &media_types(&clean),
        &policy(),
        DisclosureInspectors::default(),
        Default::default(),
    )
    .unwrap();
    let value = serde_json::to_value(&report).unwrap();
    let profile = &value["files"][0]["decoding"];
    use crate::build::disclosure::technical_profile::decoding_profile;
    assert_eq!(decoding_profile(profile), Some(None));
    let mut extra = profile.clone();
    extra["generated-mdx-literal-syntax"]["extra"] = true.into();
    assert_eq!(decoding_profile(&extra), None);
    let mut wrong = profile.clone();
    wrong["generated-mdx-literal-syntax"]["profile"] = "unknown".into();
    assert_eq!(decoding_profile(&wrong), None);
    let hidden = output("reference.mdx", b"---\ntitle: \"Public\"\n---\n\n<pre><span children={\"SE\"} /><span children={\"CRET\"} /></pre>\n");
    assert!(matches!(
        check_disclosure_policy_with_inspectors(
            &hidden,
            &media_types(&hidden),
            &policy(),
            DisclosureInspectors::default(),
            Default::default()
        ),
        Err(DisclosureProbeError::Findings { .. })
    ));
}
