//! Explicit public interfaces bind once to retained site/content owners, without inferred authority.

use gazette_adapter_markdown::Comrak;
use gazette_compiler::{
    authored::{self, Selection, SelectionError},
    corpus::{Input, ParsedCorpus},
    toml::PathSegment,
    workspace::{self, DeclarationKind, FieldQuery, ParsedWorkspace},
};
use gazette_core::{
    publication::{Key, content::Disposition},
    raster,
};
use serde_json::json;
use std::collections::BTreeMap;

fn key(value: &str) -> Key {
    value.parse().unwrap()
}

fn configuration() -> &'static str {
    "[[principals]]\nid='owner:one'\nkind='person'\n\
     [[principals]]\nid='owner:two'\nkind='organization'\n\
     [[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n\
     [[sites]]\nid='site:two'\npublisher='owner:two'\ndeployable=false\n\
     [[sources]]\nid='source:retained'\nlocation={kind='repository',root='/private/metadata-fixture',commit='1111111111111111111111111111111111111111',subdirectory='content'}\n\
     [[brands]]\nid='brand:parent'\nname='Parent publication'\n\
     [[brands.publications]]\nsite='site:one'\npublic_base_url='https://one.test/mount/'\nhome='content:one-home'\nlanguage='EN-gb'\n\
     repository='https://github.com/example/one'\nauthored=[{source='source:retained',target='one'}]\n\
     contacts={correspondence='mail@one.test',rights='rights@one.test',discovery='hi@one.test'}\n\
     social={youtube={handle='@explicit',url='https://www.youtube.com/@explicit',channel_id='UCobserved'},linkedin={url='https://www.linkedin.com/in/explicit/'},x={handle='@explicit',url='https://x.com/explicit'},github={handle='example',url='https://github.com/example'},future-platform={url='https://profiles.test/explicit'}}\n\
     [[brands]]\nid='brand:child'\nname='Independent child publication'\nparent='brand:parent'\n\
     [[brands.publications]]\nsite='site:two'\npublic_base_url='https://two.test/other/'\nhome='content:two-home'\n\
     authored=[{source='source:retained',target='two'}]\ncontacts={rights='rights@two.test'}\n"
}

fn page(id: &str, site: &str, title: &str, route: Option<&str>) -> String {
    let route = route
        .map(|route| format!("route={{path='{route}'}}\n"))
        .unwrap_or_default();
    format!(
        "+++\n[content]\nid='{id}'\nsite='{site}'\ntitle='{title}'\nlifecycle='published'\n{route}structure={{kind='page',family={{kind='article'}}}}\n+++\n# {title}\n\nRetained original prose.\n"
    )
}

fn files() -> Vec<(&'static str, String)> {
    vec![
        (
            "one/home.md",
            page(
                "content:one-home",
                "site:one",
                "Selected home",
                Some("/welcome/"),
            ),
        ),
        (
            "one/article.md",
            page("content:article", "site:one", "Article", Some("/article/")),
        ),
        (
            "two/home.md",
            page("content:two-home", "site:two", "Other home", Some("/home/")),
        ),
    ]
}
fn corpus<'a>(files: &'a [(&str, String)]) -> ParsedCorpus<'a> {
    ParsedCorpus::parse(
        files.iter().map(|(path, bytes)| Input {
            path: path.parse().unwrap(),
            bytes: bytes.as_bytes(),
        }),
        Default::default(),
        &Comrak,
    )
    .unwrap()
}
fn compilation(config: &str) -> workspace::Compilation {
    workspace::compile(config, Default::default()).unwrap()
}

#[test]
fn account_ownership_reaches_json_ld_while_displayed_shared_accounts_do_not() {
    let config = format!(
        "{}\n[[accounts]]\nid='account:owned'\nplatform='github'\nhandle='owned'\nowner='owner:one'\n[[accounts]]\nid='account:shared'\nplatform='x'\nhandle='shared'\n",
        configuration().replace(
            "site='site:two'\npublic_base_url",
            "site='site:two'\naccounts=['account:owned','account:shared']\npublic_base_url"
        )
    );
    let compiled = compilation(&config);
    let mut files = files();
    files[1].1 = files[1].1.replace(
        "lifecycle='published'",
        "lifecycle='published'\nattributions=[{principal='owner:one',role='author'}]",
    );
    let corpus = corpus(&files);
    let resolved = authored::resolve(&compiled, &corpus, Default::default()).unwrap();
    let owned = resolved
        .project_page(&key("content:article"), &raster::unavailable, &Comrak)
        .unwrap();
    let shared = resolved
        .project_page(&key("content:two-home"), &raster::unavailable, &Comrak)
        .unwrap();
    let owned_json = json!(owned.json_ld());
    assert!(owned_json.to_string().contains("https://github.com/owned"));
    assert!(owned_json.to_string().contains("sameAs"));
    let shared_json = json!(shared.json_ld()).to_string();
    assert!(!shared_json.contains("sameAs"));
    assert!(!shared_json.contains("https://x.com/shared"));
    let shared_page = json!(shared);
    assert_eq!(
        shared_page["site"]["metadata"]["social"]["github"]["url"],
        "https://github.com/owned"
    );
    assert_eq!(
        shared_page["site"]["metadata"]["social"]["x"]["url"],
        "https://x.com/shared"
    );
    assert!(shared_page["attributions"].as_array().unwrap().is_empty());
}

