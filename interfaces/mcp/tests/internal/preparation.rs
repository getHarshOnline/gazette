//! Discovery, bounded delivery and native guarded preparation through the protocol projection.

use super::*;
use serde_json::json;
use std::{fs, os::unix::fs::PermissionsExt};

fn recipe() -> Value {
    json!({
        "workspace":{"kind":"supplied","source":""}, "journal":"/not-opened/gazette-journal",
        "expected_header":{"schema_version":1,"journal_id":vec![0_u8;32],"journal_epoch":u64::MAX,"writer_fence":u64::MAX},
        "selection":{"content":{},"assets":{}}, "disclosure":{"rules":{}},
        "candidate_check":null,
        "coordinator":null,
        "sites":[{
            "render":{"site":"site:one","model_output":"model.json","graph":null},
            "stage_parent":"/not-opened/gazette-stages", "stage_name":"one",
            "preparation_id":source::Sha256Digest::of(b"one"),
            "metadata":{"author":{"name":"Fixture","email":"fixture@example.invalid","seconds":100,"offset_minutes":0},"committer":{"name":"Fixture","email":"fixture@example.invalid","seconds":100,"offset_minutes":0},"parent":null,"message":"Local fixture"},
            "parent_source":null
        }]
    })
}

fn arguments(recipe: &Value) -> JsonObject {
    json!({"recipe_json":recipe.to_string()})
        .as_object()
        .expect("named arguments")
        .clone()
}

fn qualification() -> Value {
    // Scanner coordinates come from the real native owner, not a second copy
    // of its evolving decoder/limit profile. The other values below remain
    // deliberately unverified transport fixtures.
    let output = source::CapturedSource::from_parts(
        source::InventoryBuilder::new(Default::default()).finish(),
        Default::default(),
    )
    .expect("empty retained output");
    let scan = json!(
        build::probe_disclosure(
            &output,
            &Default::default(),
            &std::collections::BTreeMap::from([(
                "rule:fixture".parse().expect("rule"),
                "private-fixture-token".into(),
            )]),
            Default::default(),
        )
        .expect("native scanner coordinates")
    );
    let policy =
        json!({"id":"policy:fixture", "revision":"revision:one", "source_sha256":"11".repeat(32)});
    json!({
        "scanner":scan["scanner"], "encoded_profile":scan["encoded_profile"],
        "filename_profile":scan["filename_profile"],
        "generic_rules_sha256":"22".repeat(32),
        "resource_profile":scan["resource_profile"],
        "resource_limits_sha256":scan["resource_limits_sha256"],
        "exclusion_profile":"gazette-zero-output-exclusion/0.1", "removed":[],
        "disclosure_policy_sha256":"34".repeat(32),
        "repository_source_provenance_sha256":"44".repeat(32), "rendering_sha256":"55".repeat(32),
        "runtime_build_profile":{"engine_version":"0.1.0","build_profile":"release","compilation_target":"x86_64-unknown-linux-gnu"},
        "output_policy":{"policy":policy, "command_sha256":"66".repeat(32), "response_bytes":8192},
        "candidate_policy":{"policy":policy, "command_sha256":"77".repeat(32)}
    })
}

// Exercise the actual three shared wire owners, without opening a journal or source.
fn qualification_inputs(qualification: &Value) -> [Value; 3] {
    let mut preparation = recipe();
    preparation["sites"][0]["qualification"] = qualification.clone();
    let admission = json!({"kind":"authored", "qualification":qualification});
    let selection = json!({
        "preparation_id":"88".repeat(32), "site":"site:one", "domain":"one.example",
        "commit":"aa".repeat(20), "expected_remote_oid_or_unborn":{"state":"unborn"},
        "qualification":admission
    });
    let operation = json!({
        "operation":"register", "expected":preparation["expected_header"],
        "input":{"authorization":"exact unverified authorization", "selections":[selection]}
    });
    [preparation, selection, operation]
}

fn qualification_decoders(bytes: &[String; 3]) -> [bool; 3] {
    [
        build::LocalPreparationRecipe::parse_json(bytes[0].as_bytes(), 16384, 1).is_ok(),
        gazette_cli::sdk::release::parse_preparation_selection(bytes[1].as_bytes()).is_ok(),
        gazette_cli::sdk::release::session::parse_operation(bytes[2].as_bytes()).is_ok(),
    ]
}

