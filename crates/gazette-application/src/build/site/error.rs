//! Complete-site failure classes, without partial successful output.

use super::*;
use std::fmt;

/// Local rendering cannot account for every requested object or output.
#[derive(Debug)]
pub enum SiteRenderError {
    /// Pure rendering requires one exact explicit browser capture before it can finish.
    SocialCardCaptureRequired(Box<super::super::SocialCardCaptureRequest>),
    /// Capture or reuse failed; no card fallback or native retry is implied.
    SocialCard(Box<super::super::SocialCardGenerationError>),
    /// A configured reading index could not be resolved completely.
    Listing(gazette_compiler::authored::ListingError),
    /// Exact configured taxonomy listings could not be projected from this selection.
    Taxonomy(gazette_compiler::authored::TaxonomyError),
    /// An emitted discovery selection cannot resolve every exact content binding.
    Discovery(gazette_compiler::authored::SourceBindingError),
    /// A selected feed failed exact selection, format requirements or serialization.
    Feed(gazette_compiler::authored::FeedError),
    /// The corpus and compiled Theme do not share exact configuration bytes.
    Configuration,
    /// The shared routed-page selection or a discovery projection failed.
    Pages(SitePagesError),
    /// Redirect addresses exist but no alias template was selected.
    AliasTemplateRequired(Key),
    /// A graph output was selected but the Theme exposes no graph render entry.
    GraphTemplateRequired,
    /// An invocation graph choice contradicts the publication-owned graph selection.
    GraphSelection(&'static str),
    /// A selected page did not retain its compiler-owned JSON-LD value.
    JsonLdRequired(Key),
    /// An opaque input requires an asset/capability compiler, not silent copying/omission.
    UncompiledAsset(EntryPath),
    /// A publication identity role names no compiled selected resource.
    IdentityAsset {
        /// Stable semantic role selected by publication configuration.
        role: &'static str,
        /// Selected logical resource identity.
        resource: ResourceId,
    },
    /// Configured static-asset planning failed before local output assembly.
    Asset(AssetError),
    /// The complete local graph projection failed; no partial view escapes.
    Graph(SiteGraphError),
    /// The graph-view renderer returned non-UTF-8 output.
    GraphUtf8(std::str::Utf8Error),
    /// A selected page or Theme failed its existing render contract.
    Page(PageRenderError),
    /// Alias rendering or bounded machine serialization failed.
    Render(render::Error),
    /// Output paths, inventory, exact bytes or aggregate bounds are invalid.
    Output(source::Error),
}

impl SiteRenderError {
    /// Stable class, preserving lower-layer diagnostic meaning.
    pub fn code(&self) -> &'static str {
        match self {
            Self::SocialCardCaptureRequired(_) => "social_card_capture_required",
            Self::SocialCard(error) => error.code(),
            Self::Taxonomy(error) => error.code(),
            Self::Listing(error) => error.code(),
            Self::Discovery(error) => error.code(),
            Self::Feed(error) => error.code(),
            Self::Configuration => "site_configuration",
            Self::Pages(error) => error.code(),
            Self::AliasTemplateRequired(_) => "site_alias_template_required",
            Self::GraphTemplateRequired => "site_graph_template_required",
            Self::GraphSelection(_) => "site_graph_selection",
            Self::JsonLdRequired(_) => "site_json_ld_required",
            Self::UncompiledAsset(_) => "site_uncompiled_asset",
            Self::IdentityAsset { .. } => "site_identity_asset",
            Self::Asset(error) => error.code(),
            Self::Graph(error) => error.code(),
            Self::GraphUtf8(_) => "graph_output_utf8",
            Self::Page(error) => error.code(),
            Self::Render(error) => error.code(),
            Self::Output(error) => error.code(),
        }
    }
}
impl fmt::Display for SiteRenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SocialCardCaptureRequired(request) => write!(
                f,
                "site {} requires explicit social-card capture",
                request.site()
            ),
            Self::SocialCard(error) => error.fmt(f),
            Self::Taxonomy(error) => error.fmt(f),
            Self::Listing(error) => error.fmt(f),
            Self::Discovery(error) => error.fmt(f),
            Self::Feed(error) => error.fmt(f),
            Self::Configuration => f.write_str("site corpus and Theme configuration differ"),
            Self::Pages(error) => error.fmt(f),
            Self::AliasTemplateRequired(id) => {
                write!(f, "site object {id} needs an alias template")
            }
            Self::GraphTemplateRequired => {
                f.write_str("site graph output needs a Theme-owned graph template")
            }
            Self::GraphSelection(reason) => f.write_str(reason),
            Self::JsonLdRequired(id) => write!(f, "site object {id} is missing selected JSON-LD"),
            Self::UncompiledAsset(path) => write!(
                f,
                "input {path} needs a capability/asset compiler; no partial site produced"
            ),
            Self::IdentityAsset { role, resource } => write!(
                f,
                "publication identity role {role} requires a compatible enabled compiled image resource; {resource} did not resolve"
            ),
            Self::Asset(error) => error.fmt(f),
            Self::Graph(error) => error.fmt(f),
            Self::GraphUtf8(error) => write!(
                f,
                "graph renderer returned invalid UTF-8: {error}; no site produced"
            ),
            Self::Page(error) => error.fmt(f),
            Self::Render(error) => error.fmt(f),
            Self::Output(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for SiteRenderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::SocialCard(error) => Some(error),
            Self::Taxonomy(error) => Some(error),
            Self::Listing(error) => Some(error),
            Self::Discovery(error) => Some(error),
            Self::Feed(error) => Some(error),
            Self::Pages(error) => Some(error),
            Self::Asset(error) => Some(error),
            Self::Graph(error) => Some(error),
            Self::GraphUtf8(error) => Some(error),
            Self::Page(error) => Some(error),
            Self::Render(error) => Some(error),
            Self::Output(error) => Some(error),
            _ => None,
        }
    }
}
