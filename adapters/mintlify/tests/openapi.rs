//! Real local target assembly for a bounded `OpenAPI` profile, not a hosted build.

use gazette_adapter_mintlify::Mintlify;
use gazette_core::{
    documentation::{
        Error, Format, ProjectItem, ProjectModule, Reference, Request, Selection, Target,
    },
    source::{CapturedSource, Content, Limits},
};
use gazette_ports::documentation::DocumentationTarget;
use serde_json::{Value, json};

mod support;
use support::Input;

fn document() -> Value {
    json!({
        "openapi":"3.1.0", "info":{"title":"HTTP interface & reference","version":"0.1.0"},
        "paths":{"/pets":{"get":{"operationId":"listPets","responses":{"200":{
            "description":"Complete response",
            "content":{"application/json":{"schema":{"$ref":"#/components/schemas/Node"}}}
        }}}}},
        "components":{"schemas":{"Node":{"type":"object","properties":{
            "next":{"$ref":"#/components/schemas/Node"},
            "a/b":{"type":"string"}, "a~b":{"type":"integer"},
            "space name":{"type":"boolean"}, "percent%name":{"type":"string"}
        }}}}
    })
}

fn document_30() -> Value {
    let mut value = document();
    value["openapi"] = json!("3.0.4");
    value["servers"] = json!([{"url":"https://api.example.invalid/v1"}]);
    value["components"]["schemas"]["Node"]["additionalProperties"] = json!(false);
    value["components"]["schemas"]["Node"]["properties"]["amount"] =
        json!({"type":"number","minimum":0,"exclusiveMinimum":true,"nullable":true});
    value["components"]["securitySchemes"] = json!({"oauth":{
        "type":"oauth2","flows":{"authorizationCode":{
            "authorizationUrl":"https://auth.example.invalid/authorize",
            "tokenUrl":"https://auth.example.invalid/token","scopes":{"read":"Read pets"}
        }}
    }});
    value["security"] = json!([{"oauth":["read"]}]);
    value["paths"]["/pets"]["get"]["parameters"] = json!([{
        "name":"tags","in":"query","style":"form","explode":true,
        "schema":{"type":"array","items":{"type":"string"}}
    }]);
    value["paths"]["/pets"]["post"] = json!({
        "operationId":"uploadPet","requestBody":{"required":true,"content":{
            "multipart/form-data":{"schema":{"type":"object","properties":{
                "image":{"type":"string","format":"binary"},
                "pet":{"$ref":"#/components/schemas/Node"}
            }},"encoding":{"image":{"contentType":"image/png"}}}
        }},"responses":{"201":{"description":"Created","headers":{
            "Location":{"schema":{"type":"string"}}
        }}}
    });
    value
}

fn assemble(projects: &[ProjectModule<'_>], limits: Limits) -> Result<CapturedSource, Error> {
    Mintlify::new(&gazette_adapter_markdown::Comrak).assemble(
        Request {
            name: "One documentation publication",
            public_base_url: None,
            target: &Target::Mintlify {
                theme: "mint".into(),
                primary_color: "#123abc".into(),
                light_color: None,
                dark_color: None,
                telemetry: None,
            },
            projects,
            usage: None,
        },
        limits,
    )
}

fn project<'a>(slug: &'a str, input: &'a Input) -> ProjectModule<'a> {
    ProjectModule {
        slug,
        title: "HTTP reference",
        manual: None,
        navigation: vec![ProjectItem::Reference(Reference {
            pages: None,
            slug: "http",
            title: "HTTP API",
            artifact: input.artifact(Format::OpenApiJson),
        })],
    }
}

fn render(value: &Value) -> Result<CapturedSource, Error> {
    let bytes = serde_json::to_vec(value).expect("native artifact JSON");
    assemble(
        &[project("service", &Input::new(&bytes))],
        Limits::default(),
    )
}

fn invalid(value: &Value) {
    let error =
        render(value).expect_err("unsupported selected document cannot return a partial target");
    assert_eq!(error.code(), "documentation_target_input", "{error}");
}

#[test]
fn explicit_root_directory_does_not_infer_an_openapi_file_from_its_contents() {
    let bytes = serde_json::to_vec(&document()).expect("valid native API reference");
    let mut input = Input::new(&bytes);
    assemble(&[project("service", &input)], Limits::default())
        .expect("same bytes are supported when selected as a file");
    input.selection = Selection::Directory { path: None };
    let error = assemble(&[project("service", &input)], Limits::default())
        .expect_err("an explicit directory must not fall back to its valid API sibling");
    assert_eq!(error.code(), "documentation_target_input");
}

fn file<'a>(tree: &'a CapturedSource, path: &str) -> &'a [u8] {
    tree.file(&path.parse().expect("literal output path"))
        .expect("complete generated input")
}

