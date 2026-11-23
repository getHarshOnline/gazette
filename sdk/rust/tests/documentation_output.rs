//! Native retained-stage custody and exact bounded projections; no hosted-provider qualification.

#[path = "documentation_output/manual_selection.rs"]
mod manual_selection;

use gazette_core::{
    documentation::{Selection, generator},
    process, source, stage,
};
use gazette_ports::stage::StageWriter;
use gazette_sdk::build::{
    DocumentationError, DocumentationGenerationCause, DocumentationGenerationError,
    DocumentationGenerationOptions, DocumentationJob, PublicationSetAcquisitionLimits,
    documentation_output::{
        self, Bounds, Custody, Execution, GenerationSelection, Operation, PreparationSelection,
        Report, Reservation, RetentionSelection,
    },
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    os::unix::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
};

fn bounds() -> Bounds {
    Bounds {
        jobs: 2,
        directory_bytes: 512,
        entry_bytes: 128,
    }
}
const MAXIMUM: usize = 64 << 10;

fn minimum_reservation(selected: Bounds) -> usize {
    let (mut lower, mut upper) = (1, MAXIMUM);
    assert!(Reservation::new(selected, upper).is_ok());
    while lower < upper {
        let middle = lower + (upper - lower) / 2;
        if Reservation::new(selected, middle).is_ok() {
            upper = middle;
        } else {
            lower = middle + 1;
        }
    }
    assert!(Reservation::new(selected, lower).is_ok());
    assert!(Reservation::new(selected, lower - 1).is_err());
    lower
}

fn captured(bytes: &[u8]) -> source::CapturedSource {
    let path: source::EntryPath = "api.json".parse().expect("entry");
    let mut inventory = source::InventoryBuilder::new(Default::default());
    inventory
        .insert(
            path.clone(),
            source::Content::File {
                bytes: bytes.len() as u64,
                sha256: source::Sha256Digest::of(bytes),
            },
        )
        .expect("inventory");
    source::CapturedSource::from_parts(inventory.finish(), BTreeMap::from([(path, bytes.to_vec())]))
        .expect("exact capture")
}

fn retain(parent: &Path, capture: &source::CapturedSource) -> stage::RetainedStage {
    gazette_adapter_fs::FileSystem
        .stage(
            stage::Location::FreshBounded {
                parent,
                directory_bytes: bounds().directory_bytes,
            },
            &[],
            capture.as_input(),
            Default::default(),
        )
        .expect("real bounded native stage")
        .retain()
        .expect("native sync and read-back")
}

fn job(input: &stage::RetainedStage, output: &stage::RetainedStage) -> DocumentationJob {
    DocumentationJob {
        module: 0,
        reference: 1,
        input: input.path().to_owned(),
        output: output.path().to_owned(),
        output_identity: output.inventory().identity(),
        selection: Selection::File {
            path: "api.json".parse().expect("entry"),
        },
        content: source::Content::File {
            bytes: b"{\"api\":1}".len() as u64,
            sha256: source::Sha256Digest::of(b"{\"api\":1}"),
        },
        evidence: generator::Evidence {
            input: input.inventory().identity(),
            // These are explicit lower-boundary observations for projection tests, not an assertion
            // that a generator executable ran. Parent integration tests own native invocation proof.
            version: Some(generator::Invocation {
                command_sha256: source::Sha256Digest::of(b"version-command"),
                exit: Some(process::Exit { code: Some(0) }),
                stdout_sha256: Some(source::Sha256Digest::of(b"version")),
                stdout_bytes: Some(7),
            }),
            expected_version_sha256: source::Sha256Digest::of(b"version"),
            version_text: Some("version".into()),
            validation: None,
            generation: Some(generator::Invocation {
                command_sha256: source::Sha256Digest::of(b"generate-command"),
                exit: Some(process::Exit { code: Some(0) }),
                stdout_sha256: Some(source::Sha256Digest::of(&[])),
                stdout_bytes: Some(0),
            }),
        },
    }
}

fn decode(bytes: &[u8]) -> Value {
    let value: Value = serde_json::from_slice(bytes).expect("complete JSON");
    if value.get("code").and_then(Value::as_str) == Some("documentation_response_unavailable") {
        validate_schema(&value);
    }
    value
}
fn validate_schema(value: &Value) {
    jsonschema::validator_for(&Value::Object(documentation_output::schema()))
        .expect("native delivery schema")
        .validate(value)
        .expect("actual emitted representation agrees with discovery");
}
fn path(value: &Value) -> PathBuf {
    OsString::from_vec(serde_json::from_value::<Vec<u8>>(value.clone()).expect("Unix bytes")).into()
}

fn operation<'a>(
    input: &'a gazette_sdk::build::WorkspaceInput,
    site: &'a gazette_sdk::workspace::Key,
    generation: Option<GenerationSelection<'a>>,
    retention: RetentionSelection<'a>,
) -> Operation<'a> {
    Operation {
        preview_link_bases: None,
        input,
        site,
        syntax: Default::default(),
        acquisition: PublicationSetAcquisitionLimits::default().sources,
        output: Default::default(),
        generation,
        retention,
        response_bytes: MAXIMUM,
        reserve_read_only_response: false,
    }
}

