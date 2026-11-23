//! Assemble one resolved Theme-package graph into a captured virtual template namespace.

use std::collections::BTreeMap;
use std::fmt;

use gazette_compiler::{
    theme_graph::Graph,
    theme_package::{Namespace, ThemePackageId},
};
use gazette_core::source::{
    self, CapturedSource, Content, EntryPath, Inventory, InventoryBuilder, Subdirectory,
    TreeIdentity, TreeInput,
};

use crate::source::AcquiredInput;

/// Minimal retained-tree view needed to assemble a Theme namespace.
///
/// Revision, repository and development provenance remain on the owning capture;
/// this view neither erases nor fabricates that evidence.
pub trait RetainedTree {
    /// Exact complete structural inventory.
    fn inventory(&self) -> &Inventory;
    /// Retained regular-file bytes without reopening native state.
    fn file(&self, path: &EntryPath) -> Option<&[u8]>;
}

impl RetainedTree for CapturedSource {
    fn inventory(&self) -> &Inventory {
        self.inventory()
    }

    fn file(&self, path: &EntryPath) -> Option<&[u8]> {
        self.file(path)
    }
}

impl RetainedTree for AcquiredInput {
    fn inventory(&self) -> &Inventory {
        self.inventory()
    }

    fn file(&self, path: &EntryPath) -> Option<&[u8]> {
        self.file(path)
    }
}

impl<T: RetainedTree + ?Sized> RetainedTree for &T {
    fn inventory(&self) -> &Inventory {
        (*self).inventory()
    }

    fn file(&self, path: &EntryPath) -> Option<&[u8]> {
        (*self).file(path)
    }
}

/// One package's exact contribution to the virtual template namespace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageEvidence {
    package: ThemePackageId,
    namespace: Namespace,
    templates: Subdirectory,
    source_tree: TreeIdentity,
    files: Vec<EntryPath>,
}

impl PackageEvidence {
    /// Exact package identity from the resolved graph.
    pub fn package(&self) -> &ThemePackageId {
        &self.package
    }

    /// Stable logical mount selected by the package manifest.
    pub fn namespace(&self) -> &Namespace {
        &self.namespace
    }

    /// Exact template root within the captured package source.
    pub fn templates(&self) -> &Subdirectory {
        &self.templates
    }

    /// Complete captured package-tree identity, not merely the mounted subtree.
    pub fn source_tree(&self) -> TreeIdentity {
        self.source_tree
    }

    /// Mounted regular files in exact virtual-path order.
    pub fn files(&self) -> &[EntryPath] {
        &self.files
    }
}

/// Owned namespace structure borrowing only immutable, already captured file bytes.
#[derive(Debug)]
pub struct ThemeNamespace<'a> {
    inventory: Inventory,
    files: BTreeMap<EntryPath, &'a [u8]>,
    packages: Vec<PackageEvidence>,
}

impl<'a> ThemeNamespace<'a> {
    /// Assemble the selected graph without filesystem access or template execution.
    ///
    /// Captures outside the graph are ignored. Every selected package contributes
    /// all regular files beneath its exact template root at
    /// `<namespace>/<relative-path>`; directories exist only as structural parents.
    ///
    /// # Errors
    ///
    /// Rejects a missing capture, missing/non-directory/empty template root,
    /// namespace or mounted-path collision, missing retained payload, or source limits.
    pub fn assemble<C: RetainedTree>(
        graph: &Graph<'_>,
        captures: &'a BTreeMap<ThemePackageId, C>,
        limits: source::Limits,
    ) -> Result<Self, Error> {
        let mut builder = InventoryBuilder::new(limits);
        let mut files = BTreeMap::new();
        let mut owners = BTreeMap::new();
        let mut packages = Vec::with_capacity(graph.packages().len());

        for manifest in graph.packages() {
            let package = &manifest.package;
            let capture = captures.get(package).ok_or_else(|| Error::MissingCapture {
                package: package.clone(),
            })?;
            let root = manifest.templates.to_string();
            let root_entry = capture
                .inventory()
                .entries()
                .iter()
                .find(|entry| entry.path.as_str() == root)
                .ok_or_else(|| Error::TemplateRoot {
                    package: package.clone(),
                    root: manifest.templates.clone(),
                    reason: "template root is missing",
                })?;
            if root_entry.content != Content::Directory {
                return Err(Error::TemplateRoot {
                    package: package.clone(),
                    root: manifest.templates.clone(),
                    reason: "template root is not a directory",
                });
            }

            let namespace: EntryPath = manifest.namespace.as_str().parse().map_err(Error::Input)?;
            insert(
                &mut builder,
                &mut owners,
                namespace,
                Content::Directory,
                package,
            )?;

            let prefix = format!("{root}/");
            let mut mounted_files = Vec::new();
            for entry in capture
                .inventory()
                .entries()
                .iter()
                .filter(|entry| entry.path.as_str().starts_with(&prefix))
            {
                let relative = &entry.path.as_str()[prefix.len()..];
                let mounted: EntryPath = format!("{}/{relative}", manifest.namespace)
                    .parse()
                    .map_err(Error::Input)?;
                insert(
                    &mut builder,
                    &mut owners,
                    mounted.clone(),
                    entry.content,
                    package,
                )?;
                if matches!(entry.content, Content::File { .. }) {
                    let bytes = capture
                        .file(&entry.path)
                        .ok_or_else(|| Error::MissingPayload {
                            package: package.clone(),
                            path: entry.path.clone(),
                        })?;
                    files.insert(mounted.clone(), bytes);
                    mounted_files.push(mounted);
                }
            }
            if mounted_files.is_empty() {
                return Err(Error::TemplateRoot {
                    package: package.clone(),
                    root: manifest.templates.clone(),
                    reason: "template root contains no regular files",
                });
            }
            packages.push(PackageEvidence {
                package: package.clone(),
                namespace: manifest.namespace.clone(),
                templates: manifest.templates.clone(),
                source_tree: capture.inventory().identity(),
                files: mounted_files,
            });
        }

        Ok(Self {
            inventory: builder.finish(),
            files,
            packages,
        })
    }

