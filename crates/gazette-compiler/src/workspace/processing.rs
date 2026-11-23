//! Intended processing choices, not observations of running services or legal approval.

use gazette_core::publication::{
    CoreCapabilityCatalog,
    browser::{GoogleMeasurementId, WebMcpTool, WebMcpTools},
    capability_runtime::CapabilityValue,
};
use serde::{Deserialize, Serialize};

use super::{CapabilitySelection, Diagnostic, Workspace, resolve::report};

/// Explicit publication choices; omission is distinct from an explicit disabled feature.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PublicationProcessing {
    /// Intended host, without a deployed-site or connection-data assertion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hosting: Option<HostingSelection>,
    /// Explicit public analytics binding, never browser consent or property readiness.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analytics: Option<AnalyticsSelection>,
    /// Subscription selection, never a guessed form endpoint or credential.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subscription: Option<ServiceSelection<SubscriptionProvider>>,
    /// Discussion selection over an independently configured native capability.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discussion: Option<ProcessingDiscussion>,
    /// Browser capability selection; not activated by declaration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webmcp: Option<WebMcpSelection>,
    /// Reader-initiated preferred-source invitation, without third-party SDK loading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_sources: Option<super::PreferredSourcesSelection>,
}

impl PublicationProcessing {
    /// Configured public client values for acquisition, not selected resources or effects.
    /// Workspace validation separately requires their enabled capability bindings.
    pub fn browser_bindings(&self) -> impl Iterator<Item = CapabilityValue> + '_ {
        let analytics = self
            .analytics
            .as_ref()
            .filter(|selection| selection.enabled)
            .and_then(|selection| selection.measurement_id.clone())
            .map(|measurement_id| CapabilityValue::PublicationAnalyticsV1 { measurement_id });
        let webmcp = self
            .webmcp
            .as_ref()
            .filter(|selection| selection.enabled)
            .and_then(|selection| selection.tools.clone())
            .map(|tools| CapabilityValue::PublicationWebMcpV1 { tools });
        [analytics, webmcp].into_iter().flatten()
    }
}

/// An explicit GA4 client binding; no remote property is read during compilation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AnalyticsSelection {
    /// Desired activation, separate from consent and client resource selection.
    pub enabled: bool,
    /// Closed analytics family.
    pub provider: AnalyticsProvider,
    /// Explicit public tag identity, required when enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measurement_id: Option<GoogleMeasurementId>,
}

/// A provider choice with an explicit activation switch, without speculative provider fields.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ServiceSelection<P> {
    /// Explicit desired activation, never inferred from a provider name.
    pub enabled: bool,
    /// Closed provider identity even when disabled.
    pub provider: P,
}

/// Explicit read-only public browser tools, not Gazette's native MCP effect catalogue.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WebMcpSelection {
    /// Explicit desired activation.
    pub enabled: bool,
    /// Nonempty unique selection, required when enabled; no inferred default tools.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<WebMcpTools>,
}

/// Intended hosting choice; no runtime activation or remote observation occurs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HostingSelection {
    /// Selected host identity.
    pub provider: HostingProvider,
}

pub use gazette_core::hosting::HostingProvider;

/// Analytics family represented by the configured public client binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum AnalyticsProvider {
    /// Google Analytics.
    GoogleAnalytics,
}

/// Currently named subscription provider; no endpoint is inferred.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SubscriptionProvider {
    /// Google Apps Script.
    GoogleAppsScript,
}

/// Explicit discussion provider identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DiscussionProvider {
    /// Giscus, with its native capability configured separately.
    Giscus,
}

/// Discussion desired state, not an alternate owner of native repository/category IDs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProcessingDiscussion {
    /// Explicit activation; false conflicts with an enabled native discussion binding.
    pub enabled: bool,
    /// Named provider, without guessed runtime configuration.
    pub provider: DiscussionProvider,
    /// Optional exact public repository selector; never fetched during validation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
}

pub(super) fn validate(workspace: &Workspace, errors: &mut Vec<Diagnostic>) {
    validate_with_projects(workspace, None, errors);
}

