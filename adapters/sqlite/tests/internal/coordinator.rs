//! Native trusted-local anchor tests; no publisher, host-adversary or power-loss claims.

#[path = "coordinator_adoption.rs"]
mod adoption;
#[path = "coordinator_continuity.rs"]
mod continuity;
#[path = "coordinator_recovery.rs"]
mod recovery;

use super::*;
use gazette_core::{
    release::{
        RemoteReadError, RemoteReadFailure, RemoteState, Target, TargetReadback, TargetReadbackRow,
    },
    store::{
        InitializeOptions, JournalHeader, ReadOptions, RefObservationInput,
        RefObservationWriteOptions,
    },
};
use gazette_ports::store::{JournalReader, JournalWriter, RefObservationWriter, StoreInitializer};
use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt},
};

fn session() -> SessionOptions {
    SessionOptions {
        allow_local_reservation: true,
        read: ReadOptions {
            allow_coordination_files: true,
            ..Default::default()
        },
    }
}
fn options() -> WriterOptions {
    WriterOptions {
        allow_generation_advance: true,
        session: session(),
    }
}
fn private() -> tempfile::TempDir {
    tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .expect("private fixture root")
}

struct Fixture {
    anchor: tempfile::TempDir,
    journal: tempfile::TempDir,
    seal: LocalCoordinatorSeal,
}
impl Fixture {
    fn new() -> Self {
        let anchor = private();
        let journal = private();
        Sqlite
            .initialize(
                journal.path(),
                InitializeOptions {
                    allow_create: true,
                    session: session(),
                    ..Default::default()
                },
            )
            .expect("real fresh journal");
        let seal = LocalCoordinator::enroll(
            anchor.path(),
            journal.path(),
            "coordinator local-Éxact",
            Sha256Digest::of(b"local principal scope, not a credential"),
            session(),
            true,
        )
        .expect("fresh bounded enrollment");
        Self {
            anchor,
            journal,
            seal,
        }
    }
    fn connector(&self) -> LocalCoordinator {
        LocalCoordinator::new(
            self.anchor.path().to_owned(),
            self.seal.binding.clone(),
            true,
        )
    }
    fn state(&self) -> LocalCoordinatorState {
        LocalCoordinator::inspect(self.anchor.path(), &self.seal.binding, session())
            .expect("historical anchor")
    }
    fn database(&self) -> PathBuf {
        self.journal.path().join(journal::DATABASE)
    }
    fn header(&self) -> JournalHeader {
        Sqlite
            .inspect_journal(&self.database(), session().read)
            .expect("native journal")
            .header
    }
}

fn append(writer: &mut Writer, id: &[u8]) {
    try_append(writer, id).expect("real immutable SQL append, not provider truth");
}

fn try_append(
    writer: &mut Writer,
    id: &[u8],
) -> Result<gazette_core::store::RefObservationReceipt, gazette_core::store::RefObservationWriteError>
{
    let oid = "1111111111111111111111111111111111111111"
        .parse()
        .expect("full fixture OID");
    let targets = [Target {
        ordinal: 0,
        domain: "fixture.invalid".into(),
        remote: "/fixture-not-opened.git".into(),
        branch: "main".into(),
        refspec: format!("{oid}:refs/heads/main"),
        expected_remote_oid_or_unborn: RemoteState::Unborn,
        prepared_release_receipt_ref: Sha256Digest::of(b"fixture preparation"),
        local_commit_oid: oid,
        output_digest: Sha256Digest::of(b"fixture output"),
    }];
    let readbacks = [TargetReadbackRow {
        ordinal: 0,
        readback: TargetReadback::Unavailable {
            error: RemoteReadError::new(RemoteReadFailure::Transport),
        },
    }];
    writer.record_ref_observation(
        RefObservationInput {
            id: Sha256Digest::of(id),
            targets: &targets,
            readbacks: &readbacks,
        },
        RefObservationWriteOptions { allow_record: true },
    )
}

