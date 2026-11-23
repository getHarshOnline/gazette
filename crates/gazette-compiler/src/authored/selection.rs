//! One closed candidate model before resource execution and all projections.

use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    fmt,
    sync::OnceLock,
};

use gazette_core::{
    publication::{
        ContentLifecycle, Key,
        content::{self, ContentKind, Disposition, PageFamily},
    },
    source::EntryPath,
};
use serde::{Deserialize, Serialize, de::IntoDeserializer};

use super::{ResolvedAuthored, SourceBindingError};
use crate::{corpus::File, markdown};

/// Complete explicit dispositions for a retained corpus. This is a mechanical
/// candidate selection, not a policy attestation, signed approval or release ABI.
/// Content relations follow their declaration owner. Assets are independently
/// selected, including opaque program inputs and notebook files.
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    /// Exactly one decision for every resolved content identity.
    #[serde(deserialize_with = "dispositions")]
    #[schemars(schema_with = "disposition_schema::<Key>")]
    pub content: BTreeMap<Key, Disposition>,
    /// Exactly one decision for every retained asset/notebook logical path.
    #[serde(deserialize_with = "dispositions")]
    #[schemars(schema_with = "disposition_schema::<EntryPath>")]
    pub assets: BTreeMap<EntryPath, Disposition>,
}

fn disposition_schema<K: schemars::JsonSchema>(
    generator: &mut schemars::SchemaGenerator,
) -> schemars::Schema {
    // The schema generator's generic map projection retains only a key's pattern, not
    // its other constraints. Use the complete native key schema without copying
    // identity/path grammar into candidate selection or its interface callers.
    schemars::json_schema!({
        "type": "object",
        "propertyNames": generator.subschema_for::<K>(),
        "additionalProperties": generator.subschema_for::<Disposition>()
    })
}

fn dispositions<'de, D, K>(decoder: D) -> Result<BTreeMap<K, Disposition>, D::Error>
where
    D: serde::Deserializer<'de>,
    K: Deserialize<'de> + Ord,
{
    // Values have the native enum grammar, but only its string wire spelling.
    BTreeMap::<K, String>::deserialize(decoder)?
        .into_iter()
        .map(|(key, value)| {
            Disposition::deserialize(value.into_deserializer()).map(|value| (key, value))
        })
        .collect()
}

/// Candidate selection failed before any resource or Theme program was invoked.
#[derive(Debug)]
pub enum SelectionError {
    /// An included owner requires its exact declared static artwork.
    Artwork(super::ListingArtworkError),
    /// Authored citation download could not be resolved.
    Citation(super::CitationError),
    /// Selected publication metadata lost its required same-site home endpoint.
    PublicationMetadata(super::PublicationMetadataError),
    /// A selected page's notebook attachment is not an included retained input.
    Notebook(super::NotebookError),
    /// Original structural closure or resource-ceiling failure.
    Content(content::Error),
    /// Draft content cannot enter this publication-candidate phase.
    Draft(Key),
    /// Complete asset decisions or selected resource inputs are inconsistent.
    Asset {
        /// Exact retained logical input, never a basename-only match.
        path: EntryPath,
        /// The violated selection or dependency constraint.
        reason: &'static str,
    },
    /// Source/home binding for a selected program is unresolved.
    Source(SourceBindingError),
    /// Retained body inspection or source-reference validation failed.
    Body {
        /// Selected declaration whose retained body could not be checked.
        content: Key,
        /// Original parser/reference failure and its native coordinates.
        error: markdown::Error,
    },
    /// A known canonical, alias or native-document link targets excluded content.
    Link {
        /// Selected body containing the reference.
        content: Key,
        /// Exact excluded content identity, possibly on another site.
        target: Key,
        /// Half-open UTF-8 byte range in the original Markdown body.
        span: gazette_core::source::Span,
    },
    /// Existing static mapping could not be constructed for reference validation.
    Assets(super::AssetError),
}

