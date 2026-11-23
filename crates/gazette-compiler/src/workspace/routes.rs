//! One route compilation for every consumer; no renderer-side URL reconstruction.

use std::collections::{BTreeMap, BTreeSet};

use gazette_core::{
    publication::{
        Key,
        content::{Diagnostic, Error, Limits},
    },
    source::EntryPath,
};
use serde::Serialize;

use super::ResolvedContent;

/// Resolved redirect address. Its target is the enclosing record's canonical URL;
/// no independent target, redirect chain or publication effect is invented.
#[derive(Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct ResolvedAlias {
    /// Absolute address using the selected serving base (configured or ephemeral preview).
    pub url: String,
    /// Literal file beneath this site's output root, without its deployment prefix.
    pub output: EntryPath,
}

/// A redirect presentation of the enclosing canonical object on another site.
/// No copied content identity, source, authorship or renderer-side URL lookup.
#[derive(Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct ResolvedMirror {
    /// Exact configured serving site, distinct from the canonical object's home.
    pub site: Key,
    /// Absolute serving address using that site's selected base.
    pub url: String,
    /// Literal file beneath the serving site's output root.
    pub output: EntryPath,
}

/// Complete explicit route for one content identity, before disclosure admission.
#[derive(Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct ResolvedRoute {
    /// Original's canonical public address, preserved when serving URLs become local.
    pub public_url: String,
    /// This record's serving address, distinct from a syndicated copy's origin.
    pub url: String,
    /// Explicit original's address, or this record's own address when canonical. In an
    /// explicitly marked nonrelease preview this is a session serving address;
    /// the original publication base remains in the immutable compilation.
    pub canonical_url: String,
    /// Literal site-relative output file; native filesystem validation is still required.
    pub output: EntryPath,
    /// All declared aliases in normalized route order, with no silent omissions.
    pub aliases: Vec<ResolvedAlias>,
    /// Explicit foreign redirect presentations; every target is this exact route.
    pub mirrors: Vec<ResolvedMirror>,
}

