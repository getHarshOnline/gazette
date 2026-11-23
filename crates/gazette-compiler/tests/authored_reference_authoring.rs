//! Native authored-reference fields and reduced legal schema, without another parser.

use gazette_compiler::{
    authored::{AuthoredAuthoring, AuthoredReferenceField, AuthoredSchema},
    workspace,
};

#[test]
fn subject_and_provider_slots_retain_their_own_role_and_exact_identity() {
    for (field, role) in [
        ("principal", AuthoredReferenceField::SubjectPrincipal),
        ("provider", AuthoredReferenceField::ServiceProvider),
    ] {
        let source =
            format!("+++\n[content.subject]\n{field}='person:one'\nname='person:one'\n+++\nBody\n");
        let authoring = AuthoredAuthoring::parse(&source, Default::default())
            .unwrap()
            .unwrap();
        let slot = authoring
            .reference_slot(&source, source.find("'person:one'").unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(slot.field, role);
        assert_eq!(slot.target.as_str(), "person:one");
        assert_eq!(slot.location.span.slice(&source), Some("'person:one'"));
        assert!(
            authoring
                .reference_slot(&source, source.rfind("'person:one'").unwrap())
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn exact_native_reference_slots_preserve_unicode_crlf_escaped_scalar_and_field_namespace() {
    let source = "+++\r\n# é🦀\r\nrelated=['content:t\\u006fo']\r\n[content]\r\nsite='site:one'\r\nattributions=[{principal=\"person:o\\u006ene\",role='author'}]\r\ntitle='person:one'\r\n+++\r\n# Body\r\n";
    let authoring = AuthoredAuthoring::parse(source, Default::default())
        .unwrap()
        .unwrap();
    for (literal, identity, field) in [
        (
            "'content:t\\u006fo'",
            "content:t\\u006fo",
            AuthoredReferenceField::RelatedContent,
        ),
        ("'site:one'", "site:one", AuthoredReferenceField::Site),
        (
            "\"person:o\\u006ene\"",
            "person:onne",
            AuthoredReferenceField::AttributionPrincipal,
        ),
    ] {
        // Literal single-quoted TOML preserves backslashes; the opaque Key owner
        // accepts that exact spelling, rather than inventing a decoded identity.
        let slot = authoring
            .reference_slot(source, source.find(literal).unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(slot.field, field);
        assert_eq!(slot.target.as_str(), identity);
        assert_eq!(slot.location.span.slice(source), Some(literal));
    }
    assert!(
        authoring
            .reference_slot(source, source.find("title=").unwrap() + 7)
            .unwrap()
            .is_none()
    );
    assert!(
        authoring
            .reference_slot(source, source.find("# Body").unwrap())
            .unwrap()
            .is_none()
    );
}

#[test]
fn recovered_reference_scalar_survives_incomplete_neighbor_and_unclosed_frame() {
    let source = "+++\r\n# é🦀\r\n[content]\r\nsite='site:typo'\r\ntitle=\r\n";
    let authoring = AuthoredAuthoring::parse(source, Default::default())
        .unwrap()
        .unwrap();
    let slot = authoring
        .reference_slot(source, source.find("site:typo").unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(slot.field, AuthoredReferenceField::Site);
    assert_eq!(slot.target.as_str(), "site:typo");
    assert_eq!(slot.location.span.slice(source), Some("'site:typo'"));
    assert_eq!(
        slot.location.precision,
        gazette_core::source::Precision::Exact
    );
    assert!(
        gazette_compiler::document::ParsedDocument::parse(
            source,
            Default::default(),
            &gazette_adapter_markdown::Comrak
        )
        .is_err(),
        "recovered scalar evidence never admits the incomplete document"
    );
}

#[test]
fn reduced_legal_hints_use_only_the_actual_native_schema_even_while_incomplete() {
    let source = "+++\r\n# é🦀\r\n[content]\r\n";
    let legal = AuthoredAuthoring::parse_with_schema(
        source,
        Default::default(),
        AuthoredSchema::LegalSource,
    )
    .unwrap()
    .unwrap();
    assert_eq!(legal.schema(), AuthoredSchema::LegalSource);
    let help = legal.help(source, source.len(), 1 << 18).unwrap().unwrap();
    let fields = help
        .candidates
        .iter()
        .map(|field| field.label.as_str())
        .collect::<Vec<_>>();
    for expected in ["id", "title", "description", "lifecycle", "language"] {
        assert!(fields.contains(&expected), "{fields:?}");
    }
    for excluded in [
        "site",
        "structure",
        "attributions",
        "keywords",
        "schema_type",
    ] {
        assert!(!fields.contains(&excluded), "{fields:?}");
    }
    let values = "+++\r\n[content]\r\nlifecycle = ";
    let legal = AuthoredAuthoring::parse_with_schema(
        values,
        Default::default(),
        AuthoredSchema::LegalSource,
    )
    .unwrap()
    .unwrap();
    let help = legal.help(values, values.len(), 1 << 18).unwrap().unwrap();
    assert_eq!(
        help.completion_kind,
        Some(workspace::CompletionKind::Literal)
    );
    assert_eq!(
        help.candidates
            .iter()
            .map(|item| item.label.as_str())
            .collect::<Vec<_>>(),
        ["archived", "draft", "published"]
    );
    assert!(
        gazette_compiler::document::ParsedDocument::parse(
            values,
            Default::default(),
            &gazette_adapter_markdown::Comrak
        )
        .is_err(),
        "tolerant hints never admit incomplete source"
    );
}

#[test]
fn identical_page_and_legal_bytes_keep_explicit_distinct_help_profiles() {
    let source = "+++\n[content]\nsite='site:missing'\n";
    let page = AuthoredAuthoring::parse(source, Default::default())
        .unwrap()
        .unwrap();
    let legal = AuthoredAuthoring::parse_with_schema(
        source,
        Default::default(),
        AuthoredSchema::LegalSource,
    )
    .unwrap()
    .unwrap();
    let at = source.find("site:missing").unwrap();
    assert_eq!(
        page.reference_slot(source, at).unwrap().unwrap().field,
        AuthoredReferenceField::Site
    );
    assert!(legal.reference_slot(source, at).unwrap().is_none());
}
