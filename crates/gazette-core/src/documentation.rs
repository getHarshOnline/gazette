//! Authored-manual and generated-reference inputs for one documentation target.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::source::EntryPath;

mod catalog;
mod contract;
pub use catalog::{CatalogEntry, CatalogEvidence, CatalogLimits, TargetAssembly, ValidatedCatalog};
mod navigation;
pub use navigation::ReferencePage;
pub mod generator;
pub mod projection;
mod selection;
pub use selection::Selection;

/// Explicit local documentation target options, not a hosting account or release selection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Target {
    /// Assemble inputs for the selected documentation platform; no platform call occurs.
    Mintlify {
        /// Target-specific visual theme, unrelated to Gazette's authored-content Theme.
        theme: String,
        /// Explicit target presentation color; never inferred from a brand name.
        primary_color: String,
        /// Optional emphasis color on dark surfaces (Mintlify's `colors.light`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        light_color: Option<String>,
        /// Optional button and hover color (Mintlify's `colors.dark`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dark_color: Option<String>,
        /// Mintlify's telemetry integration; omission keeps the provider's default.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        telemetry: Option<bool>,
    },
}

impl Target {
    /// Map an exact configured page to its reading route using the target's qualified rules.
    /// This changes no source selection and supplies no route from a filename.
    pub fn page_route(&self, route: &str) -> String {
        match self {
            Self::Mintlify { .. } => Self::mintlify_page_route(route),
        }
    }

    /// Preserve distinct pages when Mintlify treats a final `index` as a directory entrance.
    /// The escaped namespace itself is escaped to keep the mapping injective.
    pub fn mintlify_page_route(route: &str) -> String {
        let (parent, name) = route.rsplit_once('/').unwrap_or(("", route));
        let name = if name == "index" || name.starts_with("gazette-page-") {
            format!("gazette-page-{name}")
        } else {
            name.to_owned()
        };
        if parent.is_empty() {
            name
        } else {
            format!("{parent}/{name}")
        }
    }
}

/// Native generator output format, independent of the consuming documentation target.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Format {
    /// Generated Markdown, independent of its language, generator and target.
    Markdown,
    /// `TypeDoc` project JSON; not proof that a generator was invoked or qualified.
    #[serde(rename = "typedoc-json")]
    TypeDocJson,
    /// `OpenAPI` HTTP interface description in JSON; not a language-library reference.
    #[serde(rename = "openapi-json")]
    OpenApiJson,
    /// `OpenAPI` HTTP interface description in YAML; encoding is explicit, not filename-derived.
    #[serde(rename = "openapi-yaml")]
    OpenApiYaml,
    /// Swagger 2 HTTP interface description in JSON; target conversion is explicit.
    SwaggerJson,
    /// Swagger 2 HTTP interface description in YAML; never inferred from a filename.
    SwaggerYaml,
    /// Sphinx serializing HTML JSON directory, including native page contexts and companions.
    SphinxJson,
    /// Native Rustdoc HTML tree; target-specific adaptation requires a qualified profile.
    RustdocHtml,
    /// Native Rustdoc JSON; its schema must be qualified independently.
    RustdocJson,
}

/// Native output projected from one retained tree, not a Gazette content AST.
///
/// The target may inspect companions without native reads. Their presence does not
/// authorize publishing the entire capture; target-specific selection and validation
/// remain necessary. Source revisions and execution observations stay with the caller.
pub struct Artifact<'a> {
    /// Explicit generator format; never inferred from the selected filename.
    pub format: Format,
    /// Complete immutable captured output, using the existing source boundary.
    pub tree: crate::source::TreeInput<'a>,
    /// Explicit file or directory scope within that capture.
    pub selection: &'a Selection,
}

/// One explicitly selected authored manual page.
///
/// Manual Markdown is not a generated [`Format::Markdown`] artifact and is never
/// admitted into Gazette's consumer-publication content graph. The configured
/// route and title are the target navigation authority; the retained selection
/// supplies only the authored body bytes.
pub struct ManualPage<'a> {
    /// Explicit navigation label.
    pub title: &'a str,
    /// Explicit extensionless target route.
    pub route: &'a EntryPath,
    /// Exact retained authored Markdown file.
    pub selection: &'a Selection,
}

/// One ordered authored-manual navigation section.
pub struct ManualSection<'a> {
    /// Explicit section navigation label.
    pub title: &'a str,
    /// Complete ordered page selection.
    pub pages: Vec<ManualPage<'a>>,
}