#[test]
fn candidate_qualification_discovery_and_shared_transports_require_named_optional_objects() {
    let preparation = json!(descriptor());
    let inspection = json!(crate::preparation_inspection::Route::Inspect.descriptor());
    let session = json!(crate::release_session::Route::Operate.descriptor());
    let recipe_schema = jsonschema::validator_for(
        &preparation["inputSchema"]["properties"]["recipe_json"]["contentSchema"],
    )
    .expect("native preparation schema");
    let selection_schema = jsonschema::validator_for(
        &inspection["inputSchema"]["properties"]["selections_json"]["contentSchema"],
    )
    .expect("native selection-array schema");
    let session_schema =
        jsonschema::validator_for(&session["inputSchema"]).expect("native held-operation schema");
    let check = |qualification: &Value, accepted: bool| {
        let values = qualification_inputs(qualification);
        assert_eq!(
            qualification_decoders(&values.clone().map(|v| v.to_string())),
            [accepted; 3],
            "{qualification}"
        );
        assert_eq!(recipe_schema.is_valid(&values[0]), accepted);
        assert_eq!(selection_schema.is_valid(&json!([values[1]])), accepted);
        assert_eq!(
            session_schema.is_valid(&json!({"session":"unopened", "operation":values[2]})),
            accepted
        );
    };
    let complete = qualification();
    check(&complete, true);
    let mut obsolete_engine_name = complete.clone();
    let source = obsolete_engine_name
        .as_object_mut()
        .unwrap()
        .remove("repository_source_provenance_sha256")
        .unwrap();
    obsolete_engine_name
        .as_object_mut()
        .unwrap()
        .insert("engine_source_sha256".into(), source);
    check(&obsolete_engine_name, false);
    let mut omitted = complete.clone();
    omitted.as_object_mut().unwrap().remove("candidate_policy");
    check(&omitted, true);
    for (path, value) in [
        ("/candidate_policy", Value::Null),
        ("/candidate_policy", json!([])),
        (
            "/candidate_policy",
            json!([complete["candidate_policy"]["policy"], "77".repeat(32)]),
        ),
        ("/candidate_policy/policy", Value::Null),
        (
            "/candidate_policy/policy",
            json!(["policy:fixture", "revision:one", "11".repeat(32)]),
        ),
    ] {
        let mut invalid = complete.clone();
        *invalid.pointer_mut(path).unwrap() = value;
        check(&invalid, false);
    }
    for path in ["/candidate_policy", "/candidate_policy/policy"] {
        let mut invalid = complete.clone();
        invalid
            .pointer_mut(path)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unknown".into(), true.into());
        check(&invalid, false);
    }
    // JSON Schema cannot represent duplicate-key rejection. Preserve raw bytes
    // through each real decoder, including a decoded Unicode namesake.
    let encoded = qualification_inputs(&complete).map(|v| v.to_string());
    for key in ["command_sha256", "command_sh\\u0061256"] {
        let duplicates = encoded.clone().map(|value| {
            let duplicate = value.replacen(
                "\"candidate_policy\":{",
                &format!("\"candidate_policy\":{{\"{key}\":\"{}\",", "77".repeat(32)),
                1,
            );
            assert_ne!(duplicate, value);
            duplicate
        });
        assert_eq!(qualification_decoders(&duplicates), [false; 3]);
    }
}

fn checked(result: CallToolResult) -> Value {
    let descriptor = serde_json::to_value(descriptor()).expect("discovery");
    let value = result
        .structured_content
        .expect("complete structured result");
    jsonschema::validator_for(&descriptor["outputSchema"])
        .expect("resolvable native output schema")
        .validate(&value)
        .expect("actual output matches native schema");
    let wire = serde_json::to_value(result.content).expect("readable content");
    let text: Value = serde_json::from_str(wire[0]["text"].as_str().expect("text")).expect("JSON");
    assert_eq!(text, value);
    value
}