impl SelectionError {
    /// Stable diagnostic class; lower-level source and body codes survive.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Citation(error) => error.code(),
            Self::Artwork(error) => error.code(),
            Self::PublicationMetadata(error) => error.code(),
            Self::Notebook(error) => error.code(),
            Self::Content(_) => "selection_content",
            Self::Draft(_) => "selection_draft",
            Self::Asset { .. } => "selection_asset",
            Self::Source(error) => error.code(),
            Self::Body { error, .. } => error.code(),
            Self::Link { .. } => "selection_excluded_link",
            Self::Assets(error) => error.code(),
        }
    }
}
impl fmt::Display for SelectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Citation(error) => error.fmt(f),
            Self::Artwork(error) => error.fmt(f),
            Self::PublicationMetadata(error) => error.fmt(f),
            Self::Notebook(error) => error.fmt(f),
            Self::Content(error) => error.fmt(f),
            Self::Draft(id) => write!(f, "candidate includes draft {id}; exclude it explicitly"),
            Self::Asset { path, reason } => write!(f, "candidate asset {path}: {reason}"),
            Self::Source(error) => error.fmt(f),
            Self::Body { content, error } => write!(f, "candidate body {content}: {error}"),
            Self::Link {
                content,
                target,
                span,
            } => write!(
                f,
                "candidate {content} links to excluded {target} at body bytes {}..{}",
                span.start, span.end
            ),
            Self::Assets(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for SelectionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Citation(error) => Some(error),
            Self::Artwork(error) => Some(error),
            Self::PublicationMetadata(error) => Some(error),
            Self::Notebook(error) => Some(error),
            Self::Content(error) => Some(error),
            Self::Source(error) => Some(error),
            Self::Body { error, .. } => Some(error),
            Self::Assets(error) => Some(error),
            _ => None,
        }
    }
}

impl ResolvedAuthored<'_, '_> {
    /// Derive one complete owner-native project-documentation candidate.
    ///
    /// Published and archived documentation pages under an explicit project source
    /// are included. Drafts, publication-authored pages and other families are
    /// excluded. Only retained notebooks, program inputs and static references needed
    /// by included documentation are included; unrelated opaque files stay excluded.
    /// Callers may replace individual decisions with an explicit block before
    /// consumption. [`Self::into_selected`] remains the sole structural validator.
    /// This default establishes neither policy admission nor rights, privacy,
    /// evidence or publisher approval.
    ///
    /// # Errors
    /// Preserves source, notebook, program-input, retained-body, static-route and
    /// known-link closure failures without returning a partial selection.
    pub fn default_native_documentation_selection(&self) -> Result<Selection, SelectionError> {
        self.selection_with_dependencies(|node| {
            node.lifecycle != ContentLifecycle::Draft
                && matches!(
                    &node.structure,
                    ContentKind::Page {
                        family: PageFamily::Documentation
                    }
                )
                && self
                    .source_binding(&node.id)
                    .is_ok_and(|source| source.is_documentation())
        })
    }

    /// Derive complete non-draft local intent from authored lifecycle, not policy approval.
    ///
    /// # Errors
    /// Preserves required-resource and retained-source failures. Consumption through
    /// `into_selected` still rejects broken containment or references to excluded drafts.
    pub fn non_draft_selection(&self) -> Result<Selection, SelectionError> {
        self.selection_with_dependencies(|node| node.lifecycle != ContentLifecycle::Draft)
    }

