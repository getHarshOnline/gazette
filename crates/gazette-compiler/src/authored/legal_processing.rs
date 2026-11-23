//! Borrowed configured legal context, never a provider observation or an approved notice.

use serde::Serialize;

use crate::workspace::{DiscussionProvider, HostingSelection, PublicationProcessing};

/// Processing intent composed for an explicitly bound owner-held legal output.
/// Publisher, roles and contacts remain in the page's existing shared site view.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct LegalProcessingContext<'a> {
    /// Intended hosting, not a claim that this host currently serves the site.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hosting: Option<&'a HostingSelection>,
    /// Explicit analytics switch; disabled provider details do not reach publication.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub analytics: Option<bool>,
    /// Explicit subscription switch, without an invented inbox or endpoint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscription: Option<bool>,
    /// Native capability-backed discussion intent; never exposes repository/category IDs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discussion: Option<LegalDiscussionContext>,
    /// Explicit browser-capability switch, without automatic browser registration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webmcp: Option<bool>,
}

/// Public legal selection distinct from private provider binding coordinates.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct LegalDiscussionContext {
    /// Explicit configured selection, not verified live activity.
    pub enabled: bool,
    /// Provider identity only when enabled through its existing native capability.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<DiscussionProvider>,
}

impl<'a> From<&'a PublicationProcessing> for LegalProcessingContext<'a> {
    fn from(selection: &'a PublicationProcessing) -> Self {
        Self {
            hosting: selection.hosting.as_ref(),
            analytics: selection.analytics.as_ref().map(|value| value.enabled),
            subscription: selection.subscription.as_ref().map(|value| value.enabled),
            discussion: selection
                .discussion
                .as_ref()
                .map(|value| LegalDiscussionContext {
                    enabled: value.enabled,
                    provider: value.enabled.then_some(value.provider),
                }),
            webmcp: selection.webmcp.as_ref().map(|value| value.enabled),
        }
    }
}
