//! Whole-set validation before bounded derived relationship indexing.

use std::collections::{BTreeMap, BTreeSet, btree_map::Entry};

use super::{
    ContentKind, ContentNode, Declaration, Diagnostic, Error, Graph, Key, Limits, PageFamily,
};

pub(super) type RouteDeriver<'a> = dyn Fn(super::RoutingContext<'_>) -> Result<BTreeMap<Key, super::super::routing::RoutePath>, Error>
    + 'a;

pub(super) fn graph(
    declaration: Declaration,
    limits: Limits,
    derive: Option<&RouteDeriver<'_>>,
) -> Result<Graph, Error> {
    let mut text = check_limits(&declaration, limits)?;
    let mut errors = Vec::new();
    let mut nodes = BTreeMap::new();
    for mut node in declaration.nodes {
        node.taxonomies.sort();
        for pair in node.taxonomies.windows(2) {
            if pair[0] == pair[1] {
                report(
                    &mut errors,
                    "content_duplicate_taxonomy",
                    &node.id,
                    Some(&pair[0].taxonomy),
                );
            }
        }
        if let Some(route) = &mut node.route {
            route.aliases.sort();
            route.mirrors.sort();
        }
        if node.title.trim().is_empty() || node.title.chars().any(char::is_control) {
            report(&mut errors, "content_title", &node.id, None);
        }
        if node.description.as_ref().is_some_and(|description| {
            description.trim().is_empty()
                || description
                    .chars()
                    .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\r' | '\t'))
        }) {
            report(&mut errors, "content_description", &node.id, None);
        }
        if node
            .citation
            .as_ref()
            .is_some_and(|citation| !citation.valid())
        {
            report(&mut errors, "content_citation", &node.id, None);
        }
        let mut keywords = BTreeSet::new();
        for keyword in &node.keywords {
            if keyword.trim().is_empty()
                || keyword.trim() != keyword
                || keyword.chars().any(char::is_control)
            {
                report(&mut errors, "content_keyword", &node.id, None);
            }
            if !keywords.insert(keyword) {
                report(&mut errors, "content_duplicate_keyword", &node.id, None);
            }
        }
        if (!node.keywords.is_empty()
            || node
                .schema_type
                .is_some_and(|kind| kind != super::SchemaType::AboutPage))
            && !matches!(
                node.structure,
                ContentKind::Page {
                    family: PageFamily::Article
                }
            )
        {
            report(&mut errors, "content_article_metadata_kind", &node.id, None);
        }
        match nodes.entry(node.id.clone()) {
            Entry::Vacant(entry) => {
                entry.insert(node);
            }
            Entry::Occupied(_) => report(&mut errors, "content_duplicate", &node.id, None),
        }
    }
    // A duplicate identity has no unambiguous body to validate. Do not let input
    // ordering choose which conflicting declaration produces further evidence.
    if errors.iter().any(|error| error.code == "content_duplicate") {
        return invalid(errors);
    }
    super::subject::validate(&nodes, &mut errors);
    let mut parents = BTreeMap::new();
    let mut residences: BTreeMap<Key, BTreeSet<Key>> = BTreeMap::new();
    for node in nodes.values() {
        if derive.is_none() {
            canonical_reference(node, &nodes, &mut errors);
        }
        let mut roles = BTreeSet::new();
        for attribution in &node.attributions {
            if !roles.insert(attribution) {
                report(
                    &mut errors,
                    "content_duplicate_attribution",
                    &node.id,
                    Some(&attribution.principal),
                );
            }
        }
        match &node.structure {
            ContentKind::Page { .. } => (),
            ContentKind::Collection { parent, .. } => {
                if let Some(parent) = parent
                    && containment(&nodes, &node.id, parent, is_collection, &mut errors)
                {
                    parents.insert(node.id.clone(), parent.clone());
                    residences
                        .entry(node.id.clone())
                        .or_default()
                        .insert(parent.clone());
                }
            }
            ContentKind::Journey {
                collection,
                members,
            } => {
                if let Some(collection) = collection {
                    containment(&nodes, &node.id, collection, is_collection, &mut errors);
                    residences
                        .entry(node.id.clone())
                        .or_default()
                        .insert(collection.clone());
                } else {
                    report(&mut errors, "journey_collection_required", &node.id, None);
                }
                let mut seen = BTreeSet::new();
                for member in members {
                    containment(&nodes, &node.id, &member.content, is_page, &mut errors);
                    residences
                        .entry(member.content.clone())
                        .or_default()
                        .insert(node.id.clone());
                    if !member.primary {
                        report(
                            &mut errors,
                            "journey_member_not_primary",
                            &node.id,
                            Some(&member.content),
                        );
                    }
                    if !seen.insert(&member.content) {
                        report(
                            &mut errors,
                            "journey_duplicate_member",
                            &node.id,
                            Some(&member.content),
                        );
                    }
                }
            }
        }
    }
    cycles(&parents, &mut errors);
    let mut memberships = BTreeSet::new();
    for membership in declaration.memberships {
        endpoint(
            &nodes,
            &membership.content,
            &membership.content,
            is_page,
            &mut errors,
        );
        containment(
            &nodes,
            &membership.content,
            &membership.collection,
            is_collection,
            &mut errors,
        );
        residences
            .entry(membership.content.clone())
            .or_default()
            .insert(membership.collection.clone());
        if !memberships.insert(membership.clone()) {
            report(
                &mut errors,
                "collection_duplicate_membership",
                &membership.content,
                Some(&membership.collection),
            );
        }
    }
    // Report every conflicting parent in identity order, not whichever declaration
    // happened to be visited last. Relatedness never contributes a residence.
    for (content, homes) in &residences {
        if homes.len() > 1 {
            for home in homes {
                report(
                    &mut errors,
                    "content_multiple_residence",
                    content,
                    Some(home),
                );
            }
        }
    }
    let mut related = BTreeSet::new();
    for relation in declaration.related {
        endpoint(
            &nodes,
            &relation.source,
            &relation.source,
            |_| true,
            &mut errors,
        );
        endpoint(
            &nodes,
            &relation.source,
            &relation.target,
            |_| true,
            &mut errors,
        );
        if !related.insert(relation.clone()) {
            report(
                &mut errors,
                "content_duplicate_related",
                &relation.source,
                Some(&relation.target),
            );
        }
    }
    let browsing =
        super::browsing::validate(declaration.browsing, &nodes, derive.is_none(), &mut errors);
    let projects = super::projects::validate(declaration.projects, &nodes, &mut errors);
    if !errors.is_empty() {
        return invalid(errors);
    }
    let project_links = super::projects::resolve(&nodes, &memberships, &projects, limits)?;
    // This index copies no more endpoint text than the already charged input edges.
    let mut browsing_by_source: BTreeMap<Key, BTreeSet<Key>> = BTreeMap::new();
    for relation in &browsing {
        browsing_by_source
            .entry(relation.source.clone())
            .or_default()
            .insert(relation.index.clone());
    }
    let residences: BTreeMap<Key, Key> = residences
        .into_iter()
        .filter_map(|(content, mut homes)| homes.pop_first().map(|home| (content, home)))
        .collect();
    let documentation = super::purpose::resolve(&nodes, &residences, &mut errors);
    let mut children = BTreeMap::<Key, Vec<Key>>::new();
    for (child, parent) in &residences {
        children
            .entry(parent.clone())
            .or_default()
            .push(child.clone());
    }
    super::order::apply(&nodes, &residences, &mut children, &mut errors);
    super::navigation::validate(&nodes, &children, &documentation, limits, &mut errors)?;
    if !errors.is_empty() {
        return invalid(errors);
    }
    let mut graph = Graph {
        nodes,
        memberships,
        related,
        browsing,
        browsing_by_source,
        residences,
        children,
        documentation,
        projects,
        project_links,
    };
    if let Some(derive) = derive {
        let paths = derive(super::RoutingContext { graph: &graph })?;
        for (id, path) in paths {
            let Some(node) = graph.nodes.get_mut(&id) else {
                report(&mut errors, "content_derived_route_reference", &id, None);
                continue;
            };
            if node.route.is_some() {
                report(&mut errors, "content_derived_route_replacement", &id, None);
                continue;
            }
            charge(
                &mut text,
                path.as_str().len(),
                limits.text_bytes,
                "text bytes",
            )?;
            node.route = Some(super::super::routing::Route {
                path,
                canonical: None,
                aliases: vec![],
                mirrors: vec![],
            });
        }
        for node in graph.nodes.values() {
            canonical_reference(node, &graph.nodes, &mut errors);
        }
        super::browsing::validate_routes(&graph.browsing, &graph.nodes, &mut errors);
        if !errors.is_empty() {
            return invalid(errors);
        }
    }
    Ok(graph)
}

