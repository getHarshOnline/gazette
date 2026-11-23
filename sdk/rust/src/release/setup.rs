//! Explicit native parent acquisition and configured metadata, with unchanged detached custody.

use super::*;
use gazette_ports::release::{OutputCheckout, ReleaseParentAcquisition};

/// Acquire one exact observed parent into a fresh private bare repository.
/// Remote refs are read independently before and after the bounded fetch.
///
/// # Errors
/// Preserves partial local storage and process cleanup; no failed read becomes an unborn parent.
pub fn acquire_parent(
    git: &GitSelection,
    request: ParentAcquisitionRequest<'_>,
    options: ParentAcquisitionOptions,
) -> Result<ParentAcquisitionReport, RepositorySetupError> {
    gazette_adapter_git::ParentAcquirer {
        runner: &gazette_adapter_process::Runner,
        selection: git,
    }
    .acquire_parent(request, options)
}

/// Initialize or fast-forward explicitly selected local output metadata only.
/// Existing generated and unrelated working files are neither imported nor removed.
///
/// # Errors
/// Refuses unsafe adoption, changed refs, divergence or bounds; preserves partial metadata.
pub fn prepare_output_checkout(
    request: OutputCheckoutRequest<'_>,
    options: OutputCheckoutOptions,
) -> Result<OutputCheckoutReport, RepositorySetupError> {
    gazette_adapter_git::Git.prepare_output_checkout(request, options)
}
