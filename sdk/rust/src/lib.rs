//! Gazette's public Rust API: the one place where native adapters are chosen and
//! composed over the shared application behavior.
//!
//! This crate builds on `gazette-application`, `gazette-compiler`, `gazette-core`,
//! `gazette-ports` and every adapter; `gazette-cli`, `gazette-lsp` and the Node addon
//! build on it. Native adapter selection lives here, not in individual interfaces.
//! Observing a source grants no admission, certification, publisher permission or
//! release.
//!
//! The default build is native-adapter-free. `authoring` selects only the Markdown
//! implementation required by supplied-buffer analysis. `native-configuration`
//! adds confined, explicitly selected local reads. `native-local` adds the complete
//! local application composition used by the CLI, MCP and broad Node SDK. Features
//! select implementations; every operation still enforces its own explicit
//! permissions, pins and effect boundaries.

pub use gazette_compiler::{css, html, javascript};
#[cfg(feature = "authoring")]
pub mod markdown;
pub use gazette_compiler::authored;
pub use gazette_compiler::notebook;
pub use gazette_compiler::theme;
#[cfg(feature = "native-local")]
pub mod release;
pub use gazette_compiler::toml;
pub use gazette_compiler::xml;
#[cfg(feature = "authoring")]
pub mod corpus;
#[cfg(feature = "authoring")]
pub mod document;
pub use gazette_core::effect;
pub use gazette_core::json;
pub use gazette_core::process;
pub use gazette_core::publication;
pub use gazette_core::render;
pub use gazette_core::resource;
pub mod facts;
pub mod schema;

#[cfg(feature = "authoring")]
pub mod authoring;
#[cfg(feature = "native-local")]
pub mod build;
#[cfg(feature = "native-configuration")]
pub mod configuration;
#[cfg(feature = "native-local")]
pub mod delivery;
#[cfg(feature = "native-local")]
pub mod font;
#[cfg(feature = "native-local")]
pub mod github;
#[cfg(feature = "native-local")]
mod http;
#[cfg(feature = "native-local")]
pub mod preview;
#[cfg(feature = "native-local")]
pub mod production_preview;
#[cfg(feature = "native-local")]
pub mod raster;
#[cfg(feature = "native-local")]
pub mod release_commit;
#[cfg(feature = "native-local")]
pub mod store;
pub mod workspace;

/// Source inspection, acquisition and fresh staging through selected local adapters.
#[cfg(feature = "native-local")]
pub mod source {
    pub mod input;
    use gazette_adapter_fs::FileSystem;
    use gazette_adapter_git::Git;
    use std::path::Path;

    pub use gazette_core::source::{
        CapturedSource, CommitId, Content, Entry, EntryPath, Error, InvalidCommitId,
        InvalidSha256Digest, InvalidSubdirectory, Inventory, InventoryBuilder, Limits, Location,
        Precision, RevisionLimits, Sha256Digest, Span, Subdirectory, TreeIdentity,
    };

    pub use gazette_application::source::{
        AcquiredInput, AcquiredSource, CheckedDestination, GENERATED_TREE_MANIFEST,
        GeneratedSource, GeneratedTreeBinding, GeneratedTreeInspection, GeneratedTreeManifest,
        PlacementIdentity, PlacementRequest, PreparationError, PreparationLimits,
        PreparedPlacement, PreparedSourceSet, SourceSelection, StageContext,
        VerifiedMaterialization, VerifiedSource, VerifiedSubmodule,
    };
    pub use gazette_core::destination::Error as DestinationError;
    pub use gazette_core::stage::RetainedStage;
    pub use gazette_core::stage::{Error as StageError, Location as StageLocation};

    /// A newly created private stage whose exact bytes passed native read-back.
    ///
    /// This SDK handle keeps cleanup ownership without exposing the selected
    /// filesystem adapter. Keep it alive while consuming the stage, then call
    /// [`StagedTree::close`] or [`StagedTree::retain`] explicitly.
    #[must_use = "keep the stage handle alive, then explicitly close or retain it"]
    pub struct StagedTree(gazette_adapter_fs::StagedTree);

    impl StagedTree {
        pub(crate) fn from_native(stage: gazette_adapter_fs::StagedTree) -> Self {
            Self(stage)
        }

        /// Ephemeral native path, valid only while this handle remains valid.
        pub fn path(&self) -> &Path {
            self.0.path()
        }

        /// Exact complete tree observed after staging.
        pub fn inventory(&self) -> &Inventory {
            self.0.inventory()
        }

        /// Synchronize and retain this exact stage without replacing another tree.
        ///
        /// # Errors
        /// Preserves unresolved residue when identity, privacy or synchronization fails.
        pub fn retain(self) -> Result<RetainedStage, StageError> {
            self.0.retain()
        }

