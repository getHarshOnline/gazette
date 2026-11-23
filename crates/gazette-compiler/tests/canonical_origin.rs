//! Explicit syndicated origins keep serving identity, attribution and canonicality separate.

use gazette_compiler::{
    authored,
    corpus::{Input, ParsedCorpus},
    toml::PathSegment,
    workspace,
};
use gazette_core::publication::{Key, content::Disposition};
use serde_json::{Value, json};

fn key(value: &str) -> Key {
    value.parse().expect("fixture identity")
}

fn configuration() -> String {
    let mut text = String::new();
    for name in ["original", "copy", "author", "rights"] {
        text.push_str(&format!(
            "[[principals]]\nid='principal:{name}'\nkind='person'\n"
        ));
    }
    for name in ["original", "copy"] {
        text.push_str(&format!(
            "[[sites]]\nid='site:{name}'\npublisher='principal:{name}'\ndeployable=false\n"
        ));
    }
    text.push_str("[[brands]]\nid='brand:publication'\nname='Explicit publication'\n");
    for name in ["original", "copy"] {
        text.push_str(&format!("[[brands.publications]]\nsite='site:{name}'\npublic_base_url='https://{name}.example/mount/'\n"));
    }
    text
}

fn document(id: &str, site: &str, route: &str, attribution: &str) -> String {
    format!(
        "+++\n[content]\nid='content:{id}'\nsite='site:{site}'\ntitle='{id}'\nlifecycle='published'\n{route}\n{attribution}\nstructure={{kind='page',family={{kind='article'}}}}\n+++\n# Retained {id}\n"
    )
}

fn corpus<'a>(files: &[(&str, &'a str)]) -> ParsedCorpus<'a> {
    ParsedCorpus::parse(
        files.iter().map(|(path, source)| Input {
            path: path.parse().expect("logical path"),
            bytes: source.as_bytes(),
        }),
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("native authored syntax")
}

