//! Pure workspace reference resolution before source admission or native effects.
//!
//! This is an unreleased configuration proposal, not an admitted Website wire ABI.
//! All supplied fields are consumed or rejected. Resolved references establish
//! internal consistency only: owner evidence, source pins, content, Theme and
//! release gates are subsequent phases, never implied by this result.

use std::{collections::BTreeMap, fmt};

use serde::{Deserialize, Serialize};

pub use gazette_core::publication::{
    Availability, Key, ProjectKind, ProjectLifecycle, ProjectPlacementRole, Visibility,
};
use gazette_core::publication::{ExtensionSchema, Principal, Project, Taxonomy};
use gazette_core::source::{EntryPath, Sha256Digest};

use crate::toml::{self, ParsedToml};

pub(crate) mod authoring;
mod build;
pub use build::{Build, BuildOrigin, BuildSelection, LocalDrafts};
mod accounts;
mod brands;
pub use accounts::OnlineAccount;
mod output;
mod publication_defaults;
mod publication_origins;
mod site_declaration;
pub use output::PublicationOutput;
pub use publication_origins::{PublicationFieldOrigin, PublicationOriginQuery, PublicationScope};
mod deployment;
mod legal;
mod listings;
mod machine;
pub use deployment::{
    CrawlerRule, RobotsConfiguration, TermsConfiguration, WebManifestConfiguration,
};
mod runtime_commands;
mod social_card;
pub use authoring::{
    AuthoringError, AuthoringField, AuthoringFieldPresence, AuthoringHelp, AuthoringLiteral,
    AuthoringLiteralCandidate, AuthoringNode, AuthoringPathSegment, AuthoringStructureField,
    AuthoringValueKind, CompletionCandidate, CompletionKind, SchemaHover, StringOccurrence,
    WorkspaceAuthoring,
};
pub use legal::{LegalBinding, LegalFamily};
pub use listings::{
    ListingAlternateOrder, ListingDateField, ListingDeclaration, ListingDirection, ListingOrder,
    ListingPagination, ListingProjectResidence, ListingProjectScope, ListingRemaining,
    ListingSelection, ListingUndated, ListingVariant,
};
pub use machine::{GraphConfiguration, SearchConfiguration, SearchField, SearchSelection};
pub use runtime_commands::{PolicyOutputTransport, RuntimeArgument, RuntimeCommand};
pub use social_card::SocialCardRenderer;
mod composition;
mod content;
mod project_catalog;
mod project_declaration;
pub use project_declaration::{ProjectDeclaration, ProjectLandingSource};
mod discussion;
mod preferred_sources;
mod processing;
pub use preferred_sources::{
    PreferredSourceOverride, PreferredSourceView, PreferredSourcesProvider,
    PreferredSourcesSelection,
};
pub use processing::{
    AnalyticsProvider, AnalyticsSelection, DiscussionProvider, HostingProvider, HostingSelection,
    ProcessingDiscussion, PublicationProcessing, ServiceSelection, SubscriptionProvider,
    WebMcpSelection,
};
mod documentation;
mod documentation_mounts;
mod extensions;
mod facts;
mod inspection;
mod navigation;
pub use inspection::WorkspaceInspection;
mod parsed;
pub use navigation::{
    DeclarationSymbol, DocumentDeclarationSymbol, DocumentSourceLocation,
    DocumentWorkspaceAssociation, DocumentWorkspaceReference, WorkspaceAssociation,
    WorkspaceDocumentsNavigation, WorkspaceNavigation, WorkspaceReference, WorkspaceSymbol,
    WorkspaceSymbolKind,
};
mod packages;
pub use packages::{
    PluginPackageBinding, ResourceInputMount, ResourceInputName, ThemePackageBinding,
};
mod derived_routes;
mod paths;
mod projects;
mod repositories;
mod resolve;
mod routes;
pub use derived_routes::PublicationRouting;
pub use routes::{ResolvedAlias, ResolvedMirror, ResolvedRoute};
mod feeds;
mod sources;
mod taxonomies;
pub use brands::{
    AlternateRelation, Brand, BrandPublication, DocumentationRepository, IdentityAssets,
    LlmsConfiguration, LlmsCoverage, LlmsRepresentation, LlmsSection, MachineOutputs,
    MarkdownAlternates, MarkdownDirectoryPage, MarkdownFilePage, MarkdownMediaType, PageCache,
    PublicationContacts, SearchProfile, SocialProfile, TaxonomyPage,
};
pub use composition::{
    CapabilitySelection, Composition, InvitationSelection, NavigationEntry, PluginSelection,
    ReadingAssistance, ReadingAssistanceProvider, Requirement, ResolvedChrome, ResolvedComposition,
    ResourceOwner, ResourceRef, ResourceSelection, ResourceUse, SearchControlCopy, Selected,
    SelectionField, SelectionOrigin, SelectionScope, SettingsControlCopy, ShellCopy, SiteChrome,
    ThemeSelection,
};
pub use content::ResolvedContent;
pub use documentation::{
    DocumentationBuild, DocumentationInput, DocumentationManual, DocumentationManualAsset,
    DocumentationManualPage, DocumentationModule, DocumentationNavigation,
};
pub use documentation_mounts::{
    DocumentationLowering, DocumentationMount, DocumentationMounts, ResolvedDocumentationMount,
};
pub use feeds::{
    FeedConfiguration, FeedFormat, FeedResidence, FeedScope, FeedSelection, FeedSelectionKind,
    Feeds,
};
pub use parsed::{
    CompositionFieldOrigin, CompositionOriginQuery, DeclarationKind, FieldOrigin, FieldQuery,
    OriginError, ParsedWorkspace,
};
mod documents;
pub use documents::{
    DocumentCompilation, DocumentCompositionFieldOrigin, DocumentEvidence, DocumentFieldOrigin,
    ParsedWorkspaceDocuments, RootSelection, compile_documents, included_documents,
    inspect_documents, root_selection,
};
pub use sources::{
    ContentBinding, ContentSource, PolicySource, Source, SourceLocation, SourceRoot, StaticAssets,
};