fn before_code(execution: Execution<'_>) -> &'static str {
    match execution {
        Execution::Before(error) => error.code(),
        Execution::Complete { .. } | Execution::Delivered { .. } | Execution::Contract { .. } => {
            "unexpected_documentation_effect"
        }
    }
}

struct OperationFixture {
    workspace: tempfile::TempDir,
    source: tempfile::TempDir,
    configuration: String,
}

impl OperationFixture {
    fn new(brand: &str) -> Self {
        let workspace = tempfile::tempdir().expect("explicit configuration owner");
        let source = tempfile::tempdir().expect("separate native manual input");
        fs::write(
            source.path().join("start.md"),
            b"# Start\n\nRetained manual.\n",
        )
        .expect("actual authored manual");
        fs::create_dir(workspace.path().join("domain-doc")).expect("existing configured output");
        let configuration = format!(
            "[[principals]]\nid='person:one'\nkind='person'\n\
             [[sites]]\nid='site:one'\npublisher='person:one'\ndeployable=false\n\
             [[projects]]\nid='project:one'\nlifecycle='active'\nvisibility='public'\navailability='available'\n\
             [[sources]]\nid='source:manual'\nlocation={{kind='filesystem',root={:?}}}\n\
             [[brands]]\nid='brand:one'\nname={brand:?}\n\
             [[brands.publications]]\nsite='site:one'\n\
             [brands.publications.documentation_repository]\nname='native-docs'\npath='domain-doc'\n\
             [brands.publications.documentation_repository.build]\n\
             target={{kind='mintlify',theme='mint',primary_color='#123abc'}}\n\
             [[brands.publications.documentation_repository.build.modules]]\n\
             project='project:one'\nslug='manual'\ntitle='Manual'\nmanual={{source='source:manual'}}\n\
             navigation=[{{kind='manual',title='Guide',pages=[{{title='Start',route='manual/start',input={{kind='file',path='start.md'}}}}]}}]\n",
            source.path().to_str().expect("exact native source path"),
        );
        fs::write(workspace.path().join("gazette.toml"), &configuration)
            .expect("exact selected configuration bytes");
        Self {
            workspace,
            source,
            configuration,
        }
    }

    fn local(&self) -> gazette_sdk::build::WorkspaceInput {
        gazette_sdk::build::WorkspaceInput::Local {
            root: self.workspace.path().into(),
            workspace: "gazette.toml"
                .parse()
                .expect("exact selected root document"),
            limits: Default::default(),
        }
    }
}

#[test]
fn shared_workspace_selection_exposes_retained_assembly_without_inventing_native_coordinates() {
    let fixture = OperationFixture::new("Documentation");
    let site = "site:one".parse().expect("site");
    let landing_owned = fixture.configuration.replace(
        "lifecycle='active'\nvisibility='public'\navailability='available'",
        "landing='content:uncaptured-project'",
    ).replace(
        "[[brands.publications]]\nsite='site:one'",
        "[[brands.publications]]\nsite='site:one'\nauthored=[{source='source:unavailable-publication',target='writing'}]",
    ) + "\n[[sources]]\nid='source:unavailable-publication'\nlocation={kind='filesystem',root='/not-present/publication-content'}\n";
    for (input, local) in [
        (
            gazette_sdk::build::WorkspaceInput::Supplied {
                source: fixture.configuration.clone(),
            },
            false,
        ),
        (fixture.local(), true),
        (
            gazette_sdk::build::WorkspaceInput::Supplied {
                source: landing_owned,
            },
            false,
        ),
    ] {
        operation(&input, &site, None, RetentionSelection::None).execute_with_delivery(
            |execution| {
                let Execution::Complete { report, assembly } = execution else {
                    panic!("read-only complete native assembly")
                };
                assert!(
                    report.jobs.is_empty(),
                    "imported manual invents no generator jobs"
                );
                assert!(std::ptr::eq(
                    report.inventory,
                    assembly.output().inventory()
                ));
                assert_eq!(assembly.brand().as_str(), "brand:one");
                assert!(
                    assembly
                        .output()
                        .file(&"manual/start.md".parse().expect("output entry"))
                        .expect("real retained Mintlify output")
                        .windows(b"Retained manual".len())
                        .any(|bytes| bytes == b"Retained manual")
                );
                let planned = gazette_sdk::build::plan_documentation_output(
                    assembly,
                    None,
                    Default::default(),
                );
                if local {
                    assert_eq!(
                        planned.expect("selected native owner").root(),
                        fixture.workspace.path().join("domain-doc")
                    );
                } else {
                    assert!(matches!(
                        planned,
                        Err(gazette_sdk::build::OutputPlanError::Coordinate(_))
                    ));
                }
            },
        );
    }
    assert_eq!(
        fs::read_dir(fixture.workspace.path().join("domain-doc"))
            .expect("no output effect")
            .count(),
        0
    );
}

#[test]
fn publication_reservation_and_crawler_rules_reach_the_documentation_target() {
    let fixture = OperationFixture::new("Documentation");
    let site = "site:one".parse().expect("site");
    let reserved = fixture
        .configuration
        .replace(
            "[[brands.publications]]\nsite='site:one'\n",
            "[[brands.publications]]\nsite='site:one'\npublic_base_url='https://site.example.test/'\n\
             machine={robots={enabled=true,rules=[{user_agent='*',allow=['/']},{user_agent='TrainingBot',disallow=['/']}]},terms={path='/terms/',tdm_reservation=true}}\n",
        )
        .replace(
            "path='domain-doc'\n",
            "path='domain-doc'\npublic_base_url='https://docs.example.test/'\n",
        )
        .replace(
            "primary_color='#123abc'}",
            "primary_color='#123abc',telemetry=false}",
        );
    let input = gazette_sdk::build::WorkspaceInput::Supplied {
        source: reserved.clone(),
    };
    operation(&input, &site, None, RetentionSelection::None).execute_with_delivery(|execution| {
        let Execution::Complete { assembly, .. } = execution else {
            panic!("complete reserved assembly")
        };
        let file = |path: &str| {
            String::from_utf8(
                assembly
                    .output()
                    .file(&path.parse().expect("output entry"))
                    .expect("emitted documentation file")
                    .to_vec(),
            )
            .expect("UTF-8 output")
        };
        assert_eq!(
            file("robots.txt"),
            "User-agent: *\nAllow: /\n\nUser-agent: TrainingBot\nDisallow: /\n\nSitemap: https://docs.example.test/sitemap.xml\n"
        );
        assert_eq!(
            file(".well-known/tdmrep.json"),
            "[{\"location\":\"/*\",\"tdm-reservation\":1,\"tdm-policy\":\"https://site.example.test/terms/\"}]\n"
        );
        let config: Value = serde_json::from_str(&file("docs.json")).expect("docs.json");
        assert_eq!(
            config["seo"]["metatags"],
            json!({
                "canonical": "https://docs.example.test/",
                "tdm-reservation": "1",
                "tdm-policy": "https://site.example.test/terms/"
            })
        );
        assert_eq!(config["integrations"], json!({"telemetry": {"enabled": false}}));
    });
    // A reservation that cannot name its terms address fails rather than leaving
    // the provider's default crawler policy in place.
    let unresolved = gazette_sdk::build::WorkspaceInput::Supplied {
        source: reserved.replace("public_base_url='https://site.example.test/'\n", ""),
    };
    operation(&unresolved, &site, None, RetentionSelection::None).execute_with_delivery(
        |execution| {
            assert!(
                matches!(execution, Execution::Before(_)),
                "unresolved terms refuse documentation assembly"
            );
        },
    );
}

#[test]
fn preview_manual_projection_cannot_plan_maintained_output() {
    let fixture = OperationFixture::new("Documentation");
    let input = fixture.local();
    let site = "site:one".parse().unwrap();
    let bases = gazette_core::publication::routing::PreviewLinkBases::try_from(BTreeMap::from([(
        "https://publication.example.test/".into(),
        "http://127.0.0.1:4501/".parse().unwrap(),
    )]))
    .unwrap();
    let mut selected = operation(&input, &site, None, RetentionSelection::None);
    selected.preview_link_bases = Some(&bases);
    selected.execute_with_delivery(|execution| {
        let Execution::Complete { report, assembly } = execution else {
            panic!("complete private preview assembly")
        };
        assert_eq!(report.preview_link_bases, Some(&bases));
        assert_eq!(assembly.preview_link_bases(), Some(&bases));
        assert!(matches!(
            gazette_sdk::build::plan_documentation_output(assembly, None, Default::default()),
            Err(gazette_sdk::build::OutputPlanError::Preview)
        ));
    });
    assert_eq!(
        fs::read_dir(fixture.workspace.path().join("domain-doc"))
            .unwrap()
            .count(),
        0
    );

    let forbidden = fixture.workspace.path().join("not-created");
    let preparation = PreparationSelection {
        journal: forbidden.clone(),
        expected: gazette_sdk::store::JournalHeader {
            schema_version: 1,
            journal_id: [7; 32],
            journal_epoch: 1,
            writer_fence: 1,
        },
        parent: forbidden.clone(),
        name: "preview".into(),
        id: source::Sha256Digest::of(b"preview"),
        create_stage: false,
        record_preparation: false,
        writer: Default::default(),
        evidence_bytes: 1,
    };
    let mut selected = operation(
        &input,
        &site,
        None,
        RetentionSelection::Preparation {
            selection: &preparation,
            coordinator: None,
            allow_anchor_write: false,
        },
    );
    selected.preview_link_bases = Some(&bases);
    assert_eq!(
        selected.execute_with_delivery(before_code),
        "documentation_preview_preparation"
    );
    assert!(
        !forbidden.exists(),
        "preview refusal precedes journal or stage access"
    );
}

#[test]
fn fully_reserved_delivery_borrows_same_native_assembly_and_exact_jobs_for_follow_on_planning() {
    let fixture = OperationFixture::new("Documentation");
    let site = "site:one".parse().expect("site");
    let input = fixture.local();
    let mut request = operation(&input, &site, None, RetentionSelection::None);
    request.reserve_read_only_response = true;
    request.execute_with_delivery(|execution| {
        let Execution::Delivered {
            response,
            assembly,
            jobs,
        } = execution
        else {
            panic!("pre-reserved full native delivery")
        };
        assert!(!response.is_error);
        assert!(jobs.is_empty());
        let assembly = assembly.expect("only fully delivered success exposes assembly");
        let report = decode(&response.bytes);
        validate_schema(&report);
        assert_eq!(report["inventory"], json!(assembly.output().inventory()));
        assert_eq!(report["manual"], json!(assembly.manual()));
        assert_eq!(report["jobs"], json!(jobs));
        fs::write(
            fixture.source.path().join("start.md"),
            b"changed after native assembly",
        )
        .expect("current disk need not match retained input");
        let plan =
            gazette_sdk::build::plan_documentation_output(assembly, None, Default::default())
                .expect("callback uses exact retained assembly without reacquisition");
        assert_eq!(plan.generated().inventory(), assembly.output().inventory());
    });
    assert_eq!(
        fs::read_dir(fixture.workspace.path().join("domain-doc"))
            .expect("write-free callback")
            .count(),
        0
    );
}

#[test]
fn compact_success_overflow_and_native_retention_failure_never_expose_assembly() {
    let readonly_maximum = minimum_reservation(Bounds {
        jobs: 0,
        directory_bytes: 1,
        entry_bytes: 1,
    });
    for overflow in [true, false] {
        let brand = if overflow {
            "large-brand".repeat(3000)
        } else {
            "Documentation".into()
        };
        let fixture = OperationFixture::new(&brand);
        assert!(fixture.configuration.len() <= gazette_sdk::toml::Limits::default().source_bytes);
        let site = "site:one".parse().expect("site");
        let input = fixture.local();
        let absent = fixture.workspace.path().join("absent-stage-parent");
        let retention = if overflow {
            RetentionSelection::None
        } else {
            RetentionSelection::Stage {
                parent: &absent,
                directory_bytes: 512,
            }
        };
        if overflow {
            operation(&input, &site, None, RetentionSelection::None).execute_with_delivery(
                |execution| {
                    let Execution::Complete { report, .. } = execution else {
                        panic!("valid native source must reach read-only assembly")
                    };
                    assert!(
                        serde_json::to_vec(&report)
                            .expect("actual complete native report")
                            .len()
                            > readonly_maximum,
                        "real report exceeds the exact valid custody reservation"
                    );
                },
            );
        }
        if !overflow {
            // Six worst-case native path arrays already exceed 8 KiB at this bound.
            // That is a genuine pre-effect reservation refusal, not stage-failure custody.
            let mut denied = operation(&input, &site, None, retention);
            denied.response_bytes = 8 << 10;
            denied.execute_with_delivery(|execution| {
                let Execution::Before(documentation_output::BeforeError::Response(code)) =
                    execution
                else {
                    panic!("tight complete custody must refuse before native retention")
                };
                assert_eq!(code, "documentation_response_limit");
            });
            assert!(!absent.exists());
        }
        let mut request = operation(&input, &site, None, retention);
        request.reserve_read_only_response = true;
        // Use the actual read-only witness minimum, not an arbitrary undersized quota.
        request.response_bytes = if overflow { readonly_maximum } else { MAXIMUM };
        request.execute_with_delivery(|execution| {
            let Execution::Delivered {
                response,
                assembly,
                jobs,
            } = execution
            else {
                match execution {
                    Execution::Before(error) => panic!("unexpected pre-effect refusal: {error:?}"),
                    Execution::Contract { custody } => {
                        panic!("unexpected custody contract: {custody:?}")
                    }
                    Execution::Complete { .. } => {
                        panic!("reserved operation returned unreserved completion")
                    }
                    Execution::Delivered { .. } => unreachable!("matched above"),
                }
            };
            assert!(response.is_error);
            assert!(assembly.is_none());
            assert!(jobs.is_empty());
            let value = decode(&response.bytes);
            assert_eq!(value["code"], "documentation_response_unavailable");
            assert!(
                !value["output_identity"].is_null(),
                "completed assembly remains exact custody"
            );
            assert!(value["target_unix_bytes"].is_null());
            assert_eq!(value["failure"]["code"].is_null(), overflow);
        });
        assert!(!absent.exists());
        assert_eq!(
            fs::read_dir(fixture.workspace.path().join("domain-doc"))
                .expect("no configured write")
                .count(),
            0
        );
    }
}

#[test]
fn operation_derives_effect_bounds_and_rejects_invalid_selections_before_native_work() {
    let site = "site:one".parse().expect("site key");
    let input = gazette_sdk::build::WorkspaceInput::Supplied {
        source: "this is not workspace TOML".into(),
    };

    let generation_parent = tempfile::tempdir().expect("private generator parent");
    let generation = GenerationSelection {
        parent: generation_parent.path(),
        options: DocumentationGenerationOptions {
            input: Default::default(),
            output: Default::default(),
            execution: process::Options {
                allow_execute: true,
                ..Default::default()
            },
            max_capture_bytes: 1 << 20,
            max_jobs: 1,
            directory_bytes: 0,
            entry_bytes: 128,
        },
    };
    let code = operation(&input, &site, Some(generation), RetentionSelection::None)
        .execute_with_delivery(before_code);
    assert_eq!(code, "documentation_response_limits");
    assert_eq!(
        fs::read_dir(generation_parent.path())
            .expect("generator parent")
            .count(),
        0,
        "invalid generation bounds cannot create native stages"
    );

    let stage_parent = tempfile::tempdir().expect("private target parent");
    let code = operation(
        &input,
        &site,
        None,
        RetentionSelection::Stage {
            parent: stage_parent.path(),
            directory_bytes: 0,
        },
    )
    .execute_with_delivery(before_code);
    assert_eq!(code, "documentation_response_limits");
    assert_eq!(
        fs::read_dir(stage_parent.path())
            .expect("target parent")
            .count(),
        0,
        "invalid target bounds cannot create a retained target"
    );

    let preparation_root = tempfile::tempdir().expect("private preparation fixture");
    let selection = PreparationSelection {
        journal: preparation_root.path().into(),
        expected: gazette_sdk::store::JournalHeader {
            schema_version: 1,
            journal_id: [7; 32],
            journal_epoch: 1,
            writer_fence: 1,
        },
        parent: preparation_root.path().into(),
        name: "documentation-stage".into(),
        id: source::Sha256Digest::of(b"documentation-preparation"),
        create_stage: false,
        record_preparation: false,
        writer: Default::default(),
        evidence_bytes: 1 << 20,
    };
    let code = operation(
        &input,
        &site,
        None,
        RetentionSelection::Preparation {
            selection: &selection,
            coordinator: None,
            allow_anchor_write: false,
        },
    )
    .execute_with_delivery(before_code);
    assert_eq!(code, "documentation_preparation_permission");
    assert_eq!(
        fs::read_dir(preparation_root.path())
            .expect("preparation parent")
            .count(),
        0,
        "invalid preparation permission cannot create a journal or stage"
    );
}

#[test]
fn native_stages_keep_exact_paths_and_pins_when_full_report_overflows() {
    let reservation = Reservation::new(bounds(), MAXIMUM).expect("pre-effect response allowance");
    let root = tempfile::tempdir().expect("private fixture");
    let parent = root.path().join("native-雪-\"\\\n");
    fs::create_dir(&parent).expect("portable Unicode/control parent");
    let input = retain(&parent, &captured(b"source"));
    let output = retain(&parent, &captured(b"{\"api\":1}"));
    let target = retain(&parent, &captured(b"assembled"));
    let mut jobs = vec![job(&input, &output)];
    jobs[0].evidence.version_text = Some("large version output\n".repeat(20_000));
    let version = jobs[0]
        .evidence
        .version_text
        .as_ref()
        .expect("version text");
    let digest = source::Sha256Digest::of(version.as_bytes());
    let length = version.len();
    jobs[0]
        .evidence
        .version
        .as_mut()
        .expect("version observation")
        .stdout_sha256 = Some(digest);
    jobs[0]
        .evidence
        .version
        .as_mut()
        .expect("version observation")
        .stdout_bytes = Some(length);
    let custody = Custody::Complete {
        jobs: &jobs,
        output_identity: target.inventory().identity(),
        target: Some(&target),
    };
    let full = json!({"jobs":jobs,"large":"x".repeat(MAXIMUM*2)});
    let report = reservation
        .finish(Some(&full), custody)
        .expect("native bounds honored");
    assert!(report.is_error);
    assert!(report.bytes.len() <= MAXIMUM);
    let value = decode(&report.bytes);
    assert_eq!(
        path(&value["completed"][0]["input_unix_bytes"]),
        input.path()
    );
    assert_eq!(
        path(&value["completed"][0]["output_unix_bytes"]),
        output.path()
    );
    assert_eq!(path(&value["target_unix_bytes"]), target.path());
    assert_eq!(
        value["output_identity"],
        json!(target.inventory().identity())
    );
    assert_eq!(
        value["completed"][0]["output_identity"],
        json!(jobs[0].output_identity)
    );
    assert_eq!(value["completed"][0]["selection"], json!(jobs[0].selection));
    assert_eq!(value["completed"][0]["content"], json!(jobs[0].content));
    assert_eq!(
        value["completed"][0]["generation"],
        json!(jobs[0].evidence.generation)
    );
    assert!(!String::from_utf8_lossy(&report.bytes).contains("large version output"));
    assert_eq!(
        fs::read(output.path().join("api.json")).expect("no cleanup"),
        b"{\"api\":1}"
    );
    assert_eq!(
        fs::read(target.path().join("api.json")).expect("no recreation"),
        b"assembled"
    );
    let small = json!({"complete":true});
    let report = Reservation::new(bounds(), MAXIMUM)
        .expect("reservation")
        .finish(Some(&small), custody)
        .expect("full delivery");
    assert!(!report.is_error);
    assert_eq!(decode(&report.bytes), small);
}

#[test]
fn target_failure_preserves_exact_reason_and_completed_job_custody() {
    let root = tempfile::tempdir().expect("private fixture");
    let input = retain(root.path(), &captured(b"source"));
    let output = retain(root.path(), &captured(b"{\"api\":1}"));
    let jobs = [job(&input, &output)];
    let error = DocumentationError::Target(gazette_core::documentation::Error::Invalid {
        context: "artifacts/library/reference.json".into(),
        reason: "TypeDoc local reflection target is absent",
    });
    let encoded = Reservation::new(bounds(), MAXIMUM)
        .expect("pre-effect capacity")
        .finish::<()>(
            None,
            Custody::AssemblyFailed {
                jobs: &jobs,
                error: &error,
            },
        )
        .expect("complete failure delivery");
    assert!(encoded.is_error);
    let value = decode(&encoded.bytes);
    assert_eq!(value["failure"]["code"], "documentation_target_input");
    assert_eq!(
        value["failure"]["target_diagnostic"],
        json!({
            "context":"artifacts/library/reference.json", "context_omitted":false,
            "reason":"TypeDoc local reflection target is absent", "reason_omitted":false
        })
    );
    assert_eq!(value["completed"].as_array().expect("prefix").len(), 1);
    assert_eq!(
        value["completed"][0]["input_identity"],
        json!(jobs[0].evidence.input)
    );
    assert_eq!(
        value["completed"][0]["output_identity"],
        json!(jobs[0].output_identity)
    );
    assert_eq!(
        value["completed"][0]["generation"],
        json!(jobs[0].evidence.generation)
    );
    assert_eq!(value["completed"][0]["content"], json!(jobs[0].content));
    assert_eq!(
        path(&value["completed"][0]["input_unix_bytes"]),
        input.path()
    );
    assert_eq!(
        path(&value["completed"][0]["output_unix_bytes"]),
        output.path()
    );
    assert!(value["target_unix_bytes"].is_null());
    assert_eq!(
        fs::read(output.path().join("api.json")).expect("no replay or cleanup"),
        b"{\"api\":1}"
    );
}

#[test]
fn target_diagnostic_maximum_escaping_fits_the_pre_effect_minimum_reservation() {
    static MAX_REASON: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| "\0".repeat(256));
    let minimum = minimum_reservation(bounds());
    for context in ["\"".repeat(1024), "雪".repeat(341)] {
        let error = DocumentationError::Target(gazette_core::documentation::Error::Invalid {
            context: context.clone(),
            reason: MAX_REASON.as_str(),
        });
        for maximum in [minimum, MAXIMUM] {
            let encoded = Reservation::new(bounds(), maximum)
                .expect("capacity proved before effects")
                .finish::<()>(
                    None,
                    Custody::AssemblyFailed {
                        jobs: &[],
                        error: &error,
                    },
                )
                .expect("maximum diagnostic cannot break custody reservation");
            assert!(encoded.is_error);
            assert!(encoded.bytes.len() <= maximum);
            let value = decode(&encoded.bytes);
            let diagnostic = &value["failure"]["target_diagnostic"];
            assert_eq!(diagnostic["context"], context);
            assert_eq!(diagnostic["reason"], MAX_REASON.as_str());
            assert_eq!(diagnostic["context_omitted"], false);
            assert_eq!(diagnostic["reason_omitted"], false);
        }
    }
    assert!(Reservation::new(bounds(), minimum - 1).is_err());
}

