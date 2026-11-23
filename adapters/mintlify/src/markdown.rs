//! Generated Markdown with selected-page closure and source-bound target link conversion.

use crate::{Error, configuration::Page, invalid, output::Output};
use gazette_core::{
    documentation::projection::{LinkProjection as SiteLinks, manual_html_anchors},
    documentation::{Artifact, ReferencePage, Selection},
    markdown::{Limits, LinkBinding, LinkKind, Profile},
    source::Content,
};
use gazette_ports::markdown::Markdown;
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn package(
    artifact: &Artifact<'_>,
    route: &str,
    selected_pages: Option<&[ReferencePage]>,
    output: &mut Output,
    adapter: &dyn Markdown,
    site_links: &mut SiteLinks,
    catalog: crate::catalog::Scope<'_>,
) -> Result<Vec<Page>, Error> {
    let pages = retained_pages(artifact, route, |name| {
        output.directory(&if name.is_empty() {
            route.to_owned()
        } else {
            format!("{route}/{name}")
        })
    })?;
    let navigation = selected_pages
        .map(|selected| explicit_navigation(selected, &pages, route))
        .transpose()?;
    let mut fallback = Vec::new();
    for (name, bytes) in &pages {
        let page_route = if name.is_empty() {
            route.to_owned()
        } else {
            format!("{route}/{name}")
        };
        let ProjectedMarkdown {
            converted,
            needs_navigation,
            title,
            text,
        } = project(bytes, name, &page_route, route, &pages, adapter, site_links)?;
        catalog.physical(site_links, &page_route, title, text, false)?;
        if needs_navigation {
            output.require_fragment_navigation();
        }
        match converted {
            Some(converted) => output.add_owned(
                &format!("{}.md", site_links.page_route(&page_route)),
                converted.into_bytes(),
            )?,
            None => output.add(&format!("{}.md", site_links.page_route(&page_route)), bytes)?,
        }
        fallback.push(Page::Path(crate::page_route(&page_route)));
    }
    Ok(navigation.unwrap_or(fallback))
}

pub(super) fn selected_routes(
    artifact: &Artifact<'_>,
    route: &str,
) -> Result<BTreeSet<String>, Error> {
    Ok(retained_pages(artifact, route, |_| Ok(()))?
        .keys()
        .map(|name| {
            if name.is_empty() {
                route.to_owned()
            } else {
                format!("{route}/{name}")
            }
        })
        .collect())
}

fn retained_pages<'a>(
    artifact: &'a Artifact<'_>,
    route: &str,
    mut directory: impl FnMut(&str) -> Result<(), Error>,
) -> Result<BTreeMap<String, &'a [u8]>, Error> {
    artifact.selection.content(&artifact.tree)?;
    let mut pages = BTreeMap::new();
    match artifact.selection {
        Selection::File { .. } => {
            pages.insert(String::new(), artifact.bytes()?);
        }
        Selection::Directory { path } => {
            let prefix = path.as_ref().map(|path| format!("{path}/"));
            directory("")?;
            for entry in artifact.tree.inventory().entries() {
                let name = match &prefix {
                    Some(prefix) => match entry.path.as_str().strip_prefix(prefix) {
                        Some(name) => name,
                        None => continue,
                    },
                    None => entry.path.as_str(),
                };
                match entry.content {
                    Content::Directory => {
                        if !name.split('/').all(segment) {
                            return Err(invalid(name, "unsupported Markdown directory path"));
                        }
                        directory(name)?;
                    }
                    Content::File { .. } => {
                        let stem = name.strip_suffix(".md").filter(|stem| stem.split('/').all(segment))
                            .ok_or_else(|| invalid(name, "selected Markdown directory requires URL-safe .md pages only; unsupported companion or layout"))?;
                        let bytes = artifact
                            .tree
                            .file(&entry.path)
                            .ok_or_else(|| invalid(name, "missing selected Markdown page bytes"))?;
                        pages.insert(stem.to_owned(), bytes);
                    }
                }
            }
            if pages.is_empty() {
                return Err(invalid(
                    route,
                    "selected Markdown directory contains no pages",
                ));
            }
        }
    }
    Ok(pages)
}

