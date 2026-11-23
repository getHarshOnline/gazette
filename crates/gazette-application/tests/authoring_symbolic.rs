//! Authoring consumes compiler-proved publication references over exact selected bytes.

use gazette_application::authoring::{
    self, Analysis, Error, Failure, Input, Limits, NavigationError, SelectedCorpus,
};
use gazette_compiler::markdown;
use gazette_core::source::{EntryPath, Precision, Sha256Digest};

const WORKSPACE: &str = r#"
[[principals]]
id='owner:one'
kind='person'
[[sites]]
id='site:one'
publisher='owner:one'
deployable=false
[[sites]]
id='site:two'
publisher='owner:one'
deployable=false
[[sources]]
id='source:one'
location={kind='filesystem',root='/not-opened/one'}
[[sources]]
id='source:two'
location={kind='filesystem',root='/not-opened/two'}
[[brands]]
id='brand:one'
name='Selected publication fixture'
[[brands.publications]]
site='site:one'
public_base_url='https://one.test/source-prefix/'
authored=[{source='source:one',target='content/one'}]
[[brands.publications]]
site='site:two'
public_base_url='https://two.test/publication-prefix/'
authored=[{source='source:two',target='content/two'}]
"#;

const SOURCE_PATH: &str = "content/one/source/index.md";
const SOURCE_NOTES: &str = "content/one/source/notes.md";
const TARGET_PATH: &str = "content/two/target/index.md";
const TARGET_NOTES: &str = "content/two/target/notes.md";

fn path(value: &str) -> EntryPath {
    value.parse().unwrap()
}

fn input<'a>(path: &str, text: &'a str) -> Input<'a> {
    Input {
        path: self::path(path),
        bytes: text.as_bytes(),
    }
}

fn page(id: &str, site: &str, route: &str, body: &str) -> String {
    format!(
        "+++\r\n[content]\r\nid='{id}'\r\nsite='{site}'\r\ntitle='Guide β 😀'\r\nlifecycle='published'\r\nroute={{path='{route}'}}\r\nstructure={{kind='page',family={{kind='article'}}}}\r\n+++\r\n{body}"
    )
}

fn source(body: &str) -> String {
    page("content:source", "site:one", "/source/", body)
}

fn target() -> String {
    page(
        "content:target",
        "site:two",
        "/target/",
        "# Target 😀\r\n\r\n## Détail β {#detail}\r\n\r\n[self](#detail)\r\n",
    )
}

fn analyze(
    source: &str,
    target: &str,
    notes: &[(&str, &str)],
    limits: Limits,
    reverse: bool,
) -> Result<Analysis, Error> {
    let mut documents = vec![input(SOURCE_PATH, source), input(TARGET_PATH, target)];
    let mut page_inputs: Vec<_> = notes.iter().map(|(path, text)| input(path, text)).collect();
    if reverse {
        documents.reverse();
        page_inputs.reverse();
    }
    authoring::analyze_with_manuals_and_package_baselines(
        input("workspace.toml", WORKSPACE),
        SelectedCorpus {
            asset_metadata: Default::default(),
            documents,
            manuals: Vec::new(),
            page_inputs,
        },
        [],
        limits,
        &gazette_adapter_markdown::Comrak,
    )
}