#[test]
fn supported_versions_preserve_exact_original_bytes_and_mixed_reference_navigation() {
    let typedoc = br#"{"schemaVersion":"2.0","variant":"project","id":0,"name":"Client SDK","kind":1,"flags":{},"children":[]}"#;
    let typedoc_input = Input::new(typedoc);
    for version in [
        "3.0.0", "3.0.1", "3.0.2", "3.0.3", "3.0.4", "3.1.0", "3.1.1", "3.1.2",
    ] {
        let mut value = if version.starts_with("3.0.") {
            document_30()
        } else {
            document()
        };
        value["openapi"] = json!(version);
        let mut bytes = serde_json::to_vec_pretty(&value).expect("original layout");
        bytes.extend_from_slice(b"\n \n");
        let input = Input::new(&bytes);
        let projects = [
            ProjectModule {
                slug: "z-client",
                title: "Client first",
                manual: None,
                navigation: vec![
                    ProjectItem::Reference(Reference {
                        pages: None,
                        slug: "z-http",
                        title: "HTTP first",
                        artifact: input.artifact(Format::OpenApiJson),
                    }),
                    ProjectItem::Reference(Reference {
                        pages: None,
                        slug: "a-sdk",
                        title: "Library second",
                        artifact: typedoc_input.artifact(Format::TypeDocJson),
                    }),
                ],
            },
            project("a-service", &input),
        ];
        let tree = assemble(&projects, Limits::default()).expect("complete mixed local target");
        let repeat = assemble(&projects, Limits::default()).expect("deterministic local assembly");
        assert_eq!(tree.inventory(), repeat.inventory());
        for entry in tree.inventory().entries() {
            if matches!(entry.content, Content::File { .. }) {
                assert_eq!(tree.file(&entry.path), repeat.file(&entry.path));
            }
        }
        assert_eq!(file(&tree, "artifacts/z-client/z-http.json"), bytes);
        assert_eq!(file(&tree, "artifacts/a-service/http.json"), bytes);
        assert_eq!(file(&tree, "artifacts/z-client/a-sdk.json"), typedoc);
        assert_eq!(file(&tree, ".mintignore"), b"artifacts/**\n!artifacts/a-service/\n!artifacts/z-client/\n!artifacts/a-service/http.json\n!artifacts/z-client/z-http.json\n");
        let docs: Value =
            serde_json::from_slice(file(&tree, "docs.json")).expect("target configuration");
        assert_eq!(
            docs["navigation"]["dropdowns"],
            json!([
                {"dropdown":"Client first","pages":[
                    {"group":"HTTP first","openapi":{"source":"artifacts/z-client/z-http.json","directory":"z-client/z-http"}},
                    {"group":"Library second","sdk":{"format":"typedoc","source":"artifacts/z-client/a-sdk.json","directory":"z-client/a-sdk"}}
                ]},
                {"dropdown":"HTTP reference","pages":[{"group":"HTTP API","openapi":{"source":"artifacts/a-service/http.json","directory":"a-service/http"}}]}
            ])
        );
    }
}

#[test]
fn official_schema_objects_and_explicit_version_profile_are_checked() {
    for version in [
        "2.0",
        "3.0.5",
        "3.0.4-rc1",
        "3.0",
        "3.0.4 ",
        "3.1.3",
        "3.2.0",
        "3.1",
        "3.1.0 ",
    ] {
        let mut value = document();
        value["openapi"] = json!(version);
        invalid(&value);
    }
    for schema in [
        json!({"type":"imaginary"}),
        json!({"type":7}),
        json!({"type":"string","minLength":"one"}),
        json!({"type":"object","required":"field"}),
        json!({"type":"object","additionalProperties":12}),
        json!({"type":"array","items":[]}),
    ] {
        let mut value = document();
        value["components"]["schemas"]["Node"] = schema;
        invalid(&value);
    }
    let mut value = document();
    value["paths"]["/pets"]["get"]["responses"]["200"] = json!({"content":{}});
    invalid(&value);
    let mut value = document();
    value["info"]["version"] = json!(42);
    invalid(&value);
}

