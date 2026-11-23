//! Direct role references are bound once, without identity or permission inference.

use gazette_compiler::{
    authored,
    corpus::{Input, ParsedCorpus},
    toml::PathSegment,
    workspace,
};
use gazette_core::{
    publication::{Key, content},
    source::Sha256Digest,
};

fn key(value: &str) -> Key {
    value.parse().expect("fixture key")
}

fn compilation() -> workspace::Compilation {
    workspace::compile(
        "[[principals]]\nid='principal:publisher'\nkind='organization'\n\
         [[principals]]\nid='principal:writer'\nkind='person'\n\
         [[principals]]\nid='principal:rights'\nkind='organization'\n\
         [[principals]]\nid='principal:unused'\nkind='person'\n\
         [[sites]]\nid='site:one'\npublisher='principal:publisher'\ndeployable=false\n\
         [[projects]]\nid='project:one'\nlifecycle='active'\nvisibility='public'\navailability='available'\n",
        Default::default(),
    )
    .expect("workspace")
}

fn source(attributions: &str) -> String {
    format!(
        "+++\n[content]\nid='article:one'\nsite='site:one'\ntitle='Résumé'\n\
         lifecycle='draft'\n{attributions}\nstructure={{kind='page',family={{kind='article'}}}}\n+++\n# Body\n"
    )
}

fn corpus(source: &str) -> ParsedCorpus<'_> {
    ParsedCorpus::parse(
        [Input {
            path: "é/article.md".parse().expect("path"),
            bytes: source.as_bytes(),
        }],
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("syntax")
}

