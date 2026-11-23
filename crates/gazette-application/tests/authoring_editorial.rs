//! Editorial navigation through the shared compiler, not transport-side identity lookup.

use gazette_application::authoring::{self, Analysis, Input, Limits, NavigationError};
use gazette_compiler::authored::EditorialField;
use gazette_core::source::{EntryPath, Precision, Sha256Digest};

const WORKSPACE: &str = "principals=[{id='owner:one',kind='person'}]\nsites=[{id='site:one',publisher='owner:one',deployable=false}]\n";

fn document(id: &str, relations: &str, structure: &str) -> String {
    format!(
        "+++\r\n# é😀 retained metadata\r\n{relations}[content]\r\nid='{id}'\r\nsite='site:one'\r\ntitle='collection:root'\r\nlifecycle='draft'\r\nstructure={structure}\r\n+++\r\n# Heading\r\n"
    )
}
fn fixtures() -> Vec<(&'static str, String)> {
    vec![
        (
            "owners/root.md",
            document("collection:root", "", "{kind='collection'}"),
        ),
        (
            "owners/child.md",
            document(
                "collection:child",
                "",
                "{kind='collection',parent=\"collection:r\\u006fot\"}",
            ),
        ),
        (
            "owners/journey.md",
            document(
                "journey:one",
                "",
                "{kind='journey',collection='collection:child',members=[{content='content:second',primary=true},{content='content:first',primary=true}]}",
            ),
        ),
        (
            "articles/first.md",
            document("content:first", "", "{kind='page',family={kind='article'}}"),
        ),
        (
            "articles/second.md",
            document(
                "content:second",
                "",
                "{kind='page',family={kind='article'}}",
            ),
        ),
        (
            "articles/direct.md",
            document(
                "content:direct",
                "collections=['collection:child']\r\n",
                "{kind='page',family={kind='article'}}",
            ),
        ),
    ]
}
fn analyze(files: &[(&str, String)], limits: Limits) -> Analysis {
    authoring::analyze(
        Input {
            path: "workspace.toml".parse().expect("workspace path"),
            bytes: WORKSPACE.as_bytes(),
        },
        files.iter().map(|(path, text)| Input {
            path: path.parse().expect("selected path"),
            bytes: text.as_bytes(),
        }),
        limits,
        &gazette_adapter_markdown::Comrak,
    )
    .expect("one current bounded analysis")
}
fn path(value: &str) -> EntryPath {
    value.parse().expect("selected path")
}

#[test]
fn all_editorial_fields_keep_exact_scalar_definitions_and_complete_reverse_references() {
    let files = fixtures();
    let analysis = analyze(&files, Limits::default());
    assert!(analysis.structurally_valid(), "{:?}", analysis.failures());
    let cases = [
        (1, "\"collection:r\\u006fot\"", 0, "'collection:root'"),
        (2, "'collection:child'", 1, "'collection:child'"),
        (2, "'content:second'", 4, "'content:second'"),
        (2, "'content:first'", 3, "'content:first'"),
        (5, "'collection:child'", 1, "'collection:child'"),
    ];
    for (source_index, token, target_index, definition) in cases {
        let (source_path, source) = &files[source_index];
        let (target_path, target) = &files[target_index];
        let nav = analysis
            .source_navigation(
                &path(source_path),
                source,
                source.find(token).expect("native scalar"),
            )
            .expect("current source query")
            .expect("editorial association");
        assert_eq!(nav.path.as_str(), *source_path);
        assert_eq!(nav.source_sha256, Sha256Digest::of(source.as_bytes()));
        assert_eq!(nav.occurrence.precision, Precision::Exact);
        assert_eq!(nav.occurrence.span.slice(source), Some(token));
        assert_eq!(nav.definition.path.as_str(), *target_path);
        assert_eq!(
            nav.definition.source_sha256,
            Sha256Digest::of(target.as_bytes())
        );
        assert_eq!(nav.definition.location.precision, Precision::Exact);
        assert_eq!(nav.definition.location.span.slice(target), Some(definition));
        let reverse = analysis
            .source_navigation(
                &path(target_path),
                target,
                nav.definition.location.span.start,
            )
            .expect("native declaration query")
            .expect("reverse association");
        assert_eq!(reverse.definition, nav.definition);
        assert_eq!(reverse.references, nav.references);
        assert_eq!(nav.references.len(), if target_index == 1 { 2 } else { 1 });
    }
    let child = &files[1];
    let arbitrary = child
        .1
        .find("title='collection:root'")
        .expect("non-reference string")
        + "title=".len();
    assert!(
        analysis
            .source_navigation(&path(child.0), &child.1, arbitrary)
            .expect("known non-occurrence")
            .is_none()
    );
}

