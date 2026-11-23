//! Read exact retained application evidence without re-running checks or granting approval.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

use gazette_core::{
    publication::Key,
    source::{EntryPath, Sha256Digest, TreeIdentity},
    store,
};
use serde::Deserialize;
use serde_json::{Map, Value, value::RawValue};

/// Historical coordinates for the native release join; opaque provenance is not re-admitted.
pub(crate) struct PreparationEvidence {
    pub site: Key,
    pub preparation_sha256: Sha256Digest,
    pub disclosure_sha256: Sha256Digest,
    pub policy_sha256: Sha256Digest,
    pub scanner: String,
    pub generic_rules_sha256: Sha256Digest,
    pub candidate_policy: Option<crate::build::CandidatePolicyRequirement>,
    pub output_policy: Option<crate::build::OutputPolicyRequirement>,
    pub admission: Option<PreparationAdmissionEvidence>,
    pub files: BTreeMap<EntryPath, Sha256Digest>,
    pub embedded_paths: BTreeSet<EntryPath>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PreparationAdmissionEvidence {
    Authored(Box<super::PreparationQualification>),
    GeneratedDocumentation(Box<super::super::DocumentationPreparationQualification>),
}

/// Redacted private-record refusal, not a native repair or policy decision.
#[derive(Debug)]
pub(crate) struct PreparationEvidenceError;

impl fmt::Display for PreparationEvidenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("preparation evidence is invalid, mismatched or exceeds its bound")
    }
}
impl std::error::Error for PreparationEvidenceError {}

type Result<T> = std::result::Result<T, PreparationEvidenceError>;

