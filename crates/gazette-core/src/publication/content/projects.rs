//! Project subjects and bounded structural context propagation.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::resolve::{charge, endpoint, report};
use super::{CollectionMembership, ContentKind, ContentNode, Diagnostic, Error, Key, Limits};

/// Authored relationship intent; context is not itself an assertion of subject matter.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectDeclarationKind {
    /// A page or root collection explicitly declares its own subject.
    About,
    /// A page or root collection references a project without declaring it the subject.
    Mentions,
    /// A collection or journey supplies context to structurally contained pages.
    Context,
}

/// One explicit declaration, retaining its content owner rather than traversal paths.
#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct ProjectDeclaration {
    /// Exact declaring page or container.
    pub content: Key,
    /// Exact project-catalog key, not a display name or inferred placement.
    pub project: Key,
    /// Direct page intent or container context.
    pub kind: ProjectDeclarationKind,
}

/// Resolved page-to-project assertion; inherited context can only yield mentions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectRelation {
    /// Explicit contribution subject, taking precedence over mentions of the same project.
    About,
    /// Direct mention or structurally inherited context, never inferred about.
    Mentions,
}

/// Distinct declaration origin; equivalent routes do not duplicate this evidence.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, schemars::JsonSchema)]
pub struct ProjectEvidence {
    /// Content key where the explicit declaration was made.
    pub source: Key,
    /// Original intent, including mentions/context suppressed by direct about.
    pub kind: ProjectDeclarationKind,
}

/// Immutable resolved assertion and its complete set of contributing declarations.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct ProjectLink {
    relation: ProjectRelation,
    evidence: BTreeSet<ProjectEvidence>,
}

impl ProjectLink {
    /// Final assertion; consumers must not repeat precedence or inheritance logic.
    pub fn relation(&self) -> ProjectRelation {
        self.relation
    }

    /// All explicit origins, including evidence suppressed by about precedence.
    pub fn evidence(&self) -> &BTreeSet<ProjectEvidence> {
        &self.evidence
    }
}

pub(super) type ProjectIndex = BTreeMap<Key, BTreeMap<Key, ProjectLink>>;

pub(super) fn validate(
    input: Vec<ProjectDeclaration>,
    nodes: &BTreeMap<Key, ContentNode>,
    errors: &mut Vec<Diagnostic>,
) -> BTreeSet<ProjectDeclaration> {
    let mut declarations = BTreeSet::new();
    for declaration in input {
        endpoint(
            nodes,
            &declaration.content,
            &declaration.content,
            |node| {
                let page = matches!(node.structure, ContentKind::Page { .. });
                match declaration.kind {
                    ProjectDeclarationKind::Context => !page,
                    ProjectDeclarationKind::About | ProjectDeclarationKind::Mentions => {
                        page || matches!(
                            node.structure,
                            ContentKind::Collection { parent: None, .. }
                        )
                    }
                }
            },
            errors,
        );
        if !declarations.insert(declaration.clone()) {
            report(
                errors,
                "project_duplicate_declaration",
                &declaration.content,
                Some(&declaration.project),
            );
        }
    }
    declarations
}

pub(super) fn resolve(
    nodes: &BTreeMap<Key, ContentNode>,
    memberships: &BTreeSet<CollectionMembership>,
    declarations: &BTreeSet<ProjectDeclaration>,
    limits: Limits,
) -> Result<ProjectIndex, Error> {
    // Structural validation precedes this phase. Borrow keys and traverse only
    // structural edges, never editorial relations or inferred folder ancestry.
    let mut children: BTreeMap<&Key, BTreeSet<&Key>> = BTreeMap::new();
    let mut links: ProjectIndex = BTreeMap::new();
    for node in nodes.values() {
        match &node.structure {
            ContentKind::Page { .. } => {
                links.insert(node.id.clone(), BTreeMap::new());
            }
            ContentKind::Collection { parent, .. } => {
                if let Some(parent) = parent {
                    children.entry(parent).or_default().insert(&node.id);
                }
            }
            ContentKind::Journey {
                collection,
                members,
            } => {
                if let Some(collection) = collection {
                    children.entry(collection).or_default().insert(&node.id);
                }
                children
                    .entry(&node.id)
                    .or_default()
                    .extend(members.iter().map(|member| &member.content));
            }
        }
    }
    for membership in memberships {
        children
            .entry(&membership.collection)
            .or_default()
            .insert(&membership.content);
    }
    let mut budget = Budget {
        limits,
        steps: 0,
        evidence: 0,
        text: 0,
    };
    for declaration in declarations {
        if declaration.kind != ProjectDeclarationKind::Context {
            budget.insert(&mut links, &declaration.content, declaration)?;
            continue;
        }
        let mut visited = BTreeSet::from([&declaration.content]);
        let mut pending = vec![&declaration.content];
        while let Some(source) = pending.pop() {
            if nodes
                .get(source)
                .is_some_and(|node| matches!(node.structure, ContentKind::Page { .. }))
            {
                budget.insert(&mut links, source, declaration)?;
            }
            if let Some(descendants) = children.get(source) {
                for &child in descendants {
                    charge(
                        &mut budget.steps,
                        1,
                        limits.edges,
                        "project traversal steps",
                    )?;
                    if visited.insert(child) {
                        pending.push(child);
                    }
                }
            }
        }
    }
    Ok(links)
}

// Independent ceilings bound input, propagation work and retained output. A
// declaration reaching the same leaf by several routes retains one origin only.
struct Budget {
    limits: Limits,
    steps: usize,
    evidence: usize,
    text: usize,
}

impl Budget {
    fn insert(
        &mut self,
        links: &mut ProjectIndex,
        page: &Key,
        declaration: &ProjectDeclaration,
    ) -> Result<(), Error> {
        // Validated declaration uniqueness and visited-on-enqueue mean this
        // (page, project, source, kind) tuple is new. Do not clone keys or retain
        // output until its complete evidence budget has been accepted.
        charge(&mut self.evidence, 1, self.limits.edges, "project evidence")?;
        // Conservatively count every evidence tuple's page/project/source keys,
        // including repeated map keys. Bound before retaining the new output.
        for key in [page, &declaration.project, &declaration.content] {
            charge(
                &mut self.text,
                key.as_str().len(),
                self.limits.text_bytes,
                "project evidence text bytes",
            )?;
        }
        let link = links
            .entry(page.clone())
            .or_default()
            .entry(declaration.project.clone())
            .or_insert_with(|| ProjectLink {
                relation: ProjectRelation::Mentions,
                evidence: BTreeSet::new(),
            });
        if declaration.kind == ProjectDeclarationKind::About {
            link.relation = ProjectRelation::About;
        }
        link.evidence.insert(ProjectEvidence {
            source: declaration.content.clone(),
            kind: declaration.kind,
        });
        Ok(())
    }
}
