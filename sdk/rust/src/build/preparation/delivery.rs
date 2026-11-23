//! Bounded response delivery must preserve original reconciliation coordinates.

use super::{LocalPreparationRecipe, LocalPreparationRunOptions, LocalPreparationRunReport};
use crate::{source, store, workspace::Key};
use serde::Serialize;
use std::path::Path;

/// Complete native outcome or original coordinates when that outcome cannot be encoded.
/// This private operator evidence is neither current journal state nor permission to retry.
#[derive(Serialize, schemars::JsonSchema)]
#[serde(transparent)]
pub struct LocalPreparationDelivery<'a> {
    view: Delivery<'a>,
}

#[derive(Serialize, schemars::JsonSchema)]
#[serde(untagged)]
enum Delivery<'a> {
    Native(LocalPreparationRunReport<'a>),
    Reconciliation(Reconciliation<'a>),
}

#[derive(Serialize, schemars::JsonSchema)]
#[serde(tag = "status", rename_all = "kebab-case")]
enum Reconciliation<'a> {
    ReconciliationRequired {
        journal: &'a Path,
        input_expected_header: store::JournalHeader,
        coordinator: Option<&'a store::coordinator::LocalCoordinatorSelection>,
        preparations: Vec<Coordinate<'a>>,
        message: &'static str,
    },
}

#[derive(Serialize, schemars::JsonSchema)]
struct Coordinate<'a> {
    site: &'a Key,
    preparation_id: source::Sha256Digest,
}

impl LocalPreparationRecipe {
    /// Reserve a complete fallback before effects, execute once, then encode the native report.
    /// The encoder receives the error classification and must only encode, not perform effects.
    /// If native-report encoding fails, return the reserved original coordinates unchanged.
    /// Cancellation or loss after return still requires independent native reconciliation.
    ///
    /// # Errors
    /// An encoder error while reserving the fallback prevents all native execution.
    /// Native failures are delivered as reports, not discarded as transport errors.
    pub fn execute_with_delivery<T, E>(
        &self,
        options: LocalPreparationRunOptions,
        mut encode: impl FnMut(&LocalPreparationDelivery<'_>, bool) -> Result<T, E>,
    ) -> Result<T, E> {
        let fallback = LocalPreparationDelivery {
            view: Delivery::Reconciliation(Reconciliation::ReconciliationRequired {
                journal: self.journal(),
                input_expected_header: self.expected_header(),
                coordinator: self.coordinator(),
                preparations: self
                    .preparations()
                    .map(|(site, preparation_id)| Coordinate {
                        site,
                        preparation_id,
                    })
                    .collect(),
                message: "Complete local preparation response unavailable; effects may have occurred, including selected coordinator anchor changes. Inspect the exact native journal, preparation IDs and selected anchor before deciding further work. A checker may have run even without a preparation record. Do not retry, recreate or infer rollback. The input header and coordinator selection are not current observations.",
            }),
        };
        let fallback = encode(&fallback, true)?;
        let result = self.execute(options);
        let report = LocalPreparationDelivery {
            view: Delivery::Native(LocalPreparationRunReport::new(&result)),
        };
        Ok(encode(&report, result.is_err()).unwrap_or(fallback))
    }
}
