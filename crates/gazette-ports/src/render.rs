//! Compilation of exact retained templates; selection and meaning remain inward.

use gazette_core::{
    render::{Error, Limits},
    source::{EntryPath, TreeIdentity, TreeInput},
};
use serde_json::Value;

/// Compile without reopening native files or discovering additional templates.
pub trait TemplateCompiler {
    /// Stable compiler/version and behavior profile used for pre-render dependency identity.
    fn profile(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Return an owned compiled set bound to the complete supplied tree identity.
    ///
    /// # Errors
    /// Reject invalid or oversized input; never return a partial template set.
    fn compile(
        &self,
        source: &TreeInput<'_>,
        limits: Limits,
    ) -> Result<Box<dyn CompiledTemplates>, Error>;
}

/// An owned presentation program, independent of former native source locations.
pub trait CompiledTemplates: Send + Sync {
    /// Complete identity of the template inputs consumed by compilation.
    fn identity(&self) -> TreeIdentity;

    /// Stable renderer/version and behavior-profile reference for this compiled program.
    ///
    /// Must be 1..=256 printable ASCII bytes without spaces. The application retains
    /// this declaration with the input identity and requested bounds. It is not a
    /// digest or attestation of the executable, qualification, or publication approval.
    fn profile(&self) -> &'static str;

    /// Render caller-resolved presentation data under the compilation's bounds.
    ///
    /// # Errors
    /// Reject invalid context, missing inputs or failed/oversized emission.
    fn render(&self, entry: &EntryPath, context: &Value) -> Result<Vec<u8>, Error>;
}
