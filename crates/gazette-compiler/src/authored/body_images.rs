//! Explicit body-image renditions keep source identity separate from display mode.

use std::{
    collections::{BTreeMap, BTreeSet},
    num::NonZeroU32,
};

use gazette_core::{
    publication::{Key, content::Disposition},
    source::{EntryPath, Span},
};
use serde::{Deserialize, Serialize};

mod inline_svg;
mod responsive;

use super::{
    AssetPlan, FileError, FileFailure, Origin, ResolvedAuthored, SelectionError, StaticAsset,
};
use crate::{
    corpus::File,
    markdown::{self, source_path},
};

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct ImageDeclaration {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    loading: Option<gazette_core::markdown::ImageLoading>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fetch_priority: Option<gazette_core::markdown::ImageFetchPriority>,
    /// Image reference relative to the owning page, matched by resolved source identity.
    default: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    light: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dark: Option<String>,
    /// Intended display width in CSS pixels, independent of raster resolution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    width: Option<NonZeroU32>,
    /// Explicit browser layout sizes for this image's responsive resource candidates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1, max = 1024))]
    sizes: Option<String>,
    /// Embed a strictly validated SVG while retaining its external image address.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    inline: bool,
}

impl ImageDeclaration {
    fn references(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.default.as_str())
            .chain(self.light.as_deref())
            .chain(self.dark.as_deref())
    }

    pub(super) fn len(&self) -> usize {
        self.references().count()
    }
}

