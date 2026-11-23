//! Real SDK writer, `SQLite` and Git evidence under independently retained local custody.

use gazette_sdk::{release, source::Sha256Digest, store};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};
use store::coordinator::{self, EnrollmentRequest, LocalCoordinatorState};

use gazette_test_support as native;

fn private() -> tempfile::TempDir {
    tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .expect("private fixture directory")
}

fn options() -> store::WriterOptions {
    store::WriterOptions {
        allow_generation_advance: true,
        session: store::SessionOptions {
            allow_local_reservation: true,
            read: store::ReadOptions {
                allow_coordination_files: true,
                ..Default::default()
            },
        },
    }
}

struct Fixture {
    anchor: tempfile::TempDir,
    journal: tempfile::TempDir,
    initialized: store::InitializationReceipt,
    seal: coordinator::LocalCoordinatorSeal,
}

impl Fixture {
    fn new() -> Self {
        let anchor = private();
        let journal = private();
        let initialized = store::initialize(
            journal.path(),
            store::InitializeOptions {
                allow_create: true,
                session: options().session,
                ..Default::default()
            },
        )
        .expect("native fresh journal");
        let seal = coordinator::enroll(
            EnrollmentRequest {
                anchor_root: anchor.path(),
                journal_root: journal.path(),
                coordinator_id: "local-coordinator-fixture",
                principal_scope: Sha256Digest::of(b"fixture local principal"),
                protected_roots: &[],
            },
            options().session,
            true,
        )
        .expect("fresh local enrollment");
        Self {
            anchor,
            journal,
            initialized,
            seal,
        }
    }

    fn inspect(&self) -> LocalCoordinatorState {
        coordinator::inspect(self.anchor.path(), &self.seal.binding, options().session)
            .expect("independent control-state inspection")
    }

    fn acquire(
        &self,
        header: store::JournalHeader,
    ) -> Result<store::Writer, store::WriterAcquireError> {
        coordinator::acquire_writer(
            self.anchor.path(),
            &self.seal.binding,
            header,
            options(),
            true,
        )
    }
}

fn target(remote: &Path) -> release::TargetSet {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../crates/gazette-compiler/tests/release_authorization/authorization.json"
    ))
    .expect("typed reference target");
    let mut target: release::Target = serde_json::from_value(fixture["ordered_targets"][0].clone())
        .expect("native target object");
    target.remote = remote.to_str().expect("fixture UTF-8 path").into();
    release::check_targets(&[target], Default::default()).expect("exact target admission")
}

fn managed_options() -> release::ObservationSessionOptions {
    release::ObservationSessionOptions {
        writer: options(),
        observation: release::RecordObservationOptions {
            read: release::RemoteReadOptions {
                allow_read: true,
                ..Default::default()
            },
            record: release::RefObservationWriteOptions { allow_record: true },
        },
        allow_anchor_write: true,
    }
}

#[test]
fn managed_sdk_selects_guard_seals_history_and_refuses_rollback_without_resampling() {
    let fixture = Fixture::new();
    let remote = private();
    let isolation = private();
    git2::Repository::init_bare(remote.path()).expect("actual local destination");
    let targets = target(remote.path());
    let selection = coordinator::LocalCoordinatorSelection {
        anchor_root: fixture.anchor.path().to_owned(),
        binding: fixture.seal.binding.clone(),
    };
    let original = fs::read(&fixture.initialized.database).expect("pre-operation snapshot");
    let record = |program: &Path, expected, id| {
        let git = release::GitSelection {
            program: program.to_path_buf(),
            isolation: isolation.path().to_path_buf(),
            credentials: None,
        };
        release::record_observation(
            &git,
            release::ObservationSessionRequest {
                root: &selection.binding.journal_root,
                expected,
                id: Sha256Digest::of(id),
                targets: &targets,
                protected_roots: &[remote.path().to_owned()],
                coordinator: Some(&selection),
            },
            managed_options(),
        )
    };
    let first = record(native::git(), fixture.seal.header, b"managed first")
        .expect("actual guarded observation and close");
    let second = record(native::git(), first.journal, b"managed second")
        .expect("same independently retained selection reopens the clean journal");
    let clean = fixture.inspect();
    let LocalCoordinatorState::Clean { ref seal, .. } = clean else {
        panic!("managed success requires a successful native seal")
    };
    assert_eq!(seal.header, second.journal);
    assert_eq!(seal.binding, selection.binding);
    assert_eq!(
        seal.database_sha256,
        Sha256Digest::of(&fs::read(&fixture.initialized.database).expect("final journal bytes"))
    );
    let historical = release::inspect_target_observation(
        &fixture.initialized.database,
        first.record.id,
        Some(&second.journal),
        options().session.read,
        Default::default(),
    )
    .expect("independent history, no provider read");
    assert_eq!(
        historical.record.expect("first retained record"),
        first.record
    );
    fs::write(&fixture.initialized.database, &original).expect("disposable rollback fixture only");
    let error = record(
        Path::new("/nonexistent/provider-must-not-run"),
        fixture.seal.header,
        b"refused managed rollback",
    )
    .expect_err("coordinator cannot fall back to an ordinary local writer");
    let release::ObservationSessionError::Acquire(error) = error else {
        panic!("restored generation is refused at guarded acquisition")
    };
    assert!(matches!(error.cause, store::Error::CoordinatorMismatch));
    assert_eq!(error.generation, store::GenerationEffect::NotAttempted);
    assert_eq!(fixture.inspect(), clean);
    assert_eq!(
        fs::read(&fixture.initialized.database).expect("unchanged bytes"),
        original
    );
}

