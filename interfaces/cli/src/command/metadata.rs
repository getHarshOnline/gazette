//! Stable interface discovery facts shared by CLI and protocol projections.
//!
//! This module describes how to find and close existing typed operations. It
//! does not grant permission, execute an operation, or redefine its native
//! input, output, or failure contract.

use std::fmt::Write as _;

/// A bounded capability neighborhood presented by every interface.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Capability {
    /// Supplied authoring buffers and source inspection.
    Authoring,
    /// Publication resolution, rendering and qualification.
    Build,
    /// Generated reference documentation.
    Documentation,
    /// Destination and served-output observation.
    Observation,
    /// Local preview planning and lifecycle.
    Preview,
    /// Restricted interrupted-effect recovery.
    Recovery,
    /// Preparation, authorization and release mechanics.
    Release,
    /// Local registry, journal and coordinator mechanics.
    Store,
    /// Workspace configuration and composition.
    Workspace,
}

impl Capability {
    /// Stable progressive-discovery order shared by protocol projections.
    #[must_use]
    pub const fn discovery_rank(self) -> u8 {
        match self {
            Self::Authoring => 0,
            Self::Workspace => 1,
            Self::Build => 2,
            Self::Preview => 3,
            Self::Documentation => 4,
            Self::Store => 5,
            Self::Release => 6,
            Self::Observation => 7,
            Self::Recovery => 8,
        }
    }

    /// Stable machine key used by interface discovery.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Authoring => "authoring",
            Self::Build => "build",
            Self::Documentation => "documentation",
            Self::Observation => "observation",
            Self::Preview => "preview",
            Self::Recovery => "recovery",
            Self::Release => "release",
            Self::Store => "store",
            Self::Workspace => "workspace",
        }
    }

    /// Human heading used by direct and protocol help.
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::Authoring => "Authoring",
            Self::Build => "Build",
            Self::Documentation => "Documentation",
            Self::Observation => "Observation",
            Self::Preview => "Preview",
            Self::Recovery => "Recovery",
            Self::Release => "Release",
            Self::Store => "Store",
            Self::Workspace => "Workspace",
        }
    }
}

/// Operator purpose, independent from the mechanism used to fulfil it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Intent {
    /// Read or explain retained state.
    Inspect,
    /// Request a bounded state transition.
    Act,
    /// Sample native or provider state.
    Observe,
    /// Check an independently stated expectation.
    Verify,
    /// Join evidence and recorded intent without replaying an effect.
    Reconcile,
    /// Continue or close an interrupted local lifecycle safely.
    Recover,
}

impl Intent {
    /// Stable operator-journey order within one capability neighborhood.
    #[must_use]
    pub const fn discovery_rank(self) -> u8 {
        match self {
            Self::Inspect => 0,
            Self::Act => 1,
            Self::Verify => 2,
            Self::Observe => 3,
            Self::Reconcile => 4,
            Self::Recover => 5,
        }
    }

    /// Stable machine key.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Inspect => "inspect",
            Self::Act => "act",
            Self::Observe => "observe",
            Self::Verify => "verify",
            Self::Reconcile => "reconcile",
            Self::Recover => "recover",
        }
    }
}

/// The widest class of effect an operation may request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Effect {
    /// Observation with no requested state transition.
    Read,
    /// A process-local, filesystem, Git-object or database effect may occur.
    Local,
    /// A selected provider, network or remote destination may be contacted.
    External,
}

impl Effect {
    /// Stable machine key. This class never substitutes for native permissions.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Local => "local",
            Self::External => "external",
        }
    }
}

/// An orthogonal native mechanism that an operation may exercise on its widest path.
///
/// These facets describe potential mechanics only. They neither grant a native
/// permission nor claim that every request exercises every listed mechanism.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum PotentialEffect {
    /// Mutable process-local session or handle state.
    MemoryState,
    /// Reads from explicitly selected local filesystem state.
    FilesystemRead,
    /// Creates or changes explicitly selected local filesystem state.
    FilesystemWrite,
    /// `SQLite` database work or coordination-file maintenance.
    Sqlite,
    /// Execution of an explicitly selected native subprocess.
    Process,
    /// Mutation of local or remote Git state, beyond reading Git data.
    Git,
    /// Opening, serving or closing a local listener.
    Listener,
    /// Actual network I/O, including remote observation or mutation.
    Network,
}

impl PotentialEffect {
    /// Stable machine key used by interface discovery and confirmation text.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::MemoryState => "memory-state",
            Self::FilesystemRead => "filesystem-read",
            Self::FilesystemWrite => "filesystem-write",
            Self::Sqlite => "sqlite",
            Self::Process => "process",
            Self::Git => "git",
            Self::Listener => "listener",
            Self::Network => "network",
        }
    }
}

/// How prominently an interface should present an operation by default.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Prominence {
    /// Ordinary entry point shown in the concise capability guide.
    Common,
    /// Specialist lifecycle operation retained in complete discovery.
    Advanced,
}

impl Prominence {
    /// Stable progressive-discovery order.
    #[must_use]
    pub const fn discovery_rank(self) -> u8 {
        match self {
            Self::Common => 0,
            Self::Advanced => 1,
        }
    }

    /// Stable machine key.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Common => "common",
            Self::Advanced => "advanced",
        }
    }
}

/// A typed discovery edge between existing operations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Relation {
    /// Meaning of the edge.
    pub kind: RelationKind,
    /// Exact related MCP operation name.
    pub operation: &'static str,
}

/// Why another operation is relevant.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RelationKind {
    /// Read retained state produced by or relevant to this operation.
    Inspect,
    /// Check an independently retained expectation.
    Verify,
    /// Sample native or provider state after the operation.
    Observe,
    /// Join recorded intent and evidence without repeating the effect.
    Reconcile,
    /// Continue a bounded interrupted local lifecycle.
    Recover,
    /// Similar-looking operation with a materially different boundary.
    Contrast,
}

