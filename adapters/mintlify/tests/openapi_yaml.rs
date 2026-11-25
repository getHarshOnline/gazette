//! Retained YAML reaches the real target boundary and the existing `OpenAPI` rules.

use gazette_adapter_mintlify::Mintlify;
use gazette_core::{
    documentation::{Error, Format, ProjectItem, ProjectModule, Reference, Request, Target},
    source::{CapturedSource, Limits},
};
use gazette_ports::documentation::DocumentationTarget;
use serde_json::{Value, json};

mod support;
use support::Input;

const API: &str = "# retained source comment\nopenapi: 3.1.2\ninfo:\n  title: Native API\n  version: '1'\n  description: |\n    Exact description.\n    Second line.\npaths:\n  /items:\n    get:\n      operationId: listItems\n      responses:\n        '200':\n          description: Complete\n          content:\n            application/json:\n              schema:\n                $ref: '#/components/schemas/Item'\ncomponents:\n  schemas:\n    Item:\n      type: object\n      properties:\n        next:\n          $ref: '#/components/schemas/Item'\n      examples:\n        - $ref: 'literal example, not a reference'\n";

fn assemble(inputs: &[Input], limits: Limits) -> Result<CapturedSource, Error> {
    let navigation = inputs
        .iter()
        .enumerate()
        .map(|(index, input)| {
            ProjectItem::Reference(Reference {
                pages: None,
                slug: if index == 0 { "http" } else { "second" },
                title: "HTTP API",
                artifact: input.artifact(Format::OpenApiYaml),
            })
        })
        .collect();
    Mintlify::new(&gazette_adapter_markdown::Comrak).assemble(
        Request {
            name: "Documentation publication",
            public_base_url: None,
            target: &Target::Mintlify {
                theme: "mint".into(),
                primary_color: "#123456".into(),
                light_color: None,
                dark_color: None,
                telemetry: None,
            },
            projects: &[ProjectModule {
                slug: "project",
                title: "Project",
                manual: None,
                navigation,
            }],
            usage: None,
            metadata: Default::default(),
        },
        limits,
    )
}

fn reason(source: &str) -> String {
    match assemble(&[Input::new(source.as_bytes())], Limits::default()) {
        Err(error) => error.to_string(),
        Ok(_) => panic!("invalid input returned a complete target"),
    }
}

fn file<'a>(output: &'a CapturedSource, path: &str) -> &'a [u8] {
    output
        .file(&path.parse().expect("fixture path"))
        .expect("retained file")
}

#[test]
fn yaml_uses_existing_navigation_and_preserves_original_bytes_without_json_rewriting() {
    for version in ["3.0.4", "3.1.2"] {
        let mut source = API.replace("3.1.2", version);
        if version == "3.0.4" {
            source = source.replace("examples:", "example:");
        }
        let output = assemble(&[Input::new(source.as_bytes())], Limits::default())
            .expect("native YAML target");
        assert_eq!(
            file(&output, "artifacts/project/http.yaml"),
            source.as_bytes()
        );
        let config: Value =
            serde_json::from_slice(file(&output, "docs.json")).expect("configuration");
        assert_eq!(
            config["navigation"]["groups"][0]["pages"][0]["openapi"],
            json!({"source":"artifacts/project/http.yaml","directory":"project/http"})
        );
        assert_eq!(
            file(&output, ".mintignore"),
            b"artifacts/**\n!artifacts/project/\n!artifacts/project/http.yaml\n"
        );
        let again = assemble(&[Input::new(source.as_bytes())], Limits::default())
            .expect("same exact target");
        assert_eq!(output.inventory(), again.inventory());
    }
}