fn explicit_navigation(
    selected: &[ReferencePage],
    available: &BTreeMap<String, &[u8]>,
    root: &str,
) -> Result<Vec<Page>, Error> {
    fn page(
        path: &str,
        available: &BTreeMap<String, &[u8]>,
        root: &str,
        seen: &mut BTreeSet<String>,
    ) -> Result<Page, Error> {
        if !available.contains_key(path) || !seen.insert(path.to_owned()) {
            return Err(invalid(
                path,
                "reference navigation has an unknown or repeated page",
            ));
        }
        Ok(Page::Path(crate::page_route(&format!("{root}/{path}"))))
    }

    fn directory(
        prefix: &str,
        available: &BTreeMap<String, &[u8]>,
        root: &str,
        seen: &mut BTreeSet<String>,
        depth: usize,
    ) -> Result<Vec<Page>, Error> {
        let prefix = format!("{prefix}/");
        let mut direct = Vec::new();
        let mut directories = BTreeSet::new();
        for path in available.keys() {
            if let Some(relative) = path.strip_prefix(&prefix) {
                match relative.split_once('/') {
                    Some((name, _)) => {
                        directories.insert(name);
                    }
                    None => direct.push(path.as_str()),
                }
            }
        }
        if direct.is_empty() && directories.is_empty() {
            return Err(invalid(
                &prefix,
                "reference directory selection contains no pages",
            ));
        }
        // A generated directory's landing page precedes its other pages and subgroups.
        direct.sort_by_key(|path| (path.strip_prefix(&prefix) != Some("index"), *path));
        let mut result = direct
            .into_iter()
            .map(|path| page(path, available, root, seen))
            .collect::<Result<Vec<_>, _>>()?;
        for name in directories {
            if depth >= 64 {
                return Err(invalid(
                    &prefix,
                    "reference navigation exceeds 64 nested groups",
                ));
            }
            result.push(Page::Group {
                group: name.to_owned(),
                pages: directory(&format!("{prefix}{name}"), available, root, seen, depth + 1)?,
            });
        }
        Ok(result)
    }

    fn project(
        selected: &[ReferencePage],
        available: &BTreeMap<String, &[u8]>,
        root: &str,
        seen: &mut BTreeSet<String>,
        depth: usize,
    ) -> Result<Vec<Page>, Error> {
        let mut result = Vec::new();
        for node in selected {
            match node {
                ReferencePage::Page { path } => {
                    result.push(page(path.as_str(), available, root, seen)?)
                }
                ReferencePage::Directory { path } => {
                    result.extend(directory(path.as_str(), available, root, seen, depth)?)
                }
                ReferencePage::Group { title, pages } => result.push(Page::Group {
                    group: title.clone(),
                    pages: project(pages, available, root, seen, depth + 1)?,
                }),
            }
        }
        Ok(result)
    }
    let mut seen = BTreeSet::new();
    let pages = project(selected, available, root, &mut seen, 0)?;
    if seen.len() != available.len() {
        return Err(invalid(
            root,
            "reference navigation must include every selected Markdown page exactly once",
        ));
    }
    Ok(pages)
}

