//! Pure current-input analysis shared by authoring interfaces.
//!
//! Supplied buffers are not native captures, publication admission or build
//! receipts. One explicit workspace and logical corpus use the ordinary
//! compiler. No URI, editor version, filesystem read or previous result enters
//! this operation.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    sync::Arc,
};

use gazette_compiler::{
    authored, corpus, document, landing, markdown, plugin, theme, theme_graph, theme_package, toml,
    workspace,
};
use gazette_core::{
    documentation::{
        Error as DocumentationTargetError, Selection,
        projection::{
            AuthoringClosure, AuthoringManualPage, AuthoringReference, LinkProjection,
            MANUAL_PROFILE, ManualDestinations, PageKind, manual_source,
        },
    },
    markdown::{Directive, Limits as MarkdownLimits},
    publication::{Key, content},
    source::{EntryPath, Location, Sha256Digest, Span},
};
use gazette_ports::markdown::Markdown;

mod analyze;
use analyze::{AnalysisCorpus, analyze_selected};
mod assistance;
mod authored_completion;
mod symbolic_completion;
pub use authored_completion::{AuthoredReferenceCandidate, AuthoredReferenceCompletion};
pub use symbolic_completion::{SymbolicCandidate, SymbolicReferenceCompletion};
mod completion;
mod components;
mod documents;
mod editorial_completion;
pub use editorial_completion::{EditorialCandidate, EditorialReferenceCompletion};
mod navigation;
pub use assistance::{Assistance, AssistanceScope, PageResourceNavigation};
pub use completion::{HeadingCompletion, ReferenceCandidate, ReferenceCompletion};
pub use components::{
    ComponentAssistance, ComponentCandidate, ComponentCompletion, ComponentHover, LandingChildren,
};
pub use navigation::{Navigation, NavigationError, SourceLink, SourceLocation, WorkspaceIdentity};

pub use corpus::Input;

/// Complete explicitly selected corpus roles for one authoring analysis.
///
/// Publication documents, authored manuals and opaque page-package inputs remain
/// distinct even though all three contribute retained source evidence.
pub struct SelectedCorpus<'s> {
    /// Gazette Theme-rendered publication documents.
    pub documents: Vec<Input<'s>>,
    /// Authored product-manual pages, excluded from publication semantics.
    pub manuals: Vec<ManualBaseline<'s>>,
    /// Opaque retained page-package companion bytes, excluded from text editing.
    pub page_inputs: Vec<Input<'s>>,
    /// Observed opaque file sizes for editor structure, without captured identity.
    pub asset_metadata: BTreeMap<EntryPath, u64>,
}

/// One authored-manual editor baseline bound to its configured source identity.
/// Capture-relative paths are scoped by `source`; equal paths from different
/// configured sources remain distinct inputs.
pub struct ManualBaseline<'s> {
    /// Exact configured workspace source key owning this manual tree.
    pub source: Key,
    /// Capture-relative path within the configured source.
    pub path: EntryPath,
    /// Complete retained Markdown bytes supplied by the authoring client.
    pub bytes: &'s [u8],
}

/// Exact authored-manual input identity retained by authoring analysis.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ManualBinding {
    /// Exact configured workspace source key owning this manual input.
    pub source: Key,
    /// Capture-relative path within the configured source.
    pub path: EntryPath,
}

impl ManualBinding {
    /// Collision-free compiler path for this configured manual identity. This is
    /// internal analysis routing; transports continue to expose `path`.
    ///
    /// # Errors
    ///
    /// Returns an authoring error when the source-qualified identity cannot form a
    /// confined analysis path.
    pub fn analysis_path(&self) -> Result<EntryPath, Error> {
        qualified_manual_path(self)
    }
}

#[derive(Debug)]
struct RetainedManual {
    analysis_path: EntryPath,
    source_sha256: Sha256Digest,
    bytes: Vec<u8>,
}

/// Explicit workspace authoring source shape; neither mode performs native discovery.
pub enum WorkspaceInput<'s> {
    /// One complete caller-supplied workspace buffer. Includes are rejected normally.
    Buffer(Input<'s>),
    /// One exact root plus its complete explicitly selected non-recursive document set.
    Documents {
        /// Exact logical root document, which must occur once in `documents`.
        root: EntryPath,
        /// Root and every declared include, with no extra or discovered file.
        documents: Vec<Input<'s>>,
    },
}

/// Explicit typed identity of one current package baseline.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PackageBaselineKey {
    /// Theme package declaration participating in a selected dependency graph.
    Theme(theme_package::ThemePackageId),
    /// Plugin package declaration, independent from Theme component assistance.
    Plugin(gazette_core::plugin::PluginPackageId),
}

impl PackageBaselineKey {
    fn as_str(&self) -> &str {
        match self {
            Self::Theme(package) => package.as_str(),
            Self::Plugin(package) => package.as_str(),
        }
    }
}

