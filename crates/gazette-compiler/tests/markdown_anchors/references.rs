//! Heading source navigation consumes the same native links and emitted anchor index.

use super::*;
use gazette_compiler::markdown::HeadingReference;

#[test]
fn unicode_and_percent_fragments_bind_exact_existing_heading_indices() {
    let source = "# Unicode {#\"café\"}\r\n# Percent {#a%b}\r\n\r\n[escaped](#caf%C3%A9) [raw](#café) [query](?view=x#caf%c3%a9) [percent](#a%25b)\r\n";
    let parsed = parse(source);
    let headings = parsed.headings().expect("single emitted heading index");
    let references = parsed
        .heading_references()
        .expect("native source associations");
    assert_eq!(
        references.iter().map(|r| r.heading).collect::<Vec<_>>(),
        [0, 0, 0, 1]
    );
    assert_eq!(headings[0].id, "café");
    assert_eq!(headings[1].id, "a%b");
    for reference in &references {
        assert_eq!(reference.location.precision, Precision::Construct);
        assert!(
            reference
                .location
                .span
                .slice(source)
                .expect("checked native link construct")
                .starts_with('[')
        );
        assert!(
            parsed
                .contains_anchor(&headings[reference.heading].id)
                .expect("same public anchor")
        );
    }
    assert_eq!(
        parsed
            .heading_references()
            .expect("repeat source associations"),
        references
    );
    assert!(std::ptr::eq(
        headings,
        parsed.headings().expect("same retained allocation")
    ));
}

#[test]
fn automatic_slug_collisions_follow_global_explicit_id_reservation() {
    let parsed = parse(
        "# Hello\n# Hello\n# Manual {#hello}\n\n[one](#hello-1) [two](#hello-2) [manual](#hello)\n",
    );
    let headings = parsed.headings().expect("reserved explicit IDs");
    assert_eq!(
        headings.iter().map(|h| h.id.as_str()).collect::<Vec<_>>(),
        ["hello-1", "hello-2", "hello"]
    );
    assert_eq!(
        parsed
            .heading_references()
            .expect("shared allocated indices")
            .iter()
            .map(|r| r.heading)
            .collect::<Vec<_>>(),
        [0, 1, 2]
    );
    for id in ["hello-1", "hello-2", "hello"] {
        assert!(
            parsed
                .render_html()
                .expect("actual emitted IDs")
                .contains(&format!("id=\"{id}\""))
        );
    }
}

#[test]
fn every_invalid_selected_fragment_refuses_the_complete_reference_result() {
    for fragment in ["missing", "%", "%2", "%GG", "%FF", "%00", "a%2525b"] {
        let source =
            format!("# One {{#one}}\n# Percent {{#a%b}}\n\n[good](#one) [bad](#{fragment})\n");
        let parsed = parse(&source);
        let error = parsed
            .heading_references()
            .expect_err("malformed or unknown fragment cannot yield partial associations");
        let Error::LinkResolution { span, .. } = error else {
            panic!("source-bound link failure required")
        };
        assert!(
            span.slice(&source)
                .expect("bad native link range")
                .starts_with("[bad]")
        );
    }
    let parsed = parse("# One {#same}\n# Two {#same}\n\n[read](#same)\n");
    assert_eq!(
        parsed
            .heading_references()
            .expect_err("ambiguous anchors have no navigation subset"),
        parsed.headings().expect_err("same anchor owner failure")
    );
}

#[test]
fn noneligible_links_and_valid_nonheading_anchors_do_not_invent_heading_targets() {
    let source = "# One\n\nBody[^note].\n\n[empty](#) [query](?view=x#) [external](https://example.invalid/#missing) [root](/page#missing) [relative](other.md#missing) [dot](./#missing) ![image](#missing) [note](#fn-note-6e6f7465) [back](#fnref-note-6e6f7465) [heading](#one)\n\n[^note]: Footnote.\n";
    let parsed = parse(source);
    assert!(
        parsed
            .contains_anchor("fn-note-6e6f7465")
            .expect("actual note ID")
    );
    assert!(
        parsed
            .contains_anchor("fnref-note-6e6f7465")
            .expect("actual reference ID")
    );
    let references = parsed
        .heading_references()
        .expect("only heading associations");
    assert_eq!(references.len(), 1);
    assert_eq!(references[0].heading, 0);
    assert!(
        references[0]
            .location
            .span
            .slice(source)
            .expect("heading link range")
            .starts_with("[heading]")
    );
}

#[test]
fn unused_definition_links_remain_recognized_source_references() {
    let source = "# One\n\n[visible](#one)\n\n[^unused]: [hidden](#one)\n";
    let parsed = parse(source);
    let inspection = parsed.inspect().expect("native whole-source evidence");
    assert!(
        inspection
            .links
            .iter()
            .any(|link| !link.emitted && link.destination == "#one")
    );
    let references = parsed
        .heading_references()
        .expect("emitted and non-emitted native links");
    assert_eq!(references.len(), 2);
    assert!(references.iter().all(|reference| reference.heading == 0));
    assert!(references.iter().any(|reference| {
        reference
            .location
            .span
            .slice(source)
            .expect("unused source construct")
            .starts_with("[hidden]")
    }));
    assert!(
        !parsed
            .render_html()
            .expect("unused definition omitted from output")
            .contains("hidden")
    );
}

#[test]
fn returned_record_bytes_are_charged_before_each_push_without_partial_results() {
    let source = "# One\n\n[a](#one) [b](#one)\n";
    // One borrowed lookup entry plus two returned association records.
    let exact = std::mem::size_of::<(&str, usize)>() + std::mem::size_of::<HeadingReference>() * 2;
    let parsed = ParsedMarkdown::parse(
        source,
        Limits {
            output_bytes: exact,
            ..Limits::default()
        },
        &gazette_adapter_markdown::Comrak,
    )
    .expect("native parse is independently bounded");
    assert_eq!(parsed.headings().expect("anchor allocation fits").len(), 1);
    assert_eq!(
        parsed
            .heading_references()
            .expect("exact returned-record ceiling")
            .len(),
        2
    );
    let parsed = ParsedMarkdown::parse(
        source,
        Limits {
            output_bytes: exact - 1,
            ..Limits::default()
        },
        &gazette_adapter_markdown::Comrak,
    )
    .expect("native syntax before reference projection");
    assert_eq!(
        parsed
            .headings()
            .expect("same anchor allocation still fits")
            .len(),
        1
    );
    assert_eq!(
        parsed.heading_references(),
        Err(Error::Limit("heading reference bytes"))
    );
}