        /// Remove only this unchanged stage, reporting rather than hiding residue.
        ///
        /// # Errors
        /// Leaves and identifies residue when replacement, change or cleanup failure is observed.
        pub fn close(self) -> Result<(), StageError> {
            self.0.close()
        }
    }

    impl std::fmt::Debug for StagedTree {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter
                .debug_struct("StagedTree")
                .field("path", &self.path())
                .field("identity", &self.inventory().identity())
                .finish()
        }
    }

    /// Capture the whole selected tree against independently retained structural evidence.
    ///
    /// Returns immutable in-memory bytes for parsing without a second native read.
    /// A matching pin is not publication admission, revision verification, persistent
    /// filesystem immutability or permission to clean up the source.
    ///
    /// # Errors
    /// Rejects unsafe/changing or over-limit roots and any complete-tree mismatch.
    pub fn capture_pinned(
        root: &Path,
        expected: TreeIdentity,
        limits: Limits,
    ) -> Result<CapturedSource, Error> {
        gazette_application::source::capture_pinned(&FileSystem, root, expected, limits)
    }

    /// Check staging's complete composition and native output parent without writes.
    ///
    /// Authored content and prepared projects may overlap as inputs. The actual
    /// staging parent must be disjoint from them, including whole gitlink parents.
    /// Existing parent children are not scanned or adopted. This returns a logical
    /// inventory, not a write lease or certification; `stage` checks again.
    ///
    /// # Errors
    ///
    /// Rejects unsafe sources/roots, invalid context, collisions and exhausted limits.
    pub fn check_stage(
        prepared: &PreparedSourceSet,
        context: &StageContext,
        authored_root: &Path,
        staging_parent: &Path,
        limits: Limits,
    ) -> Result<Inventory, StageError> {
        gazette_application::source::check_stage(
            &FileSystem,
            &FileSystem,
            prepared,
            context,
            authored_root,
            staging_parent,
            limits,
        )
    }

    /// Create a fresh private stage from captured authored content and prepared sources.
    ///
    /// Explicit context must bind every placement. Generated metadata participates
    /// in the same collision, byte-limit and read-back checks as source content.
    /// This writes only a newly created child of the explicit staging parent.
    /// `StageLocation::Named` selects exactly one child and refuses collisions;
    /// `StageLocation::Fresh` chooses a new private name. Neither adopts residue.
    /// Keep the returned guard alive while consuming it; use `close` to observe
    /// cleanup failures or `retain` to synchronize and keep the exact fresh tree.
    /// Existing-tree replacement and publication are not effects.
    ///
    /// # Errors
    ///
    /// Rejects unsafe sources/roots, collisions, limits, failed writes/read-back
    /// and incomplete cleanup, with any untrusted residue explicitly identified.
    pub fn stage(
        prepared: &PreparedSourceSet,
        context: &StageContext,
        authored_root: &Path,
        location: StageLocation<'_>,
        limits: Limits,
    ) -> Result<StagedTree, StageError> {
        gazette_application::source::stage(
            &FileSystem,
            &FileSystem,
            prepared,
            context,
            authored_root,
            location,
            limits,
        )
        .map(StagedTree::from_native)
    }
    pub use gazette_core::source::{InvalidTargetPath, TargetPath};

    /// Inspect a disjoint existing content root and check its complete proposed tree.
    ///
    /// Requires the same explicit placement context and generated metadata as staging.
    /// Performs no writes. Success is a bounded observation, not durable writer
    /// authority; later effects require fresh checks. Canonical-caseless collisions
    /// fail conservatively without changing path spelling.
    ///
    /// # Errors
    ///
    /// Rejects unsafe roots, source overlap, existing targets, ambiguous names,
    /// differing file/directory roles, concurrent changes or exhausted bounds.
    pub fn check_destination(
        prepared: &PreparedSourceSet,
        context: &StageContext,
        content_root: &Path,
        limits: Limits,
    ) -> Result<CheckedDestination, DestinationError> {
        gazette_application::source::check_destination(
            &FileSystem,
            prepared,
            context,
            content_root,
            limits,
        )
    }

    /// Capture generated documentation against a retained manifest pin and exact explicit binding.
    ///
    /// Returned bytes exclude generated metadata and can feed compilation without rereading
    /// live files. This grants no source, publisher, replacement or cleanup authority.
    ///
    /// # Errors
    /// Refuses malformed/unsupported evidence, mismatched pins/bindings/bytes, unsafe paths or limits.
    pub fn verify_generated_tree(
        root: &Path,
        expected: &GeneratedTreeBinding,
        manifest_sha256: Sha256Digest,
        limits: Limits,
    ) -> Result<VerifiedMaterialization, Error> {
        gazette_application::source::verify_generated_tree(
            &FileSystem,
            root,
            expected,
            manifest_sha256,
            limits,
        )
    }

    /// Observe a selected local source without writes, network or database access.
    ///
    /// Root-relative paths are sorted. An optional subdirectory is resolved
    /// component-by-component beneath the opened root without following links.
    /// Hidden entries count; this observation is not an atomic snapshot.
    ///
    /// # Errors
    ///
    /// Returns the shared source failure for unreadable, unsupported, changing
    /// or over-limit input, with no partial inventory.
    pub fn inspect(
        root: &Path,
        subdirectory: Option<&Subdirectory>,
        limits: Limits,
    ) -> Result<Inventory, Error> {
        gazette_application::source::inspect(&FileSystem, root, subdirectory, limits)
    }

    /// Verify observed local source bytes against an exact checked-out commit.
    ///
    /// Reads local Git objects only. Does not fetch, checkout, execute filters,
    /// mutate the source or establish publication admission.
    ///
    /// # Errors
    ///
    /// Fails for unsafe/unreadable sources, wrong pins, unsupported objects,
    /// differing source bytes or exceeded source/metadata ceilings.
    pub fn verify(
        root: &Path,
        subdirectory: &Subdirectory,
        commit: &CommitId,
        limits: RevisionLimits,
    ) -> Result<VerifiedSource, Error> {
        gazette_application::source::verify(&FileSystem, &Git, root, subdirectory, commit, limits)
    }

    /// Verify child source using the parent's gitlink, never a sibling clone.
    ///
    /// All local reads remain beneath the explicitly selected parent source;
    /// missing children are not fetched or initialized.
    ///
    /// # Errors
    ///
    /// Fails on unsafe paths, wrong parent/child pins, differing source bytes,
    /// unavailable objects or exhausted source/metadata ceilings.
    pub fn verify_submodule(
        parent: &Path,
        parent_commit: &CommitId,
        submodule_path: &Subdirectory,
        subdirectory: &Subdirectory,
        limits: RevisionLimits,
    ) -> Result<VerifiedSubmodule, Error> {
        gazette_application::source::verify_submodule(
            &FileSystem,
            &Git,
            parent,
            parent_commit,
            submodule_path,
            subdirectory,
            limits,
        )
    }

    /// Retain an exact revision's source bytes for later compilation without rereads.
    ///
    /// This allocates bounded payload memory but writes no source or staging files.
    ///
    /// # Errors
    ///
    /// Rejects unsafe, changing, mismatched or over-limit input; never returns a
    /// partial capture or grants publication permission.
    pub fn acquire(
        root: &Path,
        subdirectory: &Subdirectory,
        commit: &CommitId,
        limits: RevisionLimits,
    ) -> Result<AcquiredSource<VerifiedSource>, Error> {
        gazette_application::source::acquire(&FileSystem, &Git, root, subdirectory, commit, limits)
    }

    /// Retain child documentation selected by its exact parent gitlink.
    ///
    /// # Errors
    ///
    /// Propagates capture and parent/child verification failures without fetching,
    /// initializing, modifying or substituting the selected child.
    pub fn acquire_submodule(
        parent: &Path,
        parent_commit: &CommitId,
        submodule_path: &Subdirectory,
        subdirectory: &Subdirectory,
        limits: RevisionLimits,
    ) -> Result<AcquiredSource<VerifiedSubmodule>, Error> {
        gazette_application::source::acquire_submodule(
            &FileSystem,
            &Git,
            parent,
            parent_commit,
            submodule_path,
            subdirectory,
            limits,
        )
    }

    /// Preflight logical placements and acquire their complete bounded source set.
    ///
    /// This performs source reads only, not destination admission or staging writes.
    ///
    /// # Errors
    ///
    /// Rejects overlap, source-count/aggregate limits or any selected input failure;
    /// no partial source set or fallback selection is returned.
    pub fn prepare(
        requests: Vec<PlacementRequest>,
        limits: PreparationLimits,
    ) -> Result<PreparedSourceSet, PreparationError> {
        gazette_application::source::prepare(&FileSystem, &Git, requests, limits)
    }
}

/// Acquisition-adapter-independent source identities and coordinates used by portable
/// compiler and authoring projections. Native acquisition is available only
/// through the `native-local` feature.
#[cfg(not(feature = "native-local"))]
pub mod source {
    pub use gazette_core::source::{
        CapturedSource, CommitId, Content, Entry, EntryPath, Error, InvalidCommitId,
        InvalidSha256Digest, InvalidSubdirectory, InvalidTargetPath, Inventory, Limits, Location,
        Precision, RevisionLimits, Sha256Digest, Span, Subdirectory, TargetPath, TreeIdentity,
    };
}
