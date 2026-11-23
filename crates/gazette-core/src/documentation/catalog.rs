//! Validated provider reading routes and their retained semantic evidence.

use std::collections::BTreeSet;

use serde::Serialize;

use super::projection::PageKind;
use crate::source::{CapturedSource, EntryPath, TreeIdentity};

/// Aggregate catalog bounds, independent from the emitted target tree allowance.
#[derive(Clone, Copy, Debug)]
pub struct CatalogLimits {
    /// Complete physical, virtual and incomplete page registrations.
    pub pages: usize,
    /// Complete available anchor identifiers across all entries.
    pub anchors: usize,
    /// Complete retained route, title, text, anchor and evidence-path bytes.
    pub bytes: usize,
}
impl Default for CatalogLimits {
    fn default() -> Self {
        Self {
            pages: 100_000,
            anchors: 1_000_000,
            bytes: 64 << 20,
        }
    }
}

/// Exact output evidence supplied by the provider that admitted a reading route.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum CatalogEvidence {
    /// An actual provider page, optionally accompanied by native readable Markdown.
    Physical {
        /// Exact file consumed as the provider page.
        provider: EntryPath,
        /// Exact emitted companion; this is not a claim about a public `.md` URL.
        native_markdown: Option<EntryPath>,
    },
    /// A provider-qualified route backed by validated retained input artifacts.
    QualifiedVirtual {
        /// Exact nonempty artifact set required for the provider route.
        artifacts: BTreeSet<EntryPath>,
    },
}

/// One actual provider page. Optional semantic facts are never inferred from filenames.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct CatalogEntry {
    /// Configured module namespace; the application joins its existing project identity.
    pub module: String,
    /// Exact extensionless provider reading route, after target-owned mapping.
    pub route: EntryPath,
    /// Physical or qualified virtual page; incomplete roots are retained separately.
    pub kind: PageKind,
    /// Configured or provider-admitted page title, when available.
    pub title: Option<String>,
    /// Text from the admitted reading representation, when the provider exposes it.
    pub body_text: Option<String>,
    /// Actual emitted target anchors; None means the provider supplies no qualified catalog.
    pub anchors: Option<BTreeSet<String>>,
    /// Retained file evidence, distinct from public reading addresses.
    pub evidence: CatalogEvidence,
}

/// A complete checked catalog bound to the exact immutable target output.
/// Construction belongs to link projection; unvalidated deserialization is unavailable.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct ValidatedCatalog {
    output: TreeIdentity,
    entries: Vec<CatalogEntry>,
    incomplete_roots: BTreeSet<EntryPath>,
}
impl ValidatedCatalog {
    pub(super) fn new(
        output: TreeIdentity,
        entries: Vec<CatalogEntry>,
        incomplete_roots: BTreeSet<EntryPath>,
    ) -> Self {
        Self {
            output,
            entries,
            incomplete_roots,
        }
    }
    /// Exact target output against which physical pages and artifacts were checked.
    pub fn output_identity(&self) -> TreeIdentity {
        self.output
    }
    /// Validated pages in stable reading-route order; native companions are not extra pages.
    pub fn entries(&self) -> &[CatalogEntry] {
        &self.entries
    }
    /// Selected generator roots whose absent artifacts cannot establish usable routes.
    pub fn incomplete_roots(&self) -> &BTreeSet<EntryPath> {
        &self.incomplete_roots
    }
    /// Whether every selected generated root had retained provider evidence.
    pub fn complete(&self) -> bool {
        self.incomplete_roots.is_empty()
    }
}

/// One target assembly with optional explicit catalog support.
/// None means the selected adapter has not supplied validated discovery evidence.
#[derive(Debug)]
pub struct TargetAssembly {
    /// Complete immutable provider input tree, unchanged by catalog projection.
    pub output: CapturedSource,
    /// Validated route/text evidence bound to this exact output tree.
    pub catalog: Option<ValidatedCatalog>,
}

pub(super) struct Description {
    pub module: String,
    pub title: Option<String>,
    pub body_text: Option<String>,
    pub evidence: CatalogEvidence,
}

pub(super) fn evidence_paths(evidence: &CatalogEvidence) -> impl Iterator<Item = &EntryPath> {
    let (provider, native, artifacts) = match evidence {
        CatalogEvidence::Physical {
            provider,
            native_markdown,
        } => (Some(provider), native_markdown.as_ref(), None),
        CatalogEvidence::QualifiedVirtual { artifacts } => (None, None, Some(artifacts)),
    };
    provider
        .into_iter()
        .chain(native)
        .chain(artifacts.into_iter().flatten())
}