/// One explicitly supplied package-manifest baseline.
///
/// Its typed package key selects the workspace package binding. The binding alone
/// supplies the source and manifest path; callers cannot restate or override them.
pub struct PackageBaseline<'s> {
    /// Explicit package class and typed identity.
    pub package: PackageBaselineKey,
    /// Complete current candidate manifest bytes.
    pub bytes: &'s [u8],
}

/// Independent native parser/resolver limits and owned source-projection bounds.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Workspace source size and native TOML structure ceilings.
    pub workspace: toml::Limits,
    /// Whole-set ceilings for explicitly selected modular workspace documents.
    pub workspace_documents: toml::SetLimits,
    /// Selected file membership, source bytes and native content parse ceilings.
    pub corpus: corpus::Limits,
    /// Cross-document semantic graph resolution ceilings.
    pub content: content::Limits,
    /// Aggregate headings copied across the complete selected set.
    pub headings: usize,
    /// Aggregate heading text, IDs and URL-fragment bytes copied into the result.
    pub heading_bytes: usize,
    /// Aggregate retained workspace/reference, footnote, heading-link and exact
    /// completion-slot records. A heading itself uses the separate heading ceiling.
    pub navigation_occurrences: usize,
    /// Aggregate workspace keys, native association labels, enclosing definitions
    /// and complete source/publication reference records.
    pub navigation_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            workspace: toml::Limits::default(),
            workspace_documents: toml::SetLimits::default(),
            corpus: corpus::Limits::default(),
            content: content::Limits::default(),
            headings: 16_384,
            heading_bytes: 4 * 1024 * 1024,
            navigation_occurrences: 16_384,
            navigation_bytes: 4 * 1024 * 1024,
        }
    }
}

/// Original compiler records, grouped by their exact file and verbatim body.
#[derive(Debug)]
pub struct Headings {
    /// Full-file range containing the body; values retain body-relative ranges.
    pub body_span: Span,
    /// Heading constructs, not whole-section extents or exact editable tokens.
    pub values: Vec<markdown::Heading>,
}

/// Phase-native evidence, without a second diagnostic schema or lost origins.
#[derive(Debug)]
pub enum Failure {
    /// The explicitly selected workspace is not UTF-8.
    WorkspaceEncoding(std::str::Utf8Error),
    /// Current workspace syntax or semantic constraints failed.
    Workspace(workspace::Error),
    /// The selected corpus could not be parsed as a complete set.
    Corpus(corpus::Error),
    /// Complete selected-document reference resolution failed. Parsed headings
    /// remain useful, but no successful cross-document navigation subset exists.
    DocumentReferences(corpus::Error),
    /// Current declarations failed shared authored-content resolution.
    Authored(authored::Error),
    /// One explicitly supplied package baseline is invalid, misbound or incomplete.
    Package {
        /// Explicit package class and typed identity selected by the caller.
        package: PackageBaselineKey,
        /// Workspace-owned source binding, present when the package is declared.
        source: Option<Key>,
        /// Workspace-owned manifest path, present when the package is declared.
        manifest: Option<EntryPath>,
        /// Bounded phase-native failure.
        error: PackageFailure,
    },
    /// Heading allocation failed for this current file; the original error's
    /// coordinates remain body-relative, joined through the supplied body range.
    Headings {
        /// Selected logical file, bound by the analysis input inventory.
        path: EntryPath,
        /// Full-file body range used to lift the native error's coordinates.
        body_span: Span,
        /// Original failure from the Markdown heading allocator.
        error: markdown::Error,
    },
    /// Source-only Markdown rendering cannot preserve the selected construct.
    /// This is pre-Theme authoring evidence, not publication admission.
    Renderability {
        /// Selected logical file, bound by the analysis input inventory.
        path: EntryPath,
        /// Full-file body range used to lift the native error's coordinates.
        body_span: Span,
        /// Original failure from the shared Markdown compiler/adapter path.
        error: markdown::Error,
    },
    /// A retained component directive failed the selected Theme's native contract.
    Components {
        /// Selected logical authored file.
        path: EntryPath,
        /// How the native failure's position maps to the authored file.
        coordinates: ComponentCoordinates,
        /// Original component admission failure.
        error: markdown::Error,
    },
}

/// Mapping from a retained component diagnostic to its original source.
#[derive(Debug)]
pub enum ComponentCoordinates {
    /// Direct Markdown body within the selected source.
    Markdown(Span),
    /// Original containing YAML construct, when the normalized range maps back.
    Landing(Option<Location>),
}

