//! Bounded JSON-compatible projection of native YAML events, never a YAML grammar.

use serde_json::{Map, Number, Value};
use yaml_rust2::{
    Yaml,
    parser::{Event, Parser},
    scanner::{Scanner, TScalarStyle, TokenType},
};

type Result<T> = std::result::Result<T, &'static str>;

pub(super) fn parse(bytes: &[u8]) -> Result<Value> {
    let text = std::str::from_utf8(bytes).map_err(|_| "OpenAPI YAML must be UTF-8")?;
    directives(text)?;
    let mut input = Input {
        parser: Parser::new(text.chars()),
        events: 300_004,
        values: 100_000,
    };
    if input.next()? != Event::StreamStart || input.next()? != Event::DocumentStart {
        return Err("OpenAPI YAML requires exactly one document");
    }
    let event = input.next()?;
    let value = input.value(event, 0)?;
    if input.next()? != Event::DocumentEnd || input.next()? != Event::StreamEnd {
        return Err("OpenAPI YAML requires exactly one document");
    }
    Ok(value)
}

fn directives(text: &str) -> Result<()> {
    // Events intentionally omit directives. Inspect only the native token prefix,
    // not body syntax, so retained bytes cannot select a different downstream
    // scalar schema. The full parser owns malformed/duplicate directive errors.
    for (index, token) in Scanner::new(text.chars()).enumerate() {
        if index >= 3 {
            return Err("OpenAPI YAML directive prefix exceeds the supported profile");
        }
        match token.1 {
            TokenType::StreamStart(_) | TokenType::VersionDirective(1, 2) => (),
            TokenType::VersionDirective(_, _) => return Err("OpenAPI YAML requires version 1.2"),
            TokenType::TagDirective(_, _) => {
                return Err("OpenAPI YAML tag directives are outside the supported profile");
            }
            TokenType::DocumentEnd => return Err("invalid OpenAPI YAML directive prefix"),
            _ => break,
        }
    }
    Ok(())
}

struct Input<'a> {
    parser: Parser<std::str::Chars<'a>>,
    events: usize,
    values: usize,
}

impl Input<'_> {
    fn next(&mut self) -> Result<Event> {
        self.events = self
            .events
            .checked_sub(1)
            .ok_or("OpenAPI YAML event ceiling exceeded")?;
        let (event, _) = self
            .parser
            .next_token()
            .map_err(|_| "invalid OpenAPI YAML")?;
        match &event {
            Event::Scalar(_, _, anchor, tag)
            | Event::SequenceStart(anchor, tag)
            | Event::MappingStart(anchor, tag)
                if *anchor != 0 || tag.is_some() =>
            {
                Err("OpenAPI YAML tags and anchors are outside the supported profile")
            }
            Event::Alias(_) => Err("OpenAPI YAML aliases are outside the supported profile"),
            _ => Ok(event),
        }
    }

    fn value(&mut self, event: Event, depth: usize) -> Result<Value> {
        if depth >= 128 {
            return Err("OpenAPI YAML nesting ceiling exceeded");
        }
        self.values = self
            .values
            .checked_sub(1)
            .ok_or("OpenAPI YAML exceeds 100000 values; narrow the artifact")?;
        match event {
            Event::Scalar(text, style, _, _) => scalar(text, style),
            Event::SequenceStart(_, _) => {
                let mut values = Vec::new();
                loop {
                    let event = self.next()?;
                    if event == Event::SequenceEnd {
                        return Ok(Value::Array(values));
                    }
                    values.push(self.value(event, depth + 1)?);
                }
            }
            Event::MappingStart(_, _) => {
                let mut values = Map::new();
                loop {
                    let event = self.next()?;
                    if event == Event::MappingEnd {
                        return Ok(Value::Object(values));
                    }
                    let Event::Scalar(text, style, _, _) = event else {
                        return Err("OpenAPI YAML object keys must be strings");
                    };
                    if text == "<<" && style == TScalarStyle::Plain {
                        return Err("OpenAPI YAML merge keys are outside the supported profile");
                    }
                    let Value::String(key) = scalar(text, style)? else {
                        return Err(
                            "OpenAPI YAML object keys must be strings; quote numeric response keys",
                        );
                    };
                    if values.contains_key(&key) {
                        return Err("duplicate OpenAPI YAML object member");
                    }
                    let event = self.next()?;
                    values.insert(key, self.value(event, depth + 1)?);
                }
            }
            _ => Err("invalid OpenAPI YAML value"),
        }
    }
}

fn scalar(text: String, style: TScalarStyle) -> Result<Value> {
    if style != TScalarStyle::Plain {
        return Ok(Value::String(text));
    }
    // Never pass numbers through the provider's i64/f64 values. Its scalar
    // classification supplies YAML semantics; Serde consumes the original token.
    // Non-JSON numeric spellings fail explicitly rather than changing meaning.
    if text.starts_with("0x") || text.starts_with("0o") {
        return Err("OpenAPI YAML requires JSON numeric spelling");
    }
    if matches!(text.as_str(), "Null" | "NULL") {
        return Ok(Value::Null);
    }
    match Yaml::from_str(&text) {
        Yaml::Null => Ok(Value::Null),
        Yaml::Boolean(value) => Ok(Value::Bool(value)),
        Yaml::Integer(_) | Yaml::Real(_) => serde_json::from_str::<Number>(&text)
            .map(Value::Number)
            .map_err(|_| "OpenAPI YAML requires finite JSON numeric spelling"),
        Yaml::String(_) => Ok(Value::String(text)),
        _ => Err("unsupported OpenAPI YAML scalar"),
    }
}
