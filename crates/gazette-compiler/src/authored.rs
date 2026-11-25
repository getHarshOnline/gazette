//! Explicit structural frontmatter lowered from retained syntax into the shared graph.
//!
//! This profile does not replace the full authored-content schema or native-doc
//! admission. Unsupported metadata and unbound plain Markdown remain explicit failures,
//! not a successfully resolved partial publication. No native effects occur here.

mod asset_metadata;

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    sync::OnceLock,
};

use gazette_core::{
    publication::{
        Key,
        content::{
            self, BrowsingRelation, CollectionMembership, ContentNode, Declaration,
            EditorialRelation, ProjectDeclaration, ProjectDeclarationKind,
        },
    },
    source::{EntryPath, Sha256Digest, Span},
};
use serde::{Deserialize, Serialize};

mod assets;
mod authoring;
pub use authoring::{AuthoredAuthoring, AuthoredSchema};
mod bindings;
mod citation;
mod classifications;
pub use assets::{AssetError, AssetPlan, StaticAsset};
pub use bindings::SourceBindingError;
pub use citation::{CitationError, CitationView};
pub use classifications::{ClassificationDestination, ClassificationView, TaxonomyView};
mod input;
mod native;
pub use native::{NativeDerivation, NativeError, NativeTitle};
mod discovery;
mod discussion;
pub use discussion::DiscussionView;
mod feeds;
pub use feeds::{Feed, FeedError};
mod graph;
mod json_ld;
mod legal;
mod legal_processing;
pub use legal_processing::{LegalDiscussionContext, LegalProcessingContext};
mod links;
pub use legal::{
    LegalSourceError, LegalSourceMetadata, LoweredLegalSource, legal_source_schema,
    lower_legal_source,
};
mod listing_projects;
mod listings;
mod markdown_alternate;
pub use listings::{
    ListingAttribution, ListingError, ListingLink, ListingPage, ListingPageLink, ListingSortLink,
    ListingSubject, ListingSubjectLanding,
};
mod browsing;
pub use browsing::{BrowsingKind, BrowsingLocation};
mod container_contents;
mod project_resources;
pub(crate) mod public_identity;
pub use project_resources::{ProjectDocumentation, ProjectLanding};
mod redirects;
mod references;
pub use references::{PublicationReference, SymbolicTarget};
mod reference_authoring;
pub use reference_authoring::{
    AuthoredReferenceDeclaration, AuthoredReferenceField, AuthoredReferenceSlot,
};
mod editorial_references;
pub use editorial_references::EditorialReference;
mod editorial_authoring;
pub use editorial_authoring::{
    EditorialAuthoring, EditorialDeclaration, EditorialField, EditorialKind, EditorialSlot,
    editorial_authoring,
};
mod documentation_mounts;
pub use documentation_mounts::DocumentationMountView;
mod navigation;
mod notebooks;
mod notes;
mod site_chrome;
pub use notebooks::{BoundNotebook, NotebookError, NotebookEvidence};
pub use notes::{ArticleNotes, NotebookDownload, NotebookView};
mod page_packages;
pub use page_packages::{PageCompanion, PageCompanionKind, PagePackage};
mod recommendations;
pub use recommendations::{Recommendation, RecommendationReason, Recommendations};
mod presentation;
mod principals;
mod subjects;
pub use subjects::SubjectView;
mod projection;
pub use principals::PrincipalWebsiteView;
mod brand_graph;
mod publication;
pub use publication::{
    PublicationBrand, PublicationHome, PublicationMetadata, PublicationMetadataError, RenderYear,
};
mod body_images;
mod listing_artwork;
mod resources;
pub use listing_artwork::{ListingArtwork, ListingArtworkError};
mod retained_only;
mod search;
mod selection;
mod taxonomies;
pub use discovery::{
    Discovery, DiscoveryData, DiscoveryNode, DiscoveryPage, DiscoverySearch, LlmsDocument,
    LlmsPage, LlmsReadingSection, MarkdownAlternate, MirroredPage, ProjectOriginError, RoutedPage,
    SitePages, SitePagesError,
};
pub use graph::{
    ContainerLink, DiscoveryGraph, ForeignContent, JourneyStep, SiteGraph, SiteGraphError,
};
pub use json_ld::{JsonLd, PublicJsonLd, PublicListingJsonLd};
pub use navigation::{
    BreadcrumbTrail, CollectionNavigation, DocumentationNavigation, DocumentationNavigationEvent,
    JourneyMembership, NavigationLink, ResolvedDestination,
};
pub use presentation::{ContentView, SitePresentation};
pub use projection::{
    AttributionView, PageData, PageProjection, ProjectDeclarationView, ProjectView,
    ProjectionError, PublicationRoleView, SearchDocument, SiteView,
};
pub use resources::PageResource;
pub use selection::{Selection, SelectionError};
pub use site_chrome::{
    InvitationView, SearchControlView, SettingsControlView, ShellView, SiteChromeView,
    SiteNavigationEntry,
};
pub use taxonomies::{
    ProjectedTaxonomyPage, TaxonomyError, TaxonomyGroup, TaxonomyLink, TaxonomyListing,
};

