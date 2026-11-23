//! Site principal selections remain independent, source-bound facts, not page attribution.

use gazette_compiler::{
    authored,
    corpus::{Input, ParsedCorpus},
    toml::PathSegment,
    workspace::{self, DeclarationKind, FieldQuery, OriginError, ParsedWorkspace},
};
use gazette_core::{
    publication::{Key, content::AttributionRole},
    source::{EntryPath, Precision, Sha256Digest},
};
use serde_json::json;

const PRINCIPALS: &str = "[[principals]]\nid='organization:publisher'\nkind='organization'\n\
    [[principals]]\nid='organization:rights'\nkind='organization'\n\
    [[principals]]\nid='person:author'\nkind='person'\n\
    [[principals]]\nid='person:guest'\nkind='person'\n";

fn key(value: &str) -> Key {
    value.parse().expect("exact fixture identity")
}

fn source(fields: &str) -> String {
    format!(
        "{PRINCIPALS}[[sites]]\nid='site:one'\npublisher='organization:publisher'\n\
         deployable=false\n{fields}\n"
    )
}

fn query(field: &str) -> FieldQuery {
    FieldQuery {
        kind: DeclarationKind::Site,
        identity: key("site:one"),
        field: vec![PathSegment::Key(field.into())],
    }
}

#[test]
fn absent_site_principal_fields_have_no_publisher_or_sibling_fallback_or_origin() {
    for publisher in ["organization:publisher", "person:author"] {
        let source = source("").replace(
            "publisher='organization:publisher'",
            &format!("publisher='{publisher}'"),
        );
        let source = format!(
            "{source}[[sites]]\nid='site:configured'\npublisher='organization:publisher'\n\
             rights_holder='organization:rights'\ndefault_author='person:author'\ndeployable=false\n"
        );
        let parsed = ParsedWorkspace::parse(&source, Default::default())
            .expect("one resolved native workspace");
        let site = parsed
            .compilation()
            .workspace
            .site(&key("site:one"))
            .unwrap();
        assert_eq!(site.publisher.as_str(), publisher);
        assert!(site.rights_holder.is_none());
        assert!(site.default_author.is_none());
        let value = serde_json::to_value(site).expect("native site projection");
        for field in ["rights_holder", "default_author"] {
            assert!(value.get(field).is_none());
            assert_eq!(
                parsed.origin(&query(field)),
                Err(OriginError::NoExplicitField)
            );
        }
    }
}

#[test]
fn rights_holders_accept_declared_people_or_organizations_without_joining_other_roles() {
    for rights in ["person:guest", "organization:rights"] {
        let source = source(&format!(
            "rights_holder='{rights}'\ndefault_author='person:author'"
        ));
        let compilation = workspace::compile(&source, Default::default())
            .expect("independent exact principal selections");
        let site = compilation.workspace.site(&key("site:one")).unwrap();
        assert_eq!(site.publisher, key("organization:publisher"));
        assert_eq!(site.rights_holder.as_ref(), Some(&key(rights)));
        assert_eq!(site.default_author.as_ref(), Some(&key("person:author")));
        assert!(site.roles.is_empty());
        assert!(
            !site.deployable,
            "principal selection grants no release eligibility"
        );
    }
}

#[test]
fn unknown_principals_and_organizational_default_authors_preserve_exact_typed_diagnostics() {
    for (field, selected, code) in [
        ("rights_holder", "person:absent", "workspace_reference"),
        ("default_author", "person:absent", "workspace_reference"),
        (
            "default_author",
            "organization:publisher",
            "workspace_default_author_kind",
        ),
        (
            "default_author",
            "organization:rights",
            "workspace_default_author_kind",
        ),
    ] {
        let workspace::Error::Invalid(errors) = workspace::compile(
            &source(&format!("{field}='{selected}'")),
            Default::default(),
        )
        .expect_err("no partial workspace with an unresolved or wrong-kind author") else {
            panic!("native semantic diagnostic expected");
        };
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, code);
        assert_eq!(errors[0].field, format!("sites[\"site:one\"].{field}"));
        assert_eq!(errors[0].key.as_ref(), Some(&key(selected)));
    }
    let workspace::Error::Invalid(errors) = workspace::compile(
        &source("rights_holder='person:missing-rights'\ndefault_author='person:missing-author'"),
        Default::default(),
    )
    .expect_err("both independent reference errors are retained") else {
        panic!("complete native semantic diagnostics expected");
    };
    assert_eq!(errors.len(), 2);
    for (field, selected) in [
        ("rights_holder", "person:missing-rights"),
        ("default_author", "person:missing-author"),
    ] {
        assert!(
            errors
                .iter()
                .any(|error| error.code == "workspace_reference"
                    && error.field == format!("sites[\"site:one\"].{field}")
                    && error.key.as_ref() == Some(&key(selected)))
        );
    }
}

