//! Controlled domain data binds to the exact compiled workspace, not Theme components.

use gazette_compiler::{
    authored,
    corpus::{Input, ParsedCorpus},
    workspace,
};
use gazette_core::{
    publication::{
        Key,
        content::{ContentNode, Declaration, Error, Limits},
    },
    source::Sha256Digest,
};
use serde_json::{Value, json};

const BASE: &str = "[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n";
const COUNT: &str = "[[extensions]]\nid='schema:count'\nschema={'$schema'='https://json-schema.org/draft/2020-12/schema',type='object',required=['count'],additionalProperties=false,properties={count={type='integer',minimum=0,maximum=5}}}\n";
const LABEL: &str = "[[extensions]]\nid='schema:label'\nschema={'$schema'='https://json-schema.org/draft/2020-12/schema',type='object',required=['label'],additionalProperties=false,properties={label={'$ref'='#/$defs/label'}},'$defs'={label={type='string',minLength=1}}}\n";

fn key(value: &str) -> Key {
    value.parse().expect("fixture key")
}

fn compile(schemas: &str) -> workspace::Compilation {
    workspace::compile(&format!("{BASE}{schemas}"), Default::default())
        .expect("real workspace schema compilation")
}

fn document(id: &str, schema: &str, data: &str) -> String {
    format!(
        "+++\n[content]\nid='{id}'\nsite='site:one'\ntitle='Controlled data'\nlifecycle='draft'\nstructure={{kind='page',family={{kind='extension',schema='{schema}',data={data}}}}}\n+++\n# Retained authored body\n"
    )
}

fn corpus<'a>(documents: &[(&str, &'a str)]) -> ParsedCorpus<'a> {
    ParsedCorpus::parse(
        documents.iter().map(|(path, text)| Input {
            path: path.parse().expect("native entry"),
            bytes: text.as_bytes(),
        }),
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("real framed Markdown and TOML syntax")
}

fn node(id: &str, schema: &str, data: Value) -> ContentNode {
    serde_json::from_value(json!({
        "id":id, "site":"site:one", "title":"Controlled data", "lifecycle":"draft",
        "structure":{"kind":"page","family":{"kind":"extension","schema":schema,"data":data}}
    }))
    .expect("shared typed content declaration")
}

#[test]
fn extension_presentation_is_typed_bounded_and_preserved_with_admitted_meaning() {
    let compilation = compile(COUNT);
    let content: ContentNode = serde_json::from_value(json!({
        "id":"content:one", "site":"site:one", "title":"Controlled data",
        "lifecycle":"draft",
        "structure":{"kind":"page","family":{"kind":"extension",
            "schema":"schema:count","data":{"count":2},
            "presentation":{"mode":"mode:immersive","amplitude":3,"variant":"variant:night"}}}
    }))
    .expect("typed extension presentation");
    let resolved = compilation
        .resolve_content(declaration(vec![content]), Limits::default())
        .expect("admitted extension presentation");
    let value = serde_json::to_value(resolved.graph()).expect("semantic graph");
    let presentation = &value["nodes"]["content:one"]["structure"]["family"]["presentation"];
    assert_eq!(
        presentation,
        &json!({"mode":"mode:immersive","amplitude":3,"variant":"variant:night"})
    );

    let invalid: ContentNode = serde_json::from_value(json!({
        "id":"content:invalid", "site":"site:one", "title":"Invalid",
        "lifecycle":"draft",
        "structure":{"kind":"page","family":{"kind":"extension",
            "schema":"schema:count","data":{"count":2},
            "presentation":{"mode":"mode:immersive","amplitude":5}}}
    }))
    .expect("shape remains decodable so graph validation owns the bound");
    let Err(Error::Invalid(errors)) =
        compilation.resolve_content(declaration(vec![invalid]), Limits::default())
    else {
        panic!("out-of-profile amplitude must fail graph admission");
    };
    assert!(errors.iter().any(|error| {
        error.code == "content_extension_presentation_amplitude"
            && error.source == key("content:invalid")
    }));
}

fn declaration(nodes: Vec<ContentNode>) -> Declaration {
    Declaration {
        browsing: vec![],
        nodes,
        memberships: vec![],
        related: vec![],
        projects: vec![],
    }
}