/// Site identity and an independently declared publisher role.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Site {
    /// Stable site key; display domains and routes are separate configuration.
    pub id: Key,
    /// Required principal reference, with no author/controller/brand fallback.
    pub publisher: Key,
    /// Independently selected site rights holder; never a page author or licence fallback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rights_holder: Option<Key>,
    /// Author credited on published pages that have no attribution of their own.
    /// Draft and archived pages receive no default attribution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_author: Option<Key>,
    /// Explicit site responsibilities; no publisher, workspace or sibling inheritance.
    /// Empty means undeclared. Multiple principals may hold a role without becoming one identity.
    #[serde(default)]
    pub roles: Vec<gazette_core::publication::PublicationRoleBinding>,
    /// Site-owned discussion settings, independent of workspace composition inheritance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discussion: Option<gazette_core::publication::Discussion>,
    /// Whole-site release eligibility selection; never authorization for an effect.
    pub deployable: bool,
    /// Site-owned selection atoms; omitted atoms inherit workspace defaults.
    #[serde(default)]
    pub composition: Composition,
}

/// Declarative placement controls. None is proof of source admission or release permission.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Placement {
    /// Exact declared site reference.
    pub site: Key,
    /// Exact declared project reference.
    pub project: Key,
    /// Canonical publication role; every placed project has exactly one primary.
    pub role: ProjectPlacementRole,
    /// Select materialization; source/policy gates remain mandatory.
    pub render: bool,
    /// Select discovery surfaces, independently of materialization.
    pub discoverable: bool,
    /// Explicit documentation binding, required when rendering is selected.
    pub documents: Option<ContentBinding>,
    /// Explicit lowering defaults for unframed native Markdown only.
    /// Framed authored metadata is never overwritten or retried through this profile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_markdown: Option<NativeMarkdown>,
}

