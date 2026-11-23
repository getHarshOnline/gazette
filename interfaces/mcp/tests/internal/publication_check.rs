//! Discovery and dispatch agree before any effectful source acquisition.

use super::*;
use serde_json::json;

fn args() -> Value {
    json!({"source":"not valid TOML", "site":"site:one"})
}

fn decode(route: Route, value: Value) -> Result<Input, ErrorData> {
    route.decode(value.as_object().expect("named arguments").clone())
}

fn mathematical_review() -> (tempfile::TempDir, Value, Value) {
    let root = tempfile::tempdir().unwrap();
    let content = root.path().join("content");
    std::fs::create_dir(&content).unwrap();
    let theme = root.path().join("theme");
    std::fs::create_dir_all(theme.join("templates")).unwrap();
    std::fs::write(theme.join("theme.toml"), "package='theme:one'\nnamespace='one'\ntemplates='templates'\nbrowser_theme_colors={light='#ffffff',dark='#000000'}\nrender={page='page.html'}\n").unwrap();
    // Review may read declarations, but must not compile or run this template.
    std::fs::write(
        theme.join("templates/page.html"),
        "{{ deliberately uncompiled",
    )
    .unwrap();
    let article = "+++\n[content]\nid='content:calculation'\nsite='site:one'\ntitle='Calculation'\nlifecycle='draft'\nroute={path='/calculation/'}\nstructure={kind='page',family={kind='article'}}\n+++\n# Calculation\n\n$x$\n";
    std::fs::write(content.join("index.md"), article).unwrap();
    let workspace = format!(
        "[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n[[brands]]\nid='brand:one'\nname='Example'\n[[brands.publications]]\nsite='site:one'\npublic_base_url='https://example.test/'\nauthored=[{{source='source:content',target='content',markdown_profile='publication-math'}}]\n[[sources]]\nid='source:content'\nlocation={{kind='filesystem',root={}}}\n",
        serde_json::to_string(&content).unwrap()
    );
    let workspace = format!(
        "[composition]\ntheme={{package='theme:one'}}\n{workspace}\n[[sources]]\nid='source:theme'\nlocation={{kind='filesystem',root={}}}\n[[theme_packages]]\npackage='theme:one'\nsource='source:theme'\nmanifest='theme.toml'\n",
        serde_json::to_string(&theme).unwrap()
    );
    let selected: build::WorkspaceSource = workspace.clone().into();
    let acquired = build::acquire_selected_publication(
        &selected,
        &"site:one".parse().unwrap(),
        toml::Limits::default(),
        default_acquisition(),
    )
    .unwrap();
    let parsed = build::parse_publication(&acquired, corpus::Limits::default()).unwrap();
    let (location, identity, facts) = parsed.corpus().files()["content/index.md"]
        .math_sources()
        .next()
        .unwrap();
    let bindings: Vec<_> = facts.iter().filter(|fact| fact.emitted).map(|fact| json!({
        "occurrence": fact,
        "mathml": "<math xmlns=\"http://www.w3.org/1998/Math/MathML\"><semantics><mi>x</mi><annotation encoding=\"application/x-tex\">x</annotation></semantics></math>"
    })).collect();
    let retained = json!({"content/index.md":[{"location":location,"result":{"identity":identity,"bindings":bindings}}]});
    let request = json!({"source":workspace,"site":"site:one","allow_read":true});
    (root, request, retained)
}

fn run_review(value: Value) -> CallToolResult {
    decode(Route::Review, value)
        .unwrap()
        .execute(1 << 20)
        .unwrap()
}

