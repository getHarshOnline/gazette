//! Retained publication -> durable intent -> exact native stage -> independent history.

#[path = "preparation/authorization.rs"]
mod authorization;
#[path = "preparation/check.rs"]
mod check;
#[path = "preparation/destination.rs"]
mod destination;
#[path = "preparation/direct.rs"]
mod direct;
#[path = "preparation/local.rs"]
mod local;
#[path = "preparation/mcp.rs"]
mod mcp;
#[path = "preparation/raster.rs"]
mod raster;
#[path = "preparation/recovery_mcp.rs"]
mod recovery_mcp;
#[path = "preparation/set.rs"]
mod set;

use std::{collections::BTreeMap, os::unix::fs::PermissionsExt, path::Path};

use gazette_sdk::{authored, publication::content::Disposition, store};
use tempfile::TempDir;

use super::{Fixture, Value, acquire, build, bytes, fs, source};

// Publication-preparation fixtures explicitly change the draft before the source
// commit and capture. Caller overrides are retained exactly, never auto-promoted.
pub(super) fn published_fixture(overrides: &[(&str, &[u8])]) -> Fixture {
    let page = super::PAGE.replace("lifecycle='draft'", "lifecycle='published'");
    let mut files = vec![("one/page.md", page.as_bytes())];
    files.extend_from_slice(overrides);
    Fixture::new(&files)
}

pub(super) fn with_destinations(mut config: String) -> String {
    for site in ["one", "other"] {
        let anchor = format!("site='site:{site}'\npublic_base_url=");
        assert!(config.contains(&anchor));
        config = config.replace(
            &anchor,
            &format!(
                "site='site:{site}'\ndestination={{kind='git',remote='https://example.invalid/{site}.git',branch='publication'}}\npublic_base_url="
            ),
        );
    }
    config
}

pub(super) fn enable_site(mut config: String, site: &str) -> String {
    let disabled = format!("[[sites]]\nid='site:{site}'\npublisher='owner:one'\ndeployable=false");
    let enabled = format!("[[sites]]\nid='site:{site}'\npublisher='owner:one'\ndeployable=true");
    assert_eq!(config.matches(&disabled).count(), 1);
    config = config.replacen(&disabled, &enabled, 1);
    config
}

pub(super) fn select_all<'a, 's>(
    parsed: &'a build::ParsedPublication<'s>,
) -> Result<build::SelectedPublication<'a, 's>, authored::SelectionError> {
    let resolved = parsed
        .resolve(Default::default())
        .expect("complete fixture graph");
    let selection = authored::Selection {
        content: resolved
            .authored()
            .content()
            .graph()
            .nodes()
            .map(|node| (node.id.clone(), Disposition::Include))
            .collect(),
        assets: parsed
            .corpus()
            .files()
            .iter()
            .filter(|(_, file)| {
                matches!(
                    file,
                    gazette_sdk::corpus::File::Asset(_) | gazette_sdk::corpus::File::Notebook(_)
                )
            })
            .map(|(path, _)| (path.clone(), Disposition::Include))
            .collect(),
    };
    resolved.into_selected(selection)
}

pub(super) fn render(
    inputs: &build::AcquiredInputs,
) -> Result<build::PublicationRender<'_>, build::PublicationRenderError> {
    render_with_limits(inputs, Default::default())
}

fn render_with_limits(
    inputs: &build::AcquiredInputs,
    limits: build::PublicationOutputLimits,
) -> Result<build::PublicationRender<'_>, build::PublicationRenderError> {
    let parsed =
        build::parse_publication(inputs, Default::default()).expect("retained fixture syntax");
    let selected = select_all(&parsed).expect("explicit complete non-draft fixture selection");
    build::render_selected_publication(
        &selected,
        build::SiteRenderOptions {
            render_year: None,
            graph: None,
            model_output: Some(&"model.json".parse().expect("model output")),
            limits: Default::default(),
        },
        limits,
    )
}

