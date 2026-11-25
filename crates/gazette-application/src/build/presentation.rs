//! One typed presentation envelope shared by every Theme-rendered surface.

use std::collections::BTreeMap;

use gazette_compiler::workspace::Key;
use gazette_core::publication::CapabilityId;
use serde::Serialize;

use super::{
    CapabilityProjection, CompiledTheme, ResourceProjection, ThemeResourceProjection,
    resources::{BrowserModules, CompiledResources, PageBrowserModules},
    search::Search,
};

/// Publication-owned semantic roles over already compiled resources.
#[derive(Clone, Copy, Serialize, schemars::JsonSchema)]
pub(super) struct IdentityAssets<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub site_icon: Option<&'a ResourceProjection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_preview: Option<&'a ResourceProjection>,
}

impl<'a> IdentityAssets<'a> {
    pub(super) fn resolve(
        selected: &gazette_compiler::workspace::IdentityAssets,
        resources: &'a BTreeMap<gazette_core::resource::ResourceId, ResourceProjection>,
    ) -> Result<Option<Self>, (&'static str, gazette_core::resource::ResourceId)> {
        let resolve = |role: &'static str,
                       id: &Option<gazette_core::resource::ResourceId>,
                       allow_svg: bool| {
            id.as_ref()
                .map(|id| {
                    let resource = resources.get(id).ok_or_else(|| (role, id.clone()))?;
                    if !(matches!(
                        resource.kind,
                        gazette_core::resource::Kind::PngImage
                            | gazette_core::resource::Kind::JpegImage
                    ) || (allow_svg
                        && resource.kind == gazette_core::resource::Kind::SvgImage))
                    {
                        return Err((role, id.clone()));
                    }
                    Ok(resource)
                })
                .transpose()
        };
        let site_icon = resolve("site-icon", &selected.site_icon, true)?;
        let link_preview = resolve("link-preview", &selected.link_preview, false)?;
        Ok(
            (site_icon.is_some() || link_preview.is_some()).then_some(Self {
                site_icon,
                link_preview,
            }),
        )
    }
}

/// Closed Theme-facing class of the exact surface being rendered.
#[derive(Clone, Copy, Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub(super) enum SurfaceKind {
    NotFound,
    Page,
    Alias,
    Mirror,
    Graph,
    Taxonomy,
}

#[derive(Serialize, schemars::JsonSchema)]
pub(super) struct Surface {
    pub kind: SurfaceKind,
}

/// One successfully emitted feed, never merely a configured or omitted output.
#[derive(Serialize, schemars::JsonSchema)]
pub(super) struct FeedDiscovery {
    pub id: Key,
    pub channel: Key,
    /// The channel's authored title, shared by its format alternatives.
    pub title: String,
    /// Exact serving address, including the selected preview base and mount.
    pub url: String,
    pub format: gazette_compiler::workspace::FeedFormat,
    pub media_type: &'static str,
}

/// Site-level machine entrypoints that were emitted into this assembly.
#[derive(Clone, Copy, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub(super) enum DiscoveryKind {
    WebManifest,
    Sitemap,
    Llms,
    /// Disposable browser reading-cache descriptor for this publication.
    PageCache,
    /// The site's terms page, present when it reserves text and data mining rights.
    TdmPolicy,
}

/// A resolved output destination; labels and placement belong to site composition.
#[derive(Serialize, schemars::JsonSchema)]
pub(super) struct OutputDiscovery {
    pub kind: DiscoveryKind,
    pub url: String,
    pub media_type: &'static str,
}

/// Exact generated graph destination, present only in complete assemblies that emit it.
#[derive(Serialize, schemars::JsonSchema)]
pub(super) struct GraphDiscovery {
    pub url: String,
}

/// Existing public discovery metadata and retained reading text for browser tools.
#[derive(Serialize, schemars::JsonSchema)]
pub(super) struct BrowserPage<'a> {
    pub content: gazette_compiler::authored::DiscoveryNode<'a>,
    pub route: &'a gazette_compiler::workspace::ResolvedRoute,
    pub body_text: &'a str,
}

