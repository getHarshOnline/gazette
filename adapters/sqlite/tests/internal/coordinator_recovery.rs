//! Native restricted custody admission: no dispatch, generation advance or clean reset.

use super::*;
use gazette_ports::store::ReleaseRecoveryConnector;
use gazette_ports::store::ReleaseRecoverySealer;

#[test]
fn sealing_rechecks_same_generation_restoration_after_recovery_admission() {
    let fixture = Fixture::new();
    let mut writer = fixture
        .connector()
        .acquire_writer(fixture.journal.path(), fixture.seal.header, options())
        .expect("held writer");
    append(&mut writer, b"earlier");
    let earlier = fs::read(fixture.database()).expect("earlier snapshot");
    append(&mut writer, b"latest");
    let header = writer.inspect().expect("same generation").header;
    drop(writer);
    let before = fixture.state();
    let recovered = fixture
        .connector()
        .recover_release(
            fixture.journal.path(),
            header,
            gazette_core::store::RecoveryProfile::Unstarted,
            session(),
        )
        .expect("exact latest custody");
    fs::write(fixture.database(), earlier).expect("fixture-only same-inode restoration");
    assert!(matches!(
        recovered.seal_recovery(true),
        Err(Error::CoordinatorMismatch)
    ));
    assert_eq!(fixture.state(), before);
}

#[test]
fn recovery_reuses_exact_generation_and_close_preserves_interrupted_state() {
    let fixture = Fixture::new();
    let mut writer = fixture
        .connector()
        .acquire_writer(fixture.journal.path(), fixture.seal.header, options())
        .expect("actual held generation");
    append(&mut writer, b"retained original observation");
    let expected = writer.inspect().expect("current generation").header;
    assert!(matches!(
        fixture.connector().recover_release(
            fixture.journal.path(),
            expected,
            gazette_core::store::RecoveryProfile::Unstarted,
            session()
        ),
        Err(Error::ReservationBusy)
    ));
    drop(writer);
    let state = fixture.state();
    let bytes = fs::read(fixture.database()).expect("native before recovery");
    let mut recovered = fixture
        .connector()
        .recover_release(
            fixture.journal.path(),
            expected,
            gazette_core::store::RecoveryProfile::Unstarted,
            session(),
        )
        .expect("exact continuity, never a new writer");
    assert_eq!(
        recovered.inspect().expect("held recovered state").header,
        expected
    );
    assert!(matches!(
        fixture.connector().recover_release(
            fixture.journal.path(),
            expected,
            gazette_core::store::RecoveryProfile::Unstarted,
            session()
        ),
        Err(Error::ReservationBusy)
    ));
    recovered.close().expect("release only, not a clean seal");
    assert_eq!(fixture.state(), state);
    assert_eq!(fs::read(fixture.database()).expect("no SQL effect"), bytes);
    assert!(
        fixture
            .connector()
            .acquire_writer(fixture.journal.path(), expected, options())
            .is_err()
    );
}

#[test]
fn null_clean_wrong_binding_permission_and_rollback_never_admit_recovery() {
    let fixture = Fixture::new();
    assert!(matches!(
        fixture.connector().recover_release(
            fixture.journal.path(),
            fixture.seal.header,
            gazette_core::store::RecoveryProfile::Unstarted,
            session()
        ),
        Err(Error::CoordinatorState)
    ));
    let mut writer = fixture
        .connector()
        .acquire_writer(fixture.journal.path(), fixture.seal.header, options())
        .expect("generation");
    append(&mut writer, b"first");
    let older = fs::read(fixture.database()).expect("old same-generation state");
    append(&mut writer, b"second");
    let expected = writer.inspect().expect("expected current state").header;
    drop(writer);
    let state = fixture.state();
    let before = fs::read(fixture.database()).expect("latest state");
    let mut wrong = fixture.seal.binding.clone();
    wrong.coordinator_id.push_str("wrong");
    assert!(matches!(
        LocalCoordinator::new(fixture.anchor.path().to_owned(), wrong, true).recover_release(
            fixture.journal.path(),
            expected,
            gazette_core::store::RecoveryProfile::Unstarted,
            session()
        ),
        Err(Error::CoordinatorMismatch)
    ));
    assert!(
        fixture
            .connector()
            .recover_release(
                fixture.journal.path(),
                expected,
                gazette_core::store::RecoveryProfile::Unstarted,
                SessionOptions::default()
            )
            .is_err()
    );
    assert_eq!(fixture.state(), state);
    assert_eq!(
        fs::read(fixture.database()).expect("no refused effect"),
        before
    );
    fs::write(fixture.database(), &older).expect("explicit fixture-only same-inode rollback");
    assert!(matches!(
        fixture.connector().recover_release(
            fixture.journal.path(),
            expected,
            gazette_core::store::RecoveryProfile::Unstarted,
            session()
        ),
        Err(Error::CoordinatorMismatch)
    ));
    assert_eq!(fixture.state(), state);
    assert_eq!(
        fs::read(fixture.database()).expect("rollback not repaired"),
        older
    );

    let fixture = Fixture::new();
    let anchor = Anchor::acquire(fixture.anchor.path(), session().read).expect("actual lock");
    let clean = anchor.read().expect("original anchor");
    anchor
        .write(
            LocalCoordinatorState::InProgress {
                previous: fixture.seal.clone(),
                reserved_generation: fixture.seal.header.writer_fence + 1,
                transaction: None,
            },
            Some(&clean),
        )
        .expect("actual pre-generation barrier");
    drop(anchor);
    let state = fixture.state();
    assert!(matches!(
        fixture.connector().recover_release(
            fixture.journal.path(),
            fixture.seal.header,
            gazette_core::store::RecoveryProfile::Unstarted,
            session()
        ),
        Err(Error::CoordinatorState)
    ));
    assert_eq!(fixture.state(), state);
}

#[test]
fn read_only_recovery_does_not_require_anchor_write_or_grant_it() {
    let fixture = Fixture::new();
    let mut writer = fixture
        .connector()
        .acquire_writer(fixture.journal.path(), fixture.seal.header, options())
        .expect("original generation");
    let expected = writer.inspect().expect("actual header").header;
    drop(writer);
    let state = fixture.state();
    let connector = LocalCoordinator::new(
        fixture.anchor.path().to_owned(),
        fixture.seal.binding.clone(),
        false,
    );
    let mut recovered = connector
        .recover_release(
            fixture.journal.path(),
            expected,
            gazette_core::store::RecoveryProfile::Unstarted,
            session(),
        )
        .expect("read and reservation permissions suffice for observation");
    assert_eq!(
        recovered.inspect().expect("held read-only recovery").header,
        expected
    );
    recovered.close().expect("no anchor write on close");
    assert_eq!(fixture.state(), state);
}