    fn selection_with_dependencies(
        &self,
        included: impl Fn(&content::ContentNode) -> bool,
    ) -> Result<Selection, SelectionError> {
        if let Some(path) = self.asset_metadata.keys().next() {
            return Err(SelectionError::Asset {
                path: path.clone(),
                reason: "editor metadata requires a fresh complete capture before publication selection",
            });
        }
        let mut selection = Selection {
            content: self
                .content
                .graph()
                .nodes()
                .map(|node| {
                    let disposition = if included(node) {
                        Disposition::Include
                    } else {
                        Disposition::Exclude
                    };
                    (node.id.clone(), disposition)
                })
                .collect(),
            assets: self
                .corpus
                .files()
                .iter()
                .filter(|(_, file)| matches!(file, File::Asset(_) | File::Notebook(_)))
                .map(|(path, _)| (path.clone(), Disposition::Exclude))
                .collect(),
        };
        for (owner, package) in &self.page_packages {
            let disposition = selection
                .content
                .get(owner)
                .copied()
                .unwrap_or(Disposition::Exclude);
            for companion in package.companions() {
                if companion.kind() == super::PageCompanionKind::Notes {
                    continue;
                }
                *selection.assets.get_mut(companion.path()).ok_or_else(|| {
                    SelectionError::Asset {
                        path: companion.path().clone(),
                        reason: "page package companion is not a retained asset",
                    }
                })? = disposition;
            }
        }
        for node in self
            .content
            .graph()
            .nodes()
            .filter(|node| selection.content.get(&node.id) == Some(&Disposition::Include))
        {
            for notebook in self.notebooks(&node.id).map_err(SelectionError::Notebook)? {
                *selection.assets.get_mut(&notebook.path).ok_or_else(|| {
                    SelectionError::Asset {
                        path: notebook.path.clone(),
                        reason: "selected page requires an included notebook",
                    }
                })? = Disposition::Include;
            }
            if !self.resources(&node.id).is_empty() {
                let source = self
                    .source_binding(&node.id)
                    .map_err(SelectionError::Source)?;
                for input in self
                    .resources(&node.id)
                    .iter()
                    .flat_map(|resource| &resource.inputs)
                {
                    let path = source
                        .logical_path(input)
                        .map_err(|_| SelectionError::Asset {
                            path: input.clone(),
                            reason: "invalid source-relative resource input",
                        })?;
                    *selection
                        .assets
                        .get_mut(&path)
                        .ok_or_else(|| SelectionError::Asset {
                            path: path.clone(),
                            reason: "selected program requires an included input",
                        })? = Disposition::Include;
                }
            }
        }
        self.for_each_selected_body_image(&selection.content, |path| {
            *selection
                .assets
                .get_mut(path)
                .ok_or_else(|| SelectionError::Asset {
                    path: path.clone(),
                    reason: "declared image is not an already retained asset",
                })? = Disposition::Include;
            Ok(())
        })?;
        self.for_each_selected_artwork_asset(&selection.content, |path| {
            *selection
                .assets
                .get_mut(path)
                .ok_or_else(|| SelectionError::Asset {
                    path: path.clone(),
                    reason: "declared artwork is not an already retained asset",
                })? = Disposition::Include;
            Ok(())
        })?;
        self.for_each_selected_link_asset(&selection.content, |path| {
            *selection
                .assets
                .get_mut(path)
                .ok_or_else(|| SelectionError::Asset {
                    path: path.clone(),
                    reason: "selected body requires an included asset",
                })? = Disposition::Include;
            Ok(())
        })?;
        Ok(selection)
    }