/// Inspect the existing producer's closed envelopes and exact nested projection bytes.
/// Source/rendering subtrees remain opaque provenance bound by the original whole hash.
pub(crate) fn inspect_preparation_evidence(
    record: &store::PreparationRecord,
    maximum: usize,
) -> Result<PreparationEvidence> {
    let intent = &record.intent;
    let check = record.check.as_ref().ok_or(PreparationEvidenceError)?;
    let retention = record.retention.as_ref().ok_or(PreparationEvidenceError)?;
    if intent.evidence.len() > maximum
        || check.evidence.len() > maximum
        || Sha256Digest::of(intent.evidence.as_bytes()) != intent.evidence_sha256
        || Sha256Digest::of(check.evidence.as_bytes()) != check.evidence_sha256
        || check.intent_id != intent.id
        || retention.intent_id != intent.id
        || retention.output != intent.output
        || retention.entries != intent.entries
        || retention.bytes != intent.bytes
        || record
            .worktree
            .as_ref()
            .is_some_and(|worktree| worktree.check_sha256 != Some(check.evidence_sha256))
    {
        return Err(PreparationEvidenceError);
    }
    let original_value = gazette_core::json::parse(intent.evidence.as_bytes())
        .map_err(|_| PreparationEvidenceError)?;
    let format = text(
        original_value
            .get("format")
            .ok_or(PreparationEvidenceError)?,
    )?;
    let documentation = format == "gazette.documentation-preparation/1";
    let original = object(
        &original_value,
        if documentation {
            &[
                "format",
                "artifact_class",
                "repository_source_provenance",
                "inputs",
                "brand",
                "repository",
                "artifacts",
                "output",
            ]
        } else {
            &[
                "format",
                "repository_source_provenance",
                "runtime_build_profile",
                "configuration_sha256",
                "site",
                "sources",
                "selection",
                "candidate_check",
                "rendering",
            ]
        },
    )?;
    if !documentation {
        equal_text(field(original, "format")?, "gazette.preparation-evidence/1")?;
    }
    let repository_source = object(
        field(original, "repository_source_provenance")?,
        &["profile", "sha256", "files", "bytes"],
    )?;
    equal_text(
        field(repository_source, "profile")?,
        "gazette-repository-source-provenance/1",
    )?;
    digest(field(repository_source, "sha256")?)?;
    for (key, maximum) in [("files", 8192), ("bytes", 256 << 20)] {
        if !field(repository_source, key)?
            .as_u64()
            .is_some_and(|value| value > 0 && value <= maximum)
        {
            return Err(PreparationEvidenceError);
        }
    }
    if !documentation {
        let runtime: crate::build::RuntimeBuildProfile =
            serde_json::from_value(field(original, "runtime_build_profile")?.clone())
                .map_err(|_| PreparationEvidenceError)?;
        if !runtime.is_valid() {
            return Err(PreparationEvidenceError);
        }
    }
    let original_raw: BTreeMap<String, &RawValue> =
        serde_json::from_str(&intent.evidence).map_err(|_| PreparationEvidenceError)?;
    let (candidate, site, sources) = if documentation {
        equal_text(
            field(original, "artifact_class")?,
            "documentation-build-input",
        )?;
        let inputs = documentation_inputs(field(original, "inputs")?)?;
        let site: Key = text(field(inputs, "site")?)?
            .parse()
            .map_err(|_| PreparationEvidenceError)?;
        let _: Key = text(field(original, "brand")?)?
            .parse()
            .map_err(|_| PreparationEvidenceError)?;
        let repository: gazette_compiler::workspace::DocumentationRepository =
            serde_json::from_value(field(original, "repository")?.clone())
                .map_err(|_| PreparationEvidenceError)?;
        if repository.destination.as_ref() != intent.destination.as_ref()
            || tree(field(original, "output")?)? != intent.output
            || !field(original, "artifacts")?.is_array()
        {
            return Err(PreparationEvidenceError);
        }
        (None, site, field(inputs, "sources")?)
    } else {
        digest(field(original, "configuration_sha256")?)?;
        let candidate = candidate(original, &original_raw, maximum)?;
        let site = text(field(original, "site")?)?
            .parse()
            .map_err(|_| PreparationEvidenceError)?;
        if !field(original, "sources")?.is_object() || !field(original, "rendering")?.is_object() {
            return Err(PreparationEvidenceError);
        }
        let selection = field(original, "selection")?;
        if !selection.is_object() {
            return Err(PreparationEvidenceError);
        }
        // Match the producer's required contract without reconstructing its original corpus.
        // Shape validation cannot re-prove completeness, lifecycle or owner-policy admission.
        gazette_compiler::authored::Selection::deserialize(selection)
            .map_err(|_| PreparationEvidenceError)?;
        (candidate, site, field(original, "sources")?)
    };

    let checked = gazette_core::json::parse(check.evidence.as_bytes())
        .map_err(|_| PreparationEvidenceError)?;
    let mut check_keys = vec![
        "format",
        "preparation_evidence_sha256",
        "disclosure",
        "policy",
    ];
    if checked.get("technical_certificate").is_some() {
        check_keys.push("technical_certificate");
    }
    let checked = object(&checked, &check_keys)?;
    equal_text(
        field(checked, "format")?,
        if documentation {
            "gazette.documentation-preparation-check/1"
        } else {
            "gazette.preparation-check/1"
        },
    )?;
    if digest(field(checked, "preparation_evidence_sha256")?)? != intent.evidence_sha256 {
        return Err(PreparationEvidenceError);
    }
    let disclosure = disclosure(field(checked, "disclosure")?, intent.output)?;
    // The duplicate-safe parse above checks all descendants. Borrow original nested
    // slices here: reserializing Value would alter whitespace, escapes and key order.
    let raw: BTreeMap<String, &RawValue> =
        serde_json::from_str(&check.evidence).map_err(|_| PreparationEvidenceError)?;
    let disclosure_sha256 = Sha256Digest::of(
        raw.get("disclosure")
            .ok_or(PreparationEvidenceError)?
            .get()
            .as_bytes(),
    );
    let output_policy = policy(
        field(checked, "policy")?,
        sources,
        intent.output,
        disclosure_sha256,
        candidate.as_ref().map(|check| check.sha256),
        maximum,
    )?;
    // A supplied certificate always requires complete native rederivation, even for
    // an ordinary historical inspection with no independently selected qualification.
    let qualification = checked.get("technical_certificate");
    let admission = if documentation {
        qualification
            .map(|certificate| {
                crate::build::documentation::DocumentationTechnicalCertificate::inspect(
                    certificate,
                    intent.evidence.as_bytes(),
                    raw.get("disclosure")
                        .ok_or(PreparationEvidenceError)?
                        .get()
                        .as_bytes(),
                    maximum,
                )
                .map(|certificate| {
                    PreparationAdmissionEvidence::GeneratedDocumentation(Box::new(
                        certificate.requirements().clone(),
                    ))
                })
                .map_err(|_| PreparationEvidenceError)
            })
            .transpose()?
    } else {
        qualification
            .map(|certificate| {
                super::TechnicalCertificate::inspect(
                    certificate,
                    intent.evidence.as_bytes(),
                    intent.output,
                    raw.get("disclosure")
                        .ok_or(PreparationEvidenceError)?
                        .get()
                        .as_bytes(),
                    raw.get("policy")
                        .ok_or(PreparationEvidenceError)?
                        .get()
                        .as_bytes(),
                    maximum,
                )
                .map(|certificate| {
                    PreparationAdmissionEvidence::Authored(Box::new(
                        certificate.requirements().clone(),
                    ))
                })
                .map_err(|_| PreparationEvidenceError)
            })
            .transpose()?
    };
    // Nondefault limits require a fully rederived certificate. Unqualified history
    // keeps the default expectation instead of trusting the report's own digest.
    if admission.is_none()
        && !crate::build::disclosure::technical_profile::resource_report_matches(
            field(checked, "disclosure")?,
            crate::build::DisclosureProbeLimits::default().resource_limits_sha256(),
        )
    {
        return Err(PreparationEvidenceError);
    }
    let mut files = BTreeMap::new();
    for file in array(field(disclosure, "files")?)? {
        let file = file.as_object().ok_or(PreparationEvidenceError)?;
        if files
            .insert(path(field(file, "path")?)?, digest(field(file, "sha256")?)?)
            .is_some()
        {
            return Err(PreparationEvidenceError);
        }
    }
    let mut embedded_paths = BTreeSet::new();
    for embedded in array(field(disclosure, "embedded")?)? {
        let embedded = embedded.as_object().ok_or(PreparationEvidenceError)?;
        embedded_paths.insert(path(field(embedded, "path")?)?);
    }
    Ok(PreparationEvidence {
        site,
        preparation_sha256: intent.evidence_sha256,
        disclosure_sha256,
        policy_sha256: Sha256Digest::of(
            raw.get("policy")
                .ok_or(PreparationEvidenceError)?
                .get()
                .as_bytes(),
        ),
        scanner: profile(field(disclosure, "scanner")?)?.to_owned(),
        generic_rules_sha256: digest(field(disclosure, "generic_rules_sha256")?)?,
        candidate_policy: candidate.and_then(|check| check.requirement),
        files,
        embedded_paths,
        output_policy,
        admission,
    })
}