impl ResolvedAuthored<'_, '_> {
    fn inline_body_image(
        &self,
        target: &EntryPath,
        declaration: &ImageDeclaration,
    ) -> Result<gazette_core::markdown::InlineSvg, markdown::Error> {
        let fail = |reason| markdown::Error::LinkResolution {
            span: Span { start: 0, end: 0 },
            reason,
        };
        if declaration.light.is_some()
            || declaration.dark.is_some()
            || !target
                .as_str()
                .rsplit_once('.')
                .is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("svg"))
        {
            return Err(fail(
                "inline images require one SVG without light/dark alternatives",
            ));
        }
        let Some(File::Asset(bytes)) = self.corpus.files().get(target) else {
            return Err(fail("inline SVG requires retained source bytes"));
        };
        let svg = inline_svg::parse(bytes, self.projection_limits.text_bytes).map_err(fail)?;
        match declaration.width {
            Some(width) => svg.with_width(width).map_err(fail),
            None => Ok(svg),
        }
    }

    fn body_image_target(
        &self,
        owner: &Key,
        reference: &str,
    ) -> Result<EntryPath, markdown::Error> {
        let span = Span { start: 0, end: 0 };
        let fail = |reason| markdown::Error::LinkResolution { span, reason };
        if reference.is_empty()
            || reference.contains(['?', '#'])
            || source_path::is_external(reference)
        {
            return Err(fail(
                "image declarations require a local file reference without a query or fragment",
            ));
        }
        let source = self
            .source_binding(owner)
            .map_err(|_| fail("image declaration has no configured source"))?;
        let origin = self
            .source_path(owner)
            .ok_or_else(|| fail("image declaration has no owning document"))?;
        let target = super::links::source_target(
            source,
            origin,
            source_path::decode(reference, span)?,
            span,
        )?;
        if source.is_retained_only(&target) {
            return Err(fail(
                "image declaration targets an explicitly retained-only input",
            ));
        }
        let extension = target
            .as_str()
            .rsplit_once('.')
            .map(|(_, extension)| extension.to_ascii_lowercase());
        if !matches!(
            extension.as_deref(),
            Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "avif" | "svg")
        ) {
            return Err(fail(
                "image declaration must reference a supported raster image or SVG",
            ));
        }
        if !matches!(self.corpus.files().get(&target), Some(File::Asset(_)))
            && !self.asset_metadata.contains_key(&target)
        {
            return Err(fail("declared image is absent from the retained source"));
        }
        if extension.as_deref() == Some("svg")
            && !self.resources(owner).iter().any(|resource| {
                matches!(
                    resource.build,
                    gazette_core::resource::Recipe::SvgImage { .. }
                ) && source
                    .logical_path(resource.build.entry())
                    .is_ok_and(|path| path == target)
            })
        {
            return Err(fail(
                "declared SVG requires a page-package companion or an explicit SVG image resource",
            ));
        }
        Ok(target)
    }

    pub(super) fn validate_body_images(&self) -> Result<(), Vec<FileFailure>> {
        let mut failures = Vec::new();
        for (owner, declarations) in &self.image_declarations {
            let mut defaults = BTreeSet::new();
            for (index, declaration) in declarations.iter().enumerate() {
                let checked = (|| {
                    let default = self.body_image_target(owner, &declaration.default)?;
                    if declaration.sizes.as_ref().is_some_and(|sizes| {
                        !gazette_core::markdown::ImageSources::valid_sizes(sizes)
                            || declaration.inline
                    }) {
                        return Err(markdown::Error::LinkResolution {
                            span: Span { start: 0, end: 0 },
                            reason: "responsive sizes require bounded literal layout syntax on a non-inline image",
                        });
                    }
                    if declaration.inline {
                        self.inline_body_image(&default, declaration)?;
                    }
                    if !defaults.insert(default) {
                        return Err(markdown::Error::LinkResolution {
                            span: Span { start: 0, end: 0 },
                            reason: "duplicate image declarations resolve to the same default source file",
                        });
                    }
                    for reference in declaration.references().skip(1) {
                        self.body_image_target(owner, reference)?;
                    }
                    Ok(())
                })();
                if let Err(error) = checked {
                    let reason = match error {
                        markdown::Error::LinkResolution { reason, .. } => reason.to_owned(),
                        other => other.to_string(),
                    };
                    let path = self.source_path(owner).expect("authored image owner");
                    let file = &self.corpus.files()[path];
                    failures.push(FileFailure {
                        origin: Origin {
                            path: path.clone(),
                            source_sha256: file.source_sha256(),
                            span: super::input::Input::from_file(file).map(|input| {
                                input
                                    .field(&[
                                        crate::toml::PathSegment::Key("images".into()),
                                        crate::toml::PathSegment::Index(index),
                                    ])
                                    .map_or_else(
                                        || input.declaration_span(),
                                        |location| location.span,
                                    )
                            }),
                        },
                        error: FileError::Resource(gazette_core::resource::Error::Compile(
                            format!("images[{index}]: {reason}"),
                        )),
                    });
                }
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures)
        }
    }

    pub(super) fn is_body_image_input(&self, path: &EntryPath) -> bool {
        self.image_declarations.iter().any(|(owner, declarations)| {
            declarations
                .iter()
                .flat_map(ImageDeclaration::references)
                .any(|reference| {
                    self.body_image_target(owner, reference)
                        .is_ok_and(|target| &target == path)
                })
        })
    }

    pub(super) fn for_each_selected_body_image(
        &self,
        content: &BTreeMap<Key, Disposition>,
        mut require: impl FnMut(&EntryPath) -> Result<(), SelectionError>,
    ) -> Result<(), SelectionError> {
        for (owner, declarations) in &self.image_declarations {
            if content.get(owner) != Some(&Disposition::Include) {
                continue;
            }
            for reference in declarations.iter().flat_map(ImageDeclaration::references) {
                // Resolution checked these declarations before selection began.
                let path = self.body_image_target(owner, reference).map_err(|_| {
                    SelectionError::Asset {
                        path: self
                            .source_path(owner)
                            .expect("authored image owner")
                            .clone(),
                        reason: "declared image no longer resolves to its retained source",
                    }
                })?;
                require(&path)?;
            }
        }
        Ok(())
    }

    pub(super) fn project_body_images<'a>(
        &self,
        owner: &Key,
        assets: &'a AssetPlan<'a>,
        dependencies: &mut BTreeMap<&'a str, &'a StaticAsset<'a>>,
        dimensions: &BTreeMap<String, gazette_core::resource::ImageLayout>,
    ) -> Result<BTreeMap<EntryPath, markdown::ImagePresentation>, markdown::Error> {
        let mut presentations = BTreeMap::new();
        let mut remaining = self.projection_limits.text_bytes;
        for declaration in self.image_declarations.get(owner).into_iter().flatten() {
            let mut resolve = |reference: &str| {
                let resolved = self
                    .resolve_reference(
                        owner,
                        assets,
                        markdown::LinkForm::Markdown,
                        markdown::LinkKind::Image,
                        reference,
                        Span { start: 0, end: 0 },
                    )?
                    .ok_or(markdown::Error::InvalidOutput)?;
                if let Some(asset) = resolved.asset {
                    dependencies.insert(asset.url.as_str(), asset);
                }
                Ok::<_, markdown::Error>(resolved.destination)
            };
            let destination = resolve(&declaration.default)?;
            let light = declaration.light.as_deref().map(&mut resolve).transpose()?;
            let dark = declaration.dark.as_deref().map(&mut resolve).transpose()?;
            let responsive = self.responsive_body_image(
                owner,
                declaration,
                &destination,
                light.as_deref(),
                dark.as_deref(),
                dimensions,
            )?;
            for url in std::iter::once(&destination)
                .chain(light.iter())
                .chain(dark.iter())
            {
                remaining = remaining
                    .checked_sub(url.len())
                    .ok_or(markdown::Error::Limit(
                        "resolved body image destination bytes",
                    ))?;
            }
            presentations.insert(
                self.body_image_target(owner, &declaration.default)?,
                markdown::ImagePresentation {
                    responsive,
                    svg: None,
                    loading: declaration.loading,
                    fetch_priority: declaration.fetch_priority,
                    intrinsic: None,
                    inline_svg: declaration
                        .inline
                        .then(|| {
                            self.inline_body_image(
                                &self.body_image_target(owner, &declaration.default)?,
                                declaration,
                            )
                        })
                        .transpose()?,
                    variants: (light.is_some() || dark.is_some())
                        .then_some(markdown::ImageVariants { light, dark }),
                    width: declaration.width,
                },
            );
        }
        Ok(presentations)
    }
}