#[test]
fn discovery_describes_exact_native_input_and_independent_effects() {
    let tool = serde_json::to_value(descriptor()).expect("discovery");
    assert_eq!(tool["name"], "prepare_sites");
    assert_eq!(tool["annotations"]["readOnlyHint"], false);
    assert_eq!(tool["annotations"]["destructiveHint"], true);
    assert_eq!(tool["annotations"]["idempotentHint"], false);
    assert_eq!(tool["annotations"]["openWorldHint"], true);
    assert!(
        tool["description"]
            .as_str()
            .expect("tool description")
            .contains("NOT sandboxed")
    );
    let schema = &tool["inputSchema"]["properties"]["recipe_json"]["contentSchema"];
    let validator = jsonschema::validator_for(schema).expect("self-contained native recipe schema");
    let valid = recipe();
    validator
        .validate(&valid)
        .expect("complete native JSON, full u64 coordinates");
    build::LocalPreparationRecipe::parse_json(valid.to_string().as_bytes(), 8192, 1)
        .expect("native decoder agrees with discovery");
    for path in [
        "",
        "/absolute",
        "a//b",
        "a/../b",
        "a\\b",
        "a\n",
        "a\u{0080}",
    ] {
        let mut invalid = valid.clone();
        invalid["sites"][0]["render"]["model_output"] = json!(path);
        assert!(
            !validator.is_valid(&invalid),
            "invalid native entry: {path:?}"
        );
        assert!(
            build::LocalPreparationRecipe::parse_json(invalid.to_string().as_bytes(), 8192, 1)
                .is_err()
        );
    }
    for (parent, field) in [
        ("", "candidate_check"),
        ("", "coordinator"),
        ("/sites/0", "parent_source"),
        ("/sites/0/render", "graph"),
        ("/sites/0/metadata", "parent"),
    ] {
        let mut missing = valid.clone();
        missing
            .pointer_mut(parent)
            .expect("selected parent")
            .as_object_mut()
            .expect("named parent object")
            .remove(field);
        assert!(
            !validator.is_valid(&missing),
            "nullable is explicit, not optional: {field}"
        );
        assert!(
            build::LocalPreparationRecipe::parse_json(missing.to_string().as_bytes(), 8192, 1)
                .is_err()
        );
    }
    let value = checked(
        decode(arguments(&valid))
            .expect("valid argument shape")
            .execute(8192)
            .expect("complete protocol result"),
    );
    assert_eq!(value["status"], "not-started");
    assert_eq!(value["code"], "local_preparation_permission");
}

#[test]
fn malformed_options_and_duplicate_recipe_keys_are_not_effect_permissions() {
    for bad in [
        json!({"recipe_json":recipe().to_string(),"read":[]}),
        json!({"recipe_json":recipe().to_string(),"output":[]}),
        json!({"recipe_json":recipe().to_string(),"read":{"unexpected":true}}),
        json!({"recipe_json":recipe().to_string(),"allow_read":"true"}),
        json!({"recipe_json":recipe().to_string(),"write_coordinator_anchor":"true"}),
        json!({"recipe_json":recipe().to_string(),"allow_anchor_write":true}),
        json!({"recipe_json":recipe().to_string(),"allow_publish":true}),
    ] {
        assert!(decode(bad.as_object().expect("named malformed arguments").clone()).is_err());
    }
    let text = recipe()
        .to_string()
        .replacen("{", "{\"journal\":\"/other\",", 1);
    let args = json!({"recipe_json":text,"allow_read":true});
    let value = checked(
        decode(args.as_object().expect("named arguments").clone())
            .expect("valid transport shape")
            .execute(8192)
            .expect("complete protocol result"),
    );
    assert_eq!(value["code"], "local_preparation_input");
    assert!(value.get("sites").is_none());
}

#[test]
fn insufficient_reconciliation_budget_fails_before_native_execution() {
    let mut args = arguments(&recipe());
    args.insert("allow_read".into(), true.into());
    let value = checked(
        decode(args)
            .expect("valid transport shape")
            .execute(512)
            .expect("complete refusal result"),
    );
    assert_eq!(value["code"], "local_preparation_response_budget");
    assert!(
        value["message"]
            .as_str()
            .expect("refusal explanation")
            .contains("No preparation started")
    );
}

