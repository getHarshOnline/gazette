//! One deterministic scalar-row representation for desired writes and native read-back.

use std::collections::{BTreeMap, BTreeSet};

use gazette_core::{
    render::LimitedWriter,
    source::Sha256Digest,
    store::{Error, ReadOptions},
};
use rusqlite::{
    Connection,
    types::{ToSql, ToSqlOutput, ValueRef},
};
use serde::Serialize;

use crate::{native_error, schema};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub(super) enum Cell {
    Null,
    Integer(i64),
    Text(String),
    Blob(Vec<u8>),
}

impl Cell {
    pub(super) fn text(value: impl std::fmt::Display) -> Self {
        Self::Text(value.to_string())
    }
    pub(super) fn optional(value: Option<impl std::fmt::Display>) -> Self {
        value.map_or(Self::Null, Self::text)
    }
    pub(super) fn flag(value: bool) -> Self {
        Self::Integer(i64::from(value))
    }
    pub(super) fn ordinal(value: usize) -> Result<Self, Error> {
        Ok(Self::Integer(
            i64::try_from(value).map_err(|_| Error::StorageLimit)?,
        ))
    }
    pub(super) fn digest(value: Sha256Digest) -> Self {
        Self::Blob(value.as_bytes().to_vec())
    }
    pub(super) fn enumeration(value: &impl Serialize) -> Result<Self, Error> {
        match serde_json::to_value(value).map_err(|_| Error::RegistryVerification)? {
            serde_json::Value::String(value) => Ok(Self::Text(value)),
            _ => Err(Error::RegistryVerification),
        }
    }
}

impl ToSql for Cell {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::Borrowed(match self {
            Self::Null => ValueRef::Null,
            Self::Integer(value) => ValueRef::Integer(*value),
            Self::Text(value) => ValueRef::Text(value.as_bytes()),
            Self::Blob(value) => ValueRef::Blob(value),
        }))
    }
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(super) struct Rows {
    tables: BTreeMap<&'static str, BTreeSet<Vec<Cell>>>,
    #[serde(skip)]
    remaining: usize,
    #[serde(skip)]
    value_bytes: usize,
}

impl Rows {
    pub(super) fn new(options: ReadOptions) -> Result<Self, Error> {
        Ok(Self {
            tables: schema::registry_tables()?
                .iter()
                .map(|table| (table.name.as_str(), BTreeSet::new()))
                .collect(),
            remaining: usize::try_from(options.storage_bytes).map_err(|_| Error::StorageLimit)?,
            value_bytes: usize::try_from(options.value_bytes).map_err(|_| Error::InvalidLimits)?,
        })
    }

    pub(super) fn add(&mut self, table: &str, row: Vec<Cell>) -> Result<(), Error> {
        let mut row_bytes = std::mem::size_of::<Vec<Cell>>();
        for cell in &row {
            let bytes = match cell {
                Cell::Text(value) => value.len(),
                Cell::Blob(value) => value.len(),
                Cell::Integer(_) | Cell::Null => 8,
            };
            if bytes > self.value_bytes {
                return Err(Error::StorageLimit);
            }
            row_bytes = row_bytes
                .checked_add(bytes)
                .and_then(|total| total.checked_add(std::mem::size_of::<Cell>()))
                .ok_or(Error::StorageLimit)?;
        }
        self.remaining = self
            .remaining
            .checked_sub(row_bytes)
            .ok_or(Error::StorageLimit)?;
        if !self
            .tables
            .get_mut(table)
            .ok_or(Error::RegistryVerification)?
            .insert(row)
        {
            return Err(Error::RegistryVerification);
        }
        Ok(())
    }

    pub(super) fn json(&self, value: &impl Serialize) -> Result<Cell, Error> {
        let mut output = LimitedWriter::new(Vec::new(), self.value_bytes);
        let encoded = serde_json::to_writer(&mut output, value);
        if output.exceeded() {
            return Err(Error::StorageLimit);
        }
        encoded.map_err(|_| Error::RegistryVerification)?;
        String::from_utf8(output.into_inner())
            .map(Cell::Text)
            .map_err(|_| Error::RegistryVerification)
    }

    pub(super) fn same_values(&self, other: &Self) -> bool {
        self.tables == other.tables
    }

    pub(super) fn identity(&self, maximum: usize) -> Result<(Sha256Digest, u64), Error> {
        let mut writer = LimitedWriter::new(Vec::new(), maximum);
        serde_json::to_writer(&mut writer, self).map_err(|_| Error::StorageLimit)?;
        let count = self.tables.values().try_fold(0u64, |total, rows| {
            total
                .checked_add(rows.len() as u64)
                .ok_or(Error::StorageLimit)
        })?;
        Ok((Sha256Digest::of(&writer.into_inner()), count))
    }

    pub(super) fn read(connection: &Connection, options: ReadOptions) -> Result<Self, Error> {
        let mut result = Self::new(options)?;
        for table in schema::registry_tables()? {
            let mut statement = connection
                .prepare(&format!("SELECT * FROM \"{}\"", table.name))
                .map_err(native_error)?;
            let mut rows = statement.query([]).map_err(native_error)?;
            while let Some(row) = rows.next().map_err(native_error)? {
                let mut values = Vec::with_capacity(table.columns);
                for index in 0..table.columns {
                    values.push(match row.get_ref(index).map_err(native_error)? {
                        ValueRef::Null => Cell::Null,
                        ValueRef::Integer(value) => Cell::Integer(value),
                        ValueRef::Text(value) => Cell::Text(
                            std::str::from_utf8(value)
                                .map_err(|_| Error::RegistryVerification)?
                                .to_owned(),
                        ),
                        ValueRef::Blob(value) => Cell::Blob(value.to_vec()),
                        ValueRef::Real(_) => return Err(Error::RegistryVerification),
                    });
                }
                result.add(&table.name, values)?;
            }
        }
        Ok(result)
    }

    // Caller owns one IMMEDIATE transaction; all registry FKs are deferred until
    // the complete replacement exists. Journal/history tables are never selected.
    pub(super) fn replace(&self, connection: &Connection) -> Result<(), Error> {
        for table in schema::registry_tables()? {
            connection
                .execute(&format!("DELETE FROM \"{}\"", table.name), [])
                .map_err(native_error)?;
        }
        for table in schema::registry_tables()? {
            let parameters = std::iter::repeat_n("?", table.columns)
                .collect::<Vec<_>>()
                .join(",");
            let mut statement = connection
                .prepare(&format!(
                    "INSERT INTO \"{}\" VALUES ({parameters})",
                    table.name
                ))
                .map_err(native_error)?;
            for row in &self.tables[table.name.as_str()] {
                if row.len() != table.columns {
                    return Err(Error::RegistryVerification);
                }
                statement
                    .execute(rusqlite::params_from_iter(row))
                    .map_err(native_error)?;
            }
        }
        Ok(())
    }
}
