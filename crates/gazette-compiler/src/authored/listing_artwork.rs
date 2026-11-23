//! Decorative owner selections share retained source coordinates and static transport.
use super::{ListingError, Origin, ResolvedAuthored};
use crate::{corpus::File, toml::PathSegment};
use gazette_core::{
    publication::{Key, content::Disposition},
    source::EntryPath,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct ArtworkReference {
    source: Key,
    entry: EntryPath,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct ArtworkSelection {
    default: ArtworkReference,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    light: Option<ArtworkReference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dark: Option<ArtworkReference>,
}
impl ArtworkSelection {
    pub(super) fn references(&self) -> impl Iterator<Item = (&'static str, &ArtworkReference)> {
        std::iter::once(("default", &self.default))
            .chain(self.light.as_ref().map(|reference| ("light", reference)))
            .chain(self.dark.as_ref().map(|reference| ("dark", reference)))
    }
    pub(super) fn len(&self) -> usize {
        1 + usize::from(self.light.is_some()) + usize::from(self.dark.is_some())
    }
}

/// Resolved decorative rendition URLs, without private source coordinates.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct ListingArtwork {
    /// Required authored default, also used when a selected mode has no variant.
    pub default: String,
    /// Explicit light rendition only; no filename or palette inference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub light: Option<String>,
    /// Explicit dark rendition only; no synthesized transformation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dark: Option<String>,
}

/// One exact owner/variant failed retained selection or native static addressing.
#[derive(Debug)]
pub struct ListingArtworkError {
    /// Existing owning content identity.
    pub content: Key,
    /// Exact authored default/light/dark field.
    pub variant: &'static str,
    /// Original selected document/field evidence, never a fabricated body link.
    pub origin: Option<Origin>,
    /// Native refusal reason; no partial public artwork is returned.
    pub reason: String,
}
impl ListingArtworkError {
    /// Stable class independent of the selected renderer or interface.
    pub fn code(&self) -> &'static str {
        "authored_listing_artwork"
    }
}
impl std::fmt::Display for ListingArtworkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "listing artwork {}.{}: {}",
            self.content, self.variant, self.reason
        )
    }
}
impl std::error::Error for ListingArtworkError {}

impl ResolvedAuthored<'_, '_> {
    fn artwork_error(
        &self,
        owner: &Key,
        variant: &'static str,
        reason: impl Into<String>,
    ) -> ListingArtworkError {
        ListingArtworkError {
            content: owner.clone(),
            variant,
            origin: self.origin(
                owner,
                &[
                    PathSegment::Key("listing_artwork".into()),
                    PathSegment::Key(variant.into()),
                ],
            ),
            reason: reason.into(),
        }
    }
    fn artwork_target(
        &self,
        owner: &Key,
        variant: &'static str,
        reference: &ArtworkReference,
    ) -> Result<EntryPath, ListingArtworkError> {
        let fail = |reason| self.artwork_error(owner, variant, reason);
        let node = self
            .content
            .graph()
            .node(owner)
            .ok_or_else(|| fail("owning content is absent"))?;
        let mut bindings = self
            .content
            .compilation()
            .workspace
            .content_bindings()
            .filter(|binding| {
                binding.site == &node.site && binding.source_key() == &reference.source
            });
        let source = bindings.next().ok_or_else(|| {
            fail("artwork source is not an already selected same-site content binding")
        })?;
        if bindings.next().is_some() {
            return Err(fail(
                "artwork source has ambiguous same-site content bindings",
            ));
        }
        let path = source
            .logical_path(&reference.entry)
            .map_err(|_| fail("invalid exact capture-relative artwork entry"))?;
        if source.is_retained_only(&path) {
            return Err(fail("artwork entry is explicitly retained-only"));
        }
        if !matches!(self.corpus.files().get(&path), Some(File::Asset(_)))
            && !self.asset_metadata.contains_key(&path)
        {
            return Err(fail(
                "artwork entry is absent or is not a retained regular opaque file",
            ));
        }
        if !reference
            .entry
            .as_str()
            .rsplit_once('.')
            .is_some_and(|(_, extension)| {
                ["png", "jpg", "jpeg", "gif", "webp", "avif"]
                    .iter()
                    .any(|allowed| extension.eq_ignore_ascii_case(allowed))
            })
        {
            return Err(fail(
                "artwork requires a supported static image transport suffix",
            ));
        }
        if self
            .asset_route(source, &path)
            .map_err(|error| self.artwork_error(owner, variant, error.to_string()))?
            .is_none()
        {
            return Err(fail("artwork entry has no configured static image output"));
        }
        Ok(path)
    }
    pub(super) fn validate_listing_artwork(&self) -> Result<(), ListingArtworkError> {
        for (owner, selection) in &self.listing_artwork {
            for (variant, reference) in selection.references() {
                self.artwork_target(owner, variant, reference)?;
            }
        }
        Ok(())
    }

