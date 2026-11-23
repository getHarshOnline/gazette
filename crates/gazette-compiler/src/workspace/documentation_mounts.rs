//! Explicit Theme documentation mounts, independent from Mintlify repositories and projects.

use std::collections::{BTreeMap, BTreeSet};

use gazette_core::{
    publication::{ContentLifecycle, LanguageTag, routing::RoutePath},
    source::Sha256Digest,
};
use serde::{Deserialize, Serialize};

use super::{ContentBinding, Diagnostic, Key, NativeMarkdown, Site, Source, Workspace};

/// Theme-rendered native documentation selections; no implicit source discovery.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocumentationMounts {
    /// Explicit mounts in declaration order; disabled rows remain inspectable.
    #[serde(default, deserialize_with = "crate::tables::many")]
    pub mounts: Vec<DocumentationMount>,
}

/// One documentation source associated with an existing publication landing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocumentationMount {
    /// Exact configured publication, never inferred from a repository.
    pub site: Key,
    /// Same-site routed landing; this association creates no semantic subject or residence.
    pub landing: Key,
    /// Explicit public documentation label, not an identity key.
    pub name: String,
    /// Exact retained source declaration; this selects no private siblings.
    pub source: Key,
    /// Directory address used only by plain-document lowering.
    pub route_prefix: RoutePath,
    /// Explicit materialization switch, independent from approval.
    pub render: bool,
    /// Independent automatic-index selection; requires rendering.
    pub discoverable: bool,
    /// Required for rendered mounts, with no invented lifecycle or language.
    #[serde(
        default,
        deserialize_with = "crate::tables::optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub lowering: Option<DocumentationLowering>,
}

/// Explicit defaults applied only to unframed Markdown in this mount.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocumentationLowering {
    /// Exact content state, not a project lifecycle or publication permission.
    pub lifecycle: ContentLifecycle,
    /// Optional validated language; no detection or site fallback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<LanguageTag>,
    /// Existing shared Markdown syntax profile.
    #[serde(default)]
    pub markdown_profile: gazette_core::markdown::Profile,
}

/// A validated mount and its one normalized native-input binding.
#[derive(Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct ResolvedDocumentationMount {
    /// Exact configured choices, including disabled state.
    #[serde(flatten)]
    pub declaration: DocumentationMount,
    #[serde(skip)]
    #[schemars(skip)]
    pub(super) binding: ContentBinding,
    #[serde(skip)]
    #[schemars(skip)]
    pub(super) native_markdown: Option<NativeMarkdown>,
}

pub(super) fn resolve(
    configured: DocumentationMounts,
    sites: &BTreeMap<Key, Site>,
    sources: &BTreeMap<Key, Source>,
    errors: &mut Vec<Diagnostic>,
) -> Vec<ResolvedDocumentationMount> {
    let mut identities = BTreeSet::new();
    let mut result = Vec::new();
    for (index, mount) in configured.mounts.into_iter().enumerate() {
        let field = format!("documentation.mounts[{index}]");
        for (suffix, key, exists) in [
            ("site", &mount.site, sites.contains_key(&mount.site)),
            ("source", &mount.source, sources.contains_key(&mount.source)),
        ] {
            if !exists {
                super::resolve::report(
                    errors,
                    "workspace_reference",
                    &format!("{field}.{suffix}"),
                    Some(key),
                );
            }
        }
        if !identities.insert((mount.site.clone(), mount.landing.clone())) {
            super::resolve::report(
                errors,
                "workspace_documentation_mount_duplicate",
                &field,
                Some(&mount.landing),
            );
        }
        if mount.name.trim().is_empty()
            || mount.name.len() > 256
            || mount.name.chars().any(char::is_control)
        {
            super::resolve::report(
                errors,
                "workspace_documentation_mount_name",
                &format!("{field}.name"),
                Some(&mount.landing),
            );
        }
        if !mount.route_prefix.as_str().ends_with('/') {
            super::resolve::report(
                errors,
                "workspace_native_route",
                &format!("{field}.route_prefix"),
                Some(&mount.landing),
            );
        }
        if mount.discoverable && !mount.render {
            super::resolve::report(
                errors,
                "workspace_documentation_mount_discovery",
                &field,
                Some(&mount.landing),
            );
        }
        if mount.render && mount.lowering.is_none() {
            super::resolve::report(
                errors,
                "workspace_documentation_mount_lowering",
                &format!("{field}.lowering"),
                Some(&mount.landing),
            );
        }
        let coordinate = format!(
            "gazette-documentation-mount/1\0{}\0{}\0{}",
            mount.site, mount.landing, mount.source
        );
        let target = format!(
            "__documentation/{}",
            Sha256Digest::of(coordinate.as_bytes())
        )
        .parse()
        .expect("fixed digest forms a confined logical target");
        let binding = ContentBinding {
            source: mount.source.clone(),
            target,
            markdown_profile: mount
                .lowering
                .as_ref()
                .map_or(Default::default(), |lowering| lowering.markdown_profile),
            static_assets: None,
            retained_only: Default::default(),
        };
        let native_markdown = mount.lowering.as_ref().map(|lowering| NativeMarkdown {
            route_prefix: mount.route_prefix.clone(),
            lifecycle: lowering.lifecycle,
            language: lowering.language.clone(),
        });
        result.push(ResolvedDocumentationMount {
            declaration: mount,
            binding,
            native_markdown,
        });
    }
    result.sort_by(|left, right| {
        (&left.declaration.site, &left.declaration.landing)
            .cmp(&(&right.declaration.site, &right.declaration.landing))
    });
    result
}

impl Workspace {
    /// All explicit Theme documentation mounts, including disabled selections.
    pub fn documentation_mounts(&self) -> &[ResolvedDocumentationMount] {
        &self.documentation_mounts
    }
}
