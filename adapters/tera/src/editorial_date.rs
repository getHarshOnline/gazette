//! Format supplied editorial chronology in its authored calendar and offset.
use std::{collections::HashMap, fmt, io::Write};

use gazette_core::{
    publication::content::EditorialDate,
    render::{Error, LimitedWriter},
};
use serde_json::Value;
use tera::Filter;

pub(super) struct FormatDate {
    pub output_bytes: usize,
}

struct Output(LimitedWriter<Vec<u8>>);

impl fmt::Write for Output {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        self.0.write_all(value.as_bytes()).map_err(|_| fmt::Error)
    }
}

impl Filter for FormatDate {
    fn filter(&self, value: &Value, args: &HashMap<String, Value>) -> tera::Result<Value> {
        if args.keys().any(|key| key != "format") {
            return Err(tera::Error::msg("editorial_date accepts only format"));
        }
        let format = match args.get("format") {
            None => "%-d %B %Y",
            Some(Value::String(value)) => value,
            Some(_) => return Err(tera::Error::msg("editorial_date format must be a string")),
        };
        let date: EditorialDate = value
            .as_str()
            .ok_or_else(|| tera::Error::msg("editorial_date requires a date string"))?
            .parse()
            .map_err(|error| tera::Error::chain("invalid editorial date", error))?;
        let mut output = Output(LimitedWriter::new(Vec::new(), self.output_bytes));
        let result = if let Some(date) = date.as_date() {
            date.format(format).write_to(&mut output)
        } else if let Some(date) = date.as_timestamp() {
            date.format(format).write_to(&mut output)
        } else {
            unreachable!("editorial chronology is a date or timestamp")
        };
        if output.0.exceeded() {
            return Err(tera::Error::chain(
                "editorial_date exceeded its byte ceiling",
                Error::Limit("output bytes"),
            ));
        }
        result.map_err(|_| {
            tera::Error::msg(
                "editorial_date format is invalid or needs unavailable time or offset fields",
            )
        })?;
        String::from_utf8(output.0.into_inner())
            .map(Value::String)
            .map_err(|error| tera::Error::chain("editorial_date output was not UTF-8", error))
    }
}
