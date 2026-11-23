//! Transport-neutral authoring values projected from syntax and schema owners.

use std::fmt;

use gazette_core::source::Location;
use serde::Serialize;

/// One structural step through the generated workspace schema.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum AuthoringPathSegment {
    /// Descend through an exact TOML key.
    Field(String),
    /// Descend through one array or array-of-tables item.
    ArrayItem,
}

/// JSON-compatible closed literal admitted by the Rust-owned schema.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum AuthoringLiteral {
    /// String literal value without TOML quoting.
    String(String),
    /// Signed integer literal.
    Integer(i64),
    /// Boolean literal.
    Boolean(bool),
}

/// One exact decoded TOML string occurrence from the tolerant native syntax owner.
///
/// The decoded value is evidence about the current source bytes only. Consumers
/// must still join it to their strict typed model before assigning meaning.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StringOccurrence {
    /// Exact scalar construct, including its original TOML quoting.
    pub location: Location,
    /// Generated-schema path of the scalar occurrence.
    pub path: Vec<AuthoringPathSegment>,
    /// TOML-decoded string value.
    pub value: String,
}

/// One literal and its schema-derived meaning.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct AuthoringLiteralCandidate {
    /// Exact semantic value.
    pub value: AuthoringLiteral,
    /// Rustdoc-derived variant meaning, when declared.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Structural value categories admitted by the generated schema.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AuthoringValueKind {
    /// Boolean value.
    Boolean,
    /// Integer value.
    Integer,
    /// String value.
    String,
    /// Ordered array value.
    Array,
    /// TOML table or inline-table value.
    Table,
}

/// Whether all applicable structural variants require a field.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AuthoringFieldPresence {
    /// Every applicable object variant requires the field.
    Required,
    /// No applicable object variant requires the field.
    Optional,
    /// Only some applicable object variants require the field.
    VariantDependent,
}

/// One child key projected from the Rust-owned schema.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct AuthoringField {
    /// Exact authored key spelling.
    pub name: String,
    /// Rustdoc-derived field meaning, when declared.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Requiredness across applicable object variants.
    pub presence: AuthoringFieldPresence,
    /// Structural categories admitted for the field value.
    pub value_kinds: Vec<AuthoringValueKind>,
    /// Closed literal vocabulary, when one exists.
    pub allowed_values: Vec<AuthoringLiteralCandidate>,
}

/// Schema facts at one exact structural path.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct AuthoringNode {
    /// Rustdoc-derived meaning of this field or value context.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Structural categories admitted at this path.
    pub value_kinds: Vec<AuthoringValueKind>,
    /// Closed literals admitted at this path.
    pub allowed_values: Vec<AuthoringLiteralCandidate>,
    /// Deterministically ordered child keys.
    pub fields: Vec<AuthoringField>,
}

/// Kind of exact replacement proposed by workspace authoring help.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CompletionKind {
    /// Replace or insert a field name.
    Field,
    /// Replace or insert a closed literal.
    Literal,
}

/// One schema-derived replacement candidate.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct CompletionCandidate {
    /// Concise display label.
    pub label: String,
    /// Exact TOML bytes to place at the completion range.
    pub replacement: String,
    /// Schema-derived meaning, when declared.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Requiredness for a field candidate; absent for literal candidates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence: Option<AuthoringFieldPresence>,
    /// Structural categories associated with the candidate.
    pub value_kinds: Vec<AuthoringValueKind>,
}

/// Schema facts at the exact key or value beneath the cursor.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct SchemaHover {
    /// Exact source range whose facts are described.
    pub location: Location,
    /// Exact generated-schema path.
    pub path: Vec<AuthoringPathSegment>,
    /// Field facts when the cursor is on a key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<AuthoringField>,
    /// Facts for the selected field value or structural node.
    pub node: AuthoringNode,
}

/// Current source-local authoring result, independent from strict validity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct AuthoringHelp {
    /// Exact replacement range when completion is available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_location: Option<Location>,
    /// Exact current bytes at the completion range.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_preimage: Option<String>,
    /// Replacement category when completion is available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_kind: Option<CompletionKind>,
    /// Complete schema-derived candidates; never a truncated prefix.
    pub candidates: Vec<CompletionCandidate>,
    /// Schema facts beneath the cursor, when the path is known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hover: Option<SchemaHover>,
}

impl AuthoringHelp {
    /// Lift all exact ranges from a retained child slice into its source parent.
    ///
    /// # Errors
    ///
    /// Returns [`AuthoringError::Invalid`] when a child range cannot be proven
    /// to belong to the exact retained parent slice.
    pub fn in_parent(
        mut self,
        source: &str,
        parent: gazette_core::source::Span,
    ) -> Result<Self, AuthoringError> {
        if let Some(location) = &mut self.completion_location {
            location.span = location
                .span
                .in_parent(source, parent)
                .ok_or(AuthoringError::Invalid)?;
        }
        if let Some(hover) = &mut self.hover {
            hover.location.span = hover
                .location
                .span
                .in_parent(source, parent)
                .ok_or(AuthoringError::Invalid)?;
        }
        Ok(self)
    }

    pub(crate) fn relative_to(
        mut self,
        source: &str,
        parent: gazette_core::source::Span,
    ) -> Result<Self, AuthoringError> {
        let child = parent.slice(source).ok_or(AuthoringError::Invalid)?;
        let relative = |span: gazette_core::source::Span| {
            let start = span
                .start
                .checked_sub(parent.start)
                .ok_or(AuthoringError::Invalid)?;
            let end = span
                .end
                .checked_sub(parent.start)
                .ok_or(AuthoringError::Invalid)?;
            gazette_core::source::Span::checked(child, start..end).ok_or(AuthoringError::Invalid)
        };
        if let Some(location) = &mut self.completion_location {
            location.span = relative(location.span)?;
            if location.span.slice(child) != self.completion_preimage.as_deref() {
                return Err(AuthoringError::Invalid);
            }
        }
        if let Some(hover) = &mut self.hover {
            hover.location.span = relative(hover.location.span)?;
        }
        Ok(self)
    }
}

/// Tolerant authoring inspection failed without changing strict workspace behavior.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthoringError {
    /// A caller-selected workspace ceiling was exhausted.
    Limit(&'static str),
    /// The exact source position or generated schema path is invalid.
    Invalid,
}

impl fmt::Display for AuthoringError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Limit(resource) => {
                write!(formatter, "TOML authoring {resource} ceiling exceeded")
            }
            Self::Invalid => formatter.write_str("TOML authoring source or schema path is invalid"),
        }
    }
}

impl std::error::Error for AuthoringError {}

pub(super) struct Completion {
    pub(super) location: Location,
    pub(super) preimage: String,
    pub(super) kind: CompletionKind,
    pub(super) candidates: Vec<CompletionCandidate>,
}

/// One source-owned key and its schema facts for an editor outline.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct AuthoringStructureField {
    /// Exact schema path; repeated array items remain separate source occurrences.
    pub path: Vec<AuthoringPathSegment>,
    /// Exact key or table-header source range.
    pub location: Location,
    /// Original key spelling, never a normalized replacement.
    pub name: String,
    /// Original value spelling when one complete native value follows this key.
    pub value: Option<String>,
    /// Native schema facts, absent for an unknown key.
    pub schema: Option<AuthoringField>,
}
