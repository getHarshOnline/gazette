//! Leaf page packages bind retained companions without introducing another compiler.

use std::collections::{BTreeMap, BTreeSet};

use gazette_core::{
    publication::{
        Key,
        content::{ContentKind, ContentNode, PageFamily},
        routing::RoutePath,
    },
    resource,
    source::{EntryPath, Sha256Digest},
};
use serde::Serialize;

use super::{FileError, FileFailure, Origin, PageResource};
use crate::{
    corpus::{File, ParsedCorpus},
    workspace::{Compilation, ResolvedContent},
};

/// One captured or observed companion owned by a leaf `index.md` page.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct PageCompanion {
    path: EntryPath,
    relative: EntryPath,
    kind: PageCompanionKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_sha256: Option<Sha256Digest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    observed_bytes: Option<u64>,
}

impl PageCompanion {
    /// Complete logical corpus path, including the configured content target.
    pub fn path(&self) -> &EntryPath {
        &self.path
    }

    /// Exact path below the owning page directory.
    pub fn relative(&self) -> &EntryPath {
        &self.relative
    }

    /// Structural treatment selected from the file kind and its literal path.
    pub fn kind(&self) -> PageCompanionKind {
        self.kind
    }

    /// Digest of captured bytes; an editor observation has no digest.
    pub fn source_sha256(&self) -> Option<Sha256Digest> {
        self.source_sha256
    }

    /// Observed file size when only metadata was supplied, without content identity.
    pub fn observed_bytes(&self) -> Option<u64> {
        self.observed_bytes
    }
}

/// Stable structural class of a retained page-package companion.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum PageCompanionKind {
    /// Plain article-owned commentary, never an independent publication page.
    Notes,
    /// A retained notebook attached through Gazette's existing notebook owner.
    Notebook,
    /// A JavaScript or TypeScript source handled by the browser-module compiler.
    Program,
    /// A CSS, Sass or SCSS source handled by the stylesheet compiler.
    Stylesheet,
    /// An SVG image handled by the existing bounded, inert-vector compiler.
    SvgImage,
    /// A WOFF2 source which requires an explicit validated font-resource declaration.
    Font,
    /// A supported literal static asset transported through the existing asset plan.
    Static,
    /// An owned file with no implicit compiler or transport; publication must refuse it.
    Unhandled,
}

/// Deterministic source package for one leaf page, with each companion’s evidence explicit.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct PagePackage {
    document: EntryPath,
    root: Option<EntryPath>,
    companions: Vec<PageCompanion>,
}

impl PagePackage {
    /// Exact retained `index.md` document which owns this package.
    pub fn document(&self) -> &EntryPath {
        &self.document
    }

    /// Exact capture-relative directory; `None` denotes the selected source root.
    pub fn root(&self) -> Option<&EntryPath> {
        self.root.as_ref()
    }

    /// Every owned companion in logical-path order.
    pub fn companions(&self) -> &[PageCompanion] {
        &self.companions
    }

    /// Optional retained article commentary, without another content identity.
    pub fn notes(&self) -> Option<&PageCompanion> {
        self.companions
            .iter()
            .find(|entry| entry.kind == PageCompanionKind::Notes)
    }

    pub(super) fn relative(&self, path: &EntryPath) -> Option<&EntryPath> {
        self.companions
            .binary_search_by(|entry| entry.path.cmp(path))
            .ok()
            .map(|index| &self.companions[index].relative)
    }
}

pub(super) fn notes_path(node: &ContentNode, document: &EntryPath) -> Option<EntryPath> {
    if !matches!(
        node.structure,
        ContentKind::Page {
            family: PageFamily::Article
        }
    ) {
        return None;
    }
    let (root, filename) = document.as_str().rsplit_once('/')?;
    if !matches!(
        filename,
        "index.md" | "index.markdown" | "index.landing.yaml" | "index.landing.yml"
    ) {
        return None;
    }
    format!("{root}/notes.md").parse().ok()
}

pub(super) struct Resolution {
    pub packages: BTreeMap<Key, PagePackage>,
    pub automatic_resources: usize,
}

