//! Independently selected site resources and dependency resources share one retained compiler.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use gazette_compiler::{
    authored::ResolvedAuthored,
    workspace::{Key, ResourceRef, ResourceSelection, ResourceUse, SelectionOrigin},
};
use gazette_core::{
    publication::routing::PublicBaseUrl,
    render::LimitedWriter,
    resource::{self, ResourceId},
    source::{EntryPath, Sha256Digest, TreeIdentity},
};
use gazette_ports::resource::ResourceCompiler;
use serde::Serialize;

use super::{
    CompositionMetadata, PublicationRenderError,
    capability_status::charge_metadata,
    programs::{compile_selected_diagram, compile_selected_program},
};

/// Public compiled-resource facts shared by Theme and machine models.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct ResourceProjection {
    /// Original image's resolved serving URL when this output is its bound rendition.
    /// Source capture paths and native filenames never enter this field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rendition_of: Option<String>,
    /// Original serving URL for an explicitly declared responsive image candidate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub responsive_of: Option<String>,
    /// Checked recipe semantics, shared without filename inference.
    pub kind: resource::Kind,
    /// Exact address beneath the configured publication base.
    pub url: String,
    /// Exact decoded path in the generated site tree.
    pub output: EntryPath,
    /// Hash of the actual compiled bytes.
    pub sha256: Sha256Digest,
    /// Actual compiled byte count.
    pub bytes: u64,
    /// Emitted URLs bound to exact same-site output files.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub references: BTreeMap<String, EntryPath>,
    /// Emitted references reached only on demand; omitted from initial preloads.
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub deferred_references: BTreeSet<String>,
    /// Verified SVG viewport lengths and ratio, distinct from raster dimensions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub svg: Option<resource::SvgLayout>,
    /// Observed image geometry; absent for scripts and styles.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<gazette_core::raster::Geometry>,
}

/// One required or explicitly site-selected resource joined to its public projection.
///
/// Concrete package ownership, provider/adapter mechanics, recipes, tools, sources
/// and selection origins remain private build evidence.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct ThemeResourceProjection {
    /// Exact logical requirement or enabled site-use resource selection.
    pub id: ResourceId,
    /// Public facts from the independently selected and compiled resource.
    #[serde(flatten)]
    pub resource: ResourceProjection,
}

/// Browser-facing import resolution and preload facts derived from emitted module bytes.
///
/// This is a projection of the compiled graph, never a second resolver or a request to
/// execute a shared module. Theme may serialize `import_map` inline and render the
/// page-specific `preloads`; the site model receives this same value.
#[derive(Clone, Debug, Default, Serialize, schemars::JsonSchema)]
pub struct BrowserModules {
    /// Literal authored specifiers which were present in emitted JavaScript.
    pub import_map: ImportMap,
    /// Every reachable shared module, including its exact dependency edges.
    pub modules: BTreeMap<ResourceId, BrowserModule>,
    /// Exact shared-module closure required by each rendered page.
    pub pages: BTreeMap<Key, Vec<ResourceId>>,
    /// Eager site-level closure used by generated pages without page-local bindings.
    pub site_preloads: Vec<ResourceId>,
}

impl BrowserModules {
    pub(super) fn is_empty(&self) -> bool {
        self.modules.is_empty()
    }

    pub(super) fn page<'a>(&'a self, page: &Key) -> Option<PageBrowserModules<'a>> {
        (!self.modules.is_empty()).then(|| PageBrowserModules {
            import_map: &self.import_map,
            preloads: self
                .pages
                .get(page)
                .into_iter()
                .flatten()
                .filter_map(|identity| self.modules.get(identity))
                .collect(),
        })
    }

    /// Site-level view for generated Theme surfaces without a page-local closure.
    pub(super) fn site(&self) -> Option<PageBrowserModules<'_>> {
        (!self.modules.is_empty()).then(|| PageBrowserModules {
            import_map: &self.import_map,
            preloads: self
                .site_preloads
                .iter()
                .filter_map(|id| self.modules.get(id))
                .collect(),
        })
    }
}

