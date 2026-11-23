use super::*;

#[test]
fn social_card_recipe_keeps_execution_explicit_and_rejects_unbounded_requests() {
    let base = json!({"format":"gazette-local-publication-set/1", "workspace":{"kind":"supplied","source":"not opened during parsing"}, "sites":[site("one")]});
    let parse = |value: &Value| {
        build::LocalPublicationRecipe::parse_json(value.to_string().as_bytes(), 65536, 2)
    };
    let schema = build::LocalPublicationRecipe::input_schema();
    let validator = jsonschema::validator_for(schema.as_value()).unwrap();
    assert!(parse(&base).unwrap().social_card_generation().is_none());
    for permission in [false, true] {
        let mut request = base.clone();
        request["social_card_generation"] = json!({"allow_execute": permission, "cards": 2});
        assert!(validator.is_valid(&request));
        let recipe = parse(&request).unwrap();
        let generation = recipe.social_card_generation().unwrap();
        assert_eq!(generation.execution.allow_execute, permission);
        assert_eq!(generation.cards, 2);
        assert_eq!(generation.execution.timeout.as_millis(), 30_000);
        assert_eq!(
            recipe.allows_generation_execution(),
            permission,
            "shared native effect plan includes browser execution"
        );
    }
    let mut invalid = vec![Value::Null, json!([]), json!([true, 1, 30000])];
    for (field, value) in [
        ("cards", json!(0)),
        ("cards", json!(129)),
        ("max_input_bytes", json!(0)),
        ("request_bytes", json!(67108865)),
        ("assets_bytes", json!(1073741825_u64)),
        ("output_bytes", json!(0)),
        ("timeout_ms", json!(3000)),
        ("timeout_ms", json!(300001)),
        ("allow_execute", json!("true")),
        ("browser", json!("/implicit/browser")),
    ] {
        let mut options = json!(build::SocialCardGeneration::default());
        options[field] = value;
        invalid.push(options);
    }
    for options in invalid {
        let mut request = base.clone();
        request["social_card_generation"] = options;
        assert!(!validator.is_valid(&request), "{request}");
        assert!(parse(&request).is_err(), "{request}");
    }
    let mut duplicate = base;
    duplicate["social_card_generation"] = json!(build::SocialCardGeneration::default());
    let duplicate = duplicate.to_string().replace(
        "\"allow_execute\":false",
        "\"allow_execute\":true,\"allow_execute\":false",
    );
    assert!(build::LocalPublicationRecipe::parse_json(duplicate.as_bytes(), 65536, 2).is_err());
}

#[test]
fn social_card_workspace_pins_select_generation_without_granting_execution() {
    let fixture = Fixture::new(&["one"]);
    let renderer = format!(
        "social_card_renderer={{node={{program='/explicit/node',executable={{bytes=11,sha256='{}'}}}},browser={{program='/explicit/chrome',executable={{bytes=13,sha256='{}'}}}}}}\n",
        "a".repeat(64),
        "b".repeat(64),
    );
    let workspace = format!("{renderer}{}", fixture.workspace);
    for permission in [false, true] {
        let request = json!({"format":"gazette-local-publication-set/1", "workspace":{"kind":"supplied","source":workspace}, "sites":[site("one")], "allow_execute":permission});
        let recipe =
            build::LocalPublicationRecipe::parse_json(request.to_string().as_bytes(), 65536, 2)
                .unwrap();
        let resolved = recipe.resolve(Default::default()).unwrap();
        assert_eq!(
            resolved
                .social_card_generation()
                .unwrap()
                .execution
                .allow_execute,
            permission
        );
    }
}