#[test]
fn canonical_origin_retains_serving_identity_roles_and_exact_declaration() {
    let configuration = workspace::compile(&configuration(), Default::default()).unwrap();
    let original = document(
        "original",
        "original",
        "route={path='/work/'}",
        "attributions=[{principal='principal:author',role='author'}]",
    );
    let copy = document(
        "copy",
        "copy",
        "route={path='/edition/',canonical='content:original'}",
        "attributions=[{principal='principal:rights',role='rights-holder'}]",
    );
    let files = corpus(&[("original.md", &original), ("copy.md", &copy)]);
    let resolved = authored::resolve(&configuration, &files, Default::default()).unwrap();
    let page = resolved
        .project_page(
            &key("content:copy"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    let value = serde_json::to_value(&page).unwrap();
    assert_eq!(value["route"]["url"], "https://copy.example/mount/edition/");
    assert_eq!(
        value["route"]["canonical_url"],
        "https://original.example/mount/work/"
    );
    assert_eq!(value["route"]["output"], "edition/index.html");
    assert_eq!(value["content"]["route"]["canonical"], "content:original");
    assert_eq!(value["site"]["id"], "site:copy");
    assert_eq!(value["site"]["publisher"], "principal:copy");
    assert_eq!(value["site"]["roles"], json!([]));
    assert_eq!(value["attributions"].as_array().unwrap().len(), 1);
    assert_eq!(
        value["attributions"][0]["principal"]["id"],
        "principal:rights"
    );
    let linked = serde_json::to_value(page.json_ld()).unwrap();
    assert_eq!(linked["url"], value["route"]["url"]);
    assert_eq!(
        linked["mainEntityOfPage"]["url"],
        value["route"]["canonical_url"]
    );
    assert_eq!(linked["mainEntityOfPage"]["@type"], "WebPage");
    assert_eq!(linked["mainEntityOfPage"]["identifier"], "content:original");
    for field in ["breadcrumb", "publisher", "author", "copyrightHolder"] {
        assert!(
            linked["mainEntityOfPage"].get(field).is_none(),
            "copy {field} is not evidence about the original"
        );
    }
    let original_page = resolved
        .project_page(
            &key("content:original"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    let original_linked = serde_json::to_value(original_page.json_ld()).unwrap();
    assert_eq!(
        linked["mainEntityOfPage"]["@id"],
        original_linked["mainEntityOfPage"]["@id"]
    );
    assert_eq!(
        linked["gazette:canonical"]["identifier"],
        "content:original"
    );
    assert_eq!(linked["publisher"]["identifier"], "principal:copy");
    assert!(
        linked.get("author").is_none(),
        "canonicality must not infer authorship"
    );
    let graph = serde_json::to_value(
        resolved
            .project_site_graph(&key("site:copy"), Default::default())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        graph["references"]["content:original"]["site"],
        "site:original"
    );
    assert_eq!(
        graph["references"]["content:original"]["canonical_url"],
        linked["mainEntityOfPage"]["url"]
    );
    assert!(graph["nodes"].get("content:original").is_none());
    let origin = resolved
        .origin(
            &key("content:copy"),
            &[
                PathSegment::Key("content".into()),
                PathSegment::Key("route".into()),
                PathSegment::Key("canonical".into()),
            ],
        )
        .expect("exact authored origin token");
    let span = origin.span.unwrap();
    assert_eq!(origin.path.as_str(), "copy.md");
    assert_eq!(&copy[span.start..span.end], "'content:original'");

    let reverse = corpus(&[("copy.md", &copy), ("original.md", &original)]);
    let reverse = authored::resolve(&configuration, &reverse, Default::default()).unwrap();
    let reverse: Value = serde_json::to_value(
        reverse
            .project_site_graph(&key("site:copy"), Default::default())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        graph, reverse,
        "declaration order is not canonical authority"
    );
}

#[test]
fn invalid_origins_refuse_unknown_self_unrouted_chains_and_cycles() {
    let configuration = workspace::compile(&configuration(), Default::default()).unwrap();
    for (original_route, copy_route, code) in [
        (
            "route={path='/work/'}",
            "route={path='/edition/',canonical='content:missing'}",
            "content_canonical_reference",
        ),
        (
            "route={path='/work/'}",
            "route={path='/edition/',canonical='content:copy'}",
            "content_canonical_self",
        ),
        (
            "",
            "route={path='/edition/',canonical='content:original'}",
            "content_canonical_reference",
        ),
        (
            "route={path='/work/',canonical='content:third'}",
            "route={path='/edition/',canonical='content:original'}",
            "content_canonical_chain",
        ),
        (
            "route={path='/work/',canonical='content:copy'}",
            "route={path='/edition/',canonical='content:original'}",
            "content_canonical_chain",
        ),
    ] {
        let original = document("original", "original", original_route, "");
        let copy = document("copy", "copy", copy_route, "");
        let third = document("third", "original", "route={path='/third/'}", "");
        let files = corpus(&[
            ("original.md", &original),
            ("copy.md", &copy),
            ("third.md", &third),
        ]);
        let error = authored::resolve(&configuration, &files, Default::default())
            .expect_err("invalid origin must fail");
        let authored::Error::Structure {
            error: gazette_core::publication::content::Error::Invalid(diagnostics),
            declarations,
        } = error
        else {
            panic!("expected structural {code}, got {error:?}");
        };
        assert!(
            diagnostics.iter().any(|item| item.code == code),
            "missing {code}: {diagnostics:?}"
        );
        assert!(
            !declarations.is_empty(),
            "preserve original declaration evidence"
        );
    }
}

#[test]
fn candidate_cannot_keep_syndicated_copy_after_excluding_its_original() {
    let configuration = workspace::compile(&configuration(), Default::default()).unwrap();
    let original = document("original", "original", "route={path='/work/'}", "");
    let copy = document(
        "copy",
        "copy",
        "route={path='/edition/',canonical='content:original'}",
        "",
    );
    let files = corpus(&[("original.md", &original), ("copy.md", &copy)]);
    let selected = authored::Selection {
        content: [
            (key("content:original"), Disposition::Exclude),
            (key("content:copy"), Disposition::Include),
        ]
        .into(),
        assets: Default::default(),
    };
    let resolved = authored::resolve(&configuration, &files, Default::default()).unwrap();
    assert!(
        matches!(
            resolved.into_selected(&selected),
            Err(authored::SelectionError::Content(_))
        ),
        "selection cannot orphan the canonical relation"
    );
    let mut complete = selected;
    complete
        .content
        .insert(key("content:original"), Disposition::Include);
    let resolved = authored::resolve(&configuration, &files, Default::default()).unwrap();
    let selected = resolved
        .into_selected(&complete)
        .expect("complete canonical dependency selection");
    assert_eq!(
        selected
            .content()
            .route(&key("content:copy"))
            .unwrap()
            .canonical_url,
        "https://original.example/mount/work/"
    );
}
