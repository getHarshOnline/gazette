//! Native heading anchors and footnote associations through the shared SDK.

use gazette_sdk::{
    authoring::{self, Analysis, Input, Limits, NavigationError},
    source::{EntryPath, Precision, Sha256Digest},
};

const WORKSPACE: &str = "[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n";

fn analyze(files: &[(&str, &str)], limits: Limits) -> Result<Analysis, authoring::Error> {
    authoring::analyze(
        Input {
            path: "workspace.toml".parse().expect("workspace path"),
            bytes: WORKSPACE.as_bytes(),
        },
        files.iter().map(|(path, text)| Input {
            path: path.parse().expect("selected source path"),
            bytes: text.as_bytes(),
        }),
        limits,
    )
}

fn framed(body: &str) -> String {
    format!(
        "+++\r\n[content]\r\nid='content:one'\r\nsite='site:one'\r\ntitle='One'\r\nlifecycle='draft'\r\n[content.structure]\r\nkind='page'\r\nfamily={{kind='article'}}\r\n+++\r\n{body}"
    )
}

#[test]
fn native_allocated_headings_bind_framed_unicode_source_and_complete_reverse_references() {
    let body = "# Héllo 😀 {#\"é\"}\r\n\r\n😀 [first](#%C3%A9) then [query](?mode=1#é).\r\n\r\n# Repeat\r\n\r\n# Repeat\r\n\r\n[second](#repeat-1)\r\n";
    let page = framed(body);
    let path: EntryPath = "page.md".parse().expect("page path");
    let analysis =
        analyze(&[("page.md", &page)], Limits::default()).expect("current native analysis");
    assert!(analysis.structurally_valid());
    let parsed = gazette_sdk::markdown::parse(body, Default::default()).expect("same native body");
    let headings = parsed.headings().expect("emitted anchor allocation");
    assert_eq!(
        headings
            .iter()
            .map(|heading| heading.id.as_str())
            .collect::<Vec<_>>(),
        ["é", "repeat", "repeat-1"]
    );
    let native_refs = parsed
        .heading_references()
        .expect("complete compiler associations");
    assert_eq!(native_refs.len(), 3);
    let target = analysis
        .heading_navigation(&path, &page, page.find("[first]").expect("link"))
        .expect("native local anchor query")
        .expect("heading target");
    assert_eq!(target.path, &path);
    assert_eq!(target.source_sha256, Sha256Digest::of(page.as_bytes()));
    assert_eq!(target.definition.path, &path);
    assert_eq!(target.definition.source_sha256, target.source_sha256);
    assert!(
        target
            .references
            .iter()
            .all(|value| value.path == &path && value.source_sha256 == target.source_sha256)
    );
    assert_eq!(
        target.occurrence.span.slice(&page),
        Some("[first](#%C3%A9)")
    );
    assert_eq!(
        target.definition.location.span.slice(&page),
        Some("# Héllo 😀 {#\"é\"}")
    );
    assert_eq!(target.definition.location.precision, Precision::Construct);
    assert_eq!(
        target
            .references
            .iter()
            .map(|location| location
                .location
                .span
                .slice(&page)
                .expect("full source range"))
            .collect::<Vec<_>>(),
        ["[first](#%C3%A9)", "[query](?mode=1#é)"]
    );
    assert!(target.references[0].location.span.start < target.references[1].location.span.start);
    let body_start = page.len() - body.len();
    assert_eq!(
        target.definition.location.span.start,
        body_start + headings[0].span.start
    );
    assert_eq!(
        target.references[1].location.span.start,
        body_start + native_refs[1].location.span.start
    );
    let reverse = analysis
        .heading_navigation(&path, &page, target.definition.location.span.start)
        .expect("heading construct query")
        .expect("declaration target");
    assert_eq!(reverse.references, target.references);
    assert_eq!(reverse.definition, target.definition);
    let duplicate = analysis
        .heading_navigation(&path, &page, page.find("[second]").expect("duplicate link"))
        .expect("allocated suffix query")
        .expect("second emitted heading");
    assert_eq!(
        duplicate.definition.location.span.start,
        body_start + headings[2].span.start
    );
    assert_ne!(
        duplicate.definition.location.span.start,
        body_start + headings[1].span.start
    );
    assert_eq!(duplicate.references.len(), 1);
}

