//! Publisher-selected crawler rules, terms of use and install metadata, without hosting effects.

use std::collections::BTreeSet;

use gazette_core::publication::routing::{PublicBaseUrl, RoutePath};
use serde::{Deserialize, Serialize};

/// Rules written to the publication's robots.txt in their declared order.
/// Crawler directives request behaviour; they do not restrict access to files.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RobotsConfiguration {
    /// Whether to emit the complete configured policy.
    pub enabled: bool,
    /// Explicit crawler groups. Names and policies belong to the publisher.
    pub rules: Vec<CrawlerRule>,
}

/// One robots.txt user-agent group, without an inferred crawler category.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CrawlerRule {
    /// One product token, or * for crawlers without a more specific group.
    pub user_agent: String,
    /// Allowed path patterns, emitted before disallowed patterns.
    #[serde(default)]
    pub allow: Vec<String>,
    /// Disallowed path patterns. An empty string explicitly permits all paths.
    #[serde(default)]
    pub disallow: Vec<String>,
}

/// Where the site's terms of use live, shared by its TDM reservation and linked data.
/// A reservation requests behaviour from text and data miners; it does not restrict access.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TermsConfiguration {
    /// Site-relative route of the terms page, beginning with `/`.
    pub path: String,
    /// Emit `/.well-known/tdmrep.json` and a `tdm-policy` discovery entry for Themes.
    #[serde(default)]
    pub tdm_reservation: bool,
}

impl TermsConfiguration {
    /// Absolute terms address beneath `base`, or `None` when the path is not a valid route.
    pub fn url(&self, base: &PublicBaseUrl) -> Option<String> {
        self.path
            .parse::<RoutePath>()
            .ok()
            .map(|route| base.resolve(&route))
    }
}

impl super::BrandPublication {
    /// Absolute terms address beneath the configured production base, when both are set.
    pub fn terms_url(&self) -> Option<String> {
        self.machine
            .terms
            .as_ref()?
            .url(self.public_base_url.as_ref()?)
    }
}

impl CrawlerRule {
    /// The same group as the shared robots.txt writer reads it.
    pub fn group(&self) -> gazette_core::publication::usage::CrawlerGroup<'_> {
        gazette_core::publication::usage::CrawlerGroup {
            user_agent: &self.user_agent,
            allow: &self.allow,
            disallow: &self.disallow,
        }
    }
}

/// Optional browser manifest using the existing publication name and home route.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WebManifestConfiguration {
    /// Whether to emit manifest.webmanifest.
    pub enabled: bool,
    /// Optional compact name; omission keeps the publication's ordinary name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short_name: Option<String>,
}

pub(super) fn validate(publication: &super::BrandPublication, errors: &mut Vec<super::Diagnostic>) {
    let mut report = |field: &str| {
        errors.push(super::Diagnostic {
            code: "workspace_deployment_output",
            field: format!("brands.publications.machine.{field}"),
            key: Some(publication.site.clone()),
        })
    };
    if let Some(robots) = &publication.machine.robots {
        let mut agents = BTreeSet::new();
        if robots.rules.len() > 128 || (robots.enabled && robots.rules.is_empty()) {
            report("robots.rules");
        }
        for rule in &robots.rules {
            if rule.user_agent.is_empty()
                || rule.user_agent.len() > 128
                || (rule.user_agent != "*"
                    && !rule
                        .user_agent
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)))
                || !agents.insert(rule.user_agent.to_ascii_lowercase())
                || rule.allow.len() + rule.disallow.len() > 256
                || rule.allow.iter().chain(&rule.disallow).any(|path| {
                    path.len() > 4096
                        || (!path.is_empty() && !path.starts_with('/'))
                        || path
                            .chars()
                            .any(|c| c.is_control() || c.is_whitespace() || c == '#')
                })
            {
                report("robots.rules");
            }
        }
    }
    if let Some(terms) = &publication.machine.terms
        && (terms.path.len() > 1024
            || !terms.path.starts_with('/')
            || terms.path.contains("//")
            || terms
                .path
                .chars()
                .any(|c| c.is_control() || c.is_whitespace() || c == '#' || c == '?')
            || terms.path.parse::<RoutePath>().is_err())
    {
        report("terms.path");
    }
    if let Some(manifest) = &publication.machine.web_manifest
        && manifest.short_name.as_ref().is_some_and(|name| {
            name.is_empty() || name.len() > 256 || name.chars().any(char::is_control)
        })
    {
        report("web_manifest.short_name");
    }
}