#[test]
fn openapi_30_rejects_31_schema_semantics_without_rewriting_them() {
    for schema in [
        json!(true),
        json!({"type":["string","null"]}),
        json!({"type":"number","exclusiveMinimum":0}),
        json!({"type":"string","nullable":"yes"}),
        json!({"type":"object","properties":{"nested":{"type":["string","null"]}}}),
        json!({"type":"array","items":false}),
        json!({"$defs":{"Nested":{"type":"string"}}}),
    ] {
        let mut value = document_30();
        value["components"]["schemas"]["Invalid"] = schema.clone();
        invalid(&value);
        let mut value = document_30();
        value["paths"]["/pets"]["get"]["parameters"][0]["schema"] = schema.clone();
        invalid(&value);
        let mut value = document_30();
        value["paths"]["/pets"]["get"]["responses"]["200"]["content"]["application/json"]["schema"] =
            schema;
        invalid(&value);
    }
    let mut value = document_30();
    value["jsonSchemaDialect"] = json!("https://spec.openapis.org/oas/3.1/dialect/2024-11-10");
    invalid(&value);
}

#[test]
fn openapi_30_reference_objects_ignore_siblings_but_must_reach_concrete_targets() {
    let mut value = document_30();
    value["components"]["schemas"]["Alias"] = json!({
        "$ref":"#/components/schemas/Node",
        "properties":{"ignored":{"$ref":"https://never-fetch.invalid/ignored.json"}},
        "nullable":"ignored Reference Object sibling"
    });
    let tree =
        render(&value).expect("3.0 reference siblings retain bytes without gaining semantics");
    assert_eq!(
        serde_json::from_slice::<Value>(file(&tree, "artifacts/service/http.json")).unwrap(),
        value
    );
    value["components"]["schemas"]["Node"]["properties"]["next"] =
        json!({"$ref":"#/components/schemas/Alias"});
    render(&value).expect("recursive concrete object with a terminating alias");
    value["components"]["schemas"]["Alias"] = json!({"$ref":"#/components/schemas/Other"});
    value["components"]["schemas"]["Other"] = json!({"$ref":"#/components/schemas/Alias"});
    invalid(&value);
    let mut value = document_30();
    value["components"]["schemas"]["Alias"] = json!({"$ref":"#/components/schemas/Alias"});
    invalid(&value);
}

#[test]
fn openapi_30_typed_targets_exclude_boolean_switches_and_ignored_siblings() {
    for reference in [
        "#/components/schemas/Node/additionalProperties",
        "#/components/schemas/Missing",
        "#/info",
        "https://never-fetch.invalid/schema.json",
    ] {
        let mut value = document_30();
        value["components"]["schemas"]["Alias"] = json!({"$ref":reference});
        invalid(&value);
    }
    let mut value = document_30();
    value["components"]["schemas"]["Alias"] = json!({
        "$ref":"#/components/schemas/Node","properties":{"ignored":{"type":"string"}}
    });
    value["components"]["schemas"]["Other"] =
        json!({"$ref":"#/components/schemas/Alias/properties/ignored"});
    invalid(&value);
}

#[test]
fn schema_pointers_decode_fragments_once_and_preserve_recursive_schema_edges() {
    for reference in [
        "#/components/schemas/Node",
        "#/components/schemas/%4Eode",
        "#/components/schemas/Node/properties/a~1b",
        "#/components/schemas/Node/properties/a~0b",
        "#/components/schemas/Node/properties/a%7E1b",
        "#/components/schemas/Node/properties/space%20name",
        "#/components/schemas/Node/properties/percent%25name",
    ] {
        let mut value = document();
        value["paths"]["/pets"]["get"]["responses"]["200"]["content"]["application/json"]["schema"]
            ["$ref"] = json!(reference);
        let tree = render(&value).expect("exact typed schema pointer including recursive Node");
        let retained: Value = serde_json::from_slice(file(&tree, "artifacts/service/http.json"))
            .expect("retained artifact");
        assert_eq!(
            retained, value,
            "references are retained, never inlined or repaired"
        );
    }
    for reference in [
        "#/components/schemas/Missing",
        "#/info",
        "#/info/title",
        "#/components/schemas/Node/properties/a~2b",
        "#/components/schemas/Node/properties/a%257E1b",
        "#/components/schemas/Node/properties/space name",
        "#/components/schemas/%GG",
        "other.json#/components/schemas/Node",
        "https://example.invalid/openapi.json#/components/schemas/Node",
        "file:///private/never-read.json#/Node",
    ] {
        let mut value = document();
        value["paths"]["/pets"]["get"]["responses"]["200"]["content"]["application/json"]["schema"]
            ["$ref"] = json!(reference);
        invalid(&value);
    }
}