#[test]
fn document_local_heading_navigation_does_not_infer_images_paths_or_other_anchor_kinds() {
    let one = "# First {#target}\n\n[local](#target) [external](https://example.invalid/#target) [root](/one/#target) [relative](other.md#target) ![image](#target) [empty](#) [note](#fn-note-6e)\n\nBody[^n].\n\n[^n]: note\n";
    let two = "# Second {#target}\n\n[local](#target)\n";
    let analysis = analyze(&[("one.md", one), ("two.md", two)], Limits::default())
        .expect("selected native sources");
    for (file, text, title) in [
        ("one.md", one, "# First {#target}"),
        ("two.md", two, "# Second {#target}"),
    ] {
        let path: EntryPath = file.parse().expect("selected path");
        let nav = analysis
            .heading_navigation(&path, text, text.find("[local]").expect("local link"))
            .expect("source-local query")
            .expect("same document heading");
        assert_eq!(nav.definition.location.span.slice(text), Some(title));
        assert_eq!(
            nav.references.len(),
            1,
            "other documents and non-local links are not reverse references"
        );
    }
    let path = "one.md".parse().expect("first path");
    let native = gazette_sdk::markdown::parse(one, Default::default()).expect("native source");
    assert!(
        native
            .contains_anchor("fn-note-6e")
            .expect("actual footnote anchor")
    );
    for spelling in [
        "[external]",
        "[root]",
        "[relative]",
        "![image]",
        "[empty]",
        "[note]",
    ] {
        assert!(
            analysis
                .heading_navigation(&path, one, one.find(spelling).expect("excluded construct"))
                .expect("valid nonheading occurrence")
                .is_none(),
            "must not infer target for {spelling}"
        );
    }
}

#[test]
fn malformed_or_unknown_local_fragment_blocks_complete_heading_refs_but_not_valid_footnotes() {
    for fragment in ["#absent", "#bad%ZZ", "#%ff", "#%00"] {
        let page = format!(
            "# Target {{#target}}\n\n[valid](#target) [bad]({fragment}) note[^n].\n\n[^n]: retained note\n"
        );
        let analysis =
            analyze(&[("page.md", &page)], Limits::default()).expect("retained current analysis");
        let path = "page.md".parse().expect("path");
        for offset in [0, page.find("[valid]").expect("otherwise valid link")] {
            assert_eq!(
                analysis
                    .heading_navigation(&path, &page, offset)
                    .expect_err("one bad local fragment prevents a successful partial reverse set"),
                NavigationError::Unavailable
            );
        }
        let note = analysis
            .source_navigation(
                &path,
                &page,
                page.find("[^n]").expect("valid note reference"),
            )
            .expect("heading failure does not erase independent footnote evidence")
            .expect("note target");
        assert_eq!(note.occurrence.span.slice(&page), Some("[^n]"));
        assert!(
            note.definition
                .location
                .span
                .slice(&page)
                .expect("note definition")
                .contains("retained note")
        );
    }
}

#[test]
fn wrong_preimage_byte_positions_current_corpus_and_anchor_allocator_fail_closed() {
    let page = "😀\n\n# Target {#target}\n\n[go](#target)\n";
    let path: EntryPath = "page.md".parse().expect("path");
    let valid = analyze(&[("page.md", page)], Limits::default()).expect("valid source");
    for (selected, text, offset) in [
        (path.clone(), page.to_owned(), 1),
        (path.clone(), page.to_owned(), page.len() + 1),
        (path.clone(), format!("{page}\n"), 0),
        (
            "absent.md".parse().expect("unknown source"),
            page.to_owned(),
            0,
        ),
    ] {
        assert_eq!(
            valid
                .heading_navigation(&selected, &text, offset)
                .expect_err("exact source binding required"),
            NavigationError::InvalidSource
        );
        assert_eq!(
            valid
                .source_navigation(&selected, &text, offset)
                .expect_err("combined route retains same binding gate"),
            NavigationError::InvalidSource
        );
    }
    assert!(
        valid
            .heading_navigation(&path, page, page.len())
            .expect("valid EOF")
            .is_none()
    );
    let broken = analyze(
        &[("page.md", page), ("broken.md", "+++\n[broken\n+++\n")],
        Limits::default(),
    )
    .expect("failed corpus evidence");
    assert_eq!(
        broken
            .heading_navigation(
                &path,
                page,
                page.find("[go]").expect("old valid occurrence")
            )
            .expect_err("current corpus failure cannot return old navigation"),
        NavigationError::Unavailable
    );
    let invalid =
        "# One {#same}\n# Two {#same}\n\n[go](#same) note[^n].\n\n[^n]: still source evidence\n";
    let failed = analyze(&[("page.md", invalid)], Limits::default())
        .expect("allocator failure stays phase-native");
    assert_eq!(
        failed
            .heading_navigation(&path, invalid, invalid.find("[go]").expect("link"))
            .expect_err("duplicate explicit anchors never choose first target"),
        NavigationError::Unavailable
    );
    assert!(
        failed
            .source_navigation(&path, invalid, invalid.find("[^n]").expect("note"))
            .expect("independent note survives heading allocation failure")
            .is_some()
    );
}

