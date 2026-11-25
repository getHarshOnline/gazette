//! Borrowed target projection and byte-bounded JSON serialization.

use serde::Serialize;

use crate::{Config, Error};

#[derive(Serialize)]
pub(crate) struct Group<'a> {
    pub group: &'a str,
    pub pages: Vec<Page>,
}

#[derive(Serialize)]
pub(crate) struct ReferenceGroup {
    pub group: String,
    #[serde(flatten)]
    pub input: Input,
}

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Input {
    Pages(Vec<Page>),
    Sdk(Sdk),
    Openapi(OpenApi),
}

/// Target-native navigation, with hierarchy and ordering supplied by the artifact adapter.
/// Paths remain exact output routes; group labels do not supply project identities.
#[derive(Serialize)]
#[serde(untagged)]
pub(crate) enum Page {
    Path(String),
    Group { group: String, pages: Vec<Page> },
    Reference(ReferenceGroup),
}

#[derive(Serialize)]
pub(crate) struct OpenApi {
    pub source: String,
    pub directory: String,
}

#[derive(Serialize)]
pub(crate) struct Sdk {
    pub directory: String,
    pub format: &'static str,
    pub source: String,
}

#[derive(Serialize)]
struct Document<'a> {
    #[serde(rename = "$schema")]
    schema: &'static str,
    colors: Colors<'a>,
    name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    seo: Option<Seo<'a>>,
    navigation: Navigation<'a>,
    theme: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    integrations: Option<Integrations>,
}

#[derive(Serialize)]
struct Seo<'a> {
    metatags: Metatags<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    organization: Option<&'a gazette_core::documentation::Publisher<'a>>,
}

/// Global page metadata; Mintlify injects each entry into every page's head.
#[derive(Serialize)]
struct Metatags<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    canonical: Option<&'a str>,
    #[serde(rename = "tdm-reservation", skip_serializing_if = "Option::is_none")]
    tdm_reservation: Option<&'static str>,
    #[serde(rename = "tdm-policy", skip_serializing_if = "Option::is_none")]
    tdm_policy: Option<&'a str>,
}

#[derive(Serialize)]
struct Integrations {
    telemetry: Telemetry,
}

#[derive(Serialize)]
struct Telemetry {
    enabled: bool,
}

#[derive(Serialize)]
struct Colors<'a> {
    primary: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    light: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dark: Option<&'a str>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum Navigation<'a> {
    Groups { groups: Vec<Group<'a>> },
    Dropdowns { dropdowns: Vec<Dropdown<'a>> },
}

#[derive(Serialize)]
struct Dropdown<'a> {
    dropdown: &'a str,
    pages: Vec<Page>,
}

pub(crate) fn encode<'a>(
    config: &Config<'a>,
    groups: Vec<Group<'a>>,
    ceiling: u64,
) -> Result<Vec<u8>, Error> {
    let document = Document {
        schema: "https://mintlify.com/docs.json",
        colors: Colors {
            primary: config.primary_color,
            light: config.light_color,
            dark: config.dark_color,
        },
        name: config.name,
        description: config.metadata.description,
        seo: (config.public_base_url.is_some()
            || config.usage.is_some()
            || config.metadata.publisher.is_some())
        .then(|| Seo {
            metatags: Metatags {
                canonical: config
                    .public_base_url
                    .map(|base| base.as_str().trim_end_matches('/')),
                tdm_reservation: config.usage.map(|_| "1"),
                tdm_policy: config.usage.map(|usage| usage.terms_url.as_str()),
            },
            organization: config.metadata.publisher.as_ref(),
        }),
        navigation: if groups.len() > 1 {
            Navigation::Dropdowns {
                dropdowns: groups
                    .into_iter()
                    .map(|group| Dropdown {
                        dropdown: group.group,
                        pages: group.pages,
                    })
                    .collect(),
            }
        } else {
            Navigation::Groups { groups }
        },
        theme: config.theme,
        integrations: config.telemetry.map(|enabled| Integrations {
            telemetry: Telemetry { enabled },
        }),
    };
    crate::output::json(&document, "docs.json", ceiling)
}

/// Apply the owning publication's reservation to the documentation host.
///
/// A root `robots.txt` replaces Mintlify's generated file, which otherwise invites
/// training, and cites Mintlify's generated `sitemap.xml` under the public base.
/// `.well-known/tdmrep.json` carries the same rule as the site's, and explicit
/// `.mintignore` exceptions keep the dot-directory in the provider's file set.
pub(crate) fn usage(
    usage: &gazette_core::publication::usage::UsagePolicy<'_>,
    public_base_url: Option<&gazette_core::publication::routing::PublicBaseUrl>,
    output: &mut crate::output::Output,
    ignores: &mut Vec<u8>,
) -> Result<(), Error> {
    if !usage.crawlers.is_empty() {
        let mut robots = crate::output::BoundedBuffer::new(output.file_ceiling());
        let sitemap = public_base_url.map(|base| format!("{}sitemap.xml", base.as_str()));
        gazette_core::publication::usage::write_robots(
            &mut robots,
            usage.crawlers.iter().copied(),
            sitemap.as_deref(),
            Some(&usage.terms_url),
        )
        .map_err(|_| crate::invalid("robots.txt", "crawler rules exceed target byte limit"))?;
        output.add_owned("robots.txt", robots.finish())?;
    }
    output.add_owned(
        ".well-known/tdmrep.json",
        gazette_core::publication::usage::tdm_reservation(&usage.terms_url),
    )?;
    ignores.extend_from_slice(b"!.well-known/\n!.well-known/tdmrep.json\n");
    Ok(())
}

/// The provider discovers HTTP specifications through its ignore filter, while
/// SDK loaders read their selected artifacts directly. Admit only the selected
/// specifications and their parent directories; sibling artifacts stay ignored.
pub(crate) fn artifact_ignores(groups: &[Group<'_>], ceiling: u64) -> Result<Vec<u8>, Error> {
    use std::{collections::BTreeSet, io::Write};
    let sources: BTreeSet<_> = groups
        .iter()
        .flat_map(|group| &group.pages)
        .filter_map(|page| match page {
            Page::Reference(ReferenceGroup {
                input: Input::Openapi(api),
                ..
            }) => Some(api.source.as_str()),
            _ => None,
        })
        .collect();
    let parents: BTreeSet<_> = sources
        .iter()
        .filter_map(|source| source.rsplit_once('/').map(|(parent, _)| parent))
        .collect();
    let mut writer = crate::output::BoundedBuffer::new(ceiling);
    let limit = |_| {
        crate::invalid(
            ".mintignore",
            "artifact ignore rules exceed target byte limit",
        )
    };
    writer.write_all(b"artifacts/**\n").map_err(limit)?;
    for parent in parents {
        writeln!(writer, "!{parent}/").map_err(limit)?;
    }
    for source in sources {
        writeln!(writer, "!{source}").map_err(limit)?;
    }
    Ok(writer.finish())
}

#[cfg(test)]
#[path = "../tests/internal/navigation.rs"]
mod tests;