#[test]
fn unsafe_or_oversized_target_diagnostics_are_explicitly_omitted_without_losing_failure() {
    static LARGE_REASON: std::sync::LazyLock<String> =
        std::sync::LazyLock::new(|| "unbounded-static-reason".repeat(100));
    for context in [
        "/private/input.json".into(),
        "../private/input.json".into(),
        "C:/private/input.json".into(),
        "https://user:secret@docs.example.invalid/input".into(),
        "file:///private/input.json".into(),
        "safe/line\nbreak.json".into(),
        "safe\\private.json".into(),
        "x".repeat(1025),
    ] {
        let error = DocumentationError::Target(gazette_core::documentation::Error::Invalid {
            context: context.clone(),
            reason: "Exact safe invariant",
        });
        let encoded = Reservation::new(bounds(), MAXIMUM)
            .expect("reservation")
            .finish::<()>(
                None,
                Custody::AssemblyFailed {
                    jobs: &[],
                    error: &error,
                },
            )
            .expect("bounded safe failure");
        assert!(encoded.is_error);
        let value = decode(&encoded.bytes);
        assert_eq!(value["failure"]["code"], "documentation_target_input");
        assert_eq!(
            value["failure"]["target_diagnostic"],
            json!({
                "context":null, "context_omitted":true,
                "reason":"Exact safe invariant", "reason_omitted":false
            })
        );
        assert!(!String::from_utf8_lossy(&encoded.bytes).contains(context.as_str()));
    }
    let error = DocumentationError::Target(gazette_core::documentation::Error::Invalid {
        context: "artifacts/input.json".into(),
        reason: LARGE_REASON.as_str(),
    });
    let encoded = Reservation::new(bounds(), MAXIMUM)
        .expect("reservation")
        .finish::<()>(
            None,
            Custody::AssemblyFailed {
                jobs: &[],
                error: &error,
            },
        )
        .expect("oversized reason does not destroy original failure custody");
    assert!(encoded.is_error);
    let value = decode(&encoded.bytes);
    assert_eq!(
        value["failure"]["target_diagnostic"],
        json!({
            "context":"artifacts/input.json", "context_omitted":false,
            "reason":null, "reason_omitted":true
        })
    );
}