#[test]
fn clean_close_binds_every_write_and_rejects_same_header_rollback() {
    let fixture = Fixture::new();
    let mut writer = fixture
        .connector()
        .acquire_writer(fixture.journal.path(), fixture.seal.header, options())
        .expect("coordinated generation");
    append(&mut writer, b"first");
    let old = fs::read(fixture.database()).expect("first committed state");
    let header = writer.inspect().expect("held header").header;
    append(&mut writer, b"second");
    writer.close().expect("seal under both held reservations");
    let LocalCoordinatorState::Clean { seal, .. } = fixture.state() else {
        panic!("successful close is clean");
    };
    assert_eq!(seal.header, header);
    assert_eq!(
        seal.binding.coordinator_id, "coordinator local-Éxact",
        "exact owner spelling survives enrollment, mutation and seal"
    );
    assert_eq!(
        serde_json::to_value(&seal).expect("native seal JSON")["binding"]["coordinator_id"],
        "coordinator local-Éxact"
    );
    assert_eq!(
        seal.database_sha256,
        Sha256Digest::of(&fs::read(fixture.database()).expect("complete sealed database"))
    );
    assert_ne!(seal.database_sha256, Sha256Digest::of(&old));
    let inode = fs::metadata(fixture.database())
        .expect("same native file")
        .ino();
    fs::write(fixture.database(), &old).expect("simulate restored same-generation bytes in place");
    assert_eq!(
        fs::metadata(fixture.database())
            .expect("same file after restore")
            .ino(),
        inode
    );
    assert_eq!(
        fixture.header(),
        header,
        "header-only fencing cannot distinguish the rollback"
    );
    let error = fixture
        .connector()
        .acquire_writer(fixture.journal.path(), header, options())
        .err()
        .expect("sealed bytes refuse rollback");
    assert!(matches!(error.cause, Error::CoordinatorMismatch));
    assert_eq!(
        error.generation,
        gazette_core::store::GenerationEffect::NotAttempted
    );
    assert_eq!(
        fixture.state(),
        LocalCoordinatorState::Clean {
            seal,
            adoption: None,
        }
    );
    assert_eq!(fs::read(fixture.database()).expect("no repair"), old);
}

#[test]
fn dropped_writer_stays_dirty_and_competing_lifetimes_cannot_bypass_it() {
    let fixture = Fixture::new();
    let writer = fixture
        .connector()
        .acquire_writer(fixture.journal.path(), fixture.seal.header, options())
        .expect("first writer");
    let contender = fixture
        .connector()
        .acquire_writer(fixture.journal.path(), fixture.seal.header, options())
        .err()
        .expect("held coordinator reservation");
    assert!(matches!(contender.cause, Error::ReservationBusy));
    drop(writer);
    let expected = LocalCoordinatorState::InProgress {
        previous: fixture.seal.clone(),
        reserved_generation: fixture.seal.header.writer_fence + 1,
        transaction: Some(continuity::captured(&fixture)),
    };
    assert_eq!(fixture.state(), expected);
    let error = fixture
        .connector()
        .acquire_writer(fixture.journal.path(), fixture.header(), options())
        .err()
        .expect("never adopt dirty state");
    assert!(matches!(error.cause, Error::CoordinatorState));
    assert_eq!(fixture.state(), expected);
}

#[test]
fn failed_close_and_changed_guard_preserve_dirty_evidence() {
    let fixture = Fixture::new();
    let writer = fixture
        .connector()
        .acquire_writer(fixture.journal.path(), fixture.seal.header, options())
        .expect("held coordinated writer");
    fs::write(
        fixture.journal.path().join("unexpected"),
        b"native fixture interruption",
    )
    .expect("changed journal namespace");
    assert!(matches!(
        writer.close(),
        Err(Error::CoordinatorUnverified(_))
    ));
    assert!(matches!(
        fixture.state(),
        LocalCoordinatorState::InProgress { .. }
    ));

    let fixture = Fixture::new();
    let mut writer = fixture
        .connector()
        .acquire_writer(fixture.journal.path(), fixture.seal.header, options())
        .expect("held writer");
    let before = fs::read(fixture.database()).expect("before forbidden write");
    fs::write(
        fixture.anchor.path().join("coordinator.next"),
        b"retained interrupted atomic write",
    )
    .expect("fixture residue");
    assert!(
        writer.inspect().is_err(),
        "guard refuses extra anchor state before SQL work"
    );
    assert!(matches!(
        writer.close(),
        Err(Error::CoordinatorUnverified(_))
    ));
    assert_eq!(
        fs::read(fixture.database()).expect("journal unchanged"),
        before
    );
    assert!(matches!(
        LocalCoordinator::inspect(fixture.anchor.path(), &fixture.seal.binding, session()),
        Err(Error::CoordinatorState)
    ));
    assert!(
        fixture.anchor.path().join("coordinator.next").exists(),
        "never remove uncertain residue"
    );
}

