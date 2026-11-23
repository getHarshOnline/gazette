//! Resource transforms consume captured sources, never workspace or publication authority.

use gazette_core::{
    resource::{Artifact, Error, Limits, ModuleLinks, Recipe},
    source::TreeInput,
};

/// Compile one exact entry without native reads, package discovery or output writes.
pub trait ResourceCompiler {
    /// Stable compiler/version and behavior profile used for pre-render dependency identity.
    fn profile(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Return complete bytes and consumed-source evidence bound to the supplied tree.
    ///
    /// # Errors
    /// Reject unsupported inputs, unresolved imports, invalid syntax and exhausted bounds.
    /// No live filesystem, network fallback or successful partial result is permitted.
    fn compile(
        &self,
        source: &TreeInput<'_>,
        recipe: &Recipe,
        links: &ModuleLinks,
        limits: Limits,
    ) -> Result<Artifact, Error>;
}
