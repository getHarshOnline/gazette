//! Native Markdown for a compiler-written navigation fragment, never Theme HTML.

use crate::bounded::Output;
use comrak::{arena_tree::NodeEdge, nodes::NodeValue};
use gazette_core::markdown::{Error, Limits};

pub(crate) fn reading(markdown: &str, limits: Limits) -> Result<String, Error> {
    if markdown.len() > limits.output_bytes {
        return Err(Error::Limit("container contents Markdown bytes"));
    }
    let arena = comrak::Arena::new();
    let options = crate::options(gazette_core::markdown::Profile::Publication, false);
    let root = comrak::parse_document(&arena, markdown, &options);
    let mut nodes = 0usize;
    let mut depth = 0usize;
    for edge in root.traverse() {
        match edge {
            NodeEdge::Start(node) => {
                nodes += 1;
                depth += 1;
                if nodes > limits.nodes || depth > limits.depth {
                    return Err(Error::Limit("container contents Markdown structure"));
                }
                if !matches!(
                    node.data().value,
                    NodeValue::Document
                        | NodeValue::Heading(_)
                        | NodeValue::Paragraph
                        | NodeValue::List(_)
                        | NodeValue::Item(_)
                        | NodeValue::Link(_)
                        | NodeValue::Text(_)
                        | NodeValue::SoftBreak
                        | NodeValue::LineBreak
                ) {
                    return Err(Error::InvalidPlan);
                }
            }
            NodeEdge::End(_) => depth -= 1,
        }
    }
    let mut output = Output::new(limits.output_bytes);
    comrak::format_commonmark(root, &options, &mut output).map_err(|_| output.error())?;
    Ok(output.value)
}
