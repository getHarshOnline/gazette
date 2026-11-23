//! Plain metadata and destinations encoded for Theme-owned Markdown documents.
use std::{collections::HashMap, io::Write};

use gazette_core::render::{Error, LimitedWriter};
use serde_json::Value;
use tera::Filter;

pub(super) struct MarkdownText {
    pub output_bytes: usize,
    pub destination: bool,
}

impl Filter for MarkdownText {
    fn filter(&self, value: &Value, args: &HashMap<String, Value>) -> tera::Result<Value> {
        if !args.is_empty() {
            return Err(tera::Error::msg("Markdown encoders accept no arguments"));
        }
        let value = value
            .as_str()
            .ok_or_else(|| tera::Error::msg("Markdown encoders require a string"))?;
        let mut output = LimitedWriter::new(Vec::new(), self.output_bytes);
        let result = (|| {
            if self.destination {
                if value.chars().any(char::is_control) {
                    return Err(std::io::Error::other(
                        "Markdown destination contains a control character",
                    ));
                }
                for c in value.chars() {
                    if matches!(c, '(' | ')' | '&') {
                        output.write_all(b"\\")?;
                    }
                    if c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\\') {
                        let mut buffer = [0; 4];
                        for byte in c.encode_utf8(&mut buffer).as_bytes() {
                            write!(output, "%{byte:02X}")?;
                        }
                    } else {
                        write!(output, "{c}")?;
                    }
                }
            } else {
                let mut words = value.split_whitespace().peekable();
                let mut leading = true;
                let mut ordinal = true;
                while let Some(word) = words.next() {
                    for c in word.chars() {
                        if c.is_control() {
                            return Err(std::io::Error::other(
                                "Markdown text contains a control character",
                            ));
                        }
                        if matches!(
                            c,
                            '\\' | '`'
                                | '*'
                                | '_'
                                | '['
                                | ']'
                                | '<'
                                | '>'
                                | '{'
                                | '}'
                                | '#'
                                | '~'
                                | '|'
                                | '$'
                                | '&'
                        ) || leading && matches!(c, '-' | '+' | '=' | ':')
                            || ordinal && matches!(c, '.' | ')')
                        {
                            output.write_all(b"\\")?;
                        }
                        write!(output, "{c}")?;
                        leading = false;
                        ordinal &= c.is_ascii_digit();
                    }
                    if words.peek().is_some() {
                        output.write_all(b" ")?;
                        ordinal = false;
                    }
                }
            }
            Ok(())
        })();
        if output.exceeded() {
            return Err(tera::Error::chain(
                "Markdown encoding exceeded its byte ceiling",
                Error::Limit("output bytes"),
            ));
        }
        result.map_err(|error: std::io::Error| {
            tera::Error::chain("Markdown encoding failed", error)
        })?;
        String::from_utf8(output.into_inner())
            .map(Value::String)
            .map_err(|error| tera::Error::chain("Markdown encoding was not UTF-8", error))
    }
}