/// One explicit authored-manual companion copied into the documentation target.
pub struct ManualAsset<'a> {
    /// Exact retained source file or directory.
    pub selection: &'a Selection,
    /// Explicit output path for a file, or output directory for a directory selection.
    pub path: &'a EntryPath,
}

/// Retained authored-manual source shared by one project module.
///
/// The source identity and acquisition evidence remain application-owned. The
/// target receives only the checked tree plus explicit navigation and asset
/// selections; it never scans a repository or invents information architecture.
pub struct ManualSource<'a> {
    /// Exact configured capture key. Only modules selecting this same source
    /// share capture-relative manual destinations; equal bytes do not merge owners.
    pub source: &'a str,
    /// Complete retained source tree containing every selected page and asset.
    pub tree: crate::source::TreeInput<'a>,
    /// Explicit assets available to authored pages.
    pub assets: Vec<ManualAsset<'a>>,
}

impl Artifact<'_> {
    /// Borrow the selected file without rereading or copying its native bytes.
    ///
    /// # Errors
    /// Rejects directory selections and absent files; no marker or neighboring file is substituted.
    pub fn bytes(&self) -> Result<&[u8], Error> {
        match self.selection {
            Selection::File { path } => self.tree.file(path),
            Selection::Directory { .. } => None,
        }
        .ok_or_else(|| Error::Invalid {
            context: self.selection.to_string(),
            reason: "documentation format requires an explicitly selected regular file",
        })
    }
}

/// One configured artifact within a project navigation module.
pub struct Reference<'a> {
    /// Explicit reference namespace, not inferred from filenames or symbols.
    pub slug: &'a str,
    /// Reference navigation text, not an identity assertion.
    pub title: &'a str,
    /// Optional complete ordered Markdown-page navigation, relative to the artifact directory.
    /// Absence preserves the artifact adapter's existing navigation.
    pub pages: Option<&'a [ReferencePage]>,
    /// Exact retained native output.
    pub artifact: Artifact<'a>,
}

/// One explicitly ordered project documentation navigation item.
///
/// The variants share navigation placement only. Their source semantics remain
/// distinct: manual sections consume authored Markdown, while references consume
/// native generator artifacts.
pub enum ProjectItem<'a> {
    /// Authored human-readable manual section.
    Manual(ManualSection<'a>),
    /// Generated technical reference group.
    Reference(Reference<'a>),
}

/// Project module selected by the application from the workspace's identity graph.
pub struct ProjectModule<'a> {
    /// Explicit namespace in this documentation publication.
    pub slug: &'a str,
    /// Module navigation text; independent from the native project's identity.
    pub title: &'a str,
    /// Optional retained manual source, present only when a manual section is selected.
    pub manual: Option<ManualSource<'a>>,
    /// Complete ordered manual/reference navigation. No target may silently omit an item.
    pub navigation: Vec<ProjectItem<'a>>,
}

/// One resolved documentation publication; excludes native destinations and owner authority.
pub struct Request<'a> {
    /// Borrowed brand display name, not another target-owned name setting.
    pub name: &'a str,
    /// Configured public documentation base, independent of local hyperlink projection.
    pub public_base_url: Option<&'a crate::publication::routing::PublicBaseUrl>,
    /// Exact configured target options.
    pub target: &'a Target,
    /// Complete ordered project-module selection.
    pub projects: &'a [ProjectModule<'a>],
    /// The owning publication's TDM reservation and crawler rules, when it reserves rights.
    pub usage: Option<&'a crate::publication::usage::UsagePolicy<'a>>,
}

/// Target compatibility facts available before any native generator is invoked.
///
/// This contains no artifact bytes, native paths or provider observations. It lets
/// the selected target reject impossible presentation, namespace or format choices
/// before the application creates generator stages or starts a child process.
pub struct Plan<'a> {
    /// Borrowed brand display name, not a target-owned naming fallback.
    pub name: &'a str,
    /// Exact configured local target profile.
    pub target: &'a Target,
    /// Complete ordered project/reference shape and declared native formats.
    pub projects: Vec<PlannedProject<'a>>,
}

impl Plan<'_> {
    /// Validate the complete effect-free target contract.
    ///
    /// This checks configured presentation, namespaces and declared artifact
    /// shapes. It does not inspect generated bytes, convert a provider format,
    /// assemble a repository or perform any native effect.
    ///
    /// # Errors
    /// Rejects a target contract that cannot be represented without collision
    /// or unsupported target behavior.
    pub fn validate(&self) -> Result<(), Error> {
        contract::validate(self)
    }
}

