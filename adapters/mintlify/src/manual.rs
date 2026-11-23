//! Explicit authored-manual projection, separate from generated Markdown references.

mod companions;
pub(super) use companions::discover;

use std::collections::BTreeMap;

use gazette_core::{
    documentation::{
        ProjectItem, ProjectModule, Selection,
        projection::{
            AuthoringClosure, AuthoringManualPage, AuthoringReference, LinkProjection as SiteLinks,
            MANUAL_PROFILE, ManualDestinations, PageKind, manual_heading_bindings, manual_source,
        },
    },
    markdown::Limits,
    source::{Content, EntryPath},
};
use gazette_ports::markdown::Markdown;

use crate::{Error, invalid, output::Output};

pub(super) fn authoring_closure(
    pages: &[AuthoringManualPage<'_>],
    references: &[AuthoringReference<'_>],
    limits: gazette_core::source::Limits,
    markdown: &dyn Markdown,
) -> Result<AuthoringClosure, Error> {
    let mut destinations = BTreeMap::<&str, ManualDestinations>::new();
    for page in pages {
        destinations
            .entry(page.source)
            .or_default()
            .page(page.input, page.route)
            .map_err(|_| {
                invalid(
                    format!("{}:{}", page.source, page.input),
                    "authored manual source file is selected more than once for one configured source",
                )
            })?;
    }
    let mut output = Output::new(limits);
    let mut site_links = SiteLinks::default().with_anchor_projection(
        crate::target_anchors()?,
        pages.iter().map(|page| page.route.to_string()).collect(),
    );
    for reference in references {
        site_links.register_provider(reference.route.as_str(), PageKind::Incomplete, None)?;
    }
    for page in pages {
        let (body, needs_navigation) = project_page(
            page.bytes,
            page.input,
            page.route.as_str(),
            None,
            &destinations[page.source],
            markdown,
            &mut site_links,
        )?;
        if needs_navigation {
            output.require_fragment_navigation();
        }
        output.add_owned(&format!("{}.md", page.route), body.body.into_bytes())?;
    }
    let output = output.finish()?;
    let closure = site_links.validate(
        |route| {
            [format!("{route}.md"), format!("{route}.mdx")]
                .iter()
                .filter_map(|path| path.parse().ok())
                .any(|path| output.file(&path).is_some())
        },
        |path| {
            path.parse()
                .ok()
                .is_some_and(|path| output.file(&path).is_some())
        },
    )?;
    Ok(if references.is_empty() && !closure.incomplete() {
        AuthoringClosure::Complete
    } else {
        AuthoringClosure::GeneratedReferencesIncomplete
    })
}

/// Resolve every explicitly shared capture before projecting any module body.
/// Source keys, rather than equal inventories or filenames, define the boundary.
pub(super) fn destinations<'a>(
    projects: &'a [ProjectModule<'_>],
) -> Result<BTreeMap<&'a str, ManualDestinations>, Error> {
    let mut catalogs = BTreeMap::<&str, ManualDestinations>::new();
    let mut identities = BTreeMap::new();
    for project in projects {
        let Some(manual) = &project.manual else {
            continue;
        };
        if manual.source.is_empty() {
            return Err(invalid(
                project.slug,
                "manual capture requires an explicit source key",
            ));
        }
        let identity = manual.tree.inventory().identity();
        if identities
            .insert(manual.source, identity)
            .is_some_and(|prior| prior != identity)
        {
            return Err(invalid(
                manual.source,
                "shared manual source has conflicting retained inventories",
            ));
        }
        let catalog = catalogs.entry(manual.source).or_default();
        for item in &project.navigation {
            let ProjectItem::Manual(section) = item else {
                continue;
            };
            for page in &section.pages {
                let Selection::File { path } = page.selection else {
                    return Err(invalid(
                        page.route.as_str(),
                        "authored manual page requires one explicit Markdown file",
                    ));
                };
                if !path.as_str().ends_with(".md") {
                    return Err(invalid(
                        path.as_str(),
                        "authored manual page source must use the .md extension",
                    ));
                }
                catalog.page(path, page.route)?;
            }
        }
        for asset in &manual.assets {
            asset.selection.content(&manual.tree)?;
            match asset.selection {
                Selection::File { path } => register_asset(catalog, path, asset.path.as_str())?,
                Selection::Directory { path } => {
                    let prefix = path.as_ref().map(|path| format!("{path}/"));
                    for entry in manual.tree.inventory().entries() {
                        let relative = match &prefix {
                            Some(prefix) => match entry.path.as_str().strip_prefix(prefix) {
                                Some(relative) if !relative.is_empty() => relative,
                                _ => continue,
                            },
                            None => entry.path.as_str(),
                        };
                        if matches!(entry.content, Content::File { .. }) {
                            register_asset(
                                catalog,
                                &entry.path,
                                &format!("{}/{relative}", asset.path),
                            )?;
                        }
                    }
                }
            }
        }
    }
    Ok(catalogs)
}

