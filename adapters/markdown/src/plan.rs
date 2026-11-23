//! Exact occurrence admission before formatter mutation or output allocation.

use crate::bounded::Budget;
use gazette_core::markdown::{
    ComponentBinding, Error, FootnoteBinding, HeadingBinding, Limits, LinkBinding, LinkForm,
    MathBinding, Plan, Syntax,
};
use gazette_core::source::Span;
use std::collections::{BTreeMap, BTreeSet};

/// Reuse the exact checked plan indexes during normalized-tree verification.
pub(crate) struct Bindings<'a> {
    pub(crate) headings: BTreeMap<Span, &'a HeadingBinding>,
    pub(crate) links: BTreeMap<Span, &'a LinkBinding>,
    pub(crate) notes: BTreeMap<Span, &'a FootnoteBinding>,
    pub(crate) references: BTreeMap<Span, &'a gazette_core::markdown::FootnoteReference>,
    pub(crate) math: BTreeMap<Span, &'a MathBinding>,
    pub(crate) components: BTreeMap<Span, &'a ComponentBinding>,
}

pub(crate) fn unique<'a, T>(
    values: impl IntoIterator<Item = &'a T>,
    span: impl Fn(&T) -> Span,
) -> Result<BTreeMap<Span, &'a T>, Error> {
    let mut indexed = BTreeMap::new();
    for value in values {
        if indexed.insert(span(value), value).is_some() {
            return Err(Error::InvalidPlan);
        }
    }
    Ok(indexed)
}

pub(crate) fn marker(literal: &str) -> bool {
    literal
        .trim()
        .strip_prefix("<!--")
        .and_then(|s| s.strip_suffix("-->"))
        .is_some_and(|s| s.trim() == "more")
}