#[test]
fn invalid_current_graph_never_reuses_editorial_associations_and_changed_source_is_refused() {
    let files = fixtures();
    let analysis = analyze(&files, Limits::default());
    let direct = &files[5];
    let at = direct.1.find("'collection:child'").expect("membership");
    assert!(matches!(
        analysis.source_navigation(&path(direct.0), &(direct.1.clone() + "changed"), at),
        Err(NavigationError::InvalidSource)
    ));
    let mut changed = files.clone();
    changed[1].1 = changed[1]
        .1
        .replace("id='collection:child'", "id='collection:changed'");
    let invalid = analyze(&changed, Limits::default());
    assert!(!invalid.structurally_valid());
    assert!(
        invalid
            .source_navigation(&path(direct.0), &direct.1, at)
            .expect("current invalid graph has no stale facts")
            .is_none()
    );
    let restored = analyze(&files, Limits::default());
    assert!(
        restored
            .source_navigation(&path(direct.0), &direct.1, at)
            .expect("restored exact graph")
            .is_some()
    );
}

#[test]
fn editorial_associations_remain_independent_of_unrelated_broken_markdown_fragments() {
    let mut files = fixtures();
    files[5]
        .1
        .push_str("\r\n[broken](../owners/root.md#absent)\r\n");
    let analysis = analyze(&files, Limits::default());
    let direct = &files[5];
    assert!(
        analysis
            .source_navigation(
                &path(direct.0),
                &direct.1,
                direct.1.find("'collection:child'").expect("membership")
            )
            .expect("independent typed editorial source fact")
            .is_some()
    );
    assert!(matches!(
        analysis.source_navigation(
            &path(direct.0),
            &direct.1,
            direct.1.find("[broken]").expect("broken link")
        ),
        Err(NavigationError::Unavailable)
    ));
}

#[test]
fn editorial_completion_is_native_kind_and_site_correct_with_exact_original_literals() {
    let mut files = fixtures();
    files[0].1 = files[0]
        .1
        .replace("id='collection:root'", "id=\"collection:r\\u006fot\"");
    files.push((
        "foreign/collection.md",
        document("collection:foreign", "", "{kind='collection'}")
            .replace("site='site:one'", "site='site:other'"),
    ));
    files.push((
        "foreign/page.md",
        document(
            "content:foreign",
            "",
            "{kind='page',family={kind='article'}}",
        )
        .replace("site='site:one'", "site='site:other'"),
    ));
    let analysis = analyze(&files, Limits::default());
    // Unknown foreign home sites remain ordinary current graph failures; they
    // cannot leak into the same-site typed repair namespace.
    assert!(!analysis.structurally_valid());
    for (index, token, field, expected) in [
        (
            1,
            "\"collection:r\\u006fot\"",
            EditorialField::CollectionParent,
            1,
        ),
        (
            2,
            "'collection:child'",
            EditorialField::JourneyCollection,
            2,
        ),
        (2, "'content:second'", EditorialField::JourneyMember, 3),
        (
            5,
            "'collection:child'",
            EditorialField::CollectionResidence,
            2,
        ),
    ] {
        let (source_path, source) = &files[index];
        let completion = analysis
            .complete_editorial_reference(
                &path(source_path),
                source,
                source.find(token).expect("slot"),
                1 << 20,
            )
            .expect("current native proposal")
            .expect("typed editorial slot");
        assert_eq!(completion.field, field);
        assert_eq!(completion.preimage, token);
        assert_eq!(completion.location.precision, Precision::Exact);
        assert_eq!(
            completion.source_sha256,
            Sha256Digest::of(source.as_bytes())
        );
        assert_eq!(completion.candidates.len(), expected);
        assert!(completion.current.is_some());
        assert!(completion.candidates.iter().all(|candidate| {
            candidate.declaration.site.as_str() == "site:one"
                && field
                    .target_kind()
                    .is_none_or(|kind| candidate.declaration.kind == kind)
        }));
        if field != EditorialField::JourneyMember {
            assert!(
                completion
                    .candidates
                    .iter()
                    .any(|candidate| candidate.replacement == "\"collection:r\\u006fot\"")
            );
        }
        let bytes = serde_json::to_vec(&completion)
            .expect("native JSON projection")
            .len();
        assert!(
            analysis
                .complete_editorial_reference(
                    &path(source_path),
                    source,
                    source.find(token).unwrap(),
                    bytes
                )
                .is_ok()
        );
        assert!(matches!(
            analysis.complete_editorial_reference(
                &path(source_path),
                source,
                source.find(token).unwrap(),
                bytes - 1
            ),
            Err(NavigationError::Limit)
        ));
    }
}

