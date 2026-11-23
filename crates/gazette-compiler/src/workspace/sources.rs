//! Exact source declarations reuse the native coordinate and path parsers.

use std::{collections::BTreeSet, fmt, fmt::Display, str::FromStr};

use gazette_core::source::{CommitId, EntryPath, Subdirectory, TargetPath};
use serde::{Deserialize, Deserializer, Serialize, de::Error};

use super::{Key, LegalBinding, NativeMarkdown, Workspace};

#[derive(Clone, Copy, Debug)]
enum ContentAssociation<'a> {
    Tree(&'a ContentBinding),
    Legal(&'a LegalBinding),
}

fn legal_source_component(key: &Key) -> String {
    use std::fmt::Write;
    let mut component = String::new();
    for byte in key.as_str().bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'-' | b'_' | b'.') {
            component.push(char::from(byte));
        } else {
            write!(&mut component, "%{byte:02X}").expect("string write");
        }
    }
    component
}

/// A configured content-source relationship, not authorship or subject matter.
///
/// Public projections carry only site/project identities. Native source coordinates
/// stay in the private binding and acquisition evidence, never template data.
#[derive(Clone, Copy, Debug, Serialize, schemars::JsonSchema)]
pub struct ContentSource<'a> {
    /// Publication selected by the enclosing declaration.
    pub site: &'a Key,
    /// Native project context; absence identifies publication-authored input.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<&'a Key>,
    /// Explicit Theme documentation mount, retained privately rather than a fake project.
    #[serde(skip)]
    #[schemars(skip)]
    pub documentation_mount: Option<&'a super::ResolvedDocumentationMount>,
    /// Exact source role; legal documents are not authored-tree placements.
    #[serde(skip)]
    #[schemars(skip)]
    binding: ContentAssociation<'a>,
    /// Selected native lowering defaults, retained privately with the declaration.
    #[serde(skip)]
    #[schemars(skip)]
    pub native_markdown: Option<&'a NativeMarkdown>,
    /// Literal source selection for automatic indexes, independent of materialization.
    #[serde(skip)]
    #[schemars(skip)]
    pub discoverable: bool,
}

impl<'a> ContentSource<'a> {
    /// Whether source-relative native documentation grammar applies to this exact role.
    pub fn is_documentation(&self) -> bool {
        self.project.is_some() || self.documentation_mount.is_some()
    }
    /// Exact capture key, independent of output identity and public address.
    pub fn source_key(&self) -> &'a Key {
        match self.binding {
            ContentAssociation::Tree(binding) => &binding.source,
            ContentAssociation::Legal(binding) => &binding.source,
        }
    }

    /// Literal authored/rendered tree declaration; legal sources have none.
    pub fn tree_binding(&self) -> Option<&'a ContentBinding> {
        match self.binding {
            ContentAssociation::Tree(binding) => Some(binding),
            ContentAssociation::Legal(_) => None,
        }
    }

    /// Exact private retention disposition; neither a filename heuristic nor output permission.
    pub fn is_retained_only(&self, path: &EntryPath) -> bool {
        self.tree_binding()
            .zip(self.entry(path))
            .is_some_and(|(binding, entry)| binding.retained_only.contains(entry))
    }

    /// Literal legal-output declaration; authored/rendered trees have none.
    pub fn legal_binding(&self) -> Option<&'a LegalBinding> {
        match self.binding {
            ContentAssociation::Legal(binding) => Some(binding),
            ContentAssociation::Tree(_) => None,
        }
    }

    /// Map a literal captured entry to its private logical corpus coordinate.
    /// Legal bindings select exactly one document, never its siblings or assets.
    ///
    /// # Errors
    /// Refuses an unselected legal entry or invalid logical coordinate.
    pub fn logical_path(
        &self,
        entry: &EntryPath,
    ) -> Result<EntryPath, gazette_core::source::Error> {
        match self.binding {
            ContentAssociation::Tree(binding) => format!("{}/{entry}", binding.target).parse(),
            ContentAssociation::Legal(binding) if entry == &binding.entry => format!(
                "__legal/{}/{entry}",
                legal_source_component(&binding.source)
            )
            .parse(),
            ContentAssociation::Legal(_) => Err(gazette_core::source::Error::InvalidInventory {
                path: entry.as_str().into(),
                reason: "legal source selects only its explicit document entry",
            }),
        }
    }

    /// Recover the literal capture-relative entry without URL decoding or normalization.
    pub fn entry<'p>(&self, path: &'p EntryPath) -> Option<&'p str> {
        match self.binding {
            ContentAssociation::Tree(binding) => path
                .as_str()
                .strip_prefix(binding.target.as_str())?
                .strip_prefix('/')
                .filter(|entry| !entry.is_empty()),
            ContentAssociation::Legal(binding) => path
                .as_str()
                .strip_prefix("__legal/")?
                .strip_prefix(legal_source_component(&binding.source).as_str())?
                .strip_prefix('/')
                .filter(|entry| *entry == binding.entry.as_str()),
        }
    }
}

