//! Brand-owned publication settings, separate from publisher identity and source placement.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use super::Key;
use gazette_core::resource::ResourceId;

/// A configured brand, not a legal entity, author or publisher by inference.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Brand {
    /// Stable brand key, independent from the site and principal namespaces.
    pub id: Key,
    /// Explicit display name; never an identity join or a repository-name default.
    pub name: String,
    /// Public description of the brand’s role, distinct from the publication description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Explicit brand parent only; this supplies no legal role or author defaults.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<Key>,
    /// Principal who owns this brand. This is independent of the site's publisher.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<Key>,
    /// Public identity for a brand without its own routed home.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<String>,
    /// The work or programme this brand represents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<Key>,
    /// Projects grouped under this brand in its portfolio.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projects: Vec<Key>,
    /// Publication settings owned here, rather than copied into each adapter.
    #[serde(default)]
    pub publications: Vec<BrandPublication>,
}

/// One site's configured brand publication; this association transfers no legal authority.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BrandPublication {
    /// Intended processing selection, consumed by legal context without activating services.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::tables::optional"
    )]
    pub processing: Option<super::PublicationProcessing>,
    /// Exact declared site reference; one settings binding per site.
    pub site: Key,
    /// Exact same-site routed home content, never a guessed root or navigation entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home: Option<Key>,
    /// Exact About page whose primary subject is the selected brand.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub about: Option<Key>,
    /// Optional address rules for content with an explicit readable slug.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::tables::optional"
    )]
    pub routing: Option<super::PublicationRouting>,
    /// Exact same-site canonical search page, independent from shared chrome copy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search_page: Option<Key>,
    /// Publication language, inherited by content that omits its own language.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<gazette_core::publication::LanguageTag>,
    /// Distinct public correspondence interfaces; no principal or mailbox activation is inferred.
    #[serde(default, deserialize_with = "crate::tables::one")]
    pub contacts: PublicationContacts,
    /// Selected public account IDs; the complete list replaces an inherited list.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub accounts: Vec<Key>,
    /// Explicit platform-keyed public profile references, not integrations or approved `sameAs`.
    #[serde(default)]
    pub social: BTreeMap<String, SocialProfile>,
    /// Public base and optional deployment prefix, independent of names/local paths.
    /// Absence is valid for local inspection; routed content requires this binding.
    pub public_base_url: Option<gazette_core::publication::routing::PublicBaseUrl>,
    /// Explicit local addressing coordinate; declaration never starts a listener.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_port: Option<std::num::NonZeroU16>,
    /// Private local output destination; compilation does not read or create it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::tables::optional"
    )]
    pub output: Option<super::PublicationOutput>,
    /// Public repository for this site's generated output. It supplies the default Git
    /// destination on main when no explicit delivery override is present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    /// Optional delivery override; otherwise repository supplies the remote and main branch.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::tables::optional"
    )]
    pub destination: Option<gazette_core::release::PublicationDestination>,
    /// Explicit authored sources and their workspace-wide logical corpus prefixes.
    /// This is not a project placement, route, generated reference or permission.
    /// Declaration order is retained; no additional native roots are discovered.
    #[serde(
        default,
        deserialize_with = "crate::tables::many",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub authored: Vec<super::ContentBinding>,
    /// Explicit owner-held legal entries and independently identified Utility outputs.
    /// Reuse confers no controller, provider, authorship or publication approval.
    #[serde(
        default,
        deserialize_with = "crate::tables::many",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub legal: Vec<super::LegalBinding>,
    /// Optional product-manual and generated-reference repository, separate from
    /// Theme-rendered authored publication content.
    #[serde(default, deserialize_with = "crate::tables::optional")]
    pub documentation_repository: Option<DocumentationRepository>,
    /// Site-root discovery files, derived from the same resolved pages as HTML.
    /// Selection grants no public admission or deployment permission.
    #[serde(default, deserialize_with = "crate::tables::one")]
    pub machine: MachineOutputs,
    /// Semantic roles for already selected site resources.
    ///
    /// These references do not select, compile or approve a resource. They let
    /// Theme consume exact identity assets without deriving filenames or paths.
    #[serde(default, deserialize_with = "crate::tables::one")]
    pub identity_assets: IdentityAssets,
    /// Explicit taxonomy listing routes and selected Theme templates; no inferred slugs.
    #[serde(default, deserialize_with = "crate::tables::many")]
    pub taxonomy_pages: Vec<TaxonomyPage>,
    /// Explicit generated reading views over existing declaration-only index identities.
    #[serde(
        default,
        deserialize_with = "crate::tables::many",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub listings: Vec<super::ListingDeclaration>,
}