#[test]
fn explicit_file_named_directory_and_root_keep_native_selection_and_content_in_compact_delivery() {
    let root = tempfile::tempdir().expect("native stage fixture");
    let input = retain(root.path(), &captured(b"source"));
    let name = "\"".repeat(bounds().entry_bytes);
    let file_path: source::EntryPath = name.parse().expect("legal maximum-escaped selector");
    let content = source::Content::File {
        bytes: 3,
        sha256: source::Sha256Digest::of(b"api"),
    };
    for selection in [
        Selection::File {
            path: file_path.clone(),
        },
        Selection::Directory {
            path: Some(file_path.clone()),
        },
        Selection::Directory { path: None },
    ] {
        let (entries, payload) = match &selection {
            Selection::File { path } => (vec![(path.clone(), content)], path.clone()),
            Selection::Directory { path: Some(path) } => {
                let payload: source::EntryPath =
                    format!("{path}/api.json").parse().expect("child path");
                (
                    vec![
                        (path.clone(), source::Content::Directory),
                        (payload.clone(), content),
                    ],
                    payload,
                )
            }
            Selection::Directory { path: None } => {
                let payload: source::EntryPath = "api.json".parse().expect("root file");
                (vec![(payload.clone(), content)], payload)
            }
        };
        let mut inventory = source::InventoryBuilder::new(Default::default());
        for (path, content) in entries {
            inventory.insert(path, content).expect("selected tree");
        }
        let capture = source::CapturedSource::from_parts(
            inventory.finish(),
            BTreeMap::from([(payload, b"api".to_vec())]),
        )
        .expect("complete bytes");
        let native_content = selection
            .content(&capture.as_input())
            .expect("native selection kind");
        let output = retain(root.path(), &capture);
        let mut completed = job(&input, &output);
        completed.selection = selection.clone();
        completed.content = native_content;
        let jobs = [completed];
        let custody = Custody::Complete {
            jobs: &jobs,
            output_identity: output.inventory().identity(),
            target: Some(&output),
        };
        let oversized = "x".repeat(MAXIMUM * 2);
        let encoded = Reservation::new(bounds(), MAXIMUM)
            .expect("all native kinds reserved")
            .finish(Some(&oversized), custody)
            .expect("lossless bounded native outcome");
        assert!(encoded.is_error);
        let value = decode(&encoded.bytes);
        assert_eq!(value["completed"][0]["selection"], json!(selection));
        assert_eq!(value["completed"][0]["content"], json!(native_content));
        assert!(value["completed"][0].get("entry").is_none());
        assert!(value["completed"][0].get("sha256").is_none());
        if matches!(selection, Selection::Directory { .. }) {
            assert!(value["completed"][0]["content"].get("sha256").is_none());
        }
        let tight = Bounds {
            entry_bytes: bounds().entry_bytes - 1,
            ..bounds()
        };
        let result = Reservation::new(tight, MAXIMUM)
            .expect("tighter pre-effect ceiling")
            .finish(Some(&oversized), custody);
        if selection.path().is_some() {
            let error = result.err().expect("exact selection path bound enforced");
            assert!(
                matches!(error.custody, Custody::Complete { jobs: original, .. }
                if std::ptr::eq(original, jobs.as_slice()))
            );
        } else {
            assert!(
                result.is_ok(),
                "root selection has zero relative path bytes"
            );
        }
        let mut mismatched = job(&input, &output);
        mismatched.selection = Selection::Directory { path: None };
        // Public result construction cannot smuggle file metadata into a directory receipt.
        let invalid = [mismatched];
        let result = Reservation::new(bounds(), MAXIMUM)
            .expect("native witness reservation")
            .finish(
                Some(&oversized),
                Custody::Complete {
                    jobs: &invalid,
                    output_identity: output.inventory().identity(),
                    target: Some(&output),
                },
            );
        let error = result
            .err()
            .expect("kind mismatch violates the native delivery contract");
        assert!(
            matches!(error.custody, Custody::Complete { jobs: original, .. }
            if std::ptr::eq(original, invalid.as_slice()))
        );
    }
}

