//! Select native files from compiler-owned source roles and exact declarations.

use crate::configuration::LoadedWorkspace;
use crate::corpus::SourceKind;
use crate::workspace::{DocumentationNavigation, SourceLocation};
use gazette_core::documentation::Selection as DocumentationSelection;
use gazette_core::source::ListedEntry;

use super::*;

pub(super) struct Selected {
    pub state: Session,
    pub paths: BTreeMap<String, PathBuf>,
    pub candidates: Vec<Location>,
    pub resolved: bool,
}

#[derive(Clone)]
pub(super) struct Location {
    root: PathBuf,
    prefix: Option<String>,
    include: Vec<EntryPath>,
    exclude: Vec<EntryPath>,
}

impl Location {
    fn new(loaded: &LoadedWorkspace, key: &Key) -> Result<Self, Error> {
        let root = loaded
            .source_roots()
            .get(key)
            .ok_or(Error::Selection("selected source has no native root"))?;
        let root = normalized_root(root)?;
        let declaration = loaded
            .documents()
            .compilation
            .workspace
            .source(key)
            .ok_or(Error::Selection("selected source is undeclared"))?;
        let (prefix, include, exclude) = match &declaration.location {
            SourceLocation::Filesystem { subdirectory, .. }
            | SourceLocation::WorkingRepository { subdirectory, .. } => (
                subdirectory.as_ref().map(ToString::to_string),
                vec![],
                vec![],
            ),
            SourceLocation::Repository { subdirectory, .. } => {
                (Some(subdirectory.to_string()), vec![], vec![])
            }
            SourceLocation::Submodule {
                submodule_path,
                subdirectory,
                ..
            } => (
                Some(submodule_path.join(subdirectory).to_string()),
                vec![],
                vec![],
            ),
            SourceLocation::IncludedFilesystem { include, .. } => (None, include.clone(), vec![]),
            SourceLocation::FilteredFilesystem { exclude, .. } => (None, vec![], exclude.clone()),
        };
        Ok(Self {
            root,
            prefix,
            include,
            exclude,
        })
    }

    fn entry(&self, path: &EntryPath) -> Result<EntryPath, Error> {
        if !self.allows(path) {
            return Err(Error::Selection(
                "file is outside configured source selection",
            ));
        }
        match &self.prefix {
            Some(prefix) => format!("{prefix}/{path}")
                .parse()
                .map_err(|_| Error::Selection("invalid source-relative file")),
            None => Ok(path.clone()),
        }
    }

    fn allows(&self, path: &EntryPath) -> bool {
        let within = |selection: &EntryPath| {
            path == selection
                || path
                    .as_str()
                    .strip_prefix(selection.as_str())
                    .is_some_and(|tail| tail.starts_with('/'))
        };
        (self.include.is_empty() || self.include.iter().any(within))
            && !self.exclude.iter().any(within)
    }

    pub(super) fn contains(&self, path: &Path) -> bool {
        let base = self
            .prefix
            .as_ref()
            .map_or_else(|| self.root.clone(), |prefix| self.root.join(prefix));
        path.strip_prefix(base)
            .ok()
            .and_then(Path::to_str)
            .and_then(|path| path.parse::<EntryPath>().ok())
            .is_some_and(|path| self.allows(&path))
    }
}

fn normalized_root(path: &Path) -> Result<PathBuf, Error> {
    use std::path::Component;
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                if !normalized.pop() {
                    return Err(Error::Selection("source root escapes its native volume"));
                }
            }
            Component::CurDir => {}
            component => normalized.push(component.as_os_str()),
        }
    }
    if !normalized.is_absolute() {
        return Err(Error::Selection("source root must be absolute"));
    }
    if path.as_os_str() != normalized.as_os_str() {
        // A file URI removes dot segments. Refuse a symlink-dependent path whose
        // native meaning would change when represented by that client identifier.
        let original = path
            .canonicalize()
            .map_err(|_| Error::Selection("source root is unavailable"))?;
        let resolved = normalized
            .canonicalize()
            .map_err(|_| Error::Selection("normalized source root is unavailable"))?;
        if original != resolved {
            return Err(Error::Selection(
                "source root has ambiguous native and file-URI meanings",
            ));
        }
    }
    Ok(normalized)
}

#[derive(Clone, PartialEq, Eq)]
enum Role {
    Configuration,
    Content,
    Manual(Key),
    Package(authoring::PackageBaselineKey),
    Asset(u64),
}

struct File {
    root: PathBuf,
    entry: EntryPath,
    logical: EntryPath,
    role: Role,
    text: Option<String>,
}

