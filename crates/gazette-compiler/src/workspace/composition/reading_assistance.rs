//! Explicit bounded reader handoffs; neither provider activation nor a prompt evaluator.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::{Diagnostic, Key};

/// Optional user-invoked handoffs for selected reading text and its native page context.
/// Endpoint selection establishes no remote service availability or transmission permission.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadingAssistance {
    /// Accessible name for the selection action group.
    #[schemars(length(min = 1, max = 256))]
    pub label: String,
    /// Literal prompt with only the documented single-brace context slots.
    #[schemars(length(min = 1, max = 4096))]
    pub prompt_template: String,
    /// Maximum selected Unicode characters offered in a handoff.
    #[schemars(range(min = 1, max = 4096))]
    pub selection_limit: usize,
    /// Maximum nearby-context Unicode characters; zero explicitly excludes context.
    #[schemars(range(max = 1024))]
    pub context_limit: usize,
    /// Declared actions in presentation order, never guessed from a provider's label.
    #[schemars(length(min = 1, max = 8))]
    pub providers: Vec<ReadingAssistanceProvider>,
}

/// One external prompt-prefill destination, independent from API credentials or execution.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadingAssistanceProvider {
    /// Stable namespaced local identity; neither display wording nor hostname supplies it.
    #[schemars(length(max = 256))]
    pub id: Key,
    /// Reader-visible action wording.
    #[schemars(length(min = 1, max = 256))]
    pub label: String,
    /// Explicit HTTPS endpoint without credentials, fragments or a competing prompt query key.
    #[schemars(length(min = 1, max = 2048))]
    pub endpoint: String,
    /// ASCII query parameter into which the Theme URL-encodes the completed prompt.
    #[schemars(length(min = 1, max = 64))]
    #[schemars(regex(pattern = "^[A-Za-z][A-Za-z0-9_-]{0,63}$"))]
    pub query_parameter: String,
}

impl ReadingAssistance {
    pub(super) fn retained_text(&self) -> impl Iterator<Item = &str> {
        [self.label.as_str(), self.prompt_template.as_str()]
            .into_iter()
            .chain(self.providers.iter().flat_map(|provider| {
                [
                    provider.id.as_str(),
                    provider.label.as_str(),
                    provider.endpoint.as_str(),
                    provider.query_parameter.as_str(),
                ]
            }))
    }
}

fn label(value: &str) -> bool {
    !value.trim().is_empty() && value.chars().count() <= 256 && !value.chars().any(char::is_control)
}

fn slots(template: &str) -> bool {
    let mut found = BTreeSet::new();
    let mut remaining = template;
    while let Some((position, brace)) = remaining
        .char_indices()
        .find(|(_, c)| *c == '{' || *c == '}')
    {
        if brace == '}' {
            return false;
        }
        remaining = &remaining[position + 1..];
        let Some(end) = remaining.find('}') else {
            return false;
        };
        let slot = &remaining[..end];
        if !matches!(
            slot,
            "selection"
                | "article_title"
                | "article_url"
                | "site_name"
                | "site_url"
                | "section_title"
                | "context"
        ) {
            return false;
        }
        found.insert(slot);
        remaining = &remaining[end + 1..];
    }
    ["selection", "article_url", "site_name", "site_url"]
        .iter()
        .all(|required| found.contains(required))
}

fn parameter(value: &str) -> bool {
    let mut bytes = value.bytes();
    value.len() <= 64
        && bytes.next().is_some_and(|c| c.is_ascii_alphabetic())
        && bytes.all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}

fn endpoint(value: &str, parameter: &str) -> bool {
    if value.chars().count() > 2048 || value.trim() != value || value.chars().any(char::is_control)
    {
        return false;
    }
    url::Url::parse(value).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.fragment().is_none()
            && !url.query_pairs().any(|(key, _)| key == parameter)
            && value.split_once("//").is_some_and(|(_, authority)| {
                !authority
                    .split(['/', '?', '#'])
                    .next()
                    .unwrap_or_default()
                    .contains('@')
            })
    })
}

pub(super) fn validate(value: &ReadingAssistance, prefix: &str, errors: &mut Vec<Diagnostic>) {
    let prefix = format!("{prefix}.chrome.reading_assistance");
    let mut report = |field: &str, code, key| {
        errors.push(Diagnostic {
            code,
            field: format!("{prefix}.{field}"),
            key,
        })
    };
    if !label(&value.label) {
        report("label", "workspace_reading_assistance_copy", None);
    }
    if value.prompt_template.trim().is_empty()
        || value.prompt_template.chars().count() > 4096
        || value
            .prompt_template
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
        || !slots(&value.prompt_template)
    {
        report(
            "prompt_template",
            "workspace_reading_assistance_template",
            None,
        );
    }
    if !(1..=4096).contains(&value.selection_limit) {
        report(
            "selection_limit",
            "workspace_reading_assistance_limit",
            None,
        );
    }
    if value.context_limit > 1024 {
        report("context_limit", "workspace_reading_assistance_limit", None);
    }
    if value.providers.is_empty() || value.providers.len() > 8 {
        report("providers", "workspace_reading_assistance_limit", None);
    }
    let mut ids = BTreeSet::new();
    for (index, provider) in value.providers.iter().enumerate() {
        if provider.id.as_str().chars().count() > 256 || !ids.insert(&provider.id) {
            report(
                &format!("providers[{index}].id"),
                "workspace_reading_assistance_provider",
                Some(provider.id.clone()),
            );
        }
        if !label(&provider.label) {
            report(
                &format!("providers[{index}].label"),
                "workspace_reading_assistance_copy",
                Some(provider.id.clone()),
            );
        }
        if !parameter(&provider.query_parameter) {
            report(
                &format!("providers[{index}].query_parameter"),
                "workspace_reading_assistance_provider",
                Some(provider.id.clone()),
            );
        }
        if !endpoint(&provider.endpoint, &provider.query_parameter) {
            report(
                &format!("providers[{index}].endpoint"),
                "workspace_reading_assistance_provider",
                Some(provider.id.clone()),
            );
        }
    }
}
