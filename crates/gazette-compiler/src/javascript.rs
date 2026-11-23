//! Native JavaScript syntax text, not execution or a second publication model.
//!
//! Discard all callback findings if inspection returns an error. Input bytes are
//! bounded before parsing; OXC parser/semantic allocations and recursion precede
//! the node/depth checks. These limits are not an OS, CPU or parser-stack sandbox.
//! No TypeScript/JSX, concatenation, evaluation, regex or URL/base64 decoding is
//! performed. Templates contribute individual cooked chunks, never evaluated text.

use std::fmt;

use oxc_allocator::Allocator;
use oxc_ast::AstKind;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;
use serde::Serialize;

/// Pinned native parser/early-error checker and decoded syntax-text profile.
pub const PROFILE: &str = "oxc@0.95.0/gazette-syntax-text-v1";

/// Explicit native parse goal; no filename or runtime inference.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Goal {
    /// Classic JavaScript, without static module declarations.
    Script,
    /// ECMAScript module, including its strict-mode rules.
    Module,
    /// OXC selects module only when native module syntax establishes it.
    Unambiguous,
}

/// Invocation bounds, not pre-parse native allocation guarantees.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Exact source bytes, checked before UTF-8 decoding and native allocation.
    pub input_bytes: usize,
    /// All native semantic AST nodes, including the Program root.
    pub nodes: usize,
    /// Native parent-chain depth, including the Program root at depth one.
    pub depth: usize,
    /// Sum of UTF-8 bytes delivered to the consumer, including repeated values.
    pub decoded_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            input_bytes: 1 << 20,
            nodes: 100_000,
            depth: 128,
            decoded_bytes: 2 << 20,
        }
    }
}

/// Native text category; occurrences are not deduplicated or concatenated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextKind {
    /// Binding, reference, property, label or private identifier (without `#`).
    Identifier,
    /// Decoded string literal, including directives and module specifiers.
    String,
    /// One cooked template chunk, possibly empty.
    Template,
}

/// Redacted failure; only a caller error carries caller-owned details.
#[derive(Debug, PartialEq, Eq)]
pub enum Error<E> {
    /// An invocation bound was exceeded.
    Limit(&'static str),
    /// Input is not UTF-8.
    Utf8,
    /// Native parsing or JavaScript early-error validation failed.
    Malformed,
    /// A native value has no faithful supported UTF-8 representation.
    Unsupported,
    /// The consumer rejected a decoded occurrence.
    Consumer(E),
}

impl<E> fmt::Display for Error<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Limit(kind) => write!(f, "JavaScript inspection limit: {kind}"),
            Self::Utf8 => f.write_str("JavaScript input is not UTF-8"),
            Self::Malformed => f.write_str("JavaScript syntax or early-error validation failed"),
            Self::Unsupported => f.write_str("JavaScript value is outside the inspection profile"),
            Self::Consumer(_) => f.write_str("JavaScript text consumer rejected input"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for Error<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Consumer(error) => Some(error),
            _ => None,
        }
    }
}

/// Inspect decoded native syntax occurrences without executing JavaScript.
///
/// The complete parse and early-error check precede every callback. Subsequent
/// unsupported values, bounds or consumer failures invalidate earlier findings.
/// Native parent-chain traversal is iterative and stops at the requested depth;
/// this does not bound the parser or semantic builder's preceding native walk.
///
/// # Errors
/// Rejects invalid UTF-8, native syntax/early errors, unsupported cooked strings,
/// exhausted bounds and the exact consumer error. Native diagnostics are redacted.
pub fn inspect<E>(
    bytes: &[u8],
    goal: Goal,
    limits: Limits,
    mut consumer: impl FnMut(TextKind, &str) -> Result<(), E>,
) -> Result<(), Error<E>> {
    if bytes.len() > limits.input_bytes {
        return Err(Error::Limit("input bytes"));
    }
    let source = std::str::from_utf8(bytes).map_err(|_| Error::Utf8)?;
    let source_type = match goal {
        Goal::Script => SourceType::cjs(),
        Goal::Module => SourceType::mjs(),
        Goal::Unambiguous => SourceType::unambiguous(),
    };
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, source_type).parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        return Err(Error::Malformed);
    }
    let program = allocator.alloc(parsed.program);
    let checked = SemanticBuilder::new()
        .with_check_syntax_error(true)
        .build(program);
    if !checked.errors.is_empty() {
        return Err(Error::Malformed);
    }
    let nodes = checked.semantic.nodes();
    if nodes.len() > limits.nodes {
        return Err(Error::Limit("nodes"));
    }
    let mut remaining = limits.decoded_bytes;
    for node in nodes {
        if limits.depth == 0
            || nodes.ancestor_ids(node.id()).take(limits.depth).count() >= limits.depth
        {
            return Err(Error::Limit("nesting depth"));
        }
        let text = match node.kind() {
            AstKind::IdentifierName(value) => Some((TextKind::Identifier, value.name.as_str())),
            AstKind::IdentifierReference(value) => {
                Some((TextKind::Identifier, value.name.as_str()))
            }
            AstKind::BindingIdentifier(value) => Some((TextKind::Identifier, value.name.as_str())),
            AstKind::LabelIdentifier(value) => Some((TextKind::Identifier, value.name.as_str())),
            AstKind::PrivateIdentifier(value) => Some((TextKind::Identifier, value.name.as_str())),
            AstKind::StringLiteral(value) => {
                if value.lone_surrogates {
                    return Err(Error::Unsupported);
                }
                Some((TextKind::String, value.value.as_str()))
            }
            AstKind::TemplateElement(value) => {
                if value.lone_surrogates {
                    return Err(Error::Unsupported);
                }
                Some((
                    TextKind::Template,
                    value
                        .value
                        .cooked
                        .as_ref()
                        .ok_or(Error::Unsupported)?
                        .as_str(),
                ))
            }
            _ => None,
        };
        if let Some((kind, value)) = text {
            remaining = remaining
                .checked_sub(value.len())
                .ok_or(Error::Limit("decoded bytes"))?;
            consumer(kind, value).map_err(Error::Consumer)?;
        }
    }
    Ok(())
}