#[test]
fn exact_role_bindings_borrow_principals_and_keep_authored_order_and_origins() {
    let source = source(
        "attributions=[{principal='principal:writer',role='author'},\
         {principal='principal:rights',role='rights-holder'},\
         {principal='principal:writer',role='rights-holder'}]",
    )
    .replace('\n', "\r\n");
    let corpus = corpus(&source);
    let workspace = compilation();
    let resolved = authored::resolve(&workspace, &corpus, Default::default()).expect("bound roles");
    let page = resolved
        .project_page(
            &key("article:one"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("projection");
    assert_eq!(page.data.attributions.len(), 3);
    for (view, declaration) in page
        .data
        .attributions
        .iter()
        .zip(&page.data.content.attributions)
    {
        assert_eq!(view.role, declaration.role);
        assert_eq!(view.principal.id, declaration.principal);
        assert!(std::ptr::eq(
            view.principal,
            workspace
                .workspace
                .principal(&declaration.principal)
                .expect("exact principal")
        ));
    }
    assert_eq!(page.data.site.publisher, &key("principal:publisher"));
    let value = serde_json::to_value(&page).expect("machine view");
    assert_eq!(value["attributions"][0]["role"], "author");
    assert_eq!(
        value["attributions"][1]["principal"]["id"],
        "principal:rights"
    );
    assert_eq!(value["attributions"][2]["role"], "rights-holder");
    assert!(!value.to_string().contains("principal:unused"));
    let origin = resolved
        .origin(
            &key("article:one"),
            &[
                PathSegment::Key("content".into()),
                PathSegment::Key("attributions".into()),
                PathSegment::Index(1),
                PathSegment::Key("principal".into()),
            ],
        )
        .expect("exact declaration origin");
    let span = origin.span.expect("explicit source span");
    assert_eq!(&source[span.start..span.end], "'principal:rights'");
    assert_eq!(origin.source_sha256, Sha256Digest::of(source.as_bytes()));
}

#[test]
fn unknown_or_wrong_namespace_principals_fail_the_whole_binding_with_source_evidence() {
    let source = source(
        "attributions=[{principal='site:one',role='author'},\
         {principal='principal:missing',role='rights-holder'},\
         {principal='project:one',role='author'}]",
    );
    let corpus = corpus(&source);
    let workspace = compilation();
    let error = authored::resolve(&workspace, &corpus, Default::default())
        .expect_err("no guessed identity");
    let authored::Error::Structure {
        error: content::Error::Invalid(errors),
        declarations,
    } = error
    else {
        panic!("expected bound graph failure with source evidence")
    };
    assert_eq!(errors.len(), 3);
    assert!(
        errors
            .iter()
            .all(|error| error.code == "content_principal_reference")
    );
    assert_eq!(
        errors
            .iter()
            .map(|error| error.target.as_ref().expect("endpoint").as_str())
            .collect::<Vec<_>>(),
        ["principal:missing", "project:one", "site:one"]
    );
    assert_eq!(
        declarations[&key("article:one")][0].source_sha256,
        Sha256Digest::of(source.as_bytes())
    );
    assert!(
        workspace
            .workspace
            .principal(&key("principal:missing"))
            .is_none()
    );
    assert_eq!(workspace.workspace.principals().len(), 4);
}

#[test]
fn omitted_roles_have_no_publisher_fallback_and_invalid_role_claims_are_rejected() {
    let source = source("");
    let corpus = corpus(&source);
    let workspace = compilation();
    let resolved =
        authored::resolve(&workspace, &corpus, Default::default()).expect("no role declaration");
    let view = resolved
        .project_page(
            &key("article:one"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("local view");
    assert!(view.data.attributions.is_empty() && view.data.content.attributions.is_empty());
    assert!(
        resolved
            .origin(
                &key("article:one"),
                &[
                    PathSegment::Key("content".into()),
                    PathSegment::Key("attributions".into())
                ]
            )
            .is_none()
    );
    for fields in [
        "principal='principal:writer',role='publisher'",
        "principal='principal:writer'",
        "role='author'",
        "principal='principal:writer',role='author',consent=true",
    ] {
        let source = self::source(&format!("attributions=[{{{fields}}}]"));
        let corpus = self::corpus(&source);
        assert!(matches!(
            authored::resolve(&workspace, &corpus, Default::default()),
            Err(authored::Error::Files(_))
        ));
    }
}

#[test]
fn publication_defaults_apply_to_published_byline_and_json_ld_without_crediting_drafts() {
    let workspace = workspace::compile(
        "[[principals]]\nid='principal:publisher'\nkind='organization'\n\
         [[principals]]\nid='principal:writer'\nkind='person'\nname='Writer'\n\
         [[principals]]\nid='principal:guest'\nkind='person'\nname='Guest'\n\
         [[sites]]\nid='site:one'\npublisher='principal:publisher'\ndeployable=false\n\
         default_author='principal:writer'\n\
         [[brands]]\nid='brand:one'\nname='Publication'\n\
         [[brands.publications]]\nsite='site:one'\nlanguage='en'\n",
        Default::default(),
    )
    .expect("publication defaults");
    for (state, fields, expected_author, language) in [
        ("draft", "", None, "en"),
        ("archived", "", None, "en"),
        ("published", "", Some("Writer"), "en"),
        (
            "published",
            "language='hi'\nattributions=[{principal='principal:guest',role='author'}]",
            Some("Guest"),
            "hi",
        ),
        (
            "published",
            "attributions=[{principal='principal:guest',role='rights-holder'}]",
            None,
            "en",
        ),
    ] {
        let source = source(fields).replace("lifecycle='draft'", &format!("lifecycle='{state}'"));
        let corpus = corpus(&source);
        let resolved =
            authored::resolve(&workspace, &corpus, Default::default()).expect("resolved defaults");
        let page = resolved
            .project_page(
                &key("article:one"),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .expect("page");
        let byline = page
            .data
            .attributions
            .iter()
            .find(|credit| credit.role == content::AttributionRole::Author)
            .and_then(|credit| credit.principal.name.as_deref());
        assert_eq!(byline, expected_author, "{state}: {fields}");
        let json = serde_json::to_value(page.json_ld()).expect("JSON-LD");
        assert_eq!(json["author"][0]["name"].as_str(), expected_author);
        assert_eq!(
            serde_json::to_value(&page.data.content.language).unwrap(),
            language
        );
        if fields.is_empty() {
            assert!(
                resolved
                    .origin(
                        &key("article:one"),
                        &[
                            PathSegment::Key("content".into()),
                            PathSegment::Key("attributions".into()),
                        ]
                    )
                    .is_none(),
                "inherited credit has no invented source span"
            );
        }
    }
}
