//! Native directory-lock lifetime, including an incidental duplicated descriptor.

use super::Reservation;
use std::{fs, os::unix::fs::PermissionsExt};

#[test]
fn incidental_descriptor_cannot_extend_the_owners_reservation_lifetime() {
    let root = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .expect("private disposable namespace");
    let reservation = Reservation::acquire(root.path()).expect("owner");
    // Models an unrelated fork's brief pre-exec inheritance. CLOEXEC only acts
    // at exec; it does not remove the duplicated open-file description at fork.
    let incidental = reservation
        .file
        .try_clone()
        .expect("incidental native duplicate");
    assert!(Reservation::acquire(root.path()).is_err());
    drop(reservation);
    let next = Reservation::acquire(root.path())
        .expect("owner lifetime ended despite incidental descriptor");
    drop(incidental);
    assert!(
        Reservation::acquire(root.path()).is_err(),
        "closing an old duplicate must not unlock the new owner"
    );
    drop(next);
    Reservation::acquire(root.path()).expect("new owner also released");
}
