//! Hold configured-output history inside the existing application writer lifetime.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use gazette_core::{
    source::{Limits, Sha256Digest},
    store::{Error, JournalHeader, MaterializationInspection, ReadOptions},
};
use gazette_ports::{
    destination::{OutputReader, OutputWriter},
    store::{MaterializationReader, MaterializationWriter},
};

use super::Writer;
use crate::build::{
    MaterializationError, MaterializationOptions, MaterializationPlan, MaterializationResult,
    MaterializationSetPlan, MaterializationSetPlanError, MaterializationSetWriteError,
    PublicationRender, RenderedPublicationSet,
};

/// Inspect exact historical output ownership without acquiring a writer or reading the output.
/// The selected root is a historical coordinate, not an asserted filesystem capability.
///
/// # Errors
/// Refuses invalid bounds, changed root selection or an independently expected header mismatch.
pub fn inspect_materialization(
    reader: &impl MaterializationReader,
    path: &Path,
    root: &Path,
    expected: Option<&JournalHeader>,
    options: ReadOptions,
) -> Result<MaterializationInspection, Error> {
    options.validate()?;
    let inspected = reader.inspect_materialization(path, root, options)?;
    if inspected.frontier.root != root {
        return Err(Error::MaterializationVerification);
    }
    if expected.is_some_and(|header| *header != inspected.journal.header) {
        return Err(Error::JournalMismatch);
    }
    Ok(inspected)
}

impl<W: MaterializationWriter> Writer<W> {
    /// Actual held journal and coordinator roots that local output setup must protect.
    /// These come from the native writer, never from an output recipe's claimed paths.
    ///
    /// # Errors
    /// Preserves unavailable or invalidated native custody errors.
    pub fn materialization_protected_roots(&mut self) -> Result<Vec<PathBuf>, Error> {
        self.0.materialization_roots()
    }

    /// Plan a complete retained documentation repository through the same held history owner.
    /// This never substitutes the Theme destination or reopens a generator.
    ///
    /// # Errors
    /// Refuses unsafe configured roots, changed ownership, pending history and bounds.
    pub fn plan_documentation_materialization<'a>(
        &mut self,
        assembly: &'a crate::build::DocumentationAssembly<'_>,
        destination: &impl OutputReader,
        protected: &[PathBuf],
        limits: Limits,
        evidence_bytes: usize,
    ) -> Result<MaterializationPlan<'a>, MaterializationError> {
        assembly.plan_materialization(&mut self.0, destination, protected, limits, evidence_bytes)
    }
    /// Preflight every selected output through the shared complete-set owner.
    ///
    /// # Errors
    /// Refuses incomplete renders, unsafe destinations, pending history and aggregate bounds.
    pub fn plan_materializations<'a>(
        &mut self,
        rendered: &'a RenderedPublicationSet<'_>,
        destination: &(impl OutputReader + gazette_ports::destination::RootSeparation),
        limits: Limits,
        evidence_bytes: usize,
    ) -> Result<MaterializationSetPlan<'a>, MaterializationSetPlanError> {
        rendered.plan_materializations(&mut self.0, destination, limits, evidence_bytes)
    }

    /// Consume the exact complete plan with no interface-owned per-site write loop.
    ///
    /// # Errors
    /// Preserves the whole selection, completed prefix and failed site's original evidence.
    pub fn materialize_set(
        &mut self,
        plan: MaterializationSetPlan<'_>,
        destination: &impl OutputWriter,
        ids: &BTreeMap<gazette_core::publication::Key, Sha256Digest>,
        options: MaterializationOptions,
    ) -> Result<
        BTreeMap<gazette_core::publication::Key, MaterializationResult>,
        MaterializationSetWriteError,
    > {
        plan.materialize(&mut self.0, destination, ids, options)
    }

    /// Inspect exact stored output history without filesystem effects or reconciliation.
    ///
    /// # Errors
    /// Preserves stale, unsupported, corrupt or over-limit store failures.
    pub fn inspect_materialization(
        &mut self,
        root: &Path,
    ) -> Result<MaterializationInspection, Error> {
        self.0.inspect_materialization(root)
    }

    /// Bind an original local output plan to independently inspected journal ownership.
    /// Actual held journal roots are protected by the native history provider.
    ///
    /// # Errors
    /// Refuses unresolved history, changed ownership, unsafe roots and exhausted bounds.
    pub fn plan_materialization<'a>(
        &mut self,
        rendered: &'a PublicationRender<'_>,
        destination: &impl OutputReader,
        protected: &[PathBuf],
        limits: Limits,
        evidence_bytes: usize,
    ) -> Result<MaterializationPlan<'a>, MaterializationError> {
        rendered.plan_materialization(&mut self.0, destination, protected, limits, evidence_bytes)
    }

    /// Persist, write/read back and complete one original configured-output operation.
    /// Historical replay never invokes the destination writer again.
    ///
    /// # Errors
    /// Preserves pending history, native partial effects and unverified completion.
    pub fn materialize(
        &mut self,
        plan: MaterializationPlan<'_>,
        destination: &impl OutputWriter,
        id: Sha256Digest,
        options: MaterializationOptions,
    ) -> Result<MaterializationResult, MaterializationError> {
        plan.materialize(&mut self.0, destination, id, options)
    }
}