pub(super) fn select(
    workspace: &Workspace,
    configuration: &configuration::Configuration,
) -> Result<Selected, Error> {
    if let Some(reason) = configuration.failure {
        return Err(Error::Selection(reason));
    }
    let Some(loaded) = &configuration.loaded else {
        return retain_invalid(workspace, &configuration.documents);
    };
    let mut files = BTreeMap::new();
    for document in &configuration.documents {
        insert(
            &mut files,
            File {
                root: workspace.root.clone(),
                entry: document.path.clone(),
                logical: document.path.clone(),
                role: Role::Configuration,
                text: Some(document.text.clone()),
            },
        )?;
    }
    let model = &loaded.documents().compilation.workspace;
    let discovered = crate::configuration::discover_sources(
        loaded,
        model.content_bindings().map(|binding| binding.source_key()),
        workspace.listing_limits(),
    )
    .map_err(|_| Error::Selection("selected publication sources could not be listed"))?;
    let mut candidates = Vec::new();
    for source in discovered {
        let location = Location::new(loaded, &source.source)?;
        for binding in model
            .content_bindings()
            .filter(|binding| binding.source_key() == &source.source)
        {
            for (path, entry) in source.listing.entries() {
                let ListedEntry::File { observed_bytes } = entry else {
                    continue;
                };
                let logical = binding
                    .logical_path(path)
                    .map_err(|_| Error::Selection("invalid publication source path"))?;
                let role = if binding.is_retained_only(&logical)
                    || SourceKind::for_path(path) == SourceKind::Asset
                {
                    Role::Asset(*observed_bytes)
                } else {
                    Role::Content
                };
                insert(
                    &mut files,
                    File {
                        root: location.root.clone(),
                        entry: location.entry(path)?,
                        logical,
                        role,
                        text: None,
                    },
                )?;
            }
        }
        candidates.push(location);
    }
    for binding in model.legal_sources() {
        let path = &binding
            .legal_binding()
            .expect("legal source has a binding")
            .entry;
        let location = Location::new(loaded, binding.source_key())?;
        insert(
            &mut files,
            File {
                root: location.root.clone(),
                entry: location.entry(path)?,
                logical: binding
                    .logical_path(path)
                    .map_err(|_| Error::Selection("invalid legal source path"))?,
                role: Role::Content,
                text: None,
            },
        )?;
    }
    for module in model
        .brands()
        .flat_map(|brand| &brand.publications)
        .filter_map(|publication| publication.documentation_repository.as_ref())
        .filter_map(|repository| repository.build.as_ref())
        .flat_map(|build| &build.modules)
    {
        let Some(manual) = &module.manual else {
            continue;
        };
        for navigation in &module.navigation {
            let DocumentationNavigation::Manual { pages, .. } = navigation else {
                continue;
            };
            for page in pages {
                let DocumentationSelection::File { path } = &page.input else {
                    return Err(Error::Selection(
                        "manual navigation must select exact files",
                    ));
                };
                exact(
                    &mut files,
                    loaded,
                    &manual.source,
                    path,
                    Role::Manual(manual.source.clone()),
                )?;
            }
        }
    }
    for manual in &workspace.manuals {
        exact(
            &mut files,
            loaded,
            &manual.source,
            &manual.path,
            Role::Manual(manual.source.clone()),
        )?;
    }
    for package in model.theme_packages() {
        exact(
            &mut files,
            loaded,
            &package.source,
            &package.manifest,
            Role::Package(authoring::PackageBaselineKey::Theme(
                package.package.clone(),
            )),
        )?;
    }
    for package in model.plugin_packages() {
        exact(
            &mut files,
            loaded,
            &package.source,
            &package.manifest,
            Role::Package(authoring::PackageBaselineKey::Plugin(
                package.package.clone(),
            )),
        )?;
    }
    if files.len() > workspace.limits.documents {
        return Err(Error::Limit("selected documents"));
    }
    capture(workspace, &mut files)?;
    finish(workspace, files, candidates)
}

fn exact(
    files: &mut BTreeMap<PathBuf, File>,
    loaded: &LoadedWorkspace,
    source: &Key,
    path: &EntryPath,
    role: Role,
) -> Result<(), Error> {
    let location = Location::new(loaded, source)?;
    // Different packages commonly use the same manifest filename. Their editor
    // paths are package-qualified; the compiler still resolves each typed key
    // through its configured source and exact manifest path.
    let logical = match &role {
        Role::Package(package) => {
            let package = match package {
                authoring::PackageBaselineKey::Theme(package) => package.as_str(),
                authoring::PackageBaselineKey::Plugin(package) => package.as_str(),
            };
            format!("__packages/{package}/{path}")
                .parse()
                .map_err(|_| Error::Selection("invalid package editor path"))?
        }
        _ => path.clone(),
    };
    insert(
        files,
        File {
            root: location.root.clone(),
            entry: location.entry(path)?,
            logical,
            role,
            text: None,
        },
    )
}

fn insert(files: &mut BTreeMap<PathBuf, File>, file: File) -> Result<(), Error> {
    let path = file.root.join(file.entry.as_str());
    if let Some(previous) = files.get(&path) {
        if previous.logical == file.logical && previous.role == file.role {
            return Ok(());
        }
        return Err(Error::Selection(
            "one native file has conflicting logical paths or roles",
        ));
    }
    files.insert(path, file);
    Ok(())
}

