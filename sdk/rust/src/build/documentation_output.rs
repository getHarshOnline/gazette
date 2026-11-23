//! Bounded documentation-repository operation, reports and exact delivery custody.

use super::{
    AcquiredInputs, CompositionInspection, DocumentationArtifactEvidence, DocumentationAssembly,
    DocumentationError, DocumentationGenerationCause, DocumentationGenerationError,
    DocumentationGenerationOptions, DocumentationJob, DocumentationManualEvidence,
    PreparationError, PreparationFailure, PreparationOptions, PreparationRequest,
    prepare_documentation, stage_documentation,
};
use crate::release_commit::journal_output::RecoveryPin;
use crate::{
    render::LimitedWriter,
    source::{Content, EntryPath, Sha256Digest, TreeIdentity},
    store,
};
use gazette_core::{
    documentation::{Selection, generator},
    process, stage,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize, Serializer, ser::SerializeSeq};
use std::{
    fmt,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

mod custody;
mod operation;
mod preparation;
mod report;

pub use custody::{Bounds, ContractError, Custody, Encoded, Reservation};
pub use operation::{
    AcquiredOperation, BeforeError, Execution, GenerationSelection, Operation, RetentionSelection,
};
pub use preparation::PreparationSelection;
pub use report::{PreparedReport, Report, Status, schema};
