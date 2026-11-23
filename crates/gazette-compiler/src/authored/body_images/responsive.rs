//! Responsive candidates share the existing original-image binding and native geometry.

use std::{collections::BTreeMap, num::NonZeroU32};

use gazette_core::{
    markdown::ImageCandidate, publication::Key, resource::ImageLayout, source::Span,
};

use super::{ImageDeclaration, ResolvedAuthored, markdown};

impl ResolvedAuthored<'_, '_> {
    pub(super) fn responsive_body_image(
        &self,
        owner: &Key,
        declaration: &ImageDeclaration,
        destination: &str,
        light: Option<&str>,
        dark: Option<&str>,
        dimensions: &BTreeMap<String, ImageLayout>,
    ) -> Result<Option<markdown::ResponsiveImage>, markdown::Error> {
        let fail = |reason| markdown::Error::LinkResolution {
            span: Span { start: 0, end: 0 },
            reason,
        };
        let source = self
            .source_binding(owner)
            .map_err(|_| fail("responsive image has no configured source"))?;
        let base = self
            .content
            .base_url(source.site)
            .ok_or_else(|| fail("responsive image has no serving base URL"))?;
        let resources = self.resources(owner);
        let fallback = |url: &str| {
            resources
                .iter()
                .find(|resource| resource.rendition && base.resolve(&resource.output) == url)
        };
        let has_candidates = std::iter::once(destination)
            .chain(light)
            .chain(dark)
            .any(|url| {
                fallback(url).is_some_and(|image| {
                    resources.iter().any(|resource| {
                        resource.responsive && resource.build.entry() == image.build.entry()
                    })
                })
            });
        let Some(sizes) = &declaration.sizes else {
            return if has_candidates {
                Err(fail(
                    "responsive image candidates require an explicit images.sizes declaration",
                ))
            } else {
                Ok(None)
            };
        };
        if !has_candidates {
            return Err(fail(
                "image sizes require explicitly bound responsive resource candidates",
            ));
        }
        let candidates = |url: &str| {
            let image = fallback(url)
                .ok_or_else(|| fail("responsive image modes require a bound fallback rendition"))?;
            let mut result = Vec::new();
            for resource in resources.iter().filter(|resource| {
                (resource.rendition || resource.responsive)
                    && resource.build.entry() == image.build.entry()
            }) {
                let url = base.resolve(&resource.output);
                if url.contains([',', ' ', '\t', '\n', '\r']) {
                    return Err(fail(
                        "responsive candidate URL cannot contain a source-set separator",
                    ));
                }
                let Some(ImageLayout::Raster(size)) = dimensions.get(&url) else {
                    return Err(fail(
                        "responsive candidates require independently verified compiled raster dimensions",
                    ));
                };
                let width = NonZeroU32::new(size.width)
                    .filter(|_| size.height > 0)
                    .ok_or_else(|| fail("responsive candidate has invalid raster dimensions"))?;
                result.push(ImageCandidate { url, width });
            }
            result.sort_unstable_by_key(|candidate| candidate.width);
            if result.windows(2).any(|pair| pair[0].width == pair[1].width) {
                return Err(fail(
                    "responsive candidates must have distinct verified pixel widths",
                ));
            }
            Ok(result)
        };
        Ok(Some(markdown::ResponsiveImage {
            sizes: sizes.clone(),
            default: candidates(destination)?,
            light: light.map(candidates).transpose()?,
            dark: dark.map(candidates).transpose()?,
        }))
    }
}