#[test]
fn native_and_uncertain_delivery_reports_share_resolvable_discovery() {
    let recipe =
        build::LocalPreparationRecipe::parse_json(recipe().to_string().as_bytes(), 8192, 1)
            .expect("native recipe");
    let mut attempts = 0;
    let value = checked(
        recipe
            .execute_with_delivery(Default::default(), |delivery, is_error| {
                attempts += 1;
                assert!(is_error);
                // The native permission refusal is real; simulate only carrier encoding loss.
                if attempts == 2 {
                    return Err(());
                }
                crate::tools::encode_complete(delivery, is_error, 8192).map_err(|_| ())
            })
            .expect("reserved reconciliation survives native-report delivery loss"),
    );
    assert_eq!(attempts, 2);
    assert_eq!(value["status"], "reconciliation-required");
    assert_eq!(
        value["input_expected_header"]["writer_fence"],
        json!(u64::MAX)
    );
    let error = Err(build::LocalPreparationRunError::Before {
        code: "native-code",
        message: "native message".into(),
    });
    let view = build::LocalPreparationRunReport::new(&error);
    let value = checked(
        crate::tools::encode_complete(&view, true, 8192)
            .ok()
            .expect("complete native error encoding"),
    );
    assert_eq!(
        value,
        json!(error.as_ref().expect_err("expected native refusal"))
    );
}

fn private() -> tempfile::TempDir {
    tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .expect("private disposable native root")
}

fn session() -> store::SessionOptions {
    store::SessionOptions {
        allow_local_reservation: true,
        read: store::ReadOptions {
            allow_coordination_files: true,
            ..Default::default()
        },
    }
}

struct Guarded {
    source: tempfile::TempDir,
    _journal: tempfile::TempDir,
    anchor: tempfile::TempDir,
    stages: tempfile::TempDir,
    initialized: store::InitializationReceipt,
    seal: store::coordinator::LocalCoordinatorSeal,
    request: Value,
}

impl Guarded {
    fn new() -> Self {
        let source = private();
        let repository = git2::Repository::init(source.path()).expect("native fixture repository");
        for (path, bytes) in [
            (
                "theme/theme.toml",
                "package='theme:one'\nnamespace='one'\ntemplates='templates'\nbrowser_theme_colors={light='#ffffff',dark='#000000'}\nrender={page='page.html',alias='alias.html'}\n",
            ),
            (
                "theme/templates/page.html",
                "<h1>{{ content.title }}</h1>{{ body_html | safe }}",
            ),
            (
                "theme/templates/alias.html",
                "<a href=\"{{ route.canonical_url }}\">Moved</a>",
            ),
            (
                "authored/page.md",
                "+++\n[content]\nid='content:one'\nsite='site:one'\ntitle='Retained page'\nlifecycle='published'\nroute={path='/'}\nstructure={kind='page',family={kind='article'}}\n+++\n# Exact retained body\n",
            ),
        ] {
            let path = source.path().join(path);
            fs::create_dir_all(path.parent().expect("source parent")).expect("fixture directories");
            fs::write(path, bytes).expect("fixture source bytes");
        }
        let mut index = repository.index().expect("native fixture index");
        index
            .add_all(["theme", "authored"], git2::IndexAddOption::DEFAULT, None)
            .expect("exact fixture entries");
        let tree = repository
            .find_tree(index.write_tree().expect("tree OID"))
            .expect("native tree");
        let signature =
            git2::Signature::new("Fixture", "fixture@example.invalid", &git2::Time::new(0, 0))
                .expect("fixed fixture signature");
        let commit = repository
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                "retained fixture",
                &tree,
                &[],
            )
            .expect("native fixture commit");
        let workspace = format!(
            "[composition]\ntheme={{package='theme:one'}}\n\
             [[principals]]\nid='owner:one'\nkind='person'\n\
             [[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n\
             [[brands]]\nid='brand:one'\nname='Explicit fixture'\n\
             [[brands.publications]]\nsite='site:one'\npublic_base_url='https://one.example/'\nauthored=[{{source='source:authored',target='authored'}}]\n\
             [[sources]]\nid='source:theme'\nlocation={{kind='repository',root={root:?},commit='{commit}',subdirectory='theme'}}\n\
             [[sources]]\nid='source:authored'\nlocation={{kind='repository',root={root:?},commit='{commit}',subdirectory='authored'}}\n\
             [[theme_packages]]\npackage='theme:one'\nsource='source:theme'\nmanifest='theme.toml'\n",
            root = source.path().to_str().expect("native UTF-8 fixture path"),
        );
        let journal = private();
        let anchor = private();
        let stages = private();
        let initialized = store::initialize(
            journal.path(),
            store::InitializeOptions {
                allow_create: true,
                session: session(),
                ..Default::default()
            },
        )
        .expect("fresh native SQLite journal");
        let seal = store::coordinator::enroll(
            store::coordinator::EnrollmentRequest {
                anchor_root: anchor.path(),
                journal_root: journal.path(),
                coordinator_id: "coordinator local-Éxact",
                principal_scope: source::Sha256Digest::of(b"explicit fixture principal scope"),
                protected_roots: &[source.path().to_owned(), stages.path().to_owned()],
            },
            session(),
            true,
        )
        .expect("fresh native enrollment before preparation");
        let mut request = recipe();
        request["workspace"] = json!({"kind":"supplied","source":workspace});
        request["journal"] = json!(seal.binding.journal_root);
        request["expected_header"] = json!(seal.header);
        request["coordinator"] = json!({"anchor_root":anchor.path(), "binding":seal.binding});
        request["selection"]["content"] = json!({"content:one":"include"});
        request["sites"][0]["stage_parent"] = json!(stages.path());
        for (class, value) in [
            ("local-path", "/private/fixture-never-publish-92731"),
            ("private-endpoint", "private-fixture-92731.internal.invalid"),
            ("secret", "fixture-secret-92731"),
            ("generated-artifact", "private-artifact-92731"),
            ("fixture-claim", "UNAPPROVED FIXTURE CLAIM 92731"),
            ("internal-identifier", "internal-record-92731"),
        ] {
            request["disclosure"]["rules"][format!("rule:{class}")] =
                json!({"class":class,"pattern":{"kind":"literal","value":value}});
        }
        Self {
            source,
            _journal: journal,
            anchor,
            stages,
            initialized,
            seal,
            request,
        }
    }

    fn arguments(&self) -> JsonObject {
        let mut args = arguments(&self.request);
        for permission in [
            "allow_read",
            "create_stage",
            "record_preparation",
            "advance_writer_generation",
            "reserve_local_coordination",
            "initialize_worktree",
            "write_coordinator_anchor",
        ] {
            args.insert(permission.into(), true.into());
        }
        args.insert("read".into(), json!(session().read));
        args
    }

    fn untouched(&self, database: &[u8], anchor: &[u8]) {
        assert_eq!(
            fs::read(&self.initialized.database).expect("unchanged database"),
            database
        );
        assert_eq!(
            fs::read(self.anchor.path().join("coordinator.json")).expect("unchanged anchor"),
            anchor
        );
        assert_eq!(
            fs::read_dir(self.stages.path())
                .expect("stage inventory")
                .count(),
            0
        );
    }
}

