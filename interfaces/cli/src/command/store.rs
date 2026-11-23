//! Store lifecycle commands shared by CLI and MCP.

use std::path::PathBuf;

use gazette_sdk::{
    source::Sha256Digest,
    store::{self, BackupVerifyOptions, ReadOptions},
};

use super::{Command, Kernel};

pub use store::LifecycleProtectedRoots;

/// Observe one existing store without initializing or migrating it.
#[derive(Debug)]
pub struct Observe {
    /// Existing native database path.
    pub path: PathBuf,
    /// Explicit native read bounds and coordination permission.
    pub options: ReadOptions,
}

impl Command for Observe {
    type Outcome = store::Observation;
    type Failure = store::Error;

    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        store::observe(&self.path, self.options)
    }
}

/// Inspect the supported Gazette journal at exact optional prior coordinates.
#[derive(Debug)]
pub struct InspectJournal {
    /// Existing native database path.
    pub path: PathBuf,
    /// Independently retained expected coordinates, or observation-only when absent.
    pub expected: Option<store::JournalHeader>,
    /// Explicit native read bounds and coordination permission.
    pub options: ReadOptions,
}

impl Command for InspectJournal {
    type Outcome = store::JournalInspection;
    type Failure = store::Error;

    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        store::inspect_journal(&self.path, self.expected.as_ref(), self.options)
    }
}

/// Inspect and independently verify the complete private registry projection.
#[derive(Debug)]
pub struct InspectRegistry {
    /// Existing native database path.
    pub path: PathBuf,
    /// Independently retained expected coordinates, or observation-only when absent.
    pub expected: Option<store::JournalHeader>,
    /// Explicit native read bounds and coordination permission.
    pub options: ReadOptions,
}

impl Command for InspectRegistry {
    type Outcome = store::RegistryInspection;
    type Failure = store::Error;

    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        store::inspect_registry(&self.path, self.expected.as_ref(), self.options)
    }
}

/// Inspect durable configured-output history without acquiring a writer or sampling output.
#[derive(Debug)]
pub struct InspectMaterialization {
    /// Existing private database file; no initialization or migration.
    pub path: PathBuf,
    /// Exact historical canonical destination coordinate, not a write capability.
    pub root: PathBuf,
    /// Independently retained coordinates, or observation-only when absent.
    pub expected: Option<store::JournalHeader>,
    /// Native database read bounds and coordination-file permission.
    pub options: ReadOptions,
}

impl Command for InspectMaterialization {
    type Outcome = store::MaterializationInspection;
    type Failure = store::Error;

    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        store::inspect_materialization(&self.path, &self.root, self.expected.as_ref(), self.options)
    }
}

/// Initialize one fresh private Gazette store.
#[derive(Debug)]
pub struct Initialize {
    /// Existing empty private directory.
    pub directory: PathBuf,
    /// Explicit creation, coordination and output bounds.
    pub options: store::InitializeOptions,
}

impl Command for Initialize {
    type Outcome = store::InitializationReceipt;
    type Failure = store::OutputError;

    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        store::initialize(&self.directory, self.options)
    }
}

/// Create one fresh private backup of an existing store.
#[derive(Debug)]
pub struct Backup {
    /// Existing native database path.
    pub source: PathBuf,
    /// Existing private parent for one fresh backup directory.
    pub parent: PathBuf,
    /// Complete caller-selected roots protected from output placement.
    pub protected: LifecycleProtectedRoots,
    /// Explicit read, creation and output bounds.
    pub options: store::BackupOptions,
}

impl Command for Backup {
    type Outcome = store::BackupReceipt;
    type Failure = store::OutputError;

    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        store::backup(store::BackupRequest {
            source: &self.source,
            output_parent: &self.parent,
            protected: self.protected,
            options: self.options,
        })
    }
}

/// Verify one retained backup against its independent original pin.
#[derive(Debug)]
pub struct VerifyBackup {
    /// Existing private backup directory.
    pub directory: PathBuf,
    /// Independently retained manifest identity.
    pub manifest_sha256: Sha256Digest,
    /// Explicit native read and manifest bounds.
    pub options: BackupVerifyOptions,
}

impl Command for VerifyBackup {
    type Outcome = store::BackupInspection;
    type Failure = store::Error;

    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        store::verify_backup(&self.directory, self.manifest_sha256, self.options)
    }
}

/// Prepare one fresh detached recovery copy without replacing a live store.
#[derive(Debug)]
pub struct PrepareRestore {
    /// Independently pinned backup directory.
    pub backup: PathBuf,
    /// Independently retained backup manifest identity.
    pub manifest_sha256: Sha256Digest,
    /// Existing private parent for one fresh detached copy.
    pub parent: PathBuf,
    /// Complete caller-selected roots protected from output placement.
    pub protected: LifecycleProtectedRoots,
    /// Explicit verification, preparation and output bounds.
    pub options: store::RestoreOptions,
}

impl Command for PrepareRestore {
    type Outcome = store::RestoreReceipt;
    type Failure = store::OutputError;

    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        store::prepare_restore(store::PrepareRestoreRequest {
            backup: &self.backup,
            manifest_sha256: self.manifest_sha256,
            output_parent: &self.parent,
            protected: self.protected,
            options: self.options,
        })
    }
}