fn capture(workspace: &Workspace, files: &mut BTreeMap<PathBuf, File>) -> Result<(), Error> {
    let mut groups: BTreeMap<PathBuf, Vec<EntryPath>> = BTreeMap::new();
    let mut limits = workspace.source_limits();
    for file in files.values() {
        if let Some(text) = &file.text {
            limits.total_bytes = limits
                .total_bytes
                .checked_sub(text.len() as u64)
                .ok_or(Error::Limit("retained source bytes"))?;
        } else if !matches!(file.role, Role::Asset(_)) {
            groups
                .entry(file.root.clone())
                .or_default()
                .push(file.entry.clone());
        }
    }
    for (root, paths) in groups {
        let captured = crate::configuration::capture_selected(&root, &paths, limits)
            .map_err(|_| Error::Selection("selected authoring text could not be read"))?;
        limits.total_bytes = limits
            .total_bytes
            .checked_sub(captured.inventory().total_bytes())
            .ok_or(Error::Limit("retained source bytes"))?;
        for path in paths {
            let text = std::str::from_utf8(
                captured
                    .file(&path)
                    .ok_or(Error::Selection("selected text is absent"))?,
            )
            .map_err(|_| Error::Selection("selected authoring text must be UTF-8"))?;
            files
                .get_mut(&root.join(path.as_str()))
                .expect("selected capture was planned")
                .text = Some(text.to_owned());
        }
    }
    Ok(())
}

fn finish(
    workspace: &Workspace,
    files: BTreeMap<PathBuf, File>,
    candidates: Vec<Location>,
) -> Result<Selected, Error> {
    let mut root = None;
    let (mut configurations, mut documents, mut manuals, mut packages, mut assets) =
        (vec![], vec![], vec![], vec![], vec![]);
    let mut paths = BTreeMap::new();
    for (native, file) in files {
        let id = (workspace.identifier)(&native).map_err(Error::Selection)?;
        if paths.insert(id.clone(), native).is_some() {
            return Err(Error::Selection("native file identifiers must be unique"));
        }
        if let Role::Asset(observed_bytes) = file.role {
            assets.push(AssetObservationSource {
                id,
                path: file.logical,
                observed_bytes,
            });
            continue;
        }
        let input = Source {
            id,
            path: file.logical,
            text: file
                .text
                .ok_or(Error::Selection("selected text was not captured"))?,
        };
        match file.role {
            Role::Configuration if input.path == workspace.path => root = Some(input),
            Role::Configuration => configurations.push(input),
            Role::Content => documents.push(input),
            Role::Manual(source) => manuals.push(ManualSource { source, input }),
            Role::Package(package) => packages.push(PackageSource { package, input }),
            Role::Asset(_) => unreachable!(),
        }
    }
    let state = Session::new_with_workspace_documents_manuals_page_inputs(
        root.ok_or(Error::Selection("workspace root is absent"))?,
        configurations,
        documents,
        manuals,
        [],
        packages,
        workspace.limits,
    )?
    .with_asset_observations(assets)?;
    Ok(Selected {
        state,
        paths,
        candidates,
        resolved: true,
    })
}

pub(super) fn retain_invalid(
    workspace: &Workspace,
    configurations: &[Source],
) -> Result<Selected, Error> {
    let mut files = BTreeMap::new();
    for input in configurations {
        insert(
            &mut files,
            File {
                root: workspace.root.clone(),
                entry: input.path.clone(),
                logical: input.path.clone(),
                role: Role::Configuration,
                text: Some(input.text.clone()),
            },
        )?;
    }
    let state = workspace.session();
    for (id, document) in &state.files {
        let native = &workspace.paths[id];
        if files.contains_key(native) {
            continue;
        }
        let role = if id == &state.workspace
            || state
                .workspace_documents
                .as_ref()
                .is_some_and(|ids| ids.contains(id))
        {
            Role::Configuration
        } else if let Some(source) = state.manuals.get(id) {
            Role::Manual(source.clone())
        } else if let Some(package) = state.package_baselines.get(id) {
            Role::Package(package.clone())
        } else {
            Role::Content
        };
        files.insert(
            native.clone(),
            File {
                root: workspace.root.clone(),
                entry: document.path.clone(),
                logical: document.path.clone(),
                role,
                text: Some(document.baseline.clone()),
            },
        );
    }
    for (id, asset) in &state.asset_observations {
        files.insert(
            workspace.paths[id].clone(),
            File {
                root: workspace.root.clone(),
                entry: asset.path().clone(),
                logical: asset.path().clone(),
                role: Role::Asset(asset.observed_bytes()),
                text: None,
            },
        );
    }
    let mut selected = finish(workspace, files, workspace.candidates.clone())?;
    selected.resolved = false;
    Ok(selected)
}
