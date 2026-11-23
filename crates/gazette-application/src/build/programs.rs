//! One retained-input compilation and evidence check for site and page resources.

use gazette_core::{
    resource,
    source::{
        Content, EntryPath, Inventory, InventoryBuilder, Sha256Digest, TreeIdentity, TreeInput,
    },
};
use gazette_ports::resource::ResourceCompiler;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

pub(super) struct Program {
    pub bytes: Arc<[u8]>,
    pub input: TreeIdentity,
    pub dependencies: BTreeMap<EntryPath, Sha256Digest>,
    pub references: BTreeSet<String>,
    pub deferred_references: BTreeSet<String>,
    pub image: Option<gazette_core::raster::Geometry>,
    pub svg: Option<resource::SvgLayout>,
    pub tool: &'static str,
}

impl Program {
    pub(super) fn project(
        &self,
        base: &gazette_core::publication::routing::PublicBaseUrl,
        route: &gazette_core::publication::routing::RoutePath,
        kind: resource::Kind,
    ) -> Result<super::ResourceProjection, resource::Error> {
        let references = self.references.iter().map(|reference| {
            let target = base.resource_target(route, reference).map_err(|_| resource::Error::Input {
                path: route.output().clone(),
                reason: "compiled resource reference is not a confined same-site output URL",
            })?;
            Ok((reference.clone(), target.output().clone()))
        }).collect::<Result<_, resource::Error>>()?;
        Ok(super::ResourceProjection {
            rendition_of: None,
            responsive_of: None,
            kind,
            url: base.resolve(route),
            output: route.output().clone(),
            sha256: Sha256Digest::of(&self.bytes),
            bytes: self.bytes.len() as u64,
            references,
            deferred_references: self.deferred_references.clone(),
            image: self.image,
            svg: self.svg,
        })
    }
}