impl RelationKind {
    /// Stable machine key.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Inspect => "inspect",
            Self::Verify => "verify",
            Self::Observe => "observe",
            Self::Reconcile => "reconcile",
            Self::Recover => "recover",
            Self::Contrast => "contrast",
        }
    }
}

/// How an operation's immediate result connects to lifecycle closure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Closure {
    /// Inspection or verification has no required follow-up contract.
    None,
    /// One or more typed relations identify the relevant follow-up operations.
    Related,
    /// A separately named operation owns the terminal result for this lifecycle.
    TerminalOwner,
    /// The operation's own complete result is terminal; no follow-up is prescribed.
    SelfTerminal,
}

impl Closure {
    /// Stable machine key.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Related => "related",
            Self::TerminalOwner => "terminal-owner",
            Self::SelfTerminal => "self-terminal",
        }
    }
}

/// One canonical application operation and its discovery metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Operation {
    /// Exact MCP operation name.
    pub name: &'static str,
    /// Stable capability neighborhood.
    pub capability: Capability,
    /// Operator purpose.
    pub intent: Intent,
    /// Widest possible effect class; native permissions remain authoritative.
    pub effect: Effect,
    /// Orthogonal mechanisms that the widest possible native path may exercise.
    pub potential_effects: &'static [PotentialEffect],
    /// Default discovery prominence.
    pub prominence: Prominence,
    /// Proven relationships to existing operations.
    pub relations: &'static [Relation],
    /// Operation that owns the terminal result for a held lifecycle.
    pub terminal_result_owner: Option<&'static str>,
    /// Explicit action-to-closure disposition.
    pub closure: Closure,
}

impl Operation {
    /// One deterministic progressive-discovery key without another catalog.
    #[must_use]
    pub const fn discovery_key(self) -> (u8, u8, u8, &'static str) {
        (
            self.prominence.discovery_rank(),
            self.capability.discovery_rank(),
            self.intent.discovery_rank(),
            self.name,
        )
    }
}

/// One material direct-CLI mode and the exact protocol operations it exposes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CliMode {
    /// Exact option or subcommand selecting this mode; `None` is the base mode.
    pub selector: Option<&'static str>,
    /// Complete existing operation set exposed by this mode.
    pub operations: &'static [&'static str],
}

impl CliMode {
    /// Canonical operation that names this material CLI mode for discovery.
    ///
    /// Remaining operations are supporting lifecycle projections. They retain
    /// complete interface coverage without promoting a specialist mode merely
    /// because it reuses a common inspector, poller or closer.
    #[must_use]
    pub fn entry_operation(&self) -> Option<&'static str> {
        self.operations.first().copied()
    }
}

/// One direct CLI family and the exact protocol operations behind its modes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CliFamily {
    /// Stable direct CLI spelling.
    pub command: &'static str,
    /// Complete material modes, including the base invocation.
    pub modes: &'static [CliMode],
}

impl CliFamily {
    /// Iterate every operation exposed by this family; callers may de-duplicate modes.
    pub fn operations(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.modes
            .iter()
            .flat_map(|mode| mode.operations.iter().copied())
    }
}

/// Exhaustive interface registry. Native command implementations still own behavior.
#[derive(Clone, Copy, Debug)]
pub struct Registry {
    /// Every MCP operation exactly once.
    pub operations: &'static [Operation],
    /// Every non-launcher direct CLI family exactly once.
    pub cli_families: &'static [CliFamily],
}

const NONE: &[Relation] = &[];
const ASSEMBLE_DOCS: &[Relation] = &[
    Relation {
        kind: RelationKind::Inspect,
        operation: "check_documentation",
    },
    Relation {
        kind: RelationKind::Verify,
        operation: "verify_generated_tree",
    },
];
const CHECK_DOCUMENTATION: &[Relation] = &[Relation {
    kind: RelationKind::Contrast,
    operation: "assemble_docs",
}];
const BACKUP_STORE: &[Relation] = &[Relation {
    kind: RelationKind::Verify,
    operation: "verify_backup",
}];
const CHECK_COMPOSITION: &[Relation] = &[
    Relation {
        kind: RelationKind::Inspect,
        operation: "inspect_composition",
    },
    Relation {
        kind: RelationKind::Verify,
        operation: "compile_templates",
    },
];
const CHECK_WORKSPACE: &[Relation] = &[Relation {
    kind: RelationKind::Contrast,
    operation: "load_workspace",
}];
const CHECKPOINT_RELEASE: &[Relation] = &[Relation {
    kind: RelationKind::Inspect,
    operation: "inspect_release_checkpoint",
}];
const INSPECT_JOURNAL_WORKTREE: &[Relation] = &[Relation {
    kind: RelationKind::Recover,
    operation: "recover_journal_worktree",
}];
const INSPECT_PUBLICATION: &[Relation] = &[Relation {
    kind: RelationKind::Contrast,
    operation: "review_publication",
}];
const INSPECT_STORE: &[Relation] = &[Relation {
    kind: RelationKind::Contrast,
    operation: "inspect_journal",
}];
const LOAD_WORKSPACE: &[Relation] = &[Relation {
    kind: RelationKind::Contrast,
    operation: "check_workspace",
}];
const PREPARE_RELEASE: &[Relation] = &[Relation {
    kind: RelationKind::Verify,
    operation: "verify_release_commit",
}];
const PREPARE_RESTORE: &[Relation] = &[Relation {
    kind: RelationKind::Recover,
    operation: "adopt_store_restore",
}];
const RECORD_OBSERVATION: &[Relation] = &[Relation {
    kind: RelationKind::Inspect,
    operation: "inspect_release_observation",
}];
const RECONCILE_REGISTRY: &[Relation] = &[Relation {
    kind: RelationKind::Inspect,
    operation: "inspect_registry",
}];
const RENDER_SITES: &[Relation] = &[
    Relation {
        kind: RelationKind::Verify,
        operation: "qualify_sites",
    },
    Relation {
        kind: RelationKind::Contrast,
        operation: "probe_rendered_site",
    },
];
const REVIEW_PUBLICATION: &[Relation] = &[Relation {
    kind: RelationKind::Contrast,
    operation: "inspect_publication",
}];
const VERIFY_PREPARED: &[Relation] = &[Relation {
    kind: RelationKind::Inspect,
    operation: "inspect_release_preparations",
}];
const OBSERVE_TARGETS: &[Relation] = &[
    Relation {
        kind: RelationKind::Contrast,
        operation: "observe_github_pages_build",
    },
    Relation {
        kind: RelationKind::Contrast,
        operation: "observe_delivery",
    },
];
const OPERATE_RELEASE: &[Relation] = &[
    Relation {
        kind: RelationKind::Observe,
        operation: "observe_release_targets",
    },
    Relation {
        kind: RelationKind::Reconcile,
        operation: "inspect_release_reconciliations",
    },
];
const OPERATE_RECOVERY: &[Relation] = &[
    Relation {
        kind: RelationKind::Observe,
        operation: "observe_release_targets",
    },
    Relation {
        kind: RelationKind::Reconcile,
        operation: "inspect_release_reconciliations",
    },
];

