//! Primary subjects join existing principals without inventing publication roles.

use super::{PrincipalWebsiteView, ProjectionError};
use crate::workspace::ResolvedContent;
use gazette_core::publication::{
    Principal,
    content::{ContentNode, ServiceSubject, Subject},
};
use serde::Serialize;

/// One admitted subject with its exact existing principal and public identity address.
#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum SubjectView<'a> {
    /// The person or organization described by an About page.
    Principal {
        /// Exact principal record, independent from page attribution.
        principal: &'a Principal,
        /// Explicit canonical and human-navigation website, when declared.
        #[serde(skip_serializing_if = "Option::is_none")]
        website: Option<PrincipalWebsiteView>,
    },
    /// An explicitly named service with its independently declared provider.
    Service {
        /// Exact service facts, not reconstructed page metadata.
        service: &'a ServiceSubject,
        /// Existing provider principal.
        provider: &'a Principal,
        /// The provider's explicit website, when declared.
        #[serde(skip_serializing_if = "Option::is_none")]
        website: Option<PrincipalWebsiteView>,
    },
}

pub(super) fn resolve<'a>(
    resolved: &'a ResolvedContent<'_>,
    node: &'a ContentNode,
) -> Result<Option<SubjectView<'a>>, ProjectionError> {
    let Some(subject) = &node.subject else {
        return Ok(None);
    };
    let principal = resolved
        .compilation()
        .workspace
        .principal(subject.principal())
        .ok_or_else(|| ProjectionError::Binding {
            id: node.id.clone(),
            reason: "bound subject principal missing",
        })?;
    let website = resolved.principal_website(principal, &node.site);
    Ok(Some(match subject {
        Subject::Principal { .. } => SubjectView::Principal { principal, website },
        Subject::Service(service) => SubjectView::Service {
            service,
            provider: principal,
            website,
        },
    }))
}