/// Match the resolved source file, retaining the occurrence query or fragment on each rendition.
pub(super) fn presentation_for(
    presentations: &BTreeMap<EntryPath, markdown::ImagePresentation>,
    reference: Option<&super::links::ResolvedReference<'_>>,
    dimensions: &BTreeMap<String, gazette_core::resource::ImageLayout>,
    raster: &gazette_core::raster::Inspect<'_>,
) -> Option<markdown::ImagePresentation> {
    use gazette_core::resource::ImageLayout;
    let reference = reference?;
    let destination = &reference.destination;
    let end = destination.find(['?', '#']).unwrap_or(destination.len());
    let layout = dimensions.get(&destination[..end]).copied().or_else(|| {
        reference
            .asset
            .and_then(|asset| retained_layout(asset, raster))
            .map(ImageLayout::Raster)
    });
    let mut presentation = presentations
        .get(reference.asset_path.as_ref()?)
        .cloned()
        .or_else(|| {
            layout.map(|_| markdown::ImagePresentation {
                responsive: None,
                svg: None,
                loading: None,
                fetch_priority: None,
                intrinsic: None,
                inline_svg: None,
                variants: None,
                width: None,
            })
        })?;
    let verified_variants = |default: ImageLayout| {
        !destination.contains('#')
            && presentation.variants.as_ref().is_none_or(|variants| {
                [&variants.light, &variants.dark]
                    .into_iter()
                    .flatten()
                    .all(|url| {
                        let other = if url == &destination[..end] {
                            Some(default)
                        } else {
                            dimensions.get(url).copied()
                        };
                        match (default, other) {
                            (ImageLayout::Raster(a), Some(ImageLayout::Raster(b))) => {
                                u64::from(a.width) * u64::from(b.height)
                                    == u64::from(b.width) * u64::from(a.height)
                            }
                            (ImageLayout::Svg(a), Some(ImageLayout::Svg(b))) => a.same_ratio(b),
                            _ => false,
                        }
                    })
            })
    };
    match layout.filter(|layout| verified_variants(*layout)) {
        Some(ImageLayout::Raster(size)) => presentation.intrinsic = Some(size),
        Some(ImageLayout::Svg(svg)) => presentation.svg = Some(svg),
        None => (),
    }
    if let Some(variants) = &mut presentation.variants {
        for value in [&mut variants.light, &mut variants.dark]
            .into_iter()
            .flatten()
        {
            value.push_str(&destination[end..]);
        }
    }
    if let Some(responsive) = &mut presentation.responsive {
        for candidate in responsive
            .default
            .iter_mut()
            .chain(responsive.light.iter_mut().flatten())
            .chain(responsive.dark.iter_mut().flatten())
        {
            candidate.url.push_str(&destination[end..]);
        }
    }
    Some(presentation)
}

fn retained_layout(
    asset: &StaticAsset<'_>,
    raster: &gazette_core::raster::Inspect<'_>,
) -> Option<gazette_core::raster::Dimensions> {
    use gazette_core::raster::{Format, Limits};
    let bytes = asset.content();
    let format = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Format::Png
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Format::Jpeg
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Format::Webp
    } else {
        return None;
    };
    let limits = Limits::default();
    let inspected = raster(format, bytes, limits).ok()?;
    inspected.validate(format, bytes, limits).ok()?;
    inspected.display
}