fn disclosure(value: &Value, output: TreeIdentity) -> Result<&Map<String, Value>> {
    let map = object(
        value,
        &[
            "status",
            "scanner",
            "output_identity",
            "policy_sha256",
            "generic_rules_sha256",
            "findings_sha256",
            "work_bytes",
            "encoded_profile",
            "filename_profile",
            "resource_profile",
            "resource_limits_sha256",
            "encoded_bytes",
            "decoded_values",
            "embedded",
            "files",
            "findings",
        ],
    )?;
    equal_text(field(map, "status")?, "required-supported-profile-evidence")?;
    let scanner = profile(field(map, "scanner")?)?;
    let encoded = profile(field(map, "encoded_profile")?)?;
    let filenames = profile(field(map, "filename_profile")?)?;
    let generic_rules = digest(field(map, "generic_rules_sha256")?)?;
    let resource = profile(field(map, "resource_profile")?)?;
    digest(field(map, "resource_limits_sha256")?)?;
    if tree(field(map, "output_identity")?)? != output
        || !crate::build::disclosure::technical_profile::coordinates_match(
            scanner,
            encoded,
            filenames,
            generic_rules,
        )
        || resource != crate::build::DisclosureProbeLimits::RESOURCE_PROFILE
        || !array(field(map, "findings")?)?.is_empty()
        || digest(field(map, "findings_sha256")?)? != Sha256Digest::of(b"[]")
    {
        return Err(PreparationEvidenceError);
    }
    for key in ["work_bytes", "encoded_bytes", "decoded_values"] {
        number(field(map, key)?)?;
    }
    for file in array(field(map, "files")?)? {
        let file = object(file, &["path", "sha256", "decoding"])?;
        path(field(file, "path")?)?;
        digest(field(file, "sha256")?)?;
        decoding(field(file, "decoding")?)?;
    }
    for embedded in array(field(map, "embedded")?)? {
        let embedded = object(
            embedded,
            &[
                "path",
                "depth",
                "representation",
                "sha256",
                "bytes",
                "media_type",
                "decoding",
            ],
        )?;
        path(field(embedded, "path")?)?;
        if number(field(embedded, "depth")?)? == 0 {
            return Err(PreparationEvidenceError);
        }
        if !matches!(
            text(field(embedded, "representation")?)?,
            "percent-decoded" | "data-payload"
        ) {
            return Err(PreparationEvidenceError);
        }
        digest(field(embedded, "sha256")?)?;
        number(field(embedded, "bytes")?)?;
        let media = field(embedded, "media_type")?;
        if !media.is_null() {
            text(media)?;
        }
        decoding(field(embedded, "decoding")?)?;
    }
    Ok(map)
}

