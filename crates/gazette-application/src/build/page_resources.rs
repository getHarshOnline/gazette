//! Compile ordered page-local programs from their existing retained content binding.

use gazette_compiler::authored::ResolvedAuthored;
use gazette_core::{
    publication::Key,
    render::LimitedWriter,
    resource,
    source::{EntryPath, Sha256Digest, TreeIdentity},
};
use gazette_ports::resource::ResourceCompiler;
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use super::{
    AcquiredInputs, PublicationRenderError, ResourceProjection, programs::compile_program,
};

/// Private retained compilation evidence, in the declaring page's resource order.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct PageResourceEvidence {
    /// Existing configured content source; never inferred from a path or project name.
    pub source: Key,
    /// Complete declared input identity, including unused prerequisites.
    pub input: TreeIdentity,
    /// Exact compiler-independent entry/profile.
    pub recipe: resource::Recipe,
    /// Actual consumed inputs; only these discharge opaque source obligations.
    pub dependencies: BTreeMap<EntryPath, Sha256Digest>,
    /// Qualified compiler profile, not runtime or type-checking evidence.
    pub tool: String,
    /// Exact destination within the generated tree.
    pub output: EntryPath,
    /// Digest of the actual compiled bytes.
    pub output_sha256: Sha256Digest,
}

#[derive(Default)]
pub(super) struct CompiledPageResources {
    pub model: BTreeMap<Key, Vec<ResourceProjection>>,
    pub outputs: BTreeMap<EntryPath, Arc<[u8]>>,
    pub evidence: BTreeMap<Key, Vec<PageResourceEvidence>>,
    pub consumed: BTreeSet<EntryPath>,
}

impl CompiledPageResources {
    pub(super) fn dimensions(&self, id: &Key) -> BTreeMap<String, resource::ImageLayout> {
        dimensions(self.model.get(id).map_or(&[], Vec::as_slice))
    }
}

pub(super) fn dimensions(
    resources: &[ResourceProjection],
) -> BTreeMap<String, resource::ImageLayout> {
    resources
        .iter()
        .filter_map(|resource| {
            resource
                .image
                .map(|image| resource::ImageLayout::Raster(image.output))
                .or_else(|| resource.svg.map(resource::ImageLayout::Svg))
                .map(|layout| (resource.url.clone(), layout))
        })
        .collect()
}

pub(super) fn compile(
    inputs: &AcquiredInputs,
    authored: &ResolvedAuthored<'_, '_>,
    compiler: &impl ResourceCompiler,
    modules: &resource::ModuleLinks,
    limits: resource::Limits,
    metadata: &mut LimitedWriter<std::io::Sink>,
    output_limit: u64,
) -> Result<CompiledPageResources, PublicationRenderError> {
    let mut compiled = CompiledPageResources::default();
    let mut output_bytes = 0u64;
    for node in authored
        .content()
        .graph()
        .nodes()
        .filter(|node| &node.site == inputs.site())
    {
        let declarations = authored.resources(&node.id);
        if declarations.is_empty() {
            continue;
        }
        let source = authored
            .source_binding(&node.id)
            .map_err(PublicationRenderError::Source)?;
        let capture = inputs
            .sources()
            .get(source.source_key())
            .ok_or_else(|| PublicationRenderError::Binding(source.source_key().clone()))?;
        let base = authored
            .content()
            .base_url(inputs.site())
            .ok_or_else(|| PublicationRenderError::Binding(inputs.site().clone()))?;
        for declaration in declarations {
            let program = compile_program(
                compiler,
                capture,
                &declaration.inputs,
                &declaration.build,
                modules,
                limits,
                inputs
                    .diagram_capture()
                    .map(|capture| (capture, inputs.compilation().source_sha256)),
            )
            .map_err(PublicationRenderError::Resource)?;
            let route = &declaration.output;
            let mut projection = program
                .project(base, route, declaration.build.kind())
                .map_err(PublicationRenderError::Resource)?;
            if declaration.rendition || declaration.responsive {
                let logical = source
                    .logical_path(declaration.build.entry())
                    .map_err(|error| {
                        PublicationRenderError::Resource(resource::Error::Source(error))
                    })?;
                let original = Some(
                    authored
                        .assets_for(inputs.site())
                        .map_err(|error| {
                            PublicationRenderError::Resource(resource::Error::Compile(
                                error.to_string(),
                            ))
                        })?
                        .get(&logical)
                        .ok_or_else(|| {
                            PublicationRenderError::Resource(resource::Error::Compile(
                                "bound rendition original has no public asset address".into(),
                            ))
                        })?
                        .url
                        .clone(),
                );
                if declaration.rendition {
                    projection.rendition_of = original;
                } else {
                    projection.responsive_of = original;
                }
            }
            let sha256 = projection.sha256;
            let bytes = projection.bytes;
            let facts = (
                &node.id,
                source.source_key(),
                program.input,
                &declaration.build,
                &program.dependencies,
                program.tool,
                sha256,
                base.as_str(),
                route.as_str(),
                route.output(),
                &projection.references,
                &projection.deferred_references,
                &projection.image,
                &projection.svg,
                &projection.rendition_of,
                &projection.responsive_of,
            );
            let encoded = serde_json::to_writer(&mut *metadata, &facts);
            if metadata.exceeded() {
                return Err(PublicationRenderError::Resource(resource::Error::Limit(
                    "resource metadata bytes",
                )));
            }
            encoded.map_err(|error| {
                PublicationRenderError::Resource(resource::Error::Compile(error.to_string()))
            })?;
            if bytes > output_limit.saturating_sub(output_bytes) {
                return Err(PublicationRenderError::Resource(resource::Error::Limit(
                    "complete resource output bytes",
                )));
            }
            output_bytes += bytes;
            if compiled
                .outputs
                .insert(route.output().clone(), program.bytes)
                .is_some()
            {
                return Err(PublicationRenderError::Resource(resource::Error::Input {
                    path: route.output().clone(),
                    reason: "page resources claim the same output",
                }));
            }
            for dependency in program.dependencies.keys() {
                let logical = source.logical_path(dependency).map_err(|error| {
                    PublicationRenderError::Resource(resource::Error::Source(error))
                })?;
                compiled.consumed.insert(logical);
            }
            compiled
                .model
                .entry(node.id.clone())
                .or_default()
                .push(projection);
            compiled
                .evidence
                .entry(node.id.clone())
                .or_default()
                .push(PageResourceEvidence {
                    source: source.source_key().clone(),
                    input: program.input,
                    recipe: declaration.build.clone(),
                    dependencies: program.dependencies,
                    tool: program.tool.into(),
                    output: route.output().clone(),
                    output_sha256: sha256,
                });
        }
    }
    Ok(compiled)
}