impl Workspace {
    /// Resolve a source-relative retention disposition for resource admission.
    /// Conflicting bindings are rejected during native workspace compilation.
    pub fn is_retained_only(&self, source: &Key, entry: &EntryPath) -> bool {
        self.content_bindings().any(|binding| {
            binding.source_key() == source
                && binding
                    .tree_binding()
                    .is_some_and(|binding| binding.retained_only.contains(entry))
        })
    }

    /// One role-aware view of all configured publication input, excluding disabled docs.
    ///
    /// Authored sources and rendered project placements share capture/mapping rules,
    /// but retain their different provenance. Selection is not public admission.
    pub fn content_bindings(&self) -> impl Iterator<Item = ContentSource<'_>> {
        self.brands()
            .flat_map(|brand| &brand.publications)
            .flat_map(|publication| {
                publication.authored.iter().map(|binding| ContentSource {
                    site: &publication.site,
                    project: None,
                    documentation_mount: None,
                    binding: ContentAssociation::Tree(binding),
                    native_markdown: None,
                    discoverable: true,
                })
            })
            .chain(
                self.placements()
                    .iter()
                    .filter(|placement| placement.render)
                    .filter_map(|placement| {
                        placement.documents.as_ref().map(|binding| ContentSource {
                            site: &placement.site,
                            project: Some(&placement.project),
                            documentation_mount: None,
                            binding: ContentAssociation::Tree(binding),
                            native_markdown: placement.native_markdown.as_ref(),
                            discoverable: placement.discoverable,
                        })
                    }),
            )
            .chain(
                self.documentation_mounts()
                    .iter()
                    .filter(|mount| mount.declaration.render)
                    .map(|mount| ContentSource {
                        site: &mount.declaration.site,
                        project: None,
                        documentation_mount: Some(mount),
                        binding: ContentAssociation::Tree(&mount.binding),
                        native_markdown: mount.native_markdown.as_ref(),
                        discoverable: mount.declaration.discoverable,
                    }),
            )
    }

    /// Explicit legal output/source associations, never authored-tree placement views.
    pub fn legal_sources(&self) -> impl Iterator<Item = ContentSource<'_>> {
        self.brands()
            .flat_map(|brand| &brand.publications)
            .flat_map(|publication| {
                publication.legal.iter().map(|binding| ContentSource {
                    site: &publication.site,
                    project: None,
                    documentation_mount: None,
                    binding: ContentAssociation::Legal(binding),
                    native_markdown: None,
                    discoverable: true,
                })
            })
    }

    /// Every explicit output referencing this exact retained private legal document.
    pub fn legal_bindings<'a>(
        &'a self,
        path: &'a EntryPath,
    ) -> impl Iterator<Item = ContentSource<'a>> {
        self.legal_sources()
            .filter(move |source| source.entry(path).is_some())
    }

    /// Exact logical-file origin. The compiler rejects ambiguous prefixes beforehand.
    /// Missing means unbound, not a guessed owner from the filename or public route.
    pub fn content_source(&self, path: &EntryPath) -> Option<ContentSource<'_>> {
        self.content_bindings()
            .find(|source| source.entry(path).is_some())
    }

    /// Resolve syntax from the exact configured content placement. Unbound editor
    /// input uses ordinary Markdown without acquiring a placement or permission.
    pub fn markdown_profile(&self, path: &EntryPath) -> gazette_core::markdown::Profile {
        self.content_source(path)
            .and_then(|source| {
                source
                    .tree_binding()
                    .map(|binding| binding.markdown_profile)
            })
            .unwrap_or_default()
    }
}

/// A reusable source binding, distinct from a project's native identity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Source {
    /// Exact configuration binding key.
    pub id: Key,
    /// Explicit native selection; unavailable input never selects a fallback.
    pub location: SourceLocation,
}

/// Portable source-root spelling retained in workspace configuration.
///
/// `.` selects the workspace root, a validated relative spelling selects a
/// descendant of that root, `machine:name[/path]` selects a machine-bound root,
/// and an absolute spelling deliberately selects an external native root.
/// Resolution to a native path belongs to the application
/// after one workspace root has been selected; the compiler never consults the
/// process current directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceRoot {
    /// The selected workspace root itself.
    Workspace,
    /// One confined descendant of the selected workspace root.
    WorkspaceRelative(Subdirectory),
    /// A named root from the workspace's explicit machine binding, with an optional descendant.
    Machine {
        /// Name in the binding's `roots` table.
        name: String,
        /// Confined path beneath the named root.
        path: Option<Subdirectory>,
    },
    /// One explicitly selected absolute native root.
    External(String),
}