#[test]
fn exact_binding_and_fixed_root_refuse_copy_and_principal_substitution() {
    let fixture = Fixture::new();
    let copied = private();
    fs::copy(fixture.database(), copied.path().join(journal::DATABASE))
        .expect("same-coordinate database copy");
    assert!(matches!(
        fixture
            .connector()
            .acquire_writer(copied.path(), fixture.seal.header, options())
            .err()
            .expect("fixed root")
            .cause,
        Error::CoordinatorMismatch
    ));
    let mut wrong = fixture.seal.binding.clone();
    wrong.principal_scope = Sha256Digest::of(b"another scope");
    assert!(matches!(
        LocalCoordinator::inspect(fixture.anchor.path(), &wrong, session()),
        Err(Error::CoordinatorMismatch)
    ));
    wrong = fixture.seal.binding.clone();
    wrong.coordinator_id = "coordinator:another".to_owned();
    assert!(matches!(
        LocalCoordinator::inspect(fixture.anchor.path(), &wrong, session()),
        Err(Error::CoordinatorMismatch)
    ));
    assert_eq!(fixture.header(), fixture.seal.header);
}

#[test]
fn enrollment_and_limits_never_adopt_existing_or_unbounded_state() {
    let fixture = Fixture::new();
    assert!(
        LocalCoordinator::enroll(
            fixture.anchor.path(),
            fixture.journal.path(),
            &fixture.seal.binding.coordinator_id,
            fixture.seal.binding.principal_scope,
            session(),
            true
        )
        .is_err(),
        "never overwrite an anchor"
    );
    let empty = private();
    let mut writer = Sqlite
        .acquire_writer(fixture.journal.path(), fixture.seal.header, options())
        .expect("ordinary writer remains a separate unqualified API");
    append(&mut writer, b"prior history");
    writer.close().expect("ordinary close");
    assert!(matches!(
        LocalCoordinator::enroll(
            empty.path(),
            fixture.journal.path(),
            &fixture.seal.binding.coordinator_id,
            fixture.seal.binding.principal_scope,
            session(),
            true
        ),
        Err(Error::CoordinatorState)
    ));
    assert_eq!(
        fs::read_dir(empty.path())
            .expect("no adopted anchor")
            .count(),
        0
    );
    let mut limited = session();
    limited.read.value_bytes = 1;
    assert!(
        LocalCoordinator::inspect(fixture.anchor.path(), &fixture.seal.binding, limited).is_err()
    );
    let denied = LocalCoordinator::new(
        fixture.anchor.path().to_owned(),
        fixture.seal.binding.clone(),
        false,
    );
    assert!(matches!(
        denied
            .acquire_writer(fixture.journal.path(), fixture.header(), options())
            .err()
            .expect("explicit anchor permission")
            .cause,
        Error::CoordinatorPermission
    ));
    assert!(matches!(
        LocalCoordinator::enroll(
            Path::new("/nonexistent/anchor"),
            Path::new("/nonexistent/journal"),
            &fixture.seal.binding.coordinator_id,
            fixture.seal.binding.principal_scope,
            session(),
            false
        ),
        Err(Error::CoordinatorPermission)
    ));
    let pristine = private();
    let anchor = private();
    let initial = Sqlite
        .initialize(
            pristine.path(),
            InitializeOptions {
                allow_create: true,
                session: session(),
                ..Default::default()
            },
        )
        .expect("fresh empty schema");
    rusqlite::Connection::open(&initial.database)
        .expect("fixture-only epoch edit")
        .execute(
            "UPDATE gazette_journal SET journal_epoch=?1",
            [journal::INITIAL_EPOCH + 1],
        )
        .expect("reset-looking empty journal");
    assert!(
        matches!(
            LocalCoordinator::enroll(
                anchor.path(),
                pristine.path(),
                &fixture.seal.binding.coordinator_id,
                fixture.seal.binding.principal_scope,
                session(),
                true
            ),
            Err(Error::CoordinatorState)
        ),
        "epoch adoption is not enrollment"
    );
    assert_eq!(
        fs::read_dir(anchor.path())
            .expect("no epoch adoption output")
            .count(),
        0
    );
}