/// Gazette-admitted presentation facts; Theme never selects providers or opens packages.
#[derive(Serialize, schemars::JsonSchema)]
pub(super) struct Presentation<'a> {
    /// Explicit public current-page value, absent outside eligible authored pages.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webmcp: Option<&'a BrowserPage<'a>>,
    /// Existing public publication metadata for generated views without a page site object.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publication: Option<gazette_compiler::authored::PublicationMetadata<'a>>,
    /// Exact graph output selected for this complete assembly, never a guessed path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph: Option<&'a GraphDiscovery>,
    /// Verified planned alternate for this exact serving surface.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub markdown_alternate: Option<&'a gazette_compiler::authored::MarkdownAlternate>,
    /// Only successful emitted outputs; absent for an empty or unassembled set.
    #[serde(skip_serializing_if = "<[FeedDiscovery]>::is_empty")]
    pub feeds: &'a [FeedDiscovery],
    /// Only successfully emitted site entrypoints and the reserved terms page; empty for
    /// standalone pages.
    #[serde(skip_serializing_if = "<[OutputDiscovery]>::is_empty")]
    pub discovery: &'a [OutputDiscovery],
    /// Exact canonical target facts for alias/mirror redirect surfaces only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redirect_destination: Option<gazette_compiler::markdown::LinkDestination>,
    pub surface: Surface,
    pub theme: &'a gazette_compiler::theme_package::ThemePackageId,
    pub palette: Option<&'a Key>,
    pub browser_theme_colors: &'a gazette_compiler::theme_package::BrowserThemeColors,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chrome: Option<&'a gazette_compiler::authored::SiteChromeView<'a>>,
    pub capabilities: &'a BTreeMap<CapabilityId, CapabilityProjection>,
    pub theme_resources: &'a [ThemeResourceProjection],
    pub resources: Vec<&'a ResourceProjection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub browser_modules: Option<PageBrowserModules<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interaction: Option<&'a ResourceProjection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search: Option<&'a Search<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity_assets: Option<IdentityAssets<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub social_card: Option<&'a super::SocialCardProjection>,
}

impl<'a> Presentation<'a> {
    /// Site-level presentation for generated views that have no page-local resources.
    pub(super) fn site(
        theme: &'a CompiledTheme,
        capabilities: &'a BTreeMap<CapabilityId, CapabilityProjection>,
        resources: &'a CompiledResources,
        surface: SurfaceKind,
        chrome: Option<&'a gazette_compiler::authored::SiteChromeView<'a>>,
        search: Option<&'a Search<'a>>,
        identity_assets: Option<IdentityAssets<'a>>,
    ) -> Self {
        Self {
            webmcp: None,
            publication: None,
            redirect_destination: None,
            graph: None,
            markdown_alternate: None,
            feeds: &[],
            discovery: &[],
            surface: Surface { kind: surface },
            theme: &theme.evidence().theme,
            palette: theme.evidence().palette.as_ref(),
            browser_theme_colors: &theme.evidence().browser_theme_colors,
            chrome,
            capabilities,
            theme_resources: &resources.theme,
            resources: vec![],
            browser_modules: resources.browser.site(),
            interaction: None,
            search,
            identity_assets,
            social_card: None,
        }
    }

    /// Page-level presentation with the exact local resource and module closure.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn page(
        theme: &'a CompiledTheme,
        capabilities: &'a BTreeMap<CapabilityId, CapabilityProjection>,
        theme_resources: &'a [ThemeResourceProjection],
        resources: &'a [ResourceProjection],
        browser_modules: Option<&'a BrowserModules>,
        page: &Key,
        interaction: Option<&'a ResourceProjection>,
        chrome: Option<&'a gazette_compiler::authored::SiteChromeView<'a>>,
        search: Option<&'a Search<'a>>,
        identity_assets: Option<IdentityAssets<'a>>,
    ) -> Self {
        Self {
            webmcp: None,
            publication: None,
            redirect_destination: None,
            graph: None,
            markdown_alternate: None,
            feeds: &[],
            discovery: &[],
            surface: Surface {
                kind: SurfaceKind::Page,
            },
            theme: &theme.evidence().theme,
            palette: theme.evidence().palette.as_ref(),
            browser_theme_colors: &theme.evidence().browser_theme_colors,
            chrome,
            capabilities,
            theme_resources,
            resources: resources.iter().collect(),
            browser_modules: browser_modules.and_then(|modules| modules.page(page)),
            interaction,
            search,
            identity_assets,
            social_card: None,
        }
    }
}

/// Add the shared presentation envelope without rebuilding the semantic payload.
#[derive(Serialize)]
pub(super) struct Presented<'a, T: Serialize> {
    #[serde(flatten)]
    pub value: &'a T,
    pub presentation: &'a Presentation<'a>,
}