#[test]
fn review_requires_explicit_current_retained_math_and_never_rewrites_sources() {
    let (root, mut request, retained) = mathematical_review();
    let source = root.path().join("content/index.md");
    let original = std::fs::read(&source).unwrap();
    let missing = run_review(request.clone());
    assert_eq!(
        missing.structured_content.unwrap()["code"],
        "publication_review_math_required"
    );
    request["retained_math"] = retained.clone();
    let descriptor = serde_json::to_value(Route::Review.descriptor()).unwrap();
    assert!(
        jsonschema::validator_for(&descriptor["inputSchema"])
            .unwrap()
            .is_valid(&request)
    );
    let complete = run_review(request.clone());
    assert_ne!(complete.is_error, Some(true), "{complete:?}");
    assert!(
        complete.structured_content.unwrap()["request"]["surfaces"]
            .to_string()
            .contains("<math")
    );
    assert_eq!(std::fs::read(&source).unwrap(), original);

    for changed in [
        json!({}),
        {
            let mut map = retained.clone();
            map["content/extra.md"] = map["content/index.md"].clone();
            map
        },
        {
            let mut map = retained.clone();
            map["content/index.md"][0]["result"]["identity"]["source_sha256"] =
                json!(source::Sha256Digest::of(b"stale"));
            map
        },
        {
            let mut map = retained.clone();
            map["content/index.md"][0]["result"]["bindings"][0]["mathml"] =
                json!("<script>bad</script>");
            map
        },
    ] {
        request["retained_math"] = changed;
        assert_eq!(run_review(request.clone()).is_error, Some(true));
        assert_eq!(std::fs::read(&source).unwrap(), original);
    }
    request["retained_math"] = retained;
    std::fs::write(
        &source,
        String::from_utf8(original).unwrap().replace("$x$", "$y$"),
    )
    .unwrap();
    assert_eq!(
        run_review(request).is_error,
        Some(true),
        "source edit invalidates retained results"
    );
}

#[test]
fn retained_math_shape_and_budget_fail_before_source_acquisition() {
    let (_root, mut request, retained) = mathematical_review();
    request["retained_math"] = retained.clone();
    request["review"] = json!({"bytes": 1});
    request["source"] = json!("invalid TOML [");
    assert_eq!(
        run_review(request.clone()).structured_content.unwrap()["code"],
        "publication_review_math_limit"
    );
    for pointer in [
        "/content~1index.md/0/result/identity",
        "/content~1index.md/0/result/bindings/0/occurrence/location/span",
    ] {
        let mut invalid = retained.clone();
        invalid.pointer_mut(pointer).unwrap()["unknown"] = json!(true);
        request["retained_math"] = invalid;
        assert!(decode(Route::Review, request.clone()).is_err());
    }
    for (pointer, positional) in [
        (
            "/content~1index.md/0/result/identity",
            json!([
                retained["content/index.md"][0]["result"]["identity"]["source_sha256"],
                "publication-math"
            ]),
        ),
        (
            "/content~1index.md/0/result/bindings/0/occurrence/location/span",
            json!([0, 3]),
        ),
    ] {
        let mut invalid = retained.clone();
        *invalid.pointer_mut(pointer).unwrap() = positional;
        request["retained_math"] = invalid;
        assert!(decode(Route::Review, request.clone()).is_err());
    }
}

#[test]
fn named_discovery_accepts_native_limits_and_only_its_projection_options() {
    for route in [Route::Inspect, Route::Review, Route::Registry] {
        let tool = serde_json::to_value(route.descriptor()).expect("tool");
        let schema = jsonschema::validator_for(&tool["inputSchema"]).expect("discovery schema");
        for extra in [
            None,
            Some("review"),
            Some("retained_math"),
            Some("registry"),
            Some("unknown"),
            Some("allow_network"),
        ] {
            let mut value = args();
            if let Some(extra) = extra {
                value[extra] = json!({});
            }
            let accepted = extra.is_none()
                || matches!(
                    (route, extra),
                    (Route::Review, Some("review" | "retained_math"))
                        | (Route::Registry, Some("registry"))
                );
            assert_eq!(
                schema.is_valid(&value),
                accepted,
                "{}: {value}",
                route.name()
            );
            assert_eq!(decode(route, value).is_ok(), accepted, "{}", route.name());
        }
        let mut value = args();
        value["syntax"] = json!(toml::Limits::default());
        value["acquisition"] = json!(default_acquisition());
        value["corpus"] = json!(corpus::Limits::default());
        value["graph"] = json!(content::Limits::default());
        assert!(schema.is_valid(&value));
        assert!(decode(route, value).is_ok());
        assert_eq!(tool["annotations"]["readOnlyHint"], true);
        assert_eq!(tool["annotations"]["destructiveHint"], false);
        assert_eq!(tool["annotations"]["openWorldHint"], true);
    }
}

