//! Deterministic, source-eligible suggestions over resolved meaning, not new graph edges.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::LazyLock,
};

use gazette_core::markdown::LinkKind;
use gazette_core::publication::{
    Key, LanguageTag,
    content::{ContentKind, ContentNode, EditorialDate, PageFamily, ProjectRelation},
};
use serde::Serialize;
use unicode_normalization::UnicodeNormalization;

use super::{ProjectionError, ResolvedAuthored, SitePresentation, navigation::NavigationBuilder};
use crate::workspace::ResolvedRoute;

// Generated from the retained English search provider's vocabulary; its bytes and
// MIT notice stay unchanged. The JS SDK's parity test checks this native data.
static ENGLISH_STOP_WORDS: LazyLock<BTreeSet<&'static str>> = LazyLock::new(|| {
    include_str!("english-stopwords.txt")
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect()
});

/// Versioned, bounded suggestions; authored `related` remains independent and unchanged.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct Recommendations<'a> {
    /// Exact deterministic algorithm contract, including weights and eligibility.
    pub algorithm: &'static str,
    /// At most five relevant destinations in descending score, then content-ID order.
    #[schemars(length(max = 5))]
    pub items: Vec<Recommendation<'a>>,
}

/// A shallow destination with ranking evidence, not a copied content record or relationship.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct Recommendation<'a> {
    /// Existing canonical content identity.
    pub id: &'a Key,
    /// Exact declared publication home, independent of the current page.
    pub site: &'a Key,
    /// Authored title, never derived from a URL or filename.
    pub title: &'a str,
    /// Optional authored description, not an inferred summary.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<&'a str>,
    /// The already compiled serving and canonical addresses.
    pub route: &'a ResolvedRoute,
    /// Exact configured destination presentation, when selected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presentation: Option<SitePresentation>,
    /// Native destination facts relative to the recommending page, not the target's home.
    pub destination: crate::markdown::LinkDestination,
    /// Optional decorative listing artwork, not a semantic article image assertion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artwork: Option<super::ListingArtwork>,
    /// Exact authored language, without publication fallback.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<LanguageTag>,
    /// Exact authored publication chronology, retaining date or timestamp precision.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published: Option<EditorialDate>,
    /// Exact authored modification chronology; absence stays absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified: Option<EditorialDate>,
    /// Direct ordered attributions resolved by the shared shallow listing projection.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[schemars(default)]
    pub attributions: Vec<super::ListingAttribution>,
    /// Exact declared classification pairs, with independently configured destinations.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[schemars(default)]
    pub classifications: Vec<super::ClassificationView<'a>>,
    /// Integer relevance score under `gazette-related/5`, not confidence or truth.
    #[schemars(range(min = 1))]
    pub score: u32,
    /// Independent evidence categories in the algorithm's stable order.
    pub reasons: Vec<RecommendationReason>,
}

/// Why a destination ranked; these labels do not assert a new editorial relationship.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum RecommendationReason {
    /// This page explicitly names the destination as related.
    OutgoingRelated,
    /// The destination explicitly names this page as related.
    IncomingRelated,
    /// This page links to the destination in its authored body or notes.
    OutgoingReference,
    /// The destination links to this page in its authored body or notes.
    IncomingReference,
    /// Both pages are members of an eligible complete declared journey.
    SharedJourney,
    /// Both pages have the same eligible immediate collection context.
    SharedCollection,
    /// Both pages have an eligible resolved About or Mentions assertion for a project.
    SharedProject,
    /// Both pages declare the same normalized keyword phrase.
    SharedKeywords,
    /// Metadata strengthens an independently admitted candidate under the versioned floor.
    TopicalMetadata,
}

#[derive(Debug, Default)]
pub(super) struct Index {
    features: BTreeMap<Key, Features>,
    frequencies: BTreeMap<String, usize>,
}