pub(crate) fn validate<'a>(
    syntax: &'a Syntax,
    plan: &'a Plan,
    limits: Limits,
) -> Result<Bindings<'a>, Error> {
    if syntax.identity != plan.identity {
        return Err(Error::InvalidPlan);
    }
    let headings = unique(&syntax.headings, |h| h.location.span)?;
    let links = unique(syntax.links.iter().filter(|l| l.emitted), |l| {
        l.location.span
    })?;
    let notes = unique(&syntax.footnotes, |n| n.location.span)?;
    let selected_headings = unique(&plan.headings, |h| h.occurrence.location.span)?;
    let selected_links = unique(&plan.links, |l| l.occurrence.location.span)?;
    let selected_notes = unique(&plan.footnotes, |n| n.occurrence.location.span)?;
    let references = unique(&syntax.footnote_references, |r| r.location.span)?;
    let math = unique(syntax.math.iter().filter(|m| m.emitted), |m| {
        m.location.span
    })?;
    let selected_math = unique(&plan.math, |m| m.occurrence.location.span)?;
    let directives = unique(syntax.directives.iter().filter(|d| d.emitted), |d| {
        d.location.span
    })?;
    let components = unique(&plan.components, |c| c.occurrence.location.span)?;
    if headings.len() != selected_headings.len()
        || links.len() != selected_links.len()
        || notes.len() != selected_notes.len()
        || math.len() != selected_math.len()
        || directives.len() != components.len()
    {
        return Err(Error::InvalidPlan);
    }
    let mut budget = Budget(limits.output_bytes);
    let mut container_contents = false;
    for (span, binding) in &components {
        if directives.get(span).copied() != Some(&binding.occurrence) {
            return Err(Error::InvalidPlan);
        }
        if let Some(contents) = &binding.container_contents {
            if container_contents
                || binding.presentation_only
                || binding.reading_prefix.is_some()
                || !binding.arguments.is_empty()
            {
                return Err(Error::InvalidPlan);
            }
            container_contents = true;
            if let Some(markdown) = &contents.reading_markdown {
                budget.charge(markdown.len())?;
            }
        }
        let mut pending: Vec<_> = binding
            .arguments
            .values()
            .map(|value| (value, 0usize))
            .collect();
        let mut count = 0usize;
        while let Some((value, depth)) = pending.pop() {
            count += 1;
            if depth > limits.depth || count > limits.nodes {
                return Err(Error::Limit("component argument structure"));
            }
            if let Some(array) = value.as_array() {
                pending.extend(array.iter().map(|v| (v, depth + 1)));
            }
            if let Some(object) = value.as_object() {
                pending.extend(object.values().map(|v| (v, depth + 1)));
            }
        }
    }
    if !plan.components.is_empty() {
        let mut encoded = gazette_core::render::LimitedWriter::new(Vec::new(), limits.output_bytes);
        serde_json::to_writer(&mut encoded, &plan.components)
            .map_err(|_| Error::Limit("component binding bytes"))?;
        budget.charge(encoded.into_inner().len())?;
    }
    for (span, binding) in &selected_math {
        if math.get(span).copied() != Some(&binding.occurrence) {
            return Err(Error::InvalidPlan);
        }
        budget.charge(binding.mathml.len())?;
        crate::math::validate(
            &binding.mathml,
            &binding.occurrence.tex,
            binding.occurrence.display,
        )?;
    }
    let mut ids = BTreeSet::new();
    for (span, binding) in &selected_headings {
        if headings.get(span).copied() != Some(&binding.occurrence)
            || !(1..=6).contains(&binding.level)
            || !valid_id(&binding.id)
            || !ids.insert(binding.id.as_str())
        {
            return Err(Error::InvalidPlan);
        }
        budget.charge(binding.id.len())?;
    }
    let mut link_addresses = BTreeSet::new();
    for (span, binding) in &selected_links {
        if links.get(span).copied() != Some(&binding.occurrence)
            || !valid_destination(binding.occurrence.form, &binding.destination)
        {
            return Err(Error::InvalidPlan);
        }
        budget.charge(binding.destination.len())?;
        if (binding.image_sources.is_some()
            || binding.image_loading.is_some()
            || binding.image_fetch_priority.is_some()
            || binding.image_width.is_some()
            || binding.svg_layout.is_some())
            && binding.occurrence.kind != gazette_core::markdown::LinkKind::Image
        {
            return Err(Error::InvalidPlan);
        }
        if let Some(sources) = &binding.image_sources {
            let invalid_candidates = |candidates: &[gazette_core::markdown::ImageCandidate]| {
                candidates.len() > 17
                    || candidates
                        .windows(2)
                        .any(|pair| pair[0].width >= pair[1].width)
                    || candidates.iter().any(|candidate| {
                        candidate.url.contains([',', ' ', '\t', '\n', '\r'])
                            || !valid_destination(binding.occurrence.form, &candidate.url)
                    })
            };
            // A dark alternative without responsive candidates carries no layout sizes.
            let responsive = !sources.candidates.is_empty();
            if (responsive && !gazette_core::markdown::ImageSources::valid_sizes(&sources.sizes))
                || (!responsive && (!sources.sizes.is_empty() || sources.dark.is_none()))
                || binding.image_address.is_none()
                || invalid_candidates(&sources.candidates)
                || binding.inline_svg.is_some()
            {
                return Err(Error::InvalidPlan);
            }
            if let Some(dark) = &sources.dark
                && (dark.url.is_empty()
                    || dark
                        .url
                        .contains(|character: char| character.is_ascii_whitespace())
                    || dark.url.starts_with(',')
                    || dark.url.ends_with(',')
                    || !valid_destination(binding.occurrence.form, &dark.url)
                    || (!dark.candidates.is_empty() && !responsive)
                    || invalid_candidates(&dark.candidates))
            {
                return Err(Error::InvalidPlan);
            }
            budget.charge(sources.sizes.len())?;
            for candidate in sources
                .candidates
                .iter()
                .chain(sources.dark.iter().flat_map(|dark| &dark.candidates))
            {
                budget.charge(candidate.url.len())?;
            }
            if let Some(dark) = &sources.dark {
                budget.charge(dark.url.len())?;
            }
        }
        if binding.image_dimensions.is_some_and(|size| {
            size.width == 0
                || size.height == 0
                || binding.occurrence.kind != gazette_core::markdown::LinkKind::Image
        }) {
            return Err(Error::InvalidPlan);
        }
        if let Some(svg) = &binding.inline_svg {
            if binding.image_address.is_none()
                || binding.occurrence.kind != gazette_core::markdown::LinkKind::Image
            {
                return Err(Error::InvalidPlan);
            }
            budget.charge(svg.bytes())?;
            for id in svg.scoped_ids(
                binding
                    .image_address
                    .as_deref()
                    .expect("checked image address"),
            ) {
                if ids.contains(id.as_str())
                    || plan
                        .excerpt
                        .as_ref()
                        .is_some_and(|excerpt| excerpt.continuation == id)
                {
                    return Err(Error::InvalidPlan);
                }
                budget.charge(id.len())?;
            }
        }
        if let Some(address) = &binding.image_address {
            if binding.occurrence.kind != gazette_core::markdown::LinkKind::Image
                || address.is_empty()
                || address
                    .chars()
                    .any(|value| value.is_control() || value.is_whitespace())
                || !link_addresses.insert(address.as_str())
            {
                return Err(Error::InvalidPlan);
            }
            budget.charge(address.len())?;
        }
        if let Some(annotation) = &binding.annotation {
            if binding.occurrence.kind != gazette_core::markdown::LinkKind::Link
                || annotation.address.is_empty()
                || annotation
                    .address
                    .chars()
                    .any(|value| value.is_control() || value.is_whitespace())
                || !link_addresses.insert(annotation.address.as_str())
                || !annotation.relationships.is_empty() && annotation.content.is_none()
                || annotation.theme.is_some() && annotation.site.is_none()
                || annotation.palette.is_some() && annotation.theme.is_none()
            {
                return Err(Error::InvalidPlan);
            }
            let mut encoded =
                gazette_core::render::LimitedWriter::new(Vec::new(), limits.output_bytes);
            serde_json::to_writer(&mut encoded, annotation)
                .map_err(|_| Error::Limit("link annotation bytes"))?;
            budget.charge(encoded.into_inner().len())?;
        }
    }
    let mut aliases = BTreeSet::new();
    let mut labels = BTreeSet::new();
    for (span, binding) in &selected_notes {
        if notes.get(span).copied() != Some(&binding.occurrence)
            || binding.alias.is_empty()
            || !binding
                .alias
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
            || !aliases.insert(binding.alias.as_str())
            || !labels.insert(binding.occurrence.name.as_str())
        {
            return Err(Error::InvalidPlan);
        }
        budget.charge(binding.alias.len())?;
    }
    for html in &syntax.html {
        if !plan.excerpt.as_ref().is_some_and(|e| {
            e.marker == *html && marker(&html.literal) && !html.in_footnote_or_image
        }) {
            return Err(Error::Unsupported {
                location: html.location,
                reason: "raw HTML requires separately admitted rendering",
            });
        }
    }
    if let Some(excerpt) = &plan.excerpt {
        if syntax.html.len() != 1
            || syntax.html.first() != Some(&excerpt.marker)
            || !valid_id(&excerpt.continuation)
            || ids.contains(excerpt.continuation.as_str())
        {
            return Err(Error::InvalidPlan);
        }
        budget.charge(excerpt.continuation.len())?;
    }
    check_source(syntax)?;
    Ok(Bindings {
        headings: selected_headings,
        links: selected_links,
        notes: selected_notes,
        references,
        math: selected_math,
        components,
    })
}

