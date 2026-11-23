//! Streaming identity of one exact native SQL snapshot, independent of page layout.

use gazette_core::store::{Error, LocalCoordinatorTransaction, ReadOptions};
use rusqlite::{Connection, types::ValueRef};
use sha2::{Digest, Sha256};

use crate::{journal, native_error, schema};

pub(crate) fn capture(
    connection: &Connection,
    options: ReadOptions,
) -> Result<LocalCoordinatorTransaction, Error> {
    let inspection = journal::inspect(connection)?;
    let mut digest = BoundedDigest {
        hash: Sha256::new(),
        remaining: options.storage_bytes,
    };
    digest.add(b"GAZETTE_COORDINATOR_SQL_STATE_1\0")?;
    digest.add(inspection.schema_sha256.as_bytes())?;
    // This inventory is derived from the same trusted DDL as native schema verification.
    // It includes journal history, not just rebuildable registry rows.
    for table in schema::tables()? {
        digest.add(b"T")?;
        digest.bytes(table.name.as_bytes())?;
        digest.add(&(table.columns as u64).to_be_bytes())?;
        let mut statement = connection.prepare(&table.scan).map_err(native_error)?;
        if statement.column_count() != table.columns {
            return Err(Error::JournalSchema);
        }
        let mut rows = statement.query([]).map_err(native_error)?;
        while let Some(row) = rows.next().map_err(native_error)? {
            digest.add(b"R")?;
            for index in 0..table.columns {
                match row.get_ref(index).map_err(native_error)? {
                    ValueRef::Null => digest.add(b"N")?,
                    ValueRef::Integer(value) => {
                        digest.add(b"I")?;
                        digest.add(&value.to_be_bytes())?;
                    }
                    ValueRef::Text(value) => {
                        digest.add(b"S")?;
                        digest.bytes(value)?;
                    }
                    ValueRef::Blob(value) => {
                        digest.add(b"B")?;
                        digest.bytes(value)?;
                    }
                    // Gazette's admitted storage schema has no floating-point scalar family.
                    ValueRef::Real(_) => return Err(Error::JournalVerification),
                }
            }
        }
        digest.add(b"E")?;
    }
    Ok(LocalCoordinatorTransaction {
        header: inspection.header,
        content_sha256: <[u8; 32]>::from(digest.hash.finalize()).into(),
    })
}

struct BoundedDigest {
    hash: Sha256,
    remaining: u64,
}

impl BoundedDigest {
    fn add(&mut self, bytes: &[u8]) -> Result<(), Error> {
        self.remaining = self
            .remaining
            .checked_sub(bytes.len() as u64)
            .ok_or(Error::StorageLimit)?;
        self.hash.update(bytes);
        Ok(())
    }

    fn bytes(&mut self, bytes: &[u8]) -> Result<(), Error> {
        self.add(&(bytes.len() as u64).to_be_bytes())?;
        self.add(bytes)
    }
}