#[derive(Debug, Default)]
struct Features {
    outgoing: BTreeSet<Key>,
    incoming: BTreeSet<Key>,
    references: BTreeSet<Key>,
    referenced_by: BTreeSet<Key>,
    journeys: BTreeSet<Key>,
    collections: BTreeSet<Key>,
    projects: BTreeMap<Key, ProjectRelation>,
    keywords: BTreeSet<String>,
    tokens: BTreeMap<String, u32>,
}

fn supported(node: &ContentNode) -> bool {
    !matches!(
        node.structure,
        ContentKind::Page {
            family: PageFamily::Utility | PageFamily::Index | PageFamily::Extension { .. }
        }
    )
}

fn normalized(value: &str) -> String {
    value.nfkc().flat_map(char::to_lowercase).collect()
}

fn metadata(node: &ContentNode) -> Features {
    let mut result = Features::default();
    // Prose language belongs to the record, independently of a browser's search
    // profile. Missing and other languages never acquire an English fallback.
    let english = node
        .language
        .as_ref()
        .is_some_and(|language| language.as_str().split('-').next() == Some("en"));
    for (text, weight) in std::iter::once((node.title.as_str(), 3))
        .chain(node.description.as_deref().map(|value| (value, 1)))
        .chain(node.keywords.iter().map(|value| (value.as_str(), 3)))
    {
        for token in normalized(text).split(|c: char| !c.is_alphanumeric()) {
            if token.chars().count() >= 3 && !(english && ENGLISH_STOP_WORDS.contains(token)) {
                result
                    .tokens
                    .entry(token.to_owned())
                    .and_modify(|old| *old = (*old).max(weight))
                    .or_insert(weight);
            }
        }
    }
    result.keywords = node
        .keywords
        .iter()
        .map(|value| {
            normalized(value)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|value| !value.is_empty())
        .collect();
    result
}

impl Index {
    pub(super) fn new(owner: &ResolvedAuthored<'_, '_>) -> Result<Self, ProjectionError> {
        let graph = owner.content().graph();
        // Inspection may contain deliberately unbound nodes. Automatic suggestions
        // require a positive native source binding, even outside discovery rendering.
        let discoverable = |id: &Key| {
            owner
                .source_binding(id)
                .is_ok_and(|source| source.discoverable)
        };
        let mut result = Self::default();
        for node in graph
            .nodes()
            .filter(|node| supported(node) && discoverable(&node.id))
        {
            if owner
                .content()
                .route(&node.id)
                .is_none_or(|route| route.url.is_empty() || route.canonical_url.is_empty())
                || node
                    .route
                    .as_ref()
                    .and_then(|route| route.canonical.as_ref())
                    .is_some_and(|id| id != &node.id)
            {
                continue;
            }
            let mut feature = metadata(node);
            for term in feature.tokens.keys() {
                *result.frequencies.entry(term.clone()).or_default() += 1;
            }
            if let Some(projects) = graph.project_relations(&node.id) {
                for (project, link) in projects {
                    if link
                        .evidence()
                        .iter()
                        .all(|evidence| discoverable(&evidence.source))
                    {
                        feature.projects.insert(project.clone(), link.relation());
                    }
                }
            }
            result.features.insert(node.id.clone(), feature);
        }
        let mut references_remaining = owner.projection_limits.edges;
        let mut references = Vec::new();
        for id in result.features.keys() {
            let node = graph.node(id).expect("indexed graph node");
            let Some(path) = owner.source_path(id) else {
                continue;
            };
            let Some(main) = owner.file(id) else { continue };
            let notes = owner
                .page_package(id)
                .and_then(|package| package.notes())
                .and_then(|notes| owner.corpus.files().get_key_value(notes.path()));
            let assets = owner
                .assets_for(&node.site)
                .map_err(ProjectionError::Asset)?;
            for (path, file, is_notes) in std::iter::once((path, main, false))
                .chain(notes.map(|(path, file)| (path, file, true)))
            {
                let Some(body) = file.body() else { continue };
                for link in body
                    .source_links()
                    .iter()
                    .filter(|link| link.emitted && link.kind == LinkKind::Link)
                {
                    references_remaining =
                        references_remaining
                            .checked_sub(1)
                            .ok_or(ProjectionError::Limit(
                                "recommendation reference occurrences",
                            ))?;
                    let resolved = if is_notes {
                        owner.resolve_notes_reference(
                            id,
                            assets,
                            link.form,
                            link.kind,
                            &link.destination,
                            link.location.span,
                        )
                    } else {
                        owner.resolve_reference(
                            id,
                            assets,
                            link.form,
                            link.kind,
                            &link.destination,
                            link.location.span,
                        )
                    }
                    .map_err(|error| ProjectionError::body(path, file, error))?;
                    let mapped = resolved
                        .as_ref()
                        .map_or(link.destination.as_str(), |value| {
                            value.destination.as_str()
                        });
                    let effective = if mapped.starts_with('/') {
                        owner
                            .content()
                            .route(id)
                            .and_then(|route| url::Url::parse(&route.url).ok())
                            .and_then(|base| base.join(mapped).ok())
                            .map(String::from)
                    } else {
                        None
                    };
                    let destination = owner.link_destination(
                        node,
                        &link.destination,
                        effective.as_deref().unwrap_or(mapped),
                        resolved.as_ref(),
                    );
                    if let Some(target) = destination
                        .content
                        .filter(|target| target != id && result.features.contains_key(target))
                    {
                        references.push((id.clone(), target));
                    }
                }
            }
        }
        for (source, target) in references {
            result
                .features
                .get_mut(&source)
                .expect("indexed source")
                .references
                .insert(target.clone());
            result
                .features
                .get_mut(&target)
                .expect("indexed target")
                .referenced_by
                .insert(source);
        }
        for edge in graph.related() {
            if result.features.contains_key(&edge.source)
                && result.features.contains_key(&edge.target)
            {
                result
                    .features
                    .get_mut(&edge.source)
                    .expect("retained source")
                    .outgoing
                    .insert(edge.target.clone());
                result
                    .features
                    .get_mut(&edge.target)
                    .expect("retained target")
                    .incoming
                    .insert(edge.source.clone());
            }
        }
        for edge in graph
            .memberships()
            .iter()
            .filter(|edge| discoverable(&edge.collection))
        {
            if let Some(feature) = result.features.get_mut(&edge.content) {
                feature.collections.insert(edge.collection.clone());
            }
        }
        for node in graph.nodes().filter(|node| discoverable(&node.id)) {
            if let ContentKind::Journey {
                collection,
                members,
            } = &node.structure
            {
                // A hidden member cannot leak a private journey through a suggestion.
                if !members.iter().all(|member| discoverable(&member.content)) {
                    continue;
                }
                for member in members {
                    if let Some(feature) = result.features.get_mut(&member.content) {
                        feature.journeys.insert(node.id.clone());
                        if let Some(collection) = collection.as_ref().filter(|id| discoverable(id))
                        {
                            feature.collections.insert(collection.clone());
                        }
                    }
                }
            }
        }
        Ok(result)
    }

    fn score(&self, source: &Features, target: &Features) -> (u32, Vec<RecommendationReason>) {
        let mut score = 0;
        let mut reasons = Vec::new();
        let mut add = |points, reason| {
            if points > 0 {
                score += points;
                reasons.push(reason);
            }
        };
        // Directed evidence is supplied separately by the exact candidate identity.
        add(
            u32::from(!source.journeys.is_disjoint(&target.journeys)) * 80,
            RecommendationReason::SharedJourney,
        );
        add(
            u32::from(!source.collections.is_disjoint(&target.collections)) * 50,
            RecommendationReason::SharedCollection,
        );
        let project = source
            .projects
            .iter()
            .filter_map(|(id, left)| {
                target.projects.get(id).map(|right| match (left, right) {
                    (ProjectRelation::About, ProjectRelation::About) => 60,
                    (ProjectRelation::Mentions, ProjectRelation::Mentions) => 15,
                    _ => 35,
                })
            })
            .max()
            .unwrap_or(0);
        add(project, RecommendationReason::SharedProject);
        add(
            source
                .keywords
                .intersection(&target.keywords)
                .take(3)
                .count() as u32
                * 24,
            RecommendationReason::SharedKeywords,
        );
        let count = self.features.len();
        let mut matches = 0;
        let mut anchors = 0;
        let lexical = source
            .tokens
            .iter()
            .filter_map(|(token, left)| {
                let right = target.tokens.get(token)?;
                let frequency = self.frequencies[token];
                if frequency == count {
                    return None;
                }
                matches += 1;
                if *left == 3 && *right == 3 {
                    anchors += 1;
                }
                Some(((*left + *right) as usize * (count - frequency)).div_ceil(count) as u32)
            })
            .fold(0u32, u32::saturating_add)
            .min(48);
        if matches >= 2 && anchors >= 2 && lexical >= 8 {
            add(lexical, RecommendationReason::TopicalMetadata);
        }
        (score, reasons)
    }
}

impl ResolvedAuthored<'_, '_> {
    pub(super) fn recommendations<'a>(
        &'a self,
        source: &ContentNode,
        navigation: &mut NavigationBuilder<'a>,
    ) -> Result<Recommendations<'a>, ProjectionError> {
        if self.recommendation_index.get().is_none() {
            let index = Index::new(self)?;
            let _ = self.recommendation_index.set(index);
        }
        let index = self
            .recommendation_index
            .get()
            .expect("initialized recommendation index");
        let mut result = Recommendations {
            algorithm: "gazette-related/5",
            items: Vec::new(),
        };
        let Some(feature) = index.features.get(&source.id) else {
            return Ok(result);
        };
        let mut ranked = Vec::new();
        for (id, target) in &index.features {
            if id == &source.id {
                continue;
            }
            let (mut score, mut reasons) = index.score(feature, target);
            if feature.outgoing.contains(id) {
                score += 120;
                reasons.insert(0, RecommendationReason::OutgoingRelated);
            }
            if feature.incoming.contains(id) {
                score += 100;
                let position = usize::from(feature.outgoing.contains(id));
                reasons.insert(position, RecommendationReason::IncomingRelated);
            }
            if feature.references.contains(id) {
                score += 90;
                reasons.push(RecommendationReason::OutgoingReference);
            }
            if feature.referenced_by.contains(id) {
                score += 70;
                reasons.push(RecommendationReason::IncomingReference);
            }
            // Lexical overlap can order a grounded connection, but polysemous
            // words alone cannot establish one. Admission needs an authored graph
            // relationship or an exact normalized keyword phrase.
            if !reasons
                .iter()
                .any(|reason| *reason != RecommendationReason::TopicalMetadata)
            {
                continue;
            }
            ranked.push((id, score, reasons));
            ranked.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(right.0)));
            ranked.truncate(5);
        }
        let mut records_remaining = self.projection_limits.edges;
        let mut text_remaining = self.projection_limits.text_bytes;
        for (id, score, reasons) in ranked {
            let node = self.content().graph().node(id).expect("indexed graph node");
            let metadata = self
                .project_listing_link(source, node, &mut records_remaining, &mut text_remaining)
                .map_err(ProjectionError::Listing)?;
            let item = Recommendation {
                id: &node.id,
                site: &node.site,
                title: &node.title,
                description: node.description.as_deref(),
                route: self.content().route(id).expect("indexed compiled route"),
                presentation: self
                    .content()
                    .presentation_for(&node.site)
                    .map(SitePresentation::from),
                destination: metadata.destination,
                artwork: metadata.artwork,
                language: metadata.language,
                published: metadata.published,
                modified: metadata.modified,
                attributions: metadata.attributions,
                classifications: self
                    .classification_views_from(node, &source.site, &mut text_remaining)
                    .map_err(super::classifications::ClassificationError::projection)?,
                score,
                reasons,
            };
            navigation.charge(&item)?;
            result.items.push(item);
        }
        Ok(result)
    }
}
