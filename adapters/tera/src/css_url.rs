//! One supplied string becomes a bounded CSS URL token for HTML style raw text.
use std::{collections::HashMap, io::Write};

use gazette_core::render::{Error, LimitedWriter};
use serde_json::Value;
use tera::Filter;

pub(super) struct CssUrl {
    pub output_bytes: usize,
}

impl Filter for CssUrl {
    fn filter(&self, value: &Value, args: &HashMap<String, Value>) -> tera::Result<Value> {
        if !args.is_empty() {
            return Err(tera::Error::msg("css_url accepts no arguments"));
        }
        let value = value
            .as_str()
            .ok_or_else(|| tera::Error::msg("css_url requires a string"))?;
        // CSS replaces both literal and escaped NUL with U+FFFD. Refuse rather
        // than claim to retain a URL that CSS cannot represent exactly.
        if value.contains('\0') {
            return Err(tera::Error::msg("css_url cannot represent NUL"));
        }
        let mut output = LimitedWriter::new(Vec::new(), self.output_bytes);
        let encoded = (|| {
            output.write_all(b"url(\"")?;
            let mut start = 0;
            for (index, character) in value.char_indices() {
                if character.is_control()
                    || matches!(
                        character,
                        '"' | '\'' | '\\' | '<' | '>' | '&' | '\u{2028}' | '\u{2029}'
                    )
                {
                    output.write_all(&value.as_bytes()[start..index])?;
                    // The terminator prevents a following hex digit or real
                    // whitespace from being consumed as part of this escape.
                    write!(output, "\\{:x} ", u32::from(character))?;
                    start = index + character.len_utf8();
                }
            }
            output.write_all(&value.as_bytes()[start..])?;
            output.write_all(b"\")")
        })();
        if output.exceeded() {
            return Err(tera::Error::chain(
                "css_url expansion exceeded its byte ceiling",
                Error::Limit("output bytes"),
            ));
        }
        encoded.map_err(|error| tera::Error::chain("css_url encoding failed", error))?;
        let encoded = String::from_utf8(output.into_inner())
            .map_err(|error| tera::Error::chain("css_url encoding was not UTF-8", error))?;
        Ok(Value::String(encoded))
    }

    // This flag disables HTML autoescaping; it does not discover the output
    // context. Trusted templates use this only in style-element raw text,
    // never an HTML attribute, script, selector or general CSS expression.
    fn is_safe(&self) -> bool {
        true
    }
}