#[test]
fn new_optional_references_do_not_relax_native_scalar_or_unknown_field_validation() {
    for field in ["rights_holder", "default_author"] {
        for value in [
            "42",
            "true",
            "['person:author']",
            "{principal='person:author'}",
            "''",
            "'not-a-key'",
        ] {
            assert!(
                matches!(
                    workspace::compile(&source(&format!("{field}={value}")), Default::default()),
                    Err(workspace::Error::Toml(_))
                ),
                "native key scalar required: {field}={value}"
            );
        }
    }
    for field in ["rights_holders", "default_authors", "unrecognized"] {
        assert!(matches!(
            workspace::compile(
                &source(&format!(
                    "rights_holder='organization:rights'\ndefault_author='person:author'\n{field}=true"
                )),
                Default::default()
            ),
            Err(workspace::Error::Toml(_))
        ));
    }
}

#[test]
fn generated_site_and_workspace_schemas_expose_optional_reference_fields_and_remain_closed() {
    let schema = schemars::schema_for!(workspace::Site);
    let value = schema.as_value();
    assert_eq!(value["additionalProperties"], false);
    for field in ["rights_holder", "default_author"] {
        assert!(value["properties"].get(field).is_some());
        assert!(
            !value["required"]
                .as_array()
                .expect("native required fields")
                .contains(&json!(field))
        );
    }
    let site_validator = jsonschema::validator_for(value).expect("generated native site schema");
    let workspace_schema = gazette_compiler::contracts::workspace();
    let workspace_validator = jsonschema::validator_for(workspace_schema.as_value())
        .expect("generated native workspace schema");
    for fields in [
        "",
        "rights_holder='organization:rights'\ndefault_author='person:author'",
    ] {
        let compilation = workspace::compile(&source(fields), Default::default()).unwrap();
        let site = compilation.workspace.site(&key("site:one")).unwrap();
        let mut value = serde_json::to_value(site).expect("actual compiled declaration");
        assert!(site_validator.is_valid(&value));
        assert!(workspace_validator.is_valid(&json!({"sites":[value.clone()]})));
        value["unknown"] = json!(true);
        assert!(!site_validator.is_valid(&value));
        assert!(!workspace_validator.is_valid(&json!({"sites":[value]})));
    }
}

