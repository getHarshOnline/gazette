//! Explicit durable documentation-preparation selection.

use super::*;

fn raw(path: &Path) -> &[u8] {
    path.as_os_str().as_bytes()
}

/// Optional durable local custody. This never selects a remote or hosted publication.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PreparationSelection {
    /// Existing private journal root; initialization is a separate operation.
    pub journal: PathBuf,
    /// Independently retained expected journal coordinates.
    pub expected: store::JournalHeader,
    /// Existing parent for the exact new named stage, not an inferred brand destination.
    pub parent: PathBuf,
    /// Exact single child name; existing stages are not adopted.
    pub name: String,
    /// Immutable preparation coordinate, selected before effects.
    pub id: Sha256Digest,
    /// Separate permission for named stage creation.
    #[serde(default)]
    pub create_stage: bool,
    /// Separate permission for immutable preparation history.
    #[serde(default)]
    pub record_preparation: bool,
    /// Existing native writer permissions and read bounds.
    #[serde(default)]
    pub writer: store::WriterOptions,
    /// Complete private input-evidence byte ceiling.
    pub evidence_bytes: usize,
}

impl PreparationSelection {
    pub(super) fn required_directory_bytes(&self) -> Result<usize, &'static str> {
        let name: crate::source::Subdirectory = self
            .name
            .parse()
            .map_err(|_| "documentation_preparation_selection")?;
        if !self.journal.is_absolute()
            || !self.parent.is_absolute()
            || name.components().count() != 1
        {
            return Err("documentation_preparation_selection");
        }
        // The preparation owner returns canonical native paths (for example `/private/var`
        // for a selected `/var` parent). Resolve the two required existing roots read-only so
        // response custody is reserved from those exact native spellings before any effect.
        let journal = self
            .journal
            .canonicalize()
            .map_err(|_| "documentation_preparation_selection")?;
        let parent = self
            .parent
            .canonicalize()
            .map_err(|_| "documentation_preparation_selection")?;
        let selected = raw(&journal)
            .len()
            .max(raw(&parent.join(&self.name)).len())
            .max(raw(&self.journal).len())
            .max(raw(&self.parent.join(&self.name)).len())
            .max(1);
        if selected > 1 << 20 {
            return Err("documentation_preparation_selection");
        }
        Ok(selected)
    }

    /// Reject malformed coordinates, missing permissions and unbounded delivery before generation.
    ///
    /// # Errors
    /// Returns a stable selection or permission code without accessing native state.
    pub fn validate(&self, directory_bytes: usize) -> Result<(), &'static str> {
        let required = self.required_directory_bytes()?;
        if required > directory_bytes
            || raw(&self.parent.join(&self.name)).len() > directory_bytes
            || !(1..=16 << 20).contains(&self.evidence_bytes)
        {
            return Err("documentation_preparation_selection");
        }
        if !self.create_stage || !self.record_preparation || self.writer.validate().is_err() {
            return Err("documentation_preparation_permission");
        }
        Ok(())
    }

    /// Borrow exactly the selected native request, without changing journal expectations.
    pub fn request(&self) -> super::PreparationRequest<'_> {
        super::PreparationRequest {
            journal: &self.journal,
            expected: self.expected,
            parent: &self.parent,
            name: &self.name,
            id: self.id,
        }
    }

    /// Share the assembled-tree bounds with the native preparation owner.
    pub fn options(&self, output: crate::source::Limits) -> super::PreparationOptions {
        super::PreparationOptions {
            allow_stage: self.create_stage,
            record: store::PreparationWriteOptions {
                allow_record: self.record_preparation,
            },
            writer: self.writer,
            output,
            evidence_bytes: self.evidence_bytes,
        }
    }
}