#[test]
fn guarded_mcp_preparation_seals_actual_history_and_preserves_selection_on_response_overflow() {
    for maximum in [1 << 20, 2048] {
        let fixture = Guarded::new();
        let source_before =
            fs::read(fixture.source.path().join("authored/page.md")).expect("source before");
        let repository = git2::Repository::open(fixture.source.path()).expect("source repository");
        let head_before = repository.head().expect("source HEAD").target();
        let args = fixture.arguments();
        let tool = json!(descriptor());
        jsonschema::validator_for(&tool["inputSchema"])
            .expect("input schema")
            .validate(&json!(args))
            .expect("actual named permissions");
        jsonschema::validator_for(
            &tool["inputSchema"]["properties"]["recipe_json"]["contentSchema"],
        )
        .expect("native recipe schema")
        .validate(&fixture.request)
        .expect("native selected binding shape");
        let result = decode(args)
            .expect("transport input")
            .execute(maximum)
            .expect("complete protocol result");
        assert_eq!(result.is_error, Some(maximum == 2048));
        let value = checked(result);
        let state =
            store::coordinator::inspect(fixture.anchor.path(), &fixture.seal.binding, session())
                .expect("independent native final anchor inspection");
        let store::coordinator::LocalCoordinatorState::Clean { seal, .. } = state else {
            panic!("successful native preparation must explicitly close every guarded writer")
        };
        assert!(seal.header.writer_fence > fixture.seal.header.writer_fence);
        assert_eq!(seal.binding, fixture.seal.binding);
        assert_eq!(
            seal.database_sha256,
            source::Sha256Digest::of(
                &fs::read(&fixture.initialized.database).expect("final native bytes")
            )
        );
        let record = store::inspect_preparation(
            &fixture.initialized.database,
            source::Sha256Digest::of(b"one"),
            Some(&seal.header),
            session().read,
        )
        .expect("independent native preparation inspection")
        .record
        .expect("complete durable record despite bounded delivery");
        let completion = record
            .worktree
            .as_ref()
            .expect("Git intent")
            .completion
            .as_ref()
            .expect("recorded Git completion");
        release_commit::inspect_journal_worktree(
            release_commit::JournalWorktreeInspectionRequest {
                database: &fixture.initialized.database,
                preparation_id: record.intent.id,
                expected_header: Some(&seal.header),
                commit: &completion.report.commit,
                protected_roots: &[fixture.source.path().to_owned()],
            },
            release_commit::JournalWorktreeInspectionOptions {
                read: session().read,
                ..Default::default()
            },
        )
        .expect("real retained output, journal and detached Git read-back");
        if maximum == 2048 {
            assert_eq!(value["status"], "reconciliation-required");
            assert_eq!(value["coordinator"], fixture.request["coordinator"]);
            assert_eq!(
                value["input_expected_header"],
                fixture.request["expected_header"]
            );
            assert_eq!(
                value["preparations"][0]["preparation_id"],
                json!(record.intent.id)
            );
        } else {
            assert_eq!(value["status"], "local-preparations-complete-not-certified");
            assert_eq!(
                value["sites"][0]["preparation"]["journal"],
                json!(seal.header)
            );
            assert_eq!(value["sites"][0]["preparation"]["record"], json!(record));
        }
        assert_eq!(
            fs::read(fixture.source.path().join("authored/page.md")).expect("retained source"),
            source_before
        );
        assert_eq!(
            repository.head().expect("unchanged source HEAD").target(),
            head_before
        );
    }
}

