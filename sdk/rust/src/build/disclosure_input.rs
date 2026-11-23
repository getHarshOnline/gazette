//! Private literal-rule transport shared by interfaces; scanning and validation stay application-owned.

use super::{DisclosureProbeError, DisclosureProbeLimits};
use crate::workspace::Key;
use serde::{
    Deserialize, Deserializer,
    de::{MapAccess, Visitor, value::MapAccessDeserializer},
};
use std::{collections::BTreeMap, fmt, marker::PhantomData};

/// Caller-selected diagnostic ceilings shared by CLI and MCP requests.
/// Omitted fields retain native defaults; zero remains an explicit exhausted
/// budget. This changes resource selection, never disclosure policy or approval.
#[derive(Clone, Copy, Debug, Deserialize, serde::Serialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ScanLimits {
    /// Maximum bytes presented to one percent/data decoding layer.
    #[schemars(range(max = usize::MAX))]
    pub encoded_input_bytes: usize,
    /// Maximum bytes produced by one percent/data decoding layer.
    #[schemars(range(max = usize::MAX))]
    pub decoded_bytes: usize,
    /// Maximum nested decoding depth; native validation retains its hard ceiling.
    #[schemars(range(max = usize::MAX))]
    pub encoded_depth: usize,
    /// Maximum aggregate decoded values, including native Markdown facts.
    #[schemars(range(max = usize::MAX))]
    pub encoded_values: usize,
    /// Maximum aggregate decoded bytes across the complete output.
    #[schemars(range(max = usize::MAX))]
    pub encoded_bytes: usize,
    /// Shared conservative comparison-work ceiling across raw and decoded channels.
    #[schemars(range(max = u64::MAX))]
    pub work_bytes: u64,
    /// Maximum tokenizer callbacks per HTML input, including embedded Markdown HTML.
    #[schemars(range(max = usize::MAX))]
    pub html_tokens: usize,
}

impl Default for ScanLimits {
    fn default() -> Self {
        let limits = DisclosureProbeLimits::default();
        Self {
            encoded_input_bytes: limits.encoded.input_bytes,
            decoded_bytes: limits.encoded.decoded_bytes,
            encoded_depth: limits.encoded_depth,
            encoded_values: limits.encoded_values,
            encoded_bytes: limits.encoded_bytes,
            work_bytes: limits.work_bytes,
            html_tokens: limits.html.tokens,
        }
    }
}

impl ScanLimits {
    /// Select the same native resource fields for every transport. All other
    /// ceilings and native validation remain unchanged.
    pub fn native(&self) -> DisclosureProbeLimits {
        let mut limits = DisclosureProbeLimits::default();
        limits.encoded.input_bytes = self.encoded_input_bytes;
        limits.encoded.decoded_bytes = self.decoded_bytes;
        limits.encoded_depth = self.encoded_depth;
        limits.encoded_values = self.encoded_values;
        limits.encoded_bytes = self.encoded_bytes;
        limits.work_bytes = self.work_bytes;
        limits.html.tokens = self.html_tokens;
        limits
    }
}