/// Standard import-map payload. Empty scopes are not invented.
#[derive(Clone, Debug, Default, Serialize, schemars::JsonSchema)]
pub struct ImportMap {
    /// Literal module specifier to exact canonical compiled URL.
    pub imports: BTreeMap<String, String>,
}

/// One exact shared-module record suitable for a `modulepreload` hint.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct BrowserModule {
    /// Existing checked resource facts; URLs and hashes are never rebuilt here.
    #[serde(flatten)]
    pub resource: ResourceProjection,
    /// Actual emitted shared-module dependencies in stable logical-key order.
    pub dependencies: Vec<ResourceId>,
}

/// Page-local view of the site graph. Preloading fetches but never executes a module.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub(super) struct PageBrowserModules<'a> {
    pub import_map: &'a ImportMap,
    pub preloads: Vec<&'a BrowserModule>,
}

/// Private provenance of one compiled site resource, not a script activation request.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct ResourceEvidence {
    /// Exact configured source, captured before compilation.
    pub source: Key,
    /// Exact package-qualified resource; no provider or capability role is invented.
    pub resource: ResourceRef,
    /// Workspace or site selection that supplied this binding.
    pub selection: SelectionOrigin,
    /// Explicit contribution behavior selected independently from its package owner.
    #[serde(rename = "use")]
    pub use_: ResourceUse,
    /// Exact retained package manifest bytes.
    pub manifest_sha256: Sha256Digest,
    /// Normalized workspace source binding used to acquire this package.
    pub source_binding_sha256: Sha256Digest,
    /// Complete retained package tree identity.
    pub tree: TreeIdentity,
    /// Complete declared program input set.
    pub input: TreeIdentity,
    /// All declared virtual inputs and their exact original captured source coordinates.
    /// This is private evidence; public URLs retain no native source location.
    pub input_origins: BTreeMap<EntryPath, super::ResourceInputEvidence>,
    /// Exact entry and logical import bindings.
    pub recipe: resource::Recipe,
    /// Native local dependency closure and retained byte identities.
    pub dependencies: BTreeMap<EntryPath, Sha256Digest>,
    /// Native compiler/profile identity, not browser execution proof.
    pub tool: String,
    /// Explicit generated destination, independent of the source location.
    pub output: EntryPath,
    /// Exact canonical same-site module address used by consuming imports.
    pub url: String,
    /// Actual emitted bytes.
    pub output_sha256: Sha256Digest,
    /// Emitted cross-module links bound to the final output inventory.
    pub references: BTreeMap<String, EntryPath>,
    /// Emitted references reached only on demand; omitted from initial preloads.
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub deferred_references: BTreeSet<String>,
}

#[derive(Default)]
pub(super) struct CompiledResources {
    pub model: BTreeMap<ResourceId, ResourceProjection>,
    pub theme: Vec<ThemeResourceProjection>,
    pub outputs: BTreeMap<EntryPath, Arc<[u8]>>,
    pub evidence: BTreeMap<ResourceId, ResourceEvidence>,
    pub consumed: BTreeSet<EntryPath>,
    pub browser: BrowserModules,
}

pub(super) struct Compilation<'a> {
    pub links: &'a resource::ModuleLinks,
    pub global_roots: BTreeSet<String>,
    pub page_roots: BTreeMap<Key, BTreeSet<String>>,
    pub page_eager_roots: BTreeMap<Key, BTreeSet<String>>,
    pub aliases: BTreeMap<String, ResourceId>,
    pub limits: resource::Limits,
    pub budget: &'a mut LimitedWriter<std::io::Sink>,
    pub output_limit: u64,
}

