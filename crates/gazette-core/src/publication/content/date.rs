//! Explicit authored chronology without clock, filesystem or timezone inference.

use std::{cmp::Ordering, fmt, str::FromStr};

use chrono::{DateTime, Datelike, FixedOffset, NaiveDate, SecondsFormat};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

/// A calendar date or timestamp with a known numeric offset, serialized as a string.
///
/// Accepts `YYYY-MM-DD` or `YYYY-MM-DDTHH:MM:SS[.fraction](Z|±HH:MM)`.
/// Years are 0001–9999; offsets are within ±14:00 so the same value is valid
/// in XML Schema publication outputs. Fractions have one to nine digits.
/// Leap seconds, unknown `-00:00` offsets and timezone-less timestamps are unsupported.
/// Calendar validation uses Chrono; no clock or timezone database is consulted.
/// Serialization preserves date-only precision and nonzero offsets, normalizes
/// zero offsets to `Z`, and uses zero, three, six or nine fractional digits without
/// losing nanoseconds. The retained source AST, not this value, owns original spelling.
#[derive(Clone, Debug, PartialEq, Eq, schemars::JsonSchema)]
#[schemars(with = "String")]
pub struct EditorialDate(Value);

#[derive(Clone, Debug, Eq)]
enum Value {
    Date(NaiveDate),
    Timestamp(DateTime<FixedOffset>),
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Date(left), Self::Date(right)) => left == right,
            (Self::Timestamp(left), Self::Timestamp(right)) => {
                left.naive_local() == right.naive_local() && left.offset() == right.offset()
            }
            _ => false,
        }
    }
}

/// Invalid calendar value or unsupported precision/offset spelling; no default is supplied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditorialDateError;

impl fmt::Display for EditorialDateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(
            "expected a quoted YYYY-MM-DD date or YYYY-MM-DDTHH:MM:SS[.1–9 digits](Z|±HH:MM) timestamp; years 0001–9999, offsets within ±14:00, no leap seconds or -00:00",
        )
    }
}

impl std::error::Error for EditorialDateError {}

impl EditorialDate {
    /// Borrow the authored calendar date; a timestamp does not become a date-only value.
    pub fn as_date(&self) -> Option<&NaiveDate> {
        match &self.0 {
            Value::Date(value) => Some(value),
            Value::Timestamp(_) => None,
        }
    }

    /// Borrow the explicit timestamp without reparsing or promoting a calendar date.
    /// The original known offset and nanosecond precision are preserved.
    pub fn as_timestamp(&self) -> Option<&DateTime<FixedOffset>> {
        match &self.0 {
            Value::Timestamp(value) => Some(value),
            Value::Date(_) => None,
        }
    }

    /// Whether the owner supplied a calendar date rather than a timestamp.
    pub fn is_date_only(&self) -> bool {
        matches!(self.0, Value::Date(_))
    }

    /// Compare dates by calendar day or timestamps by instant, not lexical spelling.
    ///
    /// Mixed precision returns `None`: converting a date to midnight would invent
    /// a time and offset. Equal instants may retain different explicit offsets,
    /// so chronological equality does not imply equal serialized values.
    pub fn chronological_cmp(&self, other: &Self) -> Option<Ordering> {
        match (&self.0, &other.0) {
            (Value::Date(left), Value::Date(right)) => Some(left.cmp(right)),
            (Value::Timestamp(left), Value::Timestamp(right)) => Some(left.cmp(right)),
            _ => None,
        }
    }
}

impl FromStr for EditorialDate {
    type Err = EditorialDateError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if !(10..=35).contains(&value.len())
            || !value.is_ascii()
            || !date_shape(value.as_bytes().get(..10).unwrap_or_default())
        {
            return Err(EditorialDateError);
        }
        let parsed = if value.len() == 10 {
            NaiveDate::parse_from_str(value, "%Y-%m-%d").map(Value::Date)
        } else if timestamp_shape(value.as_bytes()) {
            DateTime::parse_from_rfc3339(value).map(Value::Timestamp)
        } else {
            return Err(EditorialDateError);
        }
        .map_err(|_| EditorialDateError)?;
        let (year, offset) = match &parsed {
            Value::Date(date) => (date.year(), 0),
            Value::Timestamp(date) => (date.year(), date.offset().local_minus_utc()),
        };
        if year < 1 || offset.unsigned_abs() > 14 * 60 * 60 {
            return Err(EditorialDateError);
        }
        Ok(Self(parsed))
    }
}

// Restrict lexical forms that the provider otherwise normalizes or truncates.
// Chrono parses calendar/time values; from_str applies the common-output bounds.
fn date_shape(value: &[u8]) -> bool {
    value.len() == 10
        && value.iter().enumerate().all(|(index, byte)| {
            if matches!(index, 4 | 7) {
                *byte == b'-'
            } else {
                byte.is_ascii_digit()
            }
        })
}

fn timestamp_shape(value: &[u8]) -> bool {
    if !(20..=35).contains(&value.len())
        || value[10] != b'T'
        || value[13] != b':'
        || value[16] != b':'
        || ![11, 12, 14, 15, 17, 18]
            .into_iter()
            .all(|index| value[index].is_ascii_digit())
        || &value[17..19] == b"60"
    {
        return false;
    }
    let mut offset = &value[19..];
    if let Some(fraction) = offset.strip_prefix(b".") {
        let digits = fraction
            .iter()
            .take_while(|byte| byte.is_ascii_digit())
            .count();
        if !(1..=9).contains(&digits) {
            return false;
        }
        offset = &fraction[digits..];
    }
    offset == b"Z"
        || (offset.len() == 6
            && matches!(offset[0], b'+' | b'-')
            && offset[3] == b':'
            && [1, 2, 4, 5]
                .into_iter()
                .all(|index| offset[index].is_ascii_digit())
            && offset != b"-00:00")
}

impl fmt::Display for EditorialDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Value::Date(date) => write!(f, "{}", date.format("%Y-%m-%d")),
            Value::Timestamp(date) => {
                f.write_str(&date.to_rfc3339_opts(SecondsFormat::AutoSi, true))
            }
        }
    }
}

impl Serialize for EditorialDate {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for EditorialDate {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(D::Error::custom)
    }
}