/// One planned authored-manual navigation section.
pub struct PlannedManualSection<'a> {
    /// Configured section label.
    pub title: &'a str,
    /// Complete ordered page titles and routes.
    pub pages: Vec<PlannedManualPage<'a>>,
}

/// One planned authored-manual page.
pub struct PlannedManualPage<'a> {
    /// Configured page label.
    pub title: &'a str,
    /// Configured extensionless target route.
    pub route: &'a EntryPath,
}

/// One byte-free project navigation item.
pub enum PlannedProjectItem<'a> {
    /// Authored human-readable manual section.
    Manual(PlannedManualSection<'a>),
    /// Generated technical reference group.
    Reference(PlannedReference<'a>),
}

/// One project namespace in a pre-effect target plan.
pub struct PlannedProject<'a> {
    /// Configured target namespace.
    pub slug: &'a str,
    /// Configured navigation label.
    pub title: &'a str,
    /// Explicit target paths for the project's authored-manual assets.
    pub manual_assets: Vec<&'a EntryPath>,
    /// Complete ordered manual/reference navigation.
    pub navigation: Vec<PlannedProjectItem<'a>>,
}

/// One declared native artifact shape in a pre-effect target plan.
pub struct PlannedReference<'a> {
    /// Configured target namespace within its project.
    pub slug: &'a str,
    /// Configured navigation label.
    pub title: &'a str,
    /// Declared native generator format; no bytes have been generated yet.
    pub format: Format,
    /// Declared artifact scope available to byte-free target preflight before generation.
    pub selection: &'a Selection,
    /// Optional complete ordered page navigation; no artifact bytes are inspected here.
    pub pages: Option<&'a [ReferencePage]>,
}

impl<'a> Request<'a> {
    /// Project the byte-free target plan used by the same adapter before effects.
    pub fn plan(&self) -> Plan<'a> {
        Plan {
            name: self.name,
            target: self.target,
            projects: self
                .projects
                .iter()
                .map(|project| PlannedProject {
                    slug: project.slug,
                    title: project.title,
                    manual_assets: project.manual.as_ref().map_or_else(Vec::new, |manual| {
                        manual.assets.iter().map(|asset| asset.path).collect()
                    }),
                    navigation: project
                        .navigation
                        .iter()
                        .map(|item| match item {
                            ProjectItem::Manual(section) => {
                                PlannedProjectItem::Manual(PlannedManualSection {
                                    title: section.title,
                                    pages: section
                                        .pages
                                        .iter()
                                        .map(|page| PlannedManualPage {
                                            title: page.title,
                                            route: page.route,
                                        })
                                        .collect(),
                                })
                            }
                            ProjectItem::Reference(reference) => {
                                PlannedProjectItem::Reference(PlannedReference {
                                    slug: reference.slug,
                                    title: reference.title,
                                    format: reference.artifact.format,
                                    selection: reference.artifact.selection,
                                    pages: reference.pages,
                                })
                            }
                        })
                        .collect(),
                })
                .collect(),
        }
    }
}

/// Target assembly rejection; no successful partial repository is returned.
#[derive(Debug)]
pub enum Error {
    /// Unsupported or invalid selected configuration/artifact.
    Invalid {
        /// Logical input context, never artifact payloads.
        context: String,
        /// Violated target invariant.
        reason: &'static str,
    },
    /// Native JSON decoding or target configuration encoding failed.
    Json(serde_json::Error),
    /// Bounded native Markdown analysis failed, without rewriting its source.
    Markdown(crate::markdown::Error),
    /// Complete output tree invariants or limits failed.
    Tree(crate::source::Error),
}

impl Error {
    /// Stable interface class; source failures retain their native class.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Invalid { .. } => "documentation_target_input",
            Self::Json(_) => "documentation_target_json",
            Self::Markdown(_) => "documentation_target_markdown",
            Self::Tree(error) => error.code(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid { context, reason } => write!(f, "{context}: {reason}"),
            Self::Json(error) => write!(f, "documentation target JSON: {error}"),
            Self::Markdown(error) => error.fmt(f),
            Self::Tree(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            Self::Markdown(error) => Some(error),
            Self::Tree(error) => Some(error),
            Self::Invalid { .. } => None,
        }
    }
}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}
impl From<crate::source::Error> for Error {
    fn from(error: crate::source::Error) -> Self {
        Self::Tree(error)
    }
}