    /// Consume the private structural owner into one non-draft candidate owner.
    /// All renderers then use that same graph; retained syntax/configuration is
    /// borrowed unchanged, never reparsed. No public-field, rights, privacy or
    /// publisher decision is inferred. Raw HTML and unknown external destinations
    /// still require output-policy validation; this is not certification.
    ///
    /// # Errors
    /// Rejects incomplete decisions, drafts, broken structural/known-link closure,
    /// excluded program inputs or retained-source/asset errors. No partial owner escapes.
    pub fn into_selected(mut self, selection: &Selection) -> Result<Self, SelectionError> {
        if let Some(path) = self.asset_metadata.keys().next() {
            return Err(SelectionError::Asset {
                path: path.clone(),
                reason: "editor metadata requires a fresh complete capture before publication selection",
            });
        }
        if selection.content.len() > self.projection_limits.nodes {
            return Err(SelectionError::Content(content::Error::Limit(
                "content selection decisions",
            )));
        }
        if selection.assets.len() > self.projection_limits.nodes {
            return Err(SelectionError::Content(content::Error::Limit(
                "asset selection decisions",
            )));
        }
        for (path, file) in self.corpus.files() {
            if matches!(file, File::Asset(_) | File::Notebook(_))
                && !selection.assets.contains_key(path)
            {
                return Err(SelectionError::Asset {
                    path: path.clone(),
                    reason: "missing explicit disposition",
                });
            }
        }
        for path in selection.assets.keys() {
            if self
                .content
                .compilation()
                .workspace
                .content_source(path)
                .is_some_and(|source| source.is_retained_only(path))
                && selection.assets.get(path) != Some(&Disposition::Exclude)
            {
                return Err(SelectionError::Asset {
                    path: path.clone(),
                    reason: "retained-only inputs must remain explicitly excluded",
                });
            }
            if !matches!(
                self.corpus.files().get(path),
                Some(File::Asset(_) | File::Notebook(_))
            ) {
                return Err(SelectionError::Asset {
                    path: path.clone(),
                    reason: "unknown asset or non-asset input",
                });
            }
        }
        for (owner, package) in &self.page_packages {
            let owner_disposition = selection.content.get(owner);
            for companion in package.companions() {
                if companion.kind() == super::PageCompanionKind::Notes {
                    continue;
                }
                if selection.assets.get(companion.path()) != owner_disposition {
                    return Err(SelectionError::Asset {
                        path: companion.path().clone(),
                        reason: "page package companion disposition must match its owner",
                    });
                }
            }
        }
        for node in self
            .content
            .graph()
            .nodes()
            .filter(|node| selection.content.get(&node.id) == Some(&Disposition::Include))
        {
            if node.lifecycle == ContentLifecycle::Draft {
                return Err(SelectionError::Draft(node.id.clone()));
            }
            for notebook in self.notebooks(&node.id).map_err(SelectionError::Notebook)? {
                if selection.assets.get(&notebook.path) != Some(&Disposition::Include) {
                    return Err(SelectionError::Asset {
                        path: notebook.path,
                        reason: "selected page requires an included notebook",
                    });
                }
            }
            if !self.resources(&node.id).is_empty() {
                let source = self
                    .source_binding(&node.id)
                    .map_err(SelectionError::Source)?;
                for input in self
                    .resources(&node.id)
                    .iter()
                    .flat_map(|resource| &resource.inputs)
                {
                    let path = source
                        .logical_path(input)
                        .map_err(|_| SelectionError::Asset {
                            path: input.clone(),
                            reason: "invalid source-relative resource input",
                        })?;
                    if selection.assets.get(&path) != Some(&Disposition::Include) {
                        return Err(SelectionError::Asset {
                            path,
                            reason: "selected program requires an included input",
                        });
                    }
                }
            }
        }
        self.for_each_selected_body_image(&selection.content, |path| {
            if selection.assets.get(path) != Some(&Disposition::Include) {
                return Err(SelectionError::Asset {
                    path: path.clone(),
                    reason: "selected page requires its declared image rendition",
                });
            }
            Ok(())
        })?;
        self.for_each_selected_artwork_asset(&selection.content, |path| {
            if selection.assets.get(path) != Some(&Disposition::Include) {
                return Err(SelectionError::Asset {
                    path: path.clone(),
                    reason: "selected owner requires its declared artwork asset",
                });
            }
            Ok(())
        })?;
        self.check_selected_links(selection)?;
        self.content = self
            .content
            .into_selected(&selection.content, self.projection_limits)
            .map_err(SelectionError::Content)?;
        let retained = |id: &Key| self.content.graph().node(id).is_some();
        self.paths.retain(|id, _| retained(id));
        self.derivations.retain(|id, _| retained(id));
        self.resources.retain(|id, _| retained(id));
        self.listing_artwork.retain(|id, _| retained(id));
        self.image_declarations.retain(|id, _| retained(id));
        self.notebooks.retain(|id, _| retained(id));
        self.notebook_downloads.retain(|id, _| retained(id));
        self.page_packages.retain(|id, _| retained(id));
        self.templates.retain(|id, _| retained(id));
        self.selected_assets = Some(
            selection
                .assets
                .iter()
                .filter(|(_, disposition)| **disposition == Disposition::Include)
                .map(|(path, _)| path.clone())
                .collect::<BTreeSet<_>>(),
        );
        // A plan observed during private review must not survive the phase boundary.
        self.recommendation_index = OnceLock::new();
        for cache in self.assets.values_mut() {
            *cache = OnceLock::new();
        }
        self.validate_publication_metadata()
            .map_err(SelectionError::PublicationMetadata)?;
        Ok(self)
    }