struct CandidateEvidence {
    sha256: Sha256Digest,
    requirement: Option<crate::build::CandidatePolicyRequirement>,
}

/// Reuse the historical candidate validator before qualification. Imported decisions
/// and exit-only observations have no checker-originated requirement to satisfy.
pub(super) fn candidate_requirement(
    original: &[u8],
) -> Result<Option<crate::build::CandidatePolicyRequirement>> {
    let value = gazette_core::json::parse(original).map_err(|_| PreparationEvidenceError)?;
    let fields = value.as_object().ok_or(PreparationEvidenceError)?;
    let raw: BTreeMap<String, &RawValue> =
        serde_json::from_slice(original).map_err(|_| PreparationEvidenceError)?;
    Ok(candidate(fields, &raw, original.len())?.and_then(|check| check.requirement))
}

fn candidate(
    original: &Map<String, Value>,
    raw: &BTreeMap<String, &RawValue>,
    maximum: usize,
) -> Result<Option<CandidateEvidence>> {
    let value = field(original, "candidate_check")?;
    if value.is_null() {
        return Ok(None);
    }
    let captured = value.get("requirement").is_some();
    let check = object(
        value,
        if captured {
            &[
                "status",
                "invocation",
                "review",
                "response",
                "requirement",
                "policy_source",
            ]
        } else {
            &["status", "invocation", "review", "response"]
        },
    )?;
    equal_text(field(check, "status")?, "policy-check-passed-unverified")?;
    let invocation = object(
        field(check, "invocation")?,
        &["request_sha256", "command_sha256"],
    )?;
    let command_sha256 = digest(field(invocation, "command_sha256")?)?;
    let (requirement, policy_source) = if captured {
        let value = field(check, "requirement")?;
        let requirement: crate::build::CandidatePolicyRequirement =
            serde_json::from_value(value.clone()).map_err(|_| PreparationEvidenceError)?;
        if serde_json::to_value(&requirement).map_err(|_| PreparationEvidenceError)? != *value
            || requirement.command_sha256 != command_sha256
        {
            return Err(PreparationEvidenceError);
        }
        let source = field(check, "policy_source")?;
        if policy_document(source, field(original, "sources")?)?
            != Some(requirement.policy.source_sha256)
        {
            return Err(PreparationEvidenceError);
        }
        let source: crate::build::policy::PolicyDocument =
            serde_json::from_value(source.clone()).map_err(|_| PreparationEvidenceError)?;
        (Some(requirement), Some(source))
    } else {
        (None, None)
    };
    let review = object(field(check, "review")?, &["sha256", "request"])?;
    let request = object(
        field(review, "request")?,
        &[
            "format",
            "site",
            "configuration_sha256",
            "sources",
            "selection_sha256",
            "surfaces",
        ],
    )?;
    equal_text(field(request, "format")?, "gazette-publication-review/1")?;
    if field(request, "site")? != field(original, "site")?
        || field(request, "configuration_sha256")? != field(original, "configuration_sha256")?
        || digest(field(request, "selection_sha256")?)?
            != Sha256Digest::of(
                raw.get("selection")
                    .ok_or(PreparationEvidenceError)?
                    .get()
                    .as_bytes(),
            )
    {
        return Err(PreparationEvidenceError);
    }
    candidate_sources(field(request, "sources")?, field(original, "sources")?)?;
    let surfaces = array(field(request, "surfaces")?)?;
    let values = surfaces
        .iter()
        .map(|surface| {
            let surface = object(surface, &["target", "value"])?;
            let target = field(surface, "target")?;
            let target_map = target.as_object().ok_or(PreparationEvidenceError)?;
            match text(field(target_map, "kind")?)? {
                "page" => {
                    object(target, &["kind", "content"])?;
                    text(field(target_map, "content")?)?
                        .parse::<Key>()
                        .map_err(|_| PreparationEvidenceError)?;
                }
                "asset" => {
                    object(target, &["kind", "path"])?;
                    path(field(target_map, "path")?)?;
                }
                "site-graph" | "composition" | "uncompiled" => {
                    object(target, &["kind"])?;
                }
                _ => return Err(PreparationEvidenceError),
            }
            field(surface, "value")
        })
        .collect::<Result<Vec<_>>>()?;
    let check_raw = raw.get("candidate_check").ok_or(PreparationEvidenceError)?;
    let fields: BTreeMap<String, &RawValue> =
        serde_json::from_str(check_raw.get()).map_err(|_| PreparationEvidenceError)?;
    let review_raw = fields.get("review").ok_or(PreparationEvidenceError)?;
    let review_fields: BTreeMap<String, &RawValue> =
        serde_json::from_str(review_raw.get()).map_err(|_| PreparationEvidenceError)?;
    let request_sha256 = Sha256Digest::of(
        review_fields
            .get("request")
            .ok_or(PreparationEvidenceError)?
            .get()
            .as_bytes(),
    );
    if request_sha256 != digest(field(review, "sha256")?)? {
        return Err(PreparationEvidenceError);
    }
    let response = field(check, "response")?;
    if captured && response.is_null() {
        return Err(PreparationEvidenceError);
    }
    if !response.is_null() {
        candidate_response(
            response,
            request_sha256,
            &values,
            requirement.as_ref().map(|r| &r.policy),
            maximum,
        )?;
    }
    let input = if captured {
        // Fresh evaluator results were not available in the request delivered before execution.
        crate::build::policy::encode_candidate_request(
            review_raw,
            &None::<&crate::build::MatchedReviewResponse>,
            requirement.as_ref(),
            policy_source.as_ref(),
            maximum,
        )
    } else {
        crate::build::policy::encode_candidate_request(
            review_raw,
            fields.get("response").ok_or(PreparationEvidenceError)?,
            None,
            None,
            maximum,
        )
    }
    .map_err(|_| PreparationEvidenceError)?;
    if Sha256Digest::of(&input) != digest(field(invocation, "request_sha256")?)? {
        return Err(PreparationEvidenceError);
    }
    Ok(Some(CandidateEvidence {
        sha256: Sha256Digest::of(check_raw.get().as_bytes()),
        requirement,
    }))
}

