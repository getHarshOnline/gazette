//! Reduced legal-source diagnostics preserve the owner's exact metadata coordinates.

use gazette_sdk::authoring::{self, Input};

const WORKSPACE: &str = "\
[[principals]]\nid='person:owner'\nkind='person'\n\
[[sites]]\nid='site:one'\npublisher='person:owner'\ndeployable=false\n\
[[sources]]\nid='source:legal'\nlocation={kind='filesystem',root='.'}\n\
[[brands]]\nid='brand:one'\nname='One'\n\
[[brands.publications]]\nsite='site:one'\npublic_base_url='https://example.invalid/'\n\
legal=[{source='source:legal',entry='terms.md',source_content='content:owner-terms',id='content:site-terms',route='/terms/',family='utility'}]\n";

fn input<'a>(path: &str, source: &'a str) -> Input<'a> {
    Input {
        path: path.parse().expect("exact logical input"),
        bytes: source.as_bytes(),
    }
}

#[test]
fn wrong_legal_owner_identity_reports_its_real_span_not_the_composed_output_id() {
    let source = "+++\n[content]\nid='content:wrong-owner'\ntitle='Terms'\nlifecycle='draft'\n+++\n# Retained owner prose\n";
    let path = "__legal/source:legal/terms.md".parse().unwrap();
    let analysis = authoring::analyze(
        input("workspace.toml", WORKSPACE),
        [input("__legal/source:legal/terms.md", source)],
        Default::default(),
    )
    .unwrap();
    let rows = authoring::diagnostics(&analysis, &path, source, 1 << 20).unwrap();
    let row = rows
        .iter()
        .find(|row| row.code == "legal_source_identity")
        .expect("native legal identity diagnostic");
    let selected = row
        .location
        .expect("actual source identity coordinates")
        .span
        .slice(source)
        .unwrap();
    assert!(selected.contains("content:wrong-owner"));
    assert!(!selected.contains("content:site-terms"));
    assert!(!row.selection_wide);
}

#[test]
fn reduced_legal_schema_failure_retains_native_reason_and_source_coordinates() {
    let source = "+++\n[content]\nid='content:owner-terms'\ntitle='Terms'\nlifecycle='draft'\nsite='site:one'\n+++\n# Prose\n";
    let path = "__legal/source:legal/terms.md".parse().unwrap();
    let analysis = authoring::analyze(
        input("workspace.toml", WORKSPACE),
        [input("__legal/source:legal/terms.md", source)],
        Default::default(),
    )
    .unwrap();
    let rows = authoring::diagnostics(&analysis, &path, source, 1 << 20).unwrap();
    let row = rows
        .iter()
        .find(|row| row.code == "legal_source_metadata")
        .expect("owner source does not acquire a site from unsupported metadata");
    assert!(row.message.contains("site"));
    assert!(
        row.location
            .expect("native schema failure span")
            .span
            .slice(source)
            .unwrap()
            .contains("site")
    );
}

#[test]
fn missing_legal_input_reports_selection_dependency_without_blame_on_another_document() {
    let other = "# Unrelated editing buffer\n";
    let analysis = authoring::analyze(
        input("workspace.toml", WORKSPACE),
        [input("other.md", other)],
        Default::default(),
    )
    .unwrap();
    let path = "other.md".parse().unwrap();
    let rows = authoring::diagnostics(&analysis, &path, other, 1 << 20).unwrap();
    let row = rows
        .iter()
        .find(|row| row.code == "legal_source_input")
        .expect("missing exact legal source");
    assert!(row.selection_wide);
    assert!(row.location.is_none());
    assert!(row.source.is_none());
}