use crate::{
    corpus::{File, ParsedCorpus},
    document::ParsedDocument,
    toml::{self, PathSegment},
    workspace::{Compilation, ResolvedContent},
};

/// One explicit structural record and its outgoing non-container relations.
///
/// The `content` table uses the core type directly. No second node schema or
/// renderer-specific translation of kinds, identities or lifecycle is maintained.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Metadata<S = Key> {
    content: ContentNode<S>,
    /// Facts owned by this landing, distinct from the plural About/Mentions edges.
    #[serde(default)]
    project: Option<gazette_core::publication::Project>,
    #[serde(default)]
    listing_artwork: Option<listing_artwork::ArtworkSelection>,
    #[serde(default, deserialize_with = "crate::tables::many")]
    images: Vec<body_images::ImageDeclaration>,
    #[serde(default)]
    template: Option<EntryPath>,
    #[serde(default, deserialize_with = "crate::tables::many")]
    resources: Vec<PageResource>,
    #[serde(default, deserialize_with = "notebooks::declarations")]
    notebooks: Vec<notebooks::NotebookDeclaration>,
    #[serde(default)]
    collections: Vec<Key>,
    #[serde(default)]
    related: Vec<Key>,
    #[serde(default)]
    browsing: Vec<Key>,
    #[serde(default)]
    projects: Vec<ProjectReference>,
}

/// Draft 2020-12 schema for the exact frontmatter consumed by authored compilation.
/// Field assistance and public discovery share this native type, not a second schema.
#[must_use]
pub fn declaration_schema() -> schemars::Schema {
    schemars::schema_for!(Metadata<Option<Key>>)
}

/// Source identity comes from the enclosing node, not a duplicate frontmatter field.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct ProjectReference {
    project: Key,
    kind: ProjectDeclarationKind,
}

/// Complete-document identity and an exact range within its original UTF-8 bytes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct Origin {
    /// Logical corpus path, not a host path or canonical URL.
    pub path: EntryPath,
    /// Digest of the whole document, including fences and line endings.
    pub source_sha256: Sha256Digest,
    /// Half-open full-document byte range; absence never invents a field location.
    pub span: Option<Span>,
}

/// A file cannot supply the selected structural metadata profile.
#[derive(Debug)]
pub struct FileFailure {
    /// Exact input path and source identity; syntax failures retain parser ranges.
    pub origin: Origin,
    /// Exact reason this file cannot enter the selected structural profile.
    pub error: FileError,
}

/// Schema failure and native-document policy routing are different outcomes.
#[derive(Debug)]
pub enum FileError {
    /// Retained metadata does not conform to the selected profile.
    Metadata(toml::Error),
    /// Structured landing metadata violates the same authored schema.
    YamlMetadata(crate::yaml::Error),
    /// Unframed Markdown needs its native-document binding, not invented frontmatter.
    NativeBindingRequired,
    /// An omitted home needs an exact configured source placement.
    SiteBindingRequired,
    /// Explicit native-profile lowering failed without substituting an empty page.
    Native(NativeError),
    /// Page resource declarations violate explicit source/output or aggregate bounds.
    Resource(gazette_core::resource::Error),
    /// Selected reduced legal metadata or expected owner identity failed.
    Legal(LegalSourceError),
}