#[test]
fn explicit_permission_precedes_even_invalid_supplied_configuration() {
    for route in [Route::Inspect, Route::Review, Route::Registry] {
        let schema = serde_json::to_value(route.descriptor()).expect("tool");
        let validator = jsonschema::validator_for(&schema["outputSchema"]).expect("output schema");
        for allow in [None, Some(false), Some(true)] {
            let mut value = args();
            if let Some(allow) = allow {
                value["allow_read"] = json!(allow);
            }
            let output = decode(route, value)
                .expect("arguments")
                .execute(8192)
                .expect("tool result");
            assert_eq!(output.is_error, Some(true));
            let value = output.structured_content.expect("native error");
            validator.validate(&value).expect("error shape");
            if allow == Some(true) {
                assert_ne!(value["code"], "publication_permission");
            } else {
                assert_eq!(value["code"], "publication_permission");
            }
        }
    }
}

#[test]
fn nested_limits_refuse_positional_null_unknown_and_oversized_values() {
    for route in [Route::Inspect, Route::Review, Route::Registry] {
        let tool = serde_json::to_value(route.descriptor()).expect("tool");
        let schema = jsonschema::validator_for(&tool["inputSchema"]).expect("schema");
        let mut paths = vec![
            "/syntax",
            "/acquisition",
            "/acquisition/per_source",
            "/acquisition/per_source/content",
            "/corpus",
            "/corpus/document",
            "/corpus/document/metadata",
            "/corpus/document/markdown",
            "/corpus/notebook",
            "/graph",
        ];
        match route {
            Route::Inspect => (),
            Route::Review => paths.extend(["/review", "/review/graph"]),
            Route::Registry => paths.push("/registry"),
            Route::Reconcile => paths.push("/options"),
        }
        for path in paths {
            for invalid in [Value::Null, json!([]), json!({"unknown":1})] {
                let mut value = args();
                value["acquisition"] = json!(default_acquisition());
                value["corpus"] = json!(corpus::Limits::default());
                if matches!(route, Route::Review) {
                    value["review"] = json!(build::ReviewLimits::default());
                }
                let (parent, field) = path.rsplit_once('/').expect("field");
                value.pointer_mut(parent).expect("parent")[field] = invalid;
                assert!(
                    !schema.is_valid(&value),
                    "{} accepted {path}: {value}",
                    route.name()
                );
                assert!(
                    decode(route, value).is_err(),
                    "{} accepted {path}",
                    route.name()
                );
            }
        }
        let mut value = args();
        value["corpus"] = json!({"files": serde_json::from_str::<Value>(&((usize::MAX as u128)+1).to_string()).expect("large number")});
        assert!(!schema.is_valid(&value));
        assert!(decode(route, value).is_err());
    }
}

fn reconciliation_args() -> Value {
    json!({
        "source":"invalid TOML [", "site":"site:one", "allow_read":true,
        "root":"/unopened-private-registry",
        "expected_header": {
            "schema_version":1,"journal_id":vec![0;32],"journal_epoch":1,"writer_fence":0
        },
        "options": build::RegistryReconcileOptions::default()
    })
}

