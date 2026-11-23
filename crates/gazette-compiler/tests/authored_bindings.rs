//! Real compiler source relationships stay separate from authored semantic edges.

use gazette_compiler::{
    authored,
    corpus::{Input, ParsedCorpus},
    workspace,
};
use gazette_core::publication::Key;

fn key(value: &str) -> Key {
    value.parse().expect("fixture key")
}

fn compilation() -> workspace::Compilation {
    workspace::compile(
        "[[principals]]\nid='owner:one'\nkind='person'\n\
         [[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n\
         [[sites]]\nid='site:other'\npublisher='owner:one'\ndeployable=false\n\
         [[brands]]\nid='brand:one'\nname='One'\n\
         [[brands.publications]]\nsite='site:one'\nauthored=[{source='source:shared',target='authored'}]\n\
         [[projects]]\nid='project:native'\nlifecycle='active'\nvisibility='public'\navailability='available'\n\
         [[sources]]\nid='source:shared'\nlocation={kind='repository',root='/private/native',commit='1111111111111111111111111111111111111111',subdirectory='docs'}\n\
         [[placements]]\nrole='primary'\nsite='site:one'\nproject='project:native'\nrender=true\ndiscoverable=false\ndocuments={source='source:shared',target='projects/native'}\n",
        Default::default(),
    ).expect("workspace compiler")
}

fn document(site: &str) -> String {
    format!(
        "+++\n[content]\nid='content:guide'\nsite='{site}'\ntitle='Guide'\nlifecycle='draft'\n[content.structure]\nkind='page'\nfamily={{kind='documentation'}}\n+++\n# Retained body\n"
    )
}

#[test]
fn source_binding_is_borrowed_provenance_not_an_authored_relation() {
    let compilation = compilation();
    let text = document("site:one");
    let path: gazette_core::source::EntryPath =
        "projects/native/nested/guide.md".parse().expect("path");
    let corpus = ParsedCorpus::parse(
        [Input {
            path: path.clone(),
            bytes: text.as_bytes(),
        }],
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("syntax");
    let resolved = authored::resolve(&compilation, &corpus, Default::default()).expect("graph");
    resolved
        .validate_source_bindings()
        .expect("complete source association");
    let source = resolved
        .source_binding(&key("content:guide"))
        .expect("bound source");
    assert_eq!(source.project, Some(&key("project:native")));
    assert_eq!(source.entry(&path), Some("nested/guide.md"));
    assert!(std::ptr::eq(
        source.tree_binding().expect("authored tree binding"),
        compilation.workspace.placements()[0]
            .documents
            .as_ref()
            .expect("binding")
    ));
    let page = resolved
        .project_page(
            &key("content:guide"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("projection");
    assert!(page.data.projects.is_empty());
    assert!(page.data.project_declarations.is_empty());
    assert!(page.data.collections.is_empty());
    assert!(page.data.journeys.is_empty());
    assert!(page.body_html.contains("Retained body"));
    let json = serde_json::to_value(&page).expect("same model serialization");
    assert_eq!(
        json["provenance"],
        serde_json::json!({"site":"site:one","project":"project:native"})
    );
    let serialized = serde_json::to_string(&page).expect("model bytes");
    for private in [
        "/private/native",
        "source:shared",
        "projects/native",
        "nested/guide.md",
    ] {
        assert!(
            !serialized.contains(private),
            "private coordinate leaked: {private}"
        );
    }
    assert_eq!(resolved.source_path(&key("content:guide")), Some(&path));
}

#[test]
fn source_home_conflicts_fail_both_binding_and_projection_without_relocation() {
    let compilation = compilation();
    let text = document("site:other");
    let corpus = ParsedCorpus::parse(
        [Input {
            path: "projects/native/guide.md".parse().expect("path"),
            bytes: text.as_bytes(),
        }],
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("syntax");
    let resolved = authored::resolve(&compilation, &corpus, Default::default())
        .expect("structurally valid graph");
    assert!(
        matches!(resolved.validate_source_bindings(), Err(authored::SourceBindingError::Home {content,declared,configured}) if content == key("content:guide") && declared == key("site:other") && configured == key("site:one"))
    );
    assert_eq!(
        resolved
            .project_page(
                &key("content:guide"),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak
            )
            .expect_err("no contradictory view")
            .code(),
        "publication_home"
    );
    assert_eq!(
        resolved
            .content()
            .graph()
            .node(&key("content:guide"))
            .expect("original node")
            .site,
        key("site:other")
    );
}

#[test]
fn logical_prefix_matching_is_component_exact_and_unbound_inspection_is_not_publication() {
    let compilation = compilation();
    for path in [
        "projects/native",
        "projects/native-other/file.md",
        "unbound/file.md",
    ] {
        assert!(
            compilation
                .workspace
                .content_source(&path.parse().expect("literal path"))
                .is_none()
        );
    }
    let text = document("site:one");
    let corpus = ParsedCorpus::parse(
        [Input {
            path: "unbound/file.md".parse().expect("path"),
            bytes: text.as_bytes(),
        }],
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("syntax");
    let resolved = authored::resolve(&compilation, &corpus, Default::default())
        .expect("local graph inspection");
    assert!(
        matches!(resolved.validate_source_bindings(), Err(authored::SourceBindingError::Missing(id)) if id == key("content:guide"))
    );
    assert!(
        resolved
            .project_page(
                &key("content:guide"),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak
            )
            .expect("local unbound view")
            .data
            .provenance
            .is_none()
    );
}
