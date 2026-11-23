//! Resolve copy independently while keeping destinations and handoffs explicit.

use super::SiteChrome;
use crate::toml::{Limits, PathSegment};
use crate::workspace::composition::{
    Error, Key, SelectionField, SelectionOrigin, SelectionScope, charge,
};
use serde::Serialize;
use std::collections::BTreeMap;

/// Effective copy with an origin for each declared label and non-copy selection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct ResolvedChrome {
    value: SiteChrome,
    origins: BTreeMap<String, SelectionOrigin>,
}

fn pointer(parts: impl IntoIterator<Item = impl AsRef<str>>) -> String {
    parts.into_iter().fold(String::new(), |mut path, part| {
        path.push('/');
        path.push_str(&part.as_ref().replace('~', "~0").replace('/', "~1"));
        path
    })
}

impl ResolvedChrome {
    /// Merged declared values; rendering separately requires complete visible controls.
    pub fn value(&self) -> &SiteChrome {
        &self.value
    }

    /// One origin only when every retained field came from the same declaration.
    pub fn origin(&self) -> Option<&SelectionOrigin> {
        self.field_origin(&[])
    }

    /// Origin of a scalar, or of a table whose retained fields share one origin.
    /// Mixed tables and undeclared fields have no fabricated origin.
    pub fn field_origin(&self, path: &[PathSegment]) -> Option<&SelectionOrigin> {
        let parts: Vec<_> = path
            .iter()
            .map(|part| match part {
                PathSegment::Key(key) => key.clone(),
                PathSegment::Index(index) => index.to_string(),
            })
            .collect();
        let key = pointer(&parts);
        let prefix = format!("{key}/");
        let mut matches = self.origins.iter().filter_map(|(field, origin)| {
            (field == &key || field.starts_with(&prefix) || key.starts_with(&format!("{field}/")))
                .then_some(origin)
        });
        let first = matches.next()?;
        matches.all(|origin| origin == first).then_some(first)
    }

    fn empty() -> Self {
        Self {
            value: SiteChrome::default(),
            origins: BTreeMap::new(),
        }
    }

    fn set_label(&mut self, field: &str, value: &str, origin: SelectionOrigin) {
        *self
            .value
            .label_slot(field)
            .expect("known typed copy field") = Some(value.to_owned());
        self.origins.insert(pointer(field.split('.')), origin);
    }

    fn overlay(&mut self, copy: &SiteChrome, scope: SelectionScope) {
        let origin = SelectionOrigin {
            scope,
            field: SelectionField::Chrome {},
        };
        for (field, value) in copy.copy_fields() {
            self.set_label(field, value, origin.clone());
        }
        for (key, value) in &copy.regions {
            self.value.regions.insert(key.clone(), value.clone());
            self.origins
                .insert(pointer(["regions", key.as_str()]), origin.clone());
        }
        if let Some(search) = &copy.shell.search {
            let resolved = self.value.shell.search.get_or_insert_default();
            if let Some(enabled) = search.enabled {
                resolved.enabled = Some(enabled);
                self.origins
                    .insert("/shell/search/enabled".into(), origin.clone());
            }
            if let Some(destination) = &search.no_script_destination {
                resolved.no_script_destination = Some(destination.clone());
                self.origins
                    .insert("/shell/search/no_script_destination".into(), origin.clone());
            }
        }
        // A local chrome declaration keeps the existing whole-handoff replacement:
        // leaving it out cannot silently restore another layer's external endpoints.
        if matches!(origin.scope, SelectionScope::Site(_)) || copy.reading_assistance.is_some() {
            self.value.reading_assistance = copy.reading_assistance.clone();
            self.origins.remove("/reading_assistance");
            if copy.reading_assistance.is_some() {
                self.origins.insert("/reading_assistance".into(), origin);
            }
        }
    }
}