pub(super) fn compile(
    content: &ResolvedContent<'_>,
    limits: Limits,
) -> Result<BTreeMap<Key, ResolvedRoute>, Error> {
    let mut result = BTreeMap::new();
    let mut local: BTreeMap<&Key, BTreeMap<String, &Key>> = BTreeMap::new();
    let mut public = BTreeMap::new();
    let mut errors = BTreeSet::new();
    let mut text = 0usize;
    for node in content.graph().nodes() {
        let Some(route) = &node.route else { continue };
        let Some(base) = content.base_url(&node.site) else {
            report(
                &mut errors,
                "content_route_base",
                &node.id,
                Some(&node.site),
            );
            continue;
        };
        let mut resolved = Vec::new();
        let mut mirrors = Vec::new();
        let addresses = std::iter::once((&node.site, &route.path, false))
            .chain(route.aliases.iter().map(|path| (&node.site, path, false)))
            .chain(
                route
                    .mirrors
                    .iter()
                    .map(|mirror| (&mirror.site, &mirror.path, true)),
            );
        for (site, path, mirrored) in addresses {
            if mirrored && site == &node.site {
                report(&mut errors, "content_mirror_home", &node.id, Some(site));
                continue;
            }
            let base = if mirrored {
                if content.compilation().workspace.site(site).is_none() {
                    report(&mut errors, "content_mirror_site", &node.id, Some(site));
                    continue;
                }
                let Some(base) = content.base_url(site) else {
                    report(&mut errors, "content_route_base", &node.id, Some(site));
                    continue;
                };
                base
            } else {
                base
            };
            // Charge before expanding a long base once per object/alias. Source
            // text limits alone do not bound this derived amplification.
            text = text
                .checked_add(base.as_str().len())
                .and_then(|n| n.checked_add(path.as_str().len()))
                .and_then(|n| n.checked_add(path.output().as_str().len()))
                .and_then(|n| n.checked_add("index.html".len()))
                .filter(|&n| n <= limits.text_bytes)
                .ok_or(Error::Limit("route text bytes"))?;
            let url = base.resolve(path);
            let served_file = base.resolve_output(path);
            if let Some(previous) = public.insert(served_file, &node.id) {
                report(
                    &mut errors,
                    "content_route_collision",
                    &node.id,
                    Some(previous),
                );
            }
            if let Some(previous) = local
                .entry(site)
                .or_default()
                .insert(path.output().as_str().to_owned(), &node.id)
            {
                report(
                    &mut errors,
                    "content_route_collision",
                    &node.id,
                    Some(previous),
                );
            }
            if mirrored {
                mirrors.push(ResolvedMirror {
                    site: site.clone(),
                    url,
                    output: path.output().clone(),
                });
            } else {
                resolved.push(ResolvedAlias {
                    url,
                    output: path.output().clone(),
                });
            }
        }
        let mut resolved = resolved.into_iter();
        if let Some(primary) = resolved.next() {
            let public_url = content
                .compilation()
                .workspace
                .public_base_url(&node.site)
                .expect("configured publication base")
                .resolve(&route.path);
            text = text
                .checked_add(primary.url.len())
                .and_then(|n| n.checked_add(public_url.len()))
                .filter(|&n| n <= limits.text_bytes)
                .ok_or(Error::Limit("route text bytes"))?;
            result.insert(
                node.id.clone(),
                ResolvedRoute {
                    public_url,
                    url: primary.url.clone(),
                    canonical_url: primary.url,
                    output: primary.output,
                    aliases: resolved.collect(),
                    mirrors,
                },
            );
        }
    }
    // Canonical references are direct and validated by the content graph. Resolve
    // after the complete route set, never by insertion order or a fallback URL.
    for node in content.graph().nodes() {
        let Some(canonical) = node
            .route
            .as_ref()
            .and_then(|route| route.canonical.as_ref())
        else {
            continue;
        };
        let Some(target) = result.get(canonical) else {
            report(
                &mut errors,
                "content_canonical_reference",
                &node.id,
                Some(canonical),
            );
            continue;
        };
        text = text
            .checked_add(target.url.len())
            .and_then(|n| n.checked_add(target.public_url.len()))
            .filter(|&n| n <= limits.text_bytes)
            .ok_or(Error::Limit("route text bytes"))?;
        let url = target.url.clone();
        let public_url = target.public_url.clone();
        if let Some(route) = result.get_mut(&node.id) {
            route.canonical_url = url;
            route.public_url = public_url;
        }
    }
    // Check every parent, not just lexical neighbours: an interposed sibling such
    // as `a-1` must not hide the conflict between file `a` and file `a/b`.
    for files in local.values() {
        for (path, id) in files {
            for (offset, _) in path.match_indices('/') {
                if let Some(previous) = files.get(&path[..offset]) {
                    report(&mut errors, "content_route_collision", id, Some(previous));
                }
            }
        }
    }
    for (url, id) in &public {
        for (offset, _) in url.match_indices('/') {
            if let Some(previous) = public.get(&url[..offset]) {
                report(&mut errors, "content_route_collision", id, Some(previous));
            }
        }
    }
    if !errors.is_empty() {
        let mut diagnostic_bytes = 0usize;
        let diagnostics = errors
            .into_iter()
            .map(|(code, source, target)| {
                diagnostic_bytes = diagnostic_bytes
                    .checked_add(source.as_str().len())
                    .and_then(|n| n.checked_add(target.map_or(0, |key| key.as_str().len())))
                    .filter(|&n| n <= limits.text_bytes)
                    .ok_or(Error::Limit("route diagnostic bytes"))?;
                Ok(Diagnostic {
                    code,
                    source: source.clone(),
                    target: target.cloned(),
                })
            })
            .collect::<Result<_, Error>>()?;
        return Err(Error::Invalid(diagnostics));
    }
    Ok(result)
}

fn report<'a>(
    errors: &mut BTreeSet<(&'static str, &'a Key, Option<&'a Key>)>,
    code: &'static str,
    source: &'a Key,
    target: Option<&'a Key>,
) {
    // Repeated aliases cannot multiply copies of a long object identity.
    errors.insert((code, source, target));
}
