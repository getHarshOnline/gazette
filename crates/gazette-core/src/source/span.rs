//! Language-neutral UTF-8 byte coordinates over explicitly supplied source.

use std::ops::Range;

use serde::{Deserialize, Serialize};

/// Located source evidence, independent of a particular parser or editor protocol.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Location {
    /// Checked half-open UTF-8 coordinates within the separately bound source.
    pub span: Span,
    /// Numerical validity alone does not establish an exact token location.
    pub precision: Precision,
}

/// A range's evidentiary precision, independent of its numerical validity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub enum Precision {
    /// The containing construct is located; a token edit needs additional evidence.
    Construct,
    /// The exact original lexical bytes have been separately verified.
    Exact,
}

/// Half-open byte coordinates, not character positions or proof of source identity.
///
/// A range must be checked against its exact source before slicing or projecting
/// editor positions. Coordinate validity alone does not establish token precision,
/// an authored origin or permission to edit the source.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Span {
    /// Inclusive UTF-8 byte offset.
    #[schemars(range(max = usize::MAX))]
    pub start: usize,
    /// Exclusive UTF-8 byte offset.
    #[schemars(range(max = usize::MAX))]
    pub end: usize,
}

impl Span {
    /// Validate ordering, bounds and character boundaries against exact text.
    pub fn checked(source: &str, range: Range<usize>) -> Option<Self> {
        source.get(range.clone())?;
        Some(Self {
            start: range.start,
            end: range.end,
        })
    }

    /// Borrow the exact source slice, or refuse invalid byte coordinates.
    pub fn slice(self, source: &str) -> Option<&str> {
        source.get(self.start..self.end)
    }

    /// Locate this child range within a verbatim parent slice of the given text.
    ///
    /// Both coordinate systems are checked. This operation is appropriate for
    /// document framing, not decoded notebook strings, normalized syntax or
    /// generated fragments: those need an explicit origin relation instead.
    pub fn in_parent(self, source: &str, parent: Self) -> Option<Self> {
        self.slice(parent.slice(source)?)?;
        Self::checked(
            source,
            parent.start.checked_add(self.start)?..parent.start.checked_add(self.end)?,
        )
    }
}

impl From<&Range<usize>> for Span {
    /// Preserve supplied coordinates without asserting their source validity.
    fn from(range: &Range<usize>) -> Self {
        Self {
            start: range.start,
            end: range.end,
        }
    }
}