/// Copy explicit assets and project authored pages into one deterministic target tree.
pub(super) fn package(
    project: &ProjectModule<'_>,
    catalogs: &BTreeMap<&str, ManualDestinations>,
    output: &mut Output,
    markdown: &dyn Markdown,
    site_links: &mut SiteLinks,
    catalog: crate::catalog::Scope<'_>,
) -> Result<BTreeMap<usize, Vec<String>>, Error> {
    let manual_sections: Vec<_> = project
        .navigation
        .iter()
        .enumerate()
        .filter_map(|(index, item)| match item {
            ProjectItem::Manual(section) => Some((index, section)),
            ProjectItem::Reference(_) => None,
        })
        .collect();
    if manual_sections.is_empty() {
        if project.manual.is_some() {
            return Err(invalid(
                project.slug,
                "manual source exists without authored navigation",
            ));
        }
        return Ok(BTreeMap::new());
    }
    let manual = project.manual.as_ref().ok_or_else(|| {
        invalid(
            project.slug,
            "authored navigation has no retained manual source",
        )
    })?;
    let destinations = &catalogs[manual.source];
    for asset in &manual.assets {
        asset.selection.content(&manual.tree)?;
        match asset.selection {
            Selection::File { path } => {
                let bytes = manual
                    .tree
                    .file(path)
                    .ok_or_else(|| invalid(path.as_str(), "missing selected manual asset bytes"))?;
                emit_asset(output, site_links, path, asset.path.as_str(), bytes)?;
            }
            Selection::Directory { path } => {
                let prefix = path.as_ref().map(|path| format!("{path}/"));
                let mut files = 0usize;
                for entry in manual.tree.inventory().entries() {
                    let relative = match &prefix {
                        Some(prefix) => match entry.path.as_str().strip_prefix(prefix) {
                            Some(relative) if !relative.is_empty() => relative,
                            _ => continue,
                        },
                        None => entry.path.as_str(),
                    };
                    if !matches!(entry.content, Content::File { .. }) {
                        continue;
                    }
                    let target = format!("{}/{relative}", asset.path);
                    let bytes = manual.tree.file(&entry.path).ok_or_else(|| {
                        invalid(entry.path.as_str(), "missing selected manual asset bytes")
                    })?;
                    emit_asset(output, site_links, &entry.path, &target, bytes)?;
                    files += 1;
                }
                if files == 0 {
                    return Err(invalid(
                        asset.path.as_str(),
                        "selected manual asset directory contains no files",
                    ));
                }
            }
        }
    }
    let mut navigation = BTreeMap::new();
    for (item_index, section) in manual_sections {
        let mut routes = Vec::new();
        for page in &section.pages {
            let Selection::File { path } = page.selection else {
                unreachable!("manual page selections were checked above")
            };
            let bytes = manual
                .tree
                .file(path)
                .ok_or_else(|| invalid(path.as_str(), "missing selected manual page bytes"))?;
            let (body, needs_navigation) = project_page(
                bytes,
                path,
                page.route.as_str(),
                Some(page.title),
                destinations,
                markdown,
                site_links,
            )?;
            if needs_navigation {
                output.require_fragment_navigation();
            }
            let bytes = crate::output::titled_markdown(page.title, &body.body)?.into_bytes();
            catalog.physical(
                site_links,
                page.route.as_str(),
                catalog.title(Some(page.title))?,
                body.text,
                false,
            )?;
            let target_route = site_links.page_route(page.route.as_str());
            output.add_owned(&format!("{target_route}.md"), bytes)?;
            routes.push(target_route.to_owned());
        }
        navigation.insert(item_index, routes);
    }
    Ok(navigation)
}

