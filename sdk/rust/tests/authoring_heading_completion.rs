//! Heading edit proposals use the same allocated anchors as native publication output.

use gazette_sdk::{
    authoring::{
        self, Analysis, Input, NavigationError,
        session::{Session, Source},
    },
    source::{EntryPath, Precision, Sha256Digest},
};

const WORKSPACE: &str = "[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n";

fn analyze(text: &str) -> Analysis {
    authoring::analyze(
        Input {
            path: "workspace.toml".parse().expect("workspace path"),
            bytes: WORKSPACE.as_bytes(),
        },
        [Input {
            path: "page.md".parse().expect("page path"),
            bytes: text.as_bytes(),
        }],
        Default::default(),
    )
    .expect("retained native analysis")
}

fn framed(body: &str) -> String {
    format!(
        "+++\r\n[content]\r\nid='content:one'\r\nsite='site:one'\r\ntitle='One'\r\nlifecycle='draft'\r\nstructure={{kind='page',family={{kind='article'}}}}\r\n+++\r\n{body}"
    )
}

#[test]
fn unresolved_fragment_edits_render_exact_explicit_and_duplicate_heading_anchors() {
    let body =
        "# Héllo 😀 {#\"é\"}\r\n\r\n# Repeat\r\n\r\n# Repeat\r\n\r\n😀 [repair](#missing)\r\n";
    let page = framed(body);
    let path: EntryPath = "page.md".parse().expect("path");
    let analysis = analyze(&page);
    let offset = page.find("#missing").expect("destination");
    assert_eq!(
        analysis
            .heading_navigation(&path, &page, offset)
            .expect_err("strict navigation cannot resolve the original target"),
        NavigationError::Unavailable
    );
    let proposed = analysis
        .complete_heading(&path, &page, offset + 1, 1 << 18)
        .expect("current unresolved destination")
        .expect("native inline destination");
    assert_eq!(proposed.path, &path);
    assert_eq!(proposed.source_sha256, Sha256Digest::of(page.as_bytes()));
    assert_eq!(proposed.location.precision, Precision::Exact);
    assert_eq!(proposed.location.span.start, offset);
    assert_eq!(proposed.location.span.end, offset + "#missing".len());
    assert_eq!(proposed.location.span.slice(&page), Some("#missing"));
    assert_eq!(proposed.preimage, "#missing");
    assert_eq!(
        proposed
            .candidates
            .iter()
            .map(|heading| heading.fragment.as_str())
            .collect::<Vec<_>>(),
        ["#%C3%A9", "#repeat", "#repeat-1"]
    );
    for heading in proposed.candidates {
        let mut changed = page.clone();
        changed.replace_range(
            proposed.location.span.start..proposed.location.span.end,
            &heading.fragment,
        );
        let parsed = gazette_sdk::document::parse(&changed, Default::default())
            .expect("edited document parses through the real adapter");
        let html = parsed.render_html().expect("actual edited HTML");
        assert!(html.contains(&format!("href=\"{}\"", heading.fragment)));
        assert!(html.contains(&format!("id=\"{}\"", heading.id)));
        let repaired = analyze(&changed);
        let resolved = repaired
            .heading_navigation(&path, &changed, offset)
            .expect("edited native references now resolve")
            .expect("same-document heading");
        assert_eq!(
            resolved.definition.location.span.start,
            page.len() - body.len() + heading.span.start
        );
    }
}

#[test]
fn an_empty_literal_fragment_is_a_repair_slot_without_requiring_existing_navigation() {
    let page = "# Target\n\n[choose](#)\n";
    let analysis = analyze(page);
    let path = "page.md".parse().expect("path");
    let result = analysis
        .complete_heading(&path, page, page.find("](#").expect("slot") + 2, 1 << 16)
        .expect("current parsed slot")
        .expect("empty fragment can choose a heading");
    assert_eq!(result.preimage, "#");
    assert_eq!(result.candidates.len(), 1);
    assert_eq!(result.candidates[0].fragment, "#target");
}