/// Role-specific public contact addresses, without inferred identity or legal responsibility.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct PublicationContacts {
    /// Public correspondence address; configuration does not verify a working mailbox.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correspondence: Option<String>,
    /// Explicit rights-enquiry address, not an inferred rights holder.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rights: Option<String>,
    /// Explicit discovery/contact interface, independent from correspondence and rights.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discovery: Option<String>,
}

/// One named public profile reference; platform naming activates no provider behavior.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SocialProfile {
    /// Public HTTP(S) profile address, explicit inline or resolved by a selected account.
    pub url: String,
    /// Explicit observed handle, retained independently from the address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle: Option<String>,
    /// Explicit channel identifier when supplied; not a provider credential or identity join.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_id: Option<String>,
}

pub(super) fn validate(brands: &BTreeMap<Key, Brand>, errors: &mut Vec<super::Diagnostic>) {
    use super::resolve::report;
    // Conflicting declarations must not choose a parent or metadata winner.
    if errors
        .iter()
        .any(|error| error.code == "workspace_duplicate" && error.field == "brands")
    {
        return;
    }
    for brand in brands.values() {
        let parent_field = format!("brands[{:?}].parent", brand.id.as_str());
        if let Some(parent) = &brand.parent
            && !brands.contains_key(parent)
        {
            report(errors, "workspace_reference", &parent_field, Some(parent));
        }
        if brand
            .identity
            .as_ref()
            .is_some_and(|url| !super::repositories::valid(url))
        {
            report(
                errors,
                "workspace_brand_identity",
                "brands.identity",
                Some(&brand.id),
            );
        }
        for publication in &brand.publications {
            let field = format!(
                "brands[{:?}].publications[{:?}]",
                brand.id.as_str(),
                publication.site.as_str()
            );
            if let Some(routing) = &publication.routing {
                for (name, prefix) in routing.prefixes() {
                    if prefix.is_some_and(|path| !path.as_str().ends_with('/')) {
                        report(
                            errors,
                            "workspace_routing_directory",
                            &format!("{field}.routing.{name}"),
                            Some(&publication.site),
                        );
                    }
                }
            }
            for (role, address) in [
                ("correspondence", &publication.contacts.correspondence),
                ("rights", &publication.contacts.rights),
                ("discovery", &publication.contacts.discovery),
            ] {
                if address.as_ref().is_some_and(|value| {
                    !plain(value)
                        || value.chars().any(char::is_whitespace)
                        || !value.split_once('@').is_some_and(|(local, domain)| {
                            !local.is_empty() && !domain.is_empty() && !domain.contains('@')
                        })
                }) {
                    report(
                        errors,
                        "workspace_publication_contact",
                        &format!("{field}.contacts.{role}"),
                        Some(&publication.site),
                    );
                }
            }
            for (platform, profile) in &publication.social {
                if platform.is_empty()
                    || !platform
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
                    || !super::repositories::valid(&profile.url)
                    || [&profile.handle, &profile.channel_id]
                        .into_iter()
                        .flatten()
                        .any(|value| !plain(value) || value.chars().any(char::is_whitespace))
                {
                    report(
                        errors,
                        "workspace_publication_profile",
                        &format!("{field}.social[{platform:?}]"),
                        Some(&publication.site),
                    );
                }
            }
        }
    }
    // One explicit parent per brand permits a bounded linear walk with shared completed paths.
    let mut complete = BTreeSet::new();
    for start in brands.keys() {
        let mut path: Vec<&Key> = Vec::new();
        let mut positions: BTreeMap<&Key, usize> = BTreeMap::new();
        let mut cursor = Some(start);
        while let Some(id) = cursor {
            if complete.contains(id) {
                break;
            }
            if let Some(&position) = positions.get(id) {
                for id in &path[position..] {
                    report(
                        errors,
                        "workspace_brand_parent_cycle",
                        &format!("brands[{:?}].parent", id.as_str()),
                        Some(id),
                    );
                }
                break;
            }
            positions.insert(id, path.len());
            path.push(id);
            cursor = brands.get(id).and_then(|brand| brand.parent.as_ref());
        }
        complete.extend(path);
    }
}