pub(super) fn resolve(
    compilation: &Compilation,
    corpus: &ParsedCorpus<'_>,
    asset_metadata: &BTreeMap<EntryPath, u64>,
    content: &ResolvedContent<'_>,
    paths: &BTreeMap<Key, EntryPath>,
    resources: &mut BTreeMap<Key, Vec<PageResource>>,
    notebooks: &mut BTreeMap<Key, Vec<EntryPath>>,
) -> Result<Resolution, Vec<FileFailure>> {
    let mut packages = BTreeMap::new();
    let mut failures = Vec::new();
    let mut automatic_resources = 0usize;

    for (id, owner_path) in paths {
        let Some(node) = content.graph().node(id) else {
            continue;
        };
        if !matches!(node.structure, ContentKind::Page { .. }) {
            continue;
        }
        // A configured generated listing is a view over independently declared
        // pages, not a leaf bundle claiming everything below its source folder.
        // Its explicit resources still use the ordinary resource declarations.
        if matches!(
            node.structure,
            ContentKind::Page {
                family: PageFamily::Index
            }
        ) && compilation
            .workspace
            .publication_for(&node.site)
            .is_some_and(|publication| publication.listings.iter().any(|view| &view.content == id))
        {
            continue;
        }
        let Some(source) = compilation.workspace.content_source(owner_path) else {
            continue;
        };
        let Some(entry) = source.entry(owner_path) else {
            continue;
        };
        let (root, filename) = entry.rsplit_once('/').unwrap_or(("", entry));
        if !matches!(
            filename,
            "index.md" | "index.markdown" | "index.landing.yaml" | "index.landing.yml"
        ) || root.is_empty()
            && !matches!(
                node.structure,
                ContentKind::Page {
                    family: PageFamily::Article
                }
            )
        {
            continue;
        }
        let package_root: Option<EntryPath> = (!root.is_empty())
            .then(|| root.parse())
            .transpose()
            .map_err(|_| {
                vec![failure(
                    corpus,
                    owner_path,
                    owner_path.clone(),
                    "page package root is not a confined logical path",
                )]
            })?;
        let prefix = if root.is_empty() {
            String::new()
        } else {
            format!("{root}/")
        };
        let mut companions = Vec::new();
        let mut markdown = None;
        for (path, file, observed_bytes) in corpus
            .files()
            .iter()
            .map(|(path, file)| (path, Some(file), None))
            .chain(
                asset_metadata
                    .iter()
                    .map(|(path, bytes)| (path, None, Some(*bytes))),
            )
        {
            if path == owner_path {
                continue;
            }
            let Some(candidate_source) = compilation.workspace.content_source(path) else {
                continue;
            };
            if candidate_source.is_retained_only(path) {
                continue;
            }
            if candidate_source.tree_binding() != source.tree_binding() {
                continue;
            }
            let Some(candidate) = candidate_source.entry(path) else {
                continue;
            };
            let Some(relative) = candidate.strip_prefix(&prefix) else {
                continue;
            };
            if relative.is_empty() {
                continue;
            }
            let notes = notes_path(node, owner_path).as_ref() == Some(path)
                && matches!(file, Some(File::Markdown(_)));
            if !notes
                && matches!(
                    file,
                    Some(File::Document(_) | File::Landing(_) | File::Markdown(_))
                )
            {
                markdown = Some(path.clone());
                break;
            }
            let relative = match relative.parse() {
                Ok(relative) => relative,
                Err(_) => {
                    failures.push(failure(
                        corpus,
                        owner_path,
                        path.clone(),
                        "page package companion has an invalid relative path",
                    ));
                    continue;
                }
            };
            let kind = if notes {
                PageCompanionKind::Notes
            } else {
                companion_kind(file, &relative)
            };
            companions.push(PageCompanion {
                path: path.clone(),
                relative,
                kind,
                source_sha256: file.map(File::source_sha256),
                observed_bytes,
            });
        }
        companions.sort_unstable_by(|a, b| a.path.cmp(&b.path));
        if let Some(path) = markdown {
            failures.push(failure(
                corpus,
                owner_path,
                path,
                "leaf page package contains another Markdown document",
            ));
            continue;
        }
        if companions.is_empty() {
            packages.insert(
                id.clone(),
                PagePackage {
                    document: owner_path.clone(),
                    root: package_root,
                    companions,
                },
            );
            continue;
        }

        let declared_inputs: BTreeSet<EntryPath> = resources
            .get(id)
            .into_iter()
            .flatten()
            .flat_map(|declaration| declaration.inputs.iter().cloned())
            .collect();
        let attached_notebooks: BTreeSet<EntryPath> =
            notebooks.get(id).into_iter().flatten().cloned().collect();
        let mut script_inputs = Vec::new();
        let mut script_entries = Vec::new();
        let mut style_inputs = Vec::new();
        let mut style_entries = Vec::new();
        let mut image_entries = Vec::new();

        for companion in &companions {
            let capture_path = match source
                .entry(&companion.path)
                .and_then(|path| path.parse().ok())
            {
                Some(path) => path,
                None => {
                    failures.push(failure(
                        corpus,
                        owner_path,
                        companion.path.clone(),
                        "page package companion is outside its retained source binding",
                    ));
                    continue;
                }
            };
            match corpus.files().get(&companion.path) {
                Some(File::Notebook(_)) => {
                    if !attached_notebooks.contains(&capture_path) {
                        notebooks.entry(id.clone()).or_default().push(capture_path);
                    }
                }
                file if (matches!(file, Some(File::Asset(_)))
                    || asset_metadata.contains_key(&companion.path))
                    && !declared_inputs.contains(&capture_path) =>
                {
                    let name = companion.relative.as_str();
                    if companion.kind == PageCompanionKind::Program {
                        script_inputs.push(capture_path.clone());
                        if script_entry(name) {
                            script_entries.push((capture_path, companion.relative.clone()));
                        }
                    } else if companion.kind == PageCompanionKind::Stylesheet {
                        style_inputs.push(capture_path.clone());
                        if stylesheet_entry(name) {
                            style_entries.push((capture_path, companion.relative.clone()));
                        }
                    } else if companion.kind == PageCompanionKind::SvgImage {
                        image_entries.push((capture_path, companion.relative.clone()));
                    }
                }
                _ => {}
            }
        }
        script_inputs.sort_unstable();
        style_inputs.sort_unstable();
        script_entries.sort_unstable();
        style_entries.sort_unstable();

        if script_entries.len() > 1 || style_entries.len() > 1 {
            let path = script_entries
                .get(1)
                .or_else(|| style_entries.get(1))
                .map_or_else(|| owner_path.clone(), |(path, _)| path.clone());
            failures.push(failure(
                corpus,
                owner_path,
                path,
                "ambiguous automatic page-package entries require an explicit resource declaration",
            ));
            continue;
        }

        let Some(route) = node.route.as_ref().map(|route| &route.path) else {
            if !script_entries.is_empty() || !style_entries.is_empty() || !image_entries.is_empty()
            {
                let path = script_entries
                    .first()
                    .or_else(|| style_entries.first())
                    .or_else(|| image_entries.first())
                    .map_or_else(|| owner_path.clone(), |(path, _)| path.clone());
                failures.push(failure(
                    corpus,
                    owner_path,
                    path,
                    "compiled page-package companions require an explicit page route",
                ));
                continue;
            }
            packages.insert(
                id.clone(),
                PagePackage {
                    document: owner_path.clone(),
                    root: package_root,
                    companions,
                },
            );
            continue;
        };

        let mut automatic = Vec::new();
        for (entry, relative) in script_entries {
            let output = match output(route, &relative, "js") {
                Ok(output) => output,
                Err(()) => {
                    failures.push(failure(
                        corpus,
                        owner_path,
                        entry,
                        "page package script cannot form a confined output route",
                    ));
                    continue;
                }
            };
            automatic.push(PageResource {
                inputs: script_inputs.clone(),
                rendition: false,
                responsive: false,
                build: resource::Recipe::BrowserModule {
                    entry: entry.clone(),
                    imports: BTreeMap::new(),
                },
                output,
            });
        }
        for (entry, relative) in style_entries {
            let output = match output(route, &relative, "css") {
                Ok(output) => output,
                Err(()) => {
                    failures.push(failure(
                        corpus,
                        owner_path,
                        entry,
                        "page package stylesheet cannot form a confined output route",
                    ));
                    continue;
                }
            };
            automatic.push(PageResource {
                inputs: style_inputs.clone(),
                rendition: false,
                responsive: false,
                build: resource::Recipe::Stylesheet {
                    entry: entry.clone(),
                },
                output,
            });
        }
        for (entry, relative) in image_entries {
            let output = match route.join_file(&relative) {
                Ok(output) => output,
                Err(_) => {
                    failures.push(failure(
                        corpus,
                        owner_path,
                        entry,
                        "page package image cannot form a confined output route",
                    ));
                    continue;
                }
            };
            automatic.push(PageResource {
                inputs: vec![entry.clone()],
                rendition: false,
                responsive: false,
                build: resource::Recipe::SvgImage { entry },
                output,
            });
        }
        if !automatic.is_empty() {
            let mut outputs: BTreeSet<_> = resources
                .get(id)
                .into_iter()
                .flatten()
                .map(|declaration| declaration.output.output().clone())
                .collect();
            if let Some(declaration) = automatic
                .iter()
                .find(|declaration| !outputs.insert(declaration.output.output().clone()))
            {
                failures.push(failure(
                    corpus,
                    owner_path,
                    declaration.build.entry().clone(),
                    "explicit and automatic page resources claim the same output",
                ));
                continue;
            }
            if let Err(error) = super::resources::validate(&mut automatic) {
                failures.push(FileFailure {
                    origin: origin(corpus, owner_path),
                    error: FileError::Resource(error),
                });
                continue;
            }
            automatic_resources += automatic.len();
            resources.entry(id.clone()).or_default().extend(automatic);
        }
        packages.insert(
            id.clone(),
            PagePackage {
                document: owner_path.clone(),
                root: package_root,
                companions,
            },
        );
    }
    if failures.is_empty() {
        Ok(Resolution {
            packages,
            automatic_resources,
        })
    } else {
        Err(failures)
    }
}

