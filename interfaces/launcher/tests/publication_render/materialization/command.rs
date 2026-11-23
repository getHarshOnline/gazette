//! Actual held-command custody, native complete-set output and pre-effect refusal.

#[path = "command/checkout.rs"]
mod checkout;

use std::{cell::Cell, collections::BTreeMap, os::unix::fs::PermissionsExt};

use gazette_cli::command::{Kernel, publication, store_session as command};
use gazette_sdk::{build, source, store, toml, workspace::Key};

use super::super::{fs, sites, tempdir};
use super::{Fixture, configuration, initialize, options, session};

const SELECTED: [&str; 2] = ["site:one", "site:other"];

fn key(value: &str) -> Key {
    value.parse().expect("exact selected site")
}
fn ids() -> BTreeMap<Key, source::Sha256Digest> {
    SELECTED
        .into_iter()
        .map(|site| {
            (
                key(site),
                source::Sha256Digest::of(format!("held-command:{site}").as_bytes()),
            )
        })
        .collect()
}
fn render(recipe_json: Vec<u8>) -> publication::Render {
    publication::Render {
        math: None,
        allow_math_execution: false,
        default_render_year: None,
        recipe_json,
        local: true,
        allow_read: true,
        max_input_bytes: 1 << 20,
        syntax: toml::Limits::default(),
        acquisition: build::PublicationSetAcquisitionLimits::default(),
        output: source::Limits::default(),
        limits: build::PublicationSetRenderLimits::default(),
    }
}
fn open_reserve(_: &str, _: &command::Selection) -> Result<(), &'static str> {
    Ok(())
}
fn operation_reserve(
    _: &str,
    _: &command::Selection,
    _: Option<store::JournalHeader>,
    _: bool,
) -> Result<(), &'static str> {
    Ok(())
}
fn open_writer(
    kernel: &Kernel,
    root: &std::path::Path,
    header: store::JournalHeader,
) -> store::JournalHeader {
    let opened = kernel
        .execute(command::Open {
            handle: "writer".into(),
            selection: command::Selection::Writer {
                root: root.into(),
                expected_header: header,
                allow_native_reads: true,
                options: store::WriterOptions {
                    allow_generation_advance: true,
                    session: session(),
                },
            },
            reserve: open_reserve,
        })
        .expect("native held writer acquisition");
    assert!(opened.active);
    let command::Outcome::Acquired { inspection } = opened.outcome else {
        panic!("actual ready writer required")
    };
    assert_eq!(inspection.header.writer_fence, header.writer_fence + 1);
    inspection.header
}
fn operate(kernel: &Kernel, handle: &str, close: bool) -> command::Report {
    kernel
        .execute(command::Operate {
            handle: handle.into(),
            close,
            reserve: operation_reserve,
        })
        .expect("explicit held operation")
}

