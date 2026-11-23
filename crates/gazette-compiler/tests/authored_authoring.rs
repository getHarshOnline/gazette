//! Frontmatter assistance reuses the compiler's tolerant TOML/schema substrate.

use gazette_compiler::{authored::AuthoredAuthoring, document, workspace};

#[test]
fn article_metadata_completion_uses_the_native_contract() {
    for (fragment, expected) in [("keyw", "keywords"), ("schema_t", "schema_type")] {
        let source = format!("+++\n[content]\n{fragment}");
        let authoring = AuthoredAuthoring::parse(&source, Default::default())
            .unwrap()
            .unwrap();
        let help = authoring
            .help(&source, source.len(), 1 << 18)
            .unwrap()
            .unwrap();
        assert_eq!(
            help.candidates
                .iter()
                .map(|candidate| candidate.label.as_str())
                .collect::<Vec<_>>(),
            [expected]
        );
    }
    let source = "+++\n[content]\nschema_type = \n+++\nBody\n";
    let authoring = AuthoredAuthoring::parse(source, Default::default())
        .unwrap()
        .unwrap();
    let help = authoring
        .help(source, source.find("\n+++").unwrap(), 1 << 18)
        .unwrap()
        .unwrap();
    assert_eq!(
        help.candidates
            .iter()
            .map(|candidate| candidate.label.as_str())
            .collect::<Vec<_>>(),
        ["AboutPage", "Article", "BlogPosting", "TechArticle"]
    );
}

#[test]
fn incomplete_frontmatter_uses_exact_content_schema_and_full_file_ranges() {
    let source = "+++\r\n# é🦀\r\n[content]\r\nlifec";
    let authoring = AuthoredAuthoring::parse(source, document::Limits::default())
        .expect("bounded facts")
        .expect("explicit frontmatter");
    let help = authoring
        .help(source, source.len(), 1 << 18)
        .expect("schema help")
        .expect("cursor is in metadata");
    assert_eq!(help.completion_kind, Some(workspace::CompletionKind::Field));
    assert_eq!(help.completion_preimage.as_deref(), Some("lifec"));
    assert_eq!(help.candidates.len(), 1);
    assert_eq!(help.candidates[0].label, "lifecycle");
    let span = help.completion_location.expect("exact edit").span;
    assert_eq!(span.slice(source), Some("lifec"));
}

#[test]
fn lifecycle_value_completion_and_hover_come_from_the_strict_serde_schema() {
    let source = "+++\n[content]\nlifecycle = \n+++\nBody\n";
    let authoring = AuthoredAuthoring::parse(source, document::Limits::default())
        .expect("bounded facts")
        .expect("explicit frontmatter");
    let values = authoring
        .help(
            source,
            source.find("\n+++").expect("closing fence"),
            1 << 18,
        )
        .expect("literal help")
        .expect("metadata help");
    assert_eq!(
        values.completion_kind,
        Some(workspace::CompletionKind::Literal)
    );
    assert_eq!(
        values
            .candidates
            .iter()
            .map(|candidate| candidate.label.as_str())
            .collect::<Vec<_>>(),
        ["archived", "draft", "published"]
    );

    let start = source.find("lifecycle").expect("field");
    let hover = authoring
        .help(source, start + 2, 1 << 18)
        .expect("hover facts")
        .expect("metadata help")
        .hover
        .expect("schema hover");
    assert_eq!(hover.location.span.slice(source), Some("lifecycle"));
    let field = hover.field.expect("field facts");
    assert_eq!(field.name, "lifecycle");
    assert_eq!(field.presence, workspace::AuthoringFieldPresence::Required);
    assert!(
        field
            .description
            .as_deref()
            .is_some_and(|description| description.contains("content state"))
    );
}

#[test]
fn plain_markdown_and_body_positions_do_not_invent_frontmatter_help() {
    assert!(
        AuthoredAuthoring::parse("# Plain\n", document::Limits::default())
            .expect("plain markdown")
            .is_none()
    );
    let source = "+++\n[content]\nlifecycle='draft'\n+++\n# Body\n";
    let authoring = AuthoredAuthoring::parse(source, document::Limits::default())
        .expect("bounded facts")
        .expect("explicit frontmatter");
    assert!(
        authoring
            .help(source, source.find("# Body").expect("body"), 1 << 18)
            .expect("body is outside metadata")
            .is_none()
    );
}