/// Explicit package-baseline failure without provider or filesystem inference.
#[derive(Debug)]
pub enum PackageFailure {
    /// Supplied bytes are not UTF-8.
    Encoding,
    /// The package is undeclared or its manifest identity does not match its typed key.
    Binding(&'static str),
    /// Native Theme-package parsing or declaration validation failed.
    ThemeManifest(theme_package::Error),
    /// Native plugin-package parsing or declaration validation failed.
    PluginManifest(plugin::Error),
    /// The selected Theme package and its complete supplied dependency graph failed.
    ThemeGraph(theme_graph::Error),
}

#[derive(Debug)]
struct ComponentContext {
    body_span: Span,
    directives: Vec<Directive>,
    definitions: Option<Arc<theme::Components>>,
    authoring_limits: toml::Limits,
}

#[derive(Debug)]
struct LandingComponentContext {
    slots: Vec<landing::ComponentSlot>,
    definitions: Arc<theme::Components>,
}

#[derive(Debug)]
enum WorkspaceNavigation {
    Buffer(workspace::WorkspaceNavigation),
    Documents(workspace::WorkspaceDocumentsNavigation),
}

/// Compiler-owned page-package membership for one exact retained input.
#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct PagePackageOwnership<'a> {
    /// Owning authored content identity.
    pub content: &'a Key,
    /// The one compiler-resolved retained package.
    pub package: &'a authored::PagePackage,
    /// Ordered resource declarations after compiler validation. Their inputs and
    /// output routes are the only compiled destinations this view can prove.
    pub resources: &'a [authored::PageResource],
    /// Whether the queried input is the owning document or one retained companion.
    pub role: PagePackageRole<'a>,
}

/// Exact role of one retained input in its compiler-resolved page package.
#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PagePackageRole<'a> {
    /// The exact lowercase `index.md` or `index.markdown` package root.
    Document,
    /// One retained descendant, including article-owned Markdown notes.
    Companion {
        /// Existing compiler-owned companion evidence.
        companion: &'a authored::PageCompanion,
    },
}

/// Effect-free documentation authoring view over the resolved workspace schema.
#[derive(Clone, Debug, serde::Serialize, schemars::JsonSchema)]
pub struct DocumentationAuthoring {
    /// Contract-validated repository and ordered manual/reference navigation.
    pub check: crate::build::DocumentationCheck,
    /// Exact authored-manual page origins and configured routes, in navigation order.
    pub manual_pages: Vec<DocumentationManualPageAuthoring>,
    /// Whether final route/anchor closure can be proved from this editor input set.
    pub closure: DocumentationClosure,
}

/// One selected manual page kept separate from generated-reference navigation.
#[derive(Clone, Debug, serde::Serialize, schemars::JsonSchema)]
pub struct DocumentationManualPageAuthoring {
    /// Configured project module namespace.
    pub module: String,
    /// Configured manual navigation section.
    pub section: String,
    /// Exact workspace source identity owning the manual capture.
    pub source: Key,
    /// Navigation label.
    pub title: String,
    /// Explicit documentation route.
    pub route: EntryPath,
    /// Exact capture-relative selection retained by the documentation schema.
    pub input: Selection,
}

/// Honest closure state; target preflight is never promoted into source validation.
#[derive(Clone, Debug, serde::Serialize, schemars::JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum DocumentationClosure {
    /// There are no manual or generated inputs requiring retained source closure.
    Complete,
    /// Selected provider artifacts required for final closure are absent.
    Unavailable {
        /// Stable explanation suitable for editor diagnostics.
        reason: &'static str,
    },
}