/// Lowering or whole-graph failure, with no successful subset.
#[derive(Debug)]
pub enum Error {
    /// An editor asset observation is ambiguous, unbound or exceeds its limits.
    AssetMetadata {
        /// Logical source coordinate, without captured bytes or a digest.
        path: EntryPath,
        /// Why the observation cannot participate in authoring analysis.
        reason: &'static str,
    },
    /// Exact retained-input disposition is absent, inapplicable or conflicts with use.
    RetainedOnly {
        /// Source-bound logical input, never a guessed publication path.
        path: EntryPath,
        /// Stable failed admission condition.
        reason: &'static str,
    },
    /// An explicit public publication interface could not bind to its selected content.
    PublicationMetadata(PublicationMetadataError),
    /// An explicit legal entry is absent or not a framed retained document.
    LegalInput {
        /// Exact private logical source coordinate, never a public output route.
        path: EntryPath,
        /// Stable explanation without a fabricated source digest or range.
        reason: &'static str,
    },
    /// An explicitly attached notebook cannot resolve to its page's retained source.
    Notebook(NotebookError),
    /// A declared decorative image cannot bind to retained static transport.
    Artwork(ListingArtworkError),
    /// Every independent file-schema failure in logical-path order.
    Files(Vec<FileFailure>),
    /// The sole graph/workspace resolver rejected the complete declaration.
    Structure {
        /// Original structural/resource failure, without class substitution.
        error: content::Error,
        /// All declaring locations, retaining every conflicting duplicate identity.
        declarations: BTreeMap<Key, Vec<Origin>>,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AssetMetadata { path, reason } => write!(f, "asset metadata {path}: {reason}"),
            Self::RetainedOnly { path, reason } => write!(
                f,
                "retained-only input {path}: {reason}; no resolved corpus produced"
            ),
            Self::PublicationMetadata(error) => error.fmt(f),
            Self::LegalInput { path, reason } => write!(
                f,
                "legal input {path}: {reason}; no resolved corpus produced"
            ),
            Self::Notebook(error) => error.fmt(f),
            Self::Artwork(error) => error.fmt(f),
            Self::Files(errors) => {
                write!(f, "{} authored metadata failures", errors.len())?;
                for error in errors {
                    write!(f, "\n  {:?}: ", error.origin.path)?;
                    match &error.error {
                        FileError::Metadata(metadata) => metadata.fmt(f)?,
                        FileError::YamlMetadata(metadata) => metadata.fmt(f)?,
                        FileError::NativeBindingRequired => f.write_str("unframed Markdown requires native-document binding; no metadata inferred")?,
                        FileError::SiteBindingRequired => f.write_str("omitted content site requires a configured source binding")?,
                        FileError::Native(error) => error.fmt(f)?,
                        FileError::Resource(error) => error.fmt(f)?,
                        FileError::Legal(error) => error.fmt(f)?,
                    }
                    if let Some(span) = error.origin.span {
                        write!(f, "; complete-document bytes {}..{}", span.start, span.end)?;
                    }
                }
                f.write_str("; no resolved authored corpus produced")
            }
            Self::Structure { error, .. } => error.fmt(f),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::PublicationMetadata(error) => Some(error),
            Self::Notebook(error) => Some(error),
            Self::Artwork(error) => Some(error),
            Self::Structure { error, .. } => Some(error),
            Self::Files(_)
            | Self::LegalInput { .. }
            | Self::RetainedOnly { .. }
            | Self::AssetMetadata { .. } => None,
        }
    }
}

/// Source-bound structural result, retaining every body and opaque asset.
///
/// The graph owns resolved values; the corpus owns syntax and bytes. The index
/// stores only their association, not another copy of content or site metadata.
#[derive(Debug)]
pub struct ResolvedAuthored<'a, 's> {
    content: ResolvedContent<'a>,
    corpus: &'a ParsedCorpus<'s>,
    paths: BTreeMap<Key, EntryPath>,
    derivations: BTreeMap<Key, NativeDerivation>,
    resources: BTreeMap<Key, Vec<PageResource>>,
    listing_artwork: BTreeMap<Key, listing_artwork::ArtworkSelection>,
    image_declarations: BTreeMap<Key, Vec<body_images::ImageDeclaration>>,
    notebooks: BTreeMap<Key, Vec<EntryPath>>,
    /// Declared inputs whose pages opted into a source download, before the scan.
    notebook_downloads: BTreeMap<Key, BTreeSet<EntryPath>>,
    page_packages: BTreeMap<Key, PagePackage>,
    templates: BTreeMap<Key, EntryPath>,
    assets: BTreeMap<Key, OnceLock<Result<AssetPlan<'a>, AssetError>>>,
    selected_assets: Option<BTreeSet<EntryPath>>,
    asset_metadata: BTreeMap<EntryPath, u64>,
    projection_limits: content::Limits,
    preview_link_bases: Option<gazette_core::publication::routing::PreviewLinkBases>,
    recommendation_index: OnceLock<recommendations::Index>,
}

impl<'a, 's> ResolvedAuthored<'a, 's> {
    /// Select explicit preview-only hyperlink bases without changing content or canonical routes.
    pub fn with_preview_link_bases(
        mut self,
        bases: gazette_core::publication::routing::PreviewLinkBases,
    ) -> Self {
        self.preview_link_bases = Some(bases);
        self
    }

