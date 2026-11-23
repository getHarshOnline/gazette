//! Complete-set configured output through the existing ordinary writer custody.

use std::{collections::BTreeMap, fmt};

use gazette_sdk::{build, source, store, workspace::Key};

use super::{Command, Failure, Kernel, Native, Selection, lock};
use crate::command::publication;

/// Render and materialize one complete selected set under an already-held writer.
/// This command neither acquires another lifetime nor closes, retries or repairs it.
pub struct Materialize<R> {
    /// Existing opaque process-local ordinary writer handle.
    pub handle: String,
    /// Native permission-checked publication render request, consumed exactly once.
    pub render: publication::Render,
    /// Exact per-site operation coordinates; the shared application checks complete membership.
    pub ids: BTreeMap<Key, source::Sha256Digest>,
    /// Optional complete-set repository setup before the ordinary output planner.
    pub checkouts: Option<BTreeMap<Key, store::CheckoutSelection>>,
    /// Aggregate complete-set observed and proposed output ceilings.
    pub limits: source::Limits,
    /// Explicit filesystem/history permissions and aggregate evidence ceiling.
    pub options: build::MaterializationOptions,
    /// Reserve complete response custody under the registry lock, before any
    /// configuration, source, generator, journal append or output effect.
    pub reserve: R,
}

/// Exact native failure phase, without transport encoding or discarded prefix evidence.
#[derive(Debug)]
pub enum MaterializationFailure {
    /// Repository setup failure retains its completed metadata prefix and native residue.
    Checkout(store::CheckoutSetError),
    /// Original shared publication parsing, acquisition or rendering failure.
    Render(publication::Failure),
    /// Complete-set planning refusal before history append or output mutation.
    Plan(build::MaterializationSetPlanError),
    /// Original execution failure, including all completed sites and uncertain native effects.
    Write(build::MaterializationSetWriteError),
}

impl fmt::Display for MaterializationFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Checkout(error) => fmt::Display::fmt(error, formatter),
            Self::Render(error) => fmt::Display::fmt(error, formatter),
            Self::Plan(error) => fmt::Display::fmt(error, formatter),
            Self::Write(error) => fmt::Display::fmt(error, formatter),
        }
    }
}

impl std::error::Error for MaterializationFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Checkout(error) => Some(error),
            Self::Render(error) => Some(error),
            Self::Plan(error) => Some(error),
            Self::Write(error) => Some(error),
        }
    }
}

/// Complete native result while the original ordinary writer remains held.
/// Retained custody is not a claim that a failed writer remains usable; explicit
/// inspect/close preserves the native lifetime's own refusal and read-back rules.
#[derive(Debug)]
pub struct MaterializationReport {
    /// Completed metadata setup, also retained when later output planning or writing fails.
    pub checkouts: BTreeMap<Key, gazette_sdk::release::OutputCheckoutReport>,
    /// Original opaque ordinary-store handle.
    pub session: String,
    /// True while this kernel still retains the actual native object, including on failure.
    pub active: bool,
    /// Original successfully acquired writer coordinates, not a refreshed authority token.
    pub acquired_header: Option<store::JournalHeader>,
    /// Full successful selection or unchanged failure with exact completed-prefix evidence.
    pub outcome: Result<BTreeMap<Key, build::MaterializationResult>, MaterializationFailure>,
}

impl<R, E> Command for Materialize<R>
where
    R: FnOnce(&str, &Selection, Option<store::JournalHeader>) -> Result<(), E>,
{
    type Outcome = MaterializationReport;
    type Failure = Failure<E>;

    fn execute(self, kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        let Self {
            handle,
            render,
            ids,
            checkouts,
            limits,
            options,
            reserve,
        } = self;
        let mut entries = lock(&kernel.store_session)?;
        let held = entries.get_mut(&handle).ok_or(Failure::Unknown)?;
        let writer = match &mut held.native {
            Native::Writer(writer) => writer,
            Native::Connection(_) => return Err(Failure::NotWriter),
        };
        reserve(&handle, &held.selection, held.acquired_header).map_err(Failure::Reservation)?;
        let mut prepared_checkouts = BTreeMap::new();
        let outcome = render
            .parse()
            .map_err(MaterializationFailure::Render)
            .and_then(|parsed| {
                parsed
                    .consume_rendered(|rendered| {
                        if let Some(checkouts) = &checkouts {
                            if !options.allow_write || !options.history.allow_record {
                                return Err(MaterializationFailure::Checkout(
                                    store::CheckoutSetError {
                                        requested: rendered.sites().keys().cloned().collect(),
                                        completed: BTreeMap::new(),
                                        failed: None,
                                        reason: "materialization_permission",
                                        native: None,
                                        acquired: None,
                                    },
                                ));
                            }
                            prepared_checkouts = writer
                                .prepare_configured_checkouts(
                                    rendered,
                                    checkouts,
                                    &ids,
                                    limits,
                                    options.evidence_bytes,
                                )
                                .map_err(MaterializationFailure::Checkout)?;
                        }
                        let plan = writer
                            .plan_materializations(rendered, limits, options.evidence_bytes)
                            .map_err(MaterializationFailure::Plan)?;
                        writer
                            .materialize_set(plan, &ids, options)
                            .map_err(MaterializationFailure::Write)
                    })
                    .map_err(MaterializationFailure::Render)
                    .and_then(|result| result)
            });
        Ok(MaterializationReport {
            checkouts: prepared_checkouts,
            session: handle,
            active: true,
            acquired_header: held.acquired_header,
            outcome,
        })
    }
}
