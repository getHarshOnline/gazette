//! Engine-owned generated-tree evidence, never a publication or deletion capability.

use super::{AcquiredInput, PreparedSourceSet, SourceSelection};
use gazette_core::source::{
    CapturedSource, CommitId, EntryPath, Error, Inventory, InventoryBuilder, Limits, Sha256Digest,
    Subdirectory, TargetPath, TreeIdentity,
};
use gazette_ports::source::SourceCapture;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

/// Reserved metadata filename in each generated project-documentation root.
pub const GENERATED_TREE_MANIFEST: &str = ".gazette-source.json";
const FORMAT: &str = "gazette.generated-source.v1";
const MANIFEST_BYTES: usize = 16 * 1024;

/// Owner-selected identifiers transported without inferring their meaning or authority.
#[derive(Clone, Debug)]
pub struct PlacementIdentity {
    /// Explicit project identity; never derived from a repository name or URL.
    pub project: String,
    /// Explicit deployment identity within the selected site.
    pub deployment: String,
}

/// Explicit context for one site's complete materialization set.
#[derive(Clone, Debug)]
pub struct StageContext {
    /// Owner-selected workspace identity, not a machine path.
    pub workspace: String,
    /// Owner-selected site identity, not inferred publication approval.
    pub site: String,
    /// Exactly one identity binding for every normalized prepared target.
    pub placements: BTreeMap<TargetPath, PlacementIdentity>,
}

/// Exact expected placement coordinates; parsing is not semantic admission.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GeneratedTreeBinding {
    /// Selected workspace identity.
    pub workspace: String,
    /// Selected site identity.
    pub site: String,
    /// Selected project identity.
    pub project: String,
    /// Selected deployment identity.
    pub deployment: String,
    /// Exact already-normalized relative placement, never reinterpreted on read-back.
    pub target: String,
}

impl StageContext {
    /// Resolve an explicit expected binding without inventing a missing identity.
    ///
    /// # Errors
    /// Rejects missing targets, empty/overlong/control-bearing identities or invalid target spelling.
    pub fn binding(&self, target: &TargetPath) -> Result<GeneratedTreeBinding, Error> {
        let identity = self
            .placements
            .get(target)
            .ok_or_else(|| invalid("missing placement identity"))?;
        let binding = GeneratedTreeBinding {
            workspace: self.workspace.clone(),
            site: self.site.clone(),
            project: identity.project.clone(),
            deployment: identity.deployment.clone(),
            target: target.as_str().to_owned(),
        };
        binding.validate()?;
        Ok(binding)
    }
}

impl GeneratedTreeBinding {
    fn validate(&self) -> Result<(), Error> {
        for value in [&self.workspace, &self.site, &self.project, &self.deployment] {
            validate_identity(value)?;
        }
        let target: TargetPath = self
            .target
            .parse()
            .map_err(|_| invalid("invalid manifest target"))?;
        if target.as_str() != self.target {
            return Err(invalid("manifest target is not already normalized"));
        }
        Ok(())
    }
}

/// Exact source provenance carried from native acquisition, excluding machine-local paths.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum GeneratedSource {
    /// One retained local observation. This is explicitly not revision lineage.
    Filesystem {
        /// Optional confined selector beneath the private native root.
        #[serde(skip_serializing_if = "Option::is_none")]
        subdirectory: Option<String>,
    },
    /// An explicitly filtered local observation; exclusions are part of provenance.
    FilteredFilesystem {
        /// Capture-relative omitted subtrees.
        exclude: Vec<EntryPath>,
    },
    /// A local observation of only the named files or directory trees.
    IncludedFilesystem {
        /// Required root-relative selections retained with the generated output.
        include: Vec<EntryPath>,
        /// Branch checked around capture; this records no committed-content claim.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_branch: Option<String>,
    },
    /// Working repository selection, without a commit or clean-tree claim.
    WorkingRepository {
        /// Folder beneath the private repository root; absence means that root.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        subdirectory: Option<Subdirectory>,
        /// Branch checked around capture.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_branch: Option<String>,
    },
    /// One checked-out repository revision.
    Repository {
        /// Exact selected commit.
        commit: String,
        /// Literal selected documentation subtree.
        docs_path: String,
    },
    /// A child selected through an exact parent's gitlink.
    Submodule {
        /// Exact selected parent commit.
        parent_commit: String,
        /// Literal gitlink beneath that parent.
        submodule_path: String,
        /// Child commit obtained from that gitlink.
        commit: String,
        /// Literal selected child documentation subtree.
        docs_path: String,
    },
}

