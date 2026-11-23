//! The five standalone GitHub markers, qualified against their original bytes.

use crate::source::Lines;
use comrak::nodes::{AlertType, Node, NodeAlert, NodeValue};
use gazette_core::{markdown::Error, source::Location};

fn kind(node: Node<'_>, text: &str) -> Option<AlertType> {
    let kind = match text {
        "[!NOTE]" => AlertType::Note,
        "[!TIP]" => AlertType::Tip,
        "[!IMPORTANT]" => AlertType::Important,
        "[!WARNING]" => AlertType::Warning,
        "[!CAUTION]" => AlertType::Caution,
        _ => return None,
    };
    let paragraph = node.parent()?;
    let quote = paragraph.parent()?;
    (matches!(paragraph.data().value, NodeValue::Paragraph)
        && paragraph
            .first_child()
            .is_some_and(|first| std::ptr::eq(first, node))
        && matches!(quote.data().value, NodeValue::BlockQuote)
        && quote
            .first_child()
            .is_some_and(|first| std::ptr::eq(first, paragraph))
        && node.next_sibling().is_none_or(|next| {
            matches!(
                next.data().value,
                NodeValue::SoftBreak | NodeValue::LineBreak
            )
        }))
    .then_some(kind)
}

pub(crate) fn label(node: Node<'_>, text: &str) -> Option<&'static str> {
    kind(node, text).map(|kind| kind.default_title())
}

fn source_kind(
    node: Node<'_>,
    text: &str,
    location: Location,
    lines: &Lines<'_>,
) -> Option<AlertType> {
    kind(node, text).filter(|_| {
        lines.slice(location.span.start..location.span.end).ok() == Some(text)
            && node.parent().is_some_and(|paragraph| {
                lines
                    .location(paragraph.data().sourcepos)
                    .is_ok_and(|parent| parent.span.start == location.span.start)
            })
    })
}

pub(crate) fn source_label(
    node: Node<'_>,
    text: &str,
    location: Location,
    lines: &Lines<'_>,
) -> Option<&'static str> {
    source_kind(node, text, location, lines).map(|kind| kind.default_title())
}

/// Remove only qualified marker text and its following line break. The quote
/// and every retained child keep their original parser locations.
pub(crate) fn take_markers<'a>(
    root: Node<'a>,
    lines: &Lines<'_>,
) -> Result<Vec<(Node<'a>, AlertType)>, Error> {
    let markers: Vec<_> = root
        .descendants()
        .filter_map(|node| {
            let data = node.data();
            let NodeValue::Text(text) = &data.value else {
                return None;
            };
            let location = lines.location(data.sourcepos).ok()?;
            Some((node, source_kind(node, text, location, lines)?))
        })
        .collect();
    markers
        .into_iter()
        .map(|(marker, kind)| {
            let paragraph = marker.parent().ok_or(Error::InvalidSource)?;
            let quote = paragraph.parent().ok_or(Error::InvalidSource)?;
            if let Some(next) = marker.next_sibling() {
                next.detach();
            }
            marker.detach();
            if paragraph.first_child().is_none() {
                paragraph.detach();
            }
            Ok((quote, kind))
        })
        .collect()
}

/// Use Comrak's native renderer after Gazette's stricter source recognition.
/// Enabling the parser extension directly would also consume same-line prose
/// as a custom title, which Gazette preserves as an ordinary quotation.
pub(crate) fn normalize(root: Node<'_>, lines: &Lines<'_>) -> Result<(), Error> {
    for (quote, alert_type) in take_markers(root, lines)? {
        quote.data_mut().value = NodeValue::Alert(Box::new(NodeAlert {
            alert_type,
            title: None,
            multiline: false,
            fence_length: 1,
            fence_offset: 0,
        }));
    }
    Ok(())
}
