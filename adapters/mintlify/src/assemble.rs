//! One complete site target, composed from resolved project-module projections.

use crate::{
    Error,
    configuration::{self, Group, Input, OpenApi, Page, ReferenceGroup, Sdk},
    invalid, manual, markdown, openapi,
    output::Output,
    rustdoc, sphinx, swagger, typedoc,
};
use gazette_core::documentation::{
    Format, Plan, PlannedProject, PlannedProjectItem, ProjectItem, ProjectModule,
    projection::LinkProjection as SiteLinks,
};
use gazette_core::source::Limits;

/// Already resolved presentation values, not an independent workspace schema.
#[derive(Clone, Copy)]
pub(crate) struct Config<'a> {
    /// Resolved documentation publication display name.
    pub name: &'a str,
    /// Existing publication identity; preview addresses never replace this setting.
    pub public_base_url: Option<&'a gazette_core::publication::routing::PublicBaseUrl>,
    /// Explicit supported Mintlify theme selection.
    pub theme: &'a str,
    /// Resolved three- or six-digit primary brand color.
    pub primary_color: &'a str,
    pub light_color: Option<&'a str>,
    pub dark_color: Option<&'a str>,
    /// Explicit telemetry integration choice; omission writes no integrations block.
    pub telemetry: Option<bool>,
    /// The owning publication's TDM reservation and crawler rules for this docs host.
    pub usage: Option<&'a gazette_core::publication::usage::UsagePolicy<'a>>,
    /// Local preview hyperlink projection; never a public target setting.
    pub preview_link_bases: Option<&'a gazette_core::publication::routing::PreviewLinkBases>,
}