/// Owned evidence over one complete, explicitly role-bound input set.
#[derive(Debug)]
pub struct Analysis {
    /// Which selected file supplied the complete workspace declaration.
    workspace: EntryPath,
    /// Exact paths playing the workspace-document role in this analysis.
    workspace_documents: BTreeSet<EntryPath>,
    /// Exact selected Markdown manuals. They participate in editor analysis only,
    /// never authored-content resolution or Theme component binding.
    manuals: BTreeMap<ManualBinding, RetainedManual>,
    /// Qualified compiler paths for manual-only Markdown analysis.
    manual_paths: BTreeSet<EntryPath>,
    /// Exact opaque retained inputs supplied only for page-package resolution.
    page_inputs: BTreeSet<EntryPath>,
    asset_metadata: BTreeMap<EntryPath, u64>,
    /// Exact selected membership and bytes, including assets without headings.
    /// Equal bytes at different paths remain different inputs.
    inputs: BTreeMap<EntryPath, Sha256Digest>,
    /// The compiler's sole page-package result; editor projections never infer paths.
    page_packages: BTreeMap<Key, authored::PagePackage>,
    /// Compiler-resolved page-local resource programs keyed by their content owner.
    page_resources: BTreeMap<Key, Vec<authored::PageResource>>,
    /// Current package-manifest identities keyed independently from logical content paths.
    package_baselines: BTreeMap<PackageBaselineKey, Sha256Digest>,
    /// Empty only when all attempted phases succeeded for this input set.
    failures: Vec<Failure>,
    /// Structural help may exist while workspace/content constraints fail.
    /// A corpus syntax failure produces no successful heading subset.
    headings: BTreeMap<EntryPath, Headings>,
    /// Source associations only; never resolved publication meaning or DOM IDs.
    footnotes: BTreeMap<EntryPath, navigation::Footnotes>,
    /// Compiler-resolved local fragments; errors never become partial reference sets.
    heading_links: BTreeMap<EntryPath, Result<Vec<markdown::HeadingReference>, markdown::Error>>,
    /// Exact literal inline fragment destinations, independent of whether they resolve.
    heading_destinations: BTreeMap<EntryPath, Vec<Span>>,
    /// Native complete `WikiLink` destination slots, lifted into their current full source.
    symbolic_destinations: BTreeMap<EntryPath, Vec<Span>>,
    /// Complete selected serving destinations from the current resolved graph only.
    symbolic_targets: Option<Vec<authored::SymbolicTarget>>,
    /// Exact native workspace site/principal declarations retained for authored references.
    workspace_reference_targets: Option<Vec<authored::AuthoredReferenceDeclaration>>,
    /// Complete graph-owned related namespace, including derived legal output identities.
    related_reference_targets: Option<Vec<authored::AuthoredReferenceDeclaration>>,
    /// Complete current selected-document associations, or their native failure.
    document_links: Result<Vec<documents::Reference>, ()>,
    /// Native editorial scalar associations, retained only after complete graph resolution.
    editorial_links: Vec<documents::Reference>,
    /// Native declaration and scalar facts for repair, separate from graph admission.
    editorial_authoring: Option<Result<authored::EditorialAuthoring, authored::ProjectionError>>,
    /// Compiler-owned declaration coordinates and typed same-workspace references.
    workspace_navigation: Option<WorkspaceNavigation>,
    /// Tolerant current-buffer structure and Rust-schema facts, independent from
    /// strict workspace validity and never substituted from an earlier analysis.
    workspace_authoring:
        BTreeMap<EntryPath, Result<workspace::WorkspaceAuthoring, workspace::AuthoringError>>,
    /// Current-buffer frontmatter facts keyed by exact selected logical path.
    authored_authoring:
        BTreeMap<EntryPath, Result<Option<authored::AuthoredAuthoring>, workspace::AuthoringError>>,
    /// Source-bound directive facts joined to the effective selected Theme catalog.
    components: BTreeMap<EntryPath, ComponentContext>,
    landing_components: BTreeMap<EntryPath, LandingComponentContext>,
    /// The same bounded compilation used for content resolution; no editor-owned cascade.
    workspace_compilation: Option<workspace::Compilation>,
}

