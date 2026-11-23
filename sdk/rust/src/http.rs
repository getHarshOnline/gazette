//! Gazette-owned HTTP executable selection shared by public native operations.

use gazette_core::process;
use serde::{Deserialize, Serialize};

/// Existing trusted curl selection, not executable discovery or dependency attestation.
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Curl {
    /// Absolute selected executable path.
    pub program: String,
    /// Complete executable byte pin, checked by Gazette's native process owner.
    pub executable: process::ExecutablePin,
    /// Absolute existing working directory; not a filesystem sandbox.
    pub directory: String,
}

impl Curl {
    pub(crate) fn native(&self) -> gazette_adapter_http::Curl {
        gazette_adapter_http::Curl {
            program: self.program.clone(),
            executable: self.executable,
            directory: self.directory.clone(),
        }
    }
}
