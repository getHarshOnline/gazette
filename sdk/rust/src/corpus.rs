//! Retained source-set parsing through the SDK's sole Markdown composition.

pub use gazette_compiler::corpus::{
    DocumentReference, Error, File, FileError, FileInspection, Input, Inspection, Limits,
    LocatedMathResult, MathLocation, MathResult, ParsedCorpus, RetainedMath, SourceKind,
    decode_retained_math,
};

/// Parse exact supplied files without acquisition, provider discovery or publication effects.
///
/// # Errors
/// Preserves complete-set naming, syntax and resource failures.
pub fn parse<'s>(
    inputs: impl IntoIterator<Item = Input<'s>>,
    limits: Limits,
) -> Result<ParsedCorpus<'s>, Error> {
    ParsedCorpus::parse(inputs, limits, &gazette_adapter_markdown::Comrak)
}