/// Mintlify serves `.txt` static files only on Enterprise plans, so a manual asset
/// whose configured output ends in `.txt` becomes a hidden page at this route instead:
/// the final name loses `.txt`, and its other dots and unsupported bytes become `-`.
/// Images and other assets keep their configured file path.
fn text_page_route(output: &str) -> Option<String> {
    let stem = output.strip_suffix(".txt")?;
    let (parent, name) = stem.rsplit_once('/').unwrap_or(("", stem));
    let name: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '-'
            }
        })
        .collect();
    let route = if parent.is_empty() {
        name
    } else {
        format!("{parent}/{name}")
    };
    Some(crate::page_route(&route))
}

/// Bind a manual asset input to its emitted file, or to its hidden text page.
pub(super) fn register_asset(
    catalog: &mut ManualDestinations,
    input: &EntryPath,
    output: &str,
) -> Result<(), Error> {
    match text_page_route(output) {
        Some(route) => catalog.page(input, &route.parse()?),
        None => catalog.asset(input, output),
    }
}

/// Copy one manual asset, or write its text as a hidden page that links can reach.
///
/// The page stays out of navigation and the catalog, carries `noindex: true`, is titled
/// with the source path and holds the file's exact text in one fenced code block. The
/// fence is longer than any backtick run in the text, so neither Markdown nor MDX reads
/// the content as markup.
pub(super) fn emit_asset(
    output: &mut Output,
    site_links: &mut SiteLinks,
    input: &EntryPath,
    target: &str,
    bytes: &[u8],
) -> Result<(), Error> {
    let Some(route) = text_page_route(target) else {
        return output.add(target, bytes);
    };
    let text = std::str::from_utf8(bytes)
        .ok()
        .filter(|text| !text.contains('\0'))
        .ok_or_else(|| {
            invalid(
                input.as_str(),
                "text manual asset must be UTF-8 without NUL",
            )
        })?;
    let extension = input
        .as_str()
        .rsplit_once('/')
        .map_or(input.as_str(), |(_, name)| name)
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .filter(|extension| {
            !extension.is_empty()
                && extension
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'_'))
        });
    let mut longest = 0usize;
    let mut run = 0usize;
    for character in text.chars() {
        run = if character == '`' { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    let fence = "`".repeat(longest.max(2) + 1);
    let title = serde_json::to_string(input.as_str())?;
    let newline = if text.is_empty() || text.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    let page = format!(
        "---\ntitle: {title}\nnoindex: true\n---\n\n{fence}{}\n{text}{newline}{fence}\n",
        extension.as_deref().unwrap_or("text")
    );
    site_links.register_hidden(&route)?;
    output.add_owned(&format!("{route}.md"), page.into_bytes())
}

fn project_page(
    bytes: &[u8],
    source: &EntryPath,
    route: &str,
    title: Option<&str>,
    destinations: &ManualDestinations,
    markdown: &dyn Markdown,
    site_links: &mut SiteLinks,
) -> Result<(gazette_core::markdown::TransportedMarkdown, bool), Error> {
    let text = manual_source(bytes, source)?;
    let document = markdown
        .parse(text, MANUAL_PROFILE, Limits::default())
        .map_err(Error::Markdown)?;
    let bindings = site_links.manual(source, route, destinations, document.syntax())?;
    let mut headings = manual_heading_bindings(document.syntax(), route)?;
    let anchors = site_links.anchor_projection(route);
    if let Some(anchors) = anchors {
        for heading in &mut headings {
            heading.id = anchors.id(&heading.id);
        }
    }
    let heading = match title {
        Some(title) if headings.iter().any(|heading| heading.level == 1) => {
            if headings.iter().filter(|heading| heading.level == 1).count() != 1 {
                return Err(invalid(
                    route,
                    "manual has multiple H1 headings beneath the configured page title",
                ));
            }
            Some(
                headings
                    .first()
                    .filter(|heading| {
                        heading.level == 1
                            && heading.occurrence.text.trim() == title.trim()
                            && text[..heading.occurrence.location.span.start]
                                .trim()
                                .is_empty()
                    })
                    .ok_or_else(|| {
                        invalid(
                            route,
                            "manual H1 must be a leading title matching the configured page title",
                        )
                    })?,
            )
        }
        Some(_) => None,
        None => None,
    };
    let body = document
        .project_mdx(
            document.syntax().identity,
            &bindings,
            &headings,
            heading,
            anchors,
        )
        .map_err(Error::Markdown)?;
    // Removing a leading title retains its native fragment as an empty anchor.
    // Authored HTML may supply the same kind of anchor without any rich layout.
    let needs_navigation = heading.is_some()
        || !document.syntax().html.is_empty()
        || !document.syntax().directives.is_empty();
    Ok((body, needs_navigation))
}