fn canonical_reference(
    node: &ContentNode,
    nodes: &BTreeMap<Key, ContentNode>,
    errors: &mut Vec<Diagnostic>,
) {
    if let Some(canonical) = node
        .route
        .as_ref()
        .and_then(|route| route.canonical.as_ref())
    {
        let code = if canonical == &node.id {
            Some("content_canonical_self")
        } else {
            match nodes
                .get(canonical)
                .and_then(|target| target.route.as_ref())
            {
                None => Some("content_canonical_reference"),
                Some(target) if target.canonical.is_some() => Some("content_canonical_chain"),
                Some(_) => None,
            }
        };
        if let Some(code) = code {
            report(errors, code, &node.id, Some(canonical));
        }
    }
}

fn is_collection(node: &ContentNode) -> bool {
    matches!(node.structure, ContentKind::Collection { .. })
}
fn is_page(node: &ContentNode) -> bool {
    matches!(node.structure, ContentKind::Page { .. })
}

// A reference may cross publications; editorial containment may not. Validate
// kinds first so a wrong-kind target never acquires a misleading site diagnostic.
fn containment(
    nodes: &BTreeMap<Key, ContentNode>,
    source: &Key,
    target: &Key,
    expected: impl FnOnce(&ContentNode) -> bool,
    errors: &mut Vec<Diagnostic>,
) -> bool {
    if !endpoint(nodes, source, target, expected, errors) {
        return false;
    }
    if let (Some(source_node), Some(target_node)) = (nodes.get(source), nodes.get(target))
        && source_node.site != target_node.site
    {
        report(errors, "content_containment_site", source, Some(target));
        return false;
    }
    true
}