/// A source root cannot be interpreted without ambient path normalization.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidSourceRoot;

impl fmt::Display for InvalidSourceRoot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(
            "source root must be '.', a confined workspace-relative path, 'machine:name[/path]', or an absolute native directory without dot components",
        )
    }
}

impl std::error::Error for InvalidSourceRoot {}

impl FromStr for SourceRoot {
    type Err = InvalidSourceRoot;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value == "." {
            return Ok(Self::Workspace);
        }
        if let Some(coordinate) = value.strip_prefix("machine:") {
            let (name, path) = coordinate
                .split_once('/')
                .map_or((coordinate, None), |(name, path)| (name, Some(path)));
            if !valid_machine_root_name(name) {
                return Err(InvalidSourceRoot);
            }
            return Ok(Self::Machine {
                name: name.to_owned(),
                path: path
                    .map(str::parse)
                    .transpose()
                    .map_err(|_| InvalidSourceRoot)?,
            });
        }
        if super::paths::absolute_directory(value) {
            return Ok(Self::External(value.to_owned()));
        }
        value
            .parse()
            .map(Self::WorkspaceRelative)
            .map_err(|_| InvalidSourceRoot)
    }
}

impl SourceRoot {
    /// Relative selection beneath the workspace root, or `None` for other root kinds.
    pub fn workspace_relative(&self) -> Option<Option<&Subdirectory>> {
        match self {
            Self::Workspace => Some(None),
            Self::WorkspaceRelative(path) => Some(Some(path)),
            Self::External(_) | Self::Machine { .. } => None,
        }
    }

    /// Explicit absolute native spelling, or `None` for a portable root.
    pub fn external(&self) -> Option<&str> {
        match self {
            Self::External(path) => Some(path),
            Self::Workspace | Self::WorkspaceRelative(_) | Self::Machine { .. } => None,
        }
    }
}

fn valid_machine_root_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.as_bytes()[0].is_ascii_alphabetic()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

impl fmt::Display for SourceRoot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Workspace => formatter.write_str("."),
            Self::WorkspaceRelative(path) => path.fmt(formatter),
            Self::Machine { name, path } => {
                write!(formatter, "machine:{name}")?;
                if let Some(path) = path {
                    write!(formatter, "/{path}")?;
                }
                Ok(())
            }
            Self::External(path) => formatter.write_str(path),
        }
    }
}

impl Serialize for SourceRoot {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for SourceRoot {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(D::Error::custom)
    }
}

impl schemars::JsonSchema for SourceRoot {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "SourceRoot".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "description": "'.', a confined workspace-relative path, 'machine:name[/path]', or an explicit absolute native directory; never resolved against the process current directory"
        })
    }
}

/// One exact policy document in an existing source capture. It creates no content
/// placement, presentation manifest, execution permission or policy authority.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PolicySource {
    /// Existing configured source key, whose native pin remains owned by that declaration.
    pub source: Key,
    /// Literal capture-relative regular file; existence is checked after acquisition.
    #[serde(deserialize_with = "decode")]
    pub entry: EntryPath,
}

