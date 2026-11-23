//! Shared, spend-only accounting across a compound revision observation.

use std::fmt;

/// Remaining decoded commit/tree bytes available to one observation.
///
/// The application creates this once and lends it across all participating
/// reads. Adapters cannot increase, reset or clone its balance. This bounds
/// charged logical metadata, not a native library's internal allocations.
#[derive(Debug)]
pub struct MetadataBudget {
    remaining: usize,
}

/// A metadata charge would exceed the operation's remaining allowance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BudgetExceeded;

impl fmt::Display for BudgetExceeded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("metadata byte budget exhausted")
    }
}

impl std::error::Error for BudgetExceeded {}

impl MetadataBudget {
    /// Start one operation's explicitly selected metadata allowance.
    pub fn new(bytes: usize) -> Self {
        Self { remaining: bytes }
    }

    /// Maximum decoded bytes the next read may admit.
    pub fn remaining(&self) -> usize {
        self.remaining
    }

    /// Debit decoded metadata without permitting underflow or partial spending.
    ///
    /// # Errors
    ///
    /// Returns [`BudgetExceeded`] without changing the balance if insufficient.
    pub fn charge(&mut self, bytes: usize) -> Result<(), BudgetExceeded> {
        self.remaining = self.remaining.checked_sub(bytes).ok_or(BudgetExceeded)?;
        Ok(())
    }
}
