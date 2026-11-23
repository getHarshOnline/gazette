//! Publication defaults retain real overrides and the declared publisher boundary.

use super::*;

const DEFAULTS: &str = r#"
[defaults.publication]
language = 'en'
contacts = { correspondence = 'mail@example.org', rights = 'rights@example.org' }
social = { github = { url = 'https://github.com/example', handle = 'example' } }
machine = { sitemap = true, json_ld = true }

[[publishers]]
id = 'owner:publisher'
[publishers.publication]
contacts = { rights = 'publisher@example.org' }
social = { github = { handle = 'publisher', url = 'https://github.com/publisher' } }
machine = { json_ld = false }
"#;

const PUBLICATION: &str = r#"
[[brands]]
id = 'brand:one'
name = 'Example'
[[brands.publications]]
site = 'site:one'
public_base_url = 'https://example.org/'
contacts = { correspondence = 'site@example.org' }
social = { github = { handle = 'site' } }
machine = { sitemap = false }
"#;

#[test]
fn flat_site_uses_the_same_publication_defaults_and_original_source_origins() {
    let flat = r#"
[[sites]]
id='site:one'
name='Example'
publisher='owner:publisher'
deployable=false
domain='example.org'
port=8080
contacts={correspondence='site@example.org'}
social={github={handle='site'}}
machine={sitemap=false}
"#;
    let source = [DEFAULTS, PRINCIPAL, flat].concat();
    let parsed = workspace::ParsedWorkspace::parse(&source, Default::default()).unwrap();
    let publisher_offset = source.find("id = 'owner:publisher'").unwrap() + "id = '".len();
    let publisher = parsed.navigation().reference_at(publisher_offset).unwrap();
    assert_eq!(
        publisher.symbol.unwrap().declaration().unwrap().kind,
        workspace::DeclarationKind::Principal
    );
    let site = "site:one".parse().unwrap();
    let publication = parsed
        .compilation()
        .workspace
        .publication_for(&site)
        .unwrap();
    assert_eq!(
        publication.public_base_url.as_ref().unwrap().as_str(),
        "https://example.org/"
    );
    assert_eq!(publication.local_port.unwrap().get(), 8080);
    assert_eq!(
        publication.contacts.rights.as_deref(),
        Some("publisher@example.org")
    );
    let explicit = PUBLICATION.replace("brand:one", "brand:site:one").replace(
        "public_base_url = 'https://example.org/'",
        "public_base_url = 'https://example.org/'\nlocal_port = 8080",
    );
    let separate = compile(&[DEFAULTS, PRINCIPAL, SITE, &explicit].concat()).unwrap();
    assert_eq!(parsed.compilation().workspace, separate.workspace);
    for (field, literal) in [("public_base_url", "'example.org'"), ("local_port", "8080")] {
        let origin = parsed
            .publication_origin(&workspace::PublicationOriginQuery {
                site: site.clone(),
                path: vec![toml::PathSegment::Key(field.into())],
            })
            .unwrap();
        assert_eq!(
            origin.scope,
            workspace::PublicationScope::Site(site.clone())
        );
        assert_eq!(&source[origin.span.start..origin.span.end], literal);
    }
    for suffix in ["public_base_url='https://elsewhere.test/'", "unknown=true"] {
        assert!(compile(&format!("{source}\n{suffix}\n")).is_err());
    }
    let source = format!("{source}\nauthored=[{{source='source:one',target='one'}}]\n{SOURCE}");
    let parsed = workspace::ParsedWorkspace::parse(&source, Default::default()).unwrap();
    let offset = source.find("source='source:one'").unwrap() + "source='".len();
    let reference = parsed
        .navigation()
        .reference_at(offset)
        .expect("flat source reference");
    assert_eq!(
        reference
            .symbol
            .unwrap()
            .declaration()
            .unwrap()
            .identity
            .as_str(),
        "source:one"
    );
}