#[test]
fn managed_selection_permission_identity_and_source_separation_precede_any_write() {
    let fixture = Fixture::new();
    let remote = private();
    let isolation = private();
    git2::Repository::init_bare(remote.path()).expect("actual local destination");
    let targets = target(remote.path());
    let original = fs::read(&fixture.initialized.database).expect("original journal");
    for case in ["permission", "identity", "source", "isolation"] {
        let mut selected = coordinator::LocalCoordinatorSelection {
            anchor_root: fixture.anchor.path().to_owned(),
            binding: fixture.seal.binding.clone(),
        };
        let mut options = managed_options();
        if case == "permission" {
            options.allow_anchor_write = false;
        }
        if case == "identity" {
            selected.binding.journal_epoch += 1;
        }
        let protected = if case == "source" {
            vec![fixture.anchor.path().to_owned()]
        } else {
            vec![remote.path().to_owned()]
        };
        let git = release::GitSelection {
            program: PathBuf::from("/nonexistent/provider-must-not-run"),
            isolation: if case == "isolation" {
                fixture.anchor.path().to_path_buf()
            } else {
                isolation.path().to_path_buf()
            },
            credentials: None,
        };
        let result = release::record_observation(
            &git,
            release::ObservationSessionRequest {
                root: &fixture.seal.binding.journal_root,
                expected: fixture.seal.header,
                id: Sha256Digest::of(case.as_bytes()),
                targets: &targets,
                protected_roots: &protected,
                coordinator: Some(&selected),
            },
            options,
        );
        let projected = serde_json::to_value(release::ObservationSessionReport::new(&result))
            .expect("same native failure projection");
        assert_eq!(projected["status"], "not-started", "{case}");
        assert_eq!(projected["failure"]["phase"], "before-read", "{case}");
        let expected = match case {
            "permission" => "store_coordinator_permission",
            "identity" => "store_coordinator_mismatch",
            _ => "store_unsafe_path",
        };
        assert_eq!(projected["failure"]["code"], expected, "{case}");
        assert_eq!(
            fixture.inspect(),
            LocalCoordinatorState::Clean {
                seal: fixture.seal.clone(),
                adoption: None,
            }
        );
        assert_eq!(
            fs::read(&fixture.initialized.database).expect("no journal writes"),
            original
        );
    }
}

#[test]
fn sdk_records_native_history_and_detects_same_generation_rollback_before_new_acquisition() {
    let fixture = Fixture::new();
    let remote = private();
    let isolation = private();
    git2::Repository::init_bare(remote.path()).expect("local bare destination");
    let targets = target(remote.path());
    let first = Sha256Digest::of(b"first exact observation");
    let second = Sha256Digest::of(b"second exact observation");
    let mut writer = fixture
        .acquire(fixture.initialized.header)
        .expect("guarded native writer");
    let record = release::RecordObservationOptions {
        read: release::RemoteReadOptions {
            allow_read: true,
            ..Default::default()
        },
        record: gazette_core::store::RefObservationWriteOptions { allow_record: true },
    };
    let git = release::GitSelection {
        program: native::git().to_owned(),
        isolation: isolation.path().to_path_buf(),
        credentials: None,
    };
    writer
        .record_target_observation(&git, &targets, first, record)
        .expect("first real native observation");
    // No SQL connection is held between operations. Retain an exact older standalone
    // file from this SAME generation, then append more history before closing.
    let older = fs::read(&fixture.initialized.database).expect("earlier same-generation snapshot");
    let receipt = writer
        .record_target_observation(&git, &targets, second, record)
        .expect("second real native observation");
    writer.close().expect("seal after final native read-back");
    let LocalCoordinatorState::Clean { seal, .. } = fixture.inspect() else {
        panic!("successful close must leave a clean exact seal")
    };
    assert_eq!(seal.header, receipt.journal);
    assert_eq!(
        seal.database_sha256,
        Sha256Digest::of(&fs::read(&fixture.initialized.database).expect("sealed bytes"))
    );
    let history = release::inspect_target_observation(
        &fixture.initialized.database,
        second,
        Some(&seal.header),
        options().session.read,
        Default::default(),
    )
    .expect("independent native history");
    assert_eq!(
        history.record.expect("second record remains"),
        receipt.record
    );
    // Simulate restoration only in this disposable fixture, preserving its pathname/inode.
    fs::write(&fixture.initialized.database, &older).expect("controlled earlier snapshot restore");
    let observed = store::inspect_journal(
        &fixture.initialized.database,
        Some(&seal.header),
        options().session.read,
    )
    .expect("a header alone still accepts the same-generation older snapshot");
    assert_eq!(observed.header, seal.header);
    let failure = match fixture.acquire(seal.header) {
        Ok(_) => panic!("exact-byte guard must reject rollback before another writer"),
        Err(error) => error,
    };
    assert_eq!(failure.generation, store::GenerationEffect::NotAttempted);
    assert_eq!(
        fs::read(&fixture.initialized.database).expect("unchanged refused store"),
        older
    );
    assert_eq!(
        fixture.inspect(),
        LocalCoordinatorState::Clean {
            seal,
            adoption: None,
        }
    );
    let old_history = release::inspect_target_observation(
        &fixture.initialized.database,
        second,
        None,
        options().session.read,
        Default::default(),
    )
    .expect("read-only reconciliation remains available");
    assert!(old_history.record.is_none());
}