#[test]
fn unresolved_editorial_scalar_can_be_repaired_without_stale_graph_or_tolerant_help_loss() {
    let mut files = fixtures();
    files[5].1 = files[5].1.replace(
        "collections=['collection:child']",
        "collections=['collection:missing']",
    );
    let analysis = analyze(&files, Limits::default());
    assert!(!analysis.structurally_valid());
    let (source_path, source) = &files[5];
    let at = source
        .find("'collection:missing'")
        .expect("unresolved scalar");
    let completion = analysis
        .complete_editorial_reference(&path(source_path), source, at, 1 << 20)
        .expect("current strict typed slot")
        .expect("repair proposal");
    assert!(completion.current.is_none());
    assert_eq!(completion.preimage, "'collection:missing'");
    assert_eq!(completion.candidates.len(), 2);
    assert!(
        analysis
            .source_navigation(&path(source_path), source, at)
            .expect("no stale graph association")
            .is_none()
    );
    assert!(matches!(
        analysis.complete_editorial_reference(
            &path(source_path),
            &(source.clone() + "change"),
            at,
            1 << 20
        ),
        Err(NavigationError::InvalidSource)
    ));
    let mut incomplete = files.clone();
    incomplete[5].1 = source
        .split("+++\r\n# Heading")
        .next()
        .expect("open frame")
        .to_owned();
    let current = analyze(&incomplete, Limits::default());
    let text = &incomplete[5].1;
    assert!(!current.structurally_valid());
    assert!(
        current
            .assistance(
                &path(source_path),
                text,
                text.find("collections").expect("sound tolerant field"),
                1 << 20
            )
            .expect("independent tolerant schema help")
            .is_some()
    );
    assert!(matches!(
        current.complete_editorial_reference(&path(source_path), text, at, 1 << 20),
        Err(NavigationError::Unavailable)
    ));
}

#[test]
fn documentation_outline_completion_accepts_native_pages_and_containers_not_group_labels() {
    let mut files = fixtures();
    files[0].1 = document(
        "collection:root",
        "",
        "{kind='collection',purpose='documentation',navigation=[{group='Read',pages=[{content='collection:missing'}]}]}",
    );
    let analysis = analyze(&files, Limits::default());
    assert!(!analysis.structurally_valid());
    let (source_path, source) = &files[0];
    let completion = analysis
        .complete_editorial_reference(
            &path(source_path),
            source,
            source.find("'collection:missing'").unwrap(),
            1 << 20,
        )
        .unwrap()
        .unwrap();
    assert_eq!(completion.field, EditorialField::DocumentationNavigation);
    assert_eq!(completion.preimage, "'collection:missing'");
    assert!(completion.current.is_none());
    assert_eq!(completion.candidates.len(), 5);
    for kind in [
        gazette_compiler::authored::EditorialKind::Page,
        gazette_compiler::authored::EditorialKind::Collection,
        gazette_compiler::authored::EditorialKind::Journey,
    ] {
        assert!(
            completion
                .candidates
                .iter()
                .any(|candidate| candidate.declaration.kind == kind)
        );
    }
    assert!(
        analysis
            .complete_editorial_reference(
                &path(source_path),
                source,
                source.find("'Read'").unwrap(),
                1 << 20
            )
            .unwrap()
            .is_none()
    );
}