impl GeneratedSource {
    fn validate(&self) -> Result<(), Error> {
        if let Self::IncludedFilesystem {
            expected_branch: Some(branch),
            ..
        }
        | Self::WorkingRepository {
            expected_branch: Some(branch),
            ..
        } = self
            && !gazette_core::source::valid_branch_name(branch)
        {
            return Err(invalid("invalid expected branch in source provenance"));
        }
        let (commit, docs_path) = match self {
            Self::WorkingRepository { .. } => return Ok(()),
            Self::Filesystem { subdirectory } => {
                if let Some(subdirectory) = subdirectory {
                    subdirectory
                        .parse::<Subdirectory>()
                        .map_err(|_| invalid("invalid filesystem selector"))?;
                }
                return Ok(());
            }
            Self::FilteredFilesystem { exclude: paths }
            | Self::IncludedFilesystem { include: paths, .. } => {
                if paths.is_empty()
                    || paths.len() > 128
                    || paths
                        .iter()
                        .collect::<std::collections::BTreeSet<_>>()
                        .len()
                        != paths.len()
                {
                    return Err(invalid("invalid explicit source selections"));
                }
                return Ok(());
            }
            Self::Repository { commit, docs_path } => (commit, docs_path),
            Self::Submodule {
                parent_commit,
                submodule_path,
                commit,
                docs_path,
            } => {
                parent_commit
                    .parse::<CommitId>()
                    .map_err(|_| invalid("invalid parent commit"))?;
                submodule_path
                    .parse::<Subdirectory>()
                    .map_err(|_| invalid("invalid gitlink path"))?;
                (commit, docs_path)
            }
        };
        commit
            .parse::<CommitId>()
            .map_err(|_| invalid("invalid source commit"))?;
        docs_path
            .parse::<Subdirectory>()
            .map_err(|_| invalid("invalid documentation selector"))?;
        Ok(())
    }
}

/// Versioned transport evidence. The tree digest binds staged bytes while source-kind
/// provenance remains explicit; it does not invent revision lineage for filesystem input.
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GeneratedTreeManifest {
    format: String,
    generator_version: String,
    binding: GeneratedTreeBinding,
    source: GeneratedSource,
    tree_format: String,
    source_tree_sha256: Sha256Digest,
}

impl GeneratedTreeManifest {
    /// Owner-selected coordinates carried by this record.
    pub fn binding(&self) -> &GeneratedTreeBinding {
        &self.binding
    }
    /// Preserved filesystem observation or repository/parent-gitlink lineage.
    pub fn source(&self) -> &GeneratedSource {
        &self.source
    }
    /// Exact acquired source tree digest, excluding this generated metadata file.
    pub fn source_tree_sha256(&self) -> Sha256Digest {
        self.source_tree_sha256
    }
    fn validate(&self) -> Result<(), Error> {
        if self.format != FORMAT
            || self.generator_version != env!("CARGO_PKG_VERSION")
            || self.tree_format != TreeIdentity::FORMAT
        {
            return Err(invalid(
                "unsupported generated-tree format or generator version",
            ));
        }
        self.binding.validate()?;
        self.source.validate()
    }
}

/// Complete captured documentation whose bytes match independently pinned generated evidence.
/// This does not prove current source reachability, rights, publication or cleanup authority.
#[derive(Debug)]
pub struct VerifiedMaterialization {
    manifest: GeneratedTreeManifest,
    source: CapturedSource,
}

/// Shared observation projection over verified metadata and the complete source inventory.
/// The generated marker is represented by `manifest`, not included in `inventory`.
#[derive(Serialize, schemars::JsonSchema)]
pub struct GeneratedTreeInspection<'a> {
    /// Metadata decoded from the independently pinned manifest bytes.
    pub manifest: &'a GeneratedTreeManifest,
    /// Verified documentation entries, excluding the generated marker itself.
    pub inventory: &'a Inventory,
}

impl VerifiedMaterialization {
    /// Borrow the verified evidence without recapturing source bytes or inferring authority.
    pub fn inspect(&self) -> GeneratedTreeInspection<'_> {
        GeneratedTreeInspection {
            manifest: self.manifest(),
            inventory: self.source().inventory(),
        }
    }

    /// Checked metadata from the exact pinned bytes.
    pub fn manifest(&self) -> &GeneratedTreeManifest {
        &self.manifest
    }
    /// Retained documentation bytes for downstream compilation; metadata is excluded.
    pub fn source(&self) -> &CapturedSource {
        &self.source
    }
}

