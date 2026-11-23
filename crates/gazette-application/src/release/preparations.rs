//! Complete selected local preparation evidence, separate from release permission.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    path::{Path, PathBuf},
};

use gazette_core::{
    publication::Key,
    release::{PublicationDestination, RemoteState, Target},
    render::LimitedWriter,
    source::{CommitId, Content, Sha256Digest, TreeIdentity},
    store::{JournalHeader, PreparationRecord},
};
use gazette_ports::{
    destination::RootSeparation,
    release::{ReleaseWorktrees, WorktreeReader},
    store::PreparationReader,
};
use serde::{Deserialize, Serialize};

use super::{
    JournalWorktreeInspectionError, JournalWorktreeInspectionOptions,
    JournalWorktreeInspectionRequest, inspect_journal_worktree,
};

/// One independently selected preparation and its explicit publication association.
/// Domain association and remote pre-state are caller assertions, not inferred
/// from a site key, source path, Git remote or native observation.
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PreparationSelection {
    /// Exact existing immutable journal record; never a latest-record query.
    pub preparation_id: Sha256Digest,
    /// Must match the site in the original retained acquisition/render evidence.
    pub site: Key,
    /// Exact independently configured domain associated with this site.
    pub domain: String,
    /// Independently retained completed commit coordinate.
    pub commit: CommitId,
    /// Independently observed remote state, not refreshed by local inspection.
    pub expected_remote_oid_or_unborn: RemoteState,
    /// Independent required historical checker binding. Omission inspects native history
    /// without qualifying its checker; never learn this expectation from the selected row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_policy: Option<crate::build::OutputPolicyRequirement>,
    /// Independent typed admission requirement. Omission remains historical inspection,
    /// even when the record contains a valid technical certificate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qualification: Option<PreparationAdmission>,
}

/// Exact technical admission profile selected for one retained artifact class.
///
/// Authored publication policy and generated-reference qualification remain distinct; neither
/// variant grants publisher authority or permits one artifact class to impersonate the other.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum PreparationAdmission {
    /// Authored output requires a captured candidate decision and the same output policy.
    Authored {
        /// Exact retained supported-profile qualification.
        qualification: Box<crate::build::PreparationQualification>,
    },
    /// Generated reference documentation requires its native generation/assembly profile.
    GeneratedDocumentation {
        /// Exact retained documentation qualification without an invented authored policy.
        qualification: Box<crate::build::DocumentationPreparationQualification>,
    },
}

impl PreparationAdmission {
    /// Construct the authored technical profile without exposing variant layout to callers.
    pub fn authored(qualification: crate::build::PreparationQualification) -> Self {
        Self::Authored {
            qualification: Box::new(qualification),
        }
    }

    /// Borrow authored qualification only; generated documentation never masquerades as it.
    pub fn as_authored(&self) -> Option<&crate::build::PreparationQualification> {
        match self {
            Self::Authored { qualification } => Some(qualification),
            Self::GeneratedDocumentation { .. } => None,
        }
    }

    /// Mutably borrow authored qualification only.
    pub fn as_authored_mut(&mut self) -> Option<&mut crate::build::PreparationQualification> {
        match self {
            Self::Authored { qualification } => Some(qualification),
            Self::GeneratedDocumentation { .. } => None,
        }
    }
}

/// One complete ordered site set under independently expected journal coordinates.
#[derive(Clone, Copy, Debug)]
pub struct PreparationBatchRequest<'a> {
    /// Existing private database, not a root to initialize or migrate.
    pub database: &'a Path,
    /// Exact expected current header for every read in this batch.
    pub expected: JournalHeader,
    /// Complete ordered selection; no required target may silently disappear.
    pub selections: &'a [PreparationSelection],
    /// All configured native inputs that must remain separate from the journal/output.
    pub protected_roots: &'a [PathBuf],
}

