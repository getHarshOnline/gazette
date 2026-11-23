//! Typed application-command ownership shared by direct CLI and MCP projections.

pub mod authoring;
pub mod configuration;
pub mod coordinator;
pub mod documentation;
pub mod inspection;
pub mod metadata;
pub mod observation;
pub mod preview;
pub mod publication;
pub mod recovery_session;
pub mod release;
pub mod release_artifact;
pub mod release_session;
pub mod rendered_output;
pub mod store;
pub mod store_session;

/// Reusable application-command environment.
///
/// Stateless commands use the selected Rust SDK directly. Process-held session
/// managers are added here only when their complete CLI/MCP family is migrated.
#[derive(Default)]
pub struct Kernel {
    authoring: authoring::Sessions,
    preview: preview::Sessions,
    release_session: release_session::Sessions,
    recovery_session: recovery_session::Sessions,
    store_session: store_session::Sessions,
}

impl Kernel {
    /// Construct one reusable command environment without starting native work.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Execute one typed command through its sole application owner.
    ///
    /// # Errors
    /// Returns the command family's typed failure without transport conversion.
    pub fn execute<C: Command>(&self, command: C) -> Result<C::Outcome, C::Failure> {
        command.execute(self)
    }
}

/// One typed application command independent of CLI or protocol framing.
pub trait Command: Sized {
    /// Successful typed application result.
    type Outcome;
    /// Typed application failure retained before transport projection.
    type Failure;

    /// Execute this exact request through the reusable command environment.
    ///
    /// # Errors
    /// Returns this command family's typed failure without transport conversion.
    fn execute(self, kernel: &Kernel) -> Result<Self::Outcome, Self::Failure>;
}