pub(super) fn compile_program(
    compiler: &impl ResourceCompiler,
    capture: &crate::source::AcquiredInput,
    inputs: &[EntryPath],
    recipe: &resource::Recipe,
    modules: &resource::ModuleLinks,
    limits: resource::Limits,
    diagrams: Option<(&super::DiagramCapture, Sha256Digest)>,
) -> Result<Program, resource::Error> {
    let files = inputs
        .iter()
        .map(|path| {
            capture
                .file(path)
                .map(|bytes| (path.clone(), bytes))
                .ok_or_else(|| resource::Error::Input {
                    path: path.clone(),
                    reason: "declared resource input is not retained",
                })
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    let inventory = input_inventory(&files, limits)?;
    let tree = TreeInput::new(&inventory, files).map_err(resource::Error::Source)?;
    if let resource::Recipe::DotImage { entry } = recipe {
        if inputs != [entry.clone()] {
            return Err(resource::Error::Input {
                path: entry.clone(),
                reason: "DOT profile requires exactly its retained entry",
            });
        }
        let (diagrams, workspace) = diagrams.ok_or_else(|| resource::Error::Input {
            path: entry.clone(),
            reason: "DOT resource requires explicit native acquisition",
        })?;
        return diagrams.program(workspace, capture, entry, inventory.identity(), limits);
    }
    compile_tree(compiler, &tree, recipe, modules, limits)
}

/// Assemble an exact virtual package namespace from independently retained inputs.
/// Native origins remain application evidence, not compiler search paths.
pub(super) fn compile_selected_program(
    compiler: &impl ResourceCompiler,
    files: BTreeMap<EntryPath, &[u8]>,
    recipe: &resource::Recipe,
    modules: &resource::ModuleLinks,
    limits: resource::Limits,
) -> Result<Program, resource::Error> {
    let inventory = input_inventory(&files, limits)?;
    let tree = TreeInput::new(&inventory, files).map_err(resource::Error::Source)?;
    compile_tree(compiler, &tree, recipe, modules, limits)
}

/// Project one already acquired native diagram into its virtual resource namespace.
/// Acquisition retains the original source/entry; compilation never invokes a provider.
pub(super) fn compile_selected_diagram(
    files: BTreeMap<EntryPath, &[u8]>,
    capture: &crate::source::AcquiredInput,
    target: &EntryPath,
    origin: &EntryPath,
    limits: resource::Limits,
    diagrams: Option<(&super::DiagramCapture, Sha256Digest)>,
) -> Result<Program, resource::Error> {
    if files.len() != 1 || !files.contains_key(target) {
        return Err(resource::Error::Input {
            path: target.clone(),
            reason: "DOT profile requires exactly its retained entry",
        });
    }
    let inventory = input_inventory(&files, limits)?;
    let tree = TreeInput::new(&inventory, files).map_err(resource::Error::Source)?;
    let (diagrams, workspace) = diagrams.ok_or_else(|| resource::Error::Input {
        path: target.clone(),
        reason: "DOT resource requires explicit native acquisition",
    })?;
    let mut program = diagrams.program(workspace, capture, origin, inventory.identity(), limits)?;
    let bytes = tree.file(target).ok_or_else(|| resource::Error::Input {
        path: target.clone(),
        reason: "DOT resource has no exact virtual retained entry",
    })?;
    if capture.file(origin) != Some(bytes) {
        return Err(resource::Error::Input {
            path: target.clone(),
            reason: "DOT origin differs from its retained virtual input",
        });
    }
    program.dependencies = BTreeMap::from([(target.clone(), Sha256Digest::of(bytes))]);
    Ok(program)
}

fn input_inventory(
    files: &BTreeMap<EntryPath, &[u8]>,
    limits: resource::Limits,
) -> Result<Inventory, resource::Error> {
    let mut builder = InventoryBuilder::new(limits.inputs);
    let mut directories = BTreeSet::new();
    for (path, bytes) in files {
        for (offset, _) in path.as_str().match_indices('/') {
            let parent: EntryPath = path.as_str()[..offset]
                .parse()
                .map_err(resource::Error::Source)?;
            if directories.insert(parent.clone()) {
                builder
                    .insert(parent, Content::Directory)
                    .map_err(resource::Error::Source)?;
            }
        }
        builder
            .insert(
                path.clone(),
                Content::File {
                    bytes: bytes.len() as u64,
                    sha256: Sha256Digest::of(bytes),
                },
            )
            .map_err(resource::Error::Source)?;
    }
    Ok(builder.finish())
}

/// Compile an exact retained or generated tree through the same evidence checks.
pub(super) fn compile_tree(
    compiler: &impl ResourceCompiler,
    tree: &TreeInput<'_>,
    recipe: &resource::Recipe,
    modules: &resource::ModuleLinks,
    limits: resource::Limits,
) -> Result<Program, resource::Error> {
    recipe.validate_imports()?;
    let selected_links =
        if let resource::Recipe::BrowserModule { imports, .. } = recipe {
            imports
                .values()
                .map(|identity| {
                    modules.get(identity).map(|url| (identity.clone(), url.clone()))
                .ok_or_else(|| resource::Error::Input {
                    path: recipe.entry().clone(),
                    reason: "module import binding is not selected or is explicitly disabled",
                })
                })
                .collect::<Result<resource::ModuleLinks, _>>()?
        } else {
            resource::ModuleLinks::new()
        };
    let artifact = compiler.compile(tree, recipe, &selected_links, limits)?;
    if artifact.input != tree.inventory().identity()
        || !artifact.dependencies.contains(recipe.entry())
        || artifact.tool.is_empty()
    {
        return Err(resource::Error::Input {
            path: recipe.entry().clone(),
            reason: "compiler evidence differs from the selected input contract",
        });
    }
    if artifact.bytes.len() > limits.output_bytes {
        return Err(resource::Error::Limit("output bytes"));
    }
    if let resource::Recipe::BrowserModule { imports, .. } = recipe
        && artifact.references.iter().any(|reference| {
            !imports
                .values()
                .any(|identity| modules.get(identity) == Some(reference))
        })
    {
        return Err(resource::Error::Input {
            path: recipe.entry().clone(),
            reason: "compiled module reference differs from its selected import bindings",
        });
    }
    if !artifact.deferred_references.is_subset(&artifact.references)
        || (!artifact.deferred_references.is_empty()
            && !matches!(recipe, resource::Recipe::BrowserModule { .. }))
    {
        return Err(resource::Error::Input {
            path: recipe.entry().clone(),
            reason: "deferred references must be a subset of emitted browser-module references",
        });
    }
    let valid_image = match (recipe, artifact.image) {
        (
            resource::Recipe::RasterImage {
                apply_orientation, ..
            },
            Some(image),
        ) => {
            [image.source, image.oriented, image.output]
                .iter()
                .all(|size| {
                    size.width > 0
                        && size.height > 0
                        && size.width <= limits.raster.width
                        && size.height <= limits.raster.height
                })
                && (image.source == image.oriented
                    || (*apply_orientation
                        && image.source.width == image.oriented.height
                        && image.source.height == image.oriented.width))
        }
        (
            resource::Recipe::BrowserModule { .. }
            | resource::Recipe::Stylesheet { .. }
            | resource::Recipe::SvgImage { .. }
            | resource::Recipe::Woff2Font { .. },
            None,
        ) => true,
        _ => false,
    };
    if !valid_image
        || (artifact.svg.is_some() && !matches!(recipe, resource::Recipe::SvgImage { .. }))
    {
        return Err(resource::Error::Input {
            path: recipe.entry().clone(),
            reason: "compiler geometry differs from the selected resource contract",
        });
    }
    let mut reference_bytes = 0usize;
    for reference in artifact
        .references
        .iter()
        .chain(&artifact.deferred_references)
    {
        reference_bytes = reference_bytes
            .checked_add(reference.len())
            .filter(|bytes| *bytes <= limits.output_bytes)
            .ok_or(resource::Error::Limit("resource reference bytes"))?;
    }
    let dependencies = artifact
        .dependencies
        .into_iter()
        .map(|path| {
            let bytes = tree.file(&path).ok_or_else(|| resource::Error::Input {
                path: path.clone(),
                reason: "compiler dependency lies outside declared inputs",
            })?;
            Ok((path, Sha256Digest::of(bytes)))
        })
        .collect::<Result<BTreeMap<_, _>, resource::Error>>()?;
    Ok(Program {
        bytes: Arc::from(artifact.bytes),
        input: artifact.input,
        dependencies,
        references: artifact.references,
        deferred_references: artifact.deferred_references,
        image: artifact.image,
        svg: artifact.svg,
        tool: artifact.tool,
    })
}