/// Instance-selected defaults for native documents without publication frontmatter.
/// They establish local content structure, not rights, authorship or release approval.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NativeMarkdown {
    /// Explicit directory route beneath the site's configured public base.
    pub route_prefix: gazette_core::publication::routing::RoutePath,
    /// Explicit default state; omission is not silently converted into published or draft.
    pub lifecycle: gazette_core::publication::ContentLifecycle,
    /// Explicit language default for unframed native documents only; no site inference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<gazette_core::publication::LanguageTag>,
}

/// Exact retained resource tree and interpreter used for mathematical typesetting.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MathTypesetter {
    /// Pinned interpreter/native executable; resource bytes are bound separately.
    #[serde(flatten)]
    pub command: RuntimeCommand,
    /// Existing exact source capture containing the renderer and all local imports.
    pub source: Key,
    /// Capture-relative renderer entrypoint passed as the first native argument.
    pub entrypoint: gazette_core::source::EntryPath,
}

/// Workspace-selected Graphviz executable. Gazette owns its renderer arguments,
/// environment and disposable working directory; configuration cannot inject them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GraphvizRenderer {
    /// Exact absolute native executable and mandatory byte pin.
    #[serde(flatten)]
    pub executable: gazette_core::process::PinnedExecutable,
}

#[derive(Default, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Declaration {
    /// Shared publication settings with explicit per-publisher and per-site overrides.
    #[serde(default)]
    defaults: Option<publication_defaults::Defaults>,
    /// Settings inherited only through a site's declared publisher relationship.
    #[serde(default)]
    publishers: Vec<publication_defaults::PublisherDefaults>,
    /// Exact additional Gazette configuration documents selected by the root.
    #[serde(default)]
    includes: Vec<EntryPath>,
    /// Exact machine binding selected by the root document, separate from publication configuration.
    #[serde(default)]
    machine_binding: Option<EntryPath>,
    /// Explicit workspace defaults for local build selection, not invocation permission.
    #[serde(default, deserialize_with = "crate::tables::optional")]
    build: Option<Build>,
    /// Optional pinned Graphviz renderer command.
    #[serde(default, deserialize_with = "crate::tables::optional")]
    dot_renderer: Option<GraphvizRenderer>,
    /// Optional pinned Node and browser for engine-owned social-card capture.
    #[serde(default, deserialize_with = "crate::tables::optional")]
    social_card_renderer: Option<SocialCardRenderer>,
    /// Optional pinned mathematical typesetter command.
    #[serde(default, deserialize_with = "crate::tables::optional")]
    math_typesetter: Option<MathTypesetter>,
    /// Explicit fact-provider selections.
    #[serde(default)]
    fact_sources: Vec<gazette_core::publication::facts::Selection>,
    /// Optional workspace-owned publication-policy checker.
    #[serde(default)]
    policy_check: Option<RuntimeCommand>,
    /// Explicit output transport; absence preserves the inline JSON protocol.
    #[serde(default)]
    policy_output_transport: Option<PolicyOutputTransport>,
    /// Optional retained input for the selected policy checker.
    #[serde(default, deserialize_with = "crate::tables::optional")]
    policy_source: Option<PolicySource>,
    /// Workspace-default Theme and capability composition.
    #[serde(default)]
    composition: Composition,
    /// Declared publication brands.
    #[serde(default)]
    brands: Vec<Brand>,
    /// Public accounts, with ownership declared independently from site display.
    #[serde(default)]
    accounts: Vec<accounts::AccountDeclaration>,
    /// Declared people and organizations.
    #[serde(default)]
    principals: Vec<Principal>,
    /// Publication sites; at least one valid site is required by resolution.
    #[serde(default)]
    sites: Vec<site_declaration::SiteDeclaration>,
    /// Declared projects.
    #[serde(default)]
    projects: Vec<ProjectDeclaration>,
    /// Declared taxonomy catalogs.
    #[serde(default)]
    taxonomies: Vec<Taxonomy>,
    /// Workspace-selected extension schemas.
    #[serde(default)]
    extensions: Vec<ExtensionSchema>,
    /// Explicit source bindings.
    #[serde(default)]
    sources: Vec<Source>,
    /// Named shared resource inputs bound to existing sources, without acquisition or generation.
    #[serde(default)]
    design_inputs: BTreeMap<ResourceInputName, Key>,
    /// Captured Theme packages, indexed by their manifest-declared identity.
    #[serde(default)]
    theme_packages: Vec<ThemePackageBinding>,
    /// Captured plugin packages, indexed by their manifest-declared identity.
    #[serde(default)]
    plugin_packages: Vec<PluginPackageBinding>,
    /// Site-to-project publication placements.
    #[serde(default)]
    placements: Vec<Placement>,
    /// Theme documentation mounts, distinct from generated-reference repositories.
    #[serde(default, deserialize_with = "crate::tables::one")]
    documentation: DocumentationMounts,
}