const NO_EFFECTS: &[PotentialEffect] = &[];
const MEMORY_STATE: &[PotentialEffect] = &[PotentialEffect::MemoryState];
const READ_FILES_MEMORY_STATE: &[PotentialEffect] = &[
    PotentialEffect::MemoryState,
    PotentialEffect::FilesystemRead,
];
const READ_FILES: &[PotentialEffect] = &[PotentialEffect::FilesystemRead];
const READ_FILES_PROCESS: &[PotentialEffect] =
    &[PotentialEffect::FilesystemRead, PotentialEffect::Process];
const READ_FILES_PROCESS_NETWORK: &[PotentialEffect] = &[
    PotentialEffect::FilesystemRead,
    PotentialEffect::Process,
    PotentialEffect::Network,
];
const READ_WRITE_FILES: &[PotentialEffect] = &[
    PotentialEffect::FilesystemRead,
    PotentialEffect::FilesystemWrite,
];
const READ_WRITE_FILES_PROCESS: &[PotentialEffect] = &[
    PotentialEffect::FilesystemRead,
    PotentialEffect::FilesystemWrite,
    PotentialEffect::Process,
];
const READ_SQLITE: &[PotentialEffect] = &[PotentialEffect::FilesystemRead, PotentialEffect::Sqlite];
const READ_WRITE_SQLITE: &[PotentialEffect] = &[
    PotentialEffect::FilesystemRead,
    PotentialEffect::FilesystemWrite,
    PotentialEffect::Sqlite,
];
const READ_WRITE_SQLITE_PROCESS: &[PotentialEffect] = &[
    PotentialEffect::FilesystemRead,
    PotentialEffect::FilesystemWrite,
    PotentialEffect::Sqlite,
    PotentialEffect::Process,
];
const MATERIALIZATION_EFFECTS: &[PotentialEffect] = &[
    PotentialEffect::MemoryState,
    PotentialEffect::FilesystemRead,
    PotentialEffect::FilesystemWrite,
    PotentialEffect::Sqlite,
    PotentialEffect::Process,
];
const MATERIALIZE_SITES: &[Relation] = &[
    Relation {
        kind: RelationKind::Inspect,
        operation: "inspect_materialization",
    },
    Relation {
        kind: RelationKind::Inspect,
        operation: "inspect_store_session",
    },
    Relation {
        kind: RelationKind::Contrast,
        operation: "stage_rendered_site",
    },
];
const MATERIALIZE_DOCS: &[Relation] = &[
    Relation {
        kind: RelationKind::Inspect,
        operation: "inspect_materialization",
    },
    Relation {
        kind: RelationKind::Inspect,
        operation: "inspect_store_session",
    },
    Relation {
        kind: RelationKind::Contrast,
        operation: "assemble_docs",
    },
];
const SQLITE_MEMORY_STATE: &[PotentialEffect] =
    &[PotentialEffect::MemoryState, PotentialEffect::Sqlite];
const READ_SQLITE_MEMORY_STATE: &[PotentialEffect] = &[
    PotentialEffect::MemoryState,
    PotentialEffect::FilesystemRead,
    PotentialEffect::Sqlite,
];
const READ_WRITE_SQLITE_MEMORY_STATE: &[PotentialEffect] = &[
    PotentialEffect::MemoryState,
    PotentialEffect::FilesystemRead,
    PotentialEffect::FilesystemWrite,
    PotentialEffect::Sqlite,
];
const READ_WRITE_GIT: &[PotentialEffect] = &[
    PotentialEffect::FilesystemRead,
    PotentialEffect::FilesystemWrite,
    PotentialEffect::Git,
];
const READ_WRITE_SQLITE_GIT: &[PotentialEffect] = &[
    PotentialEffect::FilesystemRead,
    PotentialEffect::FilesystemWrite,
    PotentialEffect::Sqlite,
    PotentialEffect::Git,
];
const READ_WRITE_SQLITE_PROCESS_GIT: &[PotentialEffect] = &[
    PotentialEffect::FilesystemRead,
    PotentialEffect::FilesystemWrite,
    PotentialEffect::Sqlite,
    PotentialEffect::Process,
    PotentialEffect::Git,
];
const READ_WRITE_SQLITE_PROCESS_NETWORK: &[PotentialEffect] = &[
    PotentialEffect::FilesystemRead,
    PotentialEffect::FilesystemWrite,
    PotentialEffect::Sqlite,
    PotentialEffect::Process,
    PotentialEffect::Network,
];
const READ_WRITE_SQLITE_PROCESS_GIT_NETWORK: &[PotentialEffect] = &[
    PotentialEffect::FilesystemRead,
    PotentialEffect::FilesystemWrite,
    PotentialEffect::Sqlite,
    PotentialEffect::Process,
    PotentialEffect::Git,
    PotentialEffect::Network,
];
const PREVIEW_START: &[PotentialEffect] = &[
    PotentialEffect::MemoryState,
    PotentialEffect::FilesystemRead,
    PotentialEffect::Process,
    PotentialEffect::Listener,
];
const PREVIEW_STOP: &[PotentialEffect] = &[PotentialEffect::MemoryState, PotentialEffect::Listener];

