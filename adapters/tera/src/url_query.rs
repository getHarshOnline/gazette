//! Encode one supplied UTF-8 query value; never construct or resolve a URL.
use std::{collections::HashMap, io::Write};

use gazette_core::render::{Error, LimitedWriter};
use serde_json::Value;
use tera::Filter;

pub(super) struct UrlQuery {
    pub output_bytes: usize,
}

impl Filter for UrlQuery {
    fn filter(&self, value: &Value, args: &HashMap<String, Value>) -> tera::Result<Value> {
        if !args.is_empty() {
            return Err(tera::Error::msg("url_query accepts no arguments"));
        }
        let value = value
            .as_str()
            .ok_or_else(|| tera::Error::msg("url_query requires a string"))?;
        let mut output = LimitedWriter::new(Vec::new(), self.output_bytes);
        for &byte in value.as_bytes() {
            let result =
                if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
                    output.write_all(&[byte])
                } else {
                    write!(output, "%{byte:02X}")
                };
            if output.exceeded() {
                return Err(tera::Error::chain(
                    "url_query expansion exceeded its byte ceiling",
                    Error::Limit("output bytes"),
                ));
            }
            result.map_err(|error| tera::Error::chain("url_query encoding failed", error))?;
        }
        String::from_utf8(output.into_inner())
            .map(Value::String)
            .map_err(|error| tera::Error::chain("url_query encoding was not UTF-8", error))
    }
}

#[cfg(test)]
#[path = "../tests/unit/url_query.rs"]
mod tests;