#[test]
fn literal_ref_fields_in_examples_defaults_enums_constants_and_extensions_are_data() {
    let data =
        json!({"$ref":"https://never-fetch.invalid/private.json","nested":{"$ref":"#/missing"}});
    let mut value = document();
    value["x-retained-data"] = data.clone();
    let schema = &mut value["components"]["schemas"]["Node"];
    schema["examples"] = json!([data.clone()]);
    schema["default"] = data.clone();
    schema["enum"] = json!([data.clone()]);
    schema["const"] = data.clone();
    schema["x-extra"] = data.clone();
    let media =
        &mut value["paths"]["/pets"]["get"]["responses"]["200"]["content"]["application/json"];
    media["examples"] = json!({"retained":{"value":data}});
    let tree = render(&value).expect("data is not promoted into a reference edge");
    assert_eq!(
        serde_json::from_slice::<Value>(file(&tree, "artifacts/service/http.json"))
            .expect("stored JSON"),
        value
    );
    value["components"]["schemas"]["Node"]["properties"]["broken"] = json!({"$ref":"#/missing"});
    invalid(&value);
}

#[test]
fn operation_refs_discriminator_mappings_and_response_cycles_have_typed_targets() {
    let mut linked = document();
    linked["paths"]["/pets"]["get"]["responses"]["200"]["links"] =
        json!({"again":{"operationRef":"#/paths/~1pets/get"}});
    linked["components"]["schemas"]["Node"]["discriminator"] =
        json!({"propertyName":"kind","mapping":{"node":"#/components/schemas/Node"}});
    render(&linked).expect("operation edge and recursive schema mapping remain explicit");
    let mut duplicate = linked.clone();
    let operation = duplicate["paths"]["/pets"]["get"].clone();
    duplicate["paths"]["/other"] = json!({"get":operation});
    invalid(&duplicate);
    for reference in [
        "#/paths/~1pets",
        "#/components/schemas/Node",
        "#/paths/~1missing/get",
        "https://example.invalid/operations.json#/get",
    ] {
        let mut value = linked.clone();
        value["paths"]["/pets"]["get"]["responses"]["200"]["links"]["again"]["operationRef"] =
            json!(reference);
        invalid(&value);
    }
    for reference in [
        "#/paths/~1pets/get",
        "#/components/schemas/Missing",
        "https://example.invalid/schema.json",
    ] {
        let mut value = linked.clone();
        value["components"]["schemas"]["Node"]["discriminator"]["mapping"]["node"] =
            json!(reference);
        invalid(&value);
    }
    let mut value = document();
    value["components"]["responses"] =
        json!({"a":{"$ref":"#/components/responses/b"},"b":{"$ref":"#/components/responses/a"}});
    value["paths"]["/pets"]["get"]["responses"]["200"] = json!({"$ref":"#/components/responses/a"});
    invalid(&value);
    value["components"]["responses"]["b"] = json!({"description":"Resolved response"});
    render(&value).expect("acyclic typed response references work without inlining");
    value["paths"]["/pets"]["get"]["responses"]["200"] =
        json!({"$ref":"#/components/schemas/Node"});
    invalid(&value);
}

#[test]
fn unsupported_nested_schema_addressing_fails_instead_of_reinterpreting_its_scope() {
    for (keyword, value) in [
        ("$id", json!("https://example.invalid/new-scope")),
        (
            "$schema",
            json!("https://json-schema.org/draft/2020-12/schema"),
        ),
        ("$anchor", json!("node")),
        ("$dynamicAnchor", json!("node")),
        ("$dynamicRef", json!("#node")),
    ] {
        let mut document = document();
        document["components"]["schemas"]["Node"][keyword] = value;
        invalid(&document);
    }
}