#[test]
fn retained_metadata_is_shared_by_page_discovery_selection_inspection_and_native_schema() {
    let compiled = compilation(configuration());
    let files = files();
    let corpus = corpus(&files);
    let resolved = authored::resolve(&compiled, &corpus, Default::default()).unwrap();
    let site = key("site:one");
    let metadata = resolved.publication_metadata(&site).unwrap().unwrap();
    let home = metadata.home.as_ref().unwrap();
    assert_eq!(home.id, &key("content:one-home"));
    assert_eq!(home.title, "Selected home");
    assert_eq!(home.url, "https://one.test/mount/welcome/");
    assert!(std::ptr::eq(
        home.url.as_ptr(),
        resolved.content().route(home.id).unwrap().url.as_ptr()
    ));
    assert_eq!(metadata.language.unwrap().as_str(), "en-GB");
    let expected = serde_json::to_value(&metadata).unwrap();
    assert_eq!(
        expected["contacts"],
        json!({"correspondence":"mail@one.test","rights":"rights@one.test","discovery":"hi@one.test"})
    );
    assert_eq!(expected["social"]["youtube"]["channel_id"], "UCobserved");
    assert_eq!(
        expected["social"]["future-platform"]["url"],
        "https://profiles.test/explicit"
    );
    assert_eq!(
        serde_json::to_value(resolved.content().publication_metadata(&site).unwrap()).unwrap(),
        expected
    );
    let projected = resolved
        .project_page(&key("content:article"), &raster::unavailable, &Comrak)
        .unwrap();
    assert_eq!(
        projected
            .data
            .content
            .language
            .as_ref()
            .map(|language| language.as_str()),
        Some("en-GB"),
        "omitted page language uses the configured publication language"
    );
    assert!(
        projected.data.attributions.is_empty(),
        "a public profile/contact supplies no author"
    );
    let page = serde_json::to_value(&projected).unwrap();
    assert_eq!(page["site"]["metadata"], expected);
    let discovery = resolved.discovery().unwrap();
    assert_eq!(
        serde_json::to_value(projected.discovery_view(&discovery).unwrap()).unwrap()["site"]["metadata"],
        expected
    );
    assert_eq!(
        serde_json::to_value(projected.listing(&discovery).unwrap().search().unwrap()).unwrap()["site"]
            ["metadata"],
        expected
    );
    let pages = resolved.project_site_pages(&site).unwrap();
    assert_eq!(
        serde_json::to_value(pages.metadata().unwrap()).unwrap(),
        expected
    );
    assert_eq!(
        serde_json::to_value(pages.discoverable(&discovery).unwrap().metadata().unwrap()).unwrap(),
        expected
    );
    let schema =
        serde_json::to_value(schemars::schema_for!(authored::PageProjection<'static>)).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    assert!(
        validator.is_valid(&page),
        "{:?}",
        validator.iter_errors(&page).collect::<Vec<_>>()
    );
    let json_ld = serde_json::to_value(projected.json_ld())
        .unwrap()
        .to_string();
    assert!(!json_ld.contains("sameAs"));
    assert!(!json_ld.contains("UCobserved"));
    assert!(!json_ld.contains("mail@one.test"));
    for private in [
        "/private/metadata-fixture",
        "source:retained",
        "1111111111111111111111111111111111111111",
    ] {
        assert!(!page.to_string().contains(private));
    }
}

#[test]
fn explicit_brand_parent_keeps_own_roles_and_contacts_and_native_reference_coordinates() {
    let config = configuration();
    let parsed = ParsedWorkspace::parse(config, Default::default()).unwrap();
    let query = FieldQuery {
        kind: DeclarationKind::Brand,
        identity: key("brand:child"),
        field: vec![PathSegment::Key("parent".into())],
    };
    let origin = parsed.origin(&query).unwrap();
    assert_eq!(origin.span.slice(config).unwrap(), "'brand:parent'");
    let association = parsed.navigation().at(origin.span.start + 1).unwrap();
    assert_eq!(
        association
            .symbol
            .declaration()
            .expect("brand declaration")
            .kind,
        DeclarationKind::Brand
    );
    assert_eq!(association.symbol.name(), key("brand:parent").as_str());
    assert_eq!(association.references.len(), 1);
    let compiled = parsed.compilation();
    let files = files();
    let corpus = corpus(&files);
    let resolved = authored::resolve(compiled, &corpus, Default::default()).unwrap();
    let metadata = resolved
        .publication_metadata(&key("site:two"))
        .unwrap()
        .unwrap();
    assert_eq!(metadata.brand.parent, Some(&key("brand:parent")));
    assert_eq!(metadata.contacts.rights.as_deref(), Some("rights@two.test"));
    assert!(metadata.contacts.correspondence.is_none());
    assert!(metadata.contacts.discovery.is_none());
    assert!(metadata.social.is_empty());
    assert!(metadata.language.is_none());
    let page = resolved
        .project_page(&key("content:two-home"), &raster::unavailable, &Comrak)
        .unwrap();
    assert_eq!(page.data.site.publisher, &key("owner:two"));
    assert!(page.data.site.roles.is_empty());
    assert!(page.data.attributions.is_empty());
    assert!(page.data.content.language.is_none());
}

#[test]
fn home_uses_validated_preview_serving_route_without_mutating_workspace_or_identity() {
    let compiled = compilation(configuration());
    let before = serde_json::to_value(&compiled).unwrap();
    let files = files();
    let corpus = corpus(&files);
    let resolved = authored::resolve(&compiled, &corpus, Default::default())
        .unwrap()
        .with_preview_urls(BTreeMap::from([(
            key("site:one"),
            "http://127.0.0.1:4310/mount/".parse().unwrap(),
        )]))
        .unwrap();
    let metadata = resolved
        .publication_metadata(&key("site:one"))
        .unwrap()
        .unwrap();
    assert_eq!(
        metadata.home.unwrap().url,
        "http://127.0.0.1:4310/mount/welcome/"
    );
    assert_eq!(
        resolved
            .publication_metadata(&key("site:two"))
            .unwrap()
            .unwrap()
            .home
            .unwrap()
            .url,
        "https://two.test/other/home/"
    );
    assert_eq!(serde_json::to_value(&compiled).unwrap(), before);
}

#[test]
fn selected_site_does_not_require_unselected_workspace_home_and_omission_is_not_inferred() {
    let compiled = compilation(configuration());
    let files: Vec<_> = files()
        .into_iter()
        .filter(|(path, _)| path.starts_with("one/"))
        .collect();
    let corpus = corpus(&files);
    let resolved = authored::resolve(&compiled, &corpus, Default::default()).unwrap();
    assert_eq!(
        resolved
            .project_site_pages(&key("site:one"))
            .unwrap()
            .pages()
            .len(),
        2
    );
    let error = resolved.publication_metadata(&key("site:two")).unwrap_err();
    assert_eq!(error.id, key("content:two-home"));
    assert_eq!(
        error.reason,
        "home is absent from the selected content graph"
    );
    assert!(matches!(
        resolved.project_site_pages(&key("site:two")),
        Err(authored::SitePagesError::Metadata(_))
    ));
    let config = configuration()
        .replace("home='content:one-home'\n", "")
        .replace("language='EN-gb'\n", "");
    let omitted = compilation(&config);
    let resolved = authored::resolve(&omitted, &corpus, Default::default()).unwrap();
    let metadata = resolved
        .publication_metadata(&key("site:one"))
        .unwrap()
        .unwrap();
    assert!(metadata.home.is_none());
    assert!(metadata.language.is_none());
}

#[test]
fn missing_foreign_unrouted_and_excluded_selected_homes_are_exact_binding_failures() {
    for (replacement, unrouted, reason) in [
        (
            "content:absent",
            false,
            "home is absent from the selected content graph",
        ),
        (
            "content:two-home",
            false,
            "home belongs to another publication site",
        ),
        (
            "content:one-home",
            true,
            "home has no resolved serving route",
        ),
    ] {
        let config =
            configuration().replace("home='content:one-home'", &format!("home='{replacement}'"));
        let compiled = compilation(&config);
        let mut files = files();
        if unrouted {
            files[0].1 = page("content:one-home", "site:one", "Unrouted home", None);
        }
        let corpus = corpus(&files);
        let authored::Error::PublicationMetadata(error) =
            authored::resolve(&compiled, &corpus, Default::default()).unwrap_err()
        else {
            panic!("exact metadata error");
        };
        assert_eq!(error.code(), "publication_metadata_binding");
        assert_eq!(error.site, key("site:one"));
        assert_eq!(error.id, key(replacement));
        assert_eq!(error.reason, reason);
    }
    let compiled = compilation(configuration());
    let files = files();
    let corpus = corpus(&files);
    let resolved = authored::resolve(&compiled, &corpus, Default::default()).unwrap();
    let selection = Selection {
        content: resolved
            .content()
            .graph()
            .nodes()
            .map(|node| {
                (
                    node.id.clone(),
                    if node.id == key("content:one-home") {
                        Disposition::Exclude
                    } else {
                        Disposition::Include
                    },
                )
            })
            .collect(),
        assets: BTreeMap::new(),
    };
    let SelectionError::PublicationMetadata(error) =
        resolved.into_selected(&selection).unwrap_err()
    else {
        panic!("selected home dependency retained");
    };
    assert_eq!(error.id, key("content:one-home"));
}

#[test]
fn strict_metadata_shapes_native_language_urls_contacts_and_brand_parentage_are_validated() {
    for (from, to, expected) in [
        (
            "parent='brand:parent'",
            "parent='brand:absent'",
            "workspace_reference",
        ),
        (
            "name='Parent publication'",
            "name='Parent publication'\nparent='brand:child'",
            "workspace_brand_parent_cycle",
        ),
        (
            "parent='brand:parent'",
            "parent='brand:child'",
            "workspace_brand_parent_cycle",
        ),
        (
            "mail@one.test",
            "person:someone",
            "workspace_publication_contact",
        ),
        ("rights@one.test", "", "workspace_publication_contact"),
        (
            "https://profiles.test/explicit",
            "javascript:alert(1)",
            "workspace_publication_profile",
        ),
        (
            "https://profiles.test/explicit",
            "https://secret@profiles.test/explicit",
            "workspace_publication_profile",
        ),
    ] {
        let config = configuration().replace(from, to);
        let workspace::Error::Invalid(errors) =
            workspace::compile(&config, Default::default()).unwrap_err()
        else {
            panic!("native semantic error");
        };
        assert!(
            errors.iter().any(|error| error.code == expected),
            "{errors:?}"
        );
    }
    for (from, to) in [
        ("language='EN-gb'", "language='not_a_language'"),
        (
            "channel_id='UCobserved'",
            "channel_id='UCobserved',activate=true",
        ),
        (
            "contacts={rights='rights@two.test'}",
            "contacts={rights='rights@two.test',publisher='owner:one'}",
        ),
    ] {
        assert!(
            workspace::compile(&configuration().replace(from, to), Default::default()).is_err()
        );
    }
    let schema = serde_json::to_value(schemars::schema_for!(workspace::SocialProfile)).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    assert!(
        validator
            .is_valid(&json!({"url":"https://profiles.test/observed","channel_id":"explicit"}))
    );
    assert!(!validator.is_valid(&json!({"url":"https://profiles.test/observed","sameAs":true})));
    let compiled = compilation(configuration());
    let child = compiled
        .workspace
        .brands()
        .find(|brand| brand.id == key("brand:child"))
        .unwrap();
    assert_eq!(
        serde_json::to_value(child).unwrap()["parent"],
        "brand:parent"
    );
}

#[test]
fn rights_holder_metadata_is_independent_and_omission_never_uses_publisher() {
    let source = configuration().replace(
        "publisher='owner:one'\n",
        "publisher='owner:one'\nrights_holder='owner:two'\n",
    );
    let compiled = compilation(&source);
    let files = files();
    let corpus = corpus(&files);
    let resolved = authored::resolve(&compiled, &corpus, Default::default()).unwrap();
    let one = resolved
        .publication_metadata(&key("site:one"))
        .unwrap()
        .unwrap();
    assert_eq!(one.rights_holder.unwrap().id, key("owner:two"));
    assert!(one.render_year.is_none(), "pure compiler reads no clock");
    let two = resolved
        .publication_metadata(&key("site:two"))
        .unwrap()
        .unwrap();
    assert!(
        two.rights_holder.is_none(),
        "publisher does not supply omitted rights"
    );
    let page = resolved
        .project_page(&key("content:article"), &raster::unavailable, &Comrak)
        .unwrap();
    assert_eq!(
        page.data.site.metadata.unwrap().rights_holder.unwrap().id,
        key("owner:two")
    );
}

#[test]
fn render_year_rejects_outside_the_explicit_calendar_range() {
    for year in [1, 1970, 2040, 9999] {
        let value: authored::RenderYear = serde_json::from_value(json!(year)).unwrap();
        assert_eq!(value.get(), year);
    }
    for value in [
        json!(0),
        json!(10000),
        json!(-1),
        json!(2040.5),
        json!("2040"),
    ] {
        assert!(serde_json::from_value::<authored::RenderYear>(value).is_err());
    }
}