pub(crate) fn declaration_schema() -> schemars::Schema {
    schemars::generate::SchemaSettings::draft2020_12()
        .for_deserialize()
        .into_generator()
        .into_root_schema_for::<Declaration>()
}

/// Complete resolved topology, sorted by stable keys and immutable after construction.
///
/// There is deliberately no deserializer or public constructor. Only the compiler
/// can construct this result after every namespace and reference check succeeds.
#[derive(Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct Workspace {
    #[serde(skip_serializing_if = "Option::is_none")]
    machine_binding: Option<EntryPath>,
    #[serde(skip_serializing_if = "Option::is_none")]
    build: Option<Build>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dot_renderer: Option<GraphvizRenderer>,
    #[serde(skip_serializing_if = "Option::is_none")]
    social_card_renderer: Option<SocialCardRenderer>,
    #[serde(skip_serializing_if = "Option::is_none")]
    math_typesetter: Option<MathTypesetter>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    fact_sources: Vec<gazette_core::publication::facts::Selection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    policy_check: Option<RuntimeCommand>,
    #[serde(skip_serializing_if = "Option::is_none")]
    policy_output_transport: Option<PolicyOutputTransport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    policy_source: Option<PolicySource>,
    composition: Composition,
    site_compositions: BTreeMap<Key, ResolvedComposition>,
    brands: BTreeMap<Key, Brand>,
    principals: BTreeMap<Key, Principal>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    accounts: BTreeMap<Key, OnlineAccount>,
    sites: BTreeMap<Key, Site>,
    projects: BTreeMap<Key, ProjectDeclaration>,
    taxonomies: BTreeMap<Key, Taxonomy>,
    extensions: extensions::Extensions,
    sources: BTreeMap<Key, Source>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    design_inputs: BTreeMap<ResourceInputName, Key>,
    theme_packages: BTreeMap<crate::theme_package::ThemePackageId, ThemePackageBinding>,
    plugin_packages: BTreeMap<gazette_core::plugin::PluginPackageId, PluginPackageBinding>,
    placements: Vec<Placement>,
    documentation_mounts: Vec<ResolvedDocumentationMount>,
}

impl Workspace {
    /// Selected machine binding path; its native root values do not enter this model.
    pub fn machine_binding(&self) -> Option<&EntryPath> {
        self.machine_binding.as_ref()
    }

