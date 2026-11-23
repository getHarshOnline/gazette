//! Native formatted Rust references adapted through one retained page/link model.

mod data;
mod emit;
mod html;
mod implementors;
mod links;
mod redirect;
mod type_impls;

use std::collections::{BTreeMap, BTreeSet};

use crate::{Error, configuration::Page as Navigation, invalid, output::Output};
use gazette_core::{
    documentation::projection::{LinkProjection as SiteLinks, PageKind},
    documentation::{Artifact, Selection},
    source::{Content, EntryPath, Limits},
};
use html::Page;

pub(super) fn package(
    artifact: &Artifact<'_>,
    route: &str,
    output: &mut Output,
    site_links: &mut SiteLinks,
    limits: Limits,
    catalog: crate::catalog::Scope<'_>,
) -> Result<Vec<Navigation>, Error> {
    let Selection::Directory { path } = artifact.selection else {
        return Err(invalid(
            route,
            "Rustdoc HTML requires a selected native directory",
        ));
    };
    artifact.selection.content(&artifact.tree)?;
    let prefix = path
        .as_ref()
        .map_or(String::new(), |path| format!("{path}/"));
    let mut pages = BTreeMap::new();
    let mut redirects = BTreeMap::new();
    let mut budget = Budget {
        bytes: 0,
        pages: 0,
        limits,
    };
    for entry in artifact.tree.inventory().entries() {
        let Some(relative) = entry.path.as_str().strip_prefix(&prefix) else {
            continue;
        };
        if !matches!(entry.content, Content::File { .. }) || !relative.ends_with(".html") {
            continue;
        }
        // These generated application-help pages contain no library reference.
        if prefix.is_empty() && matches!(relative, "settings.html" | "help.html") {
            continue;
        }
        let bytes = read_bytes(artifact, &entry.path, &mut budget)?;
        if let Some(target) = redirect::target(bytes, &entry.path)? {
            redirects.insert(entry.path.clone(), target);
            continue;
        }
        let page = html::parse(
            bytes,
            entry.path.as_str(),
            usize::try_from(limits.file_bytes).unwrap_or(usize::MAX),
        )?;
        pages.insert(entry.path.to_string(), page);
    }
    for (origin, page) in &mut pages {
        if let Some((source, self_path)) = page.type_script.take() {
            let links::Destination::Local {
                path,
                fragment: None,
            } = links::resolve(origin, &source)?
            else {
                return Err(invalid(
                    origin,
                    "Rustdoc type implementation data is not captured",
                ));
            };
            let rows = type_impls::select(
                read_bytes(artifact, &path, &mut budget)?,
                &page.crate_name,
                &self_path,
                path.as_str(),
            )?;
            html::append_implementations(
                page,
                rows,
                origin,
                path.as_str(),
                usize::try_from(limits.file_bytes).unwrap_or(usize::MAX),
            )?;
        }
    }
    for (origin, target) in &redirects {
        if pages.get(target.as_str()).is_none_or(|page| page.source) {
            return Err(invalid(
                origin.to_string(),
                "Rustdoc redirect has no selected API target",
            ));
        }
    }
    if pages.is_empty() || pages.values().all(|page| page.source) {
        return Err(invalid(route, "Rustdoc selection contains no API pages"));
    }
    let mut crates = BTreeMap::new();
    for page in pages.values().filter(|page| !page.source) {
        let value = (page.generator.clone(), page.version.clone());
        if crates
            .insert(page.crate_name.clone(), value.clone())
            .is_some_and(|prior| prior != value)
        {
            return Err(invalid(
                route,
                "conflicting native Rustdoc crate/version metadata",
            ));
        }
    }
    // Resolve only explicitly reached source-page companions outside the selection.
    // No source is reopened, and no sibling API crate is silently admitted.
    let mut pending: BTreeSet<_> = pages.keys().cloned().collect();
    while let Some(origin) = pending.pop_first() {
        let page = &pages[&origin];
        for script in &page.semantic_scripts {
            let links::Destination::Local { path, .. } = links::resolve(&origin, script)? else {
                return Err(invalid(
                    &origin,
                    "external Rustdoc semantic script is not captured",
                ));
            };
            if artifact.tree.file(&path).is_some() {
                implementors::validate(
                    read_bytes(artifact, &path, &mut budget)?,
                    &page.crate_name,
                    path.as_str(),
                )?;
            }
        }
        let destinations: Vec<_> = page
            .links()
            .map(|href| links::resolve(&origin, href))
            .collect::<Result<_, _>>()?;
        for destination in destinations {
            let links::Destination::Local { path, .. } = destination else {
                continue;
            };
            if pages.contains_key(path.as_str()) || redirects.contains_key(&path) {
                continue;
            }
            let mut page = read(artifact, &path, &mut budget)?;
            let Some((generator, version)) = crates.get(&page.crate_name) else {
                return Err(invalid(
                    path.to_string(),
                    "Rustdoc source belongs to an unselected crate",
                ));
            };
            if !page.source || &page.generator != generator {
                return Err(invalid(
                    path.to_string(),
                    "Rustdoc reference needs a selected API page or matching source companion",
                ));
            }
            if page
                .version
                .as_ref()
                .is_some_and(|value| Some(value) != version.as_ref())
            {
                return Err(invalid(
                    path.to_string(),
                    "Rustdoc source version differs from selected crate",
                ));
            }
            page.version.clone_from(version);
            pending.insert(path.to_string());
            pages.insert(path.to_string(), page);
        }
    }
    let routes: BTreeMap<_, _> = pages
        .keys()
        .map(|path| {
            let name = path
                .strip_prefix(&prefix)
                .map(str::to_owned)
                .unwrap_or_else(|| format!("sources/{path}"));
            let stem = name
                .strip_suffix(".html")
                .ok_or_else(|| invalid(path, "Rustdoc reference is not an HTML page"))?;
            Ok((path.clone(), format!("{route}/{stem}")))
        })
        .collect::<Result<_, Error>>()?;
    let mut aliases: BTreeMap<String, BTreeMap<String, Vec<String>>> = BTreeMap::new();
    let mut mapped = BTreeMap::new();
    for (origin, page) in &pages {
        let mut links = BTreeMap::new();
        for href in page.links() {
            let target = match links::resolve(origin, href)? {
                links::Destination::External(value) => value,
                links::Destination::Local { path, fragment } => {
                    let path = redirects.get(&path).unwrap_or(&path);
                    let target = pages
                        .get(path.as_str())
                        .ok_or_else(|| invalid(origin, "missing resolved Rustdoc page"))?;
                    let fragment = fragment
                        .map(|fragment| anchor(target, path.as_str(), fragment, &mut aliases))
                        .transpose()?;
                    links::url(&routes[path.as_str()], fragment.as_deref())
                }
            };
            links.insert(href.to_owned(), target);
        }
        mapped.insert(origin.clone(), links);
    }
    let maximum = usize::try_from(limits.file_bytes).unwrap_or(usize::MAX);
    for (path, page) in &pages {
        let extra = aliases.remove(path).unwrap_or_default();
        let bytes = page.render(path, maximum, &mapped[path], &extra)?;
        let native = format!("{}.md", routes[path]);
        output.add_owned(&native, bytes)?;
        let provider = page.render_provider(path, maximum, &mapped[path], &extra)?;
        output.add_owned(&format!("{}.mdx", routes[path]), provider)?;
        output.provider_companion(&native)?;
        let mut anchors = page.anchors.clone();
        anchors.extend(extra.into_values().flatten());
        site_links.register_provider(&routes[path], PageKind::Physical, Some(anchors))?;
        catalog.physical(
            site_links,
            &routes[path],
            catalog.title(Some(&page.title))?,
            Some(page.reading_text(catalog.limits.bytes)?),
            true,
        )?;
    }
    let mut navigation = Branch::default();
    for (path, page) in &pages {
        if page.source {
            continue;
        }
        let relative = path
            .strip_prefix(&prefix)
            .ok_or_else(|| invalid(path, "foreign Rustdoc navigation page"))?;
        navigation.insert(relative.split('/'), &routes[path]);
    }
    Ok(navigation.finish())
}

