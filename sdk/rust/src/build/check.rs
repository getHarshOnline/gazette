//! Check configured pages through the same acquisition and compiler as rendering.

use std::path::PathBuf;

use schemars::JsonSchema;
use serde::Serialize;

use crate::{authoring, configuration, corpus, source, workspace};

/// A problem found while checking a publication workspace.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, JsonSchema)]
pub struct CheckDiagnostic {
    /// Actual source file when its native binding is known.
    pub file: Option<PathBuf>,
    /// One-based source line, when the compiler supplies a checked location.
    pub line: Option<usize>,
    /// One-based character column, when a source location is available.
    pub column: Option<usize>,
    /// Stable compiler or acquisition error class.
    pub code: String,
    /// Explanation shared with editor diagnostics, including implicated identities.
    pub message: String,
}

impl std::fmt::Display for CheckDiagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(path) = &self.file {
            write!(f, "{}", path.display())?;
            if let Some(line) = self.line {
                write!(f, ":{line}")?;
                if let Some(column) = self.column {
                    write!(f, ":{column}")?;
                }
            }
            write!(f, ": ")?;
        }
        write!(
            f,
            "{}: {}",
            self.code,
            self.message.replace(['\n', '\r'], " ")
        )
    }
}

/// Configuration and page checks; no renderer, typesetter or publisher is run.
#[derive(Debug, Serialize, JsonSchema)]
pub struct WorkspaceCheck {
    /// Selected workspace and how the caller found it.
    pub workspace_location: configuration::WorkspaceLocation,
    /// Publication sites whose complete content was checked.
    pub checked_sites: Vec<workspace::Key>,
    /// Every reported problem, in stable order without repeated cross-site findings.
    pub diagnostics: Vec<CheckDiagnostic>,
}

impl WorkspaceCheck {
    /// Whether all attempted configuration and page checks passed.
    pub fn is_valid(&self) -> bool {
        self.diagnostics.is_empty()
    }
}

/// Read a selected workspace and check all its configured publication content.
/// This uses the render pipeline up to structural resolution. Math source can be
/// checked without generating its rendered output. All native reads are bounded
/// by `load`, `acquisition` and `corpus`; callers authorize them before this call.
/// The aggregate corpus byte ceiling follows the explicit request, then the
/// workspace setting, then the native default. Other parser bounds stay independent.
pub fn check_workspace(
    location: configuration::WorkspaceLocation,
    load: configuration::WorkspaceLoadLimits,
    acquisition: source::PreparationLimits,
    corpus: super::CorpusLimits,
) -> WorkspaceCheck {
    check_with(
        &gazette_adapter_fs::FileSystem,
        &gazette_adapter_git::Git,
        location,
        load,
        acquisition,
        corpus,
    )
}

fn check_with(
    reader: &impl gazette_ports::source::SourceCapture,
    revisions: &impl gazette_ports::source::RevisionReader,
    location: configuration::WorkspaceLocation,
    load: configuration::WorkspaceLoadLimits,
    acquisition: source::PreparationLimits,
    corpus: super::CorpusLimits,
) -> WorkspaceCheck {
    let mut report = WorkspaceCheck {
        workspace_location: location,
        checked_sites: Vec::new(),
        diagnostics: Vec::new(),
    };
    let location = &report.workspace_location;
    let loaded = match configuration::load_workspace(&location.root, &location.workspace, load) {
        Ok(loaded) => loaded,
        Err(error) => {
            report.diagnostics.push(problem(error.code(), &error));
            return report;
        }
    };
    let resolved = loaded.into_resolved();
    let workspace = &resolved.compilation().workspace;
    let corpus = corpus.resolve(workspace);
    // A configuration-only workspace has no publication pages to check.
    if workspace.content_bindings().next().is_none() && workspace.legal_sources().next().is_none() {
        return report;
    }
    let sites = workspace
        .sites()
        .filter(|site| workspace.publication_for(&site.id).is_some())
        .map(|site| site.id.clone())
        .collect::<Vec<_>>();
    if sites.is_empty() {
        report.diagnostics.push(problem(
            "workspace_publication",
            &"Configured content needs a publication to check its pages.",
        ));
        return report;
    }
    let inputs = match gazette_application::build::acquire_resolved_publications(
        reader,
        revisions,
        resolved.clone(),
        &sites,
        super::PublicationSetAcquisitionLimits {
            sites: sites.len(),
            sources: acquisition,
        },
    ) {
        Ok(inputs) => inputs,
        Err(error) => {
            report.diagnostics.push(problem(error.code(), &error));
            return report;
        }
    };
    for (site, outcome) in inputs.sites() {
        if let Err(error) = outcome {
            report
                .diagnostics
                .push(problem(error.code(), &format!("{site}: {error}")));
        }
    }
    let parsed = match super::parse_publication(inputs.content_inputs(), corpus) {
        Ok(parsed) => parsed,
        Err(error) => {
            report
                .diagnostics
                .extend(syntax_problems(inputs.content_inputs(), &resolved, &error));
            report.diagnostics.sort();
            report.diagnostics.dedup();
            return report;
        }
    };
    report.checked_sites.extend(
        inputs
            .sites()
            .iter()
            .filter(|(_, outcome)| outcome.is_ok())
            .map(|(site, _)| site.clone()),
    );
    match parsed.resolve(Default::default()) {
        Ok(publication) => {
            if let Err(error) = publication
                .authored()
                .publication_references(100_000, 1 << 20)
            {
                let origin = match &error {
                    crate::authored::ProjectionError::Document { origin, .. }
                    | crate::authored::ProjectionError::Landing { origin, .. } => Some(origin),
                    _ => None,
                };
                let text = origin
                    .and_then(|origin| parsed.corpus().files().get(&origin.path))
                    .and_then(|file| file.source());
                let rows = text.map(|text| authoring::reference_diagnostics(&error, text, 1 << 20));
                match (text, rows) {
                    (Some(text), Some(Ok(rows))) if !rows.is_empty() => {
                        report
                            .diagnostics
                            .extend(rows.into_iter().map(|row| located(&resolved, row, text)));
                    }
                    _ => report.diagnostics.push(problem(error.code(), &error)),
                }
            }
        }
        Err(error) => match &error {
            super::PublicationResolutionError::Authored(authored) => {
                let before = report.diagnostics.len();
                for (path, file) in parsed.corpus().files() {
                    let Some(text) = file.source() else { continue };
                    let digest = file.source_sha256();
                    match authoring::authored_diagnostics(authored, path, &digest, text, 1 << 20) {
                        Ok(rows) => report
                            .diagnostics
                            .extend(rows.into_iter().map(|row| located(&resolved, row, text))),
                        Err(error) => report.diagnostics.push(problem("check_diagnostic", &error)),
                    }
                }
                if report.diagnostics.len() == before {
                    report.diagnostics.push(problem(error.code(), &error));
                }
            }
            _ => report.diagnostics.push(problem(error.code(), &error)),
        },
    }
    report.diagnostics.sort();
    report.diagnostics.dedup();
    report
}