#[test]
fn selected_anchor_permission_identity_and_strict_shape_refuse_without_preparation_effects() {
    let fixture = Guarded::new();
    let database = fs::read(&fixture.initialized.database).expect("initial database");
    let anchor = fs::read(fixture.anchor.path().join("coordinator.json")).expect("initial seal");
    let mut denied = fixture.arguments();
    denied.remove("write_coordinator_anchor");
    let value = checked(
        decode(denied)
            .expect("denied option shape")
            .execute(1 << 20)
            .expect("refusal"),
    );
    assert_eq!(value["status"], "not-started");
    assert_eq!(value["code"], "local_preparation_preflight");
    fixture.untouched(&database, &anchor);
    let mut wrong = fixture.request.clone();
    wrong["coordinator"]["binding"]["principal_scope"] =
        json!(source::Sha256Digest::of(b"another scope"));
    let mut args = fixture.arguments();
    args.insert("recipe_json".into(), wrong.to_string().into());
    let result = decode(args)
        .expect("well-shaped wrong binding")
        .execute(1 << 20)
        .expect("native refusal");
    assert_eq!(result.is_error, Some(true));
    let value = checked(result);
    assert_ne!(value["status"], "local-preparations-complete-not-certified");
    fixture.untouched(&database, &anchor);
    let tool = json!(descriptor());
    let schema = jsonschema::validator_for(
        &tool["inputSchema"]["properties"]["recipe_json"]["contentSchema"],
    )
    .expect("native input schema");
    for (pointer, invalid) in [
        ("/coordinator", json!([])),
        ("/coordinator/binding", json!([])),
        ("/coordinator/binding", json!({"unknown":true})),
        (
            "/coordinator",
            json!({"anchor_root":"/anchor","binding":null}),
        ),
        ("/coordinator/binding/journal_root", json!("relative")),
    ] {
        let mut malformed = fixture.request.clone();
        *malformed.pointer_mut(pointer).expect("selected field") = invalid;
        assert!(!schema.is_valid(&malformed), "{pointer}");
        let mut args = fixture.arguments();
        args.insert("recipe_json".into(), malformed.to_string().into());
        let value = checked(
            decode(args)
                .expect("exact JSON string")
                .execute(1 << 20)
                .expect("native decode refusal"),
        );
        assert_eq!(value["code"], "local_preparation_input");
        fixture.untouched(&database, &anchor);
    }
    let duplicate = fixture.request.to_string().replacen(
        "\"coordinator_id\":",
        "\"coordinator_id\":\"duplicate\",\"coordinator_id\":",
        1,
    );
    assert_ne!(duplicate, fixture.request.to_string());
    let mut args = fixture.arguments();
    args.insert("recipe_json".into(), duplicate.into());
    let value = checked(
        decode(args)
            .expect("unmodified exact JSON string carrier")
            .execute(1 << 20)
            .expect("native duplicate-key refusal"),
    );
    assert_eq!(value["code"], "local_preparation_input");
    fixture.untouched(&database, &anchor);
}