macro_rules! operation {
    ($name:literal, $capability:ident, $intent:ident, $effect:ident, $potential_effects:ident, $prominence:ident, $relations:ident, $owner:expr, $closure:ident) => {
        Operation {
            name: $name,
            capability: Capability::$capability,
            intent: Intent::$intent,
            effect: Effect::$effect,
            potential_effects: $potential_effects,
            prominence: Prominence::$prominence,
            relations: $relations,
            terminal_result_owner: $owner,
            closure: Closure::$closure,
        }
    };
}

const OPERATIONS: &[Operation] = &[
    operation!(
        "adopt_store_restore",
        Store,
        Recover,
        Local,
        READ_WRITE_SQLITE,
        Advanced,
        NONE,
        None,
        SelfTerminal
    ),
    operation!(
        "assemble_docs",
        Documentation,
        Act,
        Local,
        READ_WRITE_SQLITE_PROCESS,
        Common,
        ASSEMBLE_DOCS,
        None,
        Related
    ),
    operation!(
        "check_documentation",
        Documentation,
        Inspect,
        Read,
        NO_EFFECTS,
        Common,
        CHECK_DOCUMENTATION,
        None,
        Related
    ),
    operation!(
        "authoring_assistance",
        Authoring,
        Inspect,
        Read,
        NO_EFFECTS,
        Advanced,
        NONE,
        None,
        None
    ),
    operation!(
        "authoring_change_document",
        Authoring,
        Act,
        Local,
        MEMORY_STATE,
        Advanced,
        NONE,
        None,
        SelfTerminal
    ),
    operation!(
        "authoring_close_document",
        Authoring,
        Act,
        Local,
        MEMORY_STATE,
        Advanced,
        NONE,
        None,
        SelfTerminal
    ),
    operation!(
        "authoring_close_session",
        Authoring,
        Act,
        Local,
        MEMORY_STATE,
        Advanced,
        NONE,
        None,
        SelfTerminal
    ),
    operation!(
        "authoring_complete_heading",
        Authoring,
        Inspect,
        Read,
        NO_EFFECTS,
        Advanced,
        NONE,
        None,
        None
    ),
    operation!(
        "authoring_complete_reference",
        Authoring,
        Inspect,
        Read,
        NO_EFFECTS,
        Advanced,
        NONE,
        None,
        None
    ),
    operation!(
        "authoring_component_assistance",
        Authoring,
        Inspect,
        Read,
        NO_EFFECTS,
        Advanced,
        NONE,
        None,
        None
    ),
    operation!(
        "authoring_definition",
        Authoring,
        Inspect,
        Read,
        NO_EFFECTS,
        Advanced,
        NONE,
        None,
        None
    ),
    operation!(
        "authoring_diagnostics",
        Authoring,
        Inspect,
        Read,
        NO_EFFECTS,
        Advanced,
        NONE,
        None,
        None
    ),
    operation!(
        "authoring_document_symbols",
        Authoring,
        Inspect,
        Read,
        NO_EFFECTS,
        Advanced,
        NONE,
        None,
        None
    ),
    operation!(
        "authoring_hover",
        Authoring,
        Inspect,
        Read,
        NO_EFFECTS,
        Advanced,
        NONE,
        None,
        None
    ),
    operation!(
        "authoring_import_editor_context",
        Authoring,
        Act,
        Local,
        READ_FILES_MEMORY_STATE,
        Advanced,
        NONE,
        Some("authoring_close_session"),
        TerminalOwner
    ),
    operation!(
        "authoring_open_document",
        Authoring,
        Act,
        Local,
        MEMORY_STATE,
        Advanced,
        NONE,
        None,
        SelfTerminal
    ),
    operation!(
        "authoring_origin",
        Authoring,
        Inspect,
        Read,
        NO_EFFECTS,
        Advanced,
        NONE,
        None,
        None
    ),
    operation!(
        "authoring_page_package",
        Authoring,
        Inspect,
        Read,
        NO_EFFECTS,
        Advanced,
        NONE,
        None,
        None
    ),
    operation!(
        "authoring_references",
        Authoring,
        Inspect,
        Read,
        NO_EFFECTS,
        Advanced,
        NONE,
        None,
        None
    ),
    operation!(
        "authoring_review_repair",
        Authoring,
        Inspect,
        Read,
        NO_EFFECTS,
        Advanced,
        NONE,
        None,
        None
    ),
    operation!(
        "authoring_search_symbols",
        Authoring,
        Inspect,
        Read,
        NO_EFFECTS,
        Advanced,
        NONE,
        None,
        None
    ),
    operation!(
        "authoring_start",
        Authoring,
        Act,
        Local,
        MEMORY_STATE,
        Advanced,
        NONE,
        Some("authoring_close_session"),
        TerminalOwner
    ),
    operation!(
        "authoring_status",
        Authoring,
        Inspect,
        Read,
        NO_EFFECTS,
        Advanced,
        NONE,
        None,
        None
    ),
    operation!(
        "backup_store",
        Store,
        Act,
        Local,
        READ_WRITE_SQLITE,
        Advanced,
        BACKUP_STORE,
        None,
        Related
    ),
    operation!(
        "check_composition",
        Workspace,
        Verify,
        Read,
        READ_FILES,
        Advanced,
        CHECK_COMPOSITION,
        None,
        Related
    ),
    operation!(
        "check_stage",
        Build,
        Verify,
        Read,
        READ_FILES,
        Advanced,
        NONE,
        None,
        None
    ),
    operation!(
        "check_workspace",
        Workspace,
        Verify,
        Read,
        READ_FILES,
        Common,
        CHECK_WORKSPACE,
        None,
        Related
    ),
    operation!(
        "checkpoint_release_commit",
        Release,
        Act,
        Local,
        READ_SQLITE,
        Advanced,
        CHECKPOINT_RELEASE,
        None,
        Related
    ),
    operation!(
        "close_recovery_session",
        Recovery,
        Recover,
        Local,
        SQLITE_MEMORY_STATE,
        Advanced,
        NONE,
        None,
        SelfTerminal
    ),
    operation!(
        "close_release_session",
        Release,
        Act,
        Local,
        SQLITE_MEMORY_STATE,
        Advanced,
        NONE,
        None,
        SelfTerminal
    ),
    operation!(
        "close_store_session",
        Store,
        Act,
        Local,
        SQLITE_MEMORY_STATE,
        Advanced,
        NONE,
        None,
        SelfTerminal
    ),
    operation!(
        "compile_templates",
        Workspace,
        Verify,
        Read,
        READ_FILES,
        Advanced,
        NONE,
        None,
        None
    ),
    operation!(
        "enroll_local_coordinator",
        Store,
        Act,
        Local,
        READ_WRITE_SQLITE,
        Advanced,
        NONE,
        None,
        SelfTerminal
    ),
    operation!(
        "initialize_store",
        Store,
        Act,
        Local,
        READ_WRITE_SQLITE,
        Advanced,
        NONE,
        None,
        SelfTerminal
    ),
    operation!(
        "inspect_composition",
        Workspace,
        Inspect,
        Read,
        READ_FILES,
        Advanced,
        NONE,
        None,
        None
    ),
    operation!(
        "inspect_journal",
        Store,
        Inspect,
        Local,
        READ_SQLITE,
        Advanced,
        NONE,
        None,
        None
    ),
    operation!(
        "inspect_journal_worktree",
        Release,
        Inspect,
        Local,
        READ_SQLITE,
        Advanced,
        INSPECT_JOURNAL_WORKTREE,
        None,
        Related
    ),
    operation!(
        "inspect_local_coordinator",
        Store,
        Inspect,
        Local,
        READ_SQLITE,
        Advanced,
        NONE,
        None,
        None
    ),
    operation!(
        "inspect_markdown",
        Authoring,
        Inspect,
        Read,
        READ_FILES,
        Common,
        NONE,
        None,
        None
    ),
    operation!(
        "inspect_preview",
        Preview,
        Inspect,
        Read,
        NO_EFFECTS,
        Common,
        NONE,
        None,
        None
    ),
    operation!(
        "inspect_publication",
        Build,
        Inspect,
        Read,
        READ_FILES,
        Advanced,
        INSPECT_PUBLICATION,
        None,
        Related
    ),
    operation!(
        "inspect_materialization",
        Store,
        Inspect,
        Local,
        READ_SQLITE,
        Advanced,
        NONE,
        None,
        None
    ),
    operation!(
        "inspect_registry",
        Store,
        Inspect,
        Local,
        READ_SQLITE,
        Advanced,
        NONE,
        None,
        None
    ),
    operation!(
        "inspect_release_attempts",
        Store,
        Inspect,
        Local,
        READ_SQLITE,
        Advanced,
        NONE,
        None,
        None
    ),
    operation!(
        "inspect_release_authorization",
        Store,
        Inspect,
        Local,
        READ_SQLITE,
        Advanced,
        NONE,
        None,
        None
    ),
    operation!(
        "inspect_release_checkpoint",
        Release,
        Inspect,
        Local,
        READ_SQLITE,
        Advanced,
        NONE,
        None,
        None
    ),
    operation!(
        "inspect_release_consumption",
        Store,
        Inspect,
        Local,
        READ_SQLITE,
        Advanced,
        NONE,
        None,
        None
    ),
    operation!(
        "inspect_release_custody",
        Release,
        Inspect,
        Read,
        READ_FILES,
        Advanced,
        NONE,
        None,
        None
    ),
    operation!(
        "inspect_release_leases",
        Store,
        Inspect,
        Local,
        READ_SQLITE,
        Advanced,
        NONE,
        None,
        None
    ),
    operation!(
        "inspect_release_observation",
        Observation,
        Inspect,
        Local,
        READ_SQLITE,
        Advanced,
        NONE,
        None,
        None
    ),
    operation!(
        "inspect_release_preparations",
        Release,
        Inspect,
        Local,
        READ_SQLITE,
        Advanced,
        NONE,
        None,
        None
    ),
    operation!(
        "inspect_release_reconciliations",
        Store,
        Inspect,
        Local,
        READ_SQLITE,
        Advanced,
        NONE,
        None,
        None
    ),
    operation!(
        "inspect_site",
        Workspace,
        Inspect,
        Read,
        READ_FILES,
        Advanced,
        NONE,
        None,
        None
    ),
    operation!(
        "inspect_source",
        Authoring,
        Inspect,
        Read,
        READ_FILES,
        Common,
        NONE,
        None,
        None
    ),
    operation!(
        "inspect_store",
        Store,
        Inspect,
        Local,
        READ_SQLITE,
        Common,
        INSPECT_STORE,
        None,
        Related
    ),
    operation!(
        "inspect_store_session",
        Store,
        Inspect,
        Local,
        READ_SQLITE_MEMORY_STATE,
        Advanced,
        NONE,
        None,
        None
    ),
    operation!(
        "inspect_toml",
        Authoring,
        Inspect,
        Read,
        READ_FILES,
        Common,
        NONE,
        None,
        None
    ),
    operation!(
        "load_workspace",
        Workspace,
        Inspect,
        Read,
        READ_FILES,
        Common,
        LOAD_WORKSPACE,
        None,
        Related
    ),
    operation!(
        "contract_schema",
        Workspace,
        Inspect,
        Read,
        NO_EFFECTS,
        Common,
        NONE,
        None,
        None
    ),
    operation!(
        "observe_delivery",
        Observation,
        Observe,
        External,
        READ_FILES_PROCESS_NETWORK,
        Common,
        NONE,
        None,
        None
    ),
    operation!(
        "observe_github_pages_build",
        Observation,
        Observe,
        External,
        READ_FILES_PROCESS_NETWORK,
        Common,
        NONE,
        None,
        None
    ),
    operation!(
        "observe_release_targets",
        Observation,
        Observe,
        External,
        READ_FILES_PROCESS_NETWORK,
        Common,
        OBSERVE_TARGETS,
        None,
        Related
    ),
    operation!(
        "open_recovery_session",
        Recovery,
        Recover,
        Local,
        READ_SQLITE_MEMORY_STATE,
        Advanced,
        NONE,
        Some("close_recovery_session"),
        TerminalOwner
    ),
    operation!(
        "open_release_session",
        Release,
        Act,
        Local,
        READ_WRITE_SQLITE_MEMORY_STATE,
        Advanced,
        NONE,
        Some("close_release_session"),
        TerminalOwner
    ),
    operation!(
        "open_store_session",
        Store,
        Act,
        Local,
        READ_WRITE_SQLITE_MEMORY_STATE,
        Advanced,
        NONE,
        Some("close_store_session"),
        TerminalOwner
    ),
    operation!(
        "operate_recovery_session",
        Recovery,
        Recover,
        Local,
        READ_WRITE_SQLITE,
        Advanced,
        OPERATE_RECOVERY,
        None,
        Related
    ),
    operation!(
        "operate_release_session",
        Release,
        Act,
        External,
        READ_WRITE_SQLITE_PROCESS_GIT_NETWORK,
        Advanced,
        OPERATE_RELEASE,
        None,
        Related
    ),
    operation!(
        "parse_site_documents",
        Workspace,
        Verify,
        Read,
        READ_FILES,
        Advanced,
        NONE,
        None,
        None
    ),
    operation!(
        "plan_preview",
        Preview,
        Inspect,
        Read,
        NO_EFFECTS,
        Common,
        NONE,
        None,
        None
    ),
    operation!(
        "plan_production_preview",
        Preview,
        Inspect,
        Read,
        NO_EFFECTS,
        Advanced,
        NONE,
        None,
        None
    ),
    operation!(
        "poll_preview",
        Preview,
        Observe,
        Read,
        NO_EFFECTS,
        Common,
        NONE,
        None,
        None
    ),
    operation!(
        "prepare_release_commit",
        Release,
        Act,
        Local,
        READ_WRITE_GIT,
        Advanced,
        PREPARE_RELEASE,
        None,
        Related
    ),
    operation!(
        "prepare_sites",
        Release,
        Act,
        Local,
        READ_WRITE_SQLITE_PROCESS_GIT,
        Advanced,
        NONE,
        None,
        SelfTerminal
    ),
    operation!(
        "prepare_store_restore",
        Store,
        Recover,
        Local,
        READ_WRITE_SQLITE,
        Advanced,
        PREPARE_RESTORE,
        None,
        Related
    ),
    operation!(
        "probe_output",
        Build,
        Verify,
        Read,
        READ_FILES,
        Advanced,
        NONE,
        None,
        None
    ),
    operation!(
        "probe_rendered_site",
        Build,
        Verify,
        Read,
        READ_FILES_PROCESS,
        Advanced,
        NONE,
        None,
        None
    ),
    operation!(
        "project_registry",
        Build,
        Inspect,
        Read,
        READ_FILES,
        Advanced,
        NONE,
        None,
        None
    ),
    operation!(
        "qualify_sites",
        Build,
        Verify,
        Local,
        READ_FILES_PROCESS,
        Common,
        NONE,
        None,
        None
    ),
    operation!(
        "reconcile_registry",
        Store,
        Reconcile,
        Local,
        READ_WRITE_SQLITE,
        Advanced,
        RECONCILE_REGISTRY,
        None,
        Related
    ),
    operation!(
        "record_release_observation",
        Observation,
        Act,
        External,
        READ_WRITE_SQLITE_PROCESS_NETWORK,
        Advanced,
        RECORD_OBSERVATION,
        None,
        Related
    ),
    operation!(
        "recover_journal_worktree",
        Release,
        Recover,
        Local,
        READ_WRITE_SQLITE_GIT,
        Advanced,
        NONE,
        None,
        SelfTerminal
    ),
    operation!(
        "render_sites",
        Build,
        Act,
        Local,
        READ_FILES_PROCESS,
        Common,
        RENDER_SITES,
        None,
        Related
    ),
    operation!(
        "materialize_sites",
        Build,
        Act,
        Local,
        MATERIALIZATION_EFFECTS,
        Common,
        MATERIALIZE_SITES,
        Some("close_store_session"),
        TerminalOwner
    ),
    operation!(
        "materialize_docs",
        Documentation,
        Act,
        Local,
        MATERIALIZATION_EFFECTS,
        Common,
        MATERIALIZE_DOCS,
        Some("close_store_session"),
        TerminalOwner
    ),
    operation!(
        "retain_release_commit",
        Release,
        Act,
        Local,
        READ_WRITE_GIT,
        Advanced,
        NONE,
        None,
        SelfTerminal
    ),
    operation!(
        "review_publication",
        Build,
        Inspect,
        Read,
        READ_FILES,
        Advanced,
        REVIEW_PUBLICATION,
        None,
        Related
    ),
    operation!(
        "stage",
        Build,
        Act,
        Local,
        READ_WRITE_FILES,
        Advanced,
        NONE,
        None,
        SelfTerminal
    ),
    operation!(
        "stage_rendered_site",
        Build,
        Act,
        Local,
        READ_WRITE_FILES_PROCESS,
        Advanced,
        NONE,
        None,
        SelfTerminal
    ),
    operation!(
        "start_preview",
        Preview,
        Act,
        Local,
        PREVIEW_START,
        Common,
        NONE,
        Some("stop_preview"),
        TerminalOwner
    ),
    operation!(
        "start_production_preview",
        Preview,
        Act,
        Local,
        PREVIEW_START,
        Advanced,
        NONE,
        Some("stop_preview"),
        TerminalOwner
    ),
    operation!(
        "stop_preview",
        Preview,
        Act,
        Local,
        PREVIEW_STOP,
        Common,
        NONE,
        None,
        SelfTerminal
    ),
    operation!(
        "verify_backup",
        Store,
        Verify,
        Local,
        READ_SQLITE,
        Advanced,
        NONE,
        None,
        None
    ),
    operation!(
        "verify_generated_tree",
        Documentation,
        Verify,
        Read,
        READ_FILES,
        Advanced,
        NONE,
        None,
        None
    ),
    operation!(
        "verify_prepared_authorization",
        Release,
        Verify,
        Local,
        READ_SQLITE,
        Advanced,
        VERIFY_PREPARED,
        None,
        Related
    ),
    operation!(
        "verify_release_authorization",
        Release,
        Verify,
        Read,
        NO_EFFECTS,
        Advanced,
        NONE,
        None,
        None
    ),
    operation!(
        "verify_release_commit",
        Release,
        Verify,
        Read,
        READ_FILES,
        Advanced,
        NONE,
        None,
        None
    ),
    operation!(
        "workspace_composition_origin",
        Workspace,
        Inspect,
        Read,
        NO_EFFECTS,
        Advanced,
        NONE,
        None,
        None
    ),
    operation!(
        "workspace_field_origin",
        Workspace,
        Inspect,
        Read,
        NO_EFFECTS,
        Advanced,
        NONE,
        None,
        None
    ),
];

