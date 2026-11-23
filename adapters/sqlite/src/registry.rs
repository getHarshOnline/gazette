//! Atomic complete-set registry rows, separate from journal coordinates and future history.

use gazette_core::{
    source::Sha256Digest,
    store::{Error, ReadOptions, RegistryHead, RegistryInput, RegistryInspection},
};
use gazette_ports::store::RegistryReader;
use rusqlite::{Connection, params};

use crate::{Sqlite, journal, native_error, with_source};

mod catalog;
mod content;
mod rows;
use rows::Rows;

pub(super) struct Desired {
    rows: Rows,
    pub head: RegistryHead,
}

impl Desired {
    pub(super) fn prepare(input: RegistryInput<'_>, options: ReadOptions) -> Result<Self, Error> {
        let maximum = usize::try_from(options.storage_bytes).map_err(|_| Error::StorageLimit)?;
        let digest = input
            .model
            .semantic_sha256(maximum)
            .map_err(|_| Error::StorageLimit)?;
        if digest != input.semantic_sha256 || input.model.sites.is_empty() {
            return Err(Error::RegistryVerification);
        }
        let mut rows = Rows::new(options)?;
        catalog::append(input.model, &mut rows)?;
        content::append(&input, &mut rows)?;
        let mut qualification = crate::schema::validation_connection(options)?;
        let transaction = qualification.transaction().map_err(native_error)?;
        rows.replace(&transaction)?;
        journal::foreign_keys(&transaction)?;
        transaction.rollback().map_err(native_error)?;
        qualification
            .close()
            .map_err(|(_, error)| native_error(error))?;
        let (rows_sha256, count) = rows.identity(maximum)?;
        Ok(Self {
            rows,
            head: RegistryHead {
                semantic_sha256: input.semantic_sha256,
                configuration_sha256: input.configuration_sha256,
                rows_sha256,
                rows: count,
            },
        })
    }

    pub(super) fn replace(
        &self,
        connection: &Connection,
        options: ReadOptions,
    ) -> Result<(), Error> {
        self.rows.replace(connection)?;
        journal::foreign_keys(connection)?;
        let actual = Rows::read(connection, options)?;
        if !actual.same_values(&self.rows) {
            return Err(Error::RegistryVerification);
        }
        connection.execute(
            "INSERT INTO gazette_registry_head(singleton,semantic_sha256,configuration_sha256,rows_sha256,rows) VALUES(1,?1,?2,?3,?4) ON CONFLICT(singleton) DO UPDATE SET semantic_sha256=excluded.semantic_sha256, configuration_sha256=excluded.configuration_sha256, rows_sha256=excluded.rows_sha256, rows=excluded.rows",
            params![self.head.semantic_sha256.as_bytes(), self.head.configuration_sha256.as_bytes(), self.head.rows_sha256.as_bytes(), i64::try_from(self.head.rows).map_err(|_| Error::StorageLimit)?],
        ).map_err(native_error)?;
        if verify(connection, options)? != Some(self.head) {
            return Err(Error::RegistryVerification);
        }
        Ok(())
    }
}

impl RegistryReader for Sqlite {
    fn inspect_registry(
        &self,
        path: &std::path::Path,
        options: ReadOptions,
    ) -> Result<RegistryInspection, Error> {
        with_source(path, options, |connection| inspect(connection, options))
    }
}

pub(super) fn inspect(
    connection: &Connection,
    options: ReadOptions,
) -> Result<RegistryInspection, Error> {
    let journal = journal::inspect(connection)?;
    let registry = verify(connection, options)?;
    Ok(RegistryInspection { journal, registry })
}

pub(super) fn verify(
    connection: &Connection,
    options: ReadOptions,
) -> Result<Option<RegistryHead>, Error> {
    let actual = Rows::read(connection, options)?;
    let (digest, count) = actual
        .identity(usize::try_from(options.storage_bytes).map_err(|_| Error::StorageLimit)?)?;
    let mut statement = connection.prepare("SELECT singleton,semantic_sha256,configuration_sha256,rows_sha256,rows FROM gazette_registry_head LIMIT 2").map_err(native_error)?;
    let mut rows = statement.query([]).map_err(native_error)?;
    let Some(row) = rows.next().map_err(native_error)? else {
        return if count == 0 {
            Ok(None)
        } else {
            Err(Error::RegistryVerification)
        };
    };
    let singleton: i64 = row.get(0).map_err(|_| Error::RegistryVerification)?;
    let read_digest = |column| {
        row.get::<_, [u8; 32]>(column)
            .map(Sha256Digest::from)
            .map_err(|_| Error::RegistryVerification)
    };
    let stored_count: i64 = row.get(4).map_err(|_| Error::RegistryVerification)?;
    let head = RegistryHead {
        semantic_sha256: read_digest(1)?,
        configuration_sha256: read_digest(2)?,
        rows_sha256: read_digest(3)?,
        rows: u64::try_from(stored_count).map_err(|_| Error::RegistryVerification)?,
    };
    if singleton != 1
        || rows.next().map_err(native_error)?.is_some()
        || head.rows != count
        || head.rows_sha256 != digest
    {
        return Err(Error::RegistryVerification);
    }
    Ok(Some(head))
}

#[cfg(test)]
#[path = "../tests/internal/registry.rs"]
mod tests;