#[test]
fn active_generator_uncertainty_survives_completed_prefix_and_large_version_text() {
    let root = tempfile::tempdir().expect("fixture");
    let input = retain(root.path(), &captured(b"source"));
    let output = retain(root.path(), &captured(b"{\"api\":1}"));
    let completed = job(&input, &output);
    let mut evidence = completed.evidence.clone();
    evidence.version_text = Some("private version".repeat(MAXIMUM));
    let version = evidence.version_text.as_ref().expect("version text");
    let digest = source::Sha256Digest::of(version.as_bytes());
    let length = version.len();
    evidence
        .version
        .as_mut()
        .expect("version observation")
        .stdout_sha256 = Some(digest);
    evidence
        .version
        .as_mut()
        .expect("version observation")
        .stdout_bytes = Some(length);
    evidence.generation.as_mut().expect("started").exit = None;
    evidence.generation.as_mut().expect("started").stdout_sha256 = None;
    evidence.generation.as_mut().expect("started").stdout_bytes = None;
    let failure = DocumentationGenerationError {
        completed: vec![completed],
        retained: vec![input.path().into(), output.path().into()],
        cause: DocumentationGenerationCause::Generator(Box::new(generator::Error {
            phase: generator::Phase::Generation,
            input: input.path().into(),
            output: output.path().into(),
            evidence: Box::new(evidence),
            cause: generator::Cause::Process(process::Error {
                failure: process::Failure::Timeout,
                cleanup: process::Cleanup::Uncertain,
                source: None,
            }),
        })),
    };
    let report = Reservation::new(bounds(), MAXIMUM)
        .expect("reservation")
        .finish::<()>(None, Custody::GenerationFailed(&failure))
        .expect("native failure evidence");
    let value = decode(&report.bytes);
    assert!(report.is_error);
    assert_eq!(value["failure"]["cleanup"], "uncertain");
    assert_eq!(value["failure"]["process_failure"], "timeout");
    assert!(value["failure"]["generation"]["exit"].is_null());
    assert!(value["failure"]["generation"]["stdout_sha256"].is_null());
    assert_eq!(path(&value["failure"]["output_unix_bytes"]), output.path());
    assert_eq!(
        value["retained_unix_bytes"]
            .as_array()
            .expect("all paths")
            .len(),
        2
    );
    assert_eq!(
        value["completed"][0]["content"],
        json!(failure.completed[0].content)
    );
    assert!(value["target_unix_bytes"].is_null());
}

