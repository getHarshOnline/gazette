//! Native serializing-HTML contexts for Mintlify's directory SDK input.
//!
//! Adjacent italic type runs are joined for the provider’s Markdown conversion.
//! The captured native artifact remains unchanged.
//! Runtime caches are deliberately outside the target; the caller retains the exact
//! complete capture. Context structure is checked, not hosted rendering fidelity.

use crate::{Error, invalid, output::Output};
use gazette_core::{
    documentation::projection::{LinkProjection as SiteLinks, PageKind},
    documentation::{Artifact, Selection},
    source::{Content, EntryPath},
};
use serde_json::Value;
use std::collections::BTreeMap;

mod emphasis;
mod links;

pub(super) fn package(
    artifact: &Artifact<'_>,
    target: &str,
    route: &str,
    output: &mut Output,
    site_links: &mut SiteLinks,
    catalog: crate::catalog::Scope<'_>,
) -> Result<(), Error> {
    let Selection::Directory { path } = artifact.selection else {
        return Err(invalid(
            target,
            "Sphinx requires an explicitly selected JSON directory",
        ));
    };
    artifact.selection.content(&artifact.tree)?;
    let prefix = path.as_ref().map(|path| format!("{path}/"));
    let mut files = BTreeMap::new();
    let mut retained = Vec::new();
    output.directory(target)?;
    for entry in artifact.tree.inventory().entries() {
        let name = match &prefix {
            Some(prefix) => match entry.path.as_str().strip_prefix(prefix) {
                Some(name) => name,
                None => continue,
            },
            None => entry.path.as_str(),
        };
        if runtime(name) {
            continue;
        }
        let destination = format!("{target}/{name}");
        match entry.content {
            Content::Directory => output.directory(&destination)?,
            Content::File { .. } => {
                if !supported(name) {
                    return Err(invalid(
                        name,
                        "unclassified Sphinx companion; target inclusion requires qualification",
                    ));
                }
                let bytes = artifact
                    .tree
                    .file(&entry.path)
                    .ok_or_else(|| invalid(name, "missing captured companion"))?;
                if bytes.len() as u64 > output.file_ceiling() {
                    return Err(invalid(name, "Sphinx input exceeds target byte limit"));
                }
                files.insert(name, bytes);
                retained.push((destination, name, bytes));
            }
        }
    }
    let pages = validate(&files)?;
    for (document, anchors) in pages.pages {
        let page_route = if document == pages.root {
            route.to_owned()
        } else {
            format!("{route}/{document}")
        };
        let source = format!("{document}.fjson");
        let page = context(
            files
                .get(source.as_str())
                .ok_or_else(|| invalid(&source, "catalog page context absent"))?,
            &source,
        )?;
        if !provider_page(&document, &page) {
            continue;
        }
        site_links.register_provider(&page_route, PageKind::QualifiedVirtual, Some(anchors))?;
        let title = page
            .get("title")
            .and_then(Value::as_str)
            .map(|title| links::reading_text(title, &source, catalog.limits.bytes))
            .transpose()?;
        let text = page
            .get("body")
            .and_then(Value::as_str)
            .map(|body| links::reading_text(body, &source, catalog.limits.bytes))
            .transpose()?;
        catalog.virtual_page(
            site_links,
            &page_route,
            title,
            text,
            &[
                &format!("{target}/{source}"),
                &format!("{target}/globalcontext.json"),
            ],
        )?;
    }
    for (path, name, bytes) in retained {
        if name.ends_with(".fjson") && (!asset(name) || name == format!("{}.fjson", pages.root)) {
            let mut page = context(bytes, name)?;
            let mut changed = false;
            for field in ["body", "toc", "title"] {
                if let Some(text) = page.get(field).and_then(Value::as_str)
                    && let Some(normalized) =
                        emphasis::normalize(text, name, output.file_ceiling())?
                {
                    page[field] = Value::String(normalized);
                    changed = true;
                }
            }
            if changed {
                let converted = crate::output::json(&page, name, output.file_ceiling())?;
                output.add_owned(&path, converted)?;
                continue;
            }
        }
        output.add(&path, bytes)?;
    }
    Ok(())
}