struct Budget {
    bytes: u64,
    pages: usize,
    limits: Limits,
}

fn read(artifact: &Artifact<'_>, path: &EntryPath, budget: &mut Budget) -> Result<Page, Error> {
    let bytes = read_bytes(artifact, path, budget)?;
    html::parse(
        bytes,
        path.as_str(),
        usize::try_from(budget.limits.file_bytes).unwrap_or(usize::MAX),
    )
}

fn read_bytes<'a>(
    artifact: &'a Artifact<'_>,
    path: &EntryPath,
    budget: &mut Budget,
) -> Result<&'a [u8], Error> {
    let bytes = artifact
        .tree
        .file(path)
        .ok_or_else(|| invalid(path.to_string(), "missing captured Rustdoc page"))?;
    budget.pages = budget
        .pages
        .checked_add(1)
        .filter(|count| *count <= budget.limits.entries)
        .ok_or_else(|| invalid(path.to_string(), "Rustdoc page count limit"))?;
    budget.bytes = budget
        .bytes
        .checked_add(bytes.len() as u64)
        .filter(|size| *size <= budget.limits.total_bytes)
        .ok_or_else(|| invalid(path.to_string(), "Rustdoc input byte limit"))?;
    if bytes.len() as u64 > budget.limits.file_bytes {
        return Err(invalid(path.to_string(), "Rustdoc input file byte limit"));
    }
    Ok(bytes)
}

