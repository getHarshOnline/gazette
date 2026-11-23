//! Exact metadata occurrences and whole-result bounds over native compiler models.
use gazette_compiler::{
    authored,
    corpus::{Input, ParsedCorpus},
    workspace,
};
use gazette_core::{publication::content, source::Precision};

#[test]
fn editorial_projection_preserves_native_array_order_locations_and_exact_complete_bounds() {
    let workspace = workspace::compile("principals=[{id='owner:one',kind='person'}]\nsites=[{id='site:one',publisher='owner:one',deployable=false}]\n", Default::default()).expect("workspace");
    let framed = |id: &str, structure: &str| {
        format!(
            "+++\r\n# é😀\r\n[content]\r\nid='{id}'\r\nsite='site:one'\r\ntitle='Title'\r\nlifecycle='draft'\r\nstructure={structure}\r\n+++\r\nBody\r\n"
        )
    };
    let files = [
        ("root.md", framed("collection:root", "{kind='collection'}")),
        (
            "journey.md",
            framed(
                "journey:one",
                "{kind='journey',collection='collection:root',members=[{content='content:z',primary=true},{content=\"content:\\u0061\",primary=true}]}",
            ),
        ),
        (
            "a.md",
            framed("content:a", "{kind='page',family={kind='article'}}"),
        ),
        (
            "z.md",
            framed("content:z", "{kind='page',family={kind='article'}}"),
        ),
    ];
    let corpus = ParsedCorpus::parse(
        files.iter().map(|(path, source)| Input {
            path: path.parse().expect("path"),
            bytes: source.as_bytes(),
        }),
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("retained native corpus");
    let resolved = authored::resolve(&workspace, &corpus, content::Limits::default())
        .expect("sole journey residence");
    let references = resolved
        .editorial_references(3, usize::MAX)
        .expect("complete projection");
    assert_eq!(references.len(), 3);
    let tokens: Vec<_> = references
        .iter()
        .map(|reference| {
            assert_eq!(reference.path.as_str(), "journey.md");
            assert_eq!(reference.location.precision, Precision::Exact);
            assert_eq!(reference.definition.precision, Precision::Exact);
            reference
                .location
                .span
                .slice(&files[1].1)
                .expect("exact source scalar")
        })
        .collect();
    assert_eq!(
        tokens,
        ["'collection:root'", "'content:z'", "\"content:\\u0061\""]
    );
    assert_eq!(references[1].target.as_str(), "z.md");
    assert_eq!(references[2].target.as_str(), "a.md");
    assert_eq!(
        references[2].definition.span.slice(&files[2].1),
        Some("'content:a'")
    );
    let bytes = references
        .iter()
        .map(|reference| {
            std::mem::size_of::<authored::EditorialReference<'_>>()
                + reference.path.as_str().len()
                + reference.target.as_str().len()
        })
        .sum();
    assert!(resolved.editorial_references(3, bytes).is_ok());
    assert!(matches!(
        resolved.editorial_references(2, bytes),
        Err(authored::ProjectionError::Limit(
            "editorial reference occurrences"
        ))
    ));
    assert!(matches!(
        resolved.editorial_references(3, bytes - 1),
        Err(authored::ProjectionError::Limit(
            "editorial reference bytes"
        ))
    ));
    let facts = authored::editorial_authoring(&workspace, &corpus, 7, usize::MAX)
        .expect("native declaration and scalar facts");
    assert_eq!(facts.declarations().len(), 4);
    assert_eq!(facts.occurrence_count(), 7);
    let at = files[1]
        .1
        .find("\"content:\\u0061\"")
        .expect("escaped native slot");
    let slot = facts
        .slot_at(&"journey.md".parse().expect("path"), at)
        .expect("exact native member slot");
    assert_eq!(slot.field, authored::EditorialField::JourneyMember);
    assert_eq!(slot.target.as_str(), "content:a");
    let declaration = facts
        .declarations()
        .iter()
        .find(|declaration| declaration.identity.as_str() == "content:a")
        .expect("typed source declaration");
    assert_eq!(declaration.replacement(), "'content:a'");
    assert!(authored::editorial_authoring(&workspace, &corpus, 7, facts.fact_bytes()).is_ok());
    assert!(matches!(
        authored::editorial_authoring(&workspace, &corpus, 6, facts.fact_bytes()),
        Err(authored::ProjectionError::Limit(
            "editorial authoring occurrences"
        ))
    ));
    assert!(matches!(
        authored::editorial_authoring(&workspace, &corpus, 7, facts.fact_bytes() - 1),
        Err(authored::ProjectionError::Limit(
            "editorial authoring bytes"
        ))
    ));
}

#[test]
fn documentation_outline_targets_keep_exact_reference_locations_and_authoring_slots() {
    let workspace = workspace::compile("principals=[{id='owner:one',kind='person'}]\nsites=[{id='site:one',publisher='owner:one',deployable=false}]\n", Default::default()).unwrap();
    let root = "+++\n[content]\nid='collection:root'\nsite='site:one'\ntitle='Manual'\nlifecycle='draft'\nstructure={kind='collection',purpose='documentation',navigation=[{group='Start',pages=[{content='page:one'}]}]}\n+++\n# Manual\n";
    for (source, field) in [
        (
            root.to_owned(),
            authored::EditorialField::DocumentationNavigation,
        ),
        (
            root.replace(
                "purpose='documentation',navigation=[{group='Start',pages=[{content='page:one'}]}]",
                "member_order=['page:one']",
            ),
            authored::EditorialField::CollectionOrder,
        ),
    ] {
        let root = source.as_str();
        let leaf = "+++\ncollections=['collection:root']\n[content]\nid='page:one'\nsite='site:one'\ntitle='First use'\nlifecycle='draft'\nstructure={kind='page',family={kind='documentation'}}\n+++\n# First use\n";
        let files = [("root.md", root), ("one.md", leaf)];
        let corpus = ParsedCorpus::parse(
            files.iter().map(|(path, source)| Input {
                path: path.parse().unwrap(),
                bytes: source.as_bytes(),
            }),
            Default::default(),
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
        let resolved = authored::resolve(&workspace, &corpus, Default::default()).unwrap();
        let references = resolved.editorial_references(8, usize::MAX).unwrap();
        let reference = references
            .iter()
            .find(|reference| reference.path.as_str() == "root.md")
            .unwrap();
        assert_eq!(reference.location.span.slice(root), Some("'page:one'"));
        assert_eq!(reference.target.as_str(), "one.md");
        let facts = authored::editorial_authoring(&workspace, &corpus, 8, usize::MAX).unwrap();
        let at = root.find("'page:one'").unwrap();
        let slot = facts.slot_at(&"root.md".parse().unwrap(), at).unwrap();
        assert_eq!(slot.field, field);
        let syntax = authored::AuthoredAuthoring::parse(root, Default::default())
            .unwrap()
            .unwrap();
        assert_eq!(syntax.editorial_field(root, at).unwrap(), Some(field));
    }
}
