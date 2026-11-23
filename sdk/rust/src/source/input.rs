//! Named placement input shared by direct interfaces; not publication admission.

use super::{
    CommitId, Error, PlacementIdentity, PlacementRequest, SourceSelection, StageContext,
    Subdirectory, TargetPath,
};
use schemars::JsonSchema;
use serde::{
    Deserialize, Deserializer,
    de::{Error as _, MapAccess, Visitor, value::MapAccessDeserializer},
};
use std::{collections::BTreeMap, fmt, marker::PhantomData, path::PathBuf, str::FromStr};

/// One explicitly named placement, with an exact native source selection.
///
/// Interfaces use [`parse`] or [`object`] to require named-object transport;
/// the derived representation alone does not enforce that transport shape.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Placement {
    /// Confined logical destination, normalized by the native path type.
    #[serde(deserialize_with = "parse_text")]
    #[schemars(with = "String")]
    pub target: TargetPath,
    /// Explicit project identity, not inferred from the source path.
    pub project: String,
    /// Explicit deployment identity within the site.
    pub deployment: String,
    /// Exact filesystem, repository or parent-gitlink selection.
    #[serde(deserialize_with = "selection_object")]
    pub source: Selection,
}

/// Native source choices; no moving-ref or source-kind fallback.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Selection {
    /// Read one complete bounded local tree without claiming revision lineage.
    Filesystem {
        /// Explicit local root.
        root: PathBuf,
        /// Optional confined subtree beneath the root.
        #[serde(default)]
        subdirectory: Option<Subdirectory>,
    },
    /// Read only the named local files or directory trees.
    IncludedFilesystem {
        /// Explicit local root.
        root: PathBuf,
        /// Required literal selections; every path must exist.
        #[schemars(length(min = 1, max = 128))]
        include: Vec<gazette_core::source::EntryPath>,
        /// Optional checked-out branch requirement; this does not pin content.
        #[serde(default)]
        expected_branch: Option<String>,
    },
    /// Read tracked and nonignored working files from one repository folder.
    WorkingRepository {
        /// Exact local repository root.
        root: PathBuf,
        /// Optional folder; absence selects the exact repository root.
        #[serde(default)]
        subdirectory: Option<Subdirectory>,
        /// Optional branch requirement, without a committed-content claim.
        #[serde(default)]
        expected_branch: Option<String>,
    },
    /// Read a verified subtree of an exact checked-out repository.
    Repository {
        /// Explicit local repository root.
        root: PathBuf,
        /// Exact lowercase forty-character commit identifier.
        commit: CommitId,
        /// Literal source-relative directory, not a URL.
        subdirectory: Subdirectory,
    },
    /// Read the child located by an exact parent gitlink.
    Submodule {
        /// Explicit local parent repository root.
        parent: PathBuf,
        /// Exact checked-out parent commit.
        parent_commit: CommitId,
        /// Literal parent-relative gitlink location.
        submodule_path: Subdirectory,
        /// Literal directory beneath the bound child.
        subdirectory: Subdirectory,
    },
}

impl Selection {
    /// Explicit native root whose access the caller must permit.
    pub fn root(&self) -> &std::path::Path {
        match self {
            Self::Filesystem { root, .. } | Self::IncludedFilesystem { root, .. } => root,
            Self::Repository { root, .. } | Self::WorkingRepository { root, .. } => root,
            Self::Submodule { parent, .. } => parent,
        }
    }
}

impl From<Selection> for SourceSelection {
    fn from(value: Selection) -> Self {
        match value {
            Selection::Filesystem { root, subdirectory } => Self::Filesystem { root, subdirectory },
            Selection::IncludedFilesystem {
                root,
                include,
                expected_branch,
            } => Self::IncludedFilesystem {
                root,
                include,
                expected_branch,
            },
            Selection::WorkingRepository {
                root,
                subdirectory,
                expected_branch,
            } => Self::WorkingRepository {
                root,
                subdirectory,
                expected_branch,
            },
            Selection::Repository {
                root,
                commit,
                subdirectory,
            } => Self::Repository {
                root,
                commit,
                subdirectory,
            },
            Selection::Submodule {
                parent,
                parent_commit,
                submodule_path,
                subdirectory,
            } => Self::Submodule {
                parent,
                parent_commit,
                submodule_path,
                subdirectory,
            },
        }
    }
}

/// Decode one named placement before any source reads.
///
/// # Errors
/// Rejects input over 16 KiB, malformed JSON, duplicate or unknown fields,
/// positional arrays and invalid native pins or paths.
pub fn parse(value: &str) -> Result<Placement, String> {
    if value.len() > 16 * 1024 {
        return Err("placement argument exceeds 16384 bytes".into());
    }
    let mut decoder = serde_json::Deserializer::from_str(value);
    let placement =
        object(&mut decoder).map_err(|error| format!("invalid placement argument: {error}"))?;
    decoder
        .end()
        .map_err(|error| format!("invalid placement argument: {error}"))?;
    Ok(placement)
}

/// Decode a named placement within another bounded transport.
///
/// # Errors
/// Rejects positional arrays at the placement and nested source boundaries and
/// propagates the shared placement decoder errors.
pub fn object<'de, D: Deserializer<'de>>(decoder: D) -> Result<Placement, D::Error> {
    named(decoder, "a named placement object")
}

fn selection_object<'de, D: Deserializer<'de>>(decoder: D) -> Result<Selection, D::Error> {
    named(decoder, "a named source object")
}

fn named<'de, T: Deserialize<'de>, D: Deserializer<'de>>(
    decoder: D,
    expected: &'static str,
) -> Result<T, D::Error> {
    struct Named<T> {
        expected: &'static str,
        value: PhantomData<T>,
    }
    impl<'de, T: Deserialize<'de>> Visitor<'de> for Named<T> {
        type Value = T;
        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str(self.expected)
        }
        fn visit_map<M: MapAccess<'de>>(self, map: M) -> Result<T, M::Error> {
            T::deserialize(MapAccessDeserializer::new(map))
        }
    }
    decoder.deserialize_map(Named {
        expected,
        value: PhantomData,
    })
}

/// Bind the complete placement set without acquiring sources or inferring identity.
///
/// Target ancestry and source budgets remain owned by [`super::prepare`].
/// Whole-context validation, including an authored-only set, remains mandatory
/// in [`super::check_stage`] and [`super::stage`] before any source read/write.
///
/// # Errors
/// Rejects duplicate normalized targets and invalid explicit binding identities.
pub fn bind(
    workspace: String,
    site: String,
    placements: Vec<Placement>,
) -> Result<(StageContext, Vec<PlacementRequest>), Error> {
    let mut context = StageContext {
        workspace,
        site,
        placements: BTreeMap::new(),
    };
    let mut requests = Vec::with_capacity(placements.len());
    for placement in placements {
        if context
            .placements
            .insert(
                placement.target.clone(),
                PlacementIdentity {
                    project: placement.project,
                    deployment: placement.deployment,
                },
            )
            .is_some()
        {
            return Err(Error::InvalidInventory {
                path: placement.target.as_str().into(),
                reason: "duplicate normalized placement target",
            });
        }
        context.binding(&placement.target)?;
        requests.push(PlacementRequest {
            target: placement.target,
            source: placement.source.into(),
        });
    }
    Ok((context, requests))
}

fn parse_text<'de, T, D>(deserializer: D) -> Result<T, D::Error>
where
    T: FromStr,
    T::Err: fmt::Display,
    D: Deserializer<'de>,
{
    String::deserialize(deserializer)?
        .parse()
        .map_err(D::Error::custom)
}