#[test]
fn duplicate_keys_and_a_late_invalid_reference_never_produce_a_successful_subset() {
    let valid = serde_json::to_vec(&document()).expect("valid artifact");
    let valid_input = Input::new(&valid);
    let duplicate = String::from_utf8(valid.clone())
        .expect("JSON text")
        .replace(
            "\"openapi\":\"3.1.0\"",
            "\"openapi\":\"3.1.0\",\"opena\\u0070i\":\"3.1.0\"",
        );
    assert_ne!(
        duplicate.as_bytes(),
        valid,
        "decoded-key duplicate is actually injected"
    );
    let error = assemble(
        &[
            project("valid", &valid_input),
            project("invalid", &Input::new(duplicate.as_bytes())),
        ],
        Limits::default(),
    )
    .expect_err("late duplicate key fails complete target");
    assert_eq!(error.code(), "documentation_target_input");
    let mut broken = document();
    broken["paths"]["/pets"]["get"]["responses"]["200"]["content"]["application/json"]["schema"]
        ["$ref"] = json!("#/missing");
    let broken = serde_json::to_vec(&broken).expect("broken selected artifact");
    let broken_input = Input::new(&broken);
    let project = ProjectModule {
        slug: "service",
        title: "No silent subset",
        manual: None,
        navigation: vec![
            ProjectItem::Reference(Reference {
                pages: None,
                slug: "first",
                title: "Valid first",
                artifact: valid_input.artifact(Format::OpenApiJson),
            }),
            ProjectItem::Reference(Reference {
                pages: None,
                slug: "last",
                title: "Invalid last",
                artifact: broken_input.artifact(Format::OpenApiJson),
            }),
        ],
    };
    assert_eq!(
        assemble(&[project], Limits::default())
            .expect_err("late selected failure")
            .code(),
        "documentation_target_input"
    );
    let first =
        render(&document()).expect("failed previous request leaves no retained partial state");
    let second = render(&document()).expect("repeat");
    assert_eq!(first.inventory(), second.inventory());
}

#[test]
fn original_artifact_bytes_count_before_validation_and_copying() {
    let mut bytes = serde_json::to_vec(&document()).expect("valid document");
    bytes.extend(std::iter::repeat_n(b' ', 2048));
    let input = Input::new(&bytes);
    let tree =
        assemble(&[project("service", &input)], Limits::default()).expect("retained padded input");
    let maximum_file = tree
        .inventory()
        .entries()
        .iter()
        .filter_map(|entry| match entry.content {
            Content::File { bytes, .. } => Some(bytes),
            Content::Directory => None,
        })
        .max()
        .expect("files");
    let exact = Limits {
        file_bytes: maximum_file,
        total_bytes: tree.inventory().total_bytes(),
        ..Limits::default()
    };
    assert_eq!(
        assemble(&[project("service", &input)], exact)
            .expect("exact complete byte bounds")
            .inventory(),
        tree.inventory()
    );
    for limits in [
        Limits {
            file_bytes: bytes.len() as u64 - 1,
            ..Limits::default()
        },
        Limits {
            total_bytes: tree.inventory().total_bytes() - 1,
            ..Limits::default()
        },
    ] {
        assert!(
            matches!(
                assemble(&[project("service", &input)], limits),
                Err(Error::Tree(_))
            ),
            "no oversized successful target"
        );
    }
}

#[test]
fn each_outer_schema_location_receives_the_selected_dialect_validation() {
    let schema = json!({"type":"object","properties":{"nested":{"type":"string","minLength":1}}});
    let mut value = document();
    value["components"]["schemas"]["Node"] = schema.clone();
    value["paths"]["/pets"]["get"]["parameters"] =
        json!([{"name":"filter","in":"query","schema":schema.clone()}]);
    value["paths"]["/pets"]["get"]["responses"]["200"]["headers"] =
        json!({"X-Result":{"schema":schema.clone()}});
    value["paths"]["/pets"]["get"]["responses"]["200"]["content"]["application/json"]["schema"] =
        schema;
    render(&value).expect("valid independent component, parameter, header and media schemas");
    for root in [
        "/components/schemas/Node",
        "/paths/~1pets/get/parameters/0/schema",
        "/paths/~1pets/get/responses/200/headers/X-Result/schema",
        "/paths/~1pets/get/responses/200/content/application~1json/schema",
    ] {
        let mut invalid = value.clone();
        let pointer = format!("{root}/properties/nested/type");
        *invalid
            .pointer_mut(&pointer)
            .expect("actual typed schema location") = json!("not-a-json-schema-type");
        let error =
            render(&invalid).expect_err("nested invalid type cannot escape split validation");
        match error {
            Error::Invalid { reason, .. } => assert_eq!(
                reason, "OpenAPI Schema Object does not conform to the pinned OAS dialect",
                "{root}"
            ),
            other => panic!("expected native dialect rejection for {root}: {other}"),
        }
    }
}