pub(super) fn validate_with_projects(
    workspace: &Workspace,
    projects: Option<&std::collections::BTreeMap<super::Key, gazette_core::publication::Project>>,
    errors: &mut Vec<Diagnostic>,
) {
    super::preferred_sources::validate(workspace, errors);
    for brand in workspace.brands() {
        for publication in &brand.publications {
            let field = format!(
                "brands[{:?}].publications[{:?}].processing",
                brand.id.as_str(),
                publication.site.as_str()
            );
            validate_browser(workspace, publication, &field, errors);
            let Some(processing) = &publication.processing else {
                continue;
            };
            if processing
                .subscription
                .as_ref()
                .is_some_and(|value| value.enabled)
            {
                report(
                    errors,
                    "workspace_processing_unsupported",
                    &format!("{field}.subscription"),
                    Some(&publication.site),
                );
            }
            let Some(discussion) = &processing.discussion else {
                continue;
            };
            if discussion
                .repository
                .as_deref()
                .is_some_and(|value| !super::repositories::valid(value))
            {
                report(
                    errors,
                    "workspace_processing_repository",
                    &format!("{field}.discussion.repository"),
                    Some(&publication.site),
                );
            }
            // Landing-owned atoms are validated after the complete captured catalog joins.
            if projects.is_none()
                && workspace.placements().iter().any(|placement| {
                    placement.render
                        && placement.site == publication.site
                        && matches!(
                            workspace.project_declaration(&placement.project),
                            Some(super::ProjectDeclaration::Landing(_))
                        )
                })
            {
                continue;
            }
            let mut active = Vec::new();
            let selected = |atom: &gazette_core::publication::Discussion| {
                workspace
                    .composition_for(&publication.site)
                    .and_then(|composition| composition.capabilities().get(&atom.capability))
                    .is_some_and(|selection| {
                        matches!(selection.value(), CapabilitySelection::Enabled { .. })
                    })
            };
            if let Some(site) = workspace.site(&publication.site)
                && site.discussion.as_ref().is_some_and(selected)
                && let Some(repository) = publication.repository.as_deref()
            {
                active.push(repository);
            }
            for placement in workspace
                .placements()
                .iter()
                .filter(|placement| placement.render && placement.site == publication.site)
            {
                if let Some(project) = projects.map_or_else(
                    || workspace.project(&placement.project),
                    |projects| projects.get(&placement.project),
                ) && project.discussion.as_ref().is_some_and(selected)
                    && let Some(repository) = project.repository.as_deref()
                {
                    active.push(repository);
                }
            }
            let code = if !discussion.enabled && !active.is_empty() {
                Some("workspace_processing_discussion_conflict")
            } else if discussion.enabled
                && (active.is_empty()
                    || discussion
                        .repository
                        .as_deref()
                        .is_some_and(|repository| !active.contains(&repository)))
            {
                Some("workspace_processing_discussion_binding")
            } else {
                None
            };
            if let Some(code) = code {
                report(
                    errors,
                    code,
                    &format!("{field}.discussion"),
                    Some(&publication.site),
                );
            }
        }
    }
}

fn validate_browser(
    workspace: &Workspace,
    publication: &super::BrandPublication,
    field: &str,
    errors: &mut Vec<Diagnostic>,
) {
    let processing = publication.processing.as_ref();
    let analytics = processing.and_then(|value| value.analytics.as_ref());
    let webmcp = processing.and_then(|value| value.webmcp.as_ref());
    let active = |capability| {
        workspace
            .composition_for(&publication.site)
            .and_then(|composition| composition.capabilities().get(&capability))
            .is_some_and(|selection| {
                matches!(selection.value(), CapabilitySelection::Enabled { .. })
            })
    };
    for (name, enabled, configured, selected, binding, conflict) in [
        (
            "analytics",
            analytics.is_some_and(|value| value.enabled),
            analytics.is_some_and(|value| value.measurement_id.is_some()),
            active(CoreCapabilityCatalog::publication_analytics()),
            "workspace_processing_analytics_binding",
            "workspace_processing_analytics_conflict",
        ),
        (
            "webmcp",
            webmcp.is_some_and(|value| value.enabled),
            webmcp.is_some_and(|value| value.tools.is_some()),
            active(CoreCapabilityCatalog::publication_webmcp()),
            "workspace_processing_webmcp_binding",
            "workspace_processing_webmcp_conflict",
        ),
    ] {
        let code = if enabled && (!configured || !selected) {
            Some(binding)
        } else if selected && !enabled {
            Some(conflict)
        } else {
            None
        };
        if let Some(code) = code {
            report(
                errors,
                code,
                &format!("{field}.{name}"),
                Some(&publication.site),
            );
        }
    }
    if webmcp.is_some_and(|value| {
        value.enabled
            && value
                .tools
                .as_ref()
                .is_some_and(|tools| tools.as_slice().contains(&WebMcpTool::Search))
    }) && publication.machine.search.is_none()
    {
        report(
            errors,
            "workspace_processing_webmcp_search",
            &format!("{field}.webmcp.tools"),
            Some(&publication.site),
        );
    }
}