    /// Project owned publication and explicitly mapped documentation hyperlinks for a preview.
    /// This supplies no listener, source capture or canonical-address replacement.
    ///
    /// # Errors
    /// Refuses unsafe paths, an unmapped owned documentation host in local mode,
    /// and loopback hyperlinks in public mode.
    pub fn preview_public_link(
        &self,
        destination: &str,
    ) -> Result<Option<String>, gazette_core::publication::routing::InvalidRoute> {
        use gazette_core::publication::routing::{InvalidRoute, is_loopback_link};
        let urls = self.content.preview_urls();
        if urls.is_none() && is_loopback_link(destination) {
            return Err(InvalidRoute);
        }
        let workspace = &self.content.compilation().workspace;
        if let Some(urls) = urls {
            for (site, target) in urls {
                if let Some(base) = workspace.public_base_url(site)
                    && let Some(projected) = base.project_link(destination, target)?
                {
                    return Ok(Some(projected));
                }
            }
        }
        let projected = self
            .preview_link_bases
            .as_ref()
            .map(|bases| bases.project(destination))
            .transpose()?
            .flatten();
        if projected.is_none() && urls.is_some() {
            for site in workspace.sites() {
                if let Some(base) = workspace
                    .documentation_repository(&site.id)
                    .and_then(|repository| repository.public_base_url.as_ref())
                    && base.project_link(destination, base)?.is_some()
                {
                    return Err(InvalidRoute);
                }
            }
        }
        Ok(projected)
    }

    /// Recompile serving URLs for a bounded nonempty set of literal-loopback HTTP
    /// bases, preserving configured path prefixes and the original compilation.
    /// Unselected sites remain production-linked; feed identities are not changed.
    /// Cached asset projections are discarded before any subsequent consumption.
    ///
    /// # Errors
    /// Refuses unknown/unconfigured sites, nonlocal or changed-prefix bases, URL
    /// amplification limits and route collisions without a partial projection.
    pub fn with_preview_urls(
        mut self,
        urls: BTreeMap<Key, gazette_core::publication::routing::PublicBaseUrl>,
    ) -> Result<Self, content::Error> {
        self.content = self
            .content
            .with_preview_urls(urls, self.projection_limits)?;
        for cache in self.assets.values_mut() {
            *cache = OnceLock::new();
        }
        Ok(self)
    }

    /// The same immutable graph and workspace binding used by other compiler consumers.
    pub fn content(&self) -> &ResolvedContent<'a> {
        &self.content
    }

    /// Complete retained input, including script/style bytes and validated notebooks.
    /// Their presence is not evidence that their capabilities have been compiled.
    pub fn corpus(&self) -> &'a ParsedCorpus<'s> {
        self.corpus
    }

    /// Exact logical input association, independent of a field's optional source span.
    pub fn source_path(&self, id: &Key) -> Option<&EntryPath> {
        self.paths.get(id)
    }

    /// Explicit entry within the selected Theme, not a content identity or a
    /// template inferred from family, source path or collection membership.
    /// Absence leaves the caller's site default in force.
    pub fn template(&self, id: &Key) -> Option<&EntryPath> {
        self.templates.get(id)
    }

    /// Effective page entry: retained source override, then composing-site binding.
    /// Absence leaves the selected Theme's default in force.
    /// Source metadata origins remain source origins, never fabricated workspace spans.
    pub fn selected_template(&self, id: &Key) -> Option<&EntryPath> {
        let node = self.content.graph().node(id)?;
        self.template(id).or_else(|| {
            self.content
                .compilation()
                .workspace
                .composition_for(&node.site)
                .and_then(|composition| composition.page_templates())
                .and_then(|bindings| bindings.value().get(id))
        })
    }

    /// Exact retained file, whether framed authored input or plain native Markdown.
    pub fn file(&self, id: &Key) -> Option<&'a File<'s>> {
        self.corpus.files().get(self.paths.get(id)?)
    }

    /// Derived metadata evidence; none means explicit metadata or an unknown identity.
    pub fn derivation(&self, id: &Key) -> Option<&NativeDerivation> {
        self.derivations.get(id)
    }

    /// Retained leaf-package companions for an exact `index.md` page.
    /// Standalone pages and unknown identities return none.
    pub fn page_package(&self, id: &Key) -> Option<&PagePackage> {
        self.page_packages.get(id)
    }

    /// Every retained leaf package, ordered by its owning content identity.
    pub fn page_packages(&self) -> &BTreeMap<Key, PagePackage> {
        &self.page_packages
    }

    /// Resolve explicitly framed metadata/body. Plain native Markdown returns none;
    /// use `file` for its original body, never synthesized frontmatter.
    pub fn document(&self, id: &Key) -> Option<&'a ParsedDocument<'s>> {
        match self.corpus.files().get(self.paths.get(id)?)? {
            File::Document(document) => Some(document),
            _ => None,
        }
    }

    /// Locate an explicit typed frontmatter key/index path in complete source bytes.
    ///
    /// The path is relative to the metadata root, e.g. `content`, `title`.
    /// Returns none for unknown content or missing/defaulted fields. It never
    /// substitutes a containing table span for an absent declaration.
    pub fn origin(&self, id: &Key, field: &[PathSegment]) -> Option<Origin> {
        if self
            .source_binding(id)
            .ok()
            .is_some_and(|source| source.legal_binding().is_some())
            && !matches!(field, [PathSegment::Key(table), PathSegment::Key(scalar)]
                if table == "content" && matches!(scalar.as_str(), "title" | "description" | "lifecycle" | "language"))
        {
            return None;
        }
        let file = self.file(id)?;
        let input = input::Input::from_file(file)?;
        Some(Origin {
            path: self.paths.get(id)?.clone(),
            source_sha256: file.source_sha256(),
            span: Some(input.field(field)?.span),
        })
    }
}