    fn excluded_content(
        &self,
        content: &BTreeMap<Key, Disposition>,
    ) -> BTreeMap<Cow<'_, str>, &Key> {
        let mut excluded = BTreeMap::new();
        for node in self
            .content
            .graph()
            .nodes()
            .filter(|node| content.get(&node.id) == Some(&Disposition::Exclude))
        {
            let Some(route) = self.content.route(&node.id) else {
                continue;
            };
            for url in std::iter::once(route.url.as_str())
                .chain(route.aliases.iter().map(|alias| alias.url.as_str()))
                .chain(route.mirrors.iter().map(|mirror| mirror.url.as_str()))
            {
                excluded.insert(Cow::Borrowed(url), &node.id);
            }
            let Some((declared, configured)) = node.route.as_ref().zip(
                self.content
                    .compilation()
                    .workspace
                    .public_base_url(&node.site),
            ) else {
                continue;
            };
            // A session address does not make the original publication address
            // external. Reuse validated route declarations; never rewrite links.
            // Original route compilation already bounded this URL amplification.
            let home = (Some(configured) != self.content.base_url(&node.site))
                .then_some(configured)
                .into_iter()
                .flat_map(|base| {
                    std::iter::once(&declared.path)
                        .chain(&declared.aliases)
                        .map(move |path| base.resolve(path))
                });
            let mirrors = declared.mirrors.iter().filter_map(|mirror| {
                let base = self
                    .content
                    .compilation()
                    .workspace
                    .public_base_url(&mirror.site)?;
                (Some(base) != self.content.base_url(&mirror.site))
                    .then(|| base.resolve(&mirror.path))
            });
            for url in home.chain(mirrors) {
                if !excluded.contains_key(url.as_str()) {
                    excluded.insert(Cow::Owned(url), &node.id);
                }
            }
        }
        excluded
    }

    fn check_selected_links(&self, selection: &Selection) -> Result<(), SelectionError> {
        self.for_each_selected_link_asset(&selection.content, |path| {
            if selection.assets.get(path) != Some(&Disposition::Include) {
                return Err(SelectionError::Asset {
                    path: path.clone(),
                    reason: "selected body requires an included asset",
                });
            }
            Ok(())
        })
    }

    fn for_each_selected_link_asset(
        &self,
        content: &BTreeMap<Key, Disposition>,
        mut require: impl FnMut(&EntryPath) -> Result<(), SelectionError>,
    ) -> Result<(), SelectionError> {
        let excluded = self.excluded_content(content);
        for node in self
            .content
            .graph()
            .nodes()
            .filter(|node| content.get(&node.id) == Some(&Disposition::Include))
        {
            if let Some(citation) = &node.citation {
                let assets = self
                    .assets_for(&node.site)
                    .map_err(SelectionError::Assets)?;
                for (field, destination) in [("bibtex", &citation.bibtex), ("pdf", &citation.pdf)] {
                    if let Some(destination) = destination {
                        let reference = self
                            .citation_reference(&node.id, assets, field, destination)
                            .map_err(SelectionError::Citation)?;
                        if let Some(asset) = reference.asset {
                            require(&asset.source().path)?;
                        }
                        let mut address = url::Url::parse(&reference.destination).ok();
                        if let Some(url) = &mut address {
                            url.set_query(None);
                            url.set_fragment(None);
                            if let Some(path) = self.asset_at_url(url.as_str())? {
                                require(path)?;
                            }
                        }
                    }
                }
            }
            let Some(body) = self.file(&node.id).and_then(File::body) else {
                continue;
            };
            let notes = self
                .page_package(&node.id)
                .and_then(|package| package.notes())
                .and_then(|companion| self.corpus.files().get(companion.path()))
                .and_then(File::body);
            for (body, is_notes) in
                std::iter::once((body, false)).chain(notes.map(|body| (body, true)))
            {
                let inspection = body.inspect().map_err(|error| SelectionError::Body {
                    content: node.id.clone(),
                    error,
                })?;
                if inspection.links.is_empty() {
                    continue;
                }
                let base = self
                    .content
                    .route(&node.id)
                    .and_then(|route| url::Url::parse(&route.url).ok());
                let assets = self
                    .assets_for(&node.site)
                    .map_err(SelectionError::Assets)?;
                for link in inspection.links {
                    let resolved = if is_notes {
                        self.resolve_notes_reference(
                            &node.id,
                            assets,
                            link.form,
                            link.kind,
                            &link.destination,
                            link.span,
                        )
                    } else {
                        self.resolve_reference(
                            &node.id,
                            assets,
                            link.form,
                            link.kind,
                            &link.destination,
                            link.span,
                        )
                    }
                    .map_err(|error| SelectionError::Body {
                        content: node.id.clone(),
                        error,
                    })?;
                    if let Some(asset) = resolved.as_ref().and_then(|reference| reference.asset) {
                        require(&asset.source().path)?;
                    }
                    let destination = resolved
                        .as_ref()
                        .map_or(link.destination.as_str(), |reference| {
                            reference.destination.as_str()
                        });
                    let Some(mut url) = base
                        .as_ref()
                        .and_then(|base| base.join(destination).ok())
                        .or_else(|| url::Url::parse(destination).ok())
                    else {
                        continue;
                    };
                    url.set_query(None);
                    url.set_fragment(None);
                    if let Some(path) = self.asset_at_url(url.as_str())? {
                        require(path)?;
                    }
                    if let Some(target) = excluded.get(url.as_str()) {
                        return Err(SelectionError::Link {
                            content: node.id.clone(),
                            target: (*target).clone(),
                            span: link.span,
                        });
                    }
                }
            }
        }
        Ok(())
    }

    fn asset_at_url(&self, url: &str) -> Result<Option<&EntryPath>, SelectionError> {
        let workspace = &self.content.compilation().workspace;
        for (path, file) in self.corpus.files() {
            if !matches!(file, File::Asset(_) | File::Notebook(_)) {
                continue;
            }
            let Some(source) = workspace.content_source(path) else {
                continue;
            };
            let configured = workspace.public_base_url(source.site);
            let effective = self.content.base_url(source.site);
            for base in configured
                .into_iter()
                .chain(effective.filter(|base| Some(*base) != configured))
            {
                // Only a referenced configured/session base needs foreign address
                // resolution. No unrelated asset plan or payload is consumed.
                if !url.starts_with(base.as_str()) {
                    continue;
                }
                if let Some(route) = self
                    .asset_route(source, path)
                    .map_err(SelectionError::Assets)?
                    && base.resolve(&route) == url
                {
                    return Ok(Some(path));
                }
            }
        }
        Ok(None)
    }
}