    /// Exact configured local build defaults. Absence leaves selection to the caller.
    pub fn build(&self) -> Option<&Build> {
        self.build.as_ref()
    }
    /// Explicit pinned DOT-to-PNG provider; selection does not permit execution or pin
    /// libraries, plugins, fonts or other ambient runtime inputs.
    pub fn dot_renderer(&self) -> Option<&GraphvizRenderer> {
        self.dot_renderer.as_ref()
    }
    /// Exact Node/browser pins for social-card capture, without caller arguments or environment.
    /// Selection alone never permits execution or creates an implicit source binding.
    pub fn social_card_renderer(&self) -> Option<&SocialCardRenderer> {
        self.social_card_renderer.as_ref()
    }
    /// Explicit native typesetter selection, independent of Markdown syntax and
    /// per-operation execution permission. The selected source and entrypoint bind the
    /// renderer and its local imports independently from the interpreter pin.
    pub fn math_typesetter(&self) -> Option<&MathTypesetter> {
        self.math_typesetter.as_ref()
    }

    /// Exact selected output framing, with legacy inline JSON as the default.
    pub fn policy_output_transport(&self) -> PolicyOutputTransport {
        self.policy_output_transport.unwrap_or_default()
    }

    /// Optional workspace-owned checker. Selection never grants permission to execute it,
    /// attests its executable bytes or makes its policy rules Engine-owned.
    pub fn policy_check(&self) -> Option<&RuntimeCommand> {
        self.policy_check.as_ref()
    }

    /// Explicit retained input for the selected checker, not policy approval or a Theme manifest.
    pub fn policy_source(&self) -> Option<&PolicySource> {
        self.policy_source.as_ref()
    }

    /// Effective desired composition; unknown sites never fall back to a default site.
    pub fn composition_for(&self, site: &Key) -> Option<&ResolvedComposition> {
        self.site_compositions.get(site)
    }
    /// Public accounts in stable key order, including accounts with unresolved ownership.
    pub fn accounts(&self) -> impl ExactSizeIterator<Item = &OnlineAccount> {
        self.accounts.values()
    }

    /// One exact account declaration with its resolved public address.
    pub fn account(&self, key: &Key) -> Option<&OnlineAccount> {
        self.accounts.get(key)
    }

    /// All brand declarations in key order; no principal role is inferred from a brand.
    pub fn brands(&self) -> impl ExactSizeIterator<Item = &Brand> {
        self.brands.values()
    }

    /// Resolve the documentation repository from its sole brand-owned settings.
    ///
    /// No binding means no configured target, not an inferred `domain_doc` folder.
    /// This does not select generators, inspect the path or authorize output writes.
    pub fn documentation_repository(&self, site: &Key) -> Option<&DocumentationRepository> {
        self.publication_for(site)
            .and_then(|publication| publication.documentation_repository.as_ref())
    }

    /// Exact brand-owned publication settings, without name/domain inference.
    pub fn publication_for(&self, site: &Key) -> Option<&BrandPublication> {
        self.brands
            .values()
            .flat_map(|brand| &brand.publications)
            .find(|publication| &publication.site == site)
    }

    /// Site delivery from its brand publication: an explicit override or its repository
    /// on main. A destination supplies coordinates, never deployability or release approval.
    pub fn destination(
        &self,
        site: &Key,
    ) -> Option<&gazette_core::release::PublicationDestination> {
        self.publication_for(site)
            .and_then(|publication| publication.destination.as_ref())
    }

    /// Sole configured public base for a site; no source or repository fallback.
    pub fn public_base_url(
        &self,
        site: &Key,
    ) -> Option<&gazette_core::publication::routing::PublicBaseUrl> {
        self.publication_for(site)
            .and_then(|publication| publication.public_base_url.as_ref())
    }

    /// All declared principal references in key order, including unused declarations.
    pub fn principals(&self) -> impl ExactSizeIterator<Item = &Principal> {
        self.principals.values()
    }

    /// Exact declared principal, without contact, name, site or project fallback.
    /// A successful lookup is structural binding, not identity or rights admission.
    pub fn principal(&self, key: &Key) -> Option<&Principal> {
        self.principals.get(key)
    }

