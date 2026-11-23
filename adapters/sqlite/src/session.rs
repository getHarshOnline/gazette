//! Local advisory reservation; journal epochs and cross-host fencing are separate.

use crate::{Sqlite, connection::ReadStore, coordination::Reservation, read_metadata};
use gazette_core::store::{Error, Observation, SessionOptions};
use gazette_ports::store::{StoreConnector, StoreSession};
use std::path::Path;

/// Reserved native read-only connection. Field order closes `SQLite` before unlocking on drop.
pub struct Session {
    store: ReadStore,
    reservation: Reservation,
    failed: bool,
}

impl StoreConnector for Sqlite {
    type Session = Session;

    fn connect(
        &self,
        path: &Path,
        coordination_root: &Path,
        options: SessionOptions,
    ) -> Result<Session, Error> {
        options.validate()?;
        let reservation = Reservation::acquire(coordination_root)?;
        let store = ReadStore::open(path, options.read)?;
        reservation.verify()?;
        Ok(Session {
            store,
            reservation,
            failed: false,
        })
    }
}

impl StoreSession for Session {
    fn observe(&mut self) -> Result<Observation, Error> {
        if self.failed {
            return Err(Error::Changed);
        }
        // Any failed observation invalidates this session; restoring path spelling
        // or retrying cannot silently heal stale lifetime evidence.
        self.failed = true;
        self.reservation.verify()?;
        let result = self.store.read(read_metadata)?;
        self.reservation.verify()?;
        self.failed = false;
        Ok(result)
    }

    fn close(self) -> Result<(), Error> {
        let Session {
            store,
            reservation,
            failed,
        } = self;
        let verified = if failed {
            Err(Error::Changed)
        } else {
            reservation.verify()
        };
        let closed = store.close();
        let after = reservation.verify();
        drop(reservation);
        verified.and(closed).and(after)
    }
}