#[test]
fn actual_two_site_command_materializes_once_and_retains_writer_until_explicit_close() {
    let fixture = Fixture::new(&[]);
    let parent = tempdir().expect("native disjoint outputs");
    let roots = [parent.path().join("one"), parent.path().join("other")];
    for root in &roots {
        fs::create_dir(root).expect("existing destination");
        fs::write(root.join("user.txt"), b"independent owner bytes").expect("preserved sentinel");
    }
    let config = configuration(&fixture, &roots[0]).replacen(
        "site='site:other'\npublic_base_url",
        &format!(
            "site='site:other'\noutput={{directory={:?}}}\npublic_base_url",
            roots[1].to_str().expect("native output")
        ),
        1,
    );
    let acquired = sites::acquire_set(&config, &SELECTED);
    let page = "page.html".parse().expect("page");
    let alias = "alias.html".parse().expect("alias");
    let model = "model.json".parse().expect("model");
    let expected = build::render_publications(
        &acquired,
        sites::options(&SELECTED, &page, &alias, &model),
        Default::default(),
    )
    .expect("independent native SDK retained render");
    assert!(expected.is_complete());
    let source_before = source::inspect(fixture.directory.path(), None, Default::default())
        .expect("complete source baseline")
        .identity();
    let (journal, initialized) = initialize();
    let kernel = Kernel::new();
    let header = open_writer(&kernel, journal.path(), initialized.header);
    let reserved = Cell::new(0);
    let operation_ids = ids();
    let report = kernel
        .execute(command::Materialize {
            checkouts: None,
            handle: "writer".into(),
            render: render(
                serde_json::to_vec(&sites::request(&config, &SELECTED))
                    .expect("exact native request"),
            ),
            ids: operation_ids.clone(),
            limits: source::Limits::default(),
            options: options(),
            reserve: |handle: &str,
                      selection: &command::Selection,
                      original: Option<store::JournalHeader>| {
                assert_eq!(handle, "writer");
                assert!(matches!(selection, command::Selection::Writer { .. }));
                assert_eq!(original, Some(header));
                assert!(roots.iter().all(|root| !root.join("model.json").exists()));
                reserved.set(reserved.get() + 1);
                Ok::<(), &'static str>(())
            },
        })
        .expect("existing writer command admitted");
    assert_eq!(reserved.get(), 1);
    assert!(report.active);
    assert_eq!(report.session, "writer");
    assert_eq!(report.acquired_header, Some(header));
    let results = report.outcome.expect("shared complete-set execution");
    assert_eq!(
        results.keys().cloned().collect::<Vec<_>>(),
        operation_ids.keys().cloned().collect::<Vec<_>>()
    );
    for (site, root) in SELECTED.into_iter().zip(&roots) {
        let result = &results[&key(site)];
        let retained = expected.sites()[&key(site)]
            .as_ref()
            .expect("complete expected site")
            .site()
            .output();
        assert!(result.history.inserted);
        assert_eq!(result.history.record.intent.id, operation_ids[&key(site)]);
        let completion = result
            .history
            .record
            .completion
            .as_ref()
            .expect("durable native read-back");
        completion
            .validate(&result.history.record.intent)
            .expect("exact completed linkage");
        let actual = result
            .output
            .as_ref()
            .expect("actual current write, not historical replay");
        assert_eq!(actual.generated(), retained.inventory());
        assert_eq!(actual.inventory(), &result.history.record.intent.expected);
        for entry in retained.inventory().entries() {
            match entry.content {
                source::Content::Directory => assert!(root.join(entry.path.as_str()).is_dir()),
                source::Content::File { .. } => assert_eq!(
                    fs::read(root.join(entry.path.as_str())).expect("actual output bytes"),
                    retained.file(&entry.path).expect("retained SDK bytes")
                ),
            }
        }
        assert_eq!(
            fs::read(root.join("user.txt")).expect("owner sentinel"),
            b"independent owner bytes"
        );
    }
    assert_eq!(
        source::inspect(fixture.directory.path(), None, Default::default())
            .expect("source remains exact")
            .identity(),
        source_before
    );
    let inspected = operate(&kernel, "writer", false);
    assert!(inspected.active);
    assert_eq!(inspected.acquired_header, Some(header));
    let command::Outcome::Inspected { inspection } = inspected.outcome else {
        panic!("same actual writer remains held")
    };
    assert_eq!(
        inspection.header, header,
        "command does not reacquire or advance the held generation"
    );
    let closed = operate(&kernel, "writer", true);
    assert!(!closed.active);
    assert!(matches!(closed.outcome, command::Outcome::Closed));
    for (site, root) in SELECTED.into_iter().zip(&roots) {
        let history = store::inspect_materialization(
            &initialized.database,
            &root.canonicalize().expect("native root"),
            Some(&header),
            session().read,
        )
        .expect("independent final durable history");
        assert!(history.frontier.active.is_none());
        assert_eq!(
            history
                .frontier
                .head
                .expect("actual confirmed head")
                .intent
                .id,
            operation_ids[&key(site)]
        );
    }
}

