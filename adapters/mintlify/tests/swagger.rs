//! Swagger conversion is exercised through the real retained-artifact target.

use gazette_adapter_mintlify::Mintlify;
use gazette_core::{
    documentation::{Error, Format, ProjectItem, ProjectModule, Reference, Request, Target},
    source::{CapturedSource, Content, Limits},
};
use gazette_ports::documentation::DocumentationTarget;
use serde_json::{Value, json};

mod support;
use support::Input;

fn document() -> Value {
    json!({
        "swagger":"2.0", "info":{"title":"Native HTTP API","version":"1.2.3"},
        "host":"api.example.invalid", "basePath":"/v1", "schemes":["https"],
        "consumes":["application/json"], "produces":["application/json"],
        "paths":{"/widgets":{"post":{
            "operationId":"createWidget", "summary":"Create a widget",
            "parameters":[{"name":"widgetBody","in":"body","required":true,
                "description":"Exact body documentation","schema":{"$ref":"#/definitions/Widget"}}],
            "responses":{"201":{"description":"Created","schema":{"$ref":"#/definitions/Widget"}}}
        }}},
        "definitions":{"Widget":{"type":"object","required":["name"],"properties":{
            "name":{"type":"string","description":"Public name"},
            "next":{"$ref":"#/definitions/Widget"}
        }}}
    })
}

fn assemble(input: &Input, format: Format, limits: Limits) -> Result<CapturedSource, Error> {
    let projects = [ProjectModule {
        slug: "service",
        title: "Service reference",
        manual: None,
        navigation: vec![ProjectItem::Reference(Reference {
            pages: None,
            slug: "http",
            title: "HTTP API",
            artifact: input.artifact(format),
        })],
    }];
    Mintlify::new(&gazette_adapter_markdown::Comrak).assemble(
        Request {
            name: "Service documentation",
            public_base_url: None,
            target: &Target::Mintlify {
                theme: "mint".into(),
                primary_color: "#123abc".into(),
                light_color: None,
                dark_color: None,
                telemetry: None,
            },
            projects: &projects,
            usage: None,
            metadata: Default::default(),
        },
        limits,
    )
}

fn render(value: &Value) -> Result<CapturedSource, Error> {
    assemble(
        &Input::new(&serde_json::to_vec(value).expect("fixture JSON")),
        Format::SwaggerJson,
        Limits::default(),
    )
}

fn file<'a>(tree: &'a CapturedSource, path: &str) -> &'a [u8] {
    tree.file(&path.parse().expect("output path"))
        .expect("complete output artifact")
}

fn converted(tree: &CapturedSource) -> Value {
    serde_json::from_slice(file(tree, "artifacts/service/http.json"))
        .expect("validated OpenAPI JSON")
}

fn dereference<'a>(document: &'a Value, value: &'a Value) -> &'a Value {
    if let Some(reference) = value.get("$ref").and_then(Value::as_str) {
        document
            .pointer(reference.strip_prefix('#').expect("local reference"))
            .expect("reference target")
    } else {
        value
    }
}