/// Retain only aliases that survived compilation as actual module references.
/// One site cannot assign the same bare specifier to two different modules.
pub(super) fn collect_aliases(
    aliases: &mut BTreeMap<String, ResourceId>,
    recipe: &resource::Recipe,
    references: &BTreeMap<String, EntryPath>,
    links: &resource::ModuleLinks,
) -> Result<(), PublicationRenderError> {
    let resource::Recipe::BrowserModule { entry, imports } = recipe else {
        return Ok(());
    };
    for (alias, identity) in imports {
        let Some(url) = links.get(identity) else {
            continue;
        };
        if !references.contains_key(url) {
            continue;
        }
        if aliases
            .insert(alias.clone(), identity.clone())
            .is_some_and(|previous| previous != *identity)
        {
            return Err(PublicationRenderError::Resource(resource::Error::Input {
                path: entry.clone(),
                reason: "one emitted module alias resolves to different shared modules",
            }));
        }
    }
    Ok(())
}

pub(super) fn links(
    metadata: &CompositionMetadata<'_>,
    base: Option<&PublicBaseUrl>,
    budget: &mut LimitedWriter<std::io::Sink>,
) -> Result<resource::ModuleLinks, PublicationRenderError> {
    let inputs = metadata.acquired;
    let composition = metadata.composition();
    let mut links = BTreeMap::new();
    let mut outputs = BTreeSet::new();
    for (identity, selection) in composition.resources() {
        if let ResourceSelection::Enabled { output, .. } = selection.value() {
            let base =
                base.ok_or_else(|| PublicationRenderError::Binding(inputs.site().clone()))?;
            if !outputs.insert(output.output().clone()) {
                return Err(PublicationRenderError::Resource(resource::Error::Input {
                    path: output.output().clone(),
                    reason: "distinct selected modules claim the same canonical output",
                }));
            }
            let url = base.resolve(output);
            charge_metadata(budget, &(identity, &url, output.output()))?;
            links.insert(identity.clone(), url);
        }
    }
    Ok(links)
}

