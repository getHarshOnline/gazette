//! Observed asset availability remains separate from retained bytes and text overlays.

use super::*;

/// A client-selected opaque file observation, without captured bytes or a digest.
pub struct AssetObservationSource {
    /// Client identifier; this layer never opens it.
    pub id: String,
    /// Logical path under a configured publication source.
    pub path: EntryPath,
    /// File size observed by discovery, not verified content identity.
    pub observed_bytes: u64,
}

/// Metadata retained for editor navigation; it cannot be transferred as source bytes.
#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct AssetObservation {
    path: EntryPath,
    observed_bytes: u64,
}

impl AssetObservation {
    /// Logical source coordinate.
    pub fn path(&self) -> &EntryPath {
        &self.path
    }
    /// Observed size, which can change before a later capture.
    pub fn observed_bytes(&self) -> u64 {
        self.observed_bytes
    }
}

impl Session {
    /// Add opaque observations and reanalyze the current text with their availability.
    /// Observations cannot overlap retained inputs or be opened as text overlays.
    ///
    /// # Errors
    /// Refuses ambiguous identities, paths and exhausted selection bounds.
    pub fn with_asset_observations(
        mut self,
        sources: impl IntoIterator<Item = AssetObservationSource>,
    ) -> Result<Self, Error> {
        let mut paths: BTreeSet<_> = self
            .files
            .values()
            .map(|file| file.analysis_path().clone())
            .chain(self.page_inputs.values().map(|file| file.path().clone()))
            .chain(
                self.asset_observations
                    .values()
                    .map(|file| file.path().clone()),
            )
            .collect();
        let mut path_bytes = self
            .files
            .iter()
            .map(|(id, file)| {
                file.path()
                    .as_str()
                    .len()
                    .checked_add(self.manuals.get(id).map_or(0, |key| key.as_str().len()))
            })
            .chain(
                self.page_inputs
                    .values()
                    .map(|file| Some(file.path().as_str().len())),
            )
            .chain(
                self.asset_observations
                    .values()
                    .map(|file| Some(file.path().as_str().len())),
            )
            .try_fold(0usize, |sum, length| sum.checked_add(length?))
            .ok_or(Error::Limit("selected path bytes"))?;
        let mut identifier_bytes = self
            .files
            .keys()
            .chain(self.page_inputs.keys())
            .chain(self.asset_observations.keys())
            .try_fold(0usize, |sum, id| sum.checked_add(id.len()))
            .ok_or(Error::Limit("selected identifier bytes"))?;
        for source in sources {
            if !paths.insert(source.path.clone())
                || self.files.contains_key(&source.id)
                || self.page_inputs.contains_key(&source.id)
                || self.asset_observations.contains_key(&source.id)
            {
                return Err(Error::Selection(
                    "duplicate observed source path or identifier",
                ));
            }
            path_bytes = path_bytes
                .checked_add(source.path.as_str().len())
                .filter(|bytes| *bytes <= self.limits.analysis.corpus.path_bytes)
                .ok_or(Error::Limit("selected path bytes"))?;
            identifier_bytes = identifier_bytes
                .checked_add(source.id.len())
                .filter(|bytes| *bytes <= self.limits.identifier_bytes)
                .ok_or(Error::Limit("selected identifier bytes"))?;
            if self.files.len() + self.page_inputs.len() + self.asset_observations.len()
                >= self.limits.documents
            {
                return Err(Error::Limit("selected documents"));
            }
            self.asset_observations.insert(
                source.id,
                AssetObservation {
                    path: source.path,
                    observed_bytes: source.observed_bytes,
                },
            );
        }
        let generation = self.next_generation()?;
        self.refresh(generation);
        Ok(self)
    }

    /// Current metadata observations, without source-byte or revision claims.
    pub fn asset_observations(&self) -> &BTreeMap<String, AssetObservation> {
        &self.asset_observations
    }
}