fn candidate_sources(review: &Value, original: &Value) -> Result<()> {
    let review = review.as_object().ok_or(PreparationEvidenceError)?;
    let original = original.as_object().ok_or(PreparationEvidenceError)?;
    if !review.keys().eq(original.keys()) {
        return Err(PreparationEvidenceError);
    }
    for (key, identity) in review {
        key.parse::<Key>().map_err(|_| PreparationEvidenceError)?;
        let source = original.get(key).ok_or(PreparationEvidenceError)?;
        let inventory = source_inventory(source)?;
        if tree(identity)? != tree(field(inventory, "identity")?)? {
            return Err(PreparationEvidenceError);
        }
    }
    Ok(())
}

fn source_inventory(value: &Value) -> Result<&Map<String, Value>> {
    let source = value.as_object().ok_or(PreparationEvidenceError)?;
    let verification = field(source, "verification")?
        .as_object()
        .ok_or(PreparationEvidenceError)?;
    let source = match text(field(source, "kind")?)? {
        "repository" => verification,
        "submodule" => field(verification, "source")?
            .as_object()
            .ok_or(PreparationEvidenceError)?,
        _ => return Err(PreparationEvidenceError),
    };
    field(source, "inventory")?
        .as_object()
        .ok_or(PreparationEvidenceError)
}