#[test]
fn anchor_rejects_duplicates_unknowns_links_and_trailing_bytes_without_repair() {
    for kind in ["duplicate", "unknown", "trailing", "symlink", "hardlink"] {
        let fixture = Fixture::new();
        let path = fixture.anchor.path().join("coordinator.json");
        let original = fs::read(&path).expect("anchor bytes");
        match kind {
            "duplicate" => {
                let text = String::from_utf8(original).expect("JSON");
                fs::write(
                    &path,
                    text.replacen(
                        "\"state\":\"clean\"",
                        "\"state\":\"clean\",\"state\":\"clean\"",
                        1,
                    ),
                )
                .expect("duplicate fixture");
            }
            "unknown" => {
                let mut value: serde_json::Value = serde_json::from_slice(&original).expect("JSON");
                value["unknown"] = true.into();
                fs::write(&path, serde_json::to_vec(&value).expect("fixture bytes"))
                    .expect("unknown field fixture");
            }
            "trailing" => {
                let mut bytes = original;
                bytes.push(b' ');
                fs::write(&path, bytes).expect("noncanonical fixture");
            }
            "symlink" => {
                let outside = fixture.journal.path().join("saved-anchor");
                fs::rename(&path, &outside).expect("preserve fixture bytes");
                std::os::unix::fs::symlink(outside, &path).expect("symlink fixture");
            }
            "hardlink" => {
                fs::hard_link(&path, fixture.journal.path().join("anchor-alias"))
                    .expect("shared fixture inode");
            }
            _ => unreachable!("closed fixture cases"),
        }
        assert!(
            LocalCoordinator::inspect(fixture.anchor.path(), &fixture.seal.binding, session())
                .is_err(),
            "{kind}"
        );
    }
}

#[test]
fn coordinator_ids_are_exact_strings_bounded_before_retention_and_checked_on_read() {
    let fixture = Fixture::new();
    for value in [
        "",
        " ",
        " owner",
        "owner ",
        "owner\tvalue",
        "owner\0value",
        "\u{a0}owner",
        "owner\u{a0}",
    ] {
        let anchor = private();
        assert!(
            matches!(
                LocalCoordinator::enroll(
                    anchor.path(),
                    fixture.journal.path(),
                    value,
                    fixture.seal.binding.principal_scope,
                    session(),
                    true
                ),
                Err(Error::CoordinatorState)
            ),
            "invalid owner ID {value:?}"
        );
        assert_eq!(
            fs::read_dir(anchor.path())
                .expect("no invalid anchor writes")
                .count(),
            0
        );
    }
    let anchor = private();
    let mut bounded = session();
    bounded.read.value_bytes = 32;
    assert!(
        matches!(
            LocalCoordinator::enroll(
                anchor.path(),
                fixture.journal.path(),
                &"x".repeat(33),
                fixture.seal.binding.principal_scope,
                bounded,
                true
            ),
            Err(Error::StorageLimit)
        ),
        "ID uses the existing anchor value ceiling before cloning"
    );
    assert_eq!(
        fs::read_dir(anchor.path())
            .expect("no oversized anchor writes")
            .count(),
        0
    );
    for value in ["", " bad id", "bad id ", "bad\u{7f}id"] {
        let mut seal = fixture.seal.clone();
        seal.binding.coordinator_id = value.to_owned();
        // Serialize through the actual owner so canonical shape succeeds; ID validity must reject.
        fs::write(
            fixture.anchor.path().join("coordinator.json"),
            serde_json::to_vec(&LocalCoordinatorState::Clean {
                seal,
                adoption: None,
            })
            .expect("canonical typed bad-ID fixture"),
        )
        .expect("persisted invalid ID fixture");
        assert!(
            matches!(
                LocalCoordinator::inspect(fixture.anchor.path(), &fixture.seal.binding, session()),
                Err(Error::CoordinatorState)
            ),
            "stored IDs are validated before expected-binding comparison"
        );
    }
}

// A subprocess-only harness uses the actual test executable and native connector.
// The ready file is written only after acquisition/inspection, never after a guessed delay.
#[test]
fn coordinator_process_child() {
    let Some(control) = std::env::var_os("GAZETTE_COORDINATOR_TEST_CONTROL") else {
        return;
    };
    let control = PathBuf::from(control);
    let request: serde_json::Value = serde_json::from_slice(
        &fs::read(control.join("request.json")).expect("explicit child request"),
    )
    .expect("fixture JSON");
    let seal: LocalCoordinatorSeal = serde_json::from_value(request["seal"].clone())
        .expect("exact independently supplied fixture seal");
    let coordinator = LocalCoordinator::new(
        PathBuf::from(request["anchor"].as_str().expect("explicit anchor path")),
        seal.binding.clone(),
        true,
    );
    let mut writer = coordinator
        .acquire_writer(&seal.binding.journal_root, seal.header, options())
        .expect("real child acquisition");
    let header = writer
        .inspect()
        .expect("actual acquired native header")
        .header;
    // Rename a complete readiness payload so the parent never reads a partial JSON write.
    fs::write(
        control.join("ready.next"),
        serde_json::to_vec(&header).expect("ready header"),
    )
    .expect("fixture ready bytes");
    fs::rename(control.join("ready.next"), control.join("ready.json"))
        .expect("complete acquisition readiness");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !control.join("close").exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "parent must close or terminate this bounded child"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    writer
        .close()
        .expect("real native clean close before process exit");
}