    /// Combined virtual namespace identity and exact structural inventory.
    pub fn inventory(&self) -> &Inventory {
        &self.inventory
    }

    /// Package contributions in the graph's dependency-before-dependent order.
    pub fn packages(&self) -> &[PackageEvidence] {
        &self.packages
    }

    /// Rebind the immutable namespace to the existing renderer input contract.
    ///
    /// # Errors
    ///
    /// Fails if the owned inventory and borrowed retained bytes disagree.
    pub fn as_input(&self) -> Result<TreeInput<'_>, source::Error> {
        TreeInput::new(&self.inventory, self.files.clone())
    }
}

fn insert(
    builder: &mut InventoryBuilder,
    owners: &mut BTreeMap<EntryPath, ThemePackageId>,
    path: EntryPath,
    content: Content,
    package: &ThemePackageId,
) -> Result<(), Error> {
    if let Some(first) = owners.get(&path) {
        return Err(Error::Collision {
            path,
            first: first.clone(),
            second: package.clone(),
        });
    }
    builder
        .insert(path.clone(), content)
        .map_err(Error::Input)?;
    owners.insert(path, package.clone());
    Ok(())
}

/// Failure to construct one complete virtual Theme namespace.
#[derive(Debug)]
pub enum Error {
    /// A graph package has no exact captured input.
    MissingCapture {
        /// Package whose capture is unavailable.
        package: ThemePackageId,
    },
    /// The exact declared template root cannot contribute a usable template set.
    TemplateRoot {
        /// Owning package.
        package: ThemePackageId,
        /// Exact package-local root.
        root: Subdirectory,
        /// Stable violated condition.
        reason: &'static str,
    },
    /// Two packages attempt to own one virtual path.
    Collision {
        /// Conflicting virtual path.
        path: EntryPath,
        /// Package encountered first in dependency order.
        first: ThemePackageId,
        /// Package attempting the conflicting insertion.
        second: ThemePackageId,
    },
    /// A retained inventory file unexpectedly lacks its already verified bytes.
    MissingPayload {
        /// Owning package.
        package: ThemePackageId,
        /// Original package-local captured path.
        path: EntryPath,
    },
    /// Virtual path construction, structural validation, or caller limits failed.
    Input(source::Error),
}

impl Error {
    /// Stable failure class for future interface projection.
    pub fn code(&self) -> &'static str {
        match self {
            Self::MissingCapture { .. } => "theme_namespace_capture",
            Self::TemplateRoot { .. } => "theme_namespace_root",
            Self::Collision { .. } => "theme_namespace_collision",
            Self::MissingPayload { .. } => "theme_namespace_payload",
            Self::Input(error) => error.code(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingCapture { package } => {
                write!(f, "Theme package {package} has no captured input")
            }
            Self::TemplateRoot {
                package,
                root,
                reason,
            } => write!(f, "Theme package {package} template root {root}: {reason}"),
            Self::Collision {
                path,
                first,
                second,
            } => write!(
                f,
                "Theme namespace path {path} is claimed by packages {first} and {second}"
            ),
            Self::MissingPayload { package, path } => write!(
                f,
                "Theme package {package} retained template {path} without its captured payload"
            ),
            Self::Input(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Input(error) => Some(error),
            _ => None,
        }
    }
}
