//! Concrete resource compiler adapters are composed once at the SDK boundary.

use gazette_core::{
    resource::{Artifact, Error, Limits, Recipe},
    source::TreeInput,
};
use gazette_ports::resource::ResourceCompiler;

pub(crate) struct Compiler;

const PROFILE: &str = "gazette-resource-composition/1;javascript=swc-80;styles=grass-0.13.4+lightningcss-1.0.0-alpha.70;raster=image-0.25.10;svg=quick-xml-0.41.0;woff2=wuff-0.2.8+read-fonts-0.39.2";

impl ResourceCompiler for Compiler {
    fn profile(&self) -> &'static str {
        PROFILE
    }

    fn compile(
        &self,
        source: &TreeInput<'_>,
        recipe: &Recipe,
        links: &gazette_core::resource::ModuleLinks,
        limits: Limits,
    ) -> Result<Artifact, Error> {
        match recipe {
            Recipe::DotImage { entry } => Err(Error::Input {
                path: entry.clone(),
                reason: "DOT resource requires explicit native acquisition",
            }),
            Recipe::BrowserModule { .. } => {
                gazette_adapter_javascript::Compiler.compile(source, recipe, links, limits)
            }
            Recipe::Stylesheet { .. } => {
                gazette_adapter_styles::Compiler.compile(source, recipe, links, limits)
            }
            Recipe::RasterImage { .. } => {
                gazette_adapter_raster::Compiler.compile(source, recipe, links, limits)
            }
            Recipe::SvgImage { .. } => {
                gazette_adapter_vector::Compiler.compile(source, recipe, links, limits)
            }
            Recipe::Woff2Font { .. } => {
                gazette_adapter_fonts::Compiler.compile(source, recipe, links, limits)
            }
        }
    }
}
