//! Effect-free validation of the configured documentation-target contract.

use std::collections::BTreeSet;

use super::{Error, Format, Plan, PlannedProjectItem, Selection, Target};

pub(super) fn validate(plan: &Plan<'_>) -> Result<(), Error> {
    label(plan.name, "publication name")?;
    let Target::Mintlify {
        theme,
        primary_color,
        light_color,
        dark_color,
        ..
    } = plan.target;
    if !matches!(
        theme.as_str(),
        "mint" | "maple" | "palm" | "willow" | "linden" | "almond" | "aspen" | "luma" | "sequoia"
    ) {
        return Err(invalid("theme", "unsupported Mintlify theme"));
    }
    for (name, color) in [
        ("primary color", Some(primary_color)),
        ("light color", light_color.as_ref()),
        ("dark color", dark_color.as_ref()),
    ] {
        if let Some(color) = color
            && (!matches!(color.len(), 4 | 7)
                || !color.starts_with('#')
                || !color.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit))
        {
            return Err(invalid(name, "expected #RGB or #RRGGBB"));
        }
    }
    if plan.projects.is_empty() {
        return Err(invalid(
            "projects",
            "documentation target has no project modules",
        ));
    }
    let mut project_slugs = BTreeSet::new();
    let mut namespaces = Vec::new();
    let mut output_paths = BTreeSet::new();
    for project in &plan.projects {
        slug(project.slug)?;
        label(project.title, project.slug)?;
        if !project_slugs.insert(project.slug) || project.navigation.is_empty() {
            return Err(invalid(project.slug, "duplicate or empty project module"));
        }
        let mut reference_slugs = BTreeSet::new();
        let mut manual_routes = BTreeSet::new();
        for item in &project.navigation {
            match item {
                PlannedProjectItem::Manual(section) => {
                    label(section.title, project.slug)?;
                    if section.pages.is_empty() {
                        return Err(invalid(project.slug, "empty authored manual section"));
                    }
                    for page in &section.pages {
                        label(page.title, page.route.as_str())?;
                        route(page.route.as_str())?;
                        if !manual_routes.insert(page.route.as_str()) {
                            return Err(invalid(
                                page.route.as_str(),
                                "duplicate authored manual route",
                            ));
                        }
                        reserve_namespace(
                            &mut namespaces,
                            NamespaceKind::Manual,
                            page.route.as_str(),
                        )?;
                        if !output_paths.insert(format!("{}.md", page.route)) {
                            return Err(invalid(
                                page.route.as_str(),
                                "authored manual output collides with another documentation item",
                            ));
                        }
                    }
                }
                PlannedProjectItem::Reference(reference) => {
                    slug(reference.slug)?;
                    label(reference.title, reference.slug)?;
                    if !reference_slugs.insert(reference.slug) {
                        return Err(invalid(reference.slug, "duplicate reference module"));
                    }
                    let root = format!("{}/{}", project.slug, reference.slug);
                    reserve_namespace(&mut namespaces, NamespaceKind::Reference, &root)?;
                    if !output_paths.insert(format!("{root}.md")) {
                        return Err(invalid(
                            reference.slug,
                            "generated reference output collides with another documentation item",
                        ));
                    }
                    if reference.format == Format::RustdocJson {
                        return Err(invalid(
                            reference.slug,
                            "unsupported native Rustdoc format; no Mintlify conversion qualified",
                        ));
                    }
                    if let Some(pages) = reference.pages {
                        if reference.format != Format::Markdown
                            || !matches!(reference.selection, Selection::Directory { .. })
                        {
                            return Err(invalid(
                                reference.slug,
                                "explicit reference pages require a Markdown directory selection",
                            ));
                        }
                        super::navigation::validate(pages, reference.slug)?;
                    }
                }
            }
        }
        for asset in &project.manual_assets {
            route(asset.as_str())?;
            reserve_namespace(&mut namespaces, NamespaceKind::Asset, asset.as_str())?;
            if !output_paths.insert(asset.to_string()) {
                return Err(invalid(
                    asset.as_str(),
                    "manual asset output collides with another documentation item",
                ));
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum NamespaceKind {
    Manual,
    Reference,
    Asset,
}

fn reserve_namespace(
    namespaces: &mut Vec<(NamespaceKind, String)>,
    kind: NamespaceKind,
    candidate: &str,
) -> Result<(), Error> {
    // A manual route owns one page, so its descendants can hold reference or
    // companion files. Reference and asset selections still own their subtrees.
    if namespaces.iter().any(|(existing_kind, existing)| {
        existing_kind != &kind
            && (existing == candidate
                || (kind != NamespaceKind::Manual
                    && existing
                        .strip_prefix(candidate)
                        .is_some_and(|tail| tail.starts_with('/')))
                || (*existing_kind != NamespaceKind::Manual
                    && candidate
                        .strip_prefix(existing)
                        .is_some_and(|tail| tail.starts_with('/'))))
    }) {
        return Err(invalid(
            candidate,
            "documentation navigation or asset namespace overlaps another selected item",
        ));
    }
    namespaces.push((kind, candidate.to_owned()));
    Ok(())
}

fn route(value: &str) -> Result<(), Error> {
    if value.starts_with("artifacts/")
        || matches!(value, "artifacts" | "docs.json" | ".mintignore")
        || value.ends_with(".md")
        || value.ends_with(".mdx")
        || !value.split('/').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        })
    {
        return Err(invalid(
            value,
            "expected an exact nonreserved target-relative route",
        ));
    }
    Ok(())
}

fn slug(value: &str) -> Result<(), Error> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        || value.starts_with('-')
        || value.ends_with('-')
        || matches!(value, "api" | "artifacts")
    {
        return Err(invalid(
            value,
            "expected a nonreserved lowercase URL segment",
        ));
    }
    Ok(())
}

fn label(value: &str, context: &str) -> Result<(), Error> {
    if value.trim().is_empty() || value.chars().any(char::is_control) {
        return Err(invalid(context, "empty or control-containing display text"));
    }
    Ok(())
}

fn invalid(context: impl Into<String>, reason: &'static str) -> Error {
    Error::Invalid {
        context: context.into(),
        reason,
    }
}
