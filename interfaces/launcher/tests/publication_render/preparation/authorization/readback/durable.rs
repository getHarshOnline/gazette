//! Real prepared sites → native Git reads → `SQLite` history → independent inspection.

#[path = "durable/managed.rs"]
mod managed;

use super::super::{Sha256Digest, read, store};
use super::{BTreeMap, Local, PathBuf, fs, observed, permitted, release};

const REF_OBSERVATION_ROW_CEILING: i32 = 64 * 1024;
const OVERSIZED_OBSERVATION_TARGETS: u32 = 64;

fn oversized_observation_targets(
    original: &release::TargetSet,
    namespace: &str,
) -> Vec<release::Target> {
    let padding = "x".repeat(1024);
    let targets: Vec<_> = (0..OVERSIZED_OBSERVATION_TARGETS)
        .map(|ordinal| {
            let mut target = original.targets()[0].clone();
            target.ordinal = ordinal;
            target.domain = format!("{ordinal}.{namespace}.{padding}.example");
            target.branch = format!("observation-{namespace}-{ordinal}");
            target.refspec = format!("{}:refs/heads/{}", target.local_commit_oid, target.branch);
            target
        })
        .collect();
    assert!(
        serde_json::to_vec(&targets)
            .expect("bounded observation fixture")
            .len()
            > REF_OBSERVATION_ROW_CEILING as usize,
        "fixture must exceed the selected row ceiling independently of existing journal rows"
    );
    targets
}

fn options() -> release::RecordObservationOptions {
    release::RecordObservationOptions {
        read: permitted(),
        record: release::RefObservationWriteOptions { allow_record: true },
    }
}

fn writer(local: &Local, header: store::JournalHeader) -> store::Writer {
    store::acquire_writer(
        local.prepared._journal_root.path(),
        header,
        store::WriterOptions {
            allow_generation_advance: true,
            session: store::SessionOptions {
                allow_local_reservation: true,
                read: read(),
            },
        },
    )
    .expect("held real journal generation")
}

fn unchanged_inputs(local: &Local) -> BTreeMap<PathBuf, Sha256Digest> {
    let journal = local
        .prepared
        ._journal_root
        .path()
        .canonicalize()
        .expect("exact native journal root");
    local
        .snapshot()
        .into_iter()
        .filter(|(path, _)| {
            !path
                .canonicalize()
                .expect("existing native snapshot entry")
                .starts_with(&journal)
        })
        .map(|(path, bytes)| (path, Sha256Digest::of(&bytes)))
        .collect()
}

#[test]
fn complete_native_observations_survive_remote_movement_and_reopen() {
    let local = Local::new();
    local.install_intended(0);
    let targets = local.targets();
    let before = unchanged_inputs(&local);
    let id = Sha256Digest::of(b"first complete native observation");
    let mut held = writer(&local, local.prepared.header);
    let first = held
        .record_target_observation(&local.git(), &targets, id, options())
        .expect("real native reads and independently verified durable append");
    assert!(first.inserted);
    assert_eq!(first.record.targets, targets.targets());
    assert_eq!(first.record.readbacks.len(), 2);
    observed(
        &first.record.readbacks[0],
        0,
        local.prepared.selections[0].commit.as_str(),
        release::RefComparison::Intended,
    );
    assert!(matches!(
        first.record.readbacks[1].readback,
        release::TargetReadback::Observed {
            advertisement: release::RefAdvertisement::Absent,
            comparison: release::RefComparison::Prestate,
        }
    ));
    assert_eq!(first.record.created, first.journal);
    held.close().expect("verified lifetime close");
    assert_eq!(
        unchanged_inputs(&local),
        before,
        "only private journal history changes"
    );
    let stored = release::inspect_target_observation(
        &local.prepared.database,
        id,
        Some(&first.journal),
        read(),
        Default::default(),
    )
    .expect("independent historical inspection")
    .record
    .expect("complete stored observation");
    assert_eq!(stored, first.record);

    local.install_intended(1);
    let before = unchanged_inputs(&local);
    let mut held = writer(&local, first.journal);
    let second = held
        .record_target_observation(
            &local.git(),
            &targets,
            Sha256Digest::of(b"second native observation"),
            options(),
        )
        .expect("new observation, new immutable coordinate");
    held.close().expect("second lifetime close");
    observed(
        &second.record.readbacks[1],
        1,
        local.prepared.selections[1].commit.as_str(),
        release::RefComparison::Intended,
    );
    assert_ne!(first.record.readbacks, second.record.readbacks);
    assert_eq!(
        first.record.targets, second.record.targets,
        "mutable observation never changes intent"
    );
    assert_eq!(unchanged_inputs(&local), before);
    let original = release::inspect_target_observation(
        &local.prepared.database,
        id,
        Some(&second.journal),
        read(),
        Default::default(),
    )
    .expect("reopened older history")
    .record
    .expect("original retained evidence");
    assert_eq!(
        original, stored,
        "later observations cannot replace or relabel history"
    );
    assert!(matches!(
        release::inspect_target_observation(
            &local.prepared.database,
            id,
            Some(&first.journal),
            read(),
            Default::default()
        ),
        Err(store::Error::JournalMismatch)
    ));
}