    /// All declared sites in key order; one-site workspaces use the same model.
    pub fn sites(&self) -> impl ExactSizeIterator<Item = &Site> {
        self.sites.values()
    }

    /// Resolve an exact site key without falling back to a brand, principal or project.
    pub fn site(&self, key: &Key) -> Option<&Site> {
        self.sites.get(key)
    }

    /// Full configuration declarations only; landing-owned facts require content resolution.
    pub fn projects(&self) -> impl Iterator<Item = &Project> {
        self.projects
            .values()
            .filter_map(ProjectDeclaration::inline)
    }

    /// Full configuration facts only. Publication consumers use `ResolvedContent::project`.
    pub fn project(&self, key: &Key) -> Option<&Project> {
        self.projects.get(key).and_then(ProjectDeclaration::inline)
    }

    /// Every configured project identity and its selected fact owner, before acquisition.
    pub fn project_declarations(&self) -> impl ExactSizeIterator<Item = &ProjectDeclaration> {
        self.projects.values()
    }

    /// Exact configuration identity, including projects whose facts await their landing.
    pub fn project_declaration(&self, key: &Key) -> Option<&ProjectDeclaration> {
        self.projects.get(key)
    }

    /// Explicit taxonomy catalog in stable identity order, without language or URL inference.
    pub fn taxonomies(&self) -> impl ExactSizeIterator<Item = &Taxonomy> {
        self.taxonomies.values()
    }

    /// Resolve an exact taxonomy identity; labels and term names are not catalog keys.
    pub fn taxonomy(&self, key: &Key) -> Option<&Taxonomy> {
        self.taxonomies.get(key)
    }

    /// Workspace-selected data contracts in stable key order, not domain truth or approval.
    pub fn extensions(&self) -> impl Iterator<Item = &ExtensionSchema> {
        self.extensions
            .definitions()
            .map(|(_, definition)| definition)
    }

    /// Exact schema declaration used by this immutable workspace compilation.
    pub fn extension(&self, key: &Key) -> Option<&ExtensionSchema> {
        self.extensions.definition(key)
    }

    /// All explicit source bindings, including bindings not currently rendered.
    pub fn sources(&self) -> impl ExactSizeIterator<Item = &Source> {
        self.sources.values()
    }

    /// Resolve a source by its exact configuration key without native access.
    pub fn source(&self, key: &Key) -> Option<&Source> {
        self.sources.get(key)
    }

    /// All explicit shared resource input names and their configured source identities.
    /// This does not select native reads, token generation, public outputs or providers.
    pub fn design_inputs(&self) -> &BTreeMap<ResourceInputName, Key> {
        &self.design_inputs
    }

    /// Resolve one shared resource input to its exact source declaration without native reads.
    pub fn design_input(&self, name: &ResourceInputName) -> Option<&Source> {
        self.design_inputs
            .get(name)
            .and_then(|source| self.sources.get(source))
    }

    /// All captured Theme-package bindings in package-identity order.
    pub fn theme_packages(&self) -> impl ExactSizeIterator<Item = &ThemePackageBinding> {
        self.theme_packages.values()
    }

    /// Resolve one Theme package without inferring it from a source or palette.
    pub fn theme_package(
        &self,
        package: &crate::theme_package::ThemePackageId,
    ) -> Option<&ThemePackageBinding> {
        self.theme_packages.get(package)
    }

    /// All captured plugin-package bindings in package-identity order.
    pub fn plugin_packages(&self) -> impl ExactSizeIterator<Item = &PluginPackageBinding> {
        self.plugin_packages.values()
    }

    /// Resolve one plugin package without inferring it from a provider or source.
    pub fn plugin_package(
        &self,
        package: &gazette_core::plugin::PluginPackageId,
    ) -> Option<&PluginPackageBinding> {
        self.plugin_packages.get(package)
    }

    /// Explicit placements ordered by site then project; flags are never inferred.
    pub fn placements(&self) -> &[Placement] {
        &self.placements
    }
}