fn close_documentation_authoring(
    analysis: &Analysis,
    view: &mut DocumentationAuthoring,
    markdown: &impl Markdown,
) -> Result<(), crate::build::DocumentationError> {
    use crate::build::DocumentationError;
    use workspace::DocumentationNavigation;

    let mut pages = Vec::new();
    for page in &view.manual_pages {
        let Selection::File { path } = &page.input else {
            return Err(DocumentationError::Target(
                DocumentationTargetError::Invalid {
                    context: format!("{}:{}", page.source, page.route),
                    reason: "authored manual page requires one explicit Markdown file",
                },
            ));
        };
        let bytes = analysis
            .manual_baseline(&page.source, path)
            .ok_or_else(|| {
                DocumentationError::Target(DocumentationTargetError::Invalid {
                    context: format!("{}:{}", page.source, path),
                    reason: "configured authored manual page has no exact retained editor baseline",
                })
            })?;
        pages.push(AuthoringManualPage {
            source: page.source.as_str(),
            input: path,
            route: &page.route,
            bytes,
        });
    }
    let routes = view
        .check
        .build
        .modules
        .iter()
        .flat_map(|module| {
            module.navigation.iter().filter_map(move |item| {
                let DocumentationNavigation::Reference { slug, .. } = item else {
                    return None;
                };
                Some(format!("{}/{}", module.slug, slug).parse().map_err(|_| {
                    DocumentationError::Target(DocumentationTargetError::Invalid {
                        context: format!("{}:{}", module.slug, slug),
                        reason: "configured generated-reference route cannot form a target path",
                    })
                }))
            })
        })
        .collect::<Result<Vec<EntryPath>, DocumentationError>>()?;
    let references = routes
        .iter()
        .map(|route| AuthoringReference { route })
        .collect::<Vec<_>>();
    let mut destinations = BTreeMap::<&str, ManualDestinations>::new();
    for page in &pages {
        destinations
            .entry(page.source)
            .or_default()
            .page(page.input, page.route)
            .map_err(|_| {
                DocumentationError::Target(DocumentationTargetError::Invalid {
                    context: format!("{}:{}", page.source, page.input),
                    reason: "authored manual source file is selected more than once for one configured source",
                })
            })?;
    }
    // Exact file companions are configured bindings, not retained editor bytes.
    // Resolve their links without inventing existence or complete materialization.
    let mut companion_paths = BTreeSet::new();
    for module in &view.check.build.modules {
        let Some(manual) = &module.manual else {
            continue;
        };
        for asset in &manual.assets {
            if let Selection::File { path } = &asset.input {
                destinations
                    .entry(manual.source.as_str())
                    .or_default()
                    .asset(path, asset.path.as_str())
                    .map_err(DocumentationError::Target)?;
                companion_paths.insert(asset.path.to_string());
            }
        }
    }
    let mut links = LinkProjection::default();
    for reference in &references {
        links
            .register_provider(reference.route.as_str(), PageKind::Incomplete, None)
            .map_err(DocumentationError::Target)?;
    }
    let mut emitted = BTreeSet::new();
    for page in &pages {
        let text = manual_source(page.bytes, page.input).map_err(DocumentationError::Target)?;
        let document = markdown
            .parse(text, MANUAL_PROFILE, MarkdownLimits::default())
            .map_err(|error| {
                DocumentationError::Target(DocumentationTargetError::Markdown(error))
            })?;
        // Authoring closes the selected source's routes and anchors; it neither
        // emits Markdown nor qualifies provider transport. Formatting here would
        // reject valid rich manuals or normalize their retained source bytes.
        let _ = links
            .manual(
                page.input,
                page.route.as_str(),
                &destinations[page.source],
                document.syntax(),
            )
            .map_err(DocumentationError::Target)?;
        emitted.insert(page.route.to_string());
    }
    let closure = links
        .validate(
            |route| emitted.contains(route),
            |path| companion_paths.contains(path),
        )
        .map_err(DocumentationError::Target)?;
    if !companion_paths.is_empty() {
        view.closure = DocumentationClosure::Unavailable {
            reason: if references.is_empty() {
                "manual companion paths are configured but their bytes are unavailable in the editor session; retained manual page routes, anchors and explicit companion bindings passed"
            } else {
                "generated-reference provider artifacts and manual companion bytes are unavailable in the editor session; retained manual page routes, anchors and explicit companion bindings passed"
            },
        };
        return Ok(());
    }
    view.closure = match if references.is_empty() && !closure.incomplete() {
        AuthoringClosure::Complete
    } else {
        AuthoringClosure::GeneratedReferencesIncomplete
    } {
        AuthoringClosure::Complete => DocumentationClosure::Complete,
        AuthoringClosure::GeneratedReferencesIncomplete => DocumentationClosure::Unavailable {
            reason: "generated-reference provider artifacts are unavailable in the editor session; retained authored-manual route and anchor closure passed",
        },
    };
    Ok(())
}

impl Analysis {
    /// Resolve every configured documentation repository through the ordinary target
    /// contract and expose its manual routes/origins without acquiring native trees.
    ///
    /// The application joins retained manual baselines by configured source and
    /// capture-relative path, then runs the inward-owned route/anchor closure.
    /// Generated artifacts remain absent.
    ///
    /// # Errors
    /// Returns the existing documentation error when the selected target rejects a
    /// resolved plan; no partial per-site authoring view is returned.
    pub fn documentation_authoring(
        &self,
        markdown: &impl Markdown,
    ) -> Result<Vec<DocumentationAuthoring>, crate::build::DocumentationError> {
        let Some(compilation) = &self.workspace_compilation else {
            return Ok(Vec::new());
        };
        let mut result = Vec::new();
        for site in compilation.workspace.sites().filter(|site| {
            compilation
                .workspace
                .documentation_repository(&site.id)
                .and_then(|repository| repository.build.as_ref())
                .is_some()
        }) {
            let check = crate::build::check_documentation_workspace_contract(
                &compilation.workspace,
                &site.id,
            )?;
            let mut manual_pages = Vec::new();
            let mut has_inputs = false;
            for module in &check.build.modules {
                has_inputs |= module.navigation.iter().any(|item| {
                    matches!(item, workspace::DocumentationNavigation::Reference { .. })
                });
                if let Some(manual) = &module.manual {
                    for navigation in &module.navigation {
                        if let workspace::DocumentationNavigation::Manual { title, pages } =
                            navigation
                        {
                            has_inputs |= !pages.is_empty();
                            manual_pages.extend(pages.iter().map(|page| {
                                DocumentationManualPageAuthoring {
                                    module: module.slug.clone(),
                                    section: title.clone(),
                                    source: manual.source.clone(),
                                    title: page.title.clone(),
                                    route: page.route.clone(),
                                    input: page.input.clone(),
                                }
                            }));
                        }
                    }
                }
            }
            let mut view = DocumentationAuthoring {
                check,
                manual_pages,
                closure: if has_inputs {
                    DocumentationClosure::Unavailable {
                        reason: "complete retained manual/reference trees are unavailable in the editor session",
                    }
                } else {
                    DocumentationClosure::Complete
                },
            };
            close_documentation_authoring(self, &mut view, markdown)?;
            result.push(view);
        }
        Ok(result)
    }