fn origin(corpus: &ParsedCorpus<'_>, owner: &EntryPath) -> Origin {
    Origin {
        path: owner.clone(),
        source_sha256: corpus
            .files()
            .get(owner)
            .expect("page package owner came from this corpus")
            .source_sha256(),
        span: None,
    }
}

fn failure(
    corpus: &ParsedCorpus<'_>,
    owner: &EntryPath,
    path: EntryPath,
    reason: &'static str,
) -> FileFailure {
    FileFailure {
        origin: origin(corpus, owner),
        error: FileError::Resource(resource::Error::Input { path, reason }),
    }
}

fn output(route: &RoutePath, entry: &EntryPath, extension: &str) -> Result<RoutePath, ()> {
    let (stem, _) = entry.as_str().rsplit_once('.').ok_or(())?;
    let path: EntryPath = format!("{stem}.{extension}").parse().map_err(|_| ())?;
    route.join_file(&path).map_err(|_| ())
}

fn matches_ascii_case(value: &str, candidates: &[&str]) -> bool {
    candidates
        .iter()
        .any(|candidate| value.eq_ignore_ascii_case(candidate))
}

fn script(path: &str) -> bool {
    extension(path).is_some_and(|extension| {
        matches_ascii_case(extension, &["js", "mjs", "cjs", "ts", "tsx", "jsx"])
    })
}