#[test]
fn inherits_each_field_and_preserves_explicit_false_and_partial_overrides() {
    let compiled = compile(&[DEFAULTS, PRINCIPAL, SITE, PUBLICATION].concat()).unwrap();
    let site = "site:one".parse().unwrap();
    let publication = compiled.workspace.publication_for(&site).unwrap();
    assert_eq!(serde_json::to_value(&publication.language).unwrap(), "en");
    assert_eq!(
        publication.contacts.rights.as_deref(),
        Some("publisher@example.org")
    );
    assert_eq!(
        publication.contacts.correspondence.as_deref(),
        Some("site@example.org")
    );
    assert_eq!(
        publication.social["github"].url,
        "https://github.com/publisher"
    );
    assert_eq!(publication.social["github"].handle.as_deref(), Some("site"));
    assert!(!publication.machine.sitemap);
    assert!(!publication.machine.json_ld);

    let independent = SITE.replace("owner:publisher", "owner:other");
    let other = PRINCIPAL.replace("owner:publisher", "owner:other");
    let compiled =
        compile(&[DEFAULTS, PRINCIPAL, &other, &independent, PUBLICATION].concat()).unwrap();
    let publication = compiled.workspace.publication_for(&site).unwrap();
    assert_eq!(
        publication.contacts.rights.as_deref(),
        Some("rights@example.org")
    );
    assert_eq!(
        publication.social["github"].url,
        "https://github.com/example"
    );
    assert!(publication.machine.json_ld);
}

#[test]
fn resolves_defaults_across_documents_without_depending_on_file_order() {
    let root: EntryPath = "gazette.toml".parse().unwrap();
    let defaults: EntryPath = "defaults.toml".parse().unwrap();
    let root_source = ["includes=['defaults.toml']\n", PRINCIPAL, SITE, PUBLICATION].concat();
    assert_eq!(
        workspace::included_documents(&root_source, Default::default()).unwrap(),
        std::slice::from_ref(&defaults)
    );
    let compiled = workspace::compile_documents(
        &root,
        [
            (&root, root_source.as_bytes()),
            (&defaults, DEFAULTS.as_bytes()),
        ],
        Default::default(),
    )
    .unwrap();
    let expected = compile(&[DEFAULTS, PRINCIPAL, SITE, PUBLICATION].concat()).unwrap();
    assert_eq!(compiled.compilation.workspace, expected.workspace);
}

#[test]
fn rejects_invalid_unused_defaults_and_unknown_publishers() {
    for invalid in [
        "[defaults.publication]\nlanguage = 12\n",
        "[defaults.publication.machine]\nsitemp = true\n",
        "[defaults.publication]\nsite = 'site:one'\n",
        "[defaults.publication.social.github]\nhandle = 12\n",
        "[[publishers]]\nid='owner:missing'\n",
        "[[publishers]]\nid='owner:publisher'\n[[publishers]]\nid='owner:publisher'\n",
    ] {
        assert!(
            compile(&[invalid, PRINCIPAL, SITE].concat()).is_err(),
            "{invalid}"
        );
    }
}

#[test]
fn sorted_feed_origins_point_to_the_matching_authored_record() {
    let source = format!(
        "{PRINCIPAL}{SITE}\n[[brands]]\nid='brand:one'\nname='Example'\n[[brands.publications]]\nsite='site:one'\nmachine={{feeds={{identity_base='https://example.org/',channels=[{{id='feed:z',channel='content:home',path='/z.xml',format='atom',scope={{kind='site'}}}},{{id='feed:a',channel='content:home',path='/a.xml',format='atom',scope={{kind='site'}}}}]}}}}\n"
    );
    let parsed = workspace::ParsedWorkspace::parse(&source, Default::default()).unwrap();
    let origin = parsed
        .publication_origin(&workspace::PublicationOriginQuery {
            site: "site:one".parse().unwrap(),
            path: vec![
                toml::PathSegment::Key("machine".into()),
                toml::PathSegment::Key("feeds".into()),
                toml::PathSegment::Key("channels".into()),
                toml::PathSegment::Index(0),
                toml::PathSegment::Key("path".into()),
            ],
        })
        .unwrap();
    assert_eq!(origin.value, "/a.xml");
    assert_eq!(&source[origin.span.start..origin.span.end], "'/a.xml'");
    assert_eq!(origin.path[7], toml::PathSegment::Index(1));
}

