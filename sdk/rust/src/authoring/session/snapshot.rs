//! Exact current source handoff between authoring transports, without filesystem reads.
use super::{Error, Session};
use crate::{
    authoring,
    source::{EntryPath, Sha256Digest},
    workspace::Key,
};
use serde::Serialize;

/// The selected role remains independent of a file's suffix and physical location.
#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourceRole<'a> {
    /// Root configuration document.
    Workspace,
    /// Explicit included configuration document.
    WorkspaceDocument,
    /// Publication source.
    Document,
    /// Authored manual, separate from publication content.
    Manual {
        /// Configured source identity.
        source: &'a Key,
    },
    /// Selected Theme manifest.
    Theme {
        /// Configured package identity.
        package: &'a str,
    },
    /// Selected plugin manifest.
    Plugin {
        /// Configured package identity.
        package: &'a str,
    },
    /// Opaque retained companion; never a text overlay.
    PageInput,
}

/// One source's exact current identity and size, suitable for a bounded chunk transfer.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct SnapshotSource<'a> {
    /// Opaque client identifier, never opened by this operation.
    pub id: &'a str,
    /// Native logical input path.
    pub path: &'a EntryPath,
    /// Role established by the original selection.
    pub role: SourceRole<'a>,
    /// Current accepted editor version, if open.
    pub version: Option<i32>,
    /// Digest of current bytes, including unsaved text.
    pub source_sha256: Sha256Digest,
    /// Saved baseline digest; an open version alone does not mean unsaved text.
    pub baseline_sha256: Sha256Digest,
    /// Complete byte length; chunks do not change it.
    pub bytes: usize,
}

/// A generation-bound manifest; it is not a build or publication receipt.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct Snapshot<'a> {
    /// Current source generation.
    pub generation: u64,
    /// Whether all analysis phases succeeded.
    pub structurally_valid: bool,
    /// Whether the root uses an explicit included-document set.
    pub modular: bool,
    /// Captured source set, including retained opaque companions.
    pub sources: Vec<SnapshotSource<'a>>,
    /// Observed opaque inputs have no captured bytes and cannot supply source chunks.
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub asset_observations: &'a std::collections::BTreeMap<String, super::AssetObservation>,
}

impl Session {
    /// Describe the complete current selection without reading or changing files.
    pub fn snapshot(&self) -> Snapshot<'_> {
        let mut sources = Vec::with_capacity(self.files.len() + self.page_inputs.len());
        for (id, document) in &self.files {
            let role = if id == &self.workspace {
                SourceRole::Workspace
            } else if self
                .workspace_documents
                .as_ref()
                .is_some_and(|set| set.contains(id))
            {
                SourceRole::WorkspaceDocument
            } else if let Some(source) = self.manuals.get(id) {
                SourceRole::Manual { source }
            } else if let Some(package) = self.package_baselines.get(id) {
                match package {
                    authoring::PackageBaselineKey::Theme(package) => SourceRole::Theme {
                        package: package.as_str(),
                    },
                    authoring::PackageBaselineKey::Plugin(package) => SourceRole::Plugin {
                        package: package.as_str(),
                    },
                }
            } else {
                SourceRole::Document
            };
            sources.push(SnapshotSource {
                id,
                path: document.path(),
                role,
                version: document.version(),
                source_sha256: Sha256Digest::of(document.text().as_bytes()),
                baseline_sha256: Sha256Digest::of(document.baseline.as_bytes()),
                bytes: document.text().len(),
            });
        }
        for (id, input) in &self.page_inputs {
            sources.push(SnapshotSource {
                id,
                path: input.path(),
                role: SourceRole::PageInput,
                version: None,
                source_sha256: input.source_sha256(),
                baseline_sha256: input.source_sha256(),
                bytes: input.bytes.len(),
            });
        }
        Snapshot {
            generation: self.generation,
            structurally_valid: self
                .analysis
                .as_ref()
                .is_ok_and(|a| a.failures().is_empty()),
            modular: self.workspace_documents.is_some(),
            sources,
            asset_observations: &self.asset_observations,
        }
    }

    /// Read at most 64 KiB of exact retained bytes at the supplied generation.
    ///
    /// # Errors
    /// Refuses a stale generation, unknown source or invalid range. Callers must
    /// verify the complete digest before importing a reconstructed snapshot.
    pub fn source_chunk(
        &self,
        generation: u64,
        id: &str,
        offset: usize,
        length: usize,
    ) -> Result<&[u8], Error> {
        if generation != self.generation {
            return Err(Error::Selection("stale snapshot generation"));
        }
        if length == 0 || length > 65536 {
            return Err(Error::Limit("snapshot chunk bytes"));
        }
        let bytes = if let Some(document) = self.files.get(id) {
            document.text().as_bytes()
        } else if let Some(input) = self.page_inputs.get(id) {
            &input.bytes
        } else {
            return Err(Error::Selection("unknown snapshot source"));
        };
        let end = offset
            .checked_add(length)
            .ok_or(Error::Limit("snapshot byte range"))?
            .min(bytes.len());
        bytes
            .get(offset..end)
            .ok_or(Error::Selection("invalid snapshot byte range"))
    }
}