/// Assemble an immutable complete repository tree without native I/O.
///
/// Array order is preserved as configured navigation order; it is not inferred
/// from names or filesystem discovery. Exact inputs yield exact bytes. Output
/// destination, repository initialization, generation, signing and deployment
/// remain caller-owned effects. This validates local packaging, not a provider
/// build, symbol rendering fidelity or publication approval.
///
/// # Errors
///
/// Rejects missing/duplicate module namespaces, invalid presentation fields,
/// malformed artifacts, broken local reflection targets and tree limits. Never
/// returns a successful subset when a selected reference fails.
pub(crate) fn assemble(
    config: Config<'_>,
    projects: &[ProjectModule<'_>],
    limits: Limits,
    markdown: &dyn gazette_ports::markdown::Markdown,
    catalog_limits: gazette_core::documentation::CatalogLimits,
) -> Result<gazette_core::documentation::TargetAssembly, Error> {
    let mut plan = Plan {
        name: config.name,
        target: &gazette_core::documentation::Target::Mintlify {
            theme: config.theme.to_owned(),
            primary_color: config.primary_color.to_owned(),
            light_color: config.light_color.map(str::to_owned),
            dark_color: config.dark_color.map(str::to_owned),
            telemetry: config.telemetry,
        },
        projects: projects
            .iter()
            .map(|project| PlannedProject {
                slug: project.slug,
                title: project.title,
                manual_assets: project.manual.as_ref().map_or_else(Vec::new, |manual| {
                    manual.assets.iter().map(|asset| asset.path).collect()
                }),
                navigation: project
                    .navigation
                    .iter()
                    .map(|item| match item {
                        ProjectItem::Manual(section) => PlannedProjectItem::Manual(
                            gazette_core::documentation::PlannedManualSection {
                                title: section.title,
                                pages: section
                                    .pages
                                    .iter()
                                    .map(|page| gazette_core::documentation::PlannedManualPage {
                                        title: page.title,
                                        route: page.route,
                                    })
                                    .collect(),
                            },
                        ),
                        ProjectItem::Reference(reference) => PlannedProjectItem::Reference(
                            gazette_core::documentation::PlannedReference {
                                slug: reference.slug,
                                title: reference.title,
                                format: reference.artifact.format,
                                selection: reference.artifact.selection,
                                pages: reference.pages,
                            },
                        ),
                    })
                    .collect(),
            })
            .collect::<Vec<_>>(),
    };
    plan.validate()?;
    let mut manual_destinations = manual::destinations(projects)?;
    let companions = manual::discover(projects, &mut manual_destinations, markdown)?;
    for companion in &companions {
        plan.projects[companion.project]
            .manual_assets
            .push(&companion.output);
    }
    plan.validate()?;
    let mut anchor_routes = std::collections::BTreeSet::new();
    for project in projects {
        for item in &project.navigation {
            match item {
                ProjectItem::Manual(section) => {
                    anchor_routes.extend(section.pages.iter().map(|page| page.route.to_string()))
                }
                ProjectItem::Reference(reference)
                    if reference.artifact.format == Format::Markdown =>
                {
                    anchor_routes.extend(markdown::selected_routes(
                        &reference.artifact,
                        &format!("{}/{}", project.slug, reference.slug),
                    )?);
                }
                ProjectItem::Reference(_) => {}
            }
        }
    }
    let mut output = Output::new(limits);
    let mut site_links = SiteLinks::default()
        .with_preview_link_bases(config.preview_link_bases.cloned())
        .with_page_routes(
            anchor_routes
                .iter()
                .map(|route| (route.clone(), crate::page_route(route)))
                .collect(),
        )?
        .with_anchor_projection(crate::target_anchors()?, anchor_routes);
    let mut groups = Vec::new();
    for companion in &companions {
        manual::emit_asset(
            &mut output,
            &mut site_links,
            &companion.input,
            companion.output.as_str(),
            companion.bytes,
        )?;
    }
    for project in projects {
        let catalog = crate::catalog::Scope {
            module: project.slug,
            limits: catalog_limits,
        };
        let manual = manual::package(
            project,
            &manual_destinations,
            &mut output,
            markdown,
            &mut site_links,
            catalog,
        )?;
        let mut pages = Vec::new();
        for (item_index, item) in project.navigation.iter().enumerate() {
            let ProjectItem::Reference(reference) = item else {
                let ProjectItem::Manual(section) = item else {
                    unreachable!()
                };
                pages.push(Page::Group {
                    group: section.title.to_owned(),
                    pages: manual
                        .get(&item_index)
                        .expect("manual packager returns every selected section")
                        .iter()
                        .cloned()
                        .map(Page::Path)
                        .collect(),
                });
                continue;
            };
            let directory = format!("{}/{}", project.slug, reference.slug);
            let input = match reference.artifact.format {
                Format::Markdown => Input::Pages(markdown::package(
                    &reference.artifact,
                    &directory,
                    reference.pages,
                    &mut output,
                    markdown,
                    &mut site_links,
                    catalog,
                )?),
                Format::TypeDocJson | Format::OpenApiJson | Format::OpenApiYaml => {
                    let bytes = reference.artifact.bytes()?;
                    let extension = if reference.artifact.format == Format::OpenApiYaml {
                        "yaml"
                    } else {
                        "json"
                    };
                    let artifact_path = format!("artifacts/{directory}.{extension}");
                    let path = output.reserve(&artifact_path, bytes)?;
                    let input = if reference.artifact.format == Format::TypeDocJson {
                        typedoc::validate(
                            bytes,
                            &artifact_path,
                            &directory,
                            &mut site_links,
                            catalog,
                        )?;
                        Input::Sdk(Sdk {
                            format: "typedoc",
                            source: artifact_path,
                            directory,
                        })
                    } else {
                        let value = if reference.artifact.format == Format::OpenApiYaml {
                            openapi::validate_yaml(bytes, &artifact_path)?
                        } else {
                            openapi::validate(bytes, &artifact_path)?
                        };
                        openapi::register(&directory, &mut site_links)?;
                        catalog.virtual_page(
                            &mut site_links,
                            &directory,
                            catalog.title(
                                value
                                    .pointer("/info/title")
                                    .and_then(serde_json::Value::as_str),
                            )?,
                            None,
                            &[&artifact_path],
                        )?;
                        Input::Openapi(OpenApi {
                            source: artifact_path,
                            directory,
                        })
                    };
                    output.retain(path, bytes);
                    input
                }
                Format::SwaggerJson | Format::SwaggerYaml => {
                    let bytes = reference.artifact.bytes()?;
                    let ceiling = output.file_ceiling();
                    if bytes.len() as u64 > ceiling {
                        return Err(invalid(
                            reference.slug,
                            "Swagger input exceeds target byte limit",
                        ));
                    }
                    let artifact_path = format!("artifacts/{directory}.json");
                    let converted = swagger::convert(
                        bytes,
                        &artifact_path,
                        reference.artifact.format == Format::SwaggerYaml,
                        ceiling,
                    )?;
                    openapi::register(&directory, &mut site_links)?;
                    let value = openapi::parse(&converted, &artifact_path, false)?;
                    catalog.virtual_page(
                        &mut site_links,
                        &directory,
                        catalog.title(
                            value
                                .pointer("/info/title")
                                .and_then(serde_json::Value::as_str),
                        )?,
                        None,
                        &[&artifact_path],
                    )?;
                    output.add_owned(&artifact_path, converted)?;
                    Input::Openapi(OpenApi {
                        source: artifact_path,
                        directory,
                    })
                }
                Format::SphinxJson => {
                    let source = format!("artifacts/{directory}");
                    sphinx::package(
                        &reference.artifact,
                        &source,
                        &directory,
                        &mut output,
                        &mut site_links,
                        catalog,
                    )?;
                    Input::Sdk(Sdk {
                        format: "sphinx",
                        source,
                        directory,
                    })
                }
                Format::RustdocHtml => Input::Pages(rustdoc::package(
                    &reference.artifact,
                    &directory,
                    &mut output,
                    &mut site_links,
                    limits,
                    catalog,
                )?),
                Format::RustdocJson => unreachable!("preflight rejects unsupported Rustdoc JSON"),
            };
            pages.push(Page::Reference(ReferenceGroup {
                group: reference.title.to_owned(),
                input,
            }));
        }
        groups.push(Group {
            group: project.title,
            pages,
        });
    }
    let mut ignores = configuration::artifact_ignores(&groups, output.file_ceiling())?;
    if let Some(usage) = config.usage {
        configuration::usage(usage, config.public_base_url, &mut output, &mut ignores)?;
    }
    let ignores = output.provider_ignores(&ignores)?;
    output.add_owned(".mintignore", ignores)?;
    if let Some(base) = config.public_base_url {
        output.add_owned(
            "llms.txt",
            reading_guide(config.name, base.as_str(), projects)?,
        )?;
    } else if config.preview_link_bases.is_some() {
        // A local, unaddressed project has no public guide or hosted export.
        let index = preview_index(config.name, &groups, projects, &output, markdown)?;
        output.add_owned("llms.txt", index)?;
    }
    let configuration = configuration::encode(&config, groups, output.file_ceiling())?;
    output.add_owned("docs.json", configuration)?;
    let output = output.finish()?;
    let catalog = site_links.validate_catalog(&output, catalog_limits)?;
    Ok(gazette_core::documentation::TargetAssembly {
        output,
        catalog: Some(catalog),
    })
}

fn reading_guide(name: &str, base: &str, projects: &[ProjectModule<'_>]) -> Result<Vec<u8>, Error> {
    use std::io::Write;

    let mut guide = crate::output::BoundedBuffer::new(10_000);
    let limit = |_| {
        invalid(
            "llms.txt",
            "documentation reading guide exceeds its byte budget",
        )
    };
    let label = |value: &str| -> String {
        value
            .chars()
            .flat_map(|character| match character {
                '\r' | '\n' => [Some(' '), None],
                '[' | ']' | '\\' => [Some('\\'), Some(character)],
                _ => [Some(character), None],
            })
            .flatten()
            .collect()
    };
    writeln!(
        guide,
        "# {}\n\n> Developer guides and reference for {}.",
        label(name),
        label(name)
    )
    .map_err(limit)?;
    for project in projects {
        let sections = project
            .navigation
            .iter()
            .filter_map(|item| match item {
                ProjectItem::Manual(section) => section.pages.first().map(|page| (section, page)),
                ProjectItem::Reference(_) => None,
            })
            .peekable();
        let mut sections = sections;
        if sections.peek().is_none() {
            continue;
        }
        writeln!(guide, "\n## {}\n", label(project.title)).map_err(limit)?;
        for (section, page) in sections {
            if section.title == page.title {
                writeln!(guide, "- [{}](<{}{}>)", label(page.title), base, page.route)
                    .map_err(limit)?;
            } else {
                writeln!(
                    guide,
                    "- [{}](<{}{}>): {}",
                    label(page.title),
                    base,
                    page.route,
                    label(section.title)
                )
                .map_err(limit)?;
            }
        }
    }
    writeln!(guide, "\n## Full documentation\n\n- [Full developer documentation](<{}llms-full.txt>): Complete guides and generated reference.", base)
        .map_err(limit)?;
    Ok(guide.finish())
}

fn preview_index(
    name: &str,
    groups: &[Group<'_>],
    projects: &[ProjectModule<'_>],
    output: &Output,
    markdown: &dyn gazette_ports::markdown::Markdown,
) -> Result<Vec<u8>, Error> {
    let ceiling = output.file_ceiling();
    let label = |value: &str| {
        let mut text = String::new();
        for character in value.chars() {
            if matches!(character, '\r' | '\n') {
                text.push(' ');
            } else {
                if character.is_ascii_punctuation() {
                    text.push('\\');
                }
                text.push(character);
            }
        }
        text
    };
    let titles: std::collections::BTreeMap<_, _> = projects
        .iter()
        .flat_map(|project| project.navigation.iter())
        .filter_map(|item| match item {
            ProjectItem::Manual(section) => Some(section.pages.iter()),
            _ => None,
        })
        .flatten()
        .map(|page| (page.route.as_str(), page.title))
        .collect();
    let mut text = format!(
        "# {}\n\nSelected pages in this private local preview. Links open the served HTML pages.\n\n",
        label(name)
    );
    for group in groups {
        let mut pending: Vec<_> = group.pages.iter().rev().collect();
        while let Some(page) = pending.pop() {
            match page {
                Page::Path(route) => {
                    let mut encoded = String::new();
                    for byte in route.bytes() {
                        if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
                            encoded.push(char::from(byte));
                        } else {
                            use std::fmt::Write;
                            write!(encoded, "%{byte:02X}").expect("writing into String");
                        }
                    }
                    let generated_title = if titles.contains_key(route.as_str()) {
                        None
                    } else {
                        let bytes = output
                            .file(&format!("{route}.md"))
                            .or_else(|| output.file(&format!("{route}.mdx")));
                        match bytes {
                            Some(bytes) => {
                                let source = std::str::from_utf8(bytes).map_err(|_| {
                                    invalid(route, "retained index page is not UTF-8")
                                })?;
                                match crate::output::native_markdown_title(source) {
                                    Some(title) => Some(title),
                                    None => {
                                        let document = markdown
                                            .parse(
                                                source,
                                                gazette_core::markdown::Profile::Publication,
                                                gazette_core::markdown::Limits::default(),
                                            )
                                            .map_err(Error::Markdown)?;
                                        document
                                            .syntax()
                                            .headings
                                            .iter()
                                            .find(|heading| heading.level == 1)
                                            .map(|heading| heading.text.clone())
                                    }
                                }
                            }
                            None => None,
                        }
                    };
                    let title = titles
                        .get(route.as_str())
                        .copied()
                        .or(generated_title.as_deref())
                        .unwrap_or(route);
                    let line = format!("- [{}](/{encoded})\n", label(title));
                    if (text.len() + line.len()) as u64 > ceiling {
                        return Err(invalid(
                            "llms.txt",
                            "private preview index exceeds target byte limit",
                        ));
                    }
                    text.push_str(&line);
                }
                Page::Group { pages, .. }
                | Page::Reference(ReferenceGroup {
                    input: Input::Pages(pages),
                    ..
                }) => {
                    pending.extend(pages.iter().rev());
                }
                // Virtual provider routes do not become retained physical pages
                // merely because a generator root was selected.
                Page::Reference(_) => {}
            }
        }
    }
    if text.len() as u64 > ceiling {
        return Err(invalid(
            "llms.txt",
            "private preview index exceeds target byte limit",
        ));
    }
    Ok(text.into_bytes())
}

pub(crate) fn preflight(plan: Plan<'_>) -> Result<(), Error> {
    plan.validate()
}