const fn base(operations: &'static [&'static str]) -> CliMode {
    CliMode {
        selector: None,
        operations,
    }
}

const fn selected(selector: &'static str, operations: &'static [&'static str]) -> CliMode {
    CliMode {
        selector: Some(selector),
        operations,
    }
}

macro_rules! family {
    ($command:literal, [$($mode:expr),+ $(,)?]) => {
        CliFamily { command: $command, modes: &[$($mode),+] }
    };
}

const CLI_FAMILIES: &[CliFamily] = &[
    // CLI validates an export without retaining it; native reconstruction requires
    // --allow-read. Protocol imports require the same explicit permission and close.
    family!(
        "import-editor-context",
        [base(&[
            "authoring_import_editor_context",
            "authoring_status",
            "authoring_close_session",
        ])]
    ),
    family!(
        "prepare-release-commit",
        [base(&["prepare_release_commit"])]
    ),
    family!("verify-release-commit", [base(&["verify_release_commit"])]),
    family!(
        "checkpoint-release-commit",
        [base(&["checkpoint_release_commit"])]
    ),
    family!(
        "inspect-release-checkpoint",
        [base(&["inspect_release_checkpoint"])]
    ),
    family!(
        "inspect-journal-worktree",
        [base(&["inspect_journal_worktree"])]
    ),
    family!(
        "recover-journal-worktree",
        [base(&["recover_journal_worktree"])]
    ),
    family!("retain-release-commit", [base(&["retain_release_commit"])]),
    family!(
        "inspect-release-custody",
        [base(&["inspect_release_custody"])]
    ),
    family!(
        "verify-release-authorization",
        [base(&["verify_release_authorization"])]
    ),
    family!(
        "inspect-release-preparations",
        [base(&["inspect_release_preparations"])]
    ),
    family!(
        "verify-prepared-authorization",
        [base(&["verify_prepared_authorization"])]
    ),
    family!(
        "observe-release-targets",
        [base(&["observe_release_targets"])]
    ),
    family!(
        "observe-github-pages-build",
        [base(&["observe_github_pages_build"])]
    ),
    family!("observe-delivery", [base(&["observe_delivery"])]),
    family!(
        "record-release-observation",
        [base(&["record_release_observation"])]
    ),
    family!(
        "inspect-release-observation",
        [base(&["inspect_release_observation"])]
    ),
    family!(
        "inspect-release-authorization",
        [base(&["inspect_release_authorization"])]
    ),
    family!(
        "inspect-release-leases",
        [base(&["inspect_release_leases"])]
    ),
    family!(
        "inspect-release-consumption",
        [base(&["inspect_release_consumption"])]
    ),
    family!(
        "inspect-release-attempts",
        [base(&["inspect_release_attempts"])]
    ),
    family!(
        "inspect-release-reconciliations",
        [base(&["inspect_release_reconciliations"])]
    ),
    family!(
        "enroll-local-coordinator",
        [base(&["enroll_local_coordinator"])]
    ),
    family!(
        "inspect-local-coordinator",
        [base(&["inspect_local_coordinator"])]
    ),
    family!("adopt-store-restore", [base(&["adopt_store_restore"])]),
    family!("reconcile-registry", [base(&["reconcile_registry"])]),
    family!("inspect-registry", [base(&["inspect_registry"])]),
    family!(
        "inspect-materialization",
        [base(&["inspect_materialization"])]
    ),
    family!("schema", [base(&["contract_schema"])]),
    family!(
        "assemble-docs",
        [
            base(&["assemble_docs"]),
            selected("--check", &["check_documentation"]),
            selected("--preparation-json", &["assemble_docs"]),
            selected("--allow-execute", &["assemble_docs"]),
        ]
    ),
    family!(
        "check",
        [
            base(&["check_workspace"]),
            selected("workspace", &["load_workspace"]),
            selected("--origin", &["workspace_field_origin"]),
            selected("--composition-origin", &["workspace_composition_origin"]),
            selected("--acquire-site", &["inspect_site"]),
            selected("--parse-documents", &["parse_site_documents"]),
            selected("--acquire-composition", &["inspect_composition"]),
            selected("--check-composition", &["check_composition"]),
            selected("--compile-templates", &["compile_templates"]),
            selected("--publication", &["inspect_publication"]),
            selected("--review", &["review_publication"]),
            selected("--registry", &["project_registry"]),
            selected("build", &["render_sites"]),
        ]
    ),
    family!(
        "render-site",
        [
            base(&["render_sites"]),
            selected("--output-parent", &["stage_rendered_site"]),
            selected("--probe-disclosure", &["probe_rendered_site"]),
        ]
    ),
    family!("probe-output", [base(&["probe_output"])]),
    family!("render-sites", [base(&["render_sites"])]),
    family!(
        "materialize-sites",
        [base(&[
            "open_store_session",
            "materialize_sites",
            "close_store_session"
        ])]
    ),
    family!("qualify-sites", [base(&["qualify_sites"])]),
    family!(
        "materialize-docs",
        [base(&[
            "open_store_session",
            "materialize_docs",
            "close_store_session"
        ])]
    ),
    family!("prepare-sites", [base(&["prepare_sites"])]),
    family!(
        "preview-sites",
        [
            base(&[
                "start_preview",
                "inspect_preview",
                "poll_preview",
                "stop_preview"
            ]),
            selected("--watch", &["start_preview", "poll_preview"]),
            selected("--local-urls", &["start_preview"]),
            selected(
                "--preview-link-bases-json",
                &["start_preview", "plan_preview"]
            ),
            selected("--plan", &["plan_preview"]),
        ]
    ),
    family!(
        "production-preview",
        [
            base(&[
                "start_production_preview",
                "inspect_preview",
                "poll_preview",
                "stop_preview"
            ]),
            selected("--plan", &["plan_production_preview"]),
        ]
    ),
    family!("check-stage", [base(&["check_stage"])]),
    family!("stage", [base(&["stage"])]),
    family!("verify-generated-tree", [base(&["verify_generated_tree"])]),
    family!(
        "acquire-writer",
        [base(&[
            "open_store_session",
            "inspect_store_session",
            "close_store_session"
        ])]
    ),
    family!(
        "release-session",
        [base(&[
            "open_release_session",
            "operate_release_session",
            "close_release_session"
        ])]
    ),
    family!(
        "recovery-session",
        [base(&[
            "open_recovery_session",
            "operate_recovery_session",
            "close_recovery_session"
        ])]
    ),
    family!("inspect-journal", [base(&["inspect_journal"])]),
    family!("initialize-store", [base(&["initialize_store"])]),
    family!(
        "connect-store",
        [base(&[
            "open_store_session",
            "inspect_store_session",
            "close_store_session"
        ])]
    ),
    family!("verify-backup", [base(&["verify_backup"])]),
    family!("prepare-store-restore", [base(&["prepare_store_restore"])]),
    family!("backup-store", [base(&["backup_store"])]),
    family!("inspect-store", [base(&["inspect_store"])]),
    family!("inspect-toml", [base(&["inspect_toml"])]),
    family!("inspect-markdown", [base(&["inspect_markdown"])]),
    family!(
        "inspect-source",
        [
            base(&["inspect_source"]),
            selected("--commit", &["inspect_source"]),
            selected("--submodule", &["inspect_source"]),
        ]
    ),
];

/// Canonical operation and CLI-family owner for every interface projection.
pub const REGISTRY: Registry = Registry {
    operations: OPERATIONS,
    cli_families: CLI_FAMILIES,
};

/// Return every canonical application operation exactly once.
#[must_use]
pub const fn operations() -> &'static [Operation] {
    REGISTRY.operations
}

