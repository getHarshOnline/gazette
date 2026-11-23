//! Strict preparation-selection decoding shared by native interface projections.

use super::{PreparationBatchError, PreparationSelection};

/// Decode one exact native selection without losing duplicate fields or accepting positional forms.
///
/// Callers bound input bytes before parsing. The shared JSON grammar checks duplicate decoded
/// keys and nesting; the native selection type owns fields and values. Comparing its serialized
/// shape rejects permissive Serde forms, including ignored fields on unit enum variants.
/// This checks representation only, not native preparation evidence or release permission.
///
/// # Errors
/// Refuses malformed JSON or any representation other than the native named selection shape.
pub fn parse_preparation_selection(
    bytes: &[u8],
) -> Result<PreparationSelection, PreparationBatchError> {
    let original = crate::json::parse(bytes).map_err(|_| PreparationBatchError::Selection)?;
    let selection: PreparationSelection =
        serde_json::from_slice(bytes).map_err(|_| PreparationBatchError::Selection)?;
    if serde_json::to_value(&selection).ok().as_ref() != Some(&original) {
        return Err(PreparationBatchError::Selection);
    }
    Ok(selection)
}
