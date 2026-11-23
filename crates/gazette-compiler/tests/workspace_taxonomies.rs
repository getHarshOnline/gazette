//! Taxonomy catalogs resolve through actual TOML and retain original field coordinates.

use gazette_compiler::{
    toml::PathSegment,
    workspace::{self, DeclarationKind, FieldQuery, OriginError, ParsedWorkspace},
};
use gazette_core::publication::Key;

const BASE: &str = "[[principals]]\nid='principal:publisher'\nkind='person'\n\
    [[sites]]\nid='site:one'\npublisher='principal:publisher'\ndeployable=false\n";
const Z: &str = "[[taxonomies]]\nid='taxonomy:z'\nlabel=' Rêve '\nlanguage='EN-us'\n\
    terms=[{id='term:z',label='Last'},{id='term:a',label='First'}]\n";
const A: &str = "[[taxonomies]]\nid='taxonomy:a'\nlabel='Another'\n\
    terms=[{id='term:z',label='Different scoped meaning'}]\n";

fn key(value: &str) -> Key {
    value.parse().expect("literal namespaced key")
}

fn compile(source: &str) -> workspace::Compilation {
    workspace::compile(source, Default::default()).expect("real workspace compiler")
}

fn diagnostics(source: &str) -> Vec<workspace::Diagnostic> {
    match workspace::compile(source, Default::default()).expect_err("invalid catalog") {
        workspace::Error::Invalid(errors) => errors,
        error => panic!("expected semantic catalog diagnostic, got {error}"),
    }
}

#[test]
fn catalogs_and_terms_sort_once_with_exact_scoped_lookup_and_no_language_inference() {
    let first = compile(&format!("{BASE}{Z}{A}"));
    let reordered_z = Z.replace(
        "terms=[{id='term:z',label='Last'},{id='term:a',label='First'}]",
        "terms=[{id='term:a',label='First'},{id='term:z',label='Last'}]",
    );
    let second = compile(&format!("{A}{BASE}{reordered_z}"));
    assert_eq!(first.workspace, second.workspace);
    assert_eq!(
        serde_json::to_vec(&first.workspace).expect("first projection"),
        serde_json::to_vec(&second.workspace).expect("reordered projection")
    );
    assert_ne!(first.source_sha256, second.source_sha256);
    assert_eq!(
        first
            .workspace
            .taxonomies()
            .map(|taxonomy| taxonomy.id.as_str())
            .collect::<Vec<_>>(),
        ["taxonomy:a", "taxonomy:z"]
    );
    let z = first
        .workspace
        .taxonomy(&key("taxonomy:z"))
        .expect("exact taxonomy");
    assert_eq!(z.label, " Rêve ", "labels are not normalized into identity");
    assert_eq!(
        z.terms
            .iter()
            .map(|term| term.id.as_str())
            .collect::<Vec<_>>(),
        ["term:a", "term:z"]
    );
    assert!(std::ptr::eq(
        z.term(&key("term:a")).expect("resolved term"),
        &z.terms[0]
    ));
    assert_eq!(z.term(&key("term:z")).expect("z term").label, "Last");
    let a = first
        .workspace
        .taxonomy(&key("taxonomy:a"))
        .expect("other taxonomy");
    assert_eq!(
        a.term(&key("term:z"))
            .expect("same key scoped differently")
            .label,
        "Different scoped meaning"
    );
    assert!(
        a.language.is_none(),
        "language from a sibling is never inherited"
    );
    assert_eq!(
        serde_json::to_value(&z.language).expect("normalized language"),
        "en-US"
    );
    for missing in ["term:A", "term:First", "taxonomy:z"] {
        assert!(
            z.term(&key(missing)).is_none(),
            "no term fallback: {missing}"
        );
    }
    assert!(first.workspace.taxonomy(&key("taxonomy:Z")).is_none());
    assert!(compile(BASE).workspace.taxonomies().next().is_none());
    let empty = compile(&format!(
        "{BASE}[[taxonomies]]\nid='taxonomy:empty'\nlabel='Empty'\n"
    ));
    let empty = empty
        .workspace
        .taxonomy(&key("taxonomy:empty"))
        .expect("explicit empty catalog");
    assert!(empty.terms.is_empty());
    assert!(empty.term(&key("term:a")).is_none());
}

#[test]
fn duplicate_term_identity_is_composite_but_taxonomy_identity_cannot_change_owner_kind() {
    let duplicate_terms = Z.replace("id='term:a'", "id='term:z'");
    let errors = diagnostics(&format!("{BASE}{duplicate_terms}{A}"));
    assert_eq!(
        errors,
        vec![workspace::Diagnostic {
            code: "workspace_duplicate",
            field: "taxonomies[\"taxonomy:z\"].terms".into(),
            key: Some(key("term:z")),
        }]
    );
    assert_eq!(
        diagnostics(&format!("{BASE}{Z}{Z}")),
        vec![workspace::Diagnostic {
            code: "workspace_duplicate",
            field: "taxonomies".into(),
            key: Some(key("taxonomy:z")),
        }]
    );
    let conflict = Z.replace("id='taxonomy:z'", "id='principal:publisher'");
    assert_eq!(
        diagnostics(&format!("{BASE}{conflict}")),
        vec![workspace::Diagnostic {
            code: "workspace_identity_conflict",
            field: "taxonomies".into(),
            key: Some(key("principal:publisher")),
        }]
    );
    let scoped = Z.replace("id='term:a'", "id='principal:publisher'");
    assert!(
        compile(&format!("{BASE}{scoped}"))
            .workspace
            .taxonomy(&key("taxonomy:z"))
            .expect("taxonomy")
            .term(&key("principal:publisher"))
            .is_some(),
        "a composite term key is not a global principal declaration"
    );
}