/// Native inspection and deterministic evidence work ceilings.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct PreparationBatchOptions {
    /// Applied per target, including explicit `SQLite` coordination-file permission.
    pub inspection: JournalWorktreeInspectionOptions,
    /// Maximum selected targets; each requires independent full native inspection.
    #[schemars(range(max = usize::MAX))]
    pub targets: usize,
    /// Maximum bytes per retained document, encoded preparation and batch projection.
    #[schemars(range(max = usize::MAX))]
    pub evidence_bytes: usize,
}
impl Default for PreparationBatchOptions {
    fn default() -> Self {
        Self {
            inspection: Default::default(),
            targets: 256,
            evidence_bytes: 1 << 20,
        }
    }
}

/// Deterministic identities of original local checks; none is a certification claim.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct PreparationEvidenceDigests {
    /// Ordered preparation-reference/output/original-render-evidence projection.
    pub validation_evidence_digest: Sha256Digest,
    /// Ordered exact retained disclosure-projection identities.
    pub disclosure_evidence_digest: Sha256Digest,
    /// Ordered exact retained tagged policy-projection identities, including absence.
    pub policy_evidence_digest: Sha256Digest,
    /// Common exact scanner profile; mixed profiles are explicitly unsupported.
    pub scanner_revision: String,
    /// Common generic scanning-procedure identity.
    pub generic_rule_set_digest: Sha256Digest,
    /// Ordered exact candidate/output Website policy identities, including absence.
    pub website_policy_projection_digest: Sha256Digest,
}

/// Closed technical disposition of one inspected preparation batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub enum PreparationStatus {
    /// Every selection has supported-profile technical qualification.
    #[serde(rename = "supported-profile-technical-preparations")]
    SupportedProfileTechnical,
    /// Local preparation history was inspected without certification.
    #[serde(rename = "local-preparations-inspected-not-certified")]
    InspectedNotCertified,
}

/// Private joined evidence. Only an independently qualified complete selection receives
/// supported-profile technical status; neither mode grants full certification or release.
/// Native reads are sequential and bracketed, not an atomic cross-worktree snapshot.
/// No public constructor or deserializer can turn supplied JSON into this observation.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct PreparationBatch {
    status: PreparationStatus,
    journal: JournalHeader,
    selections: Vec<PreparationSelection>,
    ordered_targets: Vec<Target>,
    evidence: PreparationEvidenceDigests,
}
impl PreparationBatch {
    /// Observed exact journal header; this does not hold its lifetime writer lock.
    pub fn journal(&self) -> JournalHeader {
        self.journal
    }
    /// Entire ordered native target set; destinations remain private operator evidence.
    pub fn targets(&self) -> &[Target] {
        &self.ordered_targets
    }
    /// Local evidence identities for comparison with an independently authorized body.
    pub fn evidence(&self) -> &PreparationEvidenceDigests {
        &self.evidence
    }
}

/// No partial selected-set evidence is returned when a target or proof is refused.
#[derive(Debug)]
pub enum PreparationBatchError {
    /// Missing, duplicate, invalid or over-limit selection/evidence.
    Selection,
    /// Exact native journal or worktree inspection failed.
    Inspection(JournalWorktreeInspectionError),
    /// Retained history lacks completion, check evidence or a selected destination.
    Incomplete,
    /// Site, history, current output or common check profiles disagree.
    Mismatch,
    /// Stored application evidence is malformed, unsupported or incorrectly linked.
    Evidence,
    /// Release admission lacks one exact candidate/output policy identity or qualification.
    Admission,
    /// A deterministic projection cannot fit its configured byte bound.
    Encoding,
}
impl PreparationBatchError {
    /// Redacted stable category; no untrusted path, domain or evidence is displayed.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Selection => "release_preparation_selection",
            Self::Inspection(error) => error.code(),
            Self::Incomplete => "release_preparation_incomplete",
            Self::Mismatch => "release_preparation_mismatch",
            Self::Evidence => "release_preparation_evidence",
            Self::Admission => "release_preparation_admission",
            Self::Encoding => "release_preparation_encoding",
        }
    }
}
impl fmt::Display for PreparationBatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}; no complete preparation batch returned; preserve native state",
            self.code()
        )
    }
}
impl std::error::Error for PreparationBatchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Inspection(error) => Some(error),
            _ => None,
        }
    }
}

