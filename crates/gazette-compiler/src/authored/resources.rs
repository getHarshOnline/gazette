//! Ordered page-local programs, separate from authored identities and relationships.

use std::collections::{BTreeMap, BTreeSet};

use gazette_core::{
    publication::{Key, routing::RoutePath},
    resource,
    source::EntryPath,
};
use serde::{Deserialize, Deserializer, Serialize, de::Error};

use super::ResolvedAuthored;

/// Explicit source closure and destination for one page's supplementary resource.
/// Inputs are literal paths within the page's existing configured source capture.
/// Declaration order is observable presentation order, not a set to sort.
#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PageResource {
    /// Use this raster output for image references to its original entry on this page.
    /// Other size variants remain independent resources unless explicitly bound.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub rendition: bool,
    /// Additional proportional size of the same original as its bound rendition.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub responsive: bool,
    /// Complete allowed input set; imports cannot escape to ambient files or packages.
    #[serde(deserialize_with = "paths")]
    pub inputs: Vec<EntryPath>,
    /// Existing provider-independent resource recipe, not a second compiler profile.
    pub build: resource::Recipe,
    /// Explicit site-relative public destination, independent of native input names.
    pub output: RoutePath,
}

impl ResolvedAuthored<'_, '_> {
    /// Borrow the page's ordered declarations. Unknown/no-resource objects return empty;
    /// callers must independently resolve the exact page and its source binding.
    pub fn resources(&self, id: &Key) -> &[PageResource] {
        self.resources.get(id).map_or(&[], Vec::as_slice)
    }
}

pub(super) fn validate(resources: &mut [PageResource]) -> Result<(), resource::Error> {
    let mut outputs = BTreeSet::new();
    let mut renditions = BTreeSet::new();
    for declaration in resources.iter_mut() {
        declaration.build.validate_imports()?;
        declaration.inputs.sort_unstable();
        let reason = if declaration.rendition && declaration.responsive {
            Some("a responsive candidate cannot also be the fallback rendition")
        } else if (declaration.rendition || declaration.responsive)
            && !matches!(declaration.build, resource::Recipe::RasterImage { .. })
        {
            Some("image rendition binding requires a raster image recipe")
        } else if declaration.rendition && !renditions.insert(declaration.build.entry().clone()) {
            Some("an original image may have only one bound rendition per page")
        } else if declaration.output.as_str().ends_with('/') {
            Some("page resource output must be an explicit file route")
        } else if declaration.inputs.is_empty() {
            Some("page resource needs explicit inputs")
        } else if declaration.inputs.windows(2).any(|pair| pair[0] == pair[1]) {
            Some("page resource inputs must be unique")
        } else if declaration
            .inputs
            .binary_search(declaration.build.entry())
            .is_err()
        {
            Some("page resource entry must belong to its declared inputs")
        } else if !outputs.insert(declaration.output.output()) {
            Some("page resources claim the same output")
        } else {
            None
        };
        if let Some(reason) = reason {
            return Err(resource::Error::Input {
                path: declaration.build.entry().clone(),
                reason,
            });
        }
    }
    let fallbacks: BTreeMap<_, _> = resources
        .iter()
        .filter(|resource| resource.rendition)
        .map(|resource| (resource.build.entry(), resource))
        .collect();
    let mut counts = BTreeMap::new();
    for candidate in resources.iter().filter(|resource| resource.responsive) {
        let fallback = fallbacks.get(candidate.build.entry());
        let compatible =
            fallback.is_some_and(|fallback| match (&fallback.build, &candidate.build) {
                (
                    resource::Recipe::RasterImage {
                        input_format: a,
                        resize: ar,
                        apply_orientation: ao,
                        ..
                    },
                    resource::Recipe::RasterImage {
                        input_format: b,
                        resize: br,
                        apply_orientation: bo,
                        ..
                    },
                ) => a == b && ao == bo && proportional(*ar) && proportional(*br),
                _ => false,
            });
        if !compatible {
            return Err(resource::Error::Input {
                path: candidate.build.entry().clone(),
                reason: "responsive images require a fallback from the same original, input format and orientation, with proportional resize recipes",
            });
        }
        let count = counts.entry(candidate.build.entry()).or_insert(0usize);
        *count += 1;
        if *count > 16 {
            return Err(resource::Error::Limit("responsive image candidates"));
        }
    }
    Ok(())
}

fn proportional(resize: gazette_core::raster::Resize) -> bool {
    matches!(
        resize,
        gazette_core::raster::Resize::FitWidth { .. }
            | gazette_core::raster::Resize::FitHeight { .. }
            | gazette_core::raster::Resize::Fit { .. }
    )
}

pub(super) fn paths<'de, D: Deserializer<'de>>(de: D) -> Result<Vec<EntryPath>, D::Error> {
    Vec::<String>::deserialize(de)?
        .into_iter()
        .map(|path| path.parse().map_err(D::Error::custom))
        .collect()
}
