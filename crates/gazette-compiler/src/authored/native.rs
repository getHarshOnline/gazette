//! Deterministic native-document defaults, distinct from authored declarations.

use std::fmt;

use gazette_core::{
    publication::{
        content::{ContentKind, ContentNode, PageFamily},
        routing::{InvalidRoute, Route},
    },
    source::{EntryPath, Sha256Digest, Span},
};
use serde::Serialize;

use super::{FileError, Metadata};
use crate::{
    markdown::{self, ParsedMarkdown},
    workspace::Compilation,
};

/// Source of a native document's display title; neither form is an authored metadata field.
#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum NativeTitle {
    /// Text of the first nonempty H1, using the retained parser's source coordinates.
    Heading {
        /// Exact complete native Markdown heading range.
        span: Span,
    },
    /// Literal filename stem, without invented display-name normalization.
    Filename,
}

/// Explicit evidence that node metadata was lowered from a configured native profile.
/// Identity, home, lifecycle, documentation family and route are derived/defaulted;
/// no authors, semantic subjects, memberships or publication permission are added.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct NativeDerivation {
    /// Display-title derivation, separate from explicit frontmatter coordinates.
    pub title: NativeTitle,
    /// The language came from the selected placement profile, not authored frontmatter.
    /// False means no language was supplied; no filename or body detection is performed.
    pub language_from_profile: bool,
}

/// Native metadata lowering failed; the original source remains retained unchanged.
#[derive(Debug)]
pub enum NativeError {
    /// Ambiguous/unsupported frontmatter cannot become plain text and lose its state.
    Framing,
    /// Retained Markdown inspection exhausted its own budget.
    Markdown(markdown::Error),
    /// Derived native spelling is unsupported by the shared public-route contract.
    Route(InvalidRoute),
    /// Native coordinates cannot form a nonempty document stem or deterministic key.
    Coordinate,
}

impl fmt::Display for NativeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Framing => f.write_str("unsupported or ambiguous native frontmatter; no lifecycle metadata may be discarded"),
            Self::Markdown(error) => error.fmt(f),
            Self::Route(error) => error.fmt(f),
            Self::Coordinate => f.write_str("native document requires a valid nonempty source-relative stem and identity"),
        }
    }
}

impl std::error::Error for NativeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Markdown(error) => Some(error),
            Self::Route(error) => Some(error),
            _ => None,
        }
    }
}

pub(super) fn lower(
    compilation: &Compilation,
    path: &EntryPath,
    body: &ParsedMarkdown<'_>,
) -> Result<(Metadata, NativeDerivation), FileError> {
    let source = compilation
        .workspace
        .content_source(path)
        .ok_or(FileError::NativeBindingRequired)?;
    let profile = source
        .native_markdown
        .ok_or(FileError::NativeBindingRequired)?;
    let owner = source
        .project
        .or_else(|| {
            source
                .documentation_mount
                .map(|mount| &mount.declaration.landing)
        })
        .ok_or(FileError::NativeBindingRequired)?;
    // Reject framing variants without normalizing the retained source or guessing
    // their metadata. Otherwise an unsupported draft marker could become defaults.
    let first = body
        .source()
        .trim_start_matches('\u{feff}')
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("");
    if ["---", "+++"].iter().any(|marker| {
        first
            .strip_prefix(marker)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
    }) {
        return Err(FileError::Native(NativeError::Framing));
    }
    let entry = source
        .entry(path)
        .ok_or(FileError::Native(NativeError::Coordinate))?;
    let (parent, filename) = entry.rsplit_once('/').map_or(("", entry), |parts| parts);
    let (stem, _) = filename
        .rsplit_once('.')
        .ok_or(FileError::Native(NativeError::Coordinate))?;
    if stem.is_empty() {
        return Err(FileError::Native(NativeError::Coordinate));
    }
    let relative = if stem.eq_ignore_ascii_case("readme") || stem.eq_ignore_ascii_case("index") {
        parent.to_owned()
    } else if parent.is_empty() {
        stem.to_owned()
    } else {
        format!("{parent}/{stem}")
    };
    let relative: Option<EntryPath> = if relative.is_empty() {
        None
    } else {
        Some(
            relative
                .parse()
                .map_err(|_| FileError::Native(NativeError::Coordinate))?,
        )
    };
    let route = profile
        .route_prefix
        .join_directory(relative.as_ref())
        .map_err(|error| FileError::Native(NativeError::Route(error)))?;
    let heading = body
        .headings()
        .map_err(|error| FileError::Native(NativeError::Markdown(error)))?
        .iter()
        .find(|heading| heading.level == 1 && !heading.text.trim().is_empty());
    let (title, title_origin) = match heading {
        Some(heading) => (
            heading.text.trim().to_owned(),
            NativeTitle::Heading { span: heading.span },
        ),
        None => (stem.to_owned(), NativeTitle::Filename),
    };
    // Each component excludes NUL. Content bytes, commit, native root, site and
    // presentation paths intentionally do not change this derived coordinate key.
    let coordinate = if source.documentation_mount.is_some() {
        format!(
            "gazette-mounted-markdown/1\0{}\0{owner}\0{}\0{entry}",
            source.site,
            source.source_key()
        )
    } else {
        format!(
            "gazette-native-markdown/1\0{owner}\0{}\0{entry}",
            source.source_key()
        )
    };
    let id = format!(
        "native-document:{}",
        Sha256Digest::of(coordinate.as_bytes())
    )
    .parse()
    .map_err(|_| FileError::Native(NativeError::Coordinate))?;
    Ok((
        Metadata {
            project: None,
            listing_artwork: None,
            images: Vec::new(),
            content: ContentNode {
                id,
                site: source.site.clone(),
                title,
                description: None,
                keywords: vec![],
                schema_type: None,
                subject: None,
                license: None,
                citation: None,
                published: None,
                modified: None,
                lifecycle: profile.lifecycle,
                language: profile.language.clone(),
                taxonomies: vec![],
                attributions: vec![],
                slug: None,
                route: Some(Route {
                    path: route,
                    canonical: None,
                    aliases: vec![],
                    mirrors: vec![],
                }),
                structure: ContentKind::Page {
                    family: PageFamily::Documentation,
                },
            },
            collections: vec![],
            template: None,
            resources: vec![],
            notebooks: vec![],
            related: vec![],
            browsing: vec![],
            projects: vec![],
        },
        NativeDerivation {
            title: title_origin,
            language_from_profile: profile.language.is_some(),
        },
    ))
}