#[derive(Serialize)]
struct ValidationRow {
    ordinal: u32,
    preparation_ref: Sha256Digest,
    output: TreeIdentity,
    preparation_evidence_sha256: Sha256Digest,
}
#[derive(Serialize)]
struct EvidenceRow {
    ordinal: u32,
    sha256: Sha256Digest,
}
#[derive(Serialize)]
struct WebsitePolicyRow {
    ordinal: u32,
    candidate: Option<crate::build::ReviewPolicy>,
    output: Option<crate::build::ReviewPolicy>,
}
#[derive(Serialize)]
struct Projection<'a, T: ?Sized> {
    format: &'static str,
    rows: &'a T,
}

pub(super) fn digest(
    value: &impl Serialize,
    maximum: usize,
) -> Result<Sha256Digest, PreparationBatchError> {
    let mut writer = LimitedWriter::new(Vec::new(), maximum);
    let encoded = serde_json::to_writer(&mut writer, value);
    if writer.exceeded() || encoded.is_err() {
        return Err(PreparationBatchError::Encoding);
    }
    Ok(Sha256Digest::of(&writer.into_inner()))
}

fn preparation_ref(
    record: &PreparationRecord,
    maximum: usize,
) -> Result<Sha256Digest, PreparationBatchError> {
    #[derive(Serialize)]
    struct Reference<'a> {
        format: &'static str,
        preparation: &'a PreparationRecord,
    }
    // Only immutable original records: not inserted/replayed, current generation,
    // wall clock, current provider observation or a mutable filesystem path lookup.
    digest(
        &Reference {
            format: "gazette.prepared-local/1",
            preparation: record,
        },
        maximum,
    )
}

