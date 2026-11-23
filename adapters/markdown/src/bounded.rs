//! Logical string ceilings shared by retained facts and upstream formatter output.

use gazette_core::markdown::Error;
use std::fmt;

pub(crate) struct Budget(pub(crate) usize);

impl Budget {
    pub(crate) fn charge(&mut self, bytes: usize) -> Result<(), Error> {
        self.0 = self
            .0
            .checked_sub(bytes)
            .ok_or(Error::Limit("fact string bytes"))?;
        Ok(())
    }

    pub(crate) fn copy(&mut self, value: &str) -> Result<String, Error> {
        self.charge(value.len())?;
        Ok(value.to_owned())
    }
}

pub(crate) struct Output {
    pub(crate) value: String,
    limit: usize,
    exhausted: bool,
}

impl Output {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            value: String::new(),
            limit,
            exhausted: false,
        }
    }

    pub(crate) fn error(&self) -> Error {
        if self.exhausted {
            Error::Limit("rendered output bytes")
        } else {
            Error::Formatting
        }
    }
}

impl fmt::Write for Output {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        if value.len() > self.limit.saturating_sub(self.value.len()) {
            self.exhausted = true;
            return Err(fmt::Error);
        }
        self.value.push_str(value);
        Ok(())
    }
}