#[test]
fn stage_residue_is_kept_even_when_not_present_in_completed_or_retained_lists() {
    let root = tempfile::tempdir().expect("fixture");
    let residue = retain(root.path(), &captured(b"unverified residue"));
    let error = stage::Error::Residue {
        path: residue.path().into(),
        cause: source::Error::Changed(residue.path().into()),
        cleanup: Some(Box::new(source::Error::Changed(residue.path().into()))),
    };
    let failure = DocumentationGenerationError {
        completed: vec![],
        retained: vec![],
        cause: DocumentationGenerationCause::Stage(Box::new(error)),
    };
    let report = Reservation::new(bounds(), MAXIMUM)
        .expect("reservation")
        .finish::<()>(None, Custody::GenerationFailed(&failure))
        .expect("residue report");
    let value = decode(&report.bytes);
    assert_eq!(
        path(&value["failure"]["residue_unix_bytes"]),
        residue.path()
    );
    assert!(!value["failure"]["residue_cleanup"].is_null());
    assert!(value["target_unix_bytes"].is_null());
    let DocumentationGenerationCause::Stage(error) = &failure.cause else {
        panic!("fixture stage");
    };
    let identity = captured(b"assembled").inventory().identity();
    let report = Reservation::new(bounds(), MAXIMUM)
        .expect("reservation")
        .finish::<()>(
            None,
            Custody::StageFailed {
                jobs: &[],
                output_identity: identity,
                error,
            },
        )
        .expect("target failure");
    let value = decode(&report.bytes);
    assert_eq!(value["output_identity"], json!(identity));
    assert_eq!(
        path(&value["failure"]["residue_unix_bytes"]),
        residue.path()
    );
    assert!(
        value["target_unix_bytes"].is_null(),
        "unverified residue never becomes a retained target"
    );
}