/// Source identity and resolved configuration, not an artifact or admission receipt.
#[derive(Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct Compilation {
    /// Exact single document or version-framed document-set bytes.
    pub source_sha256: Sha256Digest,
    /// Order-independent semantic topology. No partially resolved subset is returned.
    pub workspace: Workspace,
}

/// One deterministic semantic failure; locations refer to logical configuration fields.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, schemars::JsonSchema)]
pub struct Diagnostic {
    /// Stable failure class.
    pub code: &'static str,
    /// Logical field or namespace, not a host filesystem path.
    pub field: String,
    /// Exact key implicated in the failure, when applicable.
    pub key: Option<Key>,
}

/// Syntax/schema or complete-topology failure; no native state was opened or changed.
#[derive(Debug)]
pub enum Error {
    /// Derived compilation expansion exceeded its independent logical ceiling.
    Limit(&'static str),
    /// Original parser failure and source-local span.
    Toml(toml::Error),
    /// Complete explicitly selected TOML-set failure.
    TomlSet(toml::SetError),
    /// Independent semantic failures in stable order; no successful subset is retained.
    Invalid(Vec<Diagnostic>),
}

impl Error {
    /// Stable class for command and SDK consumers.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Limit(_) => "workspace_limit",
            Self::Toml(error) => error.code(),
            Self::TomlSet(error) => error.code(),
            Self::Invalid(_) => "workspace_invalid",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Limit(name) => write!(f, "workspace {name} ceiling exceeded"),
            Self::Toml(error) => error.fmt(f),
            Self::TomlSet(error) => error.fmt(f),
            Self::Invalid(errors) => {
                write!(f, "{} workspace constraints failed", errors.len())?;
                for error in errors {
                    write!(f, "\n  {} at {}", error.code, error.field)?;
                    if let Some(key) = &error.key {
                        write!(f, " ({key:?})")?;
                    }
                }
                f.write_str("; correct the declarations and check the complete workspace again")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Toml(error) => Some(error),
            Self::TomlSet(error) => Some(error),
            Self::Invalid(_) | Self::Limit(_) => None,
        }
    }
}

/// Resolve a retained TOML document without parsing it again or opening native state.
///
/// # Errors
/// Rejects missing/unknown fields, invalid state values, duplicate/conflicting keys,
/// empty site sets, and unresolved/wrong-kind references. Independent semantic
/// failures accumulate after successful schema decoding; schema failures retain
/// the original parser span and stop dependent resolution.
pub fn resolve(parsed: &ParsedToml<'_>) -> Result<Compilation, Error> {
    let declaration = publication_defaults::read(parsed)?;
    resolve_declaration(parsed, declaration, parsed.limits())
}

fn resolve_declaration(
    parsed: &ParsedToml<'_>,
    declaration: Declaration,
    limits: toml::Limits,
) -> Result<Compilation, Error> {
    if !declaration.includes.is_empty() {
        return Err(Error::Invalid(vec![Diagnostic {
            code: "workspace_includes_unresolved",
            field: "includes".into(),
            key: None,
        }]));
    }
    let workspace = resolve::workspace(declaration, limits)?;
    Ok(Compilation {
        source_sha256: Sha256Digest::of(parsed.source().as_bytes()),
        workspace,
    })
}

/// Parse and resolve one explicitly supplied complete workspace declaration.
///
/// The same path serves direct CLI, SDK and future editor projections. It does not
/// discover imports, inspect source repositories, reconcile a store, or certify content.
///
/// # Errors
/// Propagates parser resource/syntax failures and [`resolve`] failures without a
/// partial workspace. The parser's node/source limits also bound declarations and diagnostics.
pub fn compile(source: &str, limits: toml::Limits) -> Result<Compilation, Error> {
    resolve(&ParsedToml::parse(source, limits).map_err(Error::Toml)?)
}