pub(super) fn endpoint(
    nodes: &BTreeMap<Key, ContentNode>,
    source: &Key,
    target: &Key,
    expected: impl FnOnce(&ContentNode) -> bool,
    errors: &mut Vec<Diagnostic>,
) -> bool {
    match nodes.get(target) {
        None => {
            report(errors, "content_reference", source, Some(target));
            false
        }
        Some(node) if !expected(node) => {
            report(errors, "content_reference_kind", source, Some(target));
            false
        }
        Some(_) => true,
    }
}

// Each collection has at most one parent. Mark completed chains once, making
// traversal linear in graph size apart from ordered-map lookups; never recurse.
fn cycles(parents: &BTreeMap<Key, Key>, errors: &mut Vec<Diagnostic>) {
    let mut done = BTreeSet::new();
    for start in parents.keys() {
        if done.contains(start) {
            continue;
        }
        let mut trail = Vec::new();
        let mut positions = BTreeMap::new();
        let mut cursor = start;
        while !done.contains(cursor) {
            if let Some(&offset) = positions.get(cursor) {
                for &id in &trail[offset..] {
                    report(errors, "collection_cycle", id, parents.get(id));
                }
                break;
            }
            positions.insert(cursor, trail.len());
            trail.push(cursor);
            let Some(parent) = parents.get(cursor) else {
                break;
            };
            cursor = parent;
        }
        done.extend(trail);
    }
}