#[test]
fn retained_swagger_becomes_repeatable_openapi_without_mutating_native_source() {
    let mut bytes = serde_json::to_vec_pretty(&document()).expect("native JSON");
    bytes.extend_from_slice(b"\n \n");
    let input = Input::new(&bytes);
    let original = input.tree.inventory().identity();
    let first =
        assemble(&input, Format::SwaggerJson, Limits::default()).expect("Swagger target assembly");
    let second =
        assemble(&input, Format::SwaggerJson, Limits::default()).expect("same retained input");
    assert_eq!(first.inventory(), second.inventory());
    for entry in first.inventory().entries() {
        if matches!(entry.content, Content::File { .. }) {
            assert_eq!(first.file(&entry.path), second.file(&entry.path));
        }
    }
    assert_eq!(input.tree.inventory().identity(), original);
    assert_eq!(
        input
            .artifact(Format::SwaggerJson)
            .bytes()
            .expect("original capture"),
        bytes
    );
    let value = converted(&first);
    assert_eq!(value["openapi"], "3.0.3");
    assert_eq!(
        file(&first, ".mintignore"),
        b"artifacts/**\n!artifacts/service/\n!artifacts/service/http.json\n"
    );
    assert_eq!(value["servers"][0]["url"], "https://api.example.invalid/v1");
    let operation = &value["paths"]["/widgets"]["post"];
    assert_eq!(operation["x-gazette-body-name"], "widgetBody");
    let body = dereference(&value, &operation["requestBody"]);
    assert_eq!(body["description"], "Exact body documentation");
    assert_eq!(body["required"], true);
    assert_eq!(
        body["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/Widget"
    );
    assert_eq!(
        value["components"]["schemas"]["Widget"]["properties"]["next"]["$ref"],
        "#/components/schemas/Widget"
    );
    let navigation: Value =
        serde_json::from_slice(file(&first, "docs.json")).expect("target navigation");
    assert_eq!(
        navigation["navigation"]["groups"][0]["pages"][0]["openapi"]["source"],
        "artifacts/service/http.json"
    );
}

#[test]
fn oauth_queries_and_colliding_native_names_preserve_their_separate_security_meanings() {
    let mut source = document();
    source["securityDefinitions"] = json!({
        "tenant auth":{"type":"oauth2","flow":"accessCode",
            "authorizationUrl":"https://auth.example.invalid/authorize?tenant=alpha&audience=api",
            "tokenUrl":"https://auth.example.invalid/token?realm=alpha", "scopes":{"write":"Create widgets"}},
        "tenant_auth":{"type":"apiKey","in":"header","name":"X-Key"}
    });
    source["security"] = json!([{"tenant auth":["write"],"tenant_auth":[]}]);
    source["paths"]["/widgets"]["post"]["security"] = json!([{"tenant auth":["write"]}]);
    let value = converted(&render(&source).expect("both distinct security schemes"));
    let schemes = value["components"]["securitySchemes"]
        .as_object()
        .expect("scheme catalog");
    assert_eq!(schemes.len(), 2);
    assert_eq!(schemes["tenant_auth"]["name"], "X-Key");
    let (name, oauth) = schemes
        .iter()
        .find(|(_, s)| s.get("x-gazette-original-name") == Some(&json!("tenant auth")))
        .expect("original unsafe name retained");
    assert_ne!(name, "tenant_auth");
    let flow = &oauth["flows"]["authorizationCode"];
    assert_eq!(
        flow["authorizationUrl"],
        source["securityDefinitions"]["tenant auth"]["authorizationUrl"]
    );
    assert_eq!(
        flow["tokenUrl"],
        source["securityDefinitions"]["tenant auth"]["tokenUrl"]
    );
    assert_eq!(value["security"][0][name], json!(["write"]));
    assert_eq!(
        value["paths"]["/widgets"]["post"]["security"][0][name],
        json!(["write"])
    );
}

#[test]
fn both_form_media_types_survive_and_urlencoded_arrays_keep_explicit_serialization() {
    let mut source = document();
    source["consumes"] = json!(["application/x-www-form-urlencoded", "multipart/form-data"]);
    source["parameters"] = json!({"label":{"name":"label","in":"formData","type":"string","required":true,"description":"A label"}});
    source["paths"]["/widgets"]["post"]["parameters"] = json!([{"$ref":"#/parameters/label"}]);
    source["paths"]["/arrays"] = json!({"post":{
        "consumes":["application/x-www-form-urlencoded"],
        "parameters":[
            {"name":"csv","in":"formData","type":"array","items":{"type":"string"},"collectionFormat":"csv"},
            {"name":"multi","in":"formData","type":"array","items":{"type":"string"},"collectionFormat":"multi"},
            {"name":"spaces","in":"formData","type":"array","items":{"type":"string"},"collectionFormat":"ssv"},
            {"name":"pipes","in":"formData","type":"array","items":{"type":"string"},"collectionFormat":"pipes"}
        ], "responses":{"200":{"description":"Accepted"}}
    }});
    let value = converted(&render(&source).expect("all declared form alternatives"));
    let content = &value["paths"]["/widgets"]["post"]["requestBody"]["content"];
    assert_eq!(content.as_object().expect("both media").len(), 2);
    for media in ["application/x-www-form-urlencoded", "multipart/form-data"] {
        assert_eq!(
            content[media]["schema"]["properties"]["label"]["description"],
            "A label"
        );
        assert_eq!(content[media]["schema"]["required"], json!(["label"]));
    }
    let encoding = &value["paths"]["/arrays"]["post"]["requestBody"]["content"]["application/x-www-form-urlencoded"]
        ["encoding"];
    assert_eq!(encoding["csv"], json!({"style":"form","explode":false}));
    assert_eq!(encoding["multi"], json!({"style":"form","explode":true}));
    assert_eq!(
        encoding["spaces"],
        json!({"style":"spaceDelimited","explode":false})
    );
    assert_eq!(
        encoding["pipes"],
        json!({"style":"pipeDelimited","explode":false})
    );
}

#[test]
fn multipart_file_and_scalar_inputs_preserve_media_without_ignored_encoding_fields() {
    let mut source = document();
    source["consumes"] = json!(["application/x-www-form-urlencoded", "multipart/form-data"]);
    source["paths"]["/widgets"]["post"]["parameters"] = json!([
        {"name":"file","in":"formData","type":"file","required":true},
        {"name":"label","in":"formData","type":"string"}
    ]);
    let value = converted(&render(&source).expect("both file/scalar representations"));
    let content = &value["paths"]["/widgets"]["post"]["requestBody"]["content"];
    for media in ["application/x-www-form-urlencoded", "multipart/form-data"] {
        assert_eq!(
            content[media]["schema"]["properties"]["file"]["format"],
            "binary"
        );
    }
    assert_eq!(
        content["multipart/form-data"]["encoding"]["file"],
        json!({"contentType":"application/octet-stream"})
    );
    assert_eq!(
        content["multipart/form-data"]["encoding"]["label"],
        json!({"contentType":"text/plain"})
    );
    source["paths"]["/widgets"]["post"]["parameters"] = json!([
        {"name":"tags","in":"formData","type":"array","items":{"type":"string"},"collectionFormat":"csv"}
    ]);
    let error = render(&source).expect_err("multipart cannot silently ignore delimiter semantics");
    assert!(
        error
            .to_string()
            .contains("multipart encoding cannot preserve"),
        "{error}"
    );
}

#[test]
fn literal_reference_shaped_examples_are_not_acquired_or_rewritten() {
    let mut source = document();
    let example = json!({"$ref":"https://unread.example.invalid/private.json","nested":{"$ref":"#/definitions/Widget"}});
    source["definitions"]["Widget"]["example"] = example.clone();
    source["definitions"]["Widget"]["x-native"] = example.clone();
    source["paths"]["/widgets"]["post"]["responses"]["201"]["examples"] =
        json!({"application/json":example});
    let value =
        converted(&render(&source).expect("opaque source example is not a resolver request"));
    assert_eq!(value["components"]["schemas"]["Widget"]["example"], example);
    assert_eq!(
        value["components"]["schemas"]["Widget"]["x-native"],
        example
    );
    assert_eq!(
        value["paths"]["/widgets"]["post"]["responses"]["201"]["content"]["application/json"]["examples"]
            ["response"]["value"],
        example
    );
}

#[test]
fn external_or_missing_real_references_and_invalid_original_schema_are_refused() {
    for reference in [
        "file:///private/unread.json",
        "https://unread.example.invalid/schema.json",
        "#/definitions/Missing",
    ] {
        let mut source = document();
        source["definitions"]["Widget"]["properties"]["next"]["$ref"] = json!(reference);
        let error = render(&source).expect_err("no external read or unresolved target");
        assert_eq!(error.code(), "documentation_target_input");
    }
    let mut source = document();
    source["definitions"]["Widget"]["properties"]["invalid"] = json!({"type":42});
    let error = render(&source).expect_err("invalid original schema is not repaired");
    assert_eq!(error.code(), "documentation_target_input");
    assert!(
        error
            .to_string()
            .contains("Swagger 2 input does not conform")
    );
}

#[test]
fn tuple_schema_is_refused_without_confusing_literal_array_data() {
    let mut source = document();
    source["definitions"]["Widget"]["properties"]["array"] =
        json!({"type":"array","items":[{"type":"string"},{"type":"integer"}]});
    let error = render(&source).expect_err("tuple meaning cannot be silently collapsed");
    assert_eq!(error.code(), "documentation_target_input");
    assert!(
        error
            .to_string()
            .contains("Swagger tuple items cannot be preserved")
    );
    let mut source = document();
    let example = json!({"items":["first",42]});
    source["definitions"]["Widget"]["example"] = example.clone();
    let value = converted(&render(&source).expect("literal array data is not a schema"));
    assert_eq!(value["components"]["schemas"]["Widget"]["example"], example);
}

#[test]
fn output_expansion_is_bounded_before_an_artifact_can_be_returned() {
    let bytes = serde_json::to_vec(&document()).expect("native input");
    let input = Input::new(&bytes);
    let error = assemble(
        &input,
        Format::SwaggerJson,
        Limits {
            file_bytes: bytes.len() as u64,
            ..Limits::default()
        },
    )
    .expect_err("converted output and target configuration remain byte bounded");
    assert!(!error.to_string().is_empty());
}

#[test]
fn explicit_yaml_selection_uses_bounded_native_parser_and_rejects_aliases() {
    let bytes = b"swagger: '2.0'\ninfo: {title: Native API, version: '1.0'}\npaths: {}\n";
    let input = Input::new(bytes);
    let output =
        assemble(&input, Format::SwaggerYaml, Limits::default()).expect("native YAML input");
    assert_eq!(converted(&output)["info"]["title"], "Native API");
    let alias = Input::new(
        b"swagger: '2.0'\ninfo: &info {title: API, version: '1.0'}\npaths: {}\nx-copy: *info\n",
    );
    assert!(assemble(&alias, Format::SwaggerYaml, Limits::default()).is_err());
    let duplicate = Input::new(b"{\"swagger\":\"2.0\",\"swagger\":\"2.0\",\"info\":{\"title\":\"API\",\"version\":\"1\"},\"paths\":{}}");
    assert!(assemble(&duplicate, Format::SwaggerJson, Limits::default()).is_err());
}