pub(super) fn compile(
    metadata: &CompositionMetadata<'_>,
    authored: &ResolvedAuthored<'_, '_>,
    compiler: &impl ResourceCompiler,
    input: Compilation<'_>,
) -> Result<CompiledResources, PublicationRenderError> {
    let Compilation {
        links,
        mut global_roots,
        page_roots,
        page_eager_roots,
        mut aliases,
        limits,
        budget,
        output_limit,
    } = input;
    let inputs = metadata.acquired;
    let composition = metadata.composition();
    let by_url: BTreeMap<_, _> = links
        .iter()
        .map(|(identity, url)| (url.as_str(), identity))
        .collect();
    let mut pending: BTreeSet<ResourceId> = composition
        .resources()
        .iter()
        .filter(|(_, selection)| {
            matches!(
                selection.value(),
                ResourceSelection::Enabled {
                    use_: ResourceUse::Site,
                    ..
                }
            )
        })
        .map(|(identity, _)| identity.clone())
        .chain(
            global_roots
                .iter()
                .chain(page_roots.values().flatten())
                .filter_map(|url| by_url.get(url.as_str()).copied().cloned()),
        )
        .collect();
    let card_requirements = metadata
        .effective_theme()
        .render()
        .social_card
        .as_ref()
        .map(|recipe| recipe.resource_requirements.as_slice())
        .unwrap_or_default();
    pending.extend(card_requirements.iter().cloned());
    let mut compiled = CompiledResources::default();
    let mut output_bytes = 0u64;
    while let Some(identity) = pending.pop_first() {
        // Cycles preserve native ESM linking; already compiled modules never get duplicated.
        if compiled.model.contains_key(&identity) {
            continue;
        }
        let fail = || PublicationRenderError::ResourceBinding(identity.clone());
        let selected = composition.resources().get(&identity).ok_or_else(fail)?;
        let ResourceSelection::Enabled {
            resource,
            output,
            use_,
        } = selected.value()
        else {
            return Err(fail());
        };
        let admitted = metadata.resource(resource).ok_or_else(fail)?;
        let recipe = admitted.build;
        if *use_ == ResourceUse::Dependency
            && !card_requirements.contains(&identity)
            && !matches!(recipe, resource::Recipe::BrowserModule { .. })
        {
            return Err(PublicationRenderError::Resource(resource::Error::Input {
                path: recipe.entry().clone(),
                reason: "dependency resource must compile a browser module",
            }));
        }
        let input_origins = super::resource_inputs::resource_origins(&admitted)
            .map_err(PublicationRenderError::Resource)?;
        let files = super::resource_inputs::retained_files(inputs, &input_origins)
            .map_err(PublicationRenderError::Resource)?;
        let program = if let resource::Recipe::DotImage { entry } = recipe {
            let origin = input_origins.get(entry).ok_or_else(|| {
                PublicationRenderError::Resource(resource::Error::Input {
                    path: entry.clone(),
                    reason: "DOT resource has no admitted origin",
                })
            })?;
            let capture = inputs.sources().get(&origin.source).ok_or_else(|| {
                PublicationRenderError::Resource(resource::Error::Input {
                    path: entry.clone(),
                    reason: "DOT origin source was not captured",
                })
            })?;
            compile_selected_diagram(
                files,
                capture,
                entry,
                &origin.entry,
                limits,
                metadata
                    .acquired
                    .diagram_capture()
                    .map(|capture| (capture, metadata.acquired.compilation().source_sha256)),
            )
        } else {
            compile_selected_program(compiler, files, recipe, links, limits)
        }
        .map_err(PublicationRenderError::Resource)?;
        let base = authored
            .content()
            .base_url(inputs.site())
            .ok_or_else(fail)?;
        let projection = program
            .project(base, output, recipe.kind())
            .map_err(PublicationRenderError::Resource)?;
        if projection.bytes > output_limit.saturating_sub(output_bytes) {
            return Err(PublicationRenderError::Resource(resource::Error::Limit(
                "complete shared module output bytes",
            )));
        }
        output_bytes += projection.bytes;
        // These are actual compiler-emitted links, not speculative edges from every
        // configured alias. Unreachable selected modules are not emitted or autoloaded.
        for reference in &program.references {
            let dependency = by_url.get(reference.as_str()).ok_or_else(fail)?;
            pending.insert((*dependency).clone());
        }
        charge_metadata(
            budget,
            &(
                &identity,
                resource,
                selected.origin(),
                admitted.manifest_sha256,
                admitted.source_binding_sha256,
                admitted.tree,
                program.input,
                &input_origins,
                recipe,
                &program.dependencies,
                program.tool,
                &projection.url,
                &projection.output,
                projection.sha256,
                &projection.references,
                &projection.deferred_references,
            ),
        )?;
        // An explicitly shared content/presentation capture may supply a module.
        // Only actual retained dependencies discharge that content source's obligations.
        for node in authored
            .content()
            .graph()
            .nodes()
            .filter(|node| &node.site == inputs.site())
        {
            let binding = authored
                .source_binding(&node.id)
                .map_err(PublicationRenderError::Source)?;
            if binding.tree_binding().is_some() {
                for dependency in program.dependencies.keys() {
                    let origin = input_origins.get(dependency).ok_or_else(|| {
                        PublicationRenderError::Resource(resource::Error::Input {
                            path: dependency.clone(),
                            reason: "compiler dependency has no admitted origin",
                        })
                    })?;
                    if binding.source_key() != &origin.source {
                        continue;
                    }
                    compiled
                        .consumed
                        .insert(binding.logical_path(&origin.entry).map_err(|error| {
                            PublicationRenderError::Resource(resource::Error::Source(error))
                        })?);
                }
            }
        }
        compiled.evidence.insert(
            identity.clone(),
            ResourceEvidence {
                source: admitted.source.clone(),
                resource: resource.clone(),
                selection: selected.origin().clone(),
                use_: *use_,
                manifest_sha256: admitted.manifest_sha256,
                source_binding_sha256: admitted.source_binding_sha256,
                tree: admitted.tree,
                input: program.input,
                input_origins,
                recipe: recipe.clone(),
                dependencies: program.dependencies,
                tool: program.tool.into(),
                output: projection.output.clone(),
                url: projection.url.clone(),
                output_sha256: projection.sha256,
                references: projection.references.clone(),
                deferred_references: projection.deferred_references.clone(),
            },
        );
        if compiled
            .outputs
            .insert(projection.output.clone(), program.bytes)
            .is_some()
        {
            return Err(PublicationRenderError::Resource(resource::Error::Input {
                path: projection.output.clone(),
                reason: "shared modules claim the same output",
            }));
        }
        compiled.model.insert(identity, projection);
    }
    for evidence in compiled.evidence.values() {
        collect_aliases(&mut aliases, &evidence.recipe, &evidence.references, links)?;
    }
    let required = metadata.effective_theme().resource_requirements();
    let selected = composition
        .resources()
        .iter()
        .filter_map(|(identity, selection)| {
            (matches!(
                selection.value(),
                ResourceSelection::Enabled {
                    use_: ResourceUse::Site,
                    ..
                }
            ) && !required.contains(identity))
            .then_some(identity)
        });
    compiled.theme = required
        .iter()
        .chain(selected)
        .map(|identity| {
            compiled
                .model
                .get(identity)
                .cloned()
                .map(|resource| ThemeResourceProjection {
                    id: identity.clone(),
                    resource,
                })
                .ok_or_else(|| PublicationRenderError::ResourceBinding(identity.clone()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    global_roots.extend(
        compiled
            .theme
            .iter()
            .map(|resource| resource.resource.url.clone()),
    );
    let dependencies: BTreeMap<_, _> = compiled
        .model
        .iter()
        .map(|(identity, module)| {
            let dependencies = module
                .references
                .keys()
                .filter_map(|url| by_url.get(url.as_str()).copied().cloned())
                .collect::<Vec<_>>();
            (identity.clone(), dependencies)
        })
        .collect();
    let eager_dependencies: BTreeMap<_, Vec<_>> = compiled
        .model
        .iter()
        .map(|(id, module)| {
            let edges = module
                .references
                .keys()
                .filter(|url| !module.deferred_references.contains(*url))
                .filter_map(|url| by_url.get(url.as_str()).copied().cloned())
                .collect();
            (id.clone(), edges)
        })
        .collect();
    let preload = |roots: &BTreeSet<String>| {
        let mut pending: BTreeSet<_> = global_roots
            .iter()
            .chain(roots)
            .filter_map(|url| by_url.get(url.as_str()).copied().cloned())
            .collect();
        let mut closure = BTreeSet::new();
        while let Some(identity) = pending.pop_first() {
            if !closure.insert(identity.clone()) {
                continue;
            }
            if let Some(next) = eager_dependencies.get(&identity) {
                pending.extend(next.iter().cloned());
            }
        }
        closure.into_iter().collect::<Vec<_>>()
    };
    let site_preloads = preload(&BTreeSet::new());
    let pages = page_eager_roots
        .into_iter()
        .map(|(page, roots)| (page, preload(&roots)))
        .collect();
    compiled.browser = BrowserModules {
        import_map: ImportMap {
            imports: aliases
                .into_iter()
                .filter_map(|(alias, identity)| {
                    compiled
                        .model
                        .get(&identity)
                        .map(|module| (alias, module.url.clone()))
                })
                .collect(),
        },
        modules: compiled
            .model
            .iter()
            .filter(|(identity, _)| {
                matches!(
                    compiled.evidence.get(identity).map(|value| &value.recipe),
                    Some(resource::Recipe::BrowserModule { .. })
                )
            })
            .map(|(identity, resource)| {
                (
                    identity.clone(),
                    BrowserModule {
                        resource: resource.clone(),
                        dependencies: dependencies.get(identity).cloned().unwrap_or_default(),
                    },
                )
            })
            .collect(),
        pages,
        site_preloads,
    };
    Ok(compiled)
}