#[test]
fn unavailable_destinations_are_persisted_without_shrinking_the_set() {
    let local = Local::new();
    local.install_intended(1);
    let targets = local.targets();
    let mut selected = targets.targets().to_vec();
    selected[0].remote = local
        .remotes
        .path()
        .join("not-present.git")
        .to_str()
        .expect("exact native path")
        .into();
    let selected = release::check_targets(&selected, Default::default())
        .expect("explicit unavailable destination remains selected");
    let before = unchanged_inputs(&local);
    let mut held = writer(&local, local.prepared.header);
    let receipt = held
        .record_target_observation(
            &local.git(),
            &selected,
            Sha256Digest::of(b"mixed native evidence"),
            options(),
        )
        .expect("complete mixed native result is durable, not release success");
    held.close().expect("mixed history close");
    assert_eq!(receipt.record.targets, selected.targets());
    assert_eq!(receipt.record.readbacks.len(), 2);
    assert!(
        matches!(receipt.record.readbacks[0].readback, release::TargetReadback::Unavailable { error } if error.failure == release::RemoteReadFailure::Transport)
    );
    observed(
        &receipt.record.readbacks[1],
        1,
        local.prepared.selections[1].commit.as_str(),
        release::RefComparison::Intended,
    );
    let stored = release::inspect_target_observation(
        &local.prepared.database,
        receipt.record.id,
        Some(&receipt.journal),
        read(),
        Default::default(),
    )
    .expect("native mixed result reopens")
    .record
    .expect("both original rows");
    assert_eq!(stored, receipt.record);
    assert_eq!(unchanged_inputs(&local), before);
}

#[test]
fn independent_permissions_and_existing_ids_refuse_before_provider_invocation() {
    use std::os::unix::fs::PermissionsExt;
    let local = Local::new();
    let targets = local.targets();
    let mut held = writer(&local, local.prepared.header);
    let id = Sha256Digest::of(b"retained observation coordinate");
    held.record_target_observation(&local.git(), &targets, id, options())
        .expect("original complete observation");
    let marker = local.remotes.path().join("unexpected-provider-invocation");
    let program = local.remotes.path().join("detect-invocation");
    fs::write(
        &program,
        format!("#!/bin/sh\nprintf x > '{}'\nexit 91\n", marker.display()),
    )
    .expect("disposable provider invocation probe");
    fs::set_permissions(&program, fs::Permissions::from_mode(0o700)).expect("fixture executable");
    let before = local.snapshot();
    let mut no_record = options();
    no_record.record.allow_record = false;
    assert!(matches!(
        held.record_target_observation(
            &local.git_with(program.clone()),
            &targets,
            Sha256Digest::of(b"denied record"),
            no_record
        ),
        Err(release::RecordObservationError::BeforeRead(
            store::Error::ReadbackPermission
        ))
    ));
    let mut no_read = options();
    no_read.read.allow_read = false;
    assert!(
        matches!(held.record_target_observation(&local.git_with(program.clone()), &targets, Sha256Digest::of(b"denied read"), no_read),
        Err(release::RecordObservationError::Read(error)) if error.failure == release::RemoteReadFailure::Permission)
    );
    assert!(matches!(
        held.record_target_observation(&local.git_with(program.clone()), &targets, id, options()),
        Err(release::RecordObservationError::BeforeRead(
            store::Error::ReadbackConflict
        ))
    ));
    assert!(
        !marker.exists(),
        "no provider process started on any refusal"
    );
    assert_eq!(
        local.snapshot(),
        before,
        "refusals do not write history or native inputs"
    );
    held.close()
        .expect("valid held writer survives pre-read refusals");
}

