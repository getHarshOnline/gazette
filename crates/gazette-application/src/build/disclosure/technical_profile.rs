//! One exact Engine-owned immutable-output technical profile.
//!
//! These coordinates qualify bounded mechanical evidence only. They do not authenticate
//! policy, approve publication, certify a provider or grant release authority.

use std::collections::BTreeMap;

use gazette_core::source::{EntryPath, Sha256Digest};
use serde_json::Value;

pub(crate) const PROFILE: &str = "gazette-immutable-output-technical/0.1";
pub(crate) const ZERO_EXCLUSION_PROFILE: &str = "gazette-zero-output-exclusion/0.1";
pub(crate) const SCANNER: &str = "gazette-disclosure-probe/16";
pub(crate) const FILENAME_PROFILE: &str = "gazette-canonical-caseless-filenames/1";
pub(crate) const ENCODED: &str = gazette_compiler::encoded::PROFILE;
pub(crate) const RESOURCE_PROFILE: &str = "gazette-disclosure-resources/2";

/// Stable identity of every caller-selected scanner ceiling.
///
/// The projection is deliberately independent of wall-clock time and host paths. It binds
/// deterministic input, traversal, retained-output and decoded-work ceilings on Linux and macOS.
pub(crate) fn resource_limits_digest(limits: super::DisclosureProbeLimits) -> Sha256Digest {
    let value = format!(
        concat!(
            "{}\n",
            "rules={};entries={};rules_bytes={};work_bytes={};findings={};evidence_bytes={}\n",
            "html={},{},{},{}\n",
            "xml={},{},{},{},{},{},{}\n",
            "css={},{},{},{}\n",
            "javascript={},{},{},{}\n",
            "markdown={},{},{},{},{}\n",
            "encoded={},{}\n",
            "raster={},{},{},{},{}\n",
            "font={},{},{},{}\n",
            "encoded_depth={};encoded_values={};encoded_bytes={}\n"
        ),
        RESOURCE_PROFILE,
        limits.rules,
        limits.entries,
        limits.rules_bytes,
        limits.work_bytes,
        limits.findings,
        limits.evidence_bytes,
        limits.html.input_bytes,
        limits.html.unemitted_bytes,
        limits.html.tokens,
        limits.html.decoded_bytes,
        limits.xml.input_bytes,
        limits.xml.events,
        limits.xml.depth,
        limits.xml.attributes,
        limits.xml.name_bytes,
        limits.xml.value_bytes,
        limits.xml.decoded_bytes,
        limits.css.input_bytes,
        limits.css.tokens,
        limits.css.depth,
        limits.css.decoded_bytes,
        limits.javascript.input_bytes,
        limits.javascript.nodes,
        limits.javascript.depth,
        limits.javascript.decoded_bytes,
        limits.markdown.source_bytes,
        limits.markdown.nodes,
        limits.markdown.depth,
        limits.markdown.fact_bytes,
        limits.markdown.output_bytes,
        limits.encoded.input_bytes,
        limits.encoded.decoded_bytes,
        limits.raster.encoded_bytes,
        limits.raster.chunk_bytes,
        limits.raster.decoded_bytes,
        limits.raster.values,
        limits.raster.chunks,
        limits.font.encoded_bytes,
        limits.font.decoded_bytes,
        limits.font.tables,
        limits.font.glyphs,
        limits.encoded_depth,
        limits.encoded_values,
        limits.encoded_bytes,
    );
    Sha256Digest::of(value.as_bytes())
}

pub(crate) fn resource_coordinates_match(
    profile: &str,
    digest: Sha256Digest,
    expected: Sha256Digest,
) -> bool {
    profile == RESOURCE_PROFILE && digest == expected
}

pub(crate) fn resource_report_matches(report: &Value, expected: Sha256Digest) -> bool {
    report.get("resource_profile").and_then(Value::as_str) == Some(RESOURCE_PROFILE)
        && report
            .get("resource_limits_sha256")
            .cloned()
            .and_then(|value| serde_json::from_value(value).ok())
            .is_some_and(|digest| resource_coordinates_match(RESOURCE_PROFILE, digest, expected))
}

/// Exact bounded matching and decoder procedure selected by this technical profile.
pub(crate) fn generic_rules_digest() -> Sha256Digest {
    let profile = concat!(
        "gazette-disclosure-policy/3\n",
        "all-six-classes;every-path-and-raw-file;supported-decoded-channels;",
        "no-example-exemption;no-rewrite;bounded-comparison-and-findings;",
        "raster-profile-bound-to-coverage;shared-decoded-budget\n"
    );
    let profiles = [
        profile,
        gazette_compiler::disclosure::PROFILE,
        gazette_compiler::html::PROFILE,
        gazette_compiler::xml::PROFILE,
        gazette_compiler::css::PROFILE,
        gazette_compiler::javascript::PROFILE,
        ENCODED,
        RESOURCE_PROFILE,
        FILENAME_PROFILE,
        "gazette-json-all-members/1",
        super::markdown::PROFILE,
        gazette_compiler::generated_mdx::PROFILE,
    ];
    Sha256Digest::of(profiles.join("\n").as_bytes())
}

pub(crate) fn coordinates_match(
    scanner: &str,
    encoded: &str,
    filenames: &str,
    generic_rules: Sha256Digest,
) -> bool {
    scanner == SCANNER
        && encoded == ENCODED
        && filenames == FILENAME_PROFILE
        && generic_rules == generic_rules_digest()
}

/// The bounded 0.1 mechanics profile admits no generated-artifact removal.
pub(crate) fn zero_exclusion_matches(profile: &str, removed: &[EntryPath]) -> bool {
    profile == ZERO_EXCLUSION_PROFILE && removed.is_empty()
}