#[test]
fn quoted_strings_null_booleans_and_arbitrary_precision_numbers_keep_their_types() {
    // A floating-point intermediate turns this positive multipleOf into zero,
    // which the official 3.0 schema rejects. The large integer cannot fit u64.
    let source = "openapi: 3.0.4\ninfo: {title: Exact, version: '1'}\npaths: {}\ncomponents:\n  schemas:\n    Exact:\n      type: number\n      multipleOf: 1e-400\n      maximum: 18446744073709551617\n      nullable: true\n      example: null\n    Text:\n      type: string\n      maxLength: 18446744073709551617\n      example: 'true'\n";
    assemble(&[Input::new(source.as_bytes())], Limits::default())
        .expect("exact numbers survive target validation");
    let uppercase = source
        .replace("nullable: true", "nullable: TRUE")
        .replace("example: null", "example: NULL");
    assemble(&[Input::new(uppercase.as_bytes())], Limits::default())
        .expect("YAML uppercase scalar semantics");
    assert!(
        reason(&source.replace("multipleOf: 1e-400", "multipleOf: 0"))
            .contains("pinned version profile")
    );
    assert!(
        reason(&source.replace("nullable: true", "nullable: 'true'"))
            .contains("pinned version profile")
    );
    assert!(
        reason(&source.replace("maxLength: 18446744073709551617", "maxLength: 0.5"))
            .contains("pinned version profile")
    );
}

#[test]
fn decoded_duplicate_keys_and_non_string_keys_fail_before_map_coercion() {
    for extra in [
        "x-key: one\nx-key: two\n",
        "x-key: one\n\"x-\\u006bey\": two\n",
        "x-map: {1: value}\n",
        "x-map: {? [a, b]: value}\n",
    ] {
        let error = reason(&format!("{API}{extra}"));
        assert!(
            error.contains("duplicate OpenAPI YAML")
                || error.contains("object keys must be strings"),
            "{error}"
        );
    }
    assert!(reason(&API.replace("'200':", "200:")).contains("quote numeric response keys"));
}

#[test]
fn tags_anchors_aliases_and_merge_keys_fail_without_ambient_expansion() {
    for extra in [
        "x-value: !include private.yaml\n",
        "x-value: !!str true\n",
        "x-value: &value {a: b}\n",
        "x-value: *missing\n",
        "x-value: {<<: {a: b}}\n",
    ] {
        assert!(!reason(&format!("{API}{extra}")).is_empty());
    }
    // Quoted literal text is not a merge directive or a reference.
    assemble(
        &[Input::new(
            format!("{API}x-value: {{'<<': text}}\n").as_bytes(),
        )],
        Limits::default(),
    )
    .expect("quoted ordinary key");
}

#[test]
fn non_json_numeric_spellings_are_explicit_errors_not_rounded_or_retyped() {
    for number in [".nan", ".inf", "-.inf", "0x10", "0o20", "+12", ".5", "01"] {
        assert!(
            reason(&format!("{API}x-number: {number}\n")).contains("numeric spelling"),
            "{number}"
        );
    }
}

#[test]
fn complete_document_depth_value_and_input_bounds_fail_closed() {
    assemble(
        &[Input::new(format!("%YAML 1.2\n---\n{API}").as_bytes())],
        Limits::default(),
    )
    .expect("explicit same scalar schema");
    assert!(reason(&format!("%YAML 1.1\n---\n{API}")).contains("requires version 1.2"));
    assert!(matches!(
        assemble(
            &[Input::new(API.as_bytes())],
            Limits {
                file_bytes: 1,
                ..Limits::default()
            }
        ),
        Err(Error::Tree(_))
    ));
    assert!(reason(&format!("{API}---\nsecond: document\n")).contains("exactly one document"));
    let nested = format!("{API}x-value: {}0{}\n", "[".repeat(128), "]".repeat(128));
    assert!(reason(&nested).contains("nesting ceiling"));
    let many = format!("{API}x-value: [{}]\n", vec!["0"; 100_000].join(","));
    assert!(reason(&many).contains("100000 values"));
    let error = assemble(&[Input::new(&[0xff])], Limits::default()).expect_err("UTF-8 error");
    assert!(error.to_string().contains("UTF-8"));
}

#[test]
fn same_openapi_rules_reject_wrong_targets_versions_and_late_failed_modules() {
    for source in [
        API.replace("#/components/schemas/Item", "#/components/schemas/Absent"),
        API.replace("#/components/schemas/Item", "#/info"),
        API.replace("3.1.2", "3.2.0"),
        API.replace(
            "#/components/schemas/Item",
            "https://example.invalid/api.json#/Item",
        ),
    ] {
        assert!(
            assemble(
                &[Input::new(API.as_bytes()), Input::new(source.as_bytes())],
                Limits::default()
            )
            .is_err(),
            "a failed later module cannot produce a successful subset"
        );
    }
}
