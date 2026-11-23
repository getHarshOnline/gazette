//! Change discovered membership without resetting versions or analysis generations.

use super::*;

impl Session {
    pub(crate) fn reselect(&mut self, mut next: Self) -> Result<(), Error> {
        let generation = self.next_generation()?;
        for (id, previous) in &self.files {
            let (Some(overlay), Some(document)) = (&previous.overlay, next.files.get_mut(id))
            else {
                continue;
            };
            next.retained_bytes = next
                .retained_bytes
                .checked_sub(
                    document
                        .overlay
                        .as_ref()
                        .map_or(0, |value| value.text.len()),
                )
                .and_then(|bytes| bytes.checked_add(overlay.text.len()))
                .filter(|bytes| *bytes <= next.limits.source_bytes)
                .ok_or(Error::Limit("retained source bytes"))?;
            document.overlay = Some(Overlay {
                version: overlay.version,
                text: overlay.text.clone(),
            });
        }
        next.refresh(generation);
        // A configuration edit can trigger discovery without changing the selected
        // inputs. Only that exact case may retain its completion-only parse.
        if self.workspace == next.workspace
            && self.workspace_documents == next.workspace_documents
            && self.manuals == next.manuals
            && self.package_baselines == next.package_baselines
            && self.files.len() == next.files.len()
            && self.files.iter().all(|(id, file)| {
                next.files.get(id).is_some_and(|other| {
                    file.path() == other.path()
                        && file.analysis_path() == other.analysis_path()
                        && file.text() == other.text()
                })
            })
            && self.page_inputs.len() == next.page_inputs.len()
            && self.page_inputs.iter().all(|(id, input)| {
                next.page_inputs
                    .get(id)
                    .is_some_and(|other| input.path() == other.path() && input.bytes == other.bytes)
            })
            && self.asset_observations.len() == next.asset_observations.len()
            && self.asset_observations.iter().all(|(id, input)| {
                next.asset_observations.get(id).is_some_and(|other| {
                    input.path() == other.path() && input.observed_bytes() == other.observed_bytes()
                })
            })
        {
            next.completion_basis = self.completion_basis.take().filter(|basis| {
                next.retained_bytes.saturating_add(basis.source.len()) <= next.limits.source_bytes
            });
        }
        *self = next;
        Ok(())
    }

    pub(crate) fn unavailable(&mut self, reason: &'static str) -> Result<(), Error> {
        self.generation = self.next_generation()?;
        self.completion_basis = None;
        self.analysis = Err(authoring::Error::UnavailableSources(reason));
        Ok(())
    }
}