#[test]
fn reports_the_winning_document_and_exact_field_for_each_inheritance_level() {
    let root: EntryPath = "gazette.toml".parse().unwrap();
    let defaults: EntryPath = "defaults.toml".parse().unwrap();
    let root_source = ["includes=['defaults.toml']\n", PRINCIPAL, SITE, PUBLICATION].concat();
    let parsed = workspace::ParsedWorkspaceDocuments::parse(
        &root,
        [
            (&root, root_source.as_bytes()),
            (&defaults, DEFAULTS.as_bytes()),
        ],
        Default::default(),
    )
    .unwrap();
    for (field, scope, document, literal) in [
        (
            vec!["language"],
            workspace::PublicationScope::Default,
            &defaults,
            "'en'",
        ),
        (
            vec!["contacts", "rights"],
            workspace::PublicationScope::Publisher("owner:publisher".parse().unwrap()),
            &defaults,
            "'publisher@example.org'",
        ),
        (
            vec!["contacts", "correspondence"],
            workspace::PublicationScope::Site("site:one".parse().unwrap()),
            &root,
            "'site@example.org'",
        ),
        (
            vec!["social", "github", "url"],
            workspace::PublicationScope::Publisher("owner:publisher".parse().unwrap()),
            &defaults,
            "'https://github.com/publisher'",
        ),
    ] {
        let origin = parsed
            .publication_origin(&workspace::PublicationOriginQuery {
                site: "site:one".parse().unwrap(),
                path: field
                    .into_iter()
                    .map(|name| toml::PathSegment::Key(name.into()))
                    .collect(),
            })
            .unwrap();
        assert_eq!(origin.scope, scope);
        assert_eq!(origin.document.as_ref(), Some(document));
        let source = if document == &root {
            &root_source
        } else {
            DEFAULTS
        };
        assert_eq!(&source[origin.span.start..origin.span.end], literal);
    }
}

const PARENT_BRAND: &str = "[[brands]]\nid='brand:parent'\nname='Parent'\n";
const INLINE_BRAND_SITE: &str = r#"
[[sites]]
id='site:one'
name='Example'
brand={id='brand:one',parent='brand:parent'}
publisher='owner:publisher'
deployable=false
domain='example.org'
contacts={correspondence='site@example.org'}
social={github={handle='site'}}
machine={sitemap=false}
"#;

#[test]
fn inline_brand_preserves_identity_parent_settings_and_original_fields() {
    let source = [DEFAULTS, PRINCIPAL, PARENT_BRAND, INLINE_BRAND_SITE].concat();
    let explicit = PUBLICATION.replace(
        "name = 'Example'",
        "name = 'Example'\nparent='brand:parent'",
    );
    let expected = compile(&[DEFAULTS, PRINCIPAL, PARENT_BRAND, SITE, &explicit].concat()).unwrap();
    let parsed = workspace::ParsedWorkspace::parse(&source, Default::default()).unwrap();
    assert_eq!(parsed.compilation().workspace, expected.workspace);
    for (field, literal) in [
        ("id", "'brand:one'"),
        ("parent", "'brand:parent'"),
        ("name", "'Example'"),
    ] {
        let origin = parsed
            .origin(&workspace::FieldQuery {
                kind: workspace::DeclarationKind::Brand,
                identity: "brand:one".parse().unwrap(),
                field: vec![toml::PathSegment::Key(field.into())],
            })
            .unwrap();
        assert_eq!(&source[origin.span.start..origin.span.end], literal);
        assert_eq!(origin.path[0], toml::PathSegment::Key("sites".into()));
    }
    let parent = source.find("parent='brand:parent'").unwrap() + "parent='".len();
    let association = parsed.navigation().at(parent).unwrap();
    assert_eq!(association.symbol.name(), "brand:parent");
    assert_eq!(
        &source[association.definition.span.start..association.definition.span.end],
        "'brand:parent'"
    );
    let child = source.find("id='brand:one'").unwrap() + "id='".len();
    assert_eq!(
        parsed.navigation().at(child).unwrap().symbol.name(),
        "brand:one"
    );
}