fn read() -> store::ReadOptions {
    store::ReadOptions {
        allow_coordination_files: true,
        ..Default::default()
    }
}

fn private_directory() -> TempDir {
    tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .expect("disposable private directory")
}

fn initialize(path: &Path) -> store::InitializationReceipt {
    store::initialize(
        path,
        store::InitializeOptions {
            allow_create: true,
            session: store::SessionOptions {
                allow_local_reservation: true,
                read: read(),
            },
            ..Default::default()
        },
    )
    .expect("native fresh journal")
}

fn options() -> build::PreparationOptions {
    build::PreparationOptions {
        allow_stage: true,
        record: store::PreparationWriteOptions { allow_record: true },
        writer: store::WriterOptions {
            allow_generation_advance: true,
            session: store::SessionOptions {
                allow_local_reservation: true,
                read: read(),
            },
        },
        ..Default::default()
    }
}

fn request<'a>(
    journal: &'a Path,
    expected: store::JournalHeader,
    parent: &'a Path,
    name: &'a str,
) -> build::PreparationRequest<'a> {
    build::PreparationRequest {
        journal,
        expected,
        parent,
        name,
        id: source::Sha256Digest::of(b"one exact local preparation"),
    }
}

fn unchanged(journal: &store::InitializationReceipt, id: source::Sha256Digest, database: &[u8]) {
    let inspected =
        store::inspect_preparation(&journal.database, id, Some(&journal.header), read())
            .expect("unchanged independently observed journal");
    assert_eq!(inspected.journal.header, journal.header);
    assert!(inspected.record.is_none());
    assert_eq!(
        fs::read(&journal.database).expect("journal bytes"),
        database
    );
}