fn policy_document(value: &Value, sources: &Value) -> Result<Option<Sha256Digest>> {
    if value.is_null() {
        return Ok(None);
    }
    object(value, &["binding", "tree", "sha256", "document_base64"])?;
    let document: crate::build::policy::PolicyDocument =
        serde_json::from_value(value.clone()).map_err(|_| PreparationEvidenceError)?;
    let bytes = document.bytes().map_err(|_| PreparationEvidenceError)?;
    let source = sources
        .get(document.binding.source.as_str())
        .ok_or(PreparationEvidenceError)?;
    let inventory = source_inventory(source)?;
    if tree(field(inventory, "identity")?)? != document.tree {
        return Err(PreparationEvidenceError);
    }
    let mut matches = array(field(inventory, "entries")?)?.iter().filter(|entry| {
        entry.get("path").and_then(Value::as_str) == Some(document.binding.entry.as_str())
    });
    let entry = object(
        matches.next().ok_or(PreparationEvidenceError)?,
        &["path", "kind", "bytes", "sha256"],
    )?;
    if matches.next().is_some()
        || text(field(entry, "kind")?)? != "file"
        || number(field(entry, "bytes")?)? != bytes.len() as u64
        || digest(field(entry, "sha256")?)? != document.sha256
    {
        return Err(PreparationEvidenceError);
    }
    Ok(Some(document.sha256))
}

fn candidate_response(
    value: &Value,
    request_sha256: Sha256Digest,
    surfaces: &[&Value],
    expected_policy: Option<&crate::build::ReviewPolicy>,
    maximum: usize,
) -> Result<()> {
    let response = object(
        value,
        &[
            "status",
            "request_sha256",
            "response_sha256",
            "policy",
            "decisions",
            "response_json",
        ],
    )?;
    equal_text(field(response, "status")?, "matched-unverified")?;
    if digest(field(response, "request_sha256")?)? != request_sha256 {
        return Err(PreparationEvidenceError);
    }
    let policy = field(response, "policy")?;
    object(policy, &["id", "revision", "source_sha256"])?;
    let binding = crate::build::ReviewResponseBinding {
        policy: serde_json::from_value(policy.clone()).map_err(|_| PreparationEvidenceError)?,
        response_sha256: digest(field(response, "response_sha256")?)?,
    };
    let limits = crate::build::ReviewResponseLimits {
        bytes: maximum,
        decisions: maximum,
    };
    let bytes = text(field(response, "response_json")?)?.as_bytes();
    let matched = if let Some(policy) = expected_policy {
        crate::build::review::match_captured_review_response(
            request_sha256,
            surfaces.iter().copied(),
            bytes,
            policy,
            limits,
        )
    } else {
        crate::build::review::match_review_response(
            request_sha256,
            surfaces.iter().copied(),
            bytes,
            &binding,
            limits,
        )
    }
    .map_err(|_| PreparationEvidenceError)?;
    if serde_json::to_value(&matched).map_err(|_| PreparationEvidenceError)? != *value
        || matched
            .decisions()
            .iter()
            .any(|decision| decision.outcome != crate::build::ReviewOutcome::Allow)
    {
        return Err(PreparationEvidenceError);
    }
    Ok(())
}