fn charge_copy(
    copy: &SiteChrome,
    limits: Limits,
    used: &mut usize,
    text: &mut usize,
) -> Result<(), Error> {
    charge(used, 1, limits.nodes, "composition selections")?;
    for (_, value) in copy.copy_fields() {
        charge(
            text,
            value.len(),
            limits.source_bytes,
            "composition text bytes",
        )?;
    }
    for (region, value) in &copy.regions {
        charge(used, 1, limits.nodes, "composition selections")?;
        for length in [region.as_str().len(), value.len()] {
            charge(text, length, limits.source_bytes, "composition text bytes")?;
        }
    }
    if let Some(destination) = copy
        .shell
        .search
        .as_ref()
        .and_then(|search| search.no_script_destination.as_ref())
    {
        charge(
            text,
            destination.as_str().len(),
            limits.source_bytes,
            "composition text bytes",
        )?;
    }
    if let Some(assistance) = &copy.reading_assistance {
        charge(
            used,
            1 + assistance.providers.len(),
            limits.nodes,
            "composition selections",
        )?;
        for value in assistance.retained_text() {
            charge(
                text,
                value.len(),
                limits.source_bytes,
                "composition text bytes",
            )?;
        }
    }
    Ok(())
}

pub(in crate::workspace::composition) fn resolve(
    defaults: Option<&SiteChrome>,
    local: Option<&SiteChrome>,
    site: &Key,
    limits: Limits,
    used: &mut usize,
    text: &mut usize,
) -> Result<Option<ResolvedChrome>, Error> {
    if defaults.is_none() && local.is_none() {
        return Ok(None);
    }
    // Charge declared layers before allocating the merged copy or origin map.
    for copy in defaults.into_iter().chain(local) {
        charge_copy(copy, limits, used, text)?;
    }
    charge(
        text,
        site.as_str().len(),
        limits.source_bytes,
        "composition text bytes",
    )?;
    let mut resolved = ResolvedChrome::empty();
    if let Some(copy) = defaults {
        resolved.overlay(copy, SelectionScope::Workspace);
    }
    if let Some(copy) = local {
        resolved.overlay(copy, SelectionScope::Site(site.clone()));
    }
    Ok(Some(resolved))
}

pub(in crate::workspace::composition) fn inherit_theme(
    selected: Option<&ResolvedChrome>,
    theme: &crate::theme_graph::EffectiveTheme,
    limits: Limits,
) -> Result<Option<ResolvedChrome>, Error> {
    if selected.is_none() && theme.labels().is_empty() {
        return Ok(None);
    }
    let mut nodes = 0;
    let mut text = 0;
    if let Some(selected) = selected {
        charge_copy(selected.value(), limits, &mut nodes, &mut text)?;
    }
    let label_path = |field: &str| {
        if let Some(region) = field.strip_prefix("regions.") {
            pointer(["regions", region])
        } else {
            pointer(field.split('.'))
        }
    };
    for (field, default) in theme.labels().iter().filter(|(field, _)| {
        !selected.is_some_and(|selected| selected.origins.contains_key(&label_path(field)))
    }) {
        charge(&mut nodes, 1, limits.nodes, "composition selections")?;
        for length in [
            field.len(),
            default.value.len(),
            default.package.as_str().len(),
        ] {
            charge(
                &mut text,
                length,
                limits.source_bytes,
                "composition text bytes",
            )?;
        }
    }
    let mut resolved = selected.cloned().unwrap_or_else(ResolvedChrome::empty);
    for (field, default) in theme.labels() {
        let path = label_path(field);
        if resolved.origins.contains_key(&path) {
            continue;
        }
        let origin = SelectionOrigin {
            scope: SelectionScope::Theme(default.package.clone()),
            field: SelectionField::Chrome {},
        };
        if let Some(region) = field.strip_prefix("regions.") {
            resolved.value.regions.insert(
                region.parse().expect("parsed Theme region key"),
                default.value.clone(),
            );
            resolved.origins.insert(path, origin);
        } else {
            resolved.set_label(field, &default.value, origin);
        }
    }
    if resolved.value.regions.len() > 256 {
        return Err(Error::Limit("chrome regions"));
    }
    Ok(Some(resolved))
}