struct Process(std::process::Child);
impl Process {
    fn start(fixture: &Fixture, control: &Path) -> Self {
        fs::write(
            control.join("request.json"),
            serde_json::to_vec(&serde_json::json!({
                "anchor":fixture.anchor.path(),"seal":fixture.seal
            }))
            .expect("exact subprocess request"),
        )
        .expect("fixture control request");
        Self(
            std::process::Command::new(
                std::env::current_exe().expect("this actual native test executable"),
            )
            .args([
                "--exact",
                "coordinator::tests::coordinator_process_child",
                "--nocapture",
            ])
            .env("GAZETTE_COORDINATOR_TEST_CONTROL", control)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .expect("start exact fixture child"),
        )
    }

    fn ready(&mut self, control: &Path) -> JournalHeader {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            assert!(
                self.0
                    .try_wait()
                    .expect("observe actual child lifetime")
                    .is_none(),
                "child exited before readiness"
            );
            if control.join("ready.json").exists() {
                return serde_json::from_slice(
                    &fs::read(control.join("ready.json")).expect("complete readiness bytes"),
                )
                .expect("actual child-acquired header");
            }
            assert!(
                std::time::Instant::now() < deadline,
                "bounded acquisition readiness"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    fn wait(&mut self) -> std::process::ExitStatus {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            if let Some(status) = self.0.try_wait().expect("reap this exact child") {
                return status;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "bounded child termination"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        // Failed assertions must not leave a fixture process holding native locks.
        if matches!(self.0.try_wait(), Ok(None)) {
            let _ = self.0.kill();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            while std::time::Instant::now() < deadline {
                match self.0.try_wait() {
                    Ok(None) => std::thread::sleep(std::time::Duration::from_millis(5)),
                    _ => break,
                }
            }
        }
    }
}

#[test]
fn hard_process_death_releases_kernel_locks_but_retains_dirty_anchor() {
    use std::os::unix::process::ExitStatusExt;
    let fixture = Fixture::new();
    let control = private();
    let mut child = Process::start(&fixture, control.path());
    let acquired = child.ready(control.path());
    assert_eq!(acquired.writer_fence, fixture.seal.header.writer_fence + 1);
    assert!(matches!(
        fixture
            .connector()
            .acquire_writer(fixture.journal.path(), acquired, options())
            .err()
            .expect("child holds real reservation")
            .cause,
        Error::ReservationBusy
    ));
    child
        .0
        .kill()
        .expect("hard terminate only the exact disposable child");
    assert_eq!(child.wait().signal(), Some(9));
    assert_eq!(
        fixture.header(),
        acquired,
        "process death never resets the committed generation"
    );
    assert_eq!(
        fixture.state(),
        LocalCoordinatorState::InProgress {
            previous: fixture.seal.clone(),
            reserved_generation: acquired.writer_fence,
            transaction: Some(continuity::captured(&fixture)),
        },
        "inspection reacquires the kernel-released reservation without cleaning history"
    );
    let error = fixture
        .connector()
        .acquire_writer(fixture.journal.path(), acquired, options())
        .err()
        .expect("dirty state cannot acquire");
    assert!(matches!(error.cause, Error::CoordinatorState));
    assert_eq!(
        error.generation,
        gazette_core::store::GenerationEffect::NotAttempted
    );
}

#[test]
fn process_exit_after_native_clean_close_preserves_exact_reopening() {
    let fixture = Fixture::new();
    let control = private();
    let mut child = Process::start(&fixture, control.path());
    let acquired = child.ready(control.path());
    fs::write(control.path().join("close"), b"explicit clean close")
        .expect("bounded child control");
    assert!(child.wait().success(), "child completed actual close");
    let LocalCoordinatorState::Clean { seal, .. } = fixture.state() else {
        panic!("native close must seal before exit");
    };
    assert_eq!(seal.header, acquired);
    assert_eq!(
        seal.database_sha256,
        Sha256Digest::of(&fs::read(fixture.database()).expect("exact closed bytes"))
    );
    let mut writer = fixture
        .connector()
        .acquire_writer(fixture.journal.path(), acquired, options())
        .expect("explicit new writer from exact clean state");
    assert_eq!(
        writer
            .inspect()
            .expect("native next generation")
            .header
            .writer_fence,
        acquired.writer_fence + 1
    );
    writer.close().expect("next clean close");
}
