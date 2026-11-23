//! Source-coordinate validation shared by syntax and editor consumers.

use gazette_core::source::Span;

#[test]
fn spans_preserve_exact_bytes_and_reject_invalid_boundaries() {
    let source = "a\té🙂\r\nb\rc\n";
    let start = source.find('é').expect("fixture scalar");
    let end = source.find('\r').expect("fixture newline");
    let span = Span::checked(source, start..end).expect("valid complete scalars");
    assert_eq!(span.slice(source), Some("é🙂"));
    assert_eq!(Span::checked(source, start + 1..end), None);
    assert_eq!(Span::checked(source, start..end - 1), None);
    assert_eq!(Span::checked(source, end..start), None);
    assert_eq!(Span::checked(source, 0..source.len() + 1), None);
    assert_eq!(
        Span::checked(source, source.len()..source.len()).and_then(|s| s.slice(source)),
        Some("")
    );
    assert_eq!(
        Span {
            start: usize::MAX,
            end: usize::MAX
        }
        .slice(source),
        None
    );
}

#[test]
fn framing_composition_checks_both_ranges_without_normalizing_text() {
    let source = "+++\r\ntitle = \"é🙂\"\r\n+++\r\n# Body\n";
    let parent_start = source.find("title").expect("fixture metadata");
    let parent_end = source[parent_start..].find("+++").expect("closing fence") + parent_start;
    let parent = Span::checked(source, parent_start..parent_end).expect("metadata slice");
    let local =
        Span::checked(parent.slice(source).expect("metadata"), 8..16).expect("quoted value");
    let complete = local
        .in_parent(source, parent)
        .expect("complete coordinates");
    assert_eq!(complete.slice(source), Some("\"é🙂\""));
    assert_eq!(complete.start, parent_start + 8);
    assert_eq!(source, "+++\r\ntitle = \"é🙂\"\r\n+++\r\n# Body\n");

    let outside = Span {
        start: 0,
        end: parent_end - parent_start + 1,
    };
    assert_eq!(outside.in_parent(source, parent), None);
    let invalid_parent = Span {
        start: 0,
        end: usize::MAX,
    };
    assert_eq!(local.in_parent(source, invalid_parent), None);
}
