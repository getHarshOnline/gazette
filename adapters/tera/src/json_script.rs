//! JSON transport for HTML script text, not semantic generation or context guessing.

use std::{
    collections::HashMap,
    io::{self, Write},
};

use gazette_core::render::{Error, LimitedWriter};
use serde::Serialize;
use serde_json::{
    Value,
    ser::{Formatter, Serializer},
};
use tera::Filter;

pub(super) struct JsonScript {
    pub output_bytes: usize,
}

impl Filter for JsonScript {
    fn filter(&self, value: &Value, args: &HashMap<String, Value>) -> tera::Result<Value> {
        if !args.is_empty() {
            return Err(tera::Error::msg("json_script accepts no arguments"));
        }
        let mut output = LimitedWriter::new(Vec::new(), self.output_bytes);
        let serialized = value.serialize(&mut Serializer::with_formatter(
            &mut output,
            ScriptFormatter,
        ));
        if output.exceeded() {
            return Err(tera::Error::chain(
                "json_script expansion exceeded its byte ceiling",
                Error::Limit("output bytes"),
            ));
        }
        serialized
            .map_err(|error| tera::Error::chain("json_script serialization failed", error))?;
        let text = String::from_utf8(output.into_inner()).map_err(|error| {
            tera::Error::chain("json_script serialization was not UTF-8", error)
        })?;
        Ok(Value::String(text))
    }

    // Tera's safety flag only disables HTML autoescaping. Callers must select this
    // filter for HTML script-element text, never attribute/CSS/other contexts.
    fn is_safe(&self) -> bool {
        true
    }
}

/// Keep serde's compact JSON grammar and ordinary escapes; only string fragments
/// need the additional script-text transport escapes. This covers object keys too.
struct ScriptFormatter;

impl Formatter for ScriptFormatter {
    fn write_string_fragment<W>(&mut self, writer: &mut W, fragment: &str) -> io::Result<()>
    where
        W: ?Sized + Write,
    {
        let mut start = 0;
        for (index, character) in fragment.char_indices() {
            let escape: &[u8] = match character {
                '<' => b"\\u003c",
                '>' => b"\\u003e",
                '&' => b"\\u0026",
                '\'' => b"\\u0027",
                '\u{2028}' => b"\\u2028",
                '\u{2029}' => b"\\u2029",
                _ => continue,
            };
            writer.write_all(&fragment.as_bytes()[start..index])?;
            writer.write_all(escape)?;
            start = index + character.len_utf8();
        }
        writer.write_all(&fragment.as_bytes()[start..])
    }
}