#[test]
fn unknown_and_connection_handles_refuse_before_reservation_or_invalid_render() {
    let (_journal, initialized) = initialize();
    let kernel = Kernel::new();
    let coordination = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .expect("private read-only session root");
    let opened = kernel
        .execute(command::Open {
            handle: "connection".into(),
            selection: command::Selection::Connection {
                path: initialized.database.clone(),
                coordination_root: coordination.path().into(),
                allow_native_reads: true,
                options: session(),
            },
            reserve: open_reserve,
        })
        .expect("actual connection admission");
    assert!(opened.active);
    assert!(matches!(opened.outcome, command::Outcome::Connected { .. }));
    let before = fs::read(&initialized.database).expect("exact database baseline");
    let reservations = Cell::new(0);
    for handle in ["unknown", "connection"] {
        let refused = kernel
            .execute(command::Materialize {
                checkouts: None,
                handle: handle.into(),
                render: render(b"deliberately invalid JSON and no readable source".to_vec()),
                ids: ids(),
                limits: source::Limits::default(),
                options: options(),
                reserve: |_: &str, _: &command::Selection, _: Option<store::JournalHeader>| {
                    reservations.set(reservations.get() + 1);
                    Ok::<(), &'static str>(())
                },
            })
            .expect_err("no writer capability from unknown or read-only handle");
        match handle {
            "unknown" => assert!(matches!(refused, command::Failure::Unknown)),
            "connection" => assert!(matches!(refused, command::Failure::NotWriter)),
            _ => unreachable!("enumerated handles"),
        }
        assert_eq!(reservations.get(), 0);
        assert_eq!(
            fs::read(&initialized.database).expect("no journal changes"),
            before
        );
    }
    assert!(matches!(
        operate(&kernel, "connection", false).outcome,
        command::Outcome::Observed { .. }
    ));
    assert!(matches!(
        operate(&kernel, "connection", true).outcome,
        command::Outcome::Closed
    ));
}

#[test]
fn response_reservation_refusal_and_reentrant_busy_precede_all_native_render_effects() {
    let fixture = Fixture::new(&[]);
    let destination = tempdir().expect("untouched output");
    let config = configuration(&fixture, destination.path());
    let source_before = source::inspect(fixture.directory.path(), None, Default::default())
        .expect("source baseline")
        .identity();
    let (journal, initialized) = initialize();
    let kernel = Kernel::new();
    let header = open_writer(&kernel, journal.path(), initialized.header);
    let before = fs::read(&initialized.database).expect("post-acquisition baseline");
    let reservations = Cell::new(0);
    let refused = kernel
        .execute(command::Materialize {
            checkouts: None,
            handle: "writer".into(),
            render: render(
                serde_json::to_vec(&sites::request(&config, &SELECTED))
                    .expect("native render request"),
            ),
            ids: ids(),
            limits: source::Limits::default(),
            options: options(),
            reserve: |_: &str, _: &command::Selection, original: Option<store::JournalHeader>| {
                assert_eq!(original, Some(header));
                reservations.set(reservations.get() + 1);
                let blocked = kernel
                    .execute(command::Operate {
                        handle: "writer".into(),
                        close: false,
                        reserve: operation_reserve,
                    })
                    .expect_err("registry is held through response reservation");
                assert!(matches!(blocked, command::Failure::Busy));
                Err::<(), &'static str>("complete response custody unavailable")
            },
        })
        .expect_err("reservation precedes native publication work");
    assert!(matches!(
        refused,
        command::Failure::Reservation("complete response custody unavailable")
    ));
    assert_eq!(reservations.get(), 1);
    let native_failure = kernel
        .execute(command::Materialize {
            checkouts: None,
            handle: "writer".into(),
            render: render(b"invalid complete recipe".to_vec()),
            ids: ids(),
            limits: source::Limits::default(),
            options: options(),
            reserve: |_: &str, _: &command::Selection, _: Option<store::JournalHeader>| {
                Ok::<(), &'static str>(())
            },
        })
        .expect("reservation success retains typed native failure report");
    assert!(native_failure.active);
    assert_eq!(native_failure.acquired_header, Some(header));
    let Err(command::MaterializationFailure::Render(error)) = native_failure.outcome else {
        panic!("exact original publication failure remains typed")
    };
    assert_eq!(error.code(), "publication_set_input");
    assert_eq!(
        fs::read(&initialized.database).expect("journal untouched"),
        before
    );
    assert_eq!(
        fs::read_dir(destination.path())
            .expect("output namespace remains")
            .count(),
        0
    );
    assert_eq!(
        source::inspect(fixture.directory.path(), None, Default::default())
            .expect("sources unchanged")
            .identity(),
        source_before
    );
    let inspected = operate(&kernel, "writer", false);
    assert!(inspected.active);
    let command::Outcome::Inspected { inspection } = inspected.outcome else {
        panic!("reservation failure preserves exact native custody")
    };
    assert_eq!(inspection.header, header);
    assert!(matches!(
        operate(&kernel, "writer", true).outcome,
        command::Outcome::Closed
    ));
    let history = store::inspect_materialization(
        &initialized.database,
        &destination.path().canonicalize().expect("native root"),
        Some(&header),
        session().read,
    )
    .expect("no fabricated history after refusal");
    assert!(history.frontier.head.is_none());
    assert!(history.frontier.active.is_none());
}