#[test]
fn symbolic_navigation_retains_exact_unicode_crlf_preimages_and_complete_shared_backlinks() {
    let link = "[[site:two.test/target/#detail|Détail β]]";
    let source = source(&format!(
        "# Source 😀\r\n\r\nPréface β {link}.\r\n\r\n[ordinary](../../two/target/index.md#detail)\r\n"
    ));
    let target = target();
    let notes = "# Notes 😀\r\n\r\n[[site:two.test/target/#detail|Notes crossing]]\r\n";
    let analysis = analyze(
        &source,
        &target,
        &[(SOURCE_NOTES, notes)],
        Limits::default(),
        false,
    )
    .unwrap();
    assert!(analysis.structurally_valid(), "{:?}", analysis.failures());
    let navigation = analysis
        .source_navigation(&path(SOURCE_PATH), &source, source.find(link).unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(
        navigation.source_sha256,
        Sha256Digest::of(source.as_bytes())
    );
    assert_eq!(navigation.occurrence.precision, Precision::Construct);
    assert_eq!(navigation.occurrence.span.slice(&source), Some(link));
    assert_eq!(navigation.definition.path, &path(TARGET_PATH));
    assert_eq!(
        navigation.definition.source_sha256,
        Sha256Digest::of(target.as_bytes())
    );
    assert_eq!(
        navigation.definition.location.span.slice(&target),
        Some("## Détail β {#detail}")
    );
    assert_eq!(navigation.references.len(), 4);
    let references: Vec<_> = navigation
        .references
        .iter()
        .map(|reference| {
            let text = match reference.path.as_str() {
                SOURCE_PATH => source.as_str(),
                SOURCE_NOTES => notes,
                TARGET_PATH => target.as_str(),
                other => panic!("unexpected selected source {other}"),
            };
            assert_eq!(reference.source_sha256, Sha256Digest::of(text.as_bytes()));
            assert_eq!(reference.location.precision, Precision::Construct);
            (
                reference.path.as_str(),
                reference.location.span.slice(text).unwrap(),
            )
        })
        .collect();
    assert_eq!(
        references,
        [
            (SOURCE_PATH, link),
            (SOURCE_PATH, "[ordinary](../../two/target/index.md#detail)"),
            (
                SOURCE_NOTES,
                "[[site:two.test/target/#detail|Notes crossing]]"
            ),
            (TARGET_PATH, "[self](#detail)"),
        ]
    );
    let reversed = analyze(
        &source,
        &target,
        &[(SOURCE_NOTES, notes)],
        Limits::default(),
        true,
    )
    .unwrap();
    assert_eq!(
        reversed
            .source_navigation(&path(SOURCE_PATH), &source, source.find(link).unwrap())
            .unwrap()
            .unwrap(),
        navigation
    );
    assert_eq!(
        analysis.source_navigation(
            &path(SOURCE_PATH),
            &format!("{source}changed"),
            source.find(link).unwrap()
        ),
        Err(NavigationError::InvalidSource)
    );
}

#[test]
fn symbolic_notes_targets_keep_the_actual_companion_definition_and_reverse_navigation() {
    let link = "[[site:two.test/target/#gazette-notes-detail|Commentary]]";
    let source = source(&format!("# Source\r\n\r\n{link}\r\n"));
    let target = target();
    let notes = "# Notes 😀\r\n\r\n## Notes Détail β {#detail}\r\n\r\n[self](#detail)\r\n";
    let analysis = analyze(
        &source,
        &target,
        &[(TARGET_NOTES, notes)],
        Limits::default(),
        false,
    )
    .unwrap();
    assert!(analysis.structurally_valid(), "{:?}", analysis.failures());
    let navigation = analysis
        .source_navigation(&path(SOURCE_PATH), &source, source.find(link).unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(navigation.definition.path, &path(TARGET_NOTES));
    assert_eq!(
        navigation.definition.source_sha256,
        Sha256Digest::of(notes.as_bytes())
    );
    assert_eq!(
        navigation.definition.location.span.slice(notes),
        Some("## Notes Détail β {#detail}")
    );
    assert_eq!(navigation.references.len(), 2);
    let reverse = analysis
        .source_navigation(&path(TARGET_NOTES), notes, notes.find("## Notes").unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(reverse.definition, navigation.definition);
    assert_eq!(reverse.references, navigation.references);
    assert!(
        reverse
            .references
            .iter()
            .any(|reference| reference.path == &path(SOURCE_PATH)
                && reference.location.span.slice(&source) == Some(link))
    );
}

#[test]
fn symbolic_route_without_fragment_navigates_to_native_declaration_not_a_guessed_body_heading() {
    let link = "[[site:two.test/target/|Target]]";
    let source = source(&format!("# Source\r\n\r\n{link}\r\n"));
    let target = target();
    let analysis = analyze(&source, &target, &[], Limits::default(), false).unwrap();
    assert!(analysis.structurally_valid(), "{:?}", analysis.failures());
    let navigation = analysis
        .source_navigation(&path(SOURCE_PATH), &source, source.find(link).unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(navigation.definition.path, &path(TARGET_PATH));
    assert_eq!(
        navigation.definition.location.precision,
        Precision::Construct
    );
    assert_eq!(
        navigation.definition.location.span.slice(&target),
        target.split("+++\r\n").nth(1)
    );
    assert_eq!(
        navigation.definition.source_sha256,
        Sha256Digest::of(target.as_bytes())
    );
    assert!(
        !navigation
            .definition
            .location
            .span
            .slice(&target)
            .unwrap()
            .contains("# Target")
    );
}

fn assert_failure(analysis: &Analysis, failed_path: &str, failed_source: &str, link: &str) {
    let [
        Failure::Renderability {
            path: failed,
            body_span,
            error: markdown::Error::LinkResolution { span, .. },
        },
    ] = analysis.failures()
    else {
        panic!(
            "expected exact shared Markdown failure, got {:?}",
            analysis.failures()
        );
    };
    assert_eq!(failed, &path(failed_path));
    assert_eq!(
        analysis.inputs()[failed],
        Sha256Digest::of(failed_source.as_bytes())
    );
    let full = span.in_parent(failed_source, *body_span).unwrap();
    assert_eq!(full.slice(failed_source), Some(link));
    assert!(!analysis.structurally_valid());
    assert_eq!(
        analysis.source_navigation(failed, failed_source, failed_source.find(link).unwrap()),
        Err(NavigationError::Unavailable)
    );
}

#[test]
fn missing_symbolic_routes_fragments_and_omitted_footnote_references_fail_at_exact_main_origin() {
    let target = target();
    for (link, unused) in [
        ("[[site:two.test/missing/|Missing route]]", false),
        ("[[site:two.test/target/#missing|Missing fragment]]", false),
        ("[[site:two.test/missing/|Unused missing route]]", true),
    ] {
        let body = if unused {
            format!("# Source 😀\r\n\r\n[^unused]: {link}\r\n")
        } else {
            format!("# Source 😀\r\n\r\nPréface β {link}\r\n")
        };
        let source = source(&body);
        let analysis = analyze(&source, &target, &[], Limits::default(), false).unwrap();
        assert_failure(&analysis, SOURCE_PATH, &source, link);
        if unused {
            assert!(
                analysis
                    .source_navigation(
                        &path(SOURCE_PATH),
                        &source,
                        source.find("[^unused]").unwrap(),
                    )
                    .unwrap()
                    .is_some(),
                "unresolved nested link does not erase the independent footnote label"
            );
        }
        assert!(
            analysis
                .heading_symbols(&path(TARGET_PATH), &target)
                .unwrap()
                .is_some(),
            "reference failure does not erase independent heading evidence"
        );
    }
}

#[test]
fn missing_symbolic_notes_fragment_uses_exact_notes_origin_and_restoration_recovers() {
    let source = source("# Source\r\n");
    let target = target();
    let link = "[[site:two.test/target/#missing|Broken notes crossing]]";
    let notes = format!("# Notes 😀\r\n\r\nPréface β {link}\r\n");
    let analysis = analyze(
        &source,
        &target,
        &[(SOURCE_NOTES, &notes)],
        Limits::default(),
        false,
    )
    .unwrap();
    assert_failure(&analysis, SOURCE_NOTES, &notes, link);
    let [Failure::Renderability { body_span, .. }] = analysis.failures() else {
        unreachable!()
    };
    assert_eq!(
        body_span.start, 0,
        "plain commentary has no frontmatter offset"
    );
    let restored = notes.replace("#missing", "#detail");
    let recovered = analyze(
        &source,
        &target,
        &[(SOURCE_NOTES, &restored)],
        Limits::default(),
        false,
    )
    .unwrap();
    assert!(recovered.structurally_valid(), "{:?}", recovered.failures());
    let navigation = recovered
        .source_navigation(
            &path(SOURCE_NOTES),
            &restored,
            restored.find("[[site:").unwrap(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(navigation.definition.path, &path(TARGET_PATH));
    assert_eq!(
        navigation.source_sha256,
        Sha256Digest::of(restored.as_bytes())
    );
}

#[test]
fn symbolic_and_source_relative_references_share_complete_occurrence_and_byte_budgets() {
    let target = target();
    let ordinary = source("# Source\r\n\r\n[ordinary](../../two/target/index.md#detail)\r\n");
    let combined = ordinary.clone() + "\r\n[[site:two.test/target/#detail|Crossing]]\r\n";
    // Find the exact boundary through the same native analysis, not a second
    // estimate of workspace/heading/footnote record costs.
    for bytes in [false, true] {
        let mut low = 0;
        let mut high = if bytes { 1 << 20 } else { 1 << 10 };
        while low < high {
            let middle = low + (high - low) / 2;
            let limits = if bytes {
                Limits {
                    navigation_bytes: middle,
                    ..Limits::default()
                }
            } else {
                Limits {
                    navigation_occurrences: middle,
                    ..Limits::default()
                }
            };
            match analyze(&ordinary, &target, &[], limits, false) {
                Ok(analysis) => {
                    assert!(analysis.structurally_valid());
                    high = middle;
                }
                Err(Error::Limit(_)) => low = middle + 1,
                other => panic!("unexpected bounded native result {other:?}"),
            }
        }
        let limits = if bytes {
            Limits {
                navigation_bytes: low,
                ..Limits::default()
            }
        } else {
            Limits {
                navigation_occurrences: low,
                ..Limits::default()
            }
        };
        assert!(analyze(&ordinary, &target, &[], limits, false).is_ok());
        assert!(
            matches!(
                analyze(&combined, &target, &[], limits, false),
                Err(Error::Limit(_))
            ),
            "the publication reference cannot reset or exceed the shared {bytes:?} budget"
        );
    }
}

#[test]
fn identity_completion_repairs_unresolved_current_selector_with_exact_cross_site_and_notes_targets()
{
    let source = source("# Source 😀\r\n\r\nPréface β [[id:content:ta|Visible β label]]\r\n");
    let target = target();
    let notes = "## Notes Détail β {#detail}\r\n\r\nRetained commentary.\r\n";
    let analysis = analyze(
        &source,
        &target,
        &[(TARGET_NOTES, notes)],
        Limits::default(),
        false,
    )
    .unwrap();
    assert!(
        !analysis.structurally_valid(),
        "the unresolved current selector is not admitted"
    );
    let offset = source.find("id:content:ta").unwrap() + "id:".len();
    let completion = analysis
        .complete_symbolic_reference(&path(SOURCE_PATH), &source, offset, 1 << 20)
        .unwrap()
        .unwrap();
    assert_eq!(completion.path, &path(SOURCE_PATH));
    assert_eq!(
        completion.source_sha256,
        Sha256Digest::of(source.as_bytes())
    );
    assert_eq!(completion.location.precision, Precision::Exact);
    assert_eq!(completion.preimage, "id:content:ta");
    assert_eq!(
        completion.location.span.slice(&source),
        Some(completion.preimage)
    );
    assert!(completion.current.is_none());
    let page = completion
        .candidates
        .iter()
        .find(|candidate| candidate.replacement == "id:content:target")
        .unwrap();
    assert_eq!(page.content.as_str(), "content:target");
    assert_eq!(page.site.as_str(), "site:two");
    assert_eq!(page.path, path(TARGET_PATH));
    assert_eq!(page.source_sha256, Sha256Digest::of(target.as_bytes()));
    assert_eq!(page.definition.precision, Precision::Construct);
    let heading = completion
        .candidates
        .iter()
        .find(|candidate| candidate.replacement == "id:content:target#detail")
        .unwrap();
    assert_eq!(
        heading.definition.span.slice(&target),
        Some("## Détail β {#detail}")
    );
    let note = completion
        .candidates
        .iter()
        .find(|candidate| candidate.replacement == "id:content:target#gazette-notes-detail")
        .unwrap();
    assert_eq!(
        note.content, page.content,
        "notes retain the one article identity"
    );
    assert_eq!(note.path, path(TARGET_NOTES));
    assert_eq!(note.source_sha256, Sha256Digest::of(notes.as_bytes()));
    assert_eq!(
        note.definition.span.slice(notes),
        Some("## Notes Détail β {#detail}")
    );
    let mut edited = source.clone();
    edited.replace_range(
        completion.location.span.start..completion.location.span.end,
        &heading.replacement,
    );
    assert!(edited.contains("[[id:content:target#detail|Visible β label]]"));
    let repaired = analyze(
        &edited,
        &target,
        &[(TARGET_NOTES, notes)],
        Limits::default(),
        false,
    )
    .unwrap();
    assert!(repaired.structurally_valid(), "{:?}", repaired.failures());
    let navigation = repaired
        .source_navigation(&path(SOURCE_PATH), &edited, edited.find("[[id:").unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(navigation.definition.path, &path(TARGET_PATH));
    assert_eq!(navigation.definition.location, heading.definition);
    let reverse = analyze(
        &source,
        &target,
        &[(TARGET_NOTES, notes)],
        Limits::default(),
        true,
    )
    .unwrap();
    let reverse = reverse
        .complete_symbolic_reference(&path(SOURCE_PATH), &source, offset, 1 << 20)
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::to_value(completion).unwrap(),
        serde_json::to_value(reverse).unwrap(),
        "selected input order does not change the complete current candidate result"
    );
}

#[test]
fn symbolic_completion_refuses_stale_utf8_and_full_result_bounds_without_changing_labels() {
    let source = source(
        "# Source 😀\r\n\r\n[[site:|Unchanged β label]]\r\n\r\n`[[site:two.test/target/|Code]]`\r\n",
    );
    let target = target();
    let analysis = analyze(&source, &target, &[], Limits::default(), false).unwrap();
    let offset = source.find("site:|").unwrap() + 5;
    let completion = analysis
        .complete_symbolic_reference(&path(SOURCE_PATH), &source, offset, 1 << 20)
        .unwrap()
        .unwrap();
    assert_eq!(completion.preimage, "site:");
    assert!(
        !completion.candidates.is_empty(),
        "incomplete selectors can use current resolved targets"
    );
    assert!(matches!(
        analysis.complete_symbolic_reference(
            &path(SOURCE_PATH),
            &(source.clone() + "changed"),
            offset,
            1 << 20
        ),
        Err(NavigationError::InvalidSource)
    ));
    assert!(matches!(
        analysis.complete_symbolic_reference(
            &path(SOURCE_PATH),
            &source,
            source.find('😀').unwrap() + 1,
            1 << 20
        ),
        Err(NavigationError::InvalidSource)
    ));
    assert!(matches!(
        analysis.complete_symbolic_reference(&path(SOURCE_PATH), &source, offset, 1),
        Err(NavigationError::Limit)
    ));
    let code = source.find("site:two.test/target/").unwrap();
    assert!(
        analysis
            .complete_symbolic_reference(&path(SOURCE_PATH), &source, code, 1 << 20)
            .unwrap()
            .is_none()
    );
    assert!(
        analysis
            .complete_symbolic_reference(
                &path(SOURCE_PATH),
                &source,
                source.find("Unchanged β label").unwrap(),
                1 << 20
            )
            .unwrap()
            .is_none()
    );
}

#[test]
fn symbolic_completion_from_notes_keeps_notes_preimage_and_rejects_unavailable_current_graph() {
    let source = source("# Source\r\n");
    let target = target();
    let notes = "# Commentary\r\n\r\n[[site:two.test/target/#missing|Keep this label]]\r\n";
    let analysis = analyze(
        &source,
        &target,
        &[(SOURCE_NOTES, notes)],
        Limits::default(),
        false,
    )
    .unwrap();
    let offset = notes.find("site:").unwrap();
    let completion = analysis
        .complete_symbolic_reference(&path(SOURCE_NOTES), notes, offset, 1 << 20)
        .unwrap()
        .unwrap();
    assert_eq!(completion.path, &path(SOURCE_NOTES));
    assert_eq!(completion.source_sha256, Sha256Digest::of(notes.as_bytes()));
    assert_eq!(
        completion.location.span.slice(notes),
        Some("site:two.test/target/#missing")
    );
    let invalid_target = target.replace("id='content:target'", "id='content:source'");
    let invalid = analyze(
        &source,
        &invalid_target,
        &[(SOURCE_NOTES, notes)],
        Limits::default(),
        false,
    )
    .unwrap();
    assert!(
        matches!(
            invalid.complete_symbolic_reference(&path(SOURCE_NOTES), notes, offset, 1 << 20),
            Err(NavigationError::Unavailable)
        ),
        "current invalid graph never borrows a prior target catalog"
    );
}