fn runtime(name: &str) -> bool {
    matches!(
        name,
        "environment.pickle" | "last_build" | ".buildinfo" | ".doctrees"
    ) || name.starts_with(".doctrees/")
}

fn supported(name: &str) -> bool {
    matches!(
        name,
        "globalcontext.json" | "searchindex.json" | "objects.inv"
    ) || name.ends_with(".fjson")
        || asset(name)
}

fn asset(name: &str) -> bool {
    ["_images/", "_downloads/", "_static/", "_sources/"]
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

fn context(bytes: &[u8], name: &str) -> Result<Value, Error> {
    let value = gazette_core::json::parse(bytes)?;
    if !value.is_object() {
        return Err(invalid(name, "Sphinx context must be a JSON object"));
    }
    Ok(value)
}

// Mintlify's Sphinx loader omits these utility contexts, underscore paths and
// empty bodies. Retain their artifacts without inventing reader-facing routes.
fn provider_page(document: &str, page: &Value) -> bool {
    !matches!(document, "genindex" | "search" | "py-modindex")
        && !document.split('/').any(|part| part.starts_with('_'))
        && page["body"]
            .as_str()
            .is_some_and(|body| !body.trim().is_empty())
}

struct Catalog {
    root: String,
    pages: BTreeMap<String, std::collections::BTreeSet<String>>,
}

fn validate(files: &BTreeMap<&str, &[u8]>) -> Result<Catalog, Error> {
    let global = files
        .get("globalcontext.json")
        .ok_or_else(|| invalid("globalcontext.json", "missing Sphinx global context"))?;
    let global = context(global, "globalcontext.json")?;
    let root = global
        .get("root_doc")
        .or_else(|| global.get("master_doc"))
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("globalcontext.json", "missing Sphinx root document"))?;
    let _: EntryPath = root.parse()?;
    if global.get("root_doc").is_some()
        && global.get("master_doc").is_some()
        && global["root_doc"] != global["master_doc"]
    {
        return Err(invalid(
            "globalcontext.json",
            "conflicting Sphinx root documents",
        ));
    }
    let root_file = format!("{root}.fjson");
    if !files.contains_key(root_file.as_str()) {
        return Err(invalid(root_file, "missing Sphinx root page"));
    }
    let search = files
        .get("searchindex.json")
        .ok_or_else(|| invalid("searchindex.json", "missing Sphinx search index"))?;
    context(search, "searchindex.json")?;
    let mut pages = BTreeMap::new();
    for (name, bytes) in files {
        let Some(document) = name.strip_suffix(".fjson") else {
            continue;
        };
        // Asset names are opaque even if an asset happens to end in .fjson.
        if asset(name) && *name != root_file {
            continue;
        }
        let page = context(bytes, name)?;
        if page.get("current_page_name").and_then(Value::as_str) != Some(document) {
            return Err(invalid(
                *name,
                "Sphinx page name differs from its captured path",
            ));
        }
        for field in ["body", "title", "toc"] {
            if page.get(field).is_some_and(|value| !value.is_string()) {
                return Err(invalid(*name, "invalid Sphinx page text field"));
            }
        }
        // Generated domain/search/index contexts may omit body/title. The root
        // still must be an ordinary document, not a substituted auxiliary page.
        let ordinary = page["body"].is_string() && page["title"].is_string();
        if *name == root_file && (!ordinary || !provider_page(document, &page)) {
            return Err(invalid(
                *name,
                "Sphinx root must contain document body and title",
            ));
        }
        if !(ordinary || auxiliary(document, &page)) {
            return Err(invalid(*name, "unsupported Sphinx auxiliary page context"));
        }
        pages.insert(document, page);
    }
    Ok(Catalog {
        root: root.to_owned(),
        pages: links::validate(root, &pages, files)?,
    })
}

fn auxiliary(document: &str, page: &Value) -> bool {
    document == "search"
        || (matches!(document, "genindex" | "genindex-all")
            && page["genindexentries"].is_array()
            && page["genindexcounts"].is_array()
            && page["split_index"].is_boolean())
        || (document.starts_with("genindex-")
            && page["key"].is_string()
            && page["entries"].is_array()
            && page["count"].is_u64()
            && page["genindexentries"].is_array())
        || (page["indextitle"].is_string()
            && page["content"].is_array()
            && page["collapse_index"].is_boolean())
}