#[test]
fn failed_bounded_append_preserves_the_complete_native_read_evidence() {
    let local = Local::new();
    let original = local.targets();
    let targets = oversized_observation_targets(&original, "sdk");
    let targets =
        release::check_targets(&targets, Default::default()).expect("complete bounded selection");
    let id = Sha256Digest::of(b"read complete but cannot fit history row");
    let mut store_read = read();
    store_read.value_bytes = REF_OBSERVATION_ROW_CEILING;
    let mut held = store::acquire_writer(
        local.prepared._journal_root.path(),
        local.prepared.header,
        store::WriterOptions {
            allow_generation_advance: true,
            session: store::SessionOptions {
                allow_local_reservation: true,
                read: store_read,
            },
        },
    )
    .expect("native writer with a deliberately small row ceiling");
    let header = held.inspect().expect("actual acquired generation").header;
    let before = local.snapshot();
    let error = held
        .record_target_observation(&local.git(), &targets, id, options())
        .expect_err("complete native rows exceed explicit persistence bound");
    let release::RecordObservationError::Write { report, cause } = error else {
        panic!("must preserve completed native observations, not mislabel failure before reads");
    };
    assert!(matches!(cause.cause, store::Error::StorageLimit));
    assert_eq!(report.target_set_sha256, targets.identity());
    assert_eq!(report.targets.len(), targets.targets().len());
    for (ordinal, row) in report.targets.iter().enumerate() {
        assert_eq!(row.ordinal as usize, ordinal);
        assert!(matches!(
            row.readback,
            release::TargetReadback::Observed {
                advertisement: release::RefAdvertisement::Absent,
                comparison: release::RefComparison::Prestate,
            }
        ));
    }
    held.close()
        .expect("pre-insert bound refusal retains valid writer");
    assert_eq!(
        local.snapshot(),
        before,
        "reads and refused insert changed no native bytes"
    );
    assert!(
        release::inspect_target_observation(
            &local.prepared.database,
            id,
            Some(&header),
            read(),
            Default::default()
        )
        .expect("independent exact coordinate inspection")
        .record
        .is_none()
    );
}

#[test]
fn generation_changed_during_native_reads_preserves_rows_but_refuses_the_append() {
    use std::{os::unix::fs::PermissionsExt, sync::mpsc, thread, time::Duration};
    let local = Local::new();
    local.install_intended(0);
    let targets = local.targets();
    let mut held = writer(&local, local.prepared.header);
    let id = Sha256Digest::of(b"writer becomes stale while native reader runs");
    let marker = local.remotes.path().join("reader-started");
    let gate = local.remotes.path().join("generation-changed");
    let program = local
        .remotes
        .path()
        .join("read-after-native-generation-change");
    fs::write(
        &program,
        format!(
            "#!/bin/sh\n: > '{}'\nwhile [ ! -f '{}' ]; do /bin/sleep 0.01; done\nexec {} \"$@\"\n",
            marker.display(),
            gate.display(),
            crate::native::shell(crate::native::git())
        ),
    )
    .expect("controlled native reader barrier");
    fs::set_permissions(&program, fs::Permissions::from_mode(0o700))
        .expect("fixture reader executable");
    let database = local.prepared.database.clone();
    let before = unchanged_inputs(&local);
    let (operation, completed) = mpsc::channel::<()>();
    let competing = thread::spawn(move || {
        // The real provider owns its deadline. A separate shorter timer can abandon
        // the gate before a delayed child starts and manufacture provider timeouts.
        // Dropping the sender also stops this worker if the caller unwinds.
        loop {
            if !matches!(completed.try_recv(), Err(mpsc::TryRecvError::Empty)) {
                return false;
            }
            if marker.exists() {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        let connection =
            rusqlite::Connection::open(database).expect("fixture-only noncooperating writer");
        assert_eq!(
            connection
                .execute("UPDATE gazette_journal SET writer_fence=writer_fence+1", [])
                .expect("actual generation change during provider read"),
            1
        );
        connection
            .close()
            .expect("competing transaction closed before read continues");
        fs::write(gate, b"changed").expect("release native read barrier");
        true
    });
    let result = held.record_target_observation(&local.git_with(program), &targets, id, options());
    drop(operation);
    assert!(
        competing
            .join()
            .expect("controlled competing native mutation completed"),
        "real provider must reach the acquisition barrier before its operation ends"
    );
    let error = result.expect_err("no append under the now-stale lifetime");
    let release::RecordObservationError::Write { report, cause } = error else {
        panic!("native rows must remain available even when the held writer becomes stale");
    };
    assert!(matches!(cause.cause, store::Error::JournalMismatch));
    assert_eq!(report.targets.len(), targets.targets().len());
    assert_eq!(report.target_set_sha256, targets.identity());
    observed(
        &report.targets[0],
        0,
        local.prepared.selections[0].commit.as_str(),
        release::RefComparison::Intended,
    );
    assert!(matches!(
        report.targets[1].readback,
        release::TargetReadback::Observed {
            advertisement: release::RefAdvertisement::Absent,
            comparison: release::RefComparison::Prestate,
        }
    ));
    assert!(
        held.close().is_err(),
        "stale writer lifetime stays invalidated"
    );
    assert_eq!(unchanged_inputs(&local), before);
    assert!(
        release::inspect_target_observation(
            &local.prepared.database,
            id,
            None,
            read(),
            Default::default()
        )
        .expect("independent recovery inspection")
        .record
        .is_none(),
        "no stale insert"
    );
}