    /// Role selection is bound independently from the file's bytes.
    pub fn workspace(&self) -> &EntryPath {
        &self.workspace
    }

    /// Exact selected configuration documents. A supplied-buffer workspace contains
    /// only [`Self::workspace`]; a modular workspace contains its root and includes.
    pub fn workspace_documents(&self) -> &BTreeSet<EntryPath> {
        &self.workspace_documents
    }

    /// Exact selected authored-manual baselines.
    pub fn manuals(&self) -> impl Iterator<Item = (&ManualBinding, &Sha256Digest)> {
        self.manuals
            .iter()
            .map(|(binding, retained)| (binding, &retained.source_sha256))
    }

    /// Resolve one configured manual page to its exact retained baseline bytes.
    pub fn manual_baseline(&self, source: &Key, path: &EntryPath) -> Option<&[u8]> {
        self.manuals
            .get(&ManualBinding {
                source: source.clone(),
                path: path.clone(),
            })
            .map(|retained| retained.bytes.as_slice())
    }

    /// Qualified internal path used only for shared Markdown analysis.
    pub fn manual_analysis_path(&self, source: &Key, path: &EntryPath) -> Option<&EntryPath> {
        self.manuals
            .get(&ManualBinding {
                source: source.clone(),
                path: path.clone(),
            })
            .map(|manual| &manual.analysis_path)
    }

    /// Exact opaque retained inputs supplied for page-package analysis.
    pub fn page_inputs(&self) -> &BTreeSet<EntryPath> {
        &self.page_inputs
    }

    /// Resolve an observed asset by its metadata in this analysis, without a content-identity claim.
    ///
    /// # Errors
    /// Refuses a path or observed size that differs from this analysis's selection.
    pub fn observed_page_package_ownership(
        &self,
        path: &EntryPath,
        observed_bytes: u64,
    ) -> Result<Option<PagePackageOwnership<'_>>, NavigationError> {
        if self.asset_metadata.get(path) != Some(&observed_bytes) {
            return Err(NavigationError::InvalidSource);
        }
        for (content, package) in &self.page_packages {
            if let Some(companion) = package.companions().iter().find(|entry| {
                entry.path() == path
                    && entry.source_sha256().is_none()
                    && entry.observed_bytes() == Some(observed_bytes)
            }) {
                return Ok(Some(PagePackageOwnership {
                    content,
                    package,
                    resources: self.page_resources.get(content).map_or(&[], Vec::as_slice),
                    role: PagePackageRole::Companion { companion },
                }));
            }
        }
        Ok(None)
    }

    /// Captured input identities; metadata-only observations carry no revision evidence.
    pub fn inputs(&self) -> &BTreeMap<EntryPath, Sha256Digest> {
        &self.inputs
    }

    /// Resolve one exact retained input to the compiler-owned page package.
    ///
    /// # Errors
    /// Refuses unknown paths or a digest that does not name the analyzed bytes.
    pub fn page_package_ownership(
        &self,
        path: &EntryPath,
        source_sha256: Sha256Digest,
    ) -> Result<Option<PagePackageOwnership<'_>>, NavigationError> {
        if self.inputs.get(path) != Some(&source_sha256) {
            return Err(NavigationError::InvalidSource);
        }
        for (content, package) in &self.page_packages {
            if package.document() == path {
                return Ok(Some(PagePackageOwnership {
                    content,
                    package,
                    resources: self.page_resources.get(content).map_or(&[], Vec::as_slice),
                    role: PagePackageRole::Document,
                }));
            }
            if let Some(companion) = package
                .companions()
                .iter()
                .find(|entry| entry.path() == path && entry.source_sha256() == Some(source_sha256))
            {
                return Ok(Some(PagePackageOwnership {
                    content,
                    package,
                    resources: self.page_resources.get(content).map_or(&[], Vec::as_slice),
                    role: PagePackageRole::Companion { companion },
                }));
            }
        }
        Ok(None)
    }

    /// Complete immutable package-baseline membership and revision evidence.
    pub fn package_baselines(&self) -> &BTreeMap<PackageBaselineKey, Sha256Digest> {
        &self.package_baselines
    }

    /// Original native errors, including all supplied duplicate-content origins.
    pub fn failures(&self) -> &[Failure] {
        &self.failures
    }

    /// Borrow current workspace declarations without reconstructing its syntax or identity.
    ///
    /// A selected non-workspace file returns none. Valid workspace facts remain
    /// available when unrelated content parsing fails; invalid current workspace
    /// input never returns an older or partial declaration set.
    ///
    /// # Errors
    /// Refuses unknown or changed sources and unavailable current workspace facts.
    pub fn workspace_symbols(
        &self,
        path: &EntryPath,
        source: &str,
    ) -> Result<Option<Vec<&workspace::WorkspaceSymbol>>, NavigationError> {
        let Some(navigation) = self.workspace_navigation_for(path, source)? else {
            return Ok(None);
        };
        Ok(Some(match navigation {
            WorkspaceNavigation::Buffer(navigation) => navigation.symbols().iter().collect(),
            WorkspaceNavigation::Documents(navigation) => navigation.symbols(path).collect(),
        }))
    }

    /// Borrow heading facts only against their exact complete source preimage.
    /// Callers lift body ranges with `Span::in_parent` before editor conversion.
    /// Wrong revisions, unknown paths and failed/unsupported bodies return none.
    pub fn headings(&self, path: &EntryPath, source: &str) -> Option<&Headings> {
        if self.inputs.get(path)? != &Sha256Digest::of(source.as_bytes()) {
            return None;
        }
        self.headings.get(path)
    }

    /// Inspect current heading availability without confusing failure with an empty document.
    ///
    /// # Errors
    /// Refuses unknown/changed inputs and absent headings when current corpus or
    /// heading analysis failed. A supported document without headings returns none.
    pub fn heading_symbols(
        &self,
        path: &EntryPath,
        source: &str,
    ) -> Result<Option<&Headings>, NavigationError> {
        if self.inputs.get(path) != Some(&Sha256Digest::of(source.as_bytes())) {
            return Err(NavigationError::InvalidSource);
        }
        let headings = self.headings.get(path);
        if headings.is_none()
            && self
                .failures
                .iter()
                .any(|failure| matches!(failure, Failure::Corpus(_) | Failure::Headings { .. }))
        {
            return Err(NavigationError::Unavailable);
        }
        Ok(headings)
    }

    /// All selected syntax, heading allocation, source-only renderability,
    /// symbolic publication references and content/workspace constraints succeeded
    /// now. This is not complete rendering, Theme selection, general link/asset
    /// validation, public admission or a claim that every rich-content capability
    /// was checked.
    pub fn structurally_valid(&self) -> bool {
        self.failures.is_empty()
    }
}

