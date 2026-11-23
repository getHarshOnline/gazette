//! Explicit primary subjects without changing editorial families or inferring providers.

use super::resolve::report;
use super::{ContentKind, ContentNode, Diagnostic, Key, PageFamily, SchemaType};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One declared primary subject of a utility page.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Subject {
    /// An existing person or organization; not an attribution or publisher fallback.
    Principal {
        /// Exact workspace principal identity.
        principal: Key,
    },
    /// An explicitly identified service, distinct from the page describing it.
    Service(ServiceSubject),
}

/// Service facts supplied together by each declaring page.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ServiceSubject {
    /// Stable service identity; repeated declarations must agree on every fact.
    pub id: Key,
    /// Explicit service name, never copied from a page title.
    pub name: String,
    /// Exact workspace provider, independent from the page's publisher and authors.
    pub provider: Key,
    /// Optional explicit service description, without a page-description fallback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl Subject {
    /// Existing principal required by this declaration.
    pub fn principal(&self) -> &Key {
        match self {
            Self::Principal { principal } => principal,
            Self::Service(service) => &service.provider,
        }
    }
}

pub(super) fn validate(nodes: &BTreeMap<Key, ContentNode>, errors: &mut Vec<Diagnostic>) {
    let mut services: BTreeMap<&Key, (&ServiceSubject, &Key)> = BTreeMap::new();
    for node in nodes.values() {
        let utility = matches!(
            node.structure,
            ContentKind::Page {
                family: PageFamily::Utility
            }
        );
        let valid = match (&node.subject, node.schema_type) {
            (Some(Subject::Principal { .. }), Some(SchemaType::AboutPage)) => utility,
            (Some(Subject::Service(_)), None) => utility,
            (None, Some(SchemaType::AboutPage)) => utility,
            (Some(_), _) => false,
            (None, _) => true,
        };
        if !valid {
            report(errors, "content_subject_kind", &node.id, None);
        }
        if let Some(Subject::Service(service)) = &node.subject {
            if service.name.trim().is_empty() || service.name.chars().any(char::is_control) {
                report(errors, "content_service_name", &node.id, Some(&service.id));
            }
            if service.description.as_ref().is_some_and(|text| {
                text.trim().is_empty()
                    || text
                        .chars()
                        .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\r' | '\t'))
            }) {
                report(
                    errors,
                    "content_service_description",
                    &node.id,
                    Some(&service.id),
                );
            }
            if let Some((previous, owner)) = services.get(&service.id) {
                if *previous != service {
                    report(errors, "content_service_conflict", owner, Some(&service.id));
                    report(
                        errors,
                        "content_service_conflict",
                        &node.id,
                        Some(&service.id),
                    );
                }
            } else {
                services.insert(&service.id, (service, &node.id));
            }
        }
    }
}
