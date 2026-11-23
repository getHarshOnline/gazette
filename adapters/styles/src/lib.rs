//! Compile captured Sass inputs into dependency-closed CSS without native effects.
//!
//! Input and successful output sizes are bounded. Grass and Lightning CSS allocate
//! their own intermediate strings and syntax trees; this is not an allocation,
//! execution-time or browser-security sandbox.
//! Each consumed source is also parsed once by Grass's own parser to admit
//! import paths before its lossy extension probing. The public compiler cannot
//! consume that parsed AST, so validation adds a bounded-input parse, not a
//! second Sass grammar or transform owner.

mod captured;
mod guard;
mod imports;
mod validation;

use gazette_core::resource::{Artifact, Error, Limits, Recipe};
use gazette_core::source::TreeInput;
use gazette_ports::resource::ResourceCompiler;

/// Compile SCSS, indented Sass or CSS using only the supplied retained tree.
/// Declaration-only imported partials are unsupported by the selected Grass
/// parser; nested imports of complete rules remain supported.
#[derive(Clone, Copy, Debug, Default)]
pub struct Compiler;

impl ResourceCompiler for Compiler {
    fn compile(
        &self,
        source: &TreeInput<'_>,
        recipe: &Recipe,
        links: &gazette_core::resource::ModuleLinks,
        limits: Limits,
    ) -> Result<Artifact, Error> {
        guard::run(|| compile(source, recipe, links, limits))
    }
}

fn compile(
    source: &TreeInput<'_>,
    recipe: &Recipe,
    links: &gazette_core::resource::ModuleLinks,
    limits: Limits,
) -> Result<Artifact, Error> {
    if !matches!(recipe, Recipe::Stylesheet { .. }) {
        return Err(Error::Input {
            path: recipe.entry().clone(),
            reason: "stylesheet compiler requires a stylesheet recipe",
        });
    }
    if !links.is_empty() {
        return Err(Error::Input {
            path: recipe.entry().clone(),
            reason: "stylesheet recipes do not accept module links",
        });
    }
    limits.validate_input(source.inventory())?;
    let entry = recipe.entry();
    if !captured::supported(entry) || source.file(entry).is_none() {
        return Err(Error::Input {
            path: entry.clone(),
            reason: "stylesheet entry must be a retained .scss, .sass or .css file",
        });
    }
    let files = captured::Files::new(source);
    let options = grass::Options::default()
        .fs(&files)
        .quiet(true)
        .allows_charset(false)
        .style(grass::OutputStyle::Compressed);
    let compiled = grass::from_path(entry.as_str(), &options);
    // Refused transitive input must win over any compiler result, including
    // a success after its inert rejection input. Never emit partial CSS.
    if let Some(error) = files.failure() {
        return Err(Error::Compile(error));
    }
    let css = compiled.map_err(|error| Error::Compile(error.to_string()))?;
    // Grass returns a complete String. This is an artifact acceptance ceiling,
    // not a bound on the compiler's intermediate allocations.
    if css.len() > limits.output_bytes {
        return Err(Error::Limit("stylesheet output bytes"));
    }
    let references = validation::css(&css, limits.output_bytes)?;
    Ok(Artifact {
        input: source.inventory().identity(),
        bytes: css.into_bytes(),
        dependencies: files.dependencies(),
        references,
        deferred_references: Default::default(),
        svg: None,
        image: None,
        tool: "grass@0.13.4+parser-repairs.2+lightningcss@1.0.0-alpha.70/captured-css-v6-minified",
    })
}