pub(crate) fn check_source(syntax: &Syntax) -> Result<(), Error> {
    let mut ordinals = BTreeMap::<&str, u32>::new();
    for reference in &syntax.footnote_references {
        if reference.emitted {
            if reference.in_image {
                return Err(Error::InvalidPlan);
            }
            let number = ordinals.entry(reference.name.as_str()).or_default();
            *number = number.checked_add(1).ok_or(Error::InvalidPlan)?;
            if reference.number != *number {
                return Err(Error::InvalidPlan);
            }
        } else if reference.number != 0 {
            return Err(Error::InvalidPlan);
        }
    }
    Ok(())
}

fn valid_id(value: &str) -> bool {
    !value.is_empty() && !value.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// A syntactic wikilink is not permission to emit a symbolic/custom URI scheme.
/// Publication admission owns its target; this gate checks only the bound output.
pub(crate) fn valid_destination(form: LinkForm, value: &str) -> bool {
    if comrak::html::dangerous_url(value) || value.chars().any(char::is_control) {
        return false;
    }
    if form == LinkForm::Markdown {
        return true;
    }
    if value.contains('\\') {
        return false;
    }
    let value = value.trim_matches(' ');
    let Some(colon) = value.find(':') else {
        return true;
    };
    let scheme = &value[..colon];
    scheme.contains(['/', '?', '#'])
        || scheme.eq_ignore_ascii_case("https")
        || scheme.eq_ignore_ascii_case("http")
        || scheme.eq_ignore_ascii_case("mailto")
}