#[test]
fn validation_identity_and_uncertainty_survive_compact_delivery() {
    let root = tempfile::tempdir().expect("fixture");
    let input = retain(root.path(), &captured(b"source"));
    let output = retain(root.path(), &captured(b"{\"api\":1}"));
    let mut completed = job(&input, &output);
    completed.evidence.validation = Some(generator::Validation {
        tree: output.inventory().identity(),
        invocation: completed.evidence.generation.clone().expect("invocation"),
    });
    let mut evidence = completed.evidence.clone();
    let validation = evidence.validation.as_mut().expect("validation");
    validation.invocation.exit = None;
    validation.invocation.stdout_sha256 = None;
    validation.invocation.stdout_bytes = None;
    let expected = json!(evidence.validation);
    let failure = DocumentationGenerationError {
        completed: vec![completed],
        retained: vec![input.path().into(), output.path().into()],
        cause: DocumentationGenerationCause::Generator(Box::new(generator::Error {
            phase: generator::Phase::Validation,
            input: input.path().into(),
            output: output.path().into(),
            evidence: Box::new(evidence),
            cause: generator::Cause::Process(process::Error {
                failure: process::Failure::Timeout,
                cleanup: process::Cleanup::Uncertain,
                source: None,
            }),
        })),
    };
    let report = Reservation::new(bounds(), MAXIMUM)
        .expect("reservation")
        .finish::<()>(None, Custody::GenerationFailed(&failure))
        .expect("compact delivery");
    let value = decode(&report.bytes);
    assert!(report.is_error);
    assert_eq!(value["failure"]["generator_phase"], "validation");
    assert_eq!(value["failure"]["validation"], expected);
    assert_eq!(
        value["completed"][0]["validation"],
        json!(failure.completed[0].evidence.validation)
    );
    assert_eq!(value["failure"]["generation"]["exit"]["code"], 0);
}

#[test]
fn synthetic_non_utf8_locator_survives_full_report_serialization_failure() {
    // Native APFS rejects invalid UTF-8 creation. This tests the lossless carrier representation only.
    let synthetic: PathBuf = OsString::from_vec(b"/synthetic-native-\xff".to_vec()).into();
    let target = stage::RetainedStage::new(synthetic.clone(), captured(b"bytes").into_parts().0);
    let identity = target.inventory().identity();
    let report = Reservation::new(bounds(), MAXIMUM)
        .expect("reservation")
        .finish(
            Some(&synthetic),
            Custody::Complete {
                jobs: &[],
                output_identity: identity,
                target: Some(&target),
            },
        )
        .expect("lossless fallback");
    assert!(report.is_error);
    let value = decode(&report.bytes);
    assert_eq!(
        path(&value["target_unix_bytes"]).as_os_str().as_bytes(),
        synthetic.as_os_str().as_bytes()
    );
    assert_eq!(value["output_identity"], json!(identity));
}

#[test]
fn violated_native_bounds_preserve_original_custody_and_full_report() {
    let failure = DocumentationGenerationError {
        completed: vec![],
        retained: vec![PathBuf::from("/too-long")],
        cause: DocumentationGenerationCause::Limit,
    };
    let tiny = Bounds {
        directory_bytes: 2,
        ..bounds()
    };
    let full = json!({"original":"retained"});
    let error = Reservation::new(tiny, MAXIMUM)
        .expect("reservation")
        .finish(Some(&full), Custody::GenerationFailed(&failure))
        .err()
        .expect("contract refusal");
    assert!(std::ptr::eq(
        error.full.expect("original full report"),
        &full
    ));
    let Custody::GenerationFailed(original) = error.custody else {
        panic!("original failure preserved");
    };
    assert!(std::ptr::eq(original, &failure));
    let owned = serde_json::to_value(error.custody).expect("native custody is retainable");
    assert_eq!(owned["code"], "documentation_response_contract");
    assert_eq!(owned["failure"]["code"], failure.code());
    assert_eq!(
        path(&owned["retained_unix_bytes"][0]),
        PathBuf::from("/too-long")
    );
    let target =
        stage::RetainedStage::new(PathBuf::from("/synthetic"), captured(b"one").into_parts().0);
    let error = Reservation::new(bounds(), MAXIMUM)
        .expect("reservation")
        .finish(
            Some(&full),
            Custody::Complete {
                jobs: &[],
                output_identity: captured(b"other").inventory().identity(),
                target: Some(&target),
            },
        )
        .err()
        .expect("no mismatched target identity");
    assert!(
        matches!(error.custody,Custody::Complete {target:Some(original),..} if std::ptr::eq(original,&target))
    );
    let owned = serde_json::to_value(error.custody).expect("mismatch retains original facts");
    assert_eq!(
        owned["output_identity"],
        json!(captured(b"other").inventory().identity())
    );
    assert_eq!(path(&owned["target_unix_bytes"]), target.path());
    let synthetic: PathBuf = OsString::from_vec(b"/contract-native-\xff".to_vec()).into();
    let failure = DocumentationGenerationError {
        completed: vec![],
        retained: vec![synthetic.clone()],
        cause: DocumentationGenerationCause::Limit,
    };
    let owned = serde_json::to_value(Custody::GenerationFailed(&failure))
        .expect("no lossy UTF-8 conversion on contract failure");
    assert_eq!(path(&owned["retained_unix_bytes"][0]), synthetic);
}