#[test]
fn combined_navigation_chooses_the_narrower_native_construct_in_both_nesting_directions() {
    let page = "# Target {#target}\n\n# Heading[^n]\n\n[^n]: Go [jump](#target).\n";
    let path = "page.md".parse().expect("path");
    let analysis = analyze(&[("page.md", page)], Limits::default()).expect("native nested source");
    let offset = page.find("[^n]").expect("note inside heading");
    let heading = analysis
        .heading_navigation(&path, page, offset)
        .expect("outer heading")
        .expect("heading construct");
    let note = analysis
        .footnote_navigation(&path, page, offset)
        .expect("inner note")
        .expect("note construct");
    assert!(
        note.occurrence.span.end - note.occurrence.span.start
            < heading.occurrence.span.end - heading.occurrence.span.start
    );
    assert_eq!(
        analysis
            .source_navigation(&path, page, offset)
            .expect("combined query"),
        Some(note)
    );
    let offset = page
        .find("[jump]")
        .expect("heading link inside note definition");
    let note = analysis
        .footnote_navigation(&path, page, offset)
        .expect("outer note definition")
        .expect("containing definition");
    let heading = analysis
        .heading_navigation(&path, page, offset)
        .expect("inner heading link")
        .expect("heading target");
    assert!(
        heading.occurrence.span.end - heading.occurrence.span.start
            < note.occurrence.span.end - note.occurrence.span.start
    );
    assert_eq!(heading.occurrence.span.slice(page), Some("[jump](#target)"));
    assert_eq!(
        analysis
            .source_navigation(&path, page, offset)
            .expect("combined query"),
        Some(heading)
    );
}

#[test]
fn complete_heading_references_share_occurrence_budget_without_duplicate_heading_charges() {
    let page = "# Target {#target}\n\n[one](#target) [two](?q=1#target) note[^n].\n\n[^n]: note\n";
    let workspace = gazette_sdk::workspace::ParsedWorkspace::parse(WORKSPACE, Default::default())
        .expect("native workspace navigation baseline");
    let workspace_path = "workspace.toml".parse().expect("workspace path");
    let targets: Vec<_> = workspace
        .navigation()
        .symbols()
        .iter()
        .filter_map(|symbol| {
            gazette_sdk::authored::AuthoredReferenceDeclaration::workspace(
                symbol,
                &workspace_path,
                WORKSPACE,
            )
            .expect("source-bound native completion declaration")
        })
        .collect();
    let occurrences = workspace.navigation().occurrence_count() + targets.len();
    let bytes = workspace.navigation().key_bytes()
        + targets
            .iter()
            .map(|target| target.fact_bytes())
            .sum::<usize>();
    let native = gazette_sdk::markdown::parse(page, Default::default()).expect("native page facts");
    let page_occurrences = native.footnote_definitions().len()
        + native.footnote_references().len()
        + native
            .heading_references()
            .expect("heading references")
            .len()
        + native
            .heading_completion_slots()
            .expect("heading slots")
            .len()
        + native
            .symbolic_completion_slots()
            .expect("symbolic slots")
            .len();
    let page_bytes = native
        .footnote_definitions()
        .iter()
        .map(|note| note.name.len())
        .chain(
            native
                .footnote_references()
                .iter()
                .map(|note| note.name.len() + note.definition.as_deref().map_or(0, str::len)),
        )
        .sum::<usize>();
    let exact = Limits {
        headings: 1,
        navigation_occurrences: occurrences + page_occurrences,
        navigation_bytes: bytes + page_bytes,
        ..Default::default()
    };
    let analysis = analyze(&[("page.md", page)], exact)
        .expect("one heading, two heading links, one exact completion slot and two note records");
    let path = "page.md".parse().expect("path");
    assert_eq!(
        analysis
            .heading_navigation(&path, page, 0)
            .expect("heading query")
            .expect("heading")
            .references
            .len(),
        2
    );
    for limits in [
        Limits {
            navigation_occurrences: exact.navigation_occurrences - 1,
            ..exact
        },
        Limits {
            navigation_bytes: exact.navigation_bytes - 1,
            ..exact
        },
        Limits {
            headings: 0,
            ..exact
        },
    ] {
        assert!(
            matches!(
                analyze(&[("page.md", page)], limits),
                Err(authoring::Error::Limit(_))
            ),
            "no partial aggregate projection"
        );
    }
    assert!(
        matches!(
            analyze(
                &[("one.md", page), ("two.md", page)],
                Limits {
                    headings: 2,
                    navigation_occurrences: occurrences + page_occurrences * 2 - 1,
                    navigation_bytes: bytes + page_bytes * 2,
                    ..exact
                }
            ),
            Err(authoring::Error::Limit(_))
        ),
        "complete selected-set occurrences, not a per-file reset"
    );
}