/// The selection or owned projection cannot be represented completely.
#[derive(Debug)]
pub enum Error {
    /// Ambiguous file membership or workspace/content role selection.
    Selection(corpus::Error),
    /// Package-baseline identities overlap ambiguously.
    PackageSelection(&'static str),
    /// Authored-manual source/path bindings overlap or cannot be represented.
    ManualSelection(&'static str),
    /// Named complete-input or owned-projection resource ceiling was exceeded.
    Limit(&'static str),
    /// A native body or heading could not be joined to its exact selected source.
    InvalidSource,
    /// Current source discovery failed; a previous successful analysis is unavailable.
    UnavailableSources(&'static str),
}

impl Error {
    /// Stable operation class without embedding source values.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Selection(error) => error.code(),
            Self::PackageSelection(_) => "authoring_package_selection",
            Self::ManualSelection(_) => "authoring_manual_selection",
            Self::Limit(_) => "authoring_limit",
            Self::InvalidSource => "authoring_source",
            Self::UnavailableSources(_) => "authoring_sources_unavailable",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Selection(error) => error.fmt(f),
            Self::PackageSelection(reason) => {
                write!(f, "authoring package selection: {reason}")
            }
            Self::ManualSelection(reason) => {
                write!(f, "authoring manual selection: {reason}")
            }
            Self::Limit(resource) => write!(
                f,
                "authoring {resource} ceiling exceeded; no complete analysis returned"
            ),
            Self::InvalidSource => {
                f.write_str("authoring source-coordinate invariant failed; no analysis returned")
            }
            Self::UnavailableSources(reason) => {
                write!(f, "authoring sources unavailable: {reason}")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Selection(error) => Some(error),
            _ => None,
        }
    }
}

/// Analyze one complete workspace and explicitly selected logical content set.
///
/// Invalid current syntax/semantics are returned as phase-native failures, never
/// replaced by a previous successful model. Workspace and corpus parsing are
/// independent; dependent content resolution runs only when both succeeded.
/// Parser/resolver limits bound their native errors; heading and navigation
/// limits bound copied projections. A transport separately bounds its encoding.
///
/// # Errors
/// Rejects duplicate role/path selections and exhausted whole-set/projection
/// bounds without a truncated result. No I/O, rendering or provider acquisition
/// occurs; the Markdown mechanism receives only the selected exact bytes.
pub fn analyze<'s>(
    workspace: Input<'s>,
    documents: impl IntoIterator<Item = Input<'s>>,
    limits: Limits,
    markdown: &dyn Markdown,
) -> Result<Analysis, Error> {
    analyze_selected(
        WorkspaceInput::Buffer(workspace),
        AnalysisCorpus {
            documents,
            manuals: [],
            page_inputs: [],
            asset_metadata: BTreeMap::new(),
        },
        [],
        limits,
        markdown,
    )
}

/// Analyze authored inputs plus explicitly keyed Theme and plugin package baselines.
/// Local-only authoring remains available through [`analyze`] without fabricating packages.
///
/// # Errors
///
/// Rejects duplicate package identities, exhausted whole-set bounds, invalid selected bytes,
/// or phase-native workspace, corpus, package and navigation failures.
pub fn analyze_with_package_baselines<'s>(
    workspace: Input<'s>,
    documents: impl IntoIterator<Item = Input<'s>>,
    package_baselines: impl IntoIterator<Item = PackageBaseline<'s>>,
    limits: Limits,
    markdown: &dyn Markdown,
) -> Result<Analysis, Error> {
    analyze_selected(
        WorkspaceInput::Buffer(workspace),
        AnalysisCorpus {
            documents,
            manuals: [],
            page_inputs: [],
            asset_metadata: BTreeMap::new(),
        },
        package_baselines,
        limits,
        markdown,
    )
}

/// Analyze authored publication inputs and distinct authored-manual baselines.
/// Manuals receive shared Markdown diagnostics and navigation, but never become
/// authored content or Theme component inputs.
///
/// # Errors
///
/// Rejects duplicate roles or paths, exhausted whole-set bounds, and the same
/// native workspace, corpus, package and navigation failures as the ordinary path.
pub fn analyze_with_manuals_and_package_baselines<'s>(
    workspace: Input<'s>,
    corpus: SelectedCorpus<'s>,
    package_baselines: impl IntoIterator<Item = PackageBaseline<'s>>,
    limits: Limits,
    markdown: &dyn Markdown,
) -> Result<Analysis, Error> {
    analyze_selected(
        WorkspaceInput::Buffer(workspace),
        AnalysisCorpus {
            documents: corpus.documents,
            manuals: corpus.manuals,
            page_inputs: corpus.page_inputs,
            asset_metadata: corpus.asset_metadata,
        },
        package_baselines,
        limits,
        markdown,
    )
}