/// Return every non-launcher direct CLI family exactly once.
#[must_use]
pub const fn cli_families() -> &'static [CliFamily] {
    REGISTRY.cli_families
}

/// Resolve one direct CLI family from its exact hyphenated spelling.
#[must_use]
pub fn cli_family(name: &str) -> Option<&'static CliFamily> {
    REGISTRY
        .cli_families
        .iter()
        .find(|family| family.command == name)
}

/// Resolve stable metadata for an existing canonical operation name.
///
/// Hyphenated spelling is normalized mechanically. Direct CLI command names
/// are resolved separately through [`cli_family`].
#[must_use]
pub fn operation(name: &str) -> Option<&'static Operation> {
    let normalized = name.replace('-', "_");
    REGISTRY
        .operations
        .iter()
        .find(|operation| operation.name == normalized)
}

/// Render a concise capability-first guide for the flat, stable CLI command set.
///
/// The complete command list remains Clap-owned. This guide intentionally
/// presents only common entry points and counts the advanced commands that stay
/// available through the normal per-command help.
#[must_use]
pub fn grouped_cli_help<'a>(names: impl IntoIterator<Item = &'a str>) -> String {
    let mut groups = std::collections::BTreeMap::<Capability, (Vec<String>, usize)>::new();
    for name in names {
        let operations = cli_family(name)
            .map(|family| {
                family
                    .modes
                    .iter()
                    .filter_map(CliMode::entry_operation)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| vec![name]);
        let mut projections = std::collections::BTreeMap::<Capability, Prominence>::new();
        for operation_name in operations {
            let Some(metadata) = operation(operation_name) else {
                continue;
            };
            projections
                .entry(metadata.capability)
                .and_modify(|prominence| {
                    if metadata.prominence == Prominence::Common {
                        *prominence = Prominence::Common;
                    }
                })
                .or_insert(metadata.prominence);
        }
        for (capability, prominence) in projections {
            let group = groups.entry(capability).or_default();
            match prominence {
                Prominence::Common => group.0.push(name.to_owned()),
                Prominence::Advanced => group.1 += 1,
            }
        }
    }
    let mut help = String::from(
        "Capability guide (start with common operations; every command remains available below):\n",
    );
    for (capability, (common, advanced)) in groups {
        if common.is_empty() {
            let operation = if advanced == 1 {
                "operation"
            } else {
                "operations"
            };
            let _ = writeln!(
                help,
                "  {:13} {advanced} advanced {operation}",
                capability.title()
            );
        } else {
            let _ = writeln!(
                help,
                "  {:13} {}  ({advanced} advanced)",
                capability.title(),
                common.join(", ")
            );
        }
    }
    help.push_str(
        "Use `gazette <command> --help` for exact inputs, permissions, effects and results.",
    );
    help
}
