//! Real retained Mintlify output, configured native destinations and held `SQLite` ownership.

use std::{fs, os::unix::fs::PermissionsExt, path::Path};

use gazette_sdk::{build, configuration, source, store};
use serde_json::Value;
use tempfile::TempDir;

const MANUAL: &[u8] = b"# Start\n\nThe retained authored manual.\n";
const REFERENCE: &[u8] =
    br#"{"id":0,"name":"Retained API","kind":1,"variant":"project","schemaVersion":"2.0"}"#;

struct Fixture {
    source: TempDir,
    destination: TempDir,
}

impl Fixture {
    fn new() -> Self {
        let source = tempfile::tempdir().expect("separate selected inputs");
        fs::write(source.path().join("start.md"), MANUAL).expect("actual manual");
        fs::write(source.path().join("api.json"), REFERENCE).expect("actual reference");
        Self {
            source,
            destination: tempfile::tempdir().expect("existing configured output"),
        }
    }

    fn configuration(&self, destination: &str) -> String {
        format!(
            "[[principals]]\nid='person:publisher'\nkind='person'\n\
             [[sites]]\nid='site:docs'\npublisher='person:publisher'\ndeployable=false\n\
             [[projects]]\nid='project:one'\nlifecycle='active'\nvisibility='public'\navailability='available'\n\
             [[sources]]\nid='source:manual'\nlocation={{kind='filesystem',root={:?}}}\n\
             [[brands]]\nid='brand:docs'\nname='Independent documentation brand'\n\
             [[brands.publications]]\nsite='site:docs'\n\
             [brands.publications.documentation_repository]\nname='native-domain-doc'\npath={destination:?}\n\
             [brands.publications.documentation_repository.build]\n\
             target={{kind='mintlify',theme='mint',primary_color='#123abc'}}\n\
             [[brands.publications.documentation_repository.build.modules]]\n\
             project='project:one'\nslug='module'\ntitle='Module'\nmanual={{source='source:manual'}}\n\
             navigation=[{{kind='manual',title='Manual',pages=[{{title='Start',route='manual/start',input={{kind='file',path='start.md'}}}}]}},\
             {{kind='reference',slug='reference',title='API',input={{kind='artifact',source='source:manual',selection={{kind='file',path='api.json'}},format='typedoc-json'}}}}]\n",
            self.source
                .path()
                .to_str()
                .expect("native source coordinate"),
        )
    }

    fn inputs(&self, destination: &Path) -> build::AcquiredInputs {
        acquire(&self.configuration(destination.to_str().expect("native destination")))
    }
}

