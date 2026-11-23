//! Configured display names stay in the shared, scoped principal model.

use gazette_compiler::{
    authored,
    corpus::{Input, ParsedCorpus},
    toml::PathSegment,
    workspace::{self, DeclarationKind, FieldQuery, ParsedWorkspace},
};
use gazette_core::{
    publication::{Key, PrincipalKind},
    source::Sha256Digest,
};
use serde_json::json;

fn key(value: &str) -> Key {
    value.parse().expect("exact fixture key")
}

fn configuration() -> &'static str {
    "[[principals]]\nid='principal:publisher'\nkind='organization'\nname='Publisher, not author'\n\
     [[principals]]\nid='principal:writer'\nkind='person'\nname='  Écrivain & साथी  '\n\
     [[principals]]\nid='principal:rights'\nkind='organization'\nname='Rights collective'\n\
     [[principals]]\nid='principal:unnamed'\nkind='person'\n\
     [[principals]]\nid='principal:unused'\nkind='person'\nname='Unrelated roster canary'\n\
     [[sites]]\nid='site:one'\npublisher='principal:publisher'\ndeployable=false\n"
}

#[test]
fn configured_names_keep_exact_spelling_and_original_unsorted_field_coordinates() {
    let source = configuration().replace('\n', "\r\n");
    let parsed = ParsedWorkspace::parse(&source, Default::default()).expect("named principals");
    let workspace = &parsed.compilation().workspace;
    let writer = workspace
        .principal(&key("principal:writer"))
        .expect("writer");
    assert_eq!(writer.name.as_deref(), Some("  Écrivain & साथी  "));
    assert_eq!(
        serde_json::to_value(writer).expect("same model"),
        json!({
            "id":"principal:writer", "kind":"person", "name":"  Écrivain & साथी  "
        })
    );
    let origin = parsed
        .origin(&FieldQuery {
            kind: DeclarationKind::Principal,
            identity: key("principal:writer"),
            field: vec![PathSegment::Key("name".into())],
        })
        .expect("retained name origin");
    assert_eq!(
        origin.path,
        [
            PathSegment::Key("principals".into()),
            PathSegment::Index(1),
            PathSegment::Key("name".into())
        ]
    );
    assert_eq!(
        &source[origin.span.start..origin.span.end],
        "'  Écrivain & साथी  '"
    );
    assert_eq!(origin.source_sha256, Sha256Digest::of(source.as_bytes()));
    let unnamed = workspace
        .principal(&key("principal:unnamed"))
        .expect("unnamed principal");
    assert_eq!(unnamed.name, None);
    assert!(
        serde_json::to_value(unnamed)
            .expect("no fallback")
            .get("name")
            .is_none()
    );
    assert!(matches!(
        parsed.origin(&FieldQuery {
            kind: DeclarationKind::Principal,
            identity: key("principal:unnamed"),
            field: vec![PathSegment::Key("name".into())],
        }),
        Err(workspace::OriginError::NoExplicitField)
    ));
}

#[test]
fn blank_or_control_names_fail_the_complete_workspace_without_normalizing_them() {
    for name in [
        "",
        " \t ",
        "\u{a0}",
        "Line\nBreak",
        "Line\rBreak",
        "Tab\tName",
        "Nul\0Name",
        "Delete\u{7f}",
    ] {
        let quoted = serde_json::to_string(name)
            .expect("quoted name")
            .replace('\u{7f}', "\\u007F");
        let source = format!(
            "[[principals]]\nid='principal:bad'\nkind='person'\nname={quoted}\n[[sites]]\nid='site:one'\npublisher='principal:bad'\ndeployable=false\n"
        );
        let error = workspace::compile(&source, Default::default()).expect_err("invalid name");
        let workspace::Error::Invalid(errors) = error else {
            panic!("display name is a semantic validation failure: {name:?}")
        };
        assert_eq!(
            errors,
            [workspace::Diagnostic {
                code: "workspace_principal_name",
                field: "principals.name".into(),
                key: Some(key("principal:bad")),
            }]
        );
    }
    for field in [
        "name=42",
        "name=true",
        "name=['Name']",
        "display_name='Alias'",
    ] {
        let source = format!(
            "[[principals]]\nid='principal:bad'\nkind='person'\n{field}\n[[sites]]\nid='site:one'\npublisher='principal:bad'\ndeployable=false\n"
        );
        assert!(
            matches!(
                workspace::compile(&source, Default::default()),
                Err(workspace::Error::Toml(_))
            ),
            "accepted {field}"
        );
    }
}