fn located(
    workspace: &gazette_application::configuration::ResolvedWorkspace,
    row: authoring::Diagnostic<'_>,
    text: &str,
) -> CheckDiagnostic {
    let (line, column) = row.location.map_or((None, None), |location| {
        let prefix = &text[..location.span.start];
        (
            Some(prefix.bytes().filter(|b| *b == b'\n').count() + 1),
            Some(prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1),
        )
    });
    CheckDiagnostic {
        file: if row.selection_wide {
            None
        } else {
            native_file(workspace, row.path)
        },
        line,
        column,
        code: row.code.into(),
        message: row.explanation(text),
    }
}

fn problem(code: &str, error: &impl std::fmt::Display) -> CheckDiagnostic {
    CheckDiagnostic {
        file: None,
        line: None,
        column: None,
        code: code.into(),
        message: error.to_string(),
    }
}

fn syntax_problems(
    inputs: &super::AcquiredInputs,
    resolved: &gazette_application::configuration::ResolvedWorkspace,
    error: &super::PublicationInputError,
) -> Vec<CheckDiagnostic> {
    let mut fallback = problem(error.code(), error);
    if let super::PublicationInputError::Document(super::DocumentError::Corpus(
        corpus::Error::File { path, error },
    )) = error
    {
        fallback.file = native_file(resolved, path);
        let workspace = &resolved.compilation().workspace;
        let text = workspace
            .content_source(path)
            .or_else(|| workspace.legal_bindings(path).next())
            .and_then(|binding| {
                let entry = binding.entry(path)?.parse::<source::EntryPath>().ok()?;
                std::str::from_utf8(inputs.sources().get(binding.source_key())?.file(&entry)?).ok()
            });
        if let Some(text) = text {
            let digest = source::Sha256Digest::of(text.as_bytes());
            if let Ok(rows) = authoring::file_diagnostics(error, path, &digest, text, 1 << 20)
                && !rows.is_empty()
            {
                return rows
                    .into_iter()
                    .map(|row| located(resolved, row, text))
                    .collect();
            }
        }
    }
    vec![fallback]
}

fn native_file(
    resolved: &gazette_application::configuration::ResolvedWorkspace,
    path: &source::EntryPath,
) -> Option<PathBuf> {
    let workspace = &resolved.compilation().workspace;
    let binding = workspace
        .content_source(path)
        .or_else(|| workspace.legal_bindings(path).next())?;
    let location = &workspace.source(binding.source_key())?.location;
    let mut root = resolved.source_root(location.root()).ok()?;
    match location {
        workspace::SourceLocation::Filesystem {
            subdirectory: Some(subdirectory),
            ..
        }
        | workspace::SourceLocation::Repository { subdirectory, .. } => {
            root.push(subdirectory.to_string())
        }
        workspace::SourceLocation::Submodule {
            submodule_path,
            subdirectory,
            ..
        } => {
            root.push(submodule_path.to_string());
            root.push(subdirectory.to_string());
        }
        _ => {}
    }
    Some(root.join(binding.entry(path)?))
}

#[cfg(test)]
#[path = "../../tests/workspace_check/mod.rs"]
mod tests;