/// Explicit native source choice; no moving references, remote fetches or inferred fallback.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum SourceLocation {
    /// One bounded local tree captured as retained bytes without repository lineage.
    Filesystem {
        /// Portable workspace-relative or explicit external native root.
        root: SourceRoot,
        /// Optional literal selection beneath the root; absence selects the whole root.
        #[serde(
            default,
            deserialize_with = "decode_optional_subdirectory",
            skip_serializing_if = "Option::is_none"
        )]
        subdirectory: Option<Subdirectory>,
    },
    /// A local tree with explicit capture-relative subtree exclusions; no implicit ignores.
    FilteredFilesystem {
        /// Explicit root, with the same confinement as an ordinary filesystem source.
        root: SourceRoot,
        /// Literal paths excluded before opening their descendants.
        #[schemars(length(min = 1, max = 128))]
        #[serde(deserialize_with = "decode_selection_paths")]
        exclude: Vec<EntryPath>,
    },
    /// Only the named files or directories and their descendants.
    IncludedFilesystem {
        /// Root against which every included path is resolved.
        root: SourceRoot,
        /// Required literal paths; a missing selection fails acquisition.
        #[schemars(length(min = 1, max = 128))]
        #[serde(deserialize_with = "decode_selection_paths")]
        include: Vec<EntryPath>,
        /// Optional checked-out branch requirement; working bytes remain unpinned.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_branch: Option<String>,
    },
    /// Working files tracked by Git or not ignored beneath one repository folder.
    WorkingRepository {
        /// Exact repository root; parent repositories are never searched.
        root: SourceRoot,
        /// Optional folder; absence selects the exact repository root.
        #[serde(
            default,
            deserialize_with = "decode_optional_subdirectory",
            skip_serializing_if = "Option::is_none"
        )]
        subdirectory: Option<Subdirectory>,
        /// Optional branch checked around capture, without pinning working bytes.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_branch: Option<String>,
    },
    /// Selected files in one explicitly pinned repository.
    Repository {
        /// Portable workspace-relative or explicit external native root.
        root: SourceRoot,
        /// Exact checked-out commit.
        #[serde(deserialize_with = "decode")]
        commit: CommitId,
        /// Literal selection beneath the repository; its consumer supplies the content role.
        #[serde(deserialize_with = "decode")]
        subdirectory: Subdirectory,
    },
    /// Selected files in a child selected through an exact parent gitlink.
    Submodule {
        /// Portable workspace-relative or explicit external native parent root.
        root: SourceRoot,
        /// Exact parent commit; the child pin is derived, not independently chosen.
        #[serde(deserialize_with = "decode")]
        parent_commit: CommitId,
        /// Literal child location beneath the parent.
        #[serde(deserialize_with = "decode")]
        submodule_path: Subdirectory,
        /// Literal selection beneath the child, not inferred from a provider name.
        #[serde(deserialize_with = "decode")]
        subdirectory: Subdirectory,
    },
}

impl SourceLocation {
    /// Configured root spelling, not native existence or reachability evidence.
    pub fn root(&self) -> &SourceRoot {
        match self {
            Self::Filesystem { root, .. }
            | Self::FilteredFilesystem { root, .. }
            | Self::IncludedFilesystem { root, .. }
            | Self::WorkingRepository { root, .. }
            | Self::Repository { root, .. }
            | Self::Submodule { root, .. } => root,
        }
    }
}

/// Exact source mapped beneath a logical corpus prefix. The owning field supplies
/// the authored-publication or project-documentation role; neither implies admission.
/// Every binding selected by the current Engine proposal is acquired as a mandatory
/// complete-set input. Website's namespaced required/optional materialization policy
/// is deliberately not represented until its owner publishes the exact projection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ContentBinding {
    /// Exact source binding key.
    pub source: Key,
    /// Normalized logical target, not destination-filesystem admission.
    #[serde(deserialize_with = "decode")]
    pub target: TargetPath,
    /// Syntax selected for Markdown in this source placement. Selection neither
    /// invokes a typesetter nor grants generated fragments publication permission.
    #[serde(default)]
    pub markdown_profile: gazette_core::markdown::Profile,
    /// Explicit destination for supported byte-preserving static companions.
    /// This selects local transport, not capability execution or public admission.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub static_assets: Option<StaticAssets>,
    /// Exact source-relative regular opaque files retained for evidence only.
    /// They cannot become content, companions, resources or linked publication assets.
    #[serde(
        default,
        deserialize_with = "retained_paths",
        skip_serializing_if = "BTreeSet::is_empty"
    )]
    pub retained_only: BTreeSet<EntryPath>,
}

fn retained_paths<'de, D: Deserializer<'de>>(decoder: D) -> Result<BTreeSet<EntryPath>, D::Error> {
    let mut paths = BTreeSet::new();
    for path in Vec::<EntryPath>::deserialize(decoder)? {
        if !paths.insert(path) {
            return Err(D::Error::custom(
                "retained-only entries must be unique exact source-relative paths",
            ));
        }
    }
    Ok(paths)
}

/// One asset destination per content binding, shared by references and output assembly.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StaticAssets {
    /// Directory address beneath the publication base, not a native filesystem path.
    pub route_prefix: gazette_core::publication::routing::RoutePath,
}

fn decode<'de, T, D>(deserializer: D) -> Result<T, D::Error>
where
    T: FromStr,
    T::Err: Display,
    D: Deserializer<'de>,
{
    String::deserialize(deserializer)?
        .parse()
        .map_err(D::Error::custom)
}

fn decode_optional_subdirectory<'de, D>(deserializer: D) -> Result<Option<Subdirectory>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer)?
        .map(|value| value.parse().map_err(D::Error::custom))
        .transpose()
}

fn decode_selection_paths<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<EntryPath>, D::Error> {
    let paths = Vec::<EntryPath>::deserialize(deserializer)?;
    if paths.is_empty()
        || paths.len() > 128
        || paths.iter().collect::<BTreeSet<_>>().len() != paths.len()
    {
        return Err(D::Error::custom(
            "source selections must contain 1–128 unique relative paths",
        ));
    }
    Ok(paths)
}