pub(super) fn generate(
    prepared: &PreparedSourceSet,
    context: &StageContext,
) -> Result<BTreeMap<EntryPath, Vec<u8>>, Error> {
    validate_identity(&context.workspace)?;
    validate_identity(&context.site)?;
    if context.placements.len() != prepared.placements().len() {
        return Err(invalid(
            "placement bindings do not match the complete prepared set",
        ));
    }
    let mut files = BTreeMap::new();
    for placement in prepared.placements() {
        let binding = context.binding(&placement.request().target)?;
        let source = match placement.source() {
            AcquiredInput::Filesystem(_) => match &placement.request().source {
                SourceSelection::Filesystem { subdirectory, .. } => GeneratedSource::Filesystem {
                    subdirectory: subdirectory.as_ref().map(ToString::to_string),
                },
                SourceSelection::FilteredFilesystem { exclude, .. } => {
                    GeneratedSource::FilteredFilesystem {
                        exclude: exclude.clone(),
                    }
                }
                SourceSelection::IncludedFilesystem {
                    include,
                    expected_branch,
                    ..
                } => GeneratedSource::IncludedFilesystem {
                    include: include.clone(),
                    expected_branch: expected_branch.clone(),
                },
                SourceSelection::WorkingRepository {
                    subdirectory,
                    expected_branch,
                    ..
                } => GeneratedSource::WorkingRepository {
                    subdirectory: subdirectory.clone(),
                    expected_branch: expected_branch.clone(),
                },
                _ => return Err(invalid("filesystem acquisition selection mismatch")),
            },
            AcquiredInput::Development(_) => {
                return Err(invalid(
                    "development inputs cannot become verified generated materialization",
                ));
            }
            AcquiredInput::Repository(acquired) => {
                let source = acquired.verification();
                GeneratedSource::Repository {
                    commit: source.commit().to_string(),
                    docs_path: source.subdirectory().to_string(),
                }
            }
            AcquiredInput::Submodule(acquired) => {
                let source = acquired.verification();
                GeneratedSource::Submodule {
                    parent_commit: source.parent_commit().to_string(),
                    submodule_path: source.submodule_path().to_string(),
                    commit: source.source().commit().to_string(),
                    docs_path: source.source().subdirectory().to_string(),
                }
            }
        };
        let manifest = GeneratedTreeManifest {
            format: FORMAT.into(),
            generator_version: env!("CARGO_PKG_VERSION").into(),
            binding,
            source,
            tree_format: TreeIdentity::FORMAT.into(),
            source_tree_sha256: placement.source().inventory().identity().sha256(),
        };
        manifest.validate()?;
        let bytes = serde_json::to_vec(&manifest)
            .map_err(|_| invalid("cannot encode generated-tree evidence"))?;
        if bytes.len() > MANIFEST_BYTES {
            return Err(invalid("generated-tree manifest exceeds 16384 bytes"));
        }
        files.insert(
            format!("{}/{GENERATED_TREE_MANIFEST}", placement.request().target).parse()?,
            bytes,
        );
    }
    Ok(files)
}

/// Capture and verify a generated documentation root against independently retained evidence.
///
/// The manifest digest must come from prior trusted staging evidence, not a hash
/// freshly computed from the suspect marker. Native capture bounds apply to the
/// complete tree; manifest decoding has an additional 16 KiB ceiling. No source is
/// fetched, repaired, replaced, removed or admitted for publication.
///
/// # Errors
/// Rejects missing/malformed/unsupported markers, pin or binding mismatch, unsafe
/// native input, changed source-tree bytes and exhausted capture/decode bounds.
pub fn verify_generated_tree(
    reader: &impl SourceCapture,
    root: &Path,
    expected: &GeneratedTreeBinding,
    manifest_sha256: Sha256Digest,
    limits: Limits,
) -> Result<VerifiedMaterialization, Error> {
    expected.validate()?;
    let (inventory, files) = reader.capture(root, None, limits)?.into_parts();
    let mut files = files.into_files();
    let metadata: EntryPath = GENERATED_TREE_MANIFEST.parse()?;
    let bytes = files
        .remove(&metadata)
        .ok_or_else(|| invalid("missing generated-tree manifest"))?;
    if bytes.len() > MANIFEST_BYTES {
        return Err(invalid("generated-tree manifest exceeds 16384 bytes"));
    }
    if Sha256Digest::of(&bytes) != manifest_sha256 {
        return Err(invalid("generated-tree manifest pin mismatch"));
    }
    let manifest: GeneratedTreeManifest =
        serde_json::from_slice(&bytes).map_err(|_| invalid("malformed generated-tree manifest"))?;
    manifest.validate()?;
    if &manifest.binding != expected {
        return Err(invalid("generated-tree binding mismatch"));
    }
    let mut source = InventoryBuilder::new(limits);
    for entry in inventory.entries() {
        if entry.path != metadata {
            source.insert(entry.path.clone(), entry.content)?;
        }
    }
    let source = source.finish();
    if source.identity().sha256() != manifest.source_tree_sha256 {
        return Err(invalid(
            "generated documentation bytes do not match their pinned source tree",
        ));
    }
    Ok(VerifiedMaterialization {
        manifest,
        source: CapturedSource::from_parts(source, files)?,
    })
}

fn validate_identity(value: &str) -> Result<(), Error> {
    if value.is_empty()
        || value.len() > 512
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(invalid(
            "identity must be explicit, bounded and free of surrounding whitespace or controls",
        ));
    }
    Ok(())
}

fn invalid(reason: &'static str) -> Error {
    Error::InvalidInventory {
        path: GENERATED_TREE_MANIFEST.into(),
        reason,
    }
}