#[test]
fn selected_site_defaults_do_not_create_or_replace_authored_page_attributions() {
    let compilation = workspace::compile(
        &source("rights_holder='organization:rights'\ndefault_author='person:author'"),
        Default::default(),
    )
    .expect("explicit site selections");
    for attribution in [
        "",
        "attributions=[{principal='person:guest',role='author'}]",
    ] {
        let source = format!(
            "+++\n[content]\nid='article:one'\nsite='site:one'\ntitle='Résumé'\n\
             lifecycle='draft'\n{attribution}\n\
             structure={{kind='page',family={{kind='article'}}}}\n+++\n# Body\n"
        );
        let corpus = ParsedCorpus::parse(
            [Input {
                path: "é/article.md".parse().unwrap(),
                bytes: source.as_bytes(),
            }],
            Default::default(),
            &gazette_adapter_markdown::Comrak,
        )
        .expect("real native content parser");
        let resolved = authored::resolve(&compilation, &corpus, Default::default())
            .expect("same resolved content graph");
        let page = resolved
            .project_page(
                &key("article:one"),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .expect("shared native page projection");
        assert_eq!(
            page.data.attributions.len(),
            usize::from(!attribution.is_empty())
        );
        assert_eq!(
            page.data.content.attributions.len(),
            page.data.attributions.len()
        );
        if let Some(author) = page.data.attributions.first() {
            assert_eq!(author.principal.id, key("person:guest"));
            assert_eq!(author.role, AttributionRole::Author);
        }
        assert!(
            page.data
                .attributions
                .iter()
                .all(|value| value.principal.id != key("person:author")
                    && value.principal.id != key("organization:publisher")
                    && value.principal.id != key("organization:rights"))
        );
    }
}

#[test]
fn included_site_origins_and_navigation_bind_exact_principal_definitions_not_names_or_sources() {
    let root: EntryPath = "gazette.toml".parse().unwrap();
    let principal_path: EntryPath = "config/principals.toml".parse().unwrap();
    let site_path: EntryPath = "publications/site.toml".parse().unwrap();
    let root_source = "includes=['config/principals.toml','publications/site.toml']\r\n";
    let principals = PRINCIPALS.replace(
        "id='person:guest'\nkind='person'",
        "id='person:guest'\nkind='person'\nname='person:author'",
    );
    let principal_source = format!(
        "# 🦀 display-name and source namespace decoys\n{principals}\
         [[sources]]\nid='person:author'\n\
         location={{kind='filesystem',root='/not-opened/person:author'}}\n"
    )
    .replace('\n', "\r\n");
    let site_source = "# 🦀 default_author='person:guest' is only a comment\n\
        [[sites]]\nid='site:one'\npublisher='organization:publisher'\n\
        rights_holder = 'organization:rights'\ndefault_author = \"person\\u003aauthor\"\n\
        deployable=false\n"
        .replace('\n', "\r\n");
    let parsed = workspace::ParsedWorkspaceDocuments::parse(
        &root,
        [
            (&site_path, site_source.as_bytes()),
            (&root, root_source.as_bytes()),
            (&principal_path, principal_source.as_bytes()),
        ],
        Default::default(),
    )
    .expect("exact included document set through the native parser");
    assert_eq!(parsed.documents().len(), 3);
    for (field, identity, scalar) in [
        (
            "rights_holder",
            "organization:rights",
            "'organization:rights'",
        ),
        ("default_author", "person:author", "\"person\\u003aauthor\""),
    ] {
        let origin = parsed
            .origin(&query(field))
            .expect("original included site field");
        assert_eq!(origin.document, site_path);
        assert_eq!(
            origin.source_sha256,
            Sha256Digest::of(site_source.as_bytes())
        );
        assert_eq!(origin.span.slice(&site_source), Some(scalar));
        assert_eq!(
            origin.path,
            [
                PathSegment::Key("sites".into()),
                PathSegment::Index(0),
                PathSegment::Key(field.into())
            ]
        );
        let reference = parsed
            .navigation()
            .reference_at(&site_path, origin.span.start + 1)
            .expect("native principal reference scalar");
        assert_eq!(
            reference.kind,
            gazette_compiler::workspace::WorkspaceSymbolKind::Declaration(
                DeclarationKind::Principal
            )
        );
        assert_eq!(reference.location.location.precision, Precision::Exact);
        assert_eq!(reference.location.location.span, origin.span);
        assert_eq!(reference.location.source_sha256, origin.source_sha256);
        let target = reference
            .symbol
            .expect("declared principal, never source namespace");
        assert_eq!(target.document, principal_path);
        assert_eq!(
            target
                .symbol
                .declaration()
                .expect("principal declaration")
                .kind,
            DeclarationKind::Principal
        );
        assert_eq!(target.symbol.name(), key(identity).as_str());
        assert_eq!(target.replacement(), format!("'{identity}'"));
        let association = parsed
            .navigation()
            .at(&site_path, origin.span.start + 1)
            .expect("cross-document exact association");
        assert_eq!(association.definition.document, &principal_path);
        assert_eq!(
            association.definition.source_sha256,
            Sha256Digest::of(principal_source.as_bytes())
        );
        assert_eq!(
            association.definition.location.precision,
            Precision::Construct
        );
        assert_eq!(
            association
                .definition
                .location
                .span
                .slice(&principal_source),
            Some(format!("'{identity}'").as_str())
        );
        assert_eq!(association.references.len(), 1);
        assert_eq!(association.references[0].document, &site_path);
        assert_eq!(association.references[0].location.span, origin.span);
    }
    assert!(
        parsed
            .navigation()
            .reference_at(&site_path, site_source.find("person:guest").unwrap())
            .is_none()
    );
}