#[test]
fn labels_unknown_fields_and_invalid_language_fail_without_a_partial_catalog() {
    for label in ["", "   ", "\t", "line\nbreak", "bad\u{7f}"] {
        let encoded = serde_json::to_string(label)
            .expect("basic string literal")
            .replace('\u{7f}', "\\u007F");
        let invalid_taxonomy = Z.replace("label=' Rêve '", &format!("label={encoded}"));
        let errors = diagnostics(&format!("{BASE}{invalid_taxonomy}"));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "workspace_taxonomy_label");
        assert_eq!(errors[0].key, Some(key("taxonomy:z")));
        let invalid_term = Z.replace("label='First'", &format!("label={encoded}"));
        let errors = diagnostics(&format!("{BASE}{invalid_term}"));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "workspace_taxonomy_term_label");
        assert_eq!(errors[0].field, "taxonomies[\"taxonomy:z\"].terms.label");
        assert_eq!(errors[0].key, Some(key("term:a")));
    }
    for invalid in [
        Z.replace("label=' Rêve '", "label=' Rêve '\nparent='taxonomy:a'"),
        Z.replace("label='First'", "label='First',url='/guessed/'"),
        Z.replace("label='First'", "label='First',language='en'"),
        Z.replace("id='taxonomy:z'", "id='unqualified'"),
        Z.replace("id='term:a'", "id='unqualified'"),
        Z.replace("language='EN-us'", "language='not_a_language'"),
        Z.replace("language='EN-us'", "language=2"),
        Z.replace("label=' Rêve '\n", ""),
        Z.replace("{id='term:a',label='First'}", "{id='term:a'}"),
    ] {
        let source = format!("{BASE}{invalid}");
        assert!(
            matches!(
                workspace::compile(&source, Default::default()),
                Err(workspace::Error::Toml(_))
            ),
            "invalid typed fields must not silently disappear: {invalid}"
        );
        assert!(
            ParsedWorkspace::parse(&source, Default::default()).is_err(),
            "invalid catalog supplies no origin index"
        );
    }
}

#[test]
fn taxonomy_origins_keep_source_order_and_original_language_spelling_after_normalization() {
    let source = format!("# 🦀 Unicode before coordinates\r\n{BASE}{Z}{A}");
    let parsed =
        ParsedWorkspace::parse(&source, Default::default()).expect("retained taxonomy source");
    assert_eq!(parsed.compilation(), &compile(&source));
    for (taxonomy, field, expected, source_index) in [
        (
            "taxonomy:z",
            vec![PathSegment::Key("language".into())],
            "'EN-us'",
            0,
        ),
        (
            "taxonomy:z",
            vec![
                PathSegment::Key("terms".into()),
                PathSegment::Index(0),
                PathSegment::Key("label".into()),
            ],
            "'Last'",
            0,
        ),
        (
            "taxonomy:a",
            vec![
                PathSegment::Key("terms".into()),
                PathSegment::Index(0),
                PathSegment::Key("label".into()),
            ],
            "'Different scoped meaning'",
            1,
        ),
    ] {
        let query = FieldQuery {
            kind: DeclarationKind::Taxonomy,
            identity: key(taxonomy),
            field,
        };
        let origin = parsed
            .origin(&query)
            .expect("original declaration coordinate");
        assert_eq!(&source[origin.span.start..origin.span.end], expected);
        assert_eq!(origin.path[0], PathSegment::Key("taxonomies".into()));
        assert_eq!(origin.path[1], PathSegment::Index(source_index));
        assert_eq!(origin.source_sha256, parsed.compilation().source_sha256);
        assert_eq!(origin.identity, key(taxonomy));
    }
    let absent = FieldQuery {
        kind: DeclarationKind::Taxonomy,
        identity: key("taxonomy:a"),
        field: vec![PathSegment::Key("language".into())],
    };
    assert_eq!(parsed.origin(&absent), Err(OriginError::NoExplicitField));
    let unknown = FieldQuery {
        identity: key("taxonomy:missing"),
        ..absent
    };
    assert_eq!(parsed.origin(&unknown), Err(OriginError::UnknownIdentity));
    assert_eq!(parsed.source(), source);
}

#[test]
fn native_language_default_is_explicit_without_a_taxonomy_or_site_default() {
    let source = format!(
        "{BASE}{Z}\
        [[sources]]\nid='source:native'\nlocation={{kind='repository',root='/private/native',commit='1111111111111111111111111111111111111111',subdirectory='docs'}}\n\
        [[projects]]\nid='project:native'\nlifecycle='active'\nvisibility='public'\navailability='available'\n\
        [[placements]]\nrole='primary'\nsite='site:one'\nproject='project:native'\nrender=true\ndiscoverable=false\n\
        documents={{source='source:native',target='native'}}\n\
        native_markdown={{route_prefix='/docs/',lifecycle='draft',language='HI-in'}}\n"
    );
    let explicit = compile(&source);
    let profile = explicit.workspace.placements()[0]
        .native_markdown
        .as_ref()
        .expect("native profile");
    assert_eq!(
        serde_json::to_value(&profile.language).expect("explicit language"),
        "hi-IN"
    );
    let omitted = compile(&source.replace(",language='HI-in'", ""));
    assert!(
        omitted.workspace.placements()[0]
            .native_markdown
            .as_ref()
            .expect("profile")
            .language
            .is_none()
    );
    assert!(matches!(
        workspace::compile(
            &source.replace("language='HI-in'", "language='hi_IN'"),
            Default::default()
        ),
        Err(workspace::Error::Toml(_))
    ));
}
