//! Native `SQLite`/Git session with fault injection only at lifetime-close delivery.

use gazette_adapter_sqlite::{Sqlite, Writer};
use gazette_core::{source::Sha256Digest, store::*};
use gazette_ports::store::{JournalWriter, RefObservationWriter, WriterConnector};
use gazette_sdk::release;
use serde_json::json;
use std::{os::unix::fs::PermissionsExt, path::Path};

use gazette_test_support as native;

struct CloseFailure(Writer);
struct Connector;

impl WriterConnector for Connector {
    type Writer = CloseFailure;
    fn acquire_writer(
        &self,
        root: &Path,
        expected: JournalHeader,
        options: WriterOptions,
    ) -> Result<CloseFailure, WriterAcquireError> {
        Sqlite
            .acquire_writer(root, expected, options)
            .map(CloseFailure)
    }
}
impl JournalWriter for CloseFailure {
    fn inspect(&mut self) -> Result<JournalInspection, Error> {
        self.0.inspect()
    }
    fn close(self) -> Result<(), Error> {
        self.0.close()?;
        // Only this callback is faulted: all SQL/Git, append and read-back work is native.
        Err(Error::Changed)
    }
}
impl RefObservationWriter for CloseFailure {
    fn inspect_ref_observation(
        &mut self,
        id: Sha256Digest,
    ) -> Result<RefObservationInspection, Error> {
        self.0.inspect_ref_observation(id)
    }
    fn record_ref_observation(
        &mut self,
        input: RefObservationInput<'_>,
        options: RefObservationWriteOptions,
    ) -> Result<RefObservationReceipt, RefObservationWriteError> {
        self.0.record_ref_observation(input, options)
    }
}

fn private() -> tempfile::TempDir {
    tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .expect("native fixture operation or asserted evidence shape")
}

#[test]
fn successful_append_and_failed_append_both_keep_independent_close_failure_evidence() {
    for append_fits in [true, false] {
        let root = private();
        let isolation = private();
        let destination = private();
        git2::Repository::init_bare(destination.path()).expect("real fixture native destination");
        let read = ReadOptions {
            allow_coordination_files: true,
            ..Default::default()
        };
        let journal = gazette_sdk::store::initialize(
            root.path(),
            InitializeOptions {
                allow_create: true,
                session: SessionOptions {
                    allow_local_reservation: true,
                    read,
                },
                ..Default::default()
            },
        )
        .expect("native fixture operation or asserted evidence shape");
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../crates/gazette-compiler/tests/release_authorization/authorization.json"
        ))
        .expect("native fixture operation or asserted evidence shape");
        let template: release::Target =
            serde_json::from_value(fixture["ordered_targets"][0].clone())
                .expect("native fixture operation or asserted evidence shape");
        let targets: Vec<_> = (0..if append_fits { 1 } else { 12 })
            .map(|ordinal| {
                let mut target = template.clone();
                target.ordinal = ordinal;
                target.domain = format!("{ordinal}.close.example");
                target.remote = destination
                    .path()
                    .to_str()
                    .expect("native fixture operation or asserted evidence shape")
                    .into();
                target.branch = format!("observed-{ordinal}");
                target.refspec =
                    format!("{}:refs/heads/{}", target.local_commit_oid, target.branch);
                target
            })
            .collect();
        let targets = release::check_targets(&targets, Default::default())
            .expect("native fixture operation or asserted evidence shape");
        let id = Sha256Digest::of(b"native close-failure observation");
        let git = release::GitSelection {
            program: native::git().to_owned(),
            isolation: isolation.path().to_path_buf(),
            credentials: None,
        };
        let reader = gazette_adapter_git::RemoteReader {
            runner: &gazette_adapter_process::Runner,
            selection: &git,
        };
        let result = gazette_application::release::record_observation_session(
            &gazette_adapter_fs::FileSystem,
            &Connector,
            &reader,
            release::ObservationSessionRequest {
                root: root.path(),
                expected: journal.header,
                id,
                targets: &targets,
                protected_roots: &[isolation.path().to_owned(), destination.path().to_owned()],
                coordinator: None,
            },
            release::ObservationSessionOptions {
                allow_anchor_write: false,
                writer: WriterOptions {
                    allow_generation_advance: true,
                    session: SessionOptions {
                        allow_local_reservation: true,
                        read: ReadOptions {
                            value_bytes: if append_fits { 1 << 20 } else { 4096 },
                            ..read
                        },
                    },
                },
                observation: release::RecordObservationOptions {
                    read: release::RemoteReadOptions {
                        allow_read: true,
                        ..Default::default()
                    },
                    record: RefObservationWriteOptions { allow_record: true },
                },
            },
        );
        let projected = json!(release::ObservationSessionReport::new(&result));
        let history = release::inspect_target_observation(
            &journal.database,
            id,
            None,
            read,
            Default::default(),
        )
        .expect("native fixture operation or asserted evidence shape");
        assert_eq!(
            history.journal.header.writer_fence,
            journal.header.writer_fence + 1
        );
        if append_fits {
            let Err(release::ObservationSessionError::Close { receipt, cause }) = result else {
                panic!("append receipt must survive close failure")
            };
            assert!(matches!(cause, Error::Changed));
            assert_eq!(
                history
                    .record
                    .expect("native fixture operation or asserted evidence shape"),
                receipt.record
            );
            assert_eq!(projected["status"], "close-failed");
            assert_eq!(projected["receipt"], json!(receipt));
        } else {
            let Err(release::ObservationSessionError::Record { cause, close_error }) = result
            else {
                panic!("record and close failures must stay distinct")
            };
            assert!(matches!(close_error, Some(Error::Changed)));
            let release::RecordObservationError::Write { report, cause } = *cause else {
                panic!("completed provider rows remain present")
            };
            assert!(matches!(cause.cause, Error::StorageLimit));
            assert_eq!(report.targets.len(), 12);
            assert!(history.record.is_none());
            assert_eq!(projected["status"], "recording-failed");
            assert_eq!(projected["failure"]["report"], json!(report));
            assert_eq!(projected["close_error"], Error::Changed.code());
        }
        assert!(
            std::fs::read_dir(isolation.path())
                .expect("native fixture operation or asserted evidence shape")
                .next()
                .is_none()
        );
        assert!(
            git2::Repository::open_bare(destination.path())
                .expect("native fixture operation or asserted evidence shape")
                .references()
                .expect("native fixture operation or asserted evidence shape")
                .next()
                .is_none()
        );
    }
}