/// Analyze one exact modular workspace document set with authored inputs and package baselines.
/// The supplied root and include membership are checked by the compiler's native
/// [`workspace::ParsedWorkspaceDocuments`] owner on every call.
///
/// # Errors
///
/// Rejects incomplete/extra/recursive workspace document sets, overlapping roles,
/// exhausted bounds, and invalid current inputs without falling back to prior state.
pub fn analyze_workspace_documents_with_package_baselines<'s>(
    root: EntryPath,
    workspace_documents: Vec<Input<'s>>,
    documents: impl IntoIterator<Item = Input<'s>>,
    package_baselines: impl IntoIterator<Item = PackageBaseline<'s>>,
    limits: Limits,
    markdown: &dyn Markdown,
) -> Result<Analysis, Error> {
    analyze_selected(
        WorkspaceInput::Documents {
            root,
            documents: workspace_documents,
        },
        AnalysisCorpus {
            documents,
            manuals: [],
            page_inputs: [],
            asset_metadata: BTreeMap::new(),
        },
        package_baselines,
        limits,
        markdown,
    )
}

/// Analyze one modular workspace with publication inputs, distinct authored manuals,
/// and package baselines. Manual selection does not alter workspace composition.
///
/// # Errors
///
/// Rejects incomplete workspace sets, overlapping roles, exhausted bounds, and
/// invalid current inputs without falling back to prior state.
pub fn analyze_workspace_documents_with_manuals_and_package_baselines<'s>(
    root: EntryPath,
    workspace_documents: Vec<Input<'s>>,
    corpus: SelectedCorpus<'s>,
    package_baselines: impl IntoIterator<Item = PackageBaseline<'s>>,
    limits: Limits,
    markdown: &dyn Markdown,
) -> Result<Analysis, Error> {
    analyze_selected(
        WorkspaceInput::Documents {
            root,
            documents: workspace_documents,
        },
        AnalysisCorpus {
            documents: corpus.documents,
            manuals: corpus.manuals,
            page_inputs: corpus.page_inputs,
            asset_metadata: corpus.asset_metadata,
        },
        package_baselines,
        limits,
        markdown,
    )
}

fn charge(used: &mut usize, add: usize, limit: usize, resource: &'static str) -> Result<(), Error> {
    *used = used
        .checked_add(add)
        .filter(|sum| *sum <= limit)
        .ok_or(Error::Limit(resource))?;
    Ok(())
}

fn qualified_manual_path(binding: &ManualBinding) -> Result<EntryPath, Error> {
    let source = Sha256Digest::of(binding.source.as_str().as_bytes());
    format!(
        "__gazette_manual_sources/{source}/{}",
        binding.path.as_str()
    )
    .parse()
    .map_err(|_| Error::ManualSelection("configured manual identity cannot form an analysis path"))
}