fn policy(
    value: &Value,
    sources: &Value,
    output: TreeIdentity,
    disclosure_sha256: Sha256Digest,
    candidate_sha256: Option<Sha256Digest>,
    maximum: usize,
) -> Result<Option<crate::build::OutputPolicyRequirement>> {
    let mut output_policy = None;
    let map = value.as_object().ok_or(PreparationEvidenceError)?;
    match text(field(map, "status")?)? {
        "not-configured" => {
            object(value, &["status"])?;
            if candidate_sha256.is_some() {
                return Err(PreparationEvidenceError);
            }
        }
        "checked" => {
            object(value, &["status", "evidence"])?;
            let evidence = object(
                field(map, "evidence")?,
                &[
                    "status",
                    "invocation",
                    "output",
                    "disclosure_sha256",
                    "candidate_check_sha256",
                    "response",
                    "policy_source",
                ],
            )?;
            equal_text(
                field(evidence, "status")?,
                "output-policy-check-passed-unverified",
            )?;
            if tree(field(evidence, "output")?)? != output
                || digest(field(evidence, "disclosure_sha256")?)? != disclosure_sha256
            {
                return Err(PreparationEvidenceError);
            }
            let candidate = field(evidence, "candidate_check_sha256")?;
            let candidate = if candidate.is_null() {
                None
            } else {
                Some(digest(candidate)?)
            };
            if candidate != candidate_sha256 {
                return Err(PreparationEvidenceError);
            }
            let invocation = object(
                field(evidence, "invocation")?,
                &["request_sha256", "command_sha256"],
            )?;
            let request_sha256 = digest(field(invocation, "request_sha256")?)?;
            let command_sha256 = digest(field(invocation, "command_sha256")?)?;
            let response = field(evidence, "response")?;
            let policy_sha256 = policy_document(field(evidence, "policy_source")?, sources)?;
            if !response.is_null() {
                let envelope = object(
                    response,
                    &["status", "requirement", "response_sha256", "response_json"],
                )?;
                let requirement_value = field(envelope, "requirement")?;
                let requirement: crate::build::OutputPolicyRequirement =
                    serde_json::from_value(requirement_value.clone())
                        .map_err(|_| PreparationEvidenceError)?;
                if serde_json::to_value(&requirement).map_err(|_| PreparationEvidenceError)?
                    != *requirement_value
                    || requirement.command_sha256 != command_sha256
                    || policy_sha256 != Some(requirement.policy.source_sha256)
                {
                    return Err(PreparationEvidenceError);
                }
                let bytes = text(field(envelope, "response_json")?)?.as_bytes();
                if bytes.len() > maximum {
                    return Err(PreparationEvidenceError);
                }
                let matched = crate::build::policy::match_output_response(
                    request_sha256,
                    bytes,
                    &requirement,
                )
                .map_err(|_| PreparationEvidenceError)?;
                if serde_json::to_value(&matched).map_err(|_| PreparationEvidenceError)?
                    != *response
                {
                    return Err(PreparationEvidenceError);
                }
                output_policy = Some(requirement);
            }
        }
        _ => return Err(PreparationEvidenceError),
    }
    Ok(output_policy)
}

fn decoding(value: &Value) -> Result<()> {
    crate::build::disclosure::technical_profile::decoding_profile(value)
        .map(|_| ())
        .ok_or(PreparationEvidenceError)
}

fn documentation_inputs(value: &Value) -> Result<&Map<String, Value>> {
    let inputs = object(
        value,
        &[
            "configuration",
            "diagrams",
            "math",
            "compilation",
            "site",
            "sources",
        ],
    )?;
    for key in ["configuration", "compilation", "sources"] {
        if !field(inputs, key)?.is_object() {
            return Err(PreparationEvidenceError);
        }
    }
    Ok(inputs)
}

#[cfg(test)]
#[path = "../../../tests/internal/preparation_evidence.rs"]
mod tests;

fn object<'a>(value: &'a Value, keys: &[&str]) -> Result<&'a Map<String, Value>> {
    let map = value.as_object().ok_or(PreparationEvidenceError)?;
    if map.len() != keys.len() || keys.iter().any(|key| !map.contains_key(*key)) {
        return Err(PreparationEvidenceError);
    }
    Ok(map)
}
fn field<'a>(map: &'a Map<String, Value>, key: &str) -> Result<&'a Value> {
    map.get(key).ok_or(PreparationEvidenceError)
}
fn text(value: &Value) -> Result<&str> {
    value.as_str().ok_or(PreparationEvidenceError)
}
fn equal_text(value: &Value, expected: &str) -> Result<()> {
    if text(value)? == expected {
        Ok(())
    } else {
        Err(PreparationEvidenceError)
    }
}
fn profile(value: &Value) -> Result<&str> {
    let value = text(value)?;
    if value.is_empty() || value.len() > 256 || !value.bytes().all(|byte| byte.is_ascii_graphic()) {
        return Err(PreparationEvidenceError);
    }
    Ok(value)
}
fn number(value: &Value) -> Result<u64> {
    value.as_u64().ok_or(PreparationEvidenceError)
}
fn array(value: &Value) -> Result<&[Value]> {
    value
        .as_array()
        .map(Vec::as_slice)
        .ok_or(PreparationEvidenceError)
}
fn digest(value: &Value) -> Result<Sha256Digest> {
    text(value)?.parse().map_err(|_| PreparationEvidenceError)
}
fn path(value: &Value) -> Result<EntryPath> {
    text(value)?.parse().map_err(|_| PreparationEvidenceError)
}
fn tree(value: &Value) -> Result<TreeIdentity> {
    object(value, &["format", "sha256"])?;
    serde_json::from_value(value.clone()).map_err(|_| PreparationEvidenceError)
}
