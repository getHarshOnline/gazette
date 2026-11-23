//! Shared typed documentation reports and discovery schema.

use super::*;

/// Closed native outcome vocabulary shared by every documentation transport.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub enum Status {
    /// A complete local target was assembled and may also have been retained privately.
    #[serde(rename = "documentation-assembled-local")]
    AssembledLocal,
    /// Exact build inputs were durably prepared; hosted output remains uncertified.
    #[serde(rename = "documentation-prepared-local-not-certified")]
    PreparedLocalNotCertified,
}

/// Shared complete local assembly report; no hosted deployment or publication approval is implied.
#[derive(Serialize, JsonSchema)]
pub struct Report<'a> {
    /// Exact preview-only hyperlink projection, separate from configured public destination.
    pub preview_link_bases: Option<&'a gazette_core::publication::routing::PreviewLinkBases>,
    /// Native local assembly status.
    pub status: Status,
    /// Exact selected input and configuration evidence.
    pub inputs: CompositionInspection<'a>,
    /// Configured brand-owned documentation destination, not an output-created receipt.
    pub repository: &'a crate::workspace::DocumentationRepository,
    /// Explicit authored-manual page and asset evidence, never generator output.
    pub manual: &'a [DocumentationManualEvidence<'a>],
    /// Eligible Markdown names omitted from authored navigation and selected reference inputs.
    pub unselected_manual_pages:
        std::collections::BTreeMap<&'a crate::workspace::Key, Vec<&'a crate::source::EntryPath>>,
    /// Explicit reasons metadata enumeration is incomplete, keyed by source.
    pub unselected_manual_pages_incomplete:
        std::collections::BTreeMap<&'a crate::workspace::Key, &'a str>,
    /// Exact selected artifacts and any native generation evidence.
    pub artifacts: &'a [DocumentationArtifactEvidence<'a>],
    /// Complete assembled output inventory.
    pub inventory: &'a crate::source::Inventory,
    /// Actual retained native target, when requested and successfully retained.
    pub directory: Option<&'a Path>,
    /// Complete native generation jobs; imported references have no synthetic jobs.
    pub jobs: &'a [DocumentationJob],
}

impl<'a> Report<'a> {
    pub(super) fn from_assembly(
        status: Status,
        inputs: &'a AcquiredInputs,
        assembled: &'a DocumentationAssembly<'a>,
        directory: Option<&'a Path>,
        jobs: &'a [DocumentationJob],
    ) -> Self {
        Self {
            preview_link_bases: assembled.preview_link_bases(),
            status,
            inputs: inputs.inspect(),
            repository: assembled.repository(),
            manual: assembled.manual(),
            unselected_manual_pages: assembled.unselected_manual_pages(),
            unselected_manual_pages_incomplete: assembled.unselected_manual_pages_incomplete(),
            artifacts: assembled.artifacts(),
            inventory: assembled.output().inventory(),
            directory,
            jobs,
        }
    }
}

/// Complete local assembly plus its actual immutable preparation receipt, not a certificate.
#[derive(Serialize, JsonSchema)]
pub struct PreparedReport<'a> {
    /// Exact original assembly and generator evidence, without a second rendering model.
    #[serde(flatten)]
    pub assembly: Report<'a>,
    /// Native technical retention history; absent checks remain absent.
    pub preparation: &'a store::PreparationReceipt,
}

/// Serialization-mode discovery for the actual complete and compact delivery representations.
pub fn schema() -> serde_json::Map<String, serde_json::Value> {
    #[derive(JsonSchema)]
    #[serde(untagged)]
    #[allow(dead_code)]
    enum Output<'a> {
        Full(Report<'a>),
        Prepared(PreparedReport<'a>),
        Compact(Box<super::custody::Compact<'a>>),
    }
    let mut schema = schemars::generate::SchemaSettings::draft2020_12()
        .for_serialize()
        .into_generator()
        .into_root_schema_for::<Output<'_>>();
    std::mem::take(schema.ensure_object())
}