#[test]
fn reconciliation_discovery_and_strict_options_describe_real_mutation() {
    let descriptor = serde_json::to_value(Route::Reconcile.descriptor()).expect("descriptor");
    assert_eq!(descriptor["name"], "reconcile_registry");
    assert_eq!(descriptor["annotations"]["readOnlyHint"], false);
    assert_eq!(descriptor["annotations"]["destructiveHint"], true);
    assert_eq!(descriptor["annotations"]["idempotentHint"], false);
    let schema = jsonschema::validator_for(&descriptor["inputSchema"]).expect("schema");
    assert!(schema.is_valid(&reconciliation_args()));
    for path in [
        "/expected_header",
        "/options",
        "/options/projection",
        "/options/registry",
        "/options/writer",
        "/options/writer/session",
        "/options/writer/session/read",
    ] {
        for invalid in [Value::Null, json!([]), json!({"unknown":true})] {
            let mut value = reconciliation_args();
            *value.pointer_mut(path).expect("native nested option") = invalid;
            assert!(!schema.is_valid(&value), "schema accepted {path}");
            assert!(
                decode(Route::Reconcile, value).is_err(),
                "decoder accepted {path}"
            );
        }
    }
    let result = decode(Route::Reconcile, reconciliation_args())
        .expect("decode")
        .execute(8192)
        .expect("refusal before opening nonexistent path");
    assert_eq!(
        result.structured_content.expect("error")["code"],
        "store_registry_permission"
    );
    let mut value = reconciliation_args();
    value["options"]["registry"]["allow_replace"] = json!(true);
    value["options"]["writer"]["allow_generation_advance"] = json!(true);
    value["options"]["writer"]["session"]["allow_local_reservation"] = json!(true);
    value["options"]["writer"]["session"]["read"]["allow_coordination_files"] = json!(true);
    let result = decode(Route::Reconcile, value)
        .expect("decode")
        .execute(1024)
        .expect("bounded failure");
    assert_eq!(
        result.structured_content.expect("error")["code"],
        "registry_response_limit"
    );
}

#[test]
fn shared_registry_delivery_preserves_each_native_failure_phase_and_close_receipt() {
    use build::registry_output::{Reservation, report};
    let receipt = || store::RegistryReceipt {
        journal: store::JournalHeader {
            schema_version: 1,
            journal_id: [3; 32],
            journal_epoch: 7,
            writer_fence: 8,
        },
        registry: store::RegistryHead {
            semantic_sha256: source::Sha256Digest::of(b"semantic"),
            configuration_sha256: source::Sha256Digest::of(b"config"),
            rows_sha256: source::Sha256Digest::of(b"rows"),
            rows: 11,
        },
    };
    let original = serde_json::to_value(receipt()).expect("native receipt");
    let results = [
        Ok(receipt()),
        Err(build::RegistryReconcileError::Policy(
            store::Error::RegistryPermission,
        )),
        Err(build::RegistryReconcileError::Acquire(
            store::WriterAcquireError {
                cause: store::Error::InvalidLimits,
                generation: store::GenerationEffect::Unverified,
            },
        )),
        Err(build::RegistryReconcileError::Write(
            store::RegistryWriteError {
                cause: store::Error::InvalidLimits,
                effect: store::RegistryEffect::Unverified,
            },
        )),
        Err(build::RegistryReconcileError::Close {
            receipt: Box::new(receipt()),
            cause: store::Error::InvalidLimits,
        }),
    ];
    let descriptor = serde_json::to_value(Route::Reconcile.descriptor()).expect("descriptor");
    let schema = jsonschema::validator_for(&descriptor["outputSchema"]).expect("schema");
    for result in &results {
        let encoded = Reservation::new(8192)
            .expect("preflight")
            .finish(result)
            .expect("full report");
        assert_eq!(encoded.is_error, result.is_err());
        let value: Value = serde_json::from_slice(&encoded.bytes).expect("exact JSON");
        assert_eq!(
            value,
            serde_json::to_value(report(result)).expect("shared projection")
        );
        schema.validate(&value).expect("native outcome schema");
        match value["phase"].as_str() {
            Some("acquire") => assert_eq!(value["generation"], "Unverified"),
            Some("write") => assert_eq!(value["effect"], "Unverified"),
            Some("close") => assert_eq!(value["receipt"], original),
            _ => (),
        }
    }
    assert!(Reservation::new(0).is_err());
    assert!(Reservation::new(1024).is_err());
    assert!(Reservation::new((16 << 20) + 1).is_err());
    let (mut lower, mut upper) = (1, 8192);
    while lower < upper {
        let middle = lower + (upper - lower) / 2;
        if Reservation::new(middle).is_ok() {
            upper = middle;
        } else {
            lower = middle + 1;
        }
    }
    assert!(Reservation::new(lower - 1).is_err());
    for result in &results {
        let encoded = Reservation::new(lower)
            .expect("exact complete-shape capacity")
            .finish(result)
            .expect("original effect fits reserved shape");
        assert!(encoded.bytes.len() <= lower);
        assert!(
            encoded.bytes.capacity() > lower,
            "newline capacity also reserved before effects"
        );
    }
}