fn plain(value: &str) -> bool {
    !value.trim().is_empty() && !value.chars().any(char::is_control)
}

/// One generated catalog listing, not an authored content record or an approval.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TaxonomyPage {
    /// Exact workspace taxonomy identity.
    pub taxonomy: Key,
    /// Exact term within that taxonomy; omission selects its used-term index.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub term: Option<Key>,
    /// Explicit site-relative canonical route, independent of catalog labels.
    pub route: gazette_core::publication::routing::RoutePath,
    /// Exact template entry within the selected retained Theme.
    pub template: gazette_core::source::EntryPath,
}

/// Standard discovery-file selections beneath the site's configured public base.
/// Omission disables outputs; no provider, title or path is inferred.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct MachineOutputs {
    /// Explicit crawler policy; omission emits no robots.txt.
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::tables::optional"
    )]
    pub robots: Option<super::RobotsConfiguration>,
    /// Site-relative terms of use for TDM reservation and JSON-LD `usageInfo`.
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::tables::optional"
    )]
    pub terms: Option<super::TermsConfiguration>,
    /// Browser manifest derived from this publication and its selected identity icon.
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::tables::optional"
    )]
    pub web_manifest: Option<super::WebManifestConfiguration>,
    /// Emit `sitemap.xml`, partitioning the same canonical URLs when required.
    pub sitemap: bool,
    /// Optional packaging ceiling, 1 through 50,000 URLs per file; requires sitemap selection.
    /// Omission uses the protocol ceiling. This never filters the selected page set.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 50_000))]
    pub sitemap_urls_per_file: Option<usize>,
    /// Explicit alternate representation of every rendered canonical page.
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::tables::optional"
    )]
    pub markdown_alternates: Option<MarkdownAlternates>,
    /// Curated reading entrypoints, not an exhaustive second sitemap.
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::tables::optional"
    )]
    pub llms: Option<LlmsConfiguration>,
    /// Emit `search.json` and expose the explicitly selected browser profile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search: Option<super::SearchConfiguration>,
    /// Explicit graph-view output over the same discovery-safe resolved graph.
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::tables::optional"
    )]
    pub graph: Option<super::GraphConfiguration>,
    /// Supply each page's JSON-LD and emit the same values as `site.jsonld`.
    pub json_ld: bool,
    /// Explicit syndication channels and their stable identity namespace.
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::tables::optional"
    )]
    pub feeds: Option<super::Feeds>,
}

/// One closed Markdown alternate routing and discovery profile.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MarkdownAlternates {
    /// Explicit selection; declaring a profile does not admit publication.
    pub enabled: bool,
    /// Exact directory-page spelling selected by this profile.
    pub directory_page: MarkdownDirectoryPage,
    /// Exact file-page mapping selected by this profile.
    pub file_page: MarkdownFilePage,
    /// The representation's actual producer media type.
    pub media_type: MarkdownMediaType,
    /// Human-page discovery relationship.
    pub discovery_rel: AlternateRelation,
}

