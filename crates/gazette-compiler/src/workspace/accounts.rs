//! Public account declarations keep ownership separate from publication display.

use std::collections::{BTreeMap, BTreeSet};

use gazette_core::publication::Principal;
use serde::{Deserialize, Serialize};

use super::{Brand, Diagnostic, Key, SocialProfile, resolve::report};

/// One public account, with an optional owner whose identity links may include it.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct AccountDeclaration {
    /// Stable account key, selected independently by each publication.
    pub id: Key,
    /// Platform label used by the publication's social-link presentation.
    pub platform: String,
    /// Public handle; supported standard platform addresses can be derived from it.
    #[serde(default)]
    pub handle: Option<String>,
    /// Explicit public HTTP(S) profile address, required for other platforms.
    #[serde(default)]
    pub url: Option<String>,
    /// Declared owning principal. Omission leaves ownership unresolved.
    #[serde(default)]
    pub owner: Option<Key>,
    /// Optional platform channel identifier, separate from ownership and access.
    #[serde(default)]
    pub channel_id: Option<String>,
}

/// A validated account with its resolved public address and declared owner.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct OnlineAccount {
    /// Stable workspace account identity.
    pub id: Key,
    /// Platform label, independent from the owning principal's kind.
    pub platform: String,
    /// Explicit owner only; a publication displaying the account supplies no owner.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<Key>,
    /// Public address and platform metadata, shared with the publication view.
    #[serde(flatten)]
    pub profile: SocialProfile,
}

fn plain(value: &str) -> bool {
    !value.is_empty() && !value.chars().any(|c| c.is_whitespace() || c.is_control())
}

fn address(platform: &str, handle: Option<&str>) -> Option<String> {
    let base = match platform {
        "github" => "https://github.com/",
        "x" => "https://x.com/",
        "youtube" => "https://www.youtube.com/",
        _ => return None,
    };
    let handle = handle?;
    let handle = handle.strip_prefix('@').unwrap_or(handle);
    if handle.is_empty()
        || matches!(handle, "." | "..")
        || !handle
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        return None;
    }
    let mut url = url::Url::parse(base).expect("fixed platform URL");
    let segment = if platform == "youtube" {
        format!("@{handle}")
    } else {
        handle.to_owned()
    };
    url.path_segments_mut()
        .expect("platform URL has a path")
        .pop_if_empty()
        .push(&segment);
    Some(url.into())
}

pub(super) fn resolve(
    declarations: Vec<AccountDeclaration>,
    principals: &BTreeMap<Key, Principal>,
    errors: &mut Vec<Diagnostic>,
) -> BTreeMap<Key, OnlineAccount> {
    let mut accounts = BTreeMap::new();
    let mut identities = BTreeSet::new();
    for account in declarations {
        let field = format!("accounts[{:?}]", account.id.as_str());
        if let Some(owner) = &account.owner
            && !principals.contains_key(owner)
        {
            report(
                errors,
                "workspace_reference",
                &format!("{field}.owner"),
                Some(owner),
            );
        }
        if account.platform.is_empty()
            || !account
                .platform
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
            || [&account.handle, &account.channel_id]
                .into_iter()
                .flatten()
                .any(|value| !plain(value))
        {
            report(
                errors,
                "workspace_account_profile",
                &field,
                Some(&account.id),
            );
        }
        let url = account
            .url
            .or_else(|| address(&account.platform, account.handle.as_deref()));
        let Some(url) = url.filter(|url| super::repositories::valid(url)) else {
            report(
                errors,
                "workspace_account_url",
                &format!("{field}.url"),
                Some(&account.id),
            );
            continue;
        };
        if !identities.insert((account.platform.clone(), url.clone())) {
            report(
                errors,
                "workspace_account_duplicate_profile",
                &field,
                Some(&account.id),
            );
        }
        let id = account.id.clone();
        if accounts
            .insert(
                id.clone(),
                OnlineAccount {
                    id: account.id,
                    platform: account.platform,
                    owner: account.owner,
                    profile: SocialProfile {
                        url,
                        handle: account.handle,
                        channel_id: account.channel_id,
                    },
                },
            )
            .is_some()
        {
            report(errors, "workspace_duplicate", "accounts", Some(&id));
        }
    }
    accounts
}

pub(super) fn select(
    accounts: &BTreeMap<Key, OnlineAccount>,
    brands: &mut BTreeMap<Key, Brand>,
    errors: &mut Vec<Diagnostic>,
) {
    for brand in brands.values_mut() {
        for publication in &mut brand.publications {
            let field = format!(
                "brands[{:?}].publications[{:?}].accounts",
                brand.id.as_str(),
                publication.site.as_str()
            );
            validate_selection(
                accounts,
                &publication.accounts,
                publication.social.keys().map(String::as_str),
                &field,
                errors,
            );
            for id in &publication.accounts {
                let Some(account) = accounts.get(id) else {
                    continue;
                };
                publication
                    .social
                    .entry(account.platform.clone())
                    .or_insert_with(|| account.profile.clone());
            }
        }
    }
}

pub(super) fn validate_selection<'a>(
    accounts: &'a BTreeMap<Key, OnlineAccount>,
    selection: &[Key],
    inline_platforms: impl Iterator<Item = &'a str>,
    field: &str,
    errors: &mut Vec<Diagnostic>,
) {
    let mut selected = BTreeSet::new();
    let mut platforms: BTreeSet<_> = inline_platforms.collect();
    for id in selection {
        if !selected.insert(id) {
            report(
                errors,
                "workspace_account_selection_duplicate",
                field,
                Some(id),
            );
            continue;
        }
        let Some(account) = accounts.get(id) else {
            report(errors, "workspace_reference", field, Some(id));
            continue;
        };
        if !platforms.insert(account.platform.as_str()) {
            report(errors, "workspace_account_platform", field, Some(id));
        }
    }
}

pub(super) fn identities(
    accounts: &BTreeMap<Key, OnlineAccount>,
    principals: &mut BTreeMap<Key, Principal>,
) {
    for account in accounts.values() {
        if let Some(principal) = account
            .owner
            .as_ref()
            .and_then(|owner| principals.get_mut(owner))
            && !principal.same_as.contains(&account.profile.url)
        {
            principal.same_as.push(account.profile.url.clone());
        }
    }
}