fn segment(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

struct ProjectedMarkdown {
    converted: Option<String>,
    needs_navigation: bool,
    title: Option<String>,
    text: Option<String>,
}

fn project(
    bytes: &[u8],
    route: &str,
    page_route: &str,
    root_route: &str,
    pages: &BTreeMap<String, &[u8]>,
    adapter: &dyn Markdown,
    site_links: &mut SiteLinks,
) -> Result<ProjectedMarkdown, Error> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| invalid(route, "generated Markdown must be UTF-8"))?;
    if text.trim().is_empty() || text.contains('\0') {
        return Err(invalid(
            route,
            "generated Markdown is empty or contains NUL",
        ));
    }
    let first_line = text
        .strip_prefix('\u{feff}')
        .unwrap_or(text)
        .lines()
        .next()
        .unwrap_or_default()
        .trim();
    if matches!(first_line, "---" | "+++") {
        return Err(invalid(
            route,
            "frontmatter-driven target behavior requires a qualified profile",
        ));
    }
    let document = adapter
        .parse(text, Profile::Publication, Limits::default())
        .map_err(Error::Markdown)?;
    let mut raw_anchors = std::collections::BTreeSet::new();
    for html in &document.syntax().html {
        raw_anchors.extend(manual_html_anchors(&html.literal, route)?);
    }
    site_links.register_markdown(page_route, document.syntax(), raw_anchors)?;
    // Complete source facts also cover reference-style links and unused footnotes.
    // The target owns URL mapping; the Markdown adapter owns structural formatting.
    let mut bindings = Vec::with_capacity(document.syntax().links.len());
    for link in &document.syntax().links {
        let destination = &link.destination;
        let absolute = ["https://", "http://", "mailto:"].iter().any(|scheme| {
            destination
                .strip_prefix(scheme)
                .is_some_and(|tail| !tail.is_empty())
        });
        if destination.chars().any(char::is_control) {
            return Err(invalid(route, "control-containing Markdown destination"));
        }
        if absolute {
            bindings.push(LinkBinding {
                image_sources: None,
                svg_layout: None,
                image_width: None,
                image_dimensions: None,
                image_loading: None,
                image_fetch_priority: None,
                inline_svg: None,
                image_address: None,
                occurrence: link.clone(),
                destination: if link.kind == LinkKind::Link {
                    site_links.external_link(destination)?
                } else {
                    destination.clone()
                },
                annotation: None,
            });
            continue;
        }
        if destination.starts_with('#') && destination.len() > 1 {
            site_links.project(&format!("/{page_route}{destination}"))?;
            bindings.push(LinkBinding {
                image_sources: None,
                svg_layout: None,
                image_width: None,
                image_dimensions: None,
                image_loading: None,
                image_fetch_priority: None,
                inline_svg: None,
                image_address: None,
                occurrence: link.clone(),
                destination: format!(
                    "#{}",
                    site_links.project_fragment(page_route, &destination[1..])?
                ),
                annotation: None,
            });
            continue;
        }
        if destination.starts_with('/') && link.kind != LinkKind::Image {
            let projected = site_links.project(destination)?;
            bindings.push(LinkBinding {
                image_sources: None,
                svg_layout: None,
                image_width: None,
                image_dimensions: None,
                image_loading: None,
                image_fetch_priority: None,
                inline_svg: None,
                image_address: None,
                occurrence: link.clone(),
                destination: projected,
                annotation: None,
            });
            continue;
        }
        if route.is_empty() || link.kind == LinkKind::Image {
            return Err(invalid(
                route,
                "relative links require a selected Markdown page directory; embedded companions are unsupported",
            ));
        }
        let (target, projected) = relative_page(route, destination)?;
        if !pages.contains_key(&target) {
            return Err(invalid(
                destination,
                "relative Markdown link has no exact selected page route",
            ));
        }
        let fragment = projected
            .split_once('#')
            .map_or("", |(_, fragment)| fragment);
        let absolute = if fragment.is_empty() {
            format!("/{root_route}/{target}")
        } else {
            format!("/{root_route}/{target}#{fragment}")
        };
        // The provider treats relative addresses as external navigation. Use the
        // validated site-root address, including its projected page and fragment.
        let projected = site_links.project(&absolute)?;
        bindings.push(LinkBinding {
            image_sources: None,
            svg_layout: None,
            image_width: None,
            image_dimensions: None,
            image_loading: None,
            image_fetch_priority: None,
            inline_svg: None,
            image_address: None,
            occurrence: link.clone(),
            destination: projected,
            annotation: None,
        });
    }
    let mut headings = gazette_core::documentation::projection::manual_heading_bindings(
        document.syntax(),
        page_route,
    )?;
    let anchors = site_links.anchor_projection(page_route);
    if let Some(anchors) = anchors {
        for heading in &mut headings {
            heading.id = anchors.id(&heading.id);
        }
    }
    let title = headings.first().filter(|heading| {
        heading.level == 1
            && !heading.occurrence.text.trim().is_empty()
            && text[..heading.occurrence.location.span.start]
                .trim()
                .is_empty()
    });
    let transport = |title| {
        document.project_mdx(
            document.syntax().identity,
            &bindings,
            &headings,
            title,
            anchors,
        )
    };
    let admitted_text;
    let body = match title {
        Some(title) => match transport(Some(title)) {
            Ok(body) => {
                admitted_text = body.text;
                crate::output::titled_markdown(&title.occurrence.text, &body.body)?
            }
            // A rich heading is supported by legacy generated Markdown. Keep
            // its inline structure instead of flattening it into target metadata.
            Err(gazette_core::markdown::Error::Unsupported { .. }) => {
                let body = transport(None).map_err(Error::Markdown)?;
                admitted_text = body.text;
                body.body
            }
            Err(error) => return Err(Error::Markdown(error)),
        },
        None => {
            let body = transport(None).map_err(Error::Markdown)?;
            admitted_text = body.text;
            body.body
        }
    };
    Ok(ProjectedMarkdown {
        converted: (body != text).then_some(body),
        needs_navigation: title.is_some()
            || !document.syntax().html.is_empty()
            || !document.syntax().directives.is_empty(),
        title: title.map(|title| title.occurrence.text.clone()),
        text: admitted_text,
    })
}

// Mintlify's extensionless relative URL profile, not a native-path decoder or Markdown parser.
fn relative_page(origin: &str, destination: &str) -> Result<(String, String), Error> {
    let (path, fragment) = match destination.split_once('#') {
        Some((_, "")) => return Err(invalid(destination, "empty Markdown fragment")),
        Some((path, fragment)) => (path, Some(fragment)),
        None => (destination, None),
    };
    let path = path.strip_suffix(".md").unwrap_or(path);
    let mut components: Vec<_> = origin.split('/').collect();
    components.pop();
    for component in path.split('/') {
        match component {
            "." => {}
            ".." => {
                if components.pop().is_none() {
                    return Err(invalid(
                        destination,
                        "Markdown link escapes selected reference directory",
                    ));
                }
            }
            component if segment(component) => components.push(component),
            _ => {
                return Err(invalid(
                    destination,
                    "relative links require URL-safe page routes with optional .md suffix and no queries",
                ));
            }
        }
    }
    let projected = match fragment {
        Some(fragment) => format!("{path}#{fragment}"),
        None => path.to_owned(),
    };
    Ok((components.join("/"), projected))
}