#[test]
fn inline_brand_parent_navigation_and_origins_cross_configuration_documents() {
    let root: EntryPath = "gazette.toml".parse().unwrap();
    let parent: EntryPath = "parent.toml".parse().unwrap();
    let child: EntryPath = "child.toml".parse().unwrap();
    let root_source = format!("includes=['parent.toml','child.toml']\n{PRINCIPAL}{DEFAULTS}");
    // The parent is itself an inline declaration in another site.
    let parent_source = "[[sites]]\nid='site:parent'\nname='Parent'\nbrand={id='brand:parent'}\npublisher='owner:publisher'\ndeployable=false\n";
    let parsed = workspace::ParsedWorkspaceDocuments::parse(
        &root,
        [
            (&root, root_source.as_bytes()),
            (&parent, parent_source.as_bytes()),
            (&child, INLINE_BRAND_SITE.as_bytes()),
        ],
        Default::default(),
    )
    .unwrap();
    for (field, literal) in [
        ("id", "'brand:one'"),
        ("parent", "'brand:parent'"),
        ("name", "'Example'"),
    ] {
        let origin = parsed
            .origin(&workspace::FieldQuery {
                kind: workspace::DeclarationKind::Brand,
                identity: "brand:one".parse().unwrap(),
                field: vec![toml::PathSegment::Key(field.into())],
            })
            .unwrap();
        assert_eq!(origin.document, child);
        assert_eq!(
            &INLINE_BRAND_SITE[origin.span.start..origin.span.end],
            literal
        );
    }
    let offset = INLINE_BRAND_SITE.find("parent='brand:parent'").unwrap() + "parent='".len();
    let association = parsed.navigation().at(&child, offset).unwrap();
    assert_eq!(association.symbol.symbol.name(), "brand:parent");
    assert_eq!(association.definition.document, &parent);
    assert_eq!(association.references.len(), 1);
    assert_eq!(association.references[0].document, &child);
    let publication = parsed
        .publication_origin(&workspace::PublicationOriginQuery {
            site: "site:one".parse().unwrap(),
            path: vec![toml::PathSegment::Key("public_base_url".into())],
        })
        .unwrap();
    assert_eq!(publication.document.as_ref(), Some(&child));
    assert_eq!(
        &INLINE_BRAND_SITE[publication.span.start..publication.span.end],
        "'example.org'"
    );
}

#[test]
fn inline_brand_keeps_schema_closed_and_rejects_conflicting_relationships() {
    let schema = gazette_compiler::contracts::workspace();
    let validator = jsonschema::validator_for(schema.as_value()).unwrap();
    let valid = serde_json::json!({"sites":[{
        "id":"site:one", "publisher":"owner:publisher", "deployable":false,
        "name":"Example", "brand":{"id":"brand:one","parent":"brand:parent"}
    }]});
    assert!(validator.is_valid(&valid));
    for brand in [
        serde_json::json!({"parent":"brand:parent"}),
        serde_json::json!({"id":"brand:one","unknown":true}),
    ] {
        let mut invalid = valid.clone();
        invalid["sites"][0]["brand"] = brand;
        assert!(!validator.is_valid(&invalid));
    }
    let mut nameless = valid.clone();
    nameless["sites"][0].as_object_mut().unwrap().remove("name");
    assert!(!validator.is_valid(&nameless));
    for site in [
        INLINE_BRAND_SITE.replace("name='Example'\n", ""),
        INLINE_BRAND_SITE.replace("id='brand:one',", ""),
        INLINE_BRAND_SITE.replace("parent='brand:parent'", "unknown=true"),
        INLINE_BRAND_SITE.replace("parent='brand:parent'", "parent='brand:missing'"),
        INLINE_BRAND_SITE.replace("parent='brand:parent'", "parent='brand:one'"),
        INLINE_BRAND_SITE.replace("id='brand:one'", "id='brand:parent'"),
    ] {
        assert!(
            compile(&[DEFAULTS, PRINCIPAL, PARENT_BRAND, &site].concat()).is_err(),
            "{site}"
        );
    }
}
