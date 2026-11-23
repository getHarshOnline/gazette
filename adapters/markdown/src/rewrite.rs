//! Source-bound link projection through the adapter's Markdown formatter, not token surgery.

use crate::{
    bounded::{Budget, Output},
    inspect,
    plan::{unique, valid_destination},
    source::Lines,
};
use comrak::nodes::{NodeLink, NodeValue};
use gazette_core::markdown::{
    Error, Identity, Limits, LinkBinding, LinkForm, LinkKind, Syntax, TransportedMarkdown,
};
use std::fmt::Write;

pub(crate) fn document(
    source: &str,
    syntax: &Syntax,
    identity: Identity,
    links: &[LinkBinding],
    limits: Limits,
) -> Result<String, Error> {
    format(source, syntax, identity, links, Target::Markdown, limits).map(|result| result.body)
}

pub(crate) fn mdx(
    source: &str,
    syntax: &Syntax,
    identity: Identity,
    links: &[LinkBinding],
    projection: crate::mdx::Projection<'_>,
    limits: Limits,
) -> Result<String, Error> {
    format(
        source,
        syntax,
        identity,
        links,
        Target::Mdx(projection, false),
        limits,
    )
    .map(|result| result.body)
}

pub(crate) fn project_mdx(
    source: &str,
    syntax: &Syntax,
    identity: Identity,
    links: &[LinkBinding],
    projection: crate::mdx::Projection<'_>,
    limits: Limits,
) -> Result<TransportedMarkdown, Error> {
    format(
        source,
        syntax,
        identity,
        links,
        Target::Mdx(projection, true),
        limits,
    )
}

enum Target<'a> {
    Markdown,
    Mdx(crate::mdx::Projection<'a>, bool),
}

fn format(
    source: &str,
    syntax: &Syntax,
    identity: Identity,
    links: &[LinkBinding],
    target: Target<'_>,
    limits: Limits,
) -> Result<TransportedMarkdown, Error> {
    if identity != syntax.identity || links.len() != syntax.links.len() {
        return Err(Error::InvalidPlan);
    }
    let original = unique(&syntax.links, |link| link.location.span)?;
    let selected = unique(links, |link| link.occurrence.location.span)?;
    let mut budget = Budget(limits.output_bytes);
    let mut changed = false;
    for (span, binding) in &selected {
        if original.get(span).copied() != Some(&binding.occurrence)
            || !valid_destination(binding.occurrence.form, &binding.destination)
        {
            return Err(Error::InvalidPlan);
        }
        budget.charge(binding.destination.len())?;
        changed |= binding.destination != binding.occurrence.destination;
    }
    let mut output = Output::new(limits.output_bytes);
    if !changed && matches!(target, Target::Markdown) {
        output.write_str(source).map_err(|_| output.error())?;
        return Ok(TransportedMarkdown {
            body: output.value,
            text: None,
        });
    }
    if let Some(directive) = syntax.directives.first()
        && !matches!(
            &target,
            Target::Mdx(
                crate::mdx::Projection {
                    anchors: Some(_),
                    ..
                },
                _
            )
        )
    {
        return Err(Error::Unsupported {
            location: directive.header,
            reason: "native Markdown link formatting cannot preserve original component headers",
        });
    }
    // Comrak's CommonMark formatter does not serialize parsed heading attributes.
    // Do not silently erase a generated anchor or a class while changing a link.
    if let Some(heading) = syntax.headings.iter().find(|heading| {
        (matches!(target, Target::Markdown) && heading.id.is_some())
            || !heading.classes.is_empty()
            || !heading.attributes.is_empty()
    }) {
        return Err(Error::Unsupported {
            location: heading.location,
            reason: "native Markdown link formatting cannot preserve heading attributes",
        });
    }
    let arena = comrak::Arena::new();
    let mut options = crate::options(identity.profile, true);
    let root = comrak::parse_document(&arena, source, &options);
    inspect::check_tree(root, limits)?;
    let lines = Lines::new(source);
    let mut seen = 0;
    for node in root.descendants() {
        let mut data = node.data_mut();
        let span = match &data.value {
            NodeValue::Link(_) | NodeValue::Image(_) | NodeValue::WikiLink(_) => {
                lines.location(data.sourcepos)?.span
            }
            _ => continue,
        };
        let binding = selected.get(&span).ok_or(Error::InvalidSource)?;
        let (form, kind, destination, title) = match &data.value {
            NodeValue::Link(link) => (
                LinkForm::Markdown,
                LinkKind::Link,
                link.url.as_str(),
                link.title.as_str(),
            ),
            NodeValue::Image(link) => (
                LinkForm::Markdown,
                LinkKind::Image,
                link.url.as_str(),
                link.title.as_str(),
            ),
            NodeValue::WikiLink(link) => {
                (LinkForm::WikiLink, LinkKind::Link, link.url.as_str(), "")
            }
            _ => return Err(Error::InvalidSource),
        };
        if binding.occurrence.form != form
            || binding.occurrence.kind != kind
            || binding.occurrence.destination != destination
            || binding.occurrence.title != title
            || binding.occurrence.location != lines.location(data.sourcepos)?
        {
            return Err(Error::InvalidSource);
        }
        match &mut data.value {
            NodeValue::Link(link) | NodeValue::Image(link) => {
                link.url.clone_from(&binding.destination)
            }
            // The native CommonMark wikilink formatter does not escape literal
            // label pipes. Lower only the output tree to an ordinary link so its
            // existing formatter preserves the native label children faithfully.
            // The retained source/fact and unchanged fast path remain untouched.
            NodeValue::WikiLink(_) => {
                data.value = NodeValue::Link(Box::new(NodeLink {
                    url: binding.destination.clone(),
                    title: String::new(),
                }));
            }
            _ => return Err(Error::InvalidSource),
        }
        seen += 1;
    }
    if seen != selected.len() {
        return Err(Error::InvalidSource);
    }
    let text = if matches!(target, Target::Mdx(_, true)) {
        Some(crate::mdx_text::collect(root, syntax, &lines, limits)?)
    } else {
        None
    };
    if let Target::Mdx(projection, _) = target {
        if !changed
            && projection.title.is_none()
            && projection.headings.is_empty()
            && syntax.headings.is_empty()
            && !crate::mdx::needed(root, identity.profile)
        {
            output.write_str(source).map_err(|_| output.error())?;
            return Ok(TransportedMarkdown {
                body: output.value,
                text,
            });
        }
        crate::mdx::adapt(&arena, root, syntax, projection, &lines, limits)?;
        // Target adaptation must preserve the parser's block/inline invariants
        // in optimized builds too, before any serialized result is returned.
        root.validate().map_err(|_| Error::InvalidSource)?;
        options.render.prefer_fenced = true;
    }
    comrak::format_commonmark(root, &options, &mut output).map_err(|_| output.error())?;
    Ok(TransportedMarkdown {
        body: output.value,
        text,
    })
}