fn check_limits(input: &Declaration, limits: Limits) -> Result<usize, Error> {
    if input.nodes.len() > limits.nodes {
        return Err(Error::Limit("nodes"));
    }
    let mut edges = 0;
    let mut text = 0;
    for node in &input.nodes {
        charge(
            &mut text,
            node.id.as_str().len(),
            limits.text_bytes,
            "text bytes",
        )?;
        charge(
            &mut text,
            node.site.as_str().len(),
            limits.text_bytes,
            "text bytes",
        )?;
        charge(&mut text, node.title.len(), limits.text_bytes, "text bytes")?;
        if let Some(description) = &node.description {
            charge(
                &mut text,
                description.len(),
                limits.text_bytes,
                "text bytes",
            )?;
        }
        if let Some(citation) = &node.citation {
            charge(&mut edges, citation.formats.len(), limits.edges, "edges")?;
            for value in citation.texts() {
                charge(&mut text, value.len(), limits.text_bytes, "text bytes")?;
            }
        }
        for keyword in &node.keywords {
            charge(&mut text, keyword.len(), limits.text_bytes, "text bytes")?;
        }
        if node.schema_type.is_some() {
            // All admitted subtype names fit this fixed upper bound.
            charge(&mut text, 11, limits.text_bytes, "text bytes")?;
        }
        if let Some(subject) = &node.subject {
            edge(&mut edges, &mut text, subject.principal(), limits)?;
            if let super::Subject::Service(service) = subject {
                edge(&mut edges, &mut text, &service.id, limits)?;
                charge(
                    &mut text,
                    service.name.len(),
                    limits.text_bytes,
                    "text bytes",
                )?;
                if let Some(description) = &service.description {
                    charge(
                        &mut text,
                        description.len(),
                        limits.text_bytes,
                        "text bytes",
                    )?;
                }
            }
        }
        for date in [&node.published, &node.modified].into_iter().flatten() {
            charge(
                &mut text,
                date.to_string().len(),
                limits.text_bytes,
                "text bytes",
            )?;
        }
        if let Some(license) = &node.license {
            charge(
                &mut text,
                license.as_str().len(),
                limits.text_bytes,
                "text bytes",
            )?;
        }
        if let Some(language) = &node.language {
            charge(
                &mut text,
                language.as_str().len(),
                limits.text_bytes,
                "text bytes",
            )?;
        }
        for membership in &node.taxonomies {
            edge(&mut edges, &mut text, &membership.taxonomy, limits)?;
            charge(
                &mut text,
                membership.term.as_str().len(),
                limits.text_bytes,
                "text bytes",
            )?;
        }
        for attribution in &node.attributions {
            edge(&mut edges, &mut text, &attribution.principal, limits)?;
        }
        if let Some(slug) = &node.slug {
            charge(
                &mut text,
                slug.as_str().len(),
                limits.text_bytes,
                "text bytes",
            )?;
        }
        if let Some(route) = &node.route {
            if let Some(canonical) = &route.canonical {
                edge(&mut edges, &mut text, canonical, limits)?;
            }
            charge(
                &mut text,
                route.path.as_str().len(),
                limits.text_bytes,
                "text bytes",
            )?;
            for alias in &route.aliases {
                charge(&mut edges, 1, limits.edges, "edges")?;
                charge(
                    &mut text,
                    alias.as_str().len(),
                    limits.text_bytes,
                    "text bytes",
                )?;
            }
            for mirror in &route.mirrors {
                edge(&mut edges, &mut text, &mirror.site, limits)?;
                charge(
                    &mut text,
                    mirror.path.as_str().len(),
                    limits.text_bytes,
                    "text bytes",
                )?;
            }
        }
        match &node.structure {
            ContentKind::Page {
                family:
                    PageFamily::Extension {
                        schema,
                        data,
                        presentation,
                    },
            } => {
                charge(
                    &mut text,
                    schema.as_str().len(),
                    limits.text_bytes,
                    "text bytes",
                )?;
                let mut output = crate::render::LimitedWriter::new(
                    Vec::new(),
                    limits.text_bytes.saturating_sub(text),
                );
                serde_json::to_writer(&mut output, data)
                    .map_err(|_| Error::Limit("extension data bytes"))?;
                charge(
                    &mut text,
                    output.into_inner().len(),
                    limits.text_bytes,
                    "text bytes",
                )?;
                if let Some(presentation) = presentation {
                    if presentation.amplitude > 4 {
                        return Err(Error::Invalid(vec![Diagnostic {
                            code: "content_extension_presentation_amplitude",
                            source: node.id.clone(),
                            target: None,
                        }]));
                    }
                    edge(&mut edges, &mut text, &presentation.mode, limits)?;
                    if let Some(variant) = &presentation.variant {
                        edge(&mut edges, &mut text, variant, limits)?;
                    }
                }
            }
            ContentKind::Page { .. } => (),
            ContentKind::Collection {
                parent,
                navigation,
                member_order,
                ..
            } => {
                if let Some(parent) = parent {
                    edge(&mut edges, &mut text, parent, limits)?;
                }
                if let Some(items) = navigation {
                    super::navigation::charge(items, &mut edges, &mut text, limits)?;
                }
                for member in member_order {
                    edge(&mut edges, &mut text, member, limits)?;
                }
            }
            ContentKind::Journey {
                collection,
                members,
            } => {
                if let Some(collection) = collection {
                    edge(&mut edges, &mut text, collection, limits)?;
                }
                for member in members {
                    edge(&mut edges, &mut text, &member.content, limits)?;
                }
            }
        }
    }
    for membership in &input.memberships {
        charge(
            &mut text,
            membership.content.as_str().len(),
            limits.text_bytes,
            "text bytes",
        )?;
        edge(&mut edges, &mut text, &membership.collection, limits)?;
    }
    for relation in &input.related {
        charge(
            &mut text,
            relation.source.as_str().len(),
            limits.text_bytes,
            "text bytes",
        )?;
        edge(&mut edges, &mut text, &relation.target, limits)?;
    }
    for relation in &input.browsing {
        charge(
            &mut text,
            relation.source.as_str().len(),
            limits.text_bytes,
            "text bytes",
        )?;
        edge(&mut edges, &mut text, &relation.index, limits)?;
    }
    for project in &input.projects {
        charge(
            &mut text,
            project.content.as_str().len(),
            limits.text_bytes,
            "text bytes",
        )?;
        edge(&mut edges, &mut text, &project.project, limits)?;
    }
    Ok(text)
}

fn edge(edges: &mut usize, text: &mut usize, target: &Key, limits: Limits) -> Result<(), Error> {
    charge(edges, 1, limits.edges, "edges")?;
    charge(text, target.as_str().len(), limits.text_bytes, "text bytes")
}

pub(super) fn charge(
    used: &mut usize,
    amount: usize,
    maximum: usize,
    resource: &'static str,
) -> Result<(), Error> {
    *used = used
        .checked_add(amount)
        .filter(|&value| value <= maximum)
        .ok_or(Error::Limit(resource))?;
    Ok(())
}

pub(super) fn report(
    errors: &mut Vec<Diagnostic>,
    code: &'static str,
    source: &Key,
    target: Option<&Key>,
) {
    errors.push(Diagnostic {
        code,
        source: source.clone(),
        target: target.cloned(),
    });
}

fn invalid(mut errors: Vec<Diagnostic>) -> Result<Graph, Error> {
    errors.sort();
    errors.dedup();
    Err(Error::Invalid(errors))
}