/// Lower framed metadata and configured native documents into one complete content set.
///
/// The caller supplies an aggregated corpus when references span sites. This
/// does not resolve incomplete sites independently. Plain Markdown requires an explicit
/// native profile; its derivation remains distinguishable from authored metadata.
/// Assets remain in the borrowed corpus. Unknown fields fail the
/// current profile instead of being silently lost; rich metadata/admission remain
/// separate implementation work, not implied by a structurally resolved result.
///
/// # Errors
/// Accumulates typed metadata/unframed-input failures before dependent resolution.
/// Graph ceilings, duplicate identities, relation failures and unknown sites/projects retain
/// the core error plus all declaration origins. No partial result is returned.
///
/// # Panics
/// If a corpus file classified as authored syntax cannot provide its framed input,
/// violating the parser's internal classification invariant.
pub fn resolve<'a, 's>(
    compilation: &'a Compilation,
    corpus: &'a ParsedCorpus<'s>,
    limits: content::Limits,
) -> Result<ResolvedAuthored<'a, 's>, Error> {
    resolve_with_asset_metadata(compilation, corpus, BTreeMap::new(), limits)
}

/// Resolve editor structure using observed asset paths and sizes alongside captured text.
/// Observations have no content digest and cannot enter publication selection or rendering.
/// Capture the complete source set and use [`resolve`] before building output.
///
/// # Errors
/// Rejects overlapping, unbound or non-opaque observations and exhausted metadata limits,
/// followed by the same structural checks as captured input.
///
/// # Panics
/// If a corpus file classified as authored syntax cannot provide its framed input,
/// violating the parser's internal classification invariant.
pub fn resolve_with_asset_metadata<'a, 's>(
    compilation: &'a Compilation,
    corpus: &'a ParsedCorpus<'s>,
    asset_metadata: BTreeMap<EntryPath, u64>,
    limits: content::Limits,
) -> Result<ResolvedAuthored<'a, 's>, Error> {
    asset_metadata::validate(compilation, corpus, &asset_metadata, limits)?;
    retained_only::validate(compilation, corpus, &asset_metadata, limits.nodes)?;
    let mut declaration = Declaration {
        browsing: vec![],
        nodes: vec![],
        memberships: vec![],
        related: vec![],
        projects: vec![],
    };
    let mut locations: BTreeMap<Key, Vec<Origin>> = BTreeMap::new();
    let mut project_facts = BTreeMap::new();
    let mut errors = Vec::new();
    let mut derivations = BTreeMap::new();
    let mut resources = BTreeMap::new();
    let mut listing_artwork = BTreeMap::new();
    let mut image_declarations = BTreeMap::new();
    let mut notebooks = BTreeMap::new();
    let mut notebook_downloads: BTreeMap<Key, BTreeSet<EntryPath>> = BTreeMap::new();
    let mut templates = BTreeMap::new();
    let mut unframed = Vec::new();
    let mut resource_count = 0usize;
    let mut resource_bytes =
        gazette_core::render::LimitedWriter::new(std::io::sink(), limits.text_bytes);
    if compilation
        .workspace
        .legal_sources()
        .take(limits.nodes.saturating_add(1))
        .count()
        > limits.nodes
    {
        return Err(Error::Structure {
            error: content::Error::Limit("legal output nodes"),
            declarations: locations,
        });
    }
    for source in compilation.workspace.legal_sources() {
        let Some(binding) = source.legal_binding() else {
            return Err(Error::Structure {
                error: content::Error::Invalid(vec![content::Diagnostic {
                    code: "content_legal_source_binding",
                    source: source.site.clone(),
                    target: Some(source.source_key().clone()),
                }]),
                declarations: locations,
            });
        };
        let path = source
            .logical_path(&binding.entry)
            .map_err(|_| Error::LegalInput {
                path: binding.entry.clone(),
                reason: "invalid selected legal coordinate",
            })?;
        if !matches!(corpus.files().get(&path), Some(File::Document(_))) {
            return Err(Error::LegalInput {
                path,
                reason: "selected legal source requires one retained framed document",
            });
        }
    }
    for (path, file) in corpus.files() {
        let mut selected = compilation.workspace.legal_bindings(path).peekable();
        if selected.peek().is_some() {
            let File::Document(document) = file else {
                return Err(Error::LegalInput {
                    path: path.clone(),
                    reason: "selected legal source is not a framed document",
                });
            };
            let metadata = match legal::read(document) {
                Ok(metadata) => metadata,
                Err(error) => {
                    let span = match &error {
                        LegalSourceError::Metadata { span, .. }
                        | LegalSourceError::Identity { span, .. } => *span,
                    };
                    errors.push(FileFailure {
                        origin: origin(path, document, span),
                        error: FileError::Legal(error),
                    });
                    continue;
                }
            };
            for source in selected {
                if declaration.nodes.len() >= limits.nodes {
                    return Err(Error::Structure {
                        error: content::Error::Limit("legal output nodes"),
                        declarations: locations,
                    });
                }
                let Some(binding) = source.legal_binding() else {
                    return Err(Error::LegalInput {
                        path: path.clone(),
                        reason: "selected legal output has no legal source binding",
                    });
                };
                // Account for repeated output metadata and body views before cloning.
                std::io::Write::write_all(&mut resource_bytes, document.source().as_bytes())
                    .and_then(|()| {
                        std::io::Write::write_all(
                            &mut resource_bytes,
                            binding.id.as_str().as_bytes(),
                        )
                    })
                    .and_then(|()| {
                        std::io::Write::write_all(
                            &mut resource_bytes,
                            source.site.as_str().as_bytes(),
                        )
                    })
                    .and_then(|()| {
                        std::io::Write::write_all(
                            &mut resource_bytes,
                            binding.route.as_str().as_bytes(),
                        )
                    })
                    .and_then(|()| {
                        binding.aliases.iter().try_for_each(|alias| {
                            std::io::Write::write_all(
                                &mut resource_bytes,
                                alias.as_str().as_bytes(),
                            )
                        })
                    })
                    .map_err(|_| Error::Structure {
                        error: content::Error::Limit("legal output bytes"),
                        declarations: locations.clone(),
                    })?;
                match legal::compose(document, source.site, binding, &metadata) {
                    Ok(lowered) => {
                        locations
                            .entry(lowered.content.id.clone())
                            .or_default()
                            .push(origin(path, document, None));
                        declaration.nodes.push(lowered.content);
                    }
                    Err(error) => {
                        let span = match &error {
                            LegalSourceError::Metadata { span, .. }
                            | LegalSourceError::Identity { span, .. } => *span,
                        };
                        errors.push(FileFailure {
                            origin: origin(path, document, span),
                            error: FileError::Legal(error),
                        });
                    }
                }
            }
            continue;
        }
        if path.as_str().starts_with("__legal/") {
            // Only selected owner entries participate; retained siblings are not pages.
            continue;
        }
        match file {
            File::Asset(_) | File::Notebook(_) => (),
            File::Markdown(body) => {
                unframed.push((path, body));
            }
            File::Document(_) | File::Landing(_) => {
                let input = input::Input::from_file(file).expect("authored syntax arm");
                let authored_origin = |span| Origin {
                    path: path.clone(),
                    source_sha256: file.source_sha256(),
                    span,
                };
                match input.read() {
                    Err(error) => {
                        errors.push(FileFailure {
                            origin: authored_origin(input.error_span(&error)),
                            error,
                        });
                    }
                    Ok(mut metadata) => {
                        let Some(site) = metadata.home_site(compilation, path) else {
                            errors.push(FileFailure {
                                origin: authored_origin(Some(input.declaration_span())),
                                error: FileError::SiteBindingRequired,
                            });
                            continue;
                        };
                        let checked = (|| {
                            for image in &metadata.images {
                                resource_count = resource_count
                                    .checked_add(image.len())
                                    .filter(|&count| count <= limits.nodes)
                                    .ok_or(gazette_core::resource::Error::Limit(
                                        "body image declarations",
                                    ))?;
                                serde_json::to_writer(&mut resource_bytes, image).map_err(
                                    |_| {
                                        gazette_core::resource::Error::Limit(
                                            "body image declaration bytes",
                                        )
                                    },
                                )?;
                            }
                            if let Some(artwork) = &metadata.listing_artwork {
                                resource_count = resource_count
                                    .checked_add(artwork.len())
                                    .filter(|&count| count <= limits.nodes)
                                    .ok_or(gazette_core::resource::Error::Limit(
                                        "authored artwork rendition declarations",
                                    ))?;
                                serde_json::to_writer(&mut resource_bytes, artwork).map_err(
                                    |_| {
                                        gazette_core::resource::Error::Limit(
                                            "authored artwork declaration bytes",
                                        )
                                    },
                                )?;
                            }
                            if let Some(template) = &metadata.template {
                                serde_json::to_writer(&mut resource_bytes, template).map_err(
                                    |_| {
                                        gazette_core::resource::Error::Limit(
                                            "page presentation declaration bytes",
                                        )
                                    },
                                )?;
                            }
                            resource_count = resource_count
                                .checked_add(metadata.notebooks.len())
                                .filter(|&count| count <= limits.nodes)
                                .ok_or(gazette_core::resource::Error::Limit(
                                    "page supplementary declarations",
                                ))?;
                            if !metadata.notebooks.is_empty() {
                                serde_json::to_writer(&mut resource_bytes, &metadata.notebooks)
                                    .map_err(|_| {
                                        gazette_core::resource::Error::Limit(
                                            "page supplementary declaration bytes",
                                        )
                                    })?;
                            }
                            if !metadata.resources.is_empty() {
                                resource_count = resource_count
                                    .checked_add(metadata.resources.len())
                                    .filter(|&count| count <= limits.nodes)
                                    .ok_or(gazette_core::resource::Error::Limit(
                                        "page resource declarations",
                                    ))?;
                                serde_json::to_writer(&mut resource_bytes, &metadata.resources)
                                    .map_err(|_| {
                                        gazette_core::resource::Error::Limit(
                                            "page resource declaration bytes",
                                        )
                                    })?;
                                resources::validate(&mut metadata.resources)?;
                            }
                            Ok::<_, gazette_core::resource::Error>(())
                        })();
                        if let Err(error) = checked {
                            errors.push(FileFailure {
                                origin: authored_origin(Some(input.declaration_span())),
                                error: FileError::Resource(error),
                            });
                            continue;
                        }
                        let id = &metadata.content.id;
                        if let Some(project) = metadata.project {
                            project_facts.insert(id.clone(), project);
                        }
                        if !metadata.images.is_empty() {
                            image_declarations.insert(id.clone(), metadata.images);
                        }
                        if let Some(artwork) = metadata.listing_artwork {
                            listing_artwork.insert(id.clone(), artwork);
                        }
                        if let Some(template) = metadata.template {
                            templates.insert(id.clone(), template);
                        }
                        if !metadata.resources.is_empty() {
                            resources.insert(id.clone(), metadata.resources);
                        }
                        if !metadata.notebooks.is_empty() {
                            let downloads: BTreeSet<EntryPath> = metadata
                                .notebooks
                                .iter()
                                .filter(|declaration| declaration.download())
                                .map(|declaration| declaration.path().clone())
                                .collect();
                            if !downloads.is_empty() {
                                notebook_downloads.insert(id.clone(), downloads);
                            }
                            notebooks.insert(
                                id.clone(),
                                metadata
                                    .notebooks
                                    .into_iter()
                                    .map(|declaration| declaration.path().clone())
                                    .collect(),
                            );
                        }
                        locations
                            .entry(id.clone())
                            .or_default()
                            .push(authored_origin(Some(input.declaration_span())));
                        declaration
                            .memberships
                            .extend(metadata.collections.into_iter().map(|collection| {
                                CollectionMembership {
                                    content: id.clone(),
                                    collection,
                                }
                            }));
                        declaration
                            .related
                            .extend(
                                metadata
                                    .related
                                    .into_iter()
                                    .map(|target| EditorialRelation {
                                        source: id.clone(),
                                        target,
                                    }),
                            );
                        declaration
                            .projects
                            .extend(metadata.projects.into_iter().map(|project| {
                                ProjectDeclaration {
                                    content: id.clone(),
                                    project: project.project,
                                    kind: project.kind,
                                }
                            }));
                        declaration
                            .browsing
                            .extend(metadata.browsing.into_iter().map(|index| BrowsingRelation {
                                source: id.clone(),
                                index,
                            }));
                        declaration.nodes.push(metadata.content.with_site(site));
                    }
                }
            }
        }
    }
    // A plain notes companion is subordinate only to an explicitly declared article.
    // Delay native-document lowering so traversal order cannot manufacture a page
    // identity for it. Other unframed documents keep their existing native policy.
    let owned_notes: BTreeSet<_> = declaration
        .nodes
        .iter()
        .flat_map(|node| {
            locations
                .get(&node.id)
                .into_iter()
                .flatten()
                .filter_map(|origin| {
                    let path = page_packages::notes_path(node, &origin.path)?;
                    let owner = compilation.workspace.content_source(&origin.path)?;
                    let companion = compilation.workspace.content_source(&path)?;
                    (owner.tree_binding() == companion.tree_binding()).then_some(path)
                })
        })
        .collect();
    for (path, body) in unframed {
        if owned_notes.contains(path) {
            continue;
        }
        let origin = Origin {
            path: path.clone(),
            source_sha256: body.source_sha256(),
            span: None,
        };
        match native::lower(compilation, path, body) {
            Ok((metadata, derivation)) => {
                locations
                    .entry(metadata.content.id.clone())
                    .or_default()
                    .push(origin);
                derivations.insert(metadata.content.id.clone(), derivation);
                declaration.nodes.push(metadata.content);
            }
            Err(error) => errors.push(FileFailure { origin, error }),
        }
    }
    if !errors.is_empty() {
        errors.sort_by(|left, right| left.origin.path.cmp(&right.origin.path));
        return Err(Error::Files(errors));
    }
    let owner_errors: Vec<_> = declaration
        .nodes
        .iter()
        .filter(|node| project_facts.contains_key(&node.id))
        .filter_map(|node| {
            let owners = locations.get(&node.id)?;
            let valid = owners.iter().all(|origin| {
                compilation
                    .workspace
                    .content_source(&origin.path)
                    .is_some_and(|source| source.site == &node.site)
            });
            (!valid).then(|| content::Diagnostic {
                code: "project_landing_source_owner",
                source: node.id.clone(),
                target: Some(node.site.clone()),
            })
        })
        .collect();
    if !owner_errors.is_empty() {
        return Err(Error::Structure {
            error: content::Error::Invalid(owner_errors),
            declarations: locations,
        });
    }
    let content =
        match compilation.resolve_content_with_projects(declaration, project_facts, limits) {
            Ok(content) => content,
            Err(error) => {
                return Err(Error::Structure {
                    error,
                    declarations: locations,
                });
            }
        };
    // Resolution proved identity uniqueness; never choose among conflicting origins.
    let paths = locations
        .into_iter()
        .flat_map(|(id, origins)| {
            origins
                .into_iter()
                .map(move |origin| (id.clone(), origin.path))
        })
        .collect();
    let package_resolution = page_packages::resolve(
        compilation,
        corpus,
        &asset_metadata,
        &content,
        &paths,
        &mut resources,
        &mut notebooks,
    )
    .map_err(Error::Files)?;
    retained_only::dependencies(compilation, &paths, &resources, &notebooks)?;
    let package_records = resource_count
        .checked_add(package_resolution.packages.len())
        .and_then(|count| {
            count.checked_add(
                package_resolution
                    .packages
                    .values()
                    .map(|package| package.companions().len())
                    .sum::<usize>(),
            )
        })
        .and_then(|count| count.checked_add(package_resolution.automatic_resources))
        .filter(|&count| count <= limits.nodes);
    let Some(package_records) = package_records else {
        let Some((owner, _)) = package_resolution.packages.first_key_value() else {
            return Err(Error::Structure {
                error: content::Error::Limit("page package companions"),
                declarations: BTreeMap::new(),
            });
        };
        let Some(path) = paths.get(owner) else {
            return Err(Error::Structure {
                error: content::Error::Limit("page package companions"),
                declarations: BTreeMap::new(),
            });
        };
        return Err(Error::Files(vec![FileFailure {
            origin: Origin {
                path: path.clone(),
                source_sha256: corpus.files()[path].source_sha256(),
                span: None,
            },
            error: FileError::Resource(gazette_core::resource::Error::Limit(
                "page package companions",
            )),
        }]));
    };
    debug_assert!(package_records <= limits.nodes);
    for (owner, package) in &package_resolution.packages {
        let Some(path) = paths.get(owner) else {
            return Err(Error::Structure {
                error: content::Error::Limit("page package companion bytes"),
                declarations: BTreeMap::new(),
            });
        };
        serde_json::to_writer(&mut resource_bytes, package).map_err(|_| {
            Error::Files(vec![FileFailure {
                origin: Origin {
                    path: path.clone(),
                    source_sha256: corpus.files()[path].source_sha256(),
                    span: None,
                },
                error: FileError::Resource(gazette_core::resource::Error::Limit(
                    "page package companion bytes",
                )),
            }])
        })?;
    }
    let resolved = ResolvedAuthored {
        asset_metadata,
        selected_assets: None,
        assets: compilation
            .workspace
            .sites()
            .map(|site| (site.id.clone(), OnceLock::new()))
            .collect(),
        projection_limits: limits,
        preview_link_bases: None,
        recommendation_index: OnceLock::new(),
        content,
        corpus,
        paths,
        derivations,
        resources,
        listing_artwork,
        image_declarations,
        notebooks,
        notebook_downloads,
        page_packages: package_resolution.packages,
        templates,
    };
    resolved.validate_body_images().map_err(Error::Files)?;
    resolved
        .validate_listing_artwork()
        .map_err(Error::Artwork)?;
    for id in resolved.notebooks.keys() {
        resolved.notebooks(id).map_err(Error::Notebook)?;
    }
    resolved
        .validate_publication_metadata()
        .map_err(Error::PublicationMetadata)?;
    Ok(resolved)
}

fn origin(path: &EntryPath, document: &ParsedDocument<'_>, span: Option<Span>) -> Origin {
    Origin {
        path: path.clone(),
        source_sha256: document.source_sha256(),
        span,
    }
}
