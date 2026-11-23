//! Exact source acquisition to compiler projections through the real Rust SDK.

use std::fs;

use gazette_sdk::{
    document,
    markdown::{Limits, ParsedMarkdown},
    source::{self, RevisionLimits},
};
use git2::Repository;
use serde::Deserialize;

use super::{Fixture, commit_docs};

#[test]
fn pinned_document_retains_metadata_and_body_without_reopening_live_source() {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Metadata {
        title: String,
        draft: bool,
    }
    let fixture = Fixture::new();
    let original = "+++\ntitle = 'Pinned metadata'\ndraft = true\n+++\n# Pinned body\n";
    let path = fixture.root.path().join("docs/readme.md");
    fs::write(&path, original).expect("authored source");
    let repo = Repository::open(fixture.root.path()).expect("repository");
    let blob = repo.blob(original.as_bytes()).expect("blob");
    let commit = commit_docs(&repo, &[("readme.md", blob, 0o100644)], "TOML document");
    let acquired = source::acquire(
        fixture.root.path(),
        &"docs".parse().expect("selection"),
        &commit.to_string().parse().expect("pin"),
        RevisionLimits::default(),
    )
    .expect("verified capture");
    let changed = "+++\ntitle = 'Unadmitted change'\ndraft = false\n+++\n# Changed body\n";
    fs::write(&path, changed).expect("subsequent source edit");
    let bytes = acquired
        .file(&"readme.md".parse().expect("entry"))
        .expect("retained source");
    let parsed = document::ParsedDocument::parse(
        std::str::from_utf8(bytes).expect("UTF-8"),
        document::Limits::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("captured document");
    let metadata: Metadata = parsed.metadata().deserialize().expect("fixture schema");
    assert_eq!(metadata.title, "Pinned metadata");
    assert!(metadata.draft, "syntax inspection must not grant admission");
    assert_eq!(
        parsed.inspect().expect("inspection").markdown.headings[0].text,
        "Pinned body"
    );
    assert_eq!(
        parsed.render_html().expect("syntax fragment"),
        "<h1 id=\"pinned-body\">Pinned body</h1>\n"
    );
    assert_eq!(
        fs::read_to_string(path).expect("preserved live file"),
        changed
    );
}

#[test]
fn pinned_acquired_document_drives_both_projections_after_live_source_changes() {
    let fixture = Fixture::new();
    let markdown = "# Captured source\n\n[Reference](https://example.invalid/docs)\n";
    let repo = Repository::open(fixture.root.path()).expect("fixture repository");
    fs::write(fixture.root.path().join("docs/readme.md"), markdown)
        .expect("native authored document");
    let blob = repo.blob(markdown.as_bytes()).expect("blob");
    let commit = commit_docs(&repo, &[("readme.md", blob, 0o100644)], "Markdown fixture");
    let acquired = source::acquire(
        fixture.root.path(),
        &"docs".parse().expect("subdir"),
        &commit.to_string().parse().expect("pin"),
        RevisionLimits::default(),
    )
    .expect("exact acquisition");
    fs::write(
        fixture.root.path().join("docs/readme.md"),
        "# Later unadmitted bytes\n",
    )
    .expect("later edit");
    let body = std::str::from_utf8(
        acquired
            .file(&"readme.md".parse().expect("entry"))
            .expect("captured bytes"),
    )
    .expect("UTF-8");
    let parsed = ParsedMarkdown::parse(body, Limits::default(), &gazette_adapter_markdown::Comrak)
        .expect("compiler");
    let inspection = parsed.inspect().expect("inspection");
    let html = parsed.render_html().expect("fragment");
    assert_eq!(inspection.headings.len(), 1);
    assert_eq!(inspection.headings[0].text, "Captured source");
    assert_eq!(inspection.links.len(), 1);
    assert_eq!(
        inspection.links[0].destination,
        "https://example.invalid/docs"
    );
    assert_eq!(
        html,
        "<h1 id=\"captured-source\">Captured source</h1>\n<p><a href=\"https://example.invalid/docs\">Reference</a></p>\n"
    );
    assert_eq!(
        fs::read_to_string(fixture.root.path().join("docs/readme.md"))
            .expect("live source preserved"),
        "# Later unadmitted bytes\n"
    );
}