/// Existing rendered-source request, without performing acquisition or rendering.
pub struct WorkspaceInput {
    /// Supplied configuration text; no configured native path has been opened.
    pub workspace: String,
    /// Complete unique literal rules, validated by the existing scanner owner.
    pub rules: BTreeMap<Key, String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    format: String,
    workspace: String,
    rules: Vec<Object<Rule>>,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct OutputRequest {
    #[schemars(with = "OutputFormat")]
    format: String,
    #[schemars(with = "Vec<Rule>")]
    rules: Vec<Object<Rule>>,
}

#[derive(schemars::JsonSchema)]
#[allow(dead_code)]
enum OutputFormat {
    #[serde(rename = "gazette-disclosure-output-input/1")]
    V1,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Rule {
    id: Key,
    literal: String,
}

impl WorkspaceInput {
    /// Decode the existing rendered-source envelope with no native effects.
    /// The caller must bound complete input bytes before parsing.
    ///
    /// # Errors
    /// Rejects malformed, positional, duplicate or unknown fields and invalid rules.
    /// Diagnostics never include supplied configuration, literals or parser snippets.
    pub fn parse(text: &str) -> Result<Self, Error> {
        let Object(Request {
            format,
            workspace,
            rules,
        }) = serde_json::from_str(text).map_err(|_| Error::Request)?;
        if format != "gazette-disclosure-probe-input/1" {
            return Err(Error::Request);
        }
        Ok(Self {
            workspace,
            rules: select_rules(rules)?,
        })
    }
}

/// Decode the complete pinned-output rule envelope before any native read.
///
/// # Errors
/// Rejects the byte ceiling, malformed/positional/duplicate/unknown fields and
/// invalid rules with static, private-input-safe diagnostics.
pub fn output_rules(text: &str, maximum_bytes: usize) -> Result<BTreeMap<Key, String>, Error> {
    if text.len() > maximum_bytes {
        return Err(Error::OutputLimit);
    }
    let Object(OutputRequest { format, rules }) =
        serde_json::from_str(text).map_err(|_| Error::OutputRequest)?;
    if format != "gazette-disclosure-output-input/1" {
        return Err(Error::OutputRequest);
    }
    select_rules(rules).map_err(|error| match error {
        Error::Request => Error::OutputRequest,
        error => error,
    })
}

/// Inline discovery schema for the same strict private envelope parser.
/// Schema describes transport shape; native rule validation remains mandatory.
pub fn output_schema() -> schemars::Schema {
    schemars::generate::SchemaSettings::draft2020_12()
        .with(|settings| settings.inline_subschemas = true)
        .into_generator()
        .into_root_schema_for::<OutputRequest>()
}

fn select_rules(rules: Vec<Object<Rule>>) -> Result<BTreeMap<Key, String>, Error> {
    let limits = DisclosureProbeLimits::default();
    if rules.len() > limits.rules {
        return Err(Error::Rules(DisclosureProbeError::Limit("rule count")));
    }
    let mut selected = BTreeMap::new();
    for Object(Rule { id, literal }) in rules {
        if selected.insert(id, literal).is_some() {
            return Err(Error::Request);
        }
    }
    limits.validate_rules(&selected).map_err(Error::Rules)?;
    Ok(selected)
}

/// Redacted input or native rule-validation failure, never a partial rule set.
#[derive(Debug)]
pub enum Error {
    /// Invalid existing workspace/rules envelope.
    Request,
    /// Invalid existing output/rules envelope.
    OutputRequest,
    /// Complete private input exceeded its selected byte ceiling.
    OutputLimit,
    /// Native rule semantics or limits failed.
    Rules(DisclosureProbeError),
}

impl Error {
    /// Stable machine code without private values.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Request => "disclosure_probe_input",
            Self::OutputRequest => "disclosure_output_input",
            Self::OutputLimit => "disclosure_output_input_limit",
            Self::Rules(error) => error.code(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Request => f.write_str("expected exact gazette-disclosure-probe-input/1 JSON with workspace TOML and unique id/literal rules; inspect private stdin; no output produced"),
            Self::OutputRequest => f.write_str("expected exact gazette-disclosure-output-input/1 JSON with unique id/literal rules; inspect private stdin; no output produced"),
            Self::OutputLimit => f.write_str("private rules JSON byte ceiling exceeded; no output produced"),
            Self::Rules(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for Error {}

struct Object<T>(T);
impl<'de, T: Deserialize<'de>> Deserialize<'de> for Object<T> {
    fn deserialize<D: Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        struct Named<T>(PhantomData<fn() -> T>);
        impl<'de, T: Deserialize<'de>> Visitor<'de> for Named<T> {
            type Value = Object<T>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a named JSON object")
            }
            fn visit_map<M: MapAccess<'de>>(self, map: M) -> Result<Self::Value, M::Error> {
                T::deserialize(MapAccessDeserializer::new(map)).map(Object)
            }
        }
        decoder.deserialize_map(Named(PhantomData))
    }
}
