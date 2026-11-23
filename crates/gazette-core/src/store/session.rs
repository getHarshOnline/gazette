//! Explicit local coordination reservation, not durable writer-fence authority.

use super::{Error, ReadOptions};

/// Permissions and provider limits for a lifetime-bound read-only connection.
#[derive(
    Clone, Copy, Debug, Default, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(default, deny_unknown_fields)]
pub struct SessionOptions {
    /// Reserve the selected existing local coordination directory exclusively.
    /// This neither creates a directory nor grants SQL-write or release permission.
    pub allow_local_reservation: bool,
    /// Database connection limits; the VM budget applies across the connection lifetime.
    pub read: ReadOptions,
}

impl SessionOptions {
    /// Check both local reservation and native coordination permissions before access.
    ///
    /// # Errors
    /// Refuses missing permission or invalid resource limits.
    pub fn validate(self) -> Result<(), Error> {
        if !self.allow_local_reservation {
            return Err(Error::ReservationPermission);
        }
        self.read.validate()
    }
}