#[test]
fn sdk_drop_leaves_interrupted_state_and_never_auto_adopts_current_bytes() {
    let fixture = Fixture::new();
    let writer = fixture
        .acquire(fixture.initialized.header)
        .expect("native writer");
    drop(writer);
    let state = fixture.inspect();
    let LocalCoordinatorState::InProgress {
        ref previous,
        reserved_generation,
        ref transaction,
    } = state
    else {
        panic!("drop is not successful explicit close")
    };
    assert_eq!(previous, &fixture.seal);
    assert_eq!(
        transaction
            .as_ref()
            .expect("generation reached its transaction boundary")
            .header
            .writer_fence,
        reserved_generation
    );
    assert_eq!(
        reserved_generation,
        fixture.initialized.header.writer_fence + 1
    );
    let current =
        store::inspect_journal(&fixture.initialized.database, None, options().session.read)
            .expect("historical read allowed after interruption");
    let failure = match fixture.acquire(current.header) {
        Ok(_) => panic!("reading current coordinates cannot clear an interrupted guard"),
        Err(error) => error,
    };
    assert_eq!(failure.generation, store::GenerationEffect::NotAttempted);
    assert_eq!(fixture.inspect(), state);
}

#[test]
fn sdk_enrollment_blocks_protected_inputs_and_permissions_before_custody_changes() {
    let anchor = private();
    let journal = private();
    let initialized = store::initialize(
        journal.path(),
        store::InitializeOptions {
            allow_create: true,
            session: options().session,
            ..Default::default()
        },
    )
    .expect("fresh native journal");
    let before = fs::read(&initialized.database).expect("original journal");
    for (allowed, protected) in [
        (false, vec![]),
        (true, vec![anchor.path().to_owned()]),
        (true, vec![journal.path().to_owned()]),
    ] {
        let result = coordinator::enroll(
            EnrollmentRequest {
                anchor_root: anchor.path(),
                journal_root: journal.path(),
                coordinator_id: "fixture",
                principal_scope: Sha256Digest::of(b"principal"),
                protected_roots: &protected,
            },
            options().session,
            allowed,
        );
        assert!(result.is_err());
        assert!(
            fs::read_dir(anchor.path())
                .expect("anchor remains empty")
                .next()
                .is_none()
        );
        assert_eq!(
            fs::read(&initialized.database).expect("journal unchanged"),
            before
        );
    }
}

#[test]
fn sdk_successful_close_reopens_same_binding_without_pale_and_rejects_another_principal() {
    let fixture = Fixture::new();
    let mut writer = fixture
        .acquire(fixture.initialized.header)
        .expect("first native acquisition");
    let first = writer.inspect().expect("held generation").header;
    writer.close().expect("first clean close");
    let mut changed = fixture.seal.binding.clone();
    changed.principal_scope = Sha256Digest::of(b"another principal");
    assert!(coordinator::inspect(fixture.anchor.path(), &changed, options().session).is_err());
    let mut writer = fixture
        .acquire(first)
        .expect("next acquisition from sealed state");
    assert_eq!(
        writer
            .inspect()
            .expect("next generation")
            .header
            .writer_fence,
        first.writer_fence + 1
    );
    writer.close().expect("second clean close");
    assert!(fixture.journal.path().is_dir());
}