#[test]
fn same_display_name_does_not_merge_distinct_principals_or_determine_their_kind() {
    let source = "[[principals]]\nid='principal:person'\nkind='person'\nname='Shared label'\n[[principals]]\nid='principal:organization'\nkind='organization'\nname='Shared label'\n[[sites]]\nid='site:one'\npublisher='principal:organization'\ndeployable=false\n";
    let compiled =
        workspace::compile(source, Default::default()).expect("labels are not identities");
    assert_eq!(compiled.workspace.principals().len(), 2);
    let person = compiled
        .workspace
        .principal(&key("principal:person"))
        .expect("person");
    let organization = compiled
        .workspace
        .principal(&key("principal:organization"))
        .expect("organization");
    assert_eq!(person.name, organization.name);
    assert_ne!(person.id, organization.id);
    assert_eq!(person.kind, PrincipalKind::Person);
    assert_eq!(organization.kind, PrincipalKind::Organization);
}

#[test]
fn named_attributions_share_workspace_values_without_roster_or_publisher_author_fallback() {
    let compilation =
        workspace::compile(configuration(), Default::default()).expect("shared workspace");
    let source = "+++\n[content]\nid='content:one'\nsite='site:one'\ntitle='Body'\nlifecycle='draft'\n\
        attributions=[{principal='principal:writer',role='author'},{principal='principal:rights',role='rights-holder'},{principal='principal:unnamed',role='author'}]\n\
        structure={kind='page',family={kind='article'}}\n+++\nBody.\n";
    let corpus = ParsedCorpus::parse(
        [Input {
            path: "page.md".parse().expect("path"),
            bytes: source.as_bytes(),
        }],
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("retained authored syntax");
    let resolved = authored::resolve(&compilation, &corpus, Default::default())
        .expect("resolved attributions");
    let page = resolved
        .project_page(
            &key("content:one"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("shared page");
    for view in &page.data.attributions {
        assert!(std::ptr::eq(
            view.principal,
            compilation
                .workspace
                .principal(&view.principal.id)
                .expect("sole principal owner")
        ));
    }
    let page_json = serde_json::to_value(&page).expect("page model");
    let search = serde_json::to_value(page.search()).expect("search model");
    assert_eq!(page_json["attributions"], search["attributions"]);
    assert_eq!(
        page_json["attributions"][0]["principal"]["name"],
        "  Écrivain & साथी  "
    );
    assert!(
        page_json["attributions"][2]["principal"]
            .get("name")
            .is_none()
    );
    let graph = serde_json::to_value(
        resolved
            .project_site_graph(&key("site:one"), Default::default())
            .expect("scoped graph"),
    )
    .expect("graph model");
    assert_eq!(
        graph["principals"]["principal:writer"],
        page_json["attributions"][0]["principal"]
    );
    assert!(graph["principals"].get("principal:unused").is_none());
    let linked = serde_json::to_value(page.json_ld()).expect("JSON-LD");
    assert_eq!(
        linked["author"]
            .as_array()
            .expect("only declared authors")
            .len(),
        2
    );
    assert_eq!(
        linked["copyrightHolder"]
            .as_array()
            .expect("only declared rights holder")
            .len(),
        1
    );
    assert_eq!(linked["author"][0]["identifier"], "principal:writer");
    assert_eq!(linked["author"][0]["name"], "  Écrivain & साथी  ");
    assert_eq!(linked["author"][0]["@type"], "Person");
    assert_eq!(linked["author"][1]["identifier"], "principal:unnamed");
    assert!(linked["author"][1].get("name").is_none());
    assert_eq!(linked["copyrightHolder"][0]["name"], "Rights collective");
    assert_eq!(linked["copyrightHolder"][0]["@type"], "Organization");
    assert_eq!(
        linked["gazette:attributions"]["@list"][0]["gazette:principal"],
        linked["author"][0]
    );
    assert_eq!(linked["publisher"]["identifier"], "principal:publisher");
    assert_eq!(
        linked["publisher"]
            .as_object()
            .expect("bare publisher reference")
            .len(),
        2
    );
    assert!(!linked.to_string().contains("Publisher, not author"));
    let public = serde_json::to_value(page.json_ld().public()).unwrap();
    assert_eq!(public["publisher"]["identifier"], "principal:publisher");
    assert_eq!(public["publisher"]["@type"], "Organization");
    assert_eq!(public["publisher"]["name"], "Publisher, not author");
    assert!(public["publisher"].get("url").is_none());
    assert_eq!(public["author"], linked["author"]);
    assert_eq!(public["copyrightHolder"], linked["copyrightHolder"]);
    for value in [&page_json, &search, &graph, &linked, &public] {
        assert!(!value.to_string().contains("Unrelated roster canary"));
    }
}