fn script_entry(path: &str) -> bool {
    if path.contains('/') {
        return false;
    }
    let name = path.rsplit('/').next().unwrap_or(path);
    let lowered = name.to_ascii_lowercase();
    !lowered.ends_with(".d.ts") && !lowered.contains(".test.") && !lowered.contains(".spec.")
}

fn stylesheet(path: &str) -> bool {
    extension(path).is_some_and(|extension| matches_ascii_case(extension, &["css", "scss", "sass"]))
}

fn stylesheet_entry(path: &str) -> bool {
    !path.contains('/') && !path.rsplit('/').next().unwrap_or(path).starts_with('_')
}

fn font(path: &str) -> bool {
    extension(path).is_some_and(|extension| matches_ascii_case(extension, &["woff2"]))
}

fn extension(path: &str) -> Option<&str> {
    path.rsplit_once('.').map(|(_, extension)| extension)
}

pub(super) fn svg_image(path: &str) -> bool {
    extension(path).is_some_and(|extension| extension.eq_ignore_ascii_case("svg"))
}

fn companion_kind(file: Option<&File<'_>>, relative: &EntryPath) -> PageCompanionKind {
    match file {
        Some(File::Notebook(_)) => PageCompanionKind::Notebook,
        Some(File::Document(_) | File::Landing(_) | File::Markdown(_)) => {
            unreachable!("Markdown companions are refused before classification")
        }
        Some(File::Asset(_)) | None => {
            let path = relative.as_str();
            if script(path) {
                PageCompanionKind::Program
            } else if stylesheet(path) {
                PageCompanionKind::Stylesheet
            } else if svg_image(path) {
                PageCompanionKind::SvgImage
            } else if font(path) {
                PageCompanionKind::Font
            } else if super::assets::supported_static(path) {
                PageCompanionKind::Static
            } else {
                PageCompanionKind::Unhandled
            }
        }
    }
}
