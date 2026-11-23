//! A provider failure rejects this compilation while leaving its caller available.

use gazette_core::resource::Error;

pub(super) fn run<T>(
    operation: impl FnOnce() -> Result<T, Error> + std::panic::UnwindSafe,
) -> Result<T, Error> {
    std::panic::catch_unwind(operation).unwrap_or_else(|_| {
        Err(Error::Compile(
            "stylesheet compiler failed while processing the retained input".into(),
        ))
    })
}