#[test]
fn two_selected_schemas_validate_original_toml_data_through_authored_resolution() {
    let compilation = compile(&format!("{COUNT}{LABEL}"));
    let count = document("content:count", "schema:count", "{count=3}");
    let label =
        document("content:label", "schema:label", "{label='&amp; 🦀'}").replace('\n', "\r\n");
    let corpus = corpus(&[("count.md", &count), ("nested/label.md", &label)]);
    let resolved = authored::resolve(&compilation, &corpus, Default::default())
        .expect("whole controlled content graph");
    for (id, schema, data) in [
        ("content:count", "schema:count", json!({"count":3})),
        ("content:label", "schema:label", json!({"label":"&amp; 🦀"})),
    ] {
        let page = resolved
            .project_page(
                &key(id),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .expect("existing shared page projection");
        let content = serde_json::to_value(&page.data.content).expect("native semantic record");
        assert_eq!(
            content["structure"]["family"],
            json!({"kind":"extension","schema":schema,"data":data})
        );
        let linked = serde_json::to_value(page.json_ld()).expect("existing linked-data projection");
        assert_eq!(
            linked["gazette:structure"]["@value"], content["structure"],
            "linked data retains the same controlled values without inventing a domain ontology"
        );
        assert!(page.body_html.contains("Retained authored body"));
        assert_eq!(
            content["lifecycle"], "draft",
            "schema validity does not promote publication state"
        );
    }
    assert_eq!(resolved.content().graph().nodes().len(), 2);
    assert!(std::ptr::eq(resolved.content().compilation(), &compilation));
}

#[test]
fn unknown_schema_or_late_invalid_payload_never_returns_a_partial_bound_graph() {
    let compilation = compile(COUNT);
    let valid = node("content:first", "schema:count", json!({"count":1}));
    for (schema, data) in [
        ("schema:missing", json!({"count":1})),
        ("schema:count", json!({"count":"not an integer"})),
        ("schema:count", json!({"count":6})),
        ("schema:count", json!({"count":2,"extra":true})),
        ("schema:count", json!(null)),
    ] {
        let input = declaration(vec![valid.clone(), node("content:late", schema, data)]);
        let Err(Error::Invalid(errors)) = compilation.resolve_content(input, Limits::default())
        else {
            panic!("invalid extension must refuse the complete semantic join");
        };
        assert!(
            errors
                .iter()
                .any(|error| error.source == key("content:late")
                    && error.target == Some(key(schema))),
            "diagnostic binds the exact content and selected schema: {errors:?}"
        );
    }
    let valid_text = document("content:first", "schema:count", "{count=1}");
    let invalid_text = document("content:late", "schema:count", "{count='wrong'}");
    let corpus = corpus(&[("first.md", &valid_text), ("last.md", &invalid_text)]);
    assert!(
        authored::resolve(&compilation, &corpus, Default::default()).is_err(),
        "framed authored ingestion cannot bypass the same schema join"
    );
    assert!(
        compilation
            .resolve_content(declaration(vec![valid]), Limits::default())
            .is_ok(),
        "a refusal does not poison the immutable compiled schema"
    );
}

#[test]
fn wrong_dialect_invalid_schema_and_external_reference_fail_workspace_compilation() {
    for schema in [
        "{'$schema'='http://json-schema.org/draft-07/schema#',type='object'}",
        "{'$schema'='https://json-schema.org/draft/2020-12/schema',type='not-a-type'}",
        "{'$schema'='https://json-schema.org/draft/2020-12/schema','$ref'='https://example.invalid/private-schema.json'}",
        "{'$schema'='https://json-schema.org/draft/2020-12/schema','$ref'='file:///private/schema.json'}",
    ] {
        let source = format!("{BASE}[[extensions]]\nid='schema:invalid'\nschema={schema}\n");
        assert!(
            workspace::compile(&source, Default::default()).is_err(),
            "unsupported schema must fail before content resolution: {schema}"
        );
    }
}

#[test]
fn changed_schema_has_a_distinct_source_binding_without_mutating_prior_resolution() {
    let original_source = format!("{BASE}{COUNT}");
    let changed_source = original_source.replace("maximum=5", "maximum=2");
    let original =
        workspace::compile(&original_source, Default::default()).expect("original schema");
    let changed =
        workspace::compile(&changed_source, Default::default()).expect("changed valid schema");
    let input = declaration(vec![node(
        "content:one",
        "schema:count",
        json!({"count":3}),
    )]);
    let old = original
        .resolve_content(input.clone(), Limits::default())
        .expect("original admitted shape");
    assert!(
        changed
            .resolve_content(input.clone(), Limits::default())
            .is_err(),
        "new schema must be used for a new compilation"
    );
    assert_eq!(
        old.compilation().source_sha256,
        Sha256Digest::of(original_source.as_bytes())
    );
    assert_eq!(
        changed.source_sha256,
        Sha256Digest::of(changed_source.as_bytes())
    );
    assert_ne!(old.compilation().source_sha256, changed.source_sha256);
    let repeated = original
        .resolve_content(input, Limits::default())
        .expect("old immutable contract remains usable");
    assert_eq!(old.graph(), repeated.graph());
    assert!(std::ptr::eq(old.compilation(), &original));
}

#[test]
fn extension_data_uses_complete_graph_budget_and_order_independent_resolution() {
    let first = compile(&format!("{COUNT}{LABEL}"));
    let second = compile(&format!("{LABEL}{COUNT}"));
    let mut input = declaration(vec![
        node("content:count", "schema:count", json!({"count":1})),
        node("content:label", "schema:label", json!({"label":"bounded"})),
    ]);
    let limits = Limits {
        text_bytes: 1024,
        ..Default::default()
    };
    let a = first
        .resolve_content(input.clone(), limits)
        .expect("small complete payload");
    input.nodes.reverse();
    let b = second
        .resolve_content(input, limits)
        .expect("equivalent reordered declarations");
    assert_eq!(a.graph(), b.graph());
    assert_eq!(
        serde_json::to_vec(a.graph()).unwrap(),
        serde_json::to_vec(b.graph()).unwrap()
    );
    assert_ne!(
        a.compilation().source_sha256,
        b.compilation().source_sha256,
        "exact source order stays visible even when semantics agree"
    );
    let oversized = declaration(vec![node(
        "content:large",
        "schema:label",
        json!({"label":"x".repeat(4096)}),
    )]);
    assert!(
        matches!(
            first.resolve_content(oversized, limits),
            Err(Error::Limit(_))
        ),
        "extension data must not bypass the shared complete graph allowance"
    );
}
#[test]
fn schema_lookup_and_origin_preserve_unsorted_source_and_duplicate_definition_refuses() {
    use gazette_compiler::{
        toml::PathSegment,
        workspace::{DeclarationKind, FieldQuery, ParsedWorkspace},
    };
    let source =
        format!("# Original 🦀 declarations, not sorted lookup order\n{BASE}{LABEL}{COUNT}")
            .replace('\n', "\r\n");
    let parsed =
        ParsedWorkspace::parse(&source, Default::default()).expect("retained schema declarations");
    let workspace = &parsed.compilation().workspace;
    let selected = workspace
        .extension(&key("schema:count"))
        .expect("exact selected schema lookup");
    assert_eq!(selected.id, key("schema:count"));
    assert_eq!(selected.schema["properties"]["count"]["type"], "integer");
    assert_eq!(selected.schema["properties"]["count"]["maximum"], 5);
    assert!(workspace.extension(&key("schema:missing")).is_none());
    let origin = parsed
        .origin(&FieldQuery {
            kind: DeclarationKind::Extension,
            identity: key("schema:count"),
            field: ["schema", "properties", "count", "type"]
                .into_iter()
                .map(|field| PathSegment::Key(field.into()))
                .collect(),
        })
        .expect("original nested schema field origin");
    assert_eq!(
        origin.path,
        [
            PathSegment::Key("extensions".into()),
            PathSegment::Index(1),
            PathSegment::Key("schema".into()),
            PathSegment::Key("properties".into()),
            PathSegment::Key("count".into()),
            PathSegment::Key("type".into()),
        ]
    );
    assert_eq!(&source[origin.span.start..origin.span.end], "'integer'");
    assert_eq!(origin.source_sha256, Sha256Digest::of(source.as_bytes()));
    let duplicate = format!("{source}{COUNT}");
    let Err(workspace::Error::Invalid(errors)) = workspace::compile(&duplicate, Default::default())
    else {
        panic!("duplicate schema identity must refuse the complete workspace");
    };
    assert!(errors.iter().any(|error| {
        error.code == "workspace_extension_duplicate"
            && error.key == Some(key("schema:count"))
            && error.field == "extensions[2].id"
    }));
    assert_eq!(
        parsed.compilation().source_sha256,
        origin.source_sha256,
        "failed new compilation does not replace the retained source binding"
    );
}