/// Inspect every selected completed preparation and derive exact ordered references.
///
/// Original record addresses and recipes drive native reads. No source is reopened,
/// output rebuilt, journal written or Git operation repaired. Each worktree/history
/// read uses the existing inspector and the same expected journal header. Caller
/// domain/site associations and remote pre-states remain assertions; this neither
/// acquires publisher authority nor certifies semantic or disclosure completeness.
/// Trusted exclusive local namespace control is required throughout observation.
///
/// # Errors
/// Refuses the entire batch on invalid selection, missing phases, changed native
/// bytes/history, incorrect evidence, mixed scanner profiles or exhausted bounds.
pub fn inspect_preparations<F, G, S>(
    filesystem: &F,
    git: &G,
    store: &S,
    request: PreparationBatchRequest<'_>,
    options: PreparationBatchOptions,
) -> Result<PreparationBatch, PreparationBatchError>
where
    F: RootSeparation + WorktreeReader,
    G: ReleaseWorktrees,
    S: PreparationReader,
{
    use PreparationBatchError as Error;
    let selections = request.selections;
    if selections.is_empty() || selections.len() > options.targets || options.evidence_bytes == 0 {
        return Err(Error::Selection);
    }
    let (mut ids, mut sites, mut domains) = (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
    for selection in selections {
        if let Some(required) = &selection.qualification {
            match required {
                PreparationAdmission::Authored { qualification } => {
                    if qualification
                        .candidate_policy
                        .as_ref()
                        .is_some_and(|candidate| {
                            candidate.policy != qualification.output_policy.policy
                        })
                    {
                        return Err(Error::Admission);
                    }
                    qualification.validate().map_err(|_| Error::Selection)?;
                    if selection
                        .output_policy
                        .as_ref()
                        .is_some_and(|policy| policy != &qualification.output_policy)
                    {
                        return Err(Error::Selection);
                    }
                }
                PreparationAdmission::GeneratedDocumentation { qualification } => {
                    qualification.validate().map_err(|_| Error::Selection)?;
                }
            }
        }
        if let Some(required) = &selection.output_policy {
            required.validate().map_err(|_| Error::Selection)?;
        }
        if !ids.insert(selection.preparation_id.as_bytes())
            || !sites.insert(&selection.site)
            || !domains.insert(&selection.domain)
            || selection.domain.is_empty()
            || selection.domain.trim() != selection.domain
            || selection.domain.chars().any(char::is_control)
            || selection.domain.len() > options.evidence_bytes
        {
            return Err(Error::Selection);
        }
    }
    let mut targets = Vec::new();
    let (mut validation, mut disclosure, mut policy, mut website) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut profiles = None;
    let mut destinations = BTreeSet::new();
    for (ordinal, selected) in selections.iter().enumerate() {
        let inspected = inspect_journal_worktree(
            filesystem,
            git,
            store,
            JournalWorktreeInspectionRequest {
                database: request.database,
                preparation_id: selected.preparation_id,
                expected_header: Some(&request.expected),
                commit: &selected.commit,
                protected_roots: request.protected_roots,
            },
            options.inspection,
        )
        .map_err(Error::Inspection)?;
        let record = inspected.history.record.as_ref().ok_or(Error::Incomplete)?;
        let recipe = record.worktree.as_ref().ok_or(Error::Incomplete)?;
        if recipe.completion.is_none() || record.check.is_none() {
            return Err(Error::Incomplete);
        }
        if !matches!(
            (
                &selected.expected_remote_oid_or_unborn,
                &recipe.metadata.parent
            ),
            (RemoteState::Unborn, None)
        ) && !matches!((&selected.expected_remote_oid_or_unborn, &recipe.metadata.parent),
                (RemoteState::Commit { oid }, Some(parent)) if oid == parent)
        {
            return Err(Error::Mismatch);
        }
        let Some(PublicationDestination::Git { remote, branch }) = &record.intent.destination
        else {
            return Err(Error::Incomplete);
        };
        if !destinations.insert((remote.clone(), branch.clone())) {
            return Err(Error::Selection);
        }
        let evidence = crate::build::inspect_preparation_evidence(record, options.evidence_bytes)
            .map_err(|_| Error::Evidence)?;
        if selected.qualification.as_ref().is_some_and(|required| {
            use crate::build::PreparationAdmissionEvidence as Evidence;
            !matches!(
                (required, evidence.admission.as_ref()),
                (
                    PreparationAdmission::Authored { qualification: required },
                    Some(Evidence::Authored(actual))
                ) if required == actual
            ) && !matches!(
                (required, evidence.admission.as_ref()),
                (
                    PreparationAdmission::GeneratedDocumentation { qualification: required },
                    Some(Evidence::GeneratedDocumentation(actual))
                ) if required == actual
            )
        }) {
            return Err(Error::Mismatch);
        }
        if selected
            .output_policy
            .as_ref()
            .is_some_and(|required| evidence.output_policy.as_ref() != Some(required))
        {
            return Err(Error::Mismatch);
        }
        let files: BTreeMap<_, _> = inspected
            .worktree
            .output
            .entries()
            .iter()
            .filter_map(|entry| match entry.content {
                Content::File { sha256, .. } => Some((entry.path.clone(), sha256)),
                Content::Directory => None,
            })
            .collect();
        let paths: BTreeSet<_> = inspected
            .worktree
            .output
            .entries()
            .iter()
            .map(|entry| &entry.path)
            .collect();
        if evidence.site != selected.site
            || evidence.files != files
            || evidence
                .embedded_paths
                .iter()
                .any(|path| !paths.contains(path))
        {
            return Err(Error::Mismatch);
        }
        let profile = (evidence.scanner, evidence.generic_rules_sha256);
        if profiles
            .as_ref()
            .is_some_and(|expected| expected != &profile)
        {
            return Err(Error::Mismatch);
        }
        profiles = Some(profile);
        let ordinal = u32::try_from(ordinal).map_err(|_| Error::Selection)?;
        let reference = preparation_ref(record, options.evidence_bytes)?;
        targets.push(Target {
            ordinal,
            domain: selected.domain.clone(),
            remote: remote.clone(),
            branch: branch.clone(),
            refspec: format!("{}:refs/heads/{branch}", selected.commit),
            expected_remote_oid_or_unborn: selected.expected_remote_oid_or_unborn.clone(),
            prepared_release_receipt_ref: reference,
            local_commit_oid: selected.commit.clone(),
            output_digest: record.intent.output.sha256(),
        });
        validation.push(ValidationRow {
            ordinal,
            preparation_ref: reference,
            output: record.intent.output,
            preparation_evidence_sha256: evidence.preparation_sha256,
        });
        disclosure.push(EvidenceRow {
            ordinal,
            sha256: evidence.disclosure_sha256,
        });
        policy.push(EvidenceRow {
            ordinal,
            sha256: evidence.policy_sha256,
        });
        website.push(WebsitePolicyRow {
            ordinal,
            candidate: evidence
                .candidate_policy
                .as_ref()
                .map(|requirement| requirement.policy.clone()),
            output: evidence
                .output_policy
                .as_ref()
                .map(|requirement| requirement.policy.clone()),
        });
    }
    let (scanner_revision, generic_rule_set_digest) = profiles.ok_or(Error::Selection)?;
    let evidence = PreparationEvidenceDigests {
        validation_evidence_digest: digest(
            &Projection {
                format: "gazette.prepared-validation/1",
                rows: &validation,
            },
            options.evidence_bytes,
        )?,
        disclosure_evidence_digest: digest(
            &Projection {
                format: "gazette.prepared-disclosure/1",
                rows: &disclosure,
            },
            options.evidence_bytes,
        )?,
        policy_evidence_digest: digest(
            &Projection {
                format: "gazette.prepared-policy/1",
                rows: &policy,
            },
            options.evidence_bytes,
        )?,
        scanner_revision,
        generic_rule_set_digest,
        website_policy_projection_digest: digest(
            &Projection {
                format: "gazette.prepared-website-policy/1",
                rows: &website,
            },
            options.evidence_bytes,
        )?,
    };
    let batch = PreparationBatch {
        status: if selections
            .iter()
            .all(|selection| selection.qualification.is_some())
        {
            PreparationStatus::SupportedProfileTechnical
        } else {
            PreparationStatus::InspectedNotCertified
        },
        journal: request.expected,
        selections: selections.to_vec(),
        ordered_targets: targets,
        evidence,
    };
    digest(&batch, options.evidence_bytes)?;
    Ok(batch)
}

/// A complete native preparation set whose retained candidate and output checks
/// use one exact Website policy identity and whose technical qualification was
/// independently required. This private typed state is release plumbing only:
/// it cannot be deserialized and does not grant publisher authority.
pub(super) struct AdmittedPreparationBatch(PreparationBatch);

impl AdmittedPreparationBatch {
    pub(super) fn batch(&self) -> &PreparationBatch {
        &self.0
    }
}

/// Inspect historical evidence, then require the stronger admission invariants used
/// by release authorization. Generic inspection remains available for local history.
pub(super) fn inspect_admitted_preparations<F, G, S>(
    filesystem: &F,
    git: &G,
    store: &S,
    request: PreparationBatchRequest<'_>,
    options: PreparationBatchOptions,
) -> Result<AdmittedPreparationBatch, PreparationBatchError>
where
    F: RootSeparation + WorktreeReader,
    G: ReleaseWorktrees,
    S: PreparationReader,
{
    let batch = inspect_preparations(filesystem, git, store, request, options)?;
    if batch.selections.iter().any(|selection| {
        let Some(admission) = &selection.qualification else {
            return true;
        };
        match admission {
            PreparationAdmission::Authored { qualification } => {
                let Some(candidate) = &qualification.candidate_policy else {
                    return true;
                };
                candidate.policy != qualification.output_policy.policy
            }
            PreparationAdmission::GeneratedDocumentation { .. } => false,
        }
    }) {
        return Err(PreparationBatchError::Admission);
    }
    Ok(AdmittedPreparationBatch(batch))
}