pub(crate) fn valid_raster_requirements(requirements: &BTreeMap<String, String>) -> bool {
    requirements.len() <= 3
        && requirements.iter().all(|(format, profile)| {
            matches!(format.as_str(), "png" | "jpeg" | "webp") && valid_profile(profile)
        })
}

/// Validate every recorded decoder and exact independently selected raster profiles.
pub(crate) fn report_matches(
    report: &Value,
    raster_requirements: &BTreeMap<String, String>,
    resource_limits: Sha256Digest,
) -> bool {
    if !resource_report_matches(report, resource_limits)
        || !valid_raster_requirements(raster_requirements)
        || !coordinates_match(
            report.get("scanner").and_then(Value::as_str).unwrap_or(""),
            report
                .get("encoded_profile")
                .and_then(Value::as_str)
                .unwrap_or(""),
            report
                .get("filename_profile")
                .and_then(Value::as_str)
                .unwrap_or(""),
            match report
                .get("generic_rules_sha256")
                .cloned()
                .and_then(|value| serde_json::from_value(value).ok())
            {
                Some(value) => value,
                None => return false,
            },
        )
    {
        return false;
    }
    let mut observed = BTreeMap::new();
    for key in ["files", "embedded"] {
        let Some(rows) = report.get(key).and_then(Value::as_array) else {
            return false;
        };
        for row in rows {
            let Some(decoding) = row.get("decoding") else {
                return false;
            };
            let Some(raster) = decoding_profile(decoding) else {
                return false;
            };
            if let Some((format, profile)) = raster
                && observed
                    .insert(format.to_owned(), profile.to_owned())
                    .is_some_and(|existing| existing != profile)
            {
                return false;
            }
        }
    }
    observed == *raster_requirements
}

/// Exact decoder vocabulary. A raster profile remains independently selected by qualification.
pub(crate) fn decoding_profile(value: &Value) -> Option<Option<(&str, &str)>> {
    if let Some(kind) = value.as_str() {
        return matches!(kind, "utf8-text" | "json-strings").then_some(None);
    }
    let map = value.as_object()?;
    if map.len() != 1 {
        return None;
    }
    let (kind, payload) = map.iter().next()?;
    let payload = payload.as_object()?;
    let exact = |keys: &[&str]| {
        payload.len() == keys.len() && keys.iter().all(|key| payload.contains_key(*key))
    };
    match kind.as_str() {
        "generated-mdx-literal-syntax" if exact(&["profile", "html_profile", "css_profile"]) => {
            (text(payload, "profile") == Some(gazette_compiler::generated_mdx::PROFILE)
                && text(payload, "html_profile") == Some(gazette_compiler::html::PROFILE)
                && text(payload, "css_profile") == Some(gazette_compiler::css::PROFILE))
            .then_some(None)
        }
        "raster-metadata" if exact(&["format", "profile"]) => {
            let format = payload.get("format")?.as_str()?;
            let profile = payload.get("profile")?.as_str()?;
            (matches!(format, "png" | "jpeg" | "webp") && valid_profile(profile))
                .then_some(Some((format, profile)))
        }
        "woff2-font" if exact(&["profile", "tables", "glyphs"]) => (text(payload, "profile")
            .is_some_and(valid_profile)
            && payload
                .get("tables")?
                .as_u64()
                .is_some_and(|value| value > 0)
            && payload
                .get("glyphs")?
                .as_u64()
                .is_some_and(|value| value > 0))
        .then_some(None),
        "html-tokens" if exact(&["profile", "css_profile", "javascript_profile"]) => {
            (text(payload, "profile") == Some(gazette_compiler::html::PROFILE)
                && text(payload, "css_profile") == Some(gazette_compiler::css::PROFILE)
                && text(payload, "javascript_profile")
                    == Some(gazette_compiler::javascript::PROFILE))
            .then_some(None)
        }
        "markdown-syntax"
            if exact(&[
                "profile",
                "parser_profile",
                "html_profile",
                "css_profile",
                "javascript_profile",
            ]) =>
        {
            (text(payload, "profile") == Some(super::markdown::PROFILE)
                && text(payload, "parser_profile").is_some_and(valid_profile)
                && text(payload, "html_profile") == Some(gazette_compiler::html::PROFILE)
                && text(payload, "css_profile") == Some(gazette_compiler::css::PROFILE)
                && text(payload, "javascript_profile")
                    == Some(gazette_compiler::javascript::PROFILE))
            .then_some(None)
        }
        "xml-events" if exact(&["profile", "css_profile"]) => (text(payload, "profile")
            == Some(gazette_compiler::xml::PROFILE)
            && text(payload, "css_profile") == Some(gazette_compiler::css::PROFILE))
        .then_some(None),
        "css-tokens" if exact(&["profile"]) => {
            (text(payload, "profile") == Some(gazette_compiler::css::PROFILE)).then_some(None)
        }
        "javascript-syntax" if exact(&["profile", "goal"]) => (text(payload, "profile")
            == Some(gazette_compiler::javascript::PROFILE)
            && matches!(
                text(payload, "goal"),
                Some("script" | "module" | "unambiguous")
            ))
        .then_some(None),
        _ => None,
    }
}

fn text<'a>(map: &'a serde_json::Map<String, Value>, key: &str) -> Option<&'a str> {
    map.get(key)?.as_str()
}

fn valid_profile(profile: &str) -> bool {
    !profile.is_empty()
        && profile.len() <= 256
        && profile.bytes().all(|byte| byte.is_ascii_graphic())
}
