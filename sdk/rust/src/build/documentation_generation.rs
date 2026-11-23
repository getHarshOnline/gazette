//! Explicit existing documentation effects with receipts retained through publication delivery.

use super::{
    AcquiredPublicationSet, DocumentationCatalog, PublicationSetError, documentation_output,
};
use crate::{source, workspace::Key};
use serde::{Serialize, ser::SerializeStruct};
use std::{collections::BTreeMap, fmt, sync::Arc};

mod request;
mod run;
pub use request::DocumentationGenerationRequest;
pub(crate) use run::{GenerationLimits, generate, generate_acquired};

/// One original bounded native response; the bytes are never reconstructed from stage paths.
#[derive(Clone, schemars::JsonSchema)]
pub struct DocumentationReceipt {
    /// Configured documentation owner whose operation produced this response.
    pub site: Key,
    #[schemars(with = "serde_json::Value")]
    response: Arc<documentation_output::Encoded>,
    is_error: bool,
}
impl DocumentationReceipt {
    pub(crate) fn retained(site: Key, mut response: documentation_output::Encoded) -> Self {
        response.bytes = response.bytes.into_boxed_slice().into_vec();
        Self {
            site,
            is_error: response.is_error,
            response: Arc::new(response),
        }
    }
    /// Exact native JSON, including completed jobs and retained stages on failure.
    pub fn bytes(&self) -> &[u8] {
        &self.response.bytes
    }
    /// Compact/native failure is never promoted to successful publication evidence.
    pub fn is_error(&self) -> bool {
        self.is_error
    }
}
impl fmt::Debug for DocumentationReceipt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DocumentationReceipt")
            .field("site", &self.site)
            .field("bytes", &self.bytes().len())
            .field("is_error", &self.is_error())
            .finish()
    }
}
impl Serialize for DocumentationReceipt {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let raw: &serde_json::value::RawValue =
            serde_json::from_slice(self.bytes()).map_err(serde::ser::Error::custom)?;
        let mut object = serializer.serialize_struct("DocumentationReceipt", 3)?;
        object.serialize_field("site", &self.site)?;
        object.serialize_field("response", raw)?;
        object.serialize_field("is_error", &self.is_error())?;
        object.end()
    }
}

/// Pre-effect native refusal, or a later join failure with completed receipts retained separately.
#[derive(Debug)]
pub enum DocumentationPublicationFailure {
    /// Shared source, target or publication admission failed.
    Publication(PublicationSetError),
    /// Selected private stage parent overlaps a retained input or is unsafe.
    Separation(gazette_core::destination::Error),
    /// Explicit request or whole-set bounds are invalid.
    Selection(&'static str),
    /// Complete repeated staging observations and their caller-selected ceilings.
    Bounds {
        /// Observed jobs, entries and bytes, in that order.
        observed: (usize, usize, u64),
        /// Allowed jobs, entries and bytes, in that order.
        maximum: (usize, usize, u64),
    },
}
impl DocumentationPublicationFailure {
    /// Stable native failure classification.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Publication(error) => error.code(),
            Self::Separation(_) => "documentation_stage_separation",
            Self::Selection(_) => "documentation_generation_selection",
            Self::Bounds { .. } => "documentation_generation_bounds",
        }
    }
}
impl fmt::Display for DocumentationPublicationFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Publication(error) => error.fmt(f),
            Self::Separation(error) => error.fmt(f),
            Self::Selection(reason) => f.write_str(reason),
            Self::Bounds { observed, maximum } => write!(
                f,
                "documentation staging requires {} jobs, {} entries and {} bytes; allowed {} jobs, {} entries and {} bytes; no stages created",
                observed.0, observed.1, observed.2, maximum.0, maximum.1, maximum.2
            ),
        }
    }
}
impl std::error::Error for DocumentationPublicationFailure {}

/// Documentation acquisition and existing native delivery stay within one owning callback.
pub enum DocumentationPublicationExecution<'a> {
    /// Every configured catalog is installed; rendering may now use this exact acquisition.
    Ready {
        /// Original mutable publication acquisition, without a reread or alternate source graph.
        inputs: &'a mut AcquiredPublicationSet,
        /// Complete original native deliveries for newly generated documentation.
        receipts: &'a [DocumentationReceipt],
    },
    /// No successful subset is installed. Earlier effects remain explicitly accounted for.
    Refused {
        /// Completed native deliveries before this refusal, if any.
        receipts: &'a [DocumentationReceipt],
        /// Original owning failure.
        error: DocumentationPublicationFailure,
    },
    /// A native operation failed; exceptional Contract custody reaches this immediate consumer.
    Native {
        /// Exact configured owner of the current native operation.
        site: &'a Key,
        /// Completed earlier native deliveries.
        receipts: &'a [DocumentationReceipt],
        /// Original native execution, borrowed inside its existing operation lifetime.
        execution: documentation_output::Execution<'a>,
    },
}

pub(super) type Catalogs = BTreeMap<Key, Arc<DocumentationCatalog>>;

/// Join configured documentation through its existing native operation, retaining every receipt.
/// This explicit callback entry permits selected generation; the imported-only helper remains unchanged.
/// The caller must consume Native Contract inside this callback while its original custody is alive.
pub fn generate_publication_documentation<T>(
    inputs: &mut AcquiredPublicationSet,
    acquisition: source::PreparationLimits,
    output: source::Limits,
    request: Option<&DocumentationGenerationRequest>,
    preview_links: Option<&gazette_core::publication::routing::PreviewLinkBases>,
    consume: impl FnOnce(DocumentationPublicationExecution<'_>) -> T,
) -> T {
    generate(
        inputs,
        acquisition,
        output,
        &BTreeMap::new(),
        request,
        preview_links,
        consume,
    )
}

/// Immediate delivery during a local preview; exceptional custody never crosses a worker boundary.
pub enum DocumentationDelivery<'a> {
    /// Newly completed original native receipts, delivered before later rendering can fail.
    Complete {
        /// Complete original receipts for this attempt.
        receipts: &'a [DocumentationReceipt],
    },
    /// A non-native join or rendering preflight refused the attempt.
    Refused {
        /// Earlier completed native receipts.
        receipts: &'a [DocumentationReceipt],
        /// Original owning error.
        error: DocumentationPublicationFailure,
    },
    /// An unchanged failed documentation input reuses its original receipt without rerunning effects.
    ReplayedFailure {
        /// Original failed attempt, including its completed prefix and retained stages.
        receipts: &'a [DocumentationReceipt],
    },
    /// Original current native delivery, inside the operation's lifetime.
    Native {
        /// Configured documentation owner.
        site: &'a Key,
        /// Earlier completed receipts.
        receipts: &'a [DocumentationReceipt],
        /// Original native execution, including exceptional borrowed Contract.
        execution: documentation_output::Execution<'a>,
    },
}
