//! Explicit native locators, lowered once into the shared retained-source session.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use gazette_sdk::{
    authoring::{self, session},
    configuration::{EntryPath, SourceLimits},
    workspace::Key,
};
use lsp_types::{InitializeParams, Uri};
use serde::Deserialize;

use crate::Limits;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Initialization {
    native_selection: Input,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Input {
    Rooted(Rooted),
    Local(Local),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Root {
    id: String,
    uri: Uri,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Locator {
    root: String,
    entry: EntryPath,
    path: EntryPath,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Rooted {
    #[serde(default)]
    allow_read: bool,
    roots: Vec<Root>,
    workspace: Locator,
    workspace_documents: Vec<Locator>,
    #[serde(default)]
    documents: Vec<Locator>,
    #[serde(default)]
    manuals: Vec<Manual>,
    #[serde(default)]
    page_inputs: Vec<Locator>,
    #[serde(default)]
    package_baselines: Vec<Package>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manual {
    source: Key,
    root: String,
    entry: EntryPath,
    path: EntryPath,
}

impl Manual {
    fn locator(&self) -> Locator {
        Locator {
            root: self.root.clone(),
            entry: self.entry.clone(),
            path: self.path.clone(),
        }
    }
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Package {
    Theme {
        package: gazette_sdk::workspace::ThemePackageId,
        root: String,
        entry: EntryPath,
        path: EntryPath,
    },
    Plugin {
        package: gazette_sdk::workspace::PluginPackageId,
        root: String,
        entry: EntryPath,
        path: EntryPath,
    },
}

impl Package {
    fn locator(&self) -> Locator {
        let (root, entry, path) = match self {
            Self::Theme {
                root, entry, path, ..
            }
            | Self::Plugin {
                root, entry, path, ..
            } => (root, entry, path),
        };
        Locator {
            root: root.clone(),
            entry: entry.clone(),
            path: path.clone(),
        }
    }
    fn key(&self) -> authoring::PackageBaselineKey {
        match self {
            Self::Theme { package, .. } => authoring::PackageBaselineKey::Theme(package.clone()),
            Self::Plugin { package, .. } => authoring::PackageBaselineKey::Plugin(package.clone()),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Local {
    #[serde(default)]
    allow_read: bool,
    workspace: EntryPath,
    #[serde(default)]
    documents: Vec<EntryPath>,
    #[serde(default)]
    manuals: Vec<LocalManual>,
    #[serde(default)]
    page_inputs: Vec<EntryPath>,
    #[serde(default)]
    package_baselines: Vec<LocalPackage>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LocalManual {
    source: Key,
    path: EntryPath,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum LocalPackage {
    Theme {
        package: gazette_sdk::workspace::ThemePackageId,
        path: EntryPath,
    },
    Plugin {
        package: gazette_sdk::workspace::PluginPackageId,
        path: EntryPath,
    },
}

pub(super) fn select(
    params: &InitializeParams,
    options: serde_json::Value,
    limits: Limits,
) -> Result<session::Session, String> {
    let input: Initialization =
        serde_json::from_value(options).map_err(|_| "invalid native-source initialization")?;
    let (selection, editor) = match input.native_selection {
        Input::Rooted(selection) => (selection, true),
        Input::Local(selection) => {
            let root = native_root(params)?;
            let uri = url::Url::from_directory_path(root)
                .map_err(|()| "invalid native root URI")?
                .as_str()
                .parse()
                .map_err(|_| "invalid native root URI")?;
            let locator = |path: EntryPath| Locator {
                root: "workspace".into(),
                entry: path.clone(),
                path,
            };
            (
                Rooted {
                    allow_read: selection.allow_read,
                    roots: vec![Root {
                        id: "workspace".into(),
                        uri,
                    }],
                    workspace: locator(selection.workspace),
                    workspace_documents: Vec::new(),
                    documents: selection.documents.into_iter().map(locator).collect(),
                    manuals: selection
                        .manuals
                        .into_iter()
                        .map(|manual| Manual {
                            source: manual.source,
                            root: "workspace".into(),
                            entry: manual.path.clone(),
                            path: manual.path,
                        })
                        .collect(),
                    page_inputs: selection.page_inputs.into_iter().map(locator).collect(),
                    package_baselines: selection
                        .package_baselines
                        .into_iter()
                        .map(|package| match package {
                            LocalPackage::Theme { package, path } => Package::Theme {
                                package,
                                root: "workspace".into(),
                                entry: path.clone(),
                                path,
                            },
                            LocalPackage::Plugin { package, path } => Package::Plugin {
                                package,
                                root: "workspace".into(),
                                entry: path.clone(),
                                path,
                            },
                        })
                        .collect(),
                },
                false,
            )
        }
    };
    capture(params, selection, editor, limits)
}

#[allow(deprecated)]
fn capture(
    params: &InitializeParams,
    selection: Rooted,
    editor: bool,
    limits: Limits,
) -> Result<session::Session, String> {
    if !selection.allow_read {
        return Err("native selection requires allow_read=true".into());
    }
    let roots = roots(params, &selection, editor, limits)?;
    let config = roots
        .get(&selection.workspace.root)
        .ok_or("unknown selected configuration root")?;
    if selection.workspace.entry != selection.workspace.path {
        return Err("workspace logical path must match its native entry".into());
    }
    if editor
        && params
            .root_uri
            .as_ref()
            .map(file_path)
            .transpose()?
            .is_some_and(|root| root != *config)
    {
        return Err("rootUri and selected configuration root differ".into());
    }
    for module in &selection.workspace_documents {
        if module.root != selection.workspace.root || module.entry != module.path {
            return Err(
                "configuration modules must retain exact entries under the configuration root"
                    .into(),
            );
        }
    }
    let manuals: Vec<_> = selection.manuals.iter().map(Manual::locator).collect();
    let packages: Vec<_> = selection
        .package_baselines
        .iter()
        .map(Package::locator)
        .collect();
    let all: Vec<_> = std::iter::once(&selection.workspace)
        .chain(&selection.workspace_documents)
        .chain(&selection.documents)
        .chain(&manuals)
        .chain(&selection.page_inputs)
        .chain(&packages)
        .collect();
    if all.len() > limits.documents {
        return Err("selected documents ceiling exceeded".into());
    }
    let mut physical = BTreeSet::new();
    let mut path_bytes = 0usize;
    let mut uri_bytes = 0usize;
    for locator in &all {
        let root = roots
            .get(&locator.root)
            .ok_or("unknown selected native root")?;
        let path = root.join(locator.entry.as_str());
        if !physical.insert(path.clone()) {
            return Err("duplicate selected native file across roles or roots".into());
        }
        if editor {
            let owner = params
                .workspace_folders
                .as_deref()
                .unwrap_or_default()
                .iter()
                .filter_map(|folder| file_path(&folder.uri).ok())
                .filter(|folder| path.starts_with(folder))
                .max_by_key(|folder| folder.components().count());
            if owner.as_ref() != Some(root) {
                return Err("selected native file has a different workspace-folder owner".into());
            }
        }
        path_bytes = path_bytes
            .checked_add(locator.entry.as_str().len())
            .and_then(|bytes| bytes.checked_add(locator.path.as_str().len()))
            .filter(|bytes| *bytes <= limits.authoring.corpus.path_bytes)
            .ok_or("selected path bytes ceiling exceeded")?;
        uri_bytes = uri_bytes
            .checked_add(native_uri(root, &locator.entry)?.as_str().len())
            .filter(|bytes| *bytes <= limits.uri_bytes)
            .ok_or("selected identifier bytes ceiling exceeded")?;
    }
    let (_, mut remaining) = native_limits(limits)?;
    let (workspace, workspace_documents) = configuration(
        config,
        &selection,
        editor,
        limits,
        &mut remaining,
        &mut physical,
    )?;
    workspace_documents
        .len()
        .checked_add(1)
        .and_then(|count| count.checked_add(selection.documents.len()))
        .and_then(|count| count.checked_add(manuals.len()))
        .and_then(|count| count.checked_add(selection.page_inputs.len()))
        .and_then(|count| count.checked_add(packages.len()))
        .filter(|count| *count <= limits.documents)
        .ok_or("selected documents ceiling exceeded")?;
    let mut groups: BTreeMap<&str, Vec<EntryPath>> = BTreeMap::new();
    for locator in selection
        .documents
        .iter()
        .chain(&manuals)
        .chain(&selection.page_inputs)
        .chain(&packages)
    {
        groups
            .entry(&locator.root)
            .or_default()
            .push(locator.entry.clone());
    }
    let mut retained = BTreeMap::new();
    for (id, entries) in groups {
        let root = roots.get(id).ok_or("unknown selected native root")?;
        let opaque = selection.page_inputs.iter().any(|input| input.root == id);
        let file_bytes = if opaque {
            remaining.total_bytes
        } else {
            u64::try_from(
                limits
                    .authoring
                    .corpus
                    .document
                    .source_bytes
                    .max(limits.syntax.source_bytes),
            )
            .map_err(|_| "native file byte ceiling is not representable")?
        };
        let captured = gazette_sdk::configuration::capture_selected(
            root,
            &entries,
            SourceLimits {
                file_bytes,
                ..remaining
            },
        )
        .map_err(|_| "selected native documents could not be captured")?;
        debit(&mut remaining, captured.inventory())?;
        for (entry, bytes) in captured.into_parts().1.into_files() {
            retained.insert((id.to_owned(), entry), bytes);
        }
    }
    let mut take = |locator: &Locator| {
        retained
            .remove(&(locator.root.clone(), locator.entry.clone()))
            .ok_or("selected native document is unavailable")
    };
    let documents = selection
        .documents
        .iter()
        .map(|locator| {
            bounded_text_source(
                &roots,
                locator,
                take(locator)?,
                limits.authoring.corpus.document.source_bytes,
            )
        })
        .collect::<Result<Vec<_>, &'static str>>()?;
    let manuals = selection
        .manuals
        .iter()
        .zip(&manuals)
        .map(|(manual, locator)| {
            Ok(session::ManualSource {
                source: manual.source.clone(),
                input: bounded_text_source(
                    &roots,
                    locator,
                    take(locator)?,
                    limits.authoring.corpus.document.source_bytes,
                )?,
            })
        })
        .collect::<Result<Vec<_>, &'static str>>()?;
    let page_inputs = selection
        .page_inputs
        .iter()
        .map(|locator| {
            Ok(session::PageInputSource {
                id: native_uri(
                    roots
                        .get(&locator.root)
                        .ok_or("unknown selected native root")?,
                    &locator.entry,
                )?
                .as_str()
                .to_owned(),
                path: locator.path.clone(),
                bytes: take(locator)?,
            })
        })
        .collect::<Result<Vec<_>, &'static str>>()?;
    let package_baselines = selection
        .package_baselines
        .iter()
        .zip(&packages)
        .map(|(package, locator)| {
            Ok(session::PackageSource {
                package: package.key(),
                input: bounded_text_source(
                    &roots,
                    locator,
                    take(locator)?,
                    limits.syntax.source_bytes,
                )?,
            })
        })
        .collect::<Result<Vec<_>, &'static str>>()?;
    super::select_retained(
        workspace,
        Some(workspace_documents),
        documents,
        manuals,
        page_inputs,
        package_baselines,
        limits,
    )
    .map_err(str::to_owned)
}

fn configuration(
    config: &Path,
    selection: &Rooted,
    editor: bool,
    limits: Limits,
    remaining: &mut SourceLimits,
    physical: &mut BTreeSet<PathBuf>,
) -> Result<(session::Source, Vec<session::Source>), String> {
    if editor {
        // Explicit editor selections retain invalid syntax/semantics for repair.
        // Native analysis, not this transport, owns includes and complete-set validity.
        let locators: Vec<_> = std::iter::once(&selection.workspace)
            .chain(&selection.workspace_documents)
            .collect();
        let entries: Vec<_> = locators
            .iter()
            .map(|locator| locator.entry.clone())
            .collect();
        let captured = gazette_sdk::configuration::capture_selected(config, &entries, *remaining)
            .map_err(|_| "selected native workspace documents could not be captured")?;
        debit(remaining, captured.inventory())?;
        let mut files = captured.into_parts().1.into_files();
        let mut sources = locators
            .iter()
            .map(|locator| {
                text_source(
                    config,
                    locator,
                    files
                        .remove(&locator.entry)
                        .ok_or("selected workspace document unavailable")?,
                )
            })
            .collect::<Result<Vec<_>, &'static str>>()?;
        let workspace = sources.remove(0);
        return Ok((workspace, sources));
    }
    // Headless selection derives mandatory modules through the existing strict loader.
    let (workspace_limits, _) = native_limits(limits)?;
    let loaded = gazette_sdk::configuration::load_workspace(
        config,
        &selection.workspace.entry,
        workspace_limits,
    )
    .map_err(|error| format!("selected native workspace could not be loaded: {error}"))?;
    debit(remaining, loaded.inventory())?;
    let workspace = text_source(
        config,
        &selection.workspace,
        loaded
            .source(&selection.workspace.entry)
            .ok_or("selected workspace unavailable")?
            .to_vec(),
    )?;
    let workspace_documents = loaded
        .documents()
        .documents
        .iter()
        .filter(|document| document.path != selection.workspace.entry)
        .map(|document| {
            if !physical.insert(config.join(document.path.as_str())) {
                return Err("selected native source overlaps a workspace module");
            }
            text_source(
                config,
                &Locator {
                    root: selection.workspace.root.clone(),
                    entry: document.path.clone(),
                    path: document.path.clone(),
                },
                loaded
                    .source(&document.path)
                    .ok_or("selected module unavailable")?
                    .to_vec(),
            )
        })
        .collect::<Result<Vec<_>, &'static str>>()?;
    Ok((workspace, workspace_documents))
}

fn roots(
    params: &InitializeParams,
    selection: &Rooted,
    editor: bool,
    limits: Limits,
) -> Result<BTreeMap<String, PathBuf>, &'static str> {
    if selection.roots.is_empty() || selection.roots.len() > limits.documents {
        return Err("selected native roots ceiling exceeded");
    }
    let mut roots = BTreeMap::new();
    let mut physical = BTreeSet::new();
    let mut bytes = 0usize;
    for root in &selection.roots {
        if root.id.is_empty() || root.id.chars().any(char::is_control) {
            return Err("invalid selected native root identity");
        }
        bytes = bytes
            .checked_add(root.id.len())
            .and_then(|bytes| bytes.checked_add(root.uri.as_str().len()))
            .filter(|bytes| *bytes <= limits.uri_bytes)
            .ok_or("selected native root bytes ceiling exceeded")?;
        if editor
            && !params
                .workspace_folders
                .as_deref()
                .unwrap_or_default()
                .iter()
                .any(|folder| folder.uri == root.uri)
        {
            return Err("selected native root is not an explicitly opened workspace folder");
        }
        let path = file_path(&root.uri)?;
        if !physical.insert(path.clone()) || roots.insert(root.id.clone(), path).is_some() {
            return Err("duplicate selected native root identity or directory");
        }
    }
    Ok(roots)
}

fn debit(
    remaining: &mut SourceLimits,
    inventory: &gazette_sdk::source::Inventory,
) -> Result<(), &'static str> {
    remaining.entries = remaining
        .entries
        .checked_sub(inventory.entries().len())
        .ok_or("native selected entry ceiling exceeded")?;
    remaining.total_bytes = remaining
        .total_bytes
        .checked_sub(inventory.total_bytes())
        .ok_or("native selected source byte ceiling exceeded")?;
    Ok(())
}

fn bounded_text_source(
    roots: &BTreeMap<String, PathBuf>,
    locator: &Locator,
    bytes: Vec<u8>,
    ceiling: usize,
) -> Result<session::Source, &'static str> {
    if bytes.len() > ceiling {
        return Err("selected native text document exceeds its role byte ceiling");
    }
    text_source(
        roots
            .get(&locator.root)
            .ok_or("unknown selected native root")?,
        locator,
        bytes,
    )
}

fn text_source(
    root: &Path,
    locator: &Locator,
    bytes: Vec<u8>,
) -> Result<session::Source, &'static str> {
    Ok(session::Source {
        id: native_uri(root, &locator.entry)?.as_str().to_owned(),
        path: locator.path.clone(),
        text: String::from_utf8(bytes).map_err(|_| "selected native text document is not UTF-8")?,
    })
}

fn native_limits(
    limits: Limits,
) -> Result<
    (
        gazette_sdk::configuration::WorkspaceLoadLimits,
        SourceLimits,
    ),
    &'static str,
> {
    let source = SourceLimits {
        entries: limits
            .documents
            .checked_mul(SourceLimits::default().depth + 1)
            .ok_or("native entry ceiling is not representable")?,
        depth: SourceLimits::default().depth,
        file_bytes: u64::try_from(limits.syntax.source_bytes)
            .map_err(|_| "native file byte ceiling is not representable")?,
        total_bytes: u64::try_from(limits.source_bytes)
            .map_err(|_| "native total byte ceiling is not representable")?,
    };
    Ok((
        gazette_sdk::configuration::WorkspaceLoadLimits {
            source,
            syntax: gazette_sdk::toml::SetLimits {
                documents: limits.documents,
                source_bytes: limits.source_bytes,
                path_bytes: limits.uri_bytes,
                document: limits.syntax,
                ..gazette_sdk::toml::SetLimits::default()
            },
        },
        source,
    ))
}

#[allow(deprecated)]
pub(super) fn native_root(params: &InitializeParams) -> Result<PathBuf, &'static str> {
    let folder = match params.workspace_folders.as_deref() {
        None | Some([]) => None,
        Some([folder]) => Some(file_path(&folder.uri)?),
        Some(_) => return Err("native selection requires exactly one workspace folder"),
    };
    let root = params.root_uri.as_ref().map(file_path).transpose()?;
    match (root, folder) {
        (Some(root), Some(folder)) if root != folder => {
            Err("rootUri and workspace folder select different native roots")
        }
        (Some(root), _) | (None, Some(root)) => Ok(root),
        (None, None) => Err("native selection requires one absolute file workspace URI"),
    }
}

pub(super) fn file_path(uri: &Uri) -> Result<PathBuf, &'static str> {
    let uri = url::Url::parse(uri.as_str()).map_err(|_| "invalid workspace URI")?;
    if uri.query().is_some() || uri.fragment().is_some() {
        return Err("workspace URI must not contain a query or fragment");
    }
    let path = uri
        .to_file_path()
        .map_err(|()| "workspace URI must use the file scheme")?;
    if !path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::CurDir | std::path::Component::ParentDir
            )
        })
    {
        return Err("workspace URI must select an exact absolute native root");
    }
    Ok(path)
}

fn native_uri(root: &Path, path: &EntryPath) -> Result<Uri, &'static str> {
    url::Url::from_file_path(root.join(path.as_str()))
        .map_err(|()| "selected native path cannot be represented as a file URI")?
        .as_str()
        .parse()
        .map_err(|_| "selected native file URI is invalid")
}
