//! Workspace-selected resource mounts preserve native provenance in a virtual package namespace.

use std::collections::{BTreeMap, BTreeSet};

use gazette_compiler::{
    theme_package,
    workspace::{Key, ResourceInputName, ThemePackageBinding},
};
use gazette_core::{
    resource,
    source::{EntryPath, Sha256Digest, TreeIdentity},
};
use serde::Serialize;

use super::{AcquiredInputs, MetadataError};

/// Private origin of one declared resource input. The map key is its virtual package path.
/// Public resource URLs never expose these source coordinates or acquisition handles.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct ResourceInputEvidence {
    /// Explicit workspace input alias; absent for a package-local input.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<ResourceInputName>,
    /// Exact source binding, independently of the resource's package owner.
    pub source: Key,
    /// Original retained path inside that source, not the virtual target path.
    pub entry: EntryPath,
    /// Normalized source selection, including its configured pin and coordinates.
    pub source_binding_sha256: Sha256Digest,
    /// Complete retained source identity.
    pub tree: TreeIdentity,
    /// Exact file bytes selected from that retained source.
    pub sha256: Sha256Digest,
}

pub(super) fn resource_origins(
    admitted: &super::metadata::AdmittedResource<'_>,
) -> Result<BTreeMap<EntryPath, ResourceInputEvidence>, resource::Error> {
    admitted
        .inputs
        .iter()
        .map(|path| {
            let origin = match admitted.input_origins {
                Some(origins) => {
                    origins
                        .get(path)
                        .cloned()
                        .ok_or_else(|| resource::Error::Input {
                            path: path.clone(),
                            reason: "admitted resource origin is absent",
                        })?
                }
                None => {
                    let bytes =
                        admitted
                            .capture
                            .file(path)
                            .ok_or_else(|| resource::Error::Input {
                                path: path.clone(),
                                reason: "declared resource input is not retained",
                            })?;
                    ResourceInputEvidence {
                        input: None,
                        source: admitted.source.clone(),
                        entry: path.clone(),
                        source_binding_sha256: admitted.source_binding_sha256,
                        tree: admitted.tree,
                        sha256: Sha256Digest::of(bytes),
                    }
                }
            };
            Ok((path.clone(), origin))
        })
        .collect()
}

pub(super) fn retained_files<'a>(
    inputs: &'a AcquiredInputs,
    origins: &BTreeMap<EntryPath, ResourceInputEvidence>,
) -> Result<BTreeMap<EntryPath, &'a [u8]>, resource::Error> {
    origins
        .iter()
        .map(|(target, origin)| {
            if inputs
                .compilation()
                .workspace
                .is_retained_only(&origin.source, &origin.entry)
            {
                return Err(resource::Error::Input {
                    path: target.clone(),
                    reason: "resource origin is explicitly retained-only",
                });
            }
            let bytes = inputs
                .sources()
                .get(&origin.source)
                .and_then(|capture| capture.file(&origin.entry))
                .filter(|bytes| Sha256Digest::of(bytes) == origin.sha256)
                .ok_or_else(|| resource::Error::Input {
                    path: target.clone(),
                    reason: "resource origin differs from retained bytes",
                })?;
            Ok((target.clone(), bytes))
        })
        .collect()
}

pub(super) fn theme_inputs(
    inputs: &AcquiredInputs,
    binding: &ThemePackageBinding,
    manifest: &theme_package::Manifest,
) -> Result<BTreeMap<EntryPath, ResourceInputEvidence>, MetadataError> {
    let workspace = &inputs.compilation().workspace;
    let package = inputs
        .sources()
        .get(&binding.source)
        .ok_or_else(|| MetadataError::Binding {
            owner: binding.source.to_string(),
            reason: "selected Theme capture is absent",
        })?;
    let declared: BTreeSet<_> = manifest
        .resources
        .iter()
        .flat_map(|resource| &resource.inputs)
        .collect();
    for target in binding.resource_inputs.keys() {
        if !declared.contains(target) {
            return Err(MetadataError::Input {
                source: binding.source.clone(),
                path: target.clone(),
                reason: "mounted target is not a declared Theme resource input",
            });
        }
    }
    let mut origins = BTreeMap::new();
    for target in declared {
        let (source, entry, alias) = match binding.resource_inputs.get(target) {
            Some(mount) => {
                if package
                    .inventory()
                    .entries()
                    .iter()
                    .any(|entry| &entry.path == target)
                {
                    return Err(MetadataError::Input {
                        source: binding.source.clone(),
                        path: target.clone(),
                        reason: "resource input is both package-local and mounted",
                    });
                }
                let source =
                    workspace
                        .design_input(&mount.input)
                        .ok_or_else(|| MetadataError::Binding {
                            owner: binding.package.to_string(),
                            reason: "resource input alias is unresolved",
                        })?;
                (&source.id, &mount.entry, Some(mount.input.clone()))
            }
            None => (&binding.source, target, None),
        };
        if workspace.is_retained_only(source, entry) {
            return Err(MetadataError::Input {
                source: source.clone(),
                path: entry.clone(),
                reason: "Theme resource input is explicitly retained-only",
            });
        }
        let capture = inputs
            .sources()
            .get(source)
            .ok_or_else(|| MetadataError::Binding {
                owner: source.to_string(),
                reason: "selected resource source was not captured",
            })?;
        let bytes = capture.file(entry).ok_or_else(|| MetadataError::Input {
            source: source.clone(),
            path: entry.clone(),
            reason: "Theme resource input must be a retained regular file",
        })?;
        let source_binding = workspace
            .source(source)
            .ok_or_else(|| MetadataError::Binding {
                owner: source.to_string(),
                reason: "resource source binding is absent",
            })?;
        let source_binding_sha256 = super::metadata::normalized_source_digest(source_binding)
            .map_err(|()| MetadataError::Evidence {
                package: binding.package.to_string(),
            })?;
        origins.insert(
            target.clone(),
            ResourceInputEvidence {
                input: alias,
                source: source.clone(),
                entry: entry.clone(),
                source_binding_sha256,
                tree: capture.inventory().identity(),
                sha256: Sha256Digest::of(bytes),
            },
        );
    }
    Ok(origins)
}
