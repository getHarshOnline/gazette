//! Rebuildable publication artifacts keyed by exact typed build dependencies.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::{Error, JournalHeader, JournalInspection};
use crate::{
    publication::Key,
    render::MediaType,
    source::{CapturedSource, EntryPath, Sha256Digest},
};

/// One dependency namespace. Equal text in two namespaces is never interchangeable.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum BuildCacheDependencyKind {
    /// Exact retained source tree or captured generated input.
    Source,
    /// Exact effective workspace/build configuration.
    Configuration,
    /// Exact selected Theme template tree.
    ThemeTemplate,
    /// Exact compiler/parser/renderer adapter behavior profile.
    BuildMechanism,
    /// Exact admitted or explicitly selected semantic model/input.
    SelectedSemanticInput,
}

impl BuildCacheDependencyKind {
    /// Stable `SQLite` spelling for this typed namespace.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::Configuration => "configuration",
            Self::ThemeTemplate => "theme-template",
            Self::BuildMechanism => "build-mechanism",
            Self::SelectedSemanticInput => "selected-semantic-input",
        }
    }
}

/// One exact dependency identity. It contains no time or mutable remote observation.
#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct BuildCacheDependency {
    /// Namespace preventing same-word identities from aliasing.
    pub kind: BuildCacheDependencyKind,
    /// Stable logical identity within the namespace.
    pub identity: String,
    /// Exact deterministic value identity.
    pub sha256: Sha256Digest,
}

impl BuildCacheDependency {
    /// Bind a stable logical identity to exact bytes or a canonical typed projection.
    ///
    /// # Errors
    /// Refuses empty, non-ASCII or overlong logical identities.
    pub fn new(
        kind: BuildCacheDependencyKind,
        identity: impl Into<String>,
        sha256: Sha256Digest,
    ) -> Result<Self, Error> {
        let identity = identity.into();
        if identity.is_empty()
            || identity.len() > 512
            || !identity.bytes().all(|byte| byte.is_ascii_graphic())
        {
            return Err(Error::BuildCacheVerification);
        }
        Ok(Self {
            kind,
            identity,
            sha256,
        })
    }
}

/// Complete dependency closure for one site output tree.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(transparent)]
pub struct BuildCacheDependencySet(BTreeSet<BuildCacheDependency>);

impl BuildCacheDependencySet {
    /// Require every dependency class used by the Gazette 0.1 whole-site scheduler.
    ///
    /// # Errors
    /// Refuses missing classes and duplicate logical identities.
    pub fn new(values: impl IntoIterator<Item = BuildCacheDependency>) -> Result<Self, Error> {
        let mut identities = BTreeSet::new();
        let mut unique = BTreeSet::new();
        for value in values {
            if !identities.insert((value.kind, value.identity.clone())) || !unique.insert(value) {
                return Err(Error::BuildCacheVerification);
            }
        }
        let values = unique;
        let kinds: BTreeSet<_> = values.iter().map(|value| value.kind).collect();
        let required = BTreeSet::from([
            BuildCacheDependencyKind::Source,
            BuildCacheDependencyKind::Configuration,
            BuildCacheDependencyKind::ThemeTemplate,
            BuildCacheDependencyKind::BuildMechanism,
            BuildCacheDependencyKind::SelectedSemanticInput,
        ]);
        if values.is_empty() || kinds != required {
            return Err(Error::BuildCacheVerification);
        }
        Ok(Self(values))
    }

    /// Dependencies in canonical kind/identity/digest order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &BuildCacheDependency> {
        self.0.iter()
    }
}

/// One clean rendered site supplied to complete-set cache reconciliation.
pub struct BuildCacheSiteInput<'a> {
    /// Exact publication site.
    pub site: &'a Key,
    /// Complete typed dependency closure.
    pub dependencies: &'a BuildCacheDependencySet,
    /// Complete clean output tree.
    pub output: &'a CapturedSource,
    /// Producer-declared representations for generated files.
    pub media_types: &'a BTreeMap<EntryPath, MediaType>,
}

/// Exact complete successful site set. Omitted sites are stale and must be removed.
pub struct BuildCacheInput<'a> {
    /// Complete successful site set in caller-selected scope.
    pub sites: Vec<BuildCacheSiteInput<'a>>,
}

/// Independently loaded output for a dependency-exact warm hit.
#[derive(Debug)]
pub struct CachedBuildSite {
    /// Exact publication site.
    pub site: Key,
    /// Dependencies proven equal by native read-back.
    pub dependencies: BuildCacheDependencySet,
    /// Complete independently reconstructed cached output.
    pub output: CapturedSource,
    /// Producer-declared representations restored with the tree.
    pub media_types: BTreeMap<EntryPath, MediaType>,
}

/// Stored identity for one cached site without copying its payload into inspection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BuildCacheSite {
    /// Canonical complete dependency-set identity.
    pub dependency_sha256: Sha256Digest,
    /// Canonical output tree identity.
    pub output_sha256: Sha256Digest,
    /// Output inventory entry count.
    pub entries: u64,
    /// Output regular-file bytes.
    pub bytes: u64,
}

/// Complete replaceable cache projection identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BuildCacheHead {
    /// Canonical identity of every replaceable cache row.
    pub rows_sha256: Sha256Digest,
    /// Cached site count.
    pub sites: u64,
    /// Typed dependency row count.
    pub dependencies: u64,
    /// Cached regular-file count.
    pub artifacts: u64,
}

/// Read-only rebuildable cache state under a verified journal snapshot.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct BuildCacheInspection {
    /// Store/schema/integrity evidence.
    pub journal: JournalInspection,
    /// None only when the replaceable cache is empty.
    pub cache: Option<BuildCacheHead>,
}

/// Verified complete-set cache replacement.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct BuildCacheReceipt {
    /// Existing journal coordinates; cache replacement does not authorize release.
    pub journal: JournalHeader,
    /// Independently read-back replaceable projection.
    pub cache: BuildCacheHead,
}

/// Explicit permission for rebuildable cache replacement only.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct BuildCacheWriteOptions {
    /// Permit replacement and stale-row removal only.
    pub allow_replace: bool,
}

impl BuildCacheWriteOptions {
    /// Require explicit acknowledgement of complete-set stale-row removal.
    ///
    /// # Errors
    /// Refuses replacement unless the caller explicitly permits it.
    pub fn validate(self) -> Result<(), Error> {
        if !self.allow_replace {
            return Err(Error::BuildCachePermission);
        }
        Ok(())
    }
}