    /// Opaque retained files required to analyze the selected authored text.
    /// These inputs support editor semantics; they do not select media delivery.
    pub fn authoring_asset_paths(&self) -> impl Iterator<Item = &EntryPath> {
        self.corpus.files().iter().filter_map(move |(path, file)| {
            if !matches!(file, File::Asset(_)) {
                return None;
            }
            if self.is_body_image_input(path) {
                return Some(path);
            }
            let association = self.content.compilation().workspace.content_source(path)?;
            let entry = association.entry(path)?;
            self.listing_artwork
                .iter()
                .any(|(owner, selection)| {
                    self.content
                        .graph()
                        .node(owner)
                        .is_some_and(|node| &node.site == association.site)
                        && selection.references().any(|(_, reference)| {
                            &reference.source == association.source_key()
                                && reference.entry.as_str() == entry
                        })
                })
                .then_some(path)
        })
    }
    pub(super) fn for_each_selected_artwork_asset(
        &self,
        content: &BTreeMap<Key, Disposition>,
        mut require: impl FnMut(&EntryPath) -> Result<(), super::SelectionError>,
    ) -> Result<(), super::SelectionError> {
        for (owner, selection) in &self.listing_artwork {
            if content.get(owner) != Some(&Disposition::Include) {
                continue;
            }
            for (variant, reference) in selection.references() {
                let path = self
                    .artwork_target(owner, variant, reference)
                    .map_err(super::SelectionError::Artwork)?;
                require(&path).map_err(|error| match error {
                    super::SelectionError::Asset { reason, .. } => {
                        super::SelectionError::Artwork(self.artwork_error(owner, variant, reason))
                    }
                    other => other,
                })?;
            }
        }
        Ok(())
    }
    pub(super) fn project_listing_artwork(
        &self,
        owner: &Key,
        records: &mut usize,
        bytes: &mut usize,
    ) -> Result<Option<ListingArtwork>, ListingError> {
        let Some(selection) = self.listing_artwork.get(owner) else {
            return Ok(None);
        };
        let node = self.content.graph().node(owner).ok_or_else(|| {
            ListingError::Artwork(self.artwork_error(owner, "default", "owning content is absent"))
        })?;
        let assets = self.assets_for(&node.site).map_err(|error| {
            ListingError::Artwork(self.artwork_error(owner, "default", error.to_string()))
        })?;
        let mut resolve = |variant, reference: &ArtworkReference| {
            let path = self
                .artwork_target(owner, variant, reference)
                .map_err(ListingError::Artwork)?;
            let asset = assets.get(&path).ok_or_else(|| {
                ListingError::Artwork(self.artwork_error(
                    owner,
                    variant,
                    "declared artwork is excluded or uncompiled in the selected static asset plan",
                ))
            })?;
            *records = records
                .checked_sub(1)
                .ok_or(ListingError::Limit("artwork rendition occurrences"))?;
            *bytes = bytes
                .checked_sub(asset.url.len())
                .ok_or(ListingError::Limit("artwork rendition URL bytes"))?;
            Ok::<String, ListingError>(asset.url.clone())
        };
        let default = resolve("default", &selection.default)?;
        let light = selection
            .light
            .as_ref()
            .map(|reference| resolve("light", reference))
            .transpose()?;
        let dark = selection
            .dark
            .as_ref()
            .map(|reference| resolve("dark", reference))
            .transpose()?;
        Ok(Some(ListingArtwork {
            default,
            light,
            dark,
        }))
    }
}