#[test]
fn exact_compact_reservation_boundary_precedes_effects_and_includes_framing_capacity() {
    for invalid in [
        Bounds {
            jobs: 129,
            ..bounds()
        },
        Bounds {
            directory_bytes: 0,
            ..bounds()
        },
        Bounds {
            entry_bytes: 0,
            ..bounds()
        },
        Bounds {
            directory_bytes: (1 << 20) + 1,
            ..bounds()
        },
    ] {
        assert!(Reservation::new(invalid, MAXIMUM).is_err());
    }
    assert!(Reservation::new(bounds(), 0).is_err());
    assert!(Reservation::new(bounds(), (16 << 20) + 1).is_err());
    let lower = minimum_reservation(bounds());
    let full = "oversized".repeat(MAXIMUM);
    let output_identity = captured(b"target").inventory().identity();
    let report = Reservation::new(bounds(), lower)
        .expect("exact reserve")
        .finish(
            Some(&full),
            Custody::Complete {
                jobs: &[],
                output_identity,
                target: None,
            },
        )
        .expect("compact target identity");
    assert!(report.is_error);
    assert!(report.bytes.len() <= lower);
    assert!(
        report.bytes.capacity() > lower,
        "one newline already reserved"
    );
}

#[test]
fn shared_report_schema_matches_native_fields_nulls_and_omission_rules() {
    // This is a native projection fixture, not a claim of generator or target compatibility.
    let compilation = gazette_sdk::workspace::compile(
        "[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n",
        Default::default(),
    ).expect("native configuration compiler");
    let site = "site:one".parse().expect("site key");
    let project = "project:one".parse().expect("project key");
    let source_key = "source:one".parse().expect("source key");
    let selection = Selection::File {
        path: "api.json".parse().expect("entry"),
    };
    let repository = gazette_sdk::workspace::DocumentationRepository {
        public_base_url: None,
        name: "configured-docs".into(),
        path: "/selected/docs".into(),
        destination: None,
        build: None,
    };
    let capture = captured(b"{\"api\":1}");
    let root = tempfile::tempdir().expect("native stages");
    let input = retain(root.path(), &captured(b"source"));
    let target = retain(root.path(), &capture);
    let jobs = [job(&input, &target)];
    let artifacts = [gazette_sdk::build::DocumentationArtifactEvidence {
        project: &project,
        module: "library",
        reference: "api",
        source: &source_key,
        selection: &selection,
        format: gazette_core::documentation::Format::TypeDocJson,
        content: source::Content::File {
            bytes: b"{\"api\":1}".len() as u64,
            sha256: source::Sha256Digest::of(b"{\"api\":1}"),
        },
        generation: None,
        tree: capture.inventory().identity(),
    }];
    let mut report = Report {
        preview_link_bases: None,
        status: documentation_output::Status::AssembledLocal,
        inputs: gazette_sdk::build::CompositionInspection {
            configuration: &gazette_application::configuration::Evidence::Supplied {
                source_sha256: compilation.source_sha256,
            },
            diagrams: None,
            math: None,
            compilation: &compilation,
            site: &site,
            sources: BTreeMap::new(),
        },
        repository: &repository,
        artifacts: &artifacts,
        manual: &[],
        unselected_manual_pages: BTreeMap::new(),
        unselected_manual_pages_incomplete: BTreeMap::new(),
        inventory: capture.inventory(),
        directory: None,
        jobs: &jobs,
    };
    let custody = Custody::Complete {
        jobs: &jobs,
        output_identity: target.inventory().identity(),
        target: Some(&target),
    };
    let encoded = Reservation::new(bounds(), MAXIMUM)
        .expect("reserve")
        .finish(Some(&report), custody)
        .unwrap_or_else(|_| panic!("native bounds"));
    assert!(!encoded.is_error);
    let value = decode(&encoded.bytes);
    validate_schema(&value);
    assert!(value["directory"].is_null());
    assert!(value["artifacts"][0].get("generation").is_none());
    assert!(value["repository"].get("build").is_none());
    report.directory = Some(target.path());
    validate_schema(&serde_json::to_value(&report).expect("retained report"));

    let compact = Reservation::new(bounds(), MAXIMUM)
        .expect("reserve")
        .finish::<Report<'_>>(None, custody)
        .unwrap_or_else(|_| panic!("compact"));
    let mut value = decode(&compact.bytes);
    assert!(value["failure"]["generation"].is_null());
    value["failure"]
        .as_object_mut()
        .expect("failure")
        .remove("generation");
    assert!(
        !jsonschema::validator_for(&Value::Object(documentation_output::schema()))
            .expect("schema")
            .is_valid(&value),
        "explicit nullable fields cannot disappear"
    );
}

#[test]
fn reservation_enum_witnesses_bound_every_native_serialized_variant() {
    fn assert_longest<T: schemars::JsonSchema + serde::Serialize>(witness: T) {
        let schema = schemars::generate::SchemaSettings::draft2020_12()
            .for_serialize()
            .into_generator()
            .into_root_schema_for::<T>();
        let limit = serde_json::to_vec(&witness).expect("native witness").len();
        let value = schema.as_value();
        let variants: Vec<&Value> = if let Some(values) = value["enum"].as_array() {
            values.iter().collect()
        } else {
            value["oneOf"]
                .as_array()
                .expect("documented unit variants")
                .iter()
                .map(|branch| branch.get("const").expect("native unit variant"))
                .collect()
        };
        assert!(!variants.is_empty());
        for variant in variants {
            assert!(
                serde_json::to_vec(variant).expect("variant").len() <= limit,
                "new longer native variant must update the pre-effect witness: {variant}"
            );
        }
    }
    assert_longest(generator::Phase::Generation);
    assert_longest(process::Failure::ExecutableChanged);
    assert_longest(process::Cleanup::NotStarted);
    assert_longest(documentation_output::Status::PreparedLocalNotCertified);
    assert_eq!(
        serde_json::to_value(documentation_output::Status::AssembledLocal).expect("status"),
        json!("documentation-assembled-local")
    );
    assert_eq!(
        serde_json::to_value(documentation_output::Status::PreparedLocalNotCertified)
            .expect("status"),
        json!("documentation-prepared-local-not-certified")
    );
}