#[test]
fn unselected_published_and_draft_renders_never_start_durable_preparation() {
    use super::{policy::configured, worktree::metadata};
    for draft in [false, true] {
        let published_other = super::OTHER.replace("lifecycle='archived'", "lifecycle='published'");
        let fixture = if draft {
            Fixture::new(&[])
        } else {
            published_fixture(&[("other/page.md", published_other.as_bytes())])
        };
        let observation = private_directory();
        let checker = observation.path().join("must-not-run.json");
        let (config, _) = configured(&fixture, &checker, 0);
        let inputs = acquire(&config);
        let parsed =
            build::parse_publication(&inputs, Default::default()).expect("real retained syntax");
        if draft {
            let error =
                select_all(&parsed).expect_err("explicitly including draft content must fail");
            assert!(
                matches!(error, authored::SelectionError::Draft(ref id) if id.as_str() == "content:one")
            );
        } else {
            let selected =
                select_all(&parsed).expect("all-published source can be explicitly selected");
            assert!(selected.authored().content().graph().nodes().all(
                |node| node.lifecycle == gazette_sdk::publication::ContentLifecycle::Published
            ));
        }
        let rendered =
            super::render(&inputs).expect("private unfiltered rendering remains available");
        assert!(rendered.selection().is_none());
        let journal_root = private_directory();
        let journal = initialize(journal_root.path());
        let database = fs::read(&journal.database).expect("original journal bytes");
        let parent = private_directory();
        let request = request(
            journal_root.path(),
            journal.header,
            parent.path(),
            "unselected",
        );
        for allowed in [false, true] {
            let stage_options = if allowed {
                options()
            } else {
                Default::default()
            };
            let error = build::prepare_stage(&rendered, request, stage_options)
                .expect_err("selection precedes permissions and native stage/journal effects");
            assert!(matches!(*error.cause, build::PreparationFailure::Selection));
            assert!(!error.writer_acquired);
            assert!(error.journal.is_none() && error.receipt.is_none() && error.stage.is_none());
            let error = build::prepare_local(
                &rendered,
                build::LocalPreparationRequest {
                    output_policy: None,
                    qualification: None,
                    coordinator: None,
                    stage: request,
                    metadata: &metadata(),
                    parent_source: None,
                    disclosure: &check::disclosure_policy(),
                },
                build::LocalPreparationOptions {
                    stage: stage_options,
                    policy: gazette_sdk::process::Options {
                        allow_execute: allowed,
                        ..Default::default()
                    },
                    worktree: gazette_sdk::release_commit::WorktreeOptions {
                        allow_initialize: allowed,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .expect_err("unselected input cannot invoke checker or initialize Git");
            let build::LocalPreparationFailure::Stage(failure) = error.cause.as_ref() else {
                panic!("selection must be the first refusal: {error:?}");
            };
            assert!(matches!(
                *failure.cause,
                build::PreparationFailure::Selection
            ));
            assert!(!failure.writer_acquired);
            assert!(error.receipt.is_none());
            assert!(
                failure.journal.is_none() && failure.receipt.is_none() && failure.stage.is_none()
            );
            unchanged(&journal, request.id, &database);
            assert_eq!(
                fs::read_dir(parent.path())
                    .expect("no stage or Git metadata")
                    .count(),
                0
            );
            assert!(
                !checker.exists(),
                "native policy command was never executed"
            );
        }
    }
}

#[test]
fn destination_bearing_disabled_site_refuses_direct_preparation_without_effects() {
    let fixture = published_fixture(&[]);
    let config = with_destinations(fixture.config());
    let inputs = acquire(&config);
    let rendered = render(&inputs).expect("local rendering remains available while disabled");
    let journal_root = private_directory();
    let journal = initialize(journal_root.path());
    let database = fs::read(&journal.database).expect("original journal bytes");
    let parent = private_directory();
    let request = request(
        journal_root.path(),
        journal.header,
        parent.path(),
        "disabled",
    );
    let error = build::prepare_stage(&rendered, request, options())
        .expect_err("configured delivery requires explicit whole-site eligibility");
    let build::PreparationFailure::Deployability(ref eligibility) = *error.cause else {
        panic!("dedicated deployability failure required: {error:?}")
    };
    assert_eq!(eligibility.code(), "publication_not_deployable");
    assert_eq!(
        eligibility
            .sites()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        ["site:one"]
    );
    assert!(!error.writer_acquired);
    assert!(error.journal.is_none() && error.receipt.is_none() && error.stage.is_none());
    unchanged(&journal, request.id, &database);
    assert_eq!(
        fs::read_dir(parent.path()).expect("stage parent").count(),
        0
    );
}

#[test]
fn retained_render_records_exact_evidence_and_output_before_native_retention() {
    let fixture = published_fixture(&[]);
    let config = fixture.config();
    let inputs = acquire(&config);
    fs::write(
        fixture.directory.path().join("one/page.md"),
        "UNACQUIRED SOURCE",
    )
    .expect("later source edit");
    fs::write(
        fixture.directory.path().join("theme/templates/page.html"),
        "UNACQUIRED TEMPLATE",
    )
    .expect("later theme edit");
    let limits = gazette_sdk::render::Limits {
        templates: source::Limits {
            entries: 16,
            depth: 4,
            file_bytes: 8 << 10,
            total_bytes: 32 << 10,
        },
        context_bytes: 256 << 10,
        output_bytes: 512 << 10,
        execution_depth: gazette_sdk::render::Limits::default().execution_depth,
    };
    let rendered = render_with_limits(
        &inputs,
        build::PublicationOutputLimits {
            render: limits,
            ..Default::default()
        },
    )
    .expect("render retained capture only");
    let tool = "tera@1.20.1/gazette-captured-templates-v8";
    assert_eq!(rendered.site().evidence().theme.tool, tool);
    assert_eq!(rendered.site().evidence().theme.limits, limits);
    let journal_root = private_directory();
    let journal = initialize(journal_root.path());
    let parent = private_directory();
    let request = request(
        journal_root.path(),
        journal.header,
        parent.path(),
        "prepared",
    );
    let receipt =
        build::prepare_stage(&rendered, request, options()).expect("connected preparation");
    let intent = &receipt.record.intent;
    let inventory = rendered.site().output().inventory();
    assert!(receipt.inserted);
    assert_eq!(intent.id, request.id);
    assert_eq!(
        intent.address.parent,
        fs::canonicalize(parent.path()).expect("parent")
    );
    assert_eq!(intent.address.name, "prepared");
    assert_eq!(intent.output, inventory.identity());
    assert_eq!(intent.entries, inventory.entries().len() as u64);
    assert_eq!(intent.bytes, inventory.total_bytes());
    assert_eq!(intent.created, receipt.journal);
    assert_eq!(
        intent.evidence_sha256,
        source::Sha256Digest::of(intent.evidence.as_bytes())
    );
    let evidence: Value = serde_json::from_str(&intent.evidence).expect("typed private evidence");
    assert_eq!(evidence["format"], "gazette.preparation-evidence/1");
    assert_eq!(evidence["site"], "site:one");
    assert_eq!(
        evidence["configuration_sha256"],
        source::Sha256Digest::of(config.as_bytes()).to_string()
    );
    assert_eq!(
        evidence["sources"],
        serde_json::to_value(inputs.inspect().sources).expect("source evidence")
    );
    assert_eq!(
        evidence["rendering"],
        serde_json::to_value(rendered.site().evidence()).expect("render evidence")
    );
    assert_eq!(evidence["rendering"]["theme"]["tool"], tool);
    assert_eq!(
        evidence["rendering"]["theme"]["limits"],
        serde_json::to_value(limits).expect("requested rendering limits")
    );
    assert_eq!(
        evidence["selection"],
        serde_json::to_value(rendered.selection().expect("complete selected input"))
            .expect("exact selection evidence")
    );
    assert_eq!(evidence["selection"]["content"]["content:one"], "include");
    assert!(!intent.evidence.contains("UNACQUIRED"));
    let retention = receipt
        .record
        .retention
        .as_ref()
        .expect("durable retention");
    assert_eq!(retention.intent_id, intent.id);
    assert_eq!(retention.output, intent.output);
    assert_eq!(retention.entries, intent.entries);
    assert_eq!(retention.bytes, intent.bytes);
    assert_eq!(retention.recorded, receipt.journal);
    let path = intent.address.path();
    assert_eq!(
        source::inspect(&path, None, Default::default()).expect("independent stage inspection"),
        *inventory
    );
    assert_eq!(
        fs::read(path.join("guide/index.html")).expect("native HTML"),
        bytes(rendered.site().output(), "guide/index.html")
    );
    assert_eq!(
        fs::read(fixture.directory.path().join("one/page.md")).expect("source preserved"),
        b"UNACQUIRED SOURCE"
    );
    let inspected = store::inspect_preparation(
        &journal.database,
        request.id,
        Some(&receipt.journal),
        read(),
    )
    .expect("independent history");
    assert_eq!(inspected.record, Some(receipt.record.clone()));
    let stored: Value = serde_json::from_str(
        &inspected
            .record
            .as_ref()
            .expect("stored preparation")
            .intent
            .evidence,
    )
    .expect("independently read private evidence");
    assert_eq!(stored["rendering"]["theme"]["tool"], tool);
    assert_eq!(
        stored["rendering"]["theme"]["limits"],
        evidence["rendering"]["theme"]["limits"]
    );
    let public_model = fs::read(path.join("model.json")).expect("retained public model");
    assert_eq!(public_model, bytes(rendered.site().output(), "model.json"));
    let public_text = std::str::from_utf8(&public_model).expect("public JSON text");
    assert!(
        !public_text.contains(tool),
        "private provider evidence is not a public fact"
    );
    assert!(!public_text.contains("context_bytes"));
    assert!(!public_text.contains("output_bytes"));

    // A database read proves history, not the perpetual correctness of its locator.
    fs::write(path.join("guide/index.html"), "later unrelated bytes")
        .expect("alter disposable retained stage");
    assert_ne!(
        source::inspect(&path, None, Default::default())
            .expect("current native inspection")
            .identity(),
        intent.output
    );
    let history = store::inspect_preparation(
        &journal.database,
        request.id,
        Some(&receipt.journal),
        read(),
    )
    .expect("historical read without repair");
    assert_eq!(history.record, Some(receipt.record));
    assert_eq!(
        fs::read(path.join("guide/index.html")).expect("no repair"),
        b"later unrelated bytes"
    );
}

#[test]
fn complete_candidate_selection_is_bound_in_private_preparation_evidence() {
    let fixture = published_fixture(&[]);
    let inputs = acquire(&fixture.config());
    let parsed = inputs
        .parse_publication(Default::default(), &gazette_adapter_markdown::Comrak)
        .expect("retained syntax");
    let resolved = parsed.resolve(Default::default()).expect("complete graph");
    let selection = authored::Selection {
        content: resolved
            .authored()
            .content()
            .graph()
            .nodes()
            .map(|node| {
                (
                    node.id.clone(),
                    if node.id.as_str() == "collection:home" {
                        Disposition::Include
                    } else {
                        Disposition::Exclude
                    },
                )
            })
            .collect(),
        assets: BTreeMap::new(),
    };
    let expected = serde_json::to_value(&selection).expect("exact decision set");
    let selected = resolved
        .into_selected(selection)
        .expect("explicit complete candidate");
    let rendered = build::render_selected_publication(
        &selected,
        build::SiteRenderOptions {
            render_year: None,
            model_output: Some(&"model.json".parse().expect("model output")),
            graph: None,
            limits: Default::default(),
        },
        Default::default(),
    )
    .expect("selected local render");
    let journal_root = private_directory();
    let journal = initialize(journal_root.path());
    let parent = private_directory();
    let receipt = build::prepare_stage(
        &rendered,
        request(
            journal_root.path(),
            journal.header,
            parent.path(),
            "candidate",
        ),
        options(),
    )
    .expect("candidate preparation is not admission");
    let evidence: Value = serde_json::from_str(&receipt.record.intent.evidence).expect("evidence");
    assert_eq!(evidence["selection"], expected);
    assert_eq!(
        receipt.record.intent.output,
        rendered.site().output().inventory().identity()
    );
    assert!(!receipt.record.intent.address.path().join("guide").exists());
    assert!(
        receipt
            .record
            .intent
            .address
            .path()
            .join("index.html")
            .is_file()
    );
    let inspected = store::inspect_preparation(
        &journal.database,
        receipt.record.intent.id,
        Some(&receipt.journal),
        read(),
    )
    .expect("candidate history");
    assert_eq!(inspected.record, Some(receipt.record));
}

#[test]
fn exact_replay_never_adopts_or_recreates_a_retained_stage() {
    let fixture = published_fixture(&[]);
    let inputs = acquire(&fixture.config());
    let rendered = render(&inputs).expect("render");
    let journal_root = private_directory();
    let journal = initialize(journal_root.path());
    let parent = private_directory();
    let mut request = request(
        journal_root.path(),
        journal.header,
        parent.path(),
        "prepared",
    );
    let receipt = build::prepare_stage(&rendered, request, options()).expect("initial preparation");
    let path = receipt.record.intent.address.path();
    request.expected = receipt.journal;
    for remove in [false, true] {
        if remove {
            fs::remove_dir_all(&path).expect("remove only this test's retained stage");
        }
        let failure = build::prepare_stage(&rendered, request, options())
            .expect_err("replay requires reconciliation");
        assert!(matches!(
            failure.cause.as_ref(),
            build::PreparationFailure::Replay
        ));
        assert_eq!(failure.stage.as_deref(), Some(path.as_path()));
        let replay = failure.receipt.expect("exact historical receipt");
        assert!(!replay.inserted);
        assert_eq!(replay.record, receipt.record);
        assert_eq!(failure.journal, Some(replay.journal));
        assert_eq!(path.exists(), !remove);
        request.expected = replay.journal;
    }
    let inspected = store::inspect_preparation(
        &journal.database,
        request.id,
        Some(&request.expected),
        read(),
    )
    .expect("history survives absent stage");
    assert_eq!(inspected.record, Some(receipt.record));
    assert_eq!(
        fs::read_dir(parent.path()).expect("stage parent").count(),
        0
    );
}

#[test]
fn collisions_preserve_existing_bytes_and_durable_intent_without_retention() {
    let fixture = published_fixture(&[]);
    let inputs = acquire(&fixture.config());
    let rendered = render(&inputs).expect("render");
    for kind in ["file", "directory", "symlink"] {
        let journal_root = private_directory();
        let journal = initialize(journal_root.path());
        let parent = private_directory();
        let target = private_directory();
        fs::write(target.path().join("owned"), b"unrelated target").expect("target sentinel");
        let path = parent.path().join("prepared");
        match kind {
            "file" => fs::write(&path, b"existing file").expect("collision file"),
            "directory" => {
                fs::create_dir(&path).expect("collision directory");
                fs::write(path.join("owned"), b"existing directory").expect("directory sentinel");
            }
            _ => std::os::unix::fs::symlink(target.path(), &path).expect("collision symlink"),
        }
        let request = request(
            journal_root.path(),
            journal.header,
            parent.path(),
            "prepared",
        );
        let failure = build::prepare_stage(&rendered, request, options())
            .expect_err("exclusive creation refuses collision");
        assert!(
            matches!(failure.cause.as_ref(), build::PreparationFailure::Stage(_)),
            "{kind}: {failure:?}"
        );
        let receipt = failure
            .receipt
            .expect("intent precedes attempted stage creation");
        assert_eq!(failure.stage, Some(receipt.record.intent.address.path()));
        assert!(receipt.inserted);
        assert!(receipt.record.retention.is_none());
        let inspected = store::inspect_preparation(
            &journal.database,
            request.id,
            Some(&receipt.journal),
            read(),
        )
        .expect("independent incomplete history");
        assert_eq!(inspected.record, Some(receipt.record));
        assert_eq!(
            fs::read_dir(parent.path())
                .expect("no fallback stage")
                .count(),
            1
        );
        match kind {
            "file" => assert_eq!(fs::read(&path).expect("file preserved"), b"existing file"),
            "directory" => assert_eq!(
                fs::read(path.join("owned")).expect("directory preserved"),
                b"existing directory"
            ),
            _ => assert_eq!(
                fs::read_link(&path).expect("symlink preserved"),
                target.path()
            ),
        }
        assert_eq!(
            fs::read(target.path().join("owned")).expect("target untouched"),
            b"unrelated target"
        );
    }
}

#[test]
fn permissions_and_bounds_fail_before_writer_or_stage_effects() {
    let fixture = published_fixture(&[]);
    let inputs = acquire(&fixture.config());
    let rendered = render(&inputs).expect("render");
    let journal_root = private_directory();
    let journal = initialize(journal_root.path());
    let database = fs::read(&journal.database).expect("initial database");
    let parent = private_directory();
    let request = request(
        journal_root.path(),
        journal.header,
        parent.path(),
        "prepared",
    );
    for case in 0..8 {
        let mut options = options();
        match case {
            0 => options.allow_stage = false,
            1 => options.record.allow_record = false,
            2 => options.writer.allow_generation_advance = false,
            3 => options.writer.session.allow_local_reservation = false,
            4 => options.writer.session.read.allow_coordination_files = false,
            5 => options.evidence_bytes = 0,
            6 => options.output.total_bytes = 0,
            _ => options.output.entries = 0,
        }
        let failure = build::prepare_stage(&rendered, request, options)
            .expect_err("explicit independent gate");
        match case {
            0 => assert!(matches!(
                failure.cause.as_ref(),
                build::PreparationFailure::Permission
            )),
            1..=4 => assert!(matches!(
                failure.cause.as_ref(),
                build::PreparationFailure::Journal(_)
            )),
            5 => assert!(matches!(
                failure.cause.as_ref(),
                build::PreparationFailure::Evidence(_)
            )),
            _ => assert!(matches!(
                failure.cause.as_ref(),
                build::PreparationFailure::Stage(_)
            )),
        }
        assert!(failure.receipt.is_none());
        assert!(failure.journal.is_none());
        assert!(failure.stage.is_none());
        unchanged(&journal, request.id, &database);
        assert_eq!(fs::read_dir(parent.path()).expect("no stage").count(), 0);
    }
}

#[test]
fn invalid_names_and_source_journal_stage_overlap_fail_before_writer() {
    let fixture = published_fixture(&[]);
    let inputs = acquire(&fixture.config());
    let rendered = render(&inputs).expect("render");
    let journal_root = private_directory();
    let journal = initialize(journal_root.path());
    let database = fs::read(&journal.database).expect("initial database");
    let parent = private_directory();
    for name in ["", ".", "..", "../escape", "nested/stage", "/absolute"] {
        let request = request(journal_root.path(), journal.header, parent.path(), name);
        let failure =
            build::prepare_stage(&rendered, request, options()).expect_err("invalid exact child");
        assert!(matches!(
            failure.cause.as_ref(),
            build::PreparationFailure::Stage(_)
        ));
        assert!(failure.receipt.is_none());
        unchanged(&journal, request.id, &database);
    }
    for parent in [fixture.directory.path(), journal_root.path()] {
        let request = request(journal_root.path(), journal.header, parent, "prepared");
        let failure = build::prepare_stage(&rendered, request, options())
            .expect_err("stage/source or stage/journal overlap");
        assert!(matches!(
            failure.cause.as_ref(),
            build::PreparationFailure::Stage(_)
        ));
        assert!(failure.receipt.is_none());
        unchanged(&journal, request.id, &database);
        assert!(!parent.join("prepared").exists());
    }
    // A journal nested in an acquired source is unsafe even when no selected bytes changed.
    let nested_root = fixture.directory.path().join("private-journal");
    fs::create_dir(&nested_root).expect("disposable nested journal");
    fs::set_permissions(&nested_root, fs::Permissions::from_mode(0o700)).expect("private root");
    let nested = initialize(&nested_root);
    let nested_bytes = fs::read(&nested.database).expect("nested database");
    let request = request(&nested_root, nested.header, parent.path(), "prepared");
    let failure =
        build::prepare_stage(&rendered, request, options()).expect_err("journal/source overlap");
    assert!(matches!(
        failure.cause.as_ref(),
        build::PreparationFailure::Separation(_)
    ));
    assert!(failure.receipt.is_none());
    unchanged(&nested, request.id, &nested_bytes);
    assert_eq!(fs::read_dir(parent.path()).expect("no stage").count(), 0);
}

#[test]
fn unsupported_schema_refuses_preparation_without_mutating_database_or_stage() {
    let fixture = published_fixture(&[]);
    let inputs = acquire(&fixture.config());
    let rendered = render(&inputs).expect("render");
    let journal_root = private_directory();
    let journal = initialize(journal_root.path());
    let connection = rusqlite::Connection::open(&journal.database).expect("disposable database");
    connection
        .execute_batch("PRAGMA user_version=999")
        .expect("unsupported schema fixture");
    connection.close().expect("close fixture mutation");
    let before = fs::read(&journal.database).expect("unsupported database bytes");
    let parent = private_directory();
    let failure = build::prepare_stage(
        &rendered,
        request(
            journal_root.path(),
            journal.header,
            parent.path(),
            "prepared",
        ),
        options(),
    )
    .expect_err("unsupported schema cannot acquire writer");
    assert!(matches!(
        failure.cause.as_ref(),
        build::PreparationFailure::Writer(_)
    ));
    assert!(failure.receipt.is_none());
    assert!(failure.journal.is_none());
    assert!(failure.stage.is_none());
    assert_eq!(
        fs::read(&journal.database).expect("no schema repair"),
        before
    );
    assert_eq!(fs::read_dir(parent.path()).expect("no stage").count(), 0);
}