fn anchor(
    page: &Page,
    path: &str,
    fragment: String,
    aliases: &mut BTreeMap<String, BTreeMap<String, Vec<String>>>,
) -> Result<String, Error> {
    if page.anchors.contains(&fragment) {
        return Ok(fragment);
    }
    let decoded = percent_encoding::percent_decode_str(&fragment)
        .decode_utf8()
        .map_err(|_| invalid(path, "non-UTF-8 Rustdoc fragment"))?;
    if page.anchors.contains(decoded.as_ref()) {
        return Ok(decoded.into_owned());
    }
    if page.source
        && let Some((start, end)) = decoded.split_once('-')
    {
        let range = start.parse::<u64>().ok().zip(end.parse::<u64>().ok());
        if range.is_some_and(|(start, end)| start > 0 && start <= end)
            && page.anchors.contains(start)
            && page.anchors.contains(end)
        {
            let ids = aliases
                .entry(path.into())
                .or_default()
                .entry(start.into())
                .or_default();
            if !ids.contains(&decoded.to_string()) {
                ids.push(decoded.to_string());
            }
            return Ok(decoded.into_owned());
        }
    }
    Err(invalid(
        path,
        "Rustdoc fragment has no retained anchor or qualified source range",
    ))
}

#[derive(Default)]
struct Branch {
    leaves: BTreeMap<String, String>,
    children: BTreeMap<String, Branch>,
}

impl Branch {
    fn insert<'a>(&mut self, mut parts: impl Iterator<Item = &'a str>, route: &str) {
        let Some(first) = parts.next() else {
            return;
        };
        let rest: Vec<_> = parts.collect();
        if rest.is_empty() {
            self.leaves.insert(first.into(), route.into());
        } else {
            self.children
                .entry(first.into())
                .or_default()
                .insert(rest.into_iter(), route);
        }
    }

    fn finish(mut self) -> Vec<Navigation> {
        let mut pages = Vec::new();
        if let Some(index) = self.leaves.remove("index.html") {
            pages.push(Navigation::Path(index));
        }
        pages.extend(self.leaves.into_values().map(Navigation::Path));
        pages.extend(
            self.children
                .into_iter()
                .map(|(group, tree)| Navigation::Group {
                    group,
                    pages: tree.finish(),
                }),
        );
        pages
    }
}