#[test]
fn unsupported_source_forms_never_receive_guessed_destination_edits() {
    for source in [
        "[title](#target \"Title\")",
        "[angles](<#target>)",
        "[external](https://example.invalid/#target)",
        "[path](other.md#target)",
        "[query](?mode=1#target)",
        "![image](#target)",
        "`[code](#target)`",
        "```text\n[fenced](#target)\n```",
        "[reference][ref]\n\n[ref]: #target",
        "[escaped](#tar\\get)",
        "[entity](#tar&#103;et)",
        "[unclosed](#target",
    ] {
        let page = format!("# Target\n\n{source}\n");
        let analysis = analyze(&page);
        let path = "page.md".parse().expect("path");
        let offset = page.rfind('#').expect("source destination spelling");
        assert!(
            analysis
                .complete_heading(&path, &page, offset, 1 << 18)
                .expect("supported current source inspection")
                .is_none(),
            "unsupported spelling must not acquire an edit: {source}"
        );
    }
}

#[test]
fn source_identity_allocator_and_complete_result_bounds_fail_closed() {
    let page = "😀\n\n# Target\n\n[repair](#missing)\n";
    let analysis = analyze(page);
    let path: EntryPath = "page.md".parse().expect("path");
    let offset = page.find("#missing").expect("destination");
    let result = analysis
        .complete_heading(&path, page, offset, 1 << 18)
        .expect("query")
        .expect("repair slot");
    let bytes = serde_json::to_vec(&result)
        .expect("whole native result")
        .len();
    assert!(
        analysis
            .complete_heading(&path, page, offset, bytes)
            .expect("exact result ceiling")
            .is_some()
    );
    assert!(matches!(
        analysis.complete_heading(&path, page, offset, bytes - 1),
        Err(NavigationError::Limit)
    ));
    for (selected, text, position) in [
        (path.clone(), page.to_owned(), 1),
        (path.clone(), page.to_owned(), page.len() + 1),
        (path.clone(), format!("{page}\n"), offset),
        (
            "absent.md".parse().expect("unknown path"),
            page.to_owned(),
            offset,
        ),
    ] {
        assert!(matches!(
            analysis.complete_heading(&selected, &text, position, 1 << 18),
            Err(NavigationError::InvalidSource)
        ));
    }
    let duplicate = "# One {#same}\n\n# Two {#same}\n\n[repair](#missing)\n";
    assert!(matches!(
        analyze(duplicate).complete_heading(
            &path,
            duplicate,
            duplicate.find("#missing").expect("destination"),
            1 << 18,
        ),
        Err(NavigationError::Unavailable)
    ));
}

#[test]
fn accepted_overlays_replace_candidates_and_close_restores_only_the_selected_baseline() {
    let baseline = "# Baseline\n\n[repair](#missing)\n";
    let overlay = "# Changed\n\n[repair](#missing)\n";
    let source = |id: &str, path: &str, text: &str| Source {
        id: id.into(),
        path: path.parse().expect("selected path"),
        text: text.into(),
    };
    let mut state = Session::new(
        source("workspace", "workspace.toml", WORKSPACE),
        [source("page", "page.md", baseline)],
        Default::default(),
    )
    .expect("native selected session");
    let current = |state: &Session| {
        let document = &state.files()["page"];
        let result = state
            .analysis()
            .as_ref()
            .expect("current analysis")
            .complete_heading(
                document.path(),
                document.text(),
                document.text().find("#missing").expect("destination"),
                1 << 18,
            )
            .expect("current query")
            .expect("repair slot");
        (result.source_sha256, result.candidates[0].fragment.clone())
    };
    let initial = current(&state);
    assert_eq!(initial.1, "#baseline");
    assert!(state.open("page", 4, overlay.into()).expect("overlay"));
    assert_eq!(state.generation(), 2);
    assert_eq!(current(&state).1, "#changed");
    assert_ne!(current(&state).0, initial.0);
    assert!(matches!(
        state
            .analysis()
            .as_ref()
            .expect("current analysis")
            .complete_heading(
                state.files()["page"].path(),
                baseline,
                baseline.find("#missing").expect("old destination"),
                1 << 18,
            ),
        Err(NavigationError::InvalidSource)
    ));
    assert!(
        !state
            .change("page", 4, baseline.into())
            .expect("stale version no-op")
    );
    assert_eq!(current(&state).1, "#changed");
    assert!(state.close("page").expect("restore retained baseline"));
    assert_eq!(state.generation(), 3);
    assert_eq!(state.files()["page"].version(), None);
    assert_eq!(current(&state), initial);
}