#[test]
fn selected_dialect_and_nested_schema_addressing_are_checked_without_promoting_data_arrays() {
    const DIALECT: &str = "https://spec.openapis.org/oas/3.1/dialect/2024-11-10";
    let mut value = document();
    value["jsonSchemaDialect"] = json!(DIALECT);
    value["components"]["schemas"]["Node"]["$schema"] = json!(DIALECT);
    value["components"]["schemas"]["Node"]["definitions"] =
        json!({"Defined":{"$schema":DIALECT,"type":"string"}});
    value["components"]["schemas"]["Node"]["dependencies"] = json!({
        "schemaDependency":{"$schema":DIALECT,"properties":{"needed":{"type":"string"}}},
        "propertyDependency":["$ref","$schema","$recursiveRef"]
    });
    render(&value).expect("selected root/nested dialect and literal property dependency arrays");
    for pointer in [
        "/jsonSchemaDialect",
        "/components/schemas/Node/$schema",
        "/components/schemas/Node/definitions/Defined/$schema",
        "/components/schemas/Node/dependencies/schemaDependency/$schema",
    ] {
        let mut invalid = value.clone();
        *invalid
            .pointer_mut(pointer)
            .expect("existing dialect field") = json!("https://example.invalid/other-dialect");
        let error = render(&invalid).expect_err("no alternate dialect or remote resolution");
        match error {
            Error::Invalid { reason, .. } => assert!(
                matches!(
                    reason,
                    "unsupported OpenAPI JSON Schema dialect"
                        | "unsupported OpenAPI Schema Object dialect"
                ),
                "{pointer}: {reason}"
            ),
            other => panic!("expected explicit dialect rejection at {pointer}: {other}"),
        }
    }
    for container in ["definitions", "dependencies"] {
        for (keyword, content) in [
            ("$recursiveRef", json!("#")),
            ("$recursiveAnchor", json!("node")),
        ] {
            let mut invalid = value.clone();
            let mut unsupported = json!({});
            unsupported[keyword] = content;
            invalid["components"]["schemas"]["Node"][container]["Unsupported"] = unsupported;
            let error = render(&invalid)
                .expect_err("nested recursive addressing must not silently become data");
            match error {
                Error::Invalid { reason, .. } => assert_eq!(
                    reason,
                    "OpenAPI recursive addressing is outside the supported local-pointer profile"
                ),
                other => panic!("expected bounded-profile addressing rejection: {other}"),
            }
        }
    }
}

#[test]
fn exact_json_value_ceiling_is_distinct_from_schema_or_output_failure() {
    // Seven structural values: root, version, info, title, info.version, paths,
    // and this extension array. Each zero adds one value without nested depth.
    let mut value = json!({"openapi":"3.1.0","info":{"title":"Bounded","version":"1"},"paths":{},"x-values":vec![0; 100_000 - 7]});
    let tree = render(&value).expect("exactly 100000 retained values fit the declared ceiling");
    assert_eq!(
        serde_json::from_slice::<Value>(file(&tree, "artifacts/service/http.json"))
            .expect("retained original"),
        value
    );
    value["x-values"]
        .as_array_mut()
        .expect("extension array")
        .push(json!(0));
    let error = render(&value)
        .expect_err("100001 values exceed structural ceiling before schema acceptance");
    match error {
        Error::Invalid { context, reason } => {
            assert_eq!(context, "artifacts/service/http.json");
            assert_eq!(
                reason,
                "OpenAPI JSON exceeds 100000 values; narrow the artifact"
            );
        }
        other => {
            panic!("expected exact structural resource diagnostic, not another failure: {other}")
        }
    }
}

#[test]
fn operation_reference_requires_uri_encoding_before_json_pointer_decoding() {
    let mut value = document();
    value["paths"]["/pets/{id}"] = json!({"get":{"operationId":"getPet","parameters":[{"name":"id","in":"path","required":true,"schema":{"type":"string"}}],"responses":{"200":{"description":"Pet"}}}});
    value["components"]["links"] = json!({"pet":{"operationRef":"#/paths/~1pets~1%7Bid%7D/get"}});
    render(&value).expect("encoded operation pointer resolves to exact declared operation");
    value["components"]["links"]["pet"]["operationRef"] = json!("#/paths/~1pets~1{id}/get");
    match render(&value)
        .expect_err("raw braces are not valid URI syntax despite an existing target")
    {
        Error::Invalid { reason, .. } => {
            assert_eq!(reason, "OpenAPI reference is not a valid URI fragment")
        }
        other => panic!("expected URI grammar failure: {other}"),
    }
}