fn acquire(config: &str) -> build::AcquiredInputs {
    build::acquire_documentation(
        config,
        &"site:docs".parse().expect("site"),
        Default::default(),
        build::PublicationSetAcquisitionLimits::default().sources,
    )
    .expect("actual native selected documentation capture")
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

fn journal() -> (TempDir, store::InitializationReceipt) {
    let root = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .expect("private native journal");
    let initialized = store::initialize(
        root.path(),
        store::InitializeOptions {
            allow_create: true,
            session: session(),
            ..Default::default()
        },
    )
    .expect("actual baseline initialization");
    (root, initialized)
}

fn writer(root: &Path, header: store::JournalHeader) -> store::Writer {
    store::acquire_writer(
        root,
        header,
        store::WriterOptions {
            allow_generation_advance: true,
            session: session(),
        },
    )
    .expect("held native writer, no reacquisition during an operation")
}

fn options() -> build::MaterializationOptions {
    build::MaterializationOptions {
        allow_write: true,
        history: store::MaterializationWriteOptions { allow_record: true },
        ..Default::default()
    }
}

fn assert_actual_output(assembly: &build::DocumentationAssembly<'_>, root: &Path) {
    for entry in assembly.output().inventory().entries() {
        if let Some(bytes) = assembly.output().file(&entry.path) {
            assert_eq!(
                fs::read(root.join(entry.path.as_str())).expect("actual output bytes"),
                bytes
            );
        }
    }
    assert!(
        fs::read_to_string(root.join("manual/start.md"))
            .expect("actual manual output")
            .contains("retained authored manual")
    );
}

#[test]
fn actual_configured_docs_preserve_git_user_work_and_exact_history_across_lifetimes() {
    let fixture = Fixture::new();
    let git = git2::Repository::init(fixture.destination.path()).expect("actual output repository");
    fs::write(fixture.destination.path().join("user.txt"), b"unowned work").expect("user work");
    let git_before = source::inspect(git.path(), None, Default::default())
        .expect("admin baseline")
        .identity();
    let inputs = fixture.inputs(fixture.destination.path());
    let assembly =
        build::assemble_documentation(&inputs, Default::default()).expect("real Mintlify assembly");
    assert_eq!(assembly.brand().as_str(), "brand:docs");
    assert_eq!(assembly.manual().len(), 1);
    assert_eq!(assembly.artifacts().len(), 1);
    // The materializer receives retained bytes, not source or provider handles.
    fs::write(
        fixture.source.path().join("start.md"),
        b"changed after assembly",
    )
    .expect("source change");
    fs::write(
        fixture.source.path().join("api.json"),
        b"not a reference anymore",
    )
    .expect("reference change");
    let (journal, initialized) = journal();
    let mut current = writer(journal.path(), initialized.header);
    let original = current
        .plan_documentation_materialization(
            &assembly,
            &[],
            Default::default(),
            options().evidence_bytes,
        )
        .expect("native configured documentation plan");
    let replay = current
        .plan_documentation_materialization(
            &assembly,
            &[],
            Default::default(),
            options().evidence_bytes,
        )
        .expect("same original write-free plan retained before effects");
    let id = source::Sha256Digest::of(b"original documentation");
    let result = current
        .materialize(original, id, options())
        .expect("durable intent/write/readback/completion");
    let receipt = result.output.as_ref().expect("actual current output");
    assert_eq!(receipt.generated(), assembly.output().inventory());
    assert_actual_output(&assembly, fixture.destination.path());
    assert_eq!(
        fs::read(fixture.destination.path().join("user.txt")).expect("preserved user"),
        b"unowned work"
    );
    assert_eq!(
        source::inspect(git.path(), None, Default::default())
            .expect("preserved administration")
            .identity(),
        git_before
    );
    let evidence: Value = serde_json::from_str(&result.history.record.intent.evidence)
        .expect("bounded typed documentation evidence");
    assert_eq!(
        evidence["format"],
        "gazette.documentation-materialization/1"
    );
    assert_eq!(evidence["artifact_class"], "documentation-build-input");
    assert_eq!(evidence["brand"], "brand:docs");
    assert_eq!(
        evidence["configuration"],
        serde_json::to_value(inputs.configuration_evidence()).expect("exact config evidence")
    );
    assert_eq!(
        evidence["manual"],
        serde_json::to_value(assembly.manual()).expect("manual evidence")
    );
    assert_eq!(
        evidence["artifacts"],
        serde_json::to_value(assembly.artifacts()).expect("reference evidence")
    );
    assert_eq!(
        evidence["output"],
        serde_json::to_value(assembly.output().inventory().identity()).expect("artifact identity")
    );
    assert_eq!(evidence["sources"]["source:manual"]["kind"], "filesystem");
    assert_eq!(
        evidence["sources"]["source:manual"]["observation"],
        serde_json::to_value(
            inputs.sources()[&"source:manual".parse().expect("source")].inventory()
        )
        .expect("complete retained source inventory")
    );
    let repeated = current
        .materialize(replay, id, options())
        .expect("exact historical receipt replay");
    assert!(!repeated.history.inserted);
    assert!(repeated.output.is_none());
    assert_eq!(
        serde_json::to_value(repeated.inspect()).expect("shared inspection")["output"],
        Value::Null
    );
    let header = current.inspect().expect("same held writer").header;
    current.close().expect("explicit original close");
    let mut next = writer(journal.path(), header);
    // A normally changing administration directory is still the same bound root.
    fs::write(
        git.path().join("owned-fixture-marker"),
        b"ordinary Git administrative change",
    )
    .expect("admin-directory change");
    let next_plan = next
        .plan_documentation_materialization(
            &assembly,
            &[],
            Default::default(),
            options().evidence_bytes,
        )
        .expect("completed head ownership across writer lifetimes");
    assert_eq!(next_plan.previous(), Some(id));
    assert!(next_plan.output().changes().is_empty());
    let next_result = next
        .materialize(
            next_plan,
            source::Sha256Digest::of(b"next documentation"),
            options(),
        )
        .expect("actual no-op readback and new complete history");
    assert!(next_result.output.is_some());
    assert_actual_output(&assembly, fixture.destination.path());
    next.close().expect("explicit later close");
}

#[test]
fn relative_documentation_output_is_bound_to_loaded_workspace_not_process_directory() {
    let fixture = Fixture::new();
    let root = tempfile::tempdir().expect("explicit configuration owner");
    fs::create_dir(root.path().join("domain-doc")).expect("existing relative target");
    let config = fixture.configuration("domain-doc");
    fs::write(root.path().join("gazette.toml"), &config).expect("actual owner configuration");
    let loaded = configuration::load_workspace(
        root.path(),
        &"gazette.toml".parse().expect("config entry"),
        Default::default(),
    )
    .expect("exact native workspace");
    let inputs = build::acquire_documentation_workspace(
        loaded,
        &"site:docs".parse().expect("site"),
        build::PublicationSetAcquisitionLimits::default().sources,
    )
    .expect("same selected native owner");
    let assembly = build::assemble_documentation(&inputs, Default::default()).expect("real target");
    let plan = build::plan_documentation_output(&assembly, None, Default::default())
        .expect("native relative coordinate");
    assert_eq!(plan.root(), root.path().join("domain-doc"));
    let receipt = build::materialize_output(plan, true).expect("shared guarded writer");
    assert_actual_output(&assembly, receipt.root());
    let detached = acquire(&config);
    let detached_assembly = build::assemble_documentation(&detached, Default::default())
        .expect("retained detached target");
    assert!(matches!(
        build::plan_documentation_output(&detached_assembly, None, Default::default()),
        Err(build::OutputPlanError::Coordinate(_))
    ));
    assert_eq!(
        fs::read(root.path().join("gazette.toml")).expect("configuration unchanged"),
        config.as_bytes()
    );
}

#[test]
fn collision_overlap_missing_destination_and_journal_overlap_refuse_without_output_effects() {
    let fixture = Fixture::new();
    let inputs = fixture.inputs(fixture.destination.path());
    let assembly = build::assemble_documentation(&inputs, Default::default()).expect("real target");
    fs::write(
        fixture.destination.path().join("docs.json"),
        b"unowned config",
    )
    .expect("user collision");
    assert!(matches!(
        build::plan_documentation_output(&assembly, None, Default::default()),
        Err(build::OutputPlanError::Collision { .. })
    ));
    assert_eq!(
        fs::read(fixture.destination.path().join("docs.json")).expect("unchanged collision"),
        b"unowned config"
    );
    let missing = fixture.destination.path().join("absent");
    let inputs = fixture.inputs(&missing);
    let missing_assembly = build::assemble_documentation(&inputs, Default::default())
        .expect("assembly does not create output");
    assert!(matches!(
        build::plan_documentation_output(&missing_assembly, None, Default::default()),
        Err(build::OutputPlanError::Observation(_))
    ));
    assert!(!missing.exists());
    let inputs = fixture.inputs(fixture.source.path());
    let overlapping = build::assemble_documentation(&inputs, Default::default())
        .expect("target independent from output");
    assert!(matches!(
        build::plan_documentation_output(&overlapping, None, Default::default()),
        Err(build::OutputPlanError::Observation(_))
    ));
    assert_eq!(
        fs::read(fixture.source.path().join("start.md")).expect("source preserved"),
        MANUAL
    );
    let (journal, initialized) = journal();
    let inputs = fixture.inputs(journal.path());
    let journal_target = build::assemble_documentation(&inputs, Default::default())
        .expect("assembly does not open journal output");
    let mut current = writer(journal.path(), initialized.header);
    let before = current.inspect().expect("original journal").header;
    assert!(matches!(
        current.plan_documentation_materialization(
            &journal_target,
            &[],
            Default::default(),
            options().evidence_bytes
        ),
        Err(build::MaterializationError::Plan(
            build::OutputPlanError::Observation(_)
        ))
    ));
    assert_eq!(current.inspect().expect("no history effect").header, before);
    current.close().expect("explicit close");
}

#[test]
fn missing_permissions_and_bounded_evidence_refuse_before_any_intent_or_native_write() {
    for (allow_write, allow_record) in [(false, true), (true, false)] {
        let fixture = Fixture::new();
        let inputs = fixture.inputs(fixture.destination.path());
        let assembly =
            build::assemble_documentation(&inputs, Default::default()).expect("retained target");
        let (journal, initialized) = journal();
        let mut current = writer(journal.path(), initialized.header);
        let before = current.inspect().expect("held header").header;
        let plan = current
            .plan_documentation_materialization(
                &assembly,
                &[],
                Default::default(),
                options().evidence_bytes,
            )
            .expect("write-free plan");
        let refused = current
            .materialize(
                plan,
                source::Sha256Digest::of(b"unpermitted docs"),
                build::MaterializationOptions {
                    allow_write,
                    history: store::MaterializationWriteOptions { allow_record },
                    ..Default::default()
                },
            )
            .expect_err("explicit independent permissions");
        assert_eq!(
            refused.code(),
            if allow_write {
                "store_materialization_permission"
            } else {
                "materialization_permission"
            }
        );
        assert_eq!(current.inspect().expect("no history append").header, before);
        assert!(!fixture.destination.path().join("docs.json").exists());
        let error = current
            .plan_documentation_materialization(&assembly, &[], Default::default(), 1)
            .expect_err("complete evidence budget never truncates provenance");
        assert!(matches!(error, build::MaterializationError::Evidence(_)));
        assert_eq!(
            current
                .inspect()
                .expect("bounded preflight has no effects")
                .header,
            before
        );
        current.close().expect("explicit close");
    }
}

#[test]
fn native_change_after_plan_retains_exact_pending_id_and_never_blindly_retries() {
    let fixture = Fixture::new();
    let inputs = fixture.inputs(fixture.destination.path());
    let assembly = build::assemble_documentation(&inputs, Default::default()).expect("real target");
    let (journal, initialized) = journal();
    let mut current = writer(journal.path(), initialized.header);
    let plan = current
        .plan_documentation_materialization(
            &assembly,
            &[],
            Default::default(),
            options().evidence_bytes,
        )
        .expect("original plan");
    let historical_root = plan.output().binding().canonical_root().to_owned();
    fs::write(
        fixture.destination.path().join("new-user.txt"),
        b"concurrent user bytes",
    )
    .expect("native change");
    let id = source::Sha256Digest::of(b"pending documentation");
    let failed = current
        .materialize(plan, id, options())
        .expect_err("retained intent, changed FS refusal");
    let build::MaterializationError::Write { id: actual, cause } = failed else {
        panic!("exact writer phase")
    };
    assert_eq!(actual, id);
    assert!(cause.attempted.is_empty());
    assert!(cause.residue.is_none());
    assert!(!fixture.destination.path().join("docs.json").exists());
    let error = current
        .plan_documentation_materialization(
            &assembly,
            &[],
            Default::default(),
            options().evidence_bytes,
        )
        .expect_err("pending intent refuses another operation");
    assert!(matches!(
        error,
        build::MaterializationError::Store(store::Error::MaterializationPending)
    ));
    let frontier = current
        .inspect_materialization(&historical_root)
        .expect("original durable pending history");
    assert_eq!(frontier.frontier.root, historical_root);
    assert_eq!(
        frontier
            .frontier
            .active
            .expect("pending original")
            .intent
            .id,
        id
    );
    assert!(frontier.frontier.head.is_none());
    let header = current.inspect().expect("current original history").header;
    current.close().expect("explicit pending close");
    let mut later = writer(journal.path(), header);
    assert_eq!(
        later
            .plan_documentation_materialization(
                &assembly,
                &[],
                Default::default(),
                options().evidence_bytes
            )
            .expect_err("reopening is not retry permission")
            .code(),
        "store_materialization_pending"
    );
    assert_eq!(
        fs::read(fixture.destination.path().join("new-user.txt")).expect("user bytes retained"),
        b"concurrent user bytes"
    );
    later.close().expect("explicit later close");
}