/// Supported directory-page representation, never an arbitrary output selector.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub enum MarkdownDirectoryPage {
    /// Place the alternate in the canonical directory.
    #[serde(rename = "index.md")]
    IndexMd,
}

/// Supported file-page representation, preserving the existing extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum MarkdownFilePage {
    /// Append `.md` to the existing serving route.
    AppendMd,
}

/// Closed native Markdown media profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub enum MarkdownMediaType {
    /// Native Markdown, not plain-text disclosure coverage.
    #[serde(rename = "text/markdown")]
    Markdown,
}

/// Supported alternate discovery relationship.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum AlternateRelation {
    /// An alternate representation of the same served page.
    Alternate,
}

/// Ordered owner-selected machine reading entrypoints.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LlmsConfiguration {
    /// Explicit selection, independent from sitemap and search.
    pub enabled: bool,
    /// Exact same-site discoverable content supplying its declared description.
    pub description_from: Key,
    /// Required representation of curated content references.
    pub representation: LlmsRepresentation,
    /// Pair each Markdown link with its actual serving HTML address.
    pub include_human_link: bool,
    /// Complete built pages or explicitly curated entrypoints.
    #[serde(default)]
    pub coverage: LlmsCoverage,
    /// Authored section and item order in curated mode.
    #[serde(default, deserialize_with = "crate::tables::many")]
    pub sections: Vec<LlmsSection>,
}

/// Publication-owned catalogue selection; descriptions remain authored metadata.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum LlmsCoverage {
    /// Preserve the explicit section and content order.
    #[default]
    Curated,
    /// Use the publication's configured navigation order.
    Navigation,
    /// Every eligible built page and generated reading view, in serving-URL order.
    AllPages,
}

/// Supported curated machine representation; no URL convention is guessed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum LlmsRepresentation {
    /// Use the configured Markdown alternate of the selected serving page.
    MarkdownAlternate,
}

/// One literal reading-path section, not a query or automatic grouping.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LlmsSection {
    /// Plain-text heading, escaped by the bounded writer.
    pub title: String,
    /// Exact content references in the declared order.
    pub contents: Vec<Key>,
}

/// Browser search behavior selected by the publication workspace.
///
/// Gazette emits the same neutral corpus for both profiles. This value selects
/// consumer behavior and is therefore carried to Theme instead of inferred by
/// the JavaScript SDK, a language tag or a domain.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SearchProfile {
    /// Unicode-lowercase lexical matching without language stemming.
    Lexical,
    /// Explicit English trimming, stop-word and stemming pipeline.
    English,
}

/// Publication-owned roles for resources already selected by site composition.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct IdentityAssets {
    /// Browser/site icon resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub site_icon: Option<ResourceId>,
    /// Default social/link-preview image resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_preview: Option<ResourceId>,
}

/// Local product-manual and generated-reference target, with no inferred name,
/// path, remote or hosting account.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocumentationRepository {
    /// Explicit public reading base, independent of the local path and delivery remote.
    /// Omission keeps module destinations local-only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_base_url: Option<gazette_core::publication::routing::PublicBaseUrl>,
    /// Explicit repository name; may differ from the local directory's final component.
    pub name: String,
    /// Explicit absolute or confined workspace-relative local directory, using the native
    /// source-root grammar. Relative paths require a selected native workspace, never cwd.
    /// Compilation does not read or create the directory.
    pub path: String,
    /// Explicit documentation-repository delivery coordinates. Absence remains local-only
    /// and never inherits the authored site's independently configured destination.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::tables::optional"
    )]
    pub destination: Option<gazette_core::release::PublicationDestination>,
    /// Optional local assembly selection; declaring a destination alone does not choose
    /// a target, authored manual or generated-reference input.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::tables::optional"
    )]
    pub build: Option<super::DocumentationBuild>,
}
