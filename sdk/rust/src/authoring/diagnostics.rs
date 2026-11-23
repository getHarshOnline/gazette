//! Borrowed native diagnostics checked against one selected source preimage.

use std::{fmt, mem::size_of};

use gazette_compiler::{authored, corpus, document, markdown, toml, workspace};
use gazette_core::{
    publication::{Key, content},
    source::{EntryPath, Location, Precision, Sha256Digest, Span},
};
use serde::Serialize;

use super::{Analysis, ComponentCoordinates, Failure, PackageBaselineKey, PackageFailure};

/// One transport view, not another semantic diagnostic owner.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct Diagnostic<'a> {
    /// Queried selected input; selection-wide findings do not blame this file.
    pub path: &'a EntryPath,
    /// Exact full-source identity, including framing and line endings.
    pub source_sha256: &'a Sha256Digest,
    /// Native diagnostic class, or an explicit SDK class where none exists.
    pub code: &'static str,
    /// Borrowed native explanation or bounded class description; never formatted source.
    pub message: &'a str,
    /// Native logical configuration field, not an inferred source range.
    pub field: Option<&'a str>,
    /// Native implicated configuration identity, when supplied.
    pub key: Option<&'a Key>,
    /// Native declaring content identity, when supplied.
    pub source: Option<&'a Key>,
    /// Native implicated content endpoint, when supplied.
    pub target: Option<&'a Key>,
    /// Full-file checked evidence; absence never manufactures a token location.
    pub location: Option<Location>,
    /// The native error has no declaring selected-file origin.
    pub selection_wide: bool,
}

impl Diagnostic<'_> {
    /// Explain the finding with its implicated field or identity. Supply the same
    /// complete source used to project this diagnostic; broken links include their spelling.
    pub fn explanation(&self, source: &str) -> String {
        let mut message = self.message.to_owned();
        match (self.field, self.key.or(self.target)) {
            (Some(field), Some(key)) => message.push_str(&format!(": {field} = {key}")),
            (Some(field), None) => message.push_str(&format!(": {field}")),
            (None, Some(key)) => message.push_str(&format!(": {key}")),
            (None, None) => {}
        }
        if self.code == "markdown_link_resolution"
            && let Some(span) = self.location.map(|location| location.span)
            && let Some(reference) = source.get(span.start..span.end)
        {
            message.push_str(&format!(": {}", reference.replace(['\n', '\r'], " ")));
        }
        message
    }
}

#[cfg(feature = "native-local")]
pub(crate) fn file_diagnostics<'a>(
    error: &'a corpus::FileError,
    path: &'a EntryPath,
    digest: &'a Sha256Digest,
    source: &str,
    maximum_bytes: usize,
) -> Result<Vec<Diagnostic<'a>>, DiagnosticError> {
    if digest != &Sha256Digest::of(source.as_bytes()) {
        return Err(DiagnosticError::InvalidSource);
    }
    let mut view = Projection {
        path,
        digest,
        source,
        remaining: maximum_bytes,
        rows: Vec::new(),
    };
    view.file(error)?;
    Ok(view.rows)
}

#[cfg(feature = "native-local")]
pub(crate) fn authored_diagnostics<'a>(
    error: &'a authored::Error,
    path: &'a EntryPath,
    digest: &'a Sha256Digest,
    source: &str,
    maximum_bytes: usize,
) -> Result<Vec<Diagnostic<'a>>, DiagnosticError> {
    if digest != &Sha256Digest::of(source.as_bytes()) {
        return Err(DiagnosticError::InvalidSource);
    }
    let mut view = Projection {
        path,
        digest,
        source,
        remaining: maximum_bytes,
        rows: Vec::new(),
    };
    view.authored(error, path)?;
    Ok(view.rows)
}

#[cfg(feature = "native-local")]
pub(crate) fn reference_diagnostics<'a>(
    error: &'a authored::ProjectionError,
    source: &str,
    maximum_bytes: usize,
) -> Result<Vec<Diagnostic<'a>>, DiagnosticError> {
    let origin = match error {
        authored::ProjectionError::Document { origin, .. }
        | authored::ProjectionError::Landing { origin, .. } => origin,
        _ => return Ok(Vec::new()),
    };
    if origin.source_sha256 != Sha256Digest::of(source.as_bytes()) {
        return Err(DiagnosticError::InvalidSource);
    }
    let mut view = Projection {
        path: &origin.path,
        digest: &origin.source_sha256,
        source,
        remaining: maximum_bytes,
        rows: Vec::new(),
    };
    match error {
        authored::ProjectionError::Document { error, .. } => {
            if let document::Error::Markdown { error, offset } = error.as_ref() {
                view.markdown(error, view.parent(*offset)?)?;
            }
        }
        authored::ProjectionError::Landing { code, message, .. } => {
            let mut row = view.row(code, message);
            row.location = origin
                .span
                .map(|span| view.locate(span, view.parent(0)?, Precision::Construct))
                .transpose()?;
            view.push(row)?;
        }
        _ => {}
    }
    Ok(view.rows)
}

/// The complete borrowed projection could not be established.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagnosticError {
    /// Unknown input, changed bytes or an invalid native coordinate/source join.
    InvalidSource,
    /// Complete logical row footprint and borrowed text exceed the requested bound.
    Limit,
}

impl fmt::Display for DiagnosticError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidSource => "authoring diagnostic source binding is invalid",
            Self::Limit => "complete authoring diagnostic projection exceeds its byte limit",
        })
    }
}

impl std::error::Error for DiagnosticError {}

/// Project all applicable native failures for one exact selected input.
///
/// Every matching conflicting declaration produces a row. Selection-wide errors
/// are explicitly marked, with no invented file position. `limit` bounds logical
/// row footprint and borrowed string occurrences, not allocator capacity or JSON escaping/framing;
/// transports must also bound their complete encoded output. No parsing,
/// semantic resolution, source discovery or display-string formatting occurs.
///
/// # Errors
/// Returns no partial vector on a source mismatch, invalid span or exhausted bound.
pub fn diagnostics<'a>(
    analysis: &'a Analysis,
    path: &'a EntryPath,
    source: &str,
    limit: usize,
) -> Result<Vec<Diagnostic<'a>>, DiagnosticError> {
    diagnostics_for(analysis, path, path, source, limit)
}

/// Project diagnostics using an internal analysis path while preserving the
/// capture-relative transport path of a configured manual baseline.
///
/// # Errors
///
/// Returns a diagnostic error when the retained source identity or supplied bytes
/// do not match the analysis, a span is invalid, or the requested bound is exceeded.
pub fn diagnostics_for<'a>(
    analysis: &'a Analysis,
    analysis_path: &EntryPath,
    display_path: &'a EntryPath,
    source: &str,
    limit: usize,
) -> Result<Vec<Diagnostic<'a>>, DiagnosticError> {
    let (analysis_path, digest) = analysis
        .inputs()
        .get_key_value(analysis_path)
        .ok_or(DiagnosticError::InvalidSource)?;
    if digest != &Sha256Digest::of(source.as_bytes()) {
        return Err(DiagnosticError::InvalidSource);
    }
    let mut view = Projection {
        path: display_path,
        digest,
        source,
        remaining: limit,
        rows: Vec::new(),
    };
    for failure in analysis.failures() {
        match failure {
            Failure::WorkspaceEncoding(_) if analysis_path == analysis.workspace() => {
                // A &str preimage cannot match invalid UTF-8. Retain the explicit
                // class for exhaustive routing without inventing byte positions.
                view.push(view.row("workspace_utf8", "workspace source is not UTF-8"))?;
            }
            Failure::Workspace(error) if analysis.workspace_documents().contains(analysis_path) => {
                match error {
                    workspace::Error::Toml(error) => view.toml(error, 0)?,
                    workspace::Error::TomlSet(error) => match error {
                        toml::SetError::Documents(errors) => {
                            for error in errors {
                                match error {
                                    toml::DocumentError::Utf8 { path: failed, .. }
                                        if failed == analysis_path =>
                                    {
                                        view.push(view.row(
                                            error.code(),
                                            "workspace document source is not UTF-8",
                                        ))?;
                                    }
                                    toml::DocumentError::Toml {
                                        path: failed,
                                        error,
                                    } if failed == analysis_path => view.toml(error, 0)?,
                                    toml::DocumentError::Utf8 { .. }
                                    | toml::DocumentError::Toml { .. } => {}
                                }
                            }
                        }
                        toml::SetError::Duplicate(failed) if failed == analysis_path => {
                            view.push(view.row(error.code(), "duplicate workspace document"))?;
                        }
                        toml::SetError::Limit(resource) => {
                            let mut row = view.row(error.code(), resource);
                            row.selection_wide = true;
                            view.push(row)?;
                        }
                        toml::SetError::Duplicate(_) => {}
                    },
                    workspace::Error::Limit(resource) => {
                        view.push(view.row(error.code(), resource))?;
                    }
                    workspace::Error::Invalid(errors) => {
                        for error in errors {
                            let origin = if error.code == "workspace_reference" {
                                error
                                    .key
                                    .as_ref()
                                    .and_then(|key| analysis.workspace_reference_origin(key))
                            } else {
                                None
                            };
                            if let Some(origin) = origin {
                                if origin.path != analysis_path {
                                    continue;
                                }
                                if origin.source_sha256 != *digest {
                                    return Err(DiagnosticError::InvalidSource);
                                }
                                let mut row =
                                    view.row(error.code, "workspace reference does not resolve");
                                row.field = Some(&error.field);
                                row.key = error.key.as_ref();
                                row.location = Some(origin.location);
                                view.push(row)?;
                            } else if analysis_path == analysis.workspace() {
                                let mut row = view.row(error.code, "workspace constraint failed");
                                row.field = Some(&error.field);
                                row.key = error.key.as_ref();
                                row.selection_wide = analysis.workspace_documents().len() > 1;
                                view.push(row)?;
                            }
                        }
                    }
                }
            }
            Failure::Corpus(corpus::Error::File {
                path: failed,
                error,
            })
            | Failure::DocumentReferences(corpus::Error::File {
                path: failed,
                error,
            }) if failed == analysis_path => {
                view.file(error)?;
            }
            Failure::Corpus(error @ corpus::Error::Binding(failed))
            | Failure::DocumentReferences(error @ corpus::Error::Binding(failed))
                if failed == analysis_path =>
            {
                view.push(view.row(
                    error.code(),
                    "retained syntax or generated result does not match this input",
                ))?;
            }
            Failure::Corpus(
                error @ (corpus::Error::Limit(_)
                | corpus::Error::SourceBytesLimit { .. }
                | corpus::Error::Duplicate(_)),
            )
            | Failure::DocumentReferences(
                error @ (corpus::Error::Limit(_)
                | corpus::Error::SourceBytesLimit { .. }
                | corpus::Error::Duplicate(_)),
            ) => {
                let message = match error {
                    corpus::Error::Limit(resource) => *resource,
                    corpus::Error::SourceBytesLimit { .. } => {
                        "aggregate corpus.source_bytes limit exceeded"
                    }
                    _ => "duplicate selected corpus input",
                };
                let mut row = view.row(error.code(), message);
                row.selection_wide = true;
                view.push(row)?;
            }
            Failure::Authored(error) => view.authored(error, analysis_path)?,
            Failure::Headings {
                path: failed,
                body_span,
                error,
            } if failed == analysis_path => {
                view.markdown(error, *body_span)?;
            }
            Failure::Renderability {
                path: failed,
                body_span,
                error,
            } if failed == analysis_path => {
                view.markdown(error, *body_span)?;
            }
            Failure::Components {
                path: failed,
                coordinates,
                error,
            } if failed == analysis_path => match coordinates {
                ComponentCoordinates::Markdown(body_span) => view.markdown(error, *body_span)?,
                ComponentCoordinates::Landing(original) => {
                    let parent = view.parent(0)?;
                    view.markdown_mapped(error, parent, |_| *original)?;
                }
            },
            Failure::WorkspaceEncoding(_)
            | Failure::Workspace(_)
            | Failure::Corpus(corpus::Error::File { .. })
            | Failure::Corpus(corpus::Error::Binding(_))
            | Failure::DocumentReferences(corpus::Error::File { .. })
            | Failure::DocumentReferences(corpus::Error::Binding(_))
            | Failure::Package { .. }
            | Failure::Headings { .. }
            | Failure::Renderability { .. }
            | Failure::Components { .. } => {}
        }
    }
    Ok(view.rows)
}

/// Project package failures for one exact typed package baseline and selected source preimage.
///
/// The caller-supplied path is the selected editor identity, not a workspace binding coordinate.
/// The package identity is checked against the analysis-owned baseline digest before only failures
/// for that same typed package are projected. No parsing, source discovery or binding inference
/// occurs here.
///
/// # Errors
/// Returns no partial vector for an unknown package, changed bytes or exhausted bound.
pub fn package_diagnostics<'a>(
    analysis: &'a Analysis,
    package: &PackageBaselineKey,
    path: &'a EntryPath,
    source: &str,
    maximum_bytes: usize,
) -> Result<Vec<Diagnostic<'a>>, DiagnosticError> {
    let (package, digest) = analysis
        .package_baselines()
        .get_key_value(package)
        .ok_or(DiagnosticError::InvalidSource)?;
    if digest != &Sha256Digest::of(source.as_bytes()) {
        return Err(DiagnosticError::InvalidSource);
    }
    let mut view = Projection {
        path,
        digest,
        source,
        remaining: maximum_bytes,
        rows: Vec::new(),
    };
    for failure in analysis.failures() {
        if let Failure::Package {
            package: failed,
            source,
            error,
            ..
        } = failure
            && failed == package
        {
            view.package(error, source.as_ref())?;
        }
    }
    Ok(view.rows)
}

struct Projection<'a, 's> {
    path: &'a EntryPath,
    digest: &'a Sha256Digest,
    source: &'s str,
    remaining: usize,
    rows: Vec<Diagnostic<'a>>,
}

impl<'a> Projection<'a, '_> {
    fn row(&self, code: &'static str, message: &'a str) -> Diagnostic<'a> {
        Diagnostic {
            path: self.path,
            source_sha256: self.digest,
            code,
            message,
            field: None,
            key: None,
            source: None,
            target: None,
            location: None,
            selection_wide: false,
        }
    }

    fn push(&mut self, row: Diagnostic<'a>) -> Result<(), DiagnosticError> {
        let mut cost = size_of::<Diagnostic<'_>>() + 64;
        for text in [
            Some(row.path.as_str()),
            Some(row.code),
            Some(row.message),
            row.field,
            row.key.map(Key::as_str),
            row.source.map(Key::as_str),
            row.target.map(Key::as_str),
        ]
        .into_iter()
        .flatten()
        {
            cost = cost.checked_add(text.len()).ok_or(DiagnosticError::Limit)?;
        }
        self.remaining = self
            .remaining
            .checked_sub(cost)
            .ok_or(DiagnosticError::Limit)?;
        self.rows.push(row);
        Ok(())
    }

    fn parent(&self, offset: usize) -> Result<Span, DiagnosticError> {
        Span::checked(self.source, offset..self.source.len()).ok_or(DiagnosticError::InvalidSource)
    }

    fn locate(
        &self,
        span: Span,
        parent: Span,
        precision: Precision,
    ) -> Result<Location, DiagnosticError> {
        Ok(Location {
            span: span
                .in_parent(self.source, parent)
                .ok_or(DiagnosticError::InvalidSource)?,
            precision,
        })
    }

    fn toml(&mut self, error: &'a toml::Error, offset: usize) -> Result<(), DiagnosticError> {
        let (message, span) = match error {
            toml::Error::Limit(resource) => (*resource, None),
            toml::Error::Invalid { message, span } => (message.as_str(), *span),
        };
        let mut row = self.row(error.code(), message);
        if let Some(span) = span {
            row.location = Some(self.locate(span, self.parent(offset)?, Precision::Construct)?);
        }
        self.push(row)
    }

    fn package(
        &mut self,
        error: &'a PackageFailure,
        source: Option<&'a Key>,
    ) -> Result<(), DiagnosticError> {
        let (code, message, field) = match error {
            PackageFailure::Encoding => {
                ("package_utf8", "package manifest source is not UTF-8", None)
            }
            PackageFailure::Binding(reason) => ("package_binding", *reason, None),
            PackageFailure::ThemeManifest(error) => match error {
                gazette_compiler::theme_package::Error::Syntax(error) => {
                    let message = match error {
                        toml::Error::Limit(resource) => *resource,
                        toml::Error::Invalid { message, .. } => message.as_str(),
                    };
                    (error.code(), message, None)
                }
                gazette_compiler::theme_package::Error::Declaration { field, reason, .. } => {
                    (error.code(), *reason, Some(*field))
                }
            },
            PackageFailure::PluginManifest(error) => match error {
                gazette_compiler::plugin::Error::Syntax(error) => {
                    let message = match error {
                        toml::Error::Limit(resource) => *resource,
                        toml::Error::Invalid { message, .. } => message.as_str(),
                    };
                    (error.code(), message, None)
                }
                gazette_compiler::plugin::Error::Declaration { reason, .. } => {
                    (error.code(), *reason, None)
                }
            },
            PackageFailure::ThemeGraph(error) => {
                let message = match error {
                    gazette_compiler::theme_graph::Error::DuplicatePackage { .. } => {
                        "duplicate Theme package declaration"
                    }
                    gazette_compiler::theme_graph::Error::MissingPackage { .. } => {
                        "required Theme package baseline is missing"
                    }
                    gazette_compiler::theme_graph::Error::Cycle { .. } => {
                        "Theme package dependency cycle"
                    }
                    gazette_compiler::theme_graph::Error::MissingRender { .. } => {
                        "selected Theme package has no render recipe"
                    }
                    gazette_compiler::theme_graph::Error::RenderCollision { .. } => {
                        "Theme machine render entry has competing dependency providers"
                    }
                    gazette_compiler::theme_graph::Error::CopyCollision { .. } => {
                        "Theme interface label has competing dependency providers"
                    }
                    gazette_compiler::theme_graph::Error::ResourceCollision { .. } => {
                        "Theme resource default has competing dependency providers"
                    }
                    gazette_compiler::theme_graph::Error::ComponentCollision { .. } => {
                        "Theme package component collision"
                    }
                    gazette_compiler::theme_graph::Error::ComponentContract { .. } => {
                        "Theme package component contract failed"
                    }
                    gazette_compiler::theme_graph::Error::QualifiedEntry { .. } => {
                        "Theme package entry cannot enter its namespace"
                    }
                };
                (error.code(), message, None)
            }
        };
        let mut row = self.row(code, message);
        row.field = field;
        row.source = source;
        // Package bytes are keyed by typed package identity, independently from the
        // workspace-owned manifest path. Do not invent one uniform source range for
        // failure families that retain different native coordinate shapes.
        row.selection_wide = true;
        self.push(row)
    }

    fn markdown(
        &mut self,
        error: &'a markdown::Error,
        parent: Span,
    ) -> Result<(), DiagnosticError> {
        self.markdown_mapped(error, parent, |location| location)
    }

    fn markdown_mapped(
        &mut self,
        error: &'a markdown::Error,
        parent: Span,
        location_map: impl FnOnce(Option<Location>) -> Option<Location>,
    ) -> Result<(), DiagnosticError> {
        use gazette_core::markdown::Error as MechanismError;
        let (message, location) = match error {
            markdown::Error::Adapter(MechanismError::Unsupported { location, reason }) => {
                (*reason, Some(*location))
            }
            markdown::Error::Adapter(MechanismError::Limit(resource))
            | markdown::Error::Limit(resource) => (*resource, None),
            markdown::Error::Adapter(MechanismError::InvalidSource) => {
                ("Markdown parser source invariant failed", None)
            }
            markdown::Error::Adapter(MechanismError::InvalidPlan) => {
                ("Markdown formatting plan does not match source", None)
            }
            markdown::Error::Adapter(MechanismError::Formatting) => {
                ("Markdown adapter formatting failed", None)
            }
            markdown::Error::Namespace => ("invalid Markdown embedding namespace", None),
            markdown::Error::InvalidOutput => ("invalid Markdown projection", None),
            markdown::Error::RawHtml(span) => (
                "raw HTML requires an admitted renderer",
                Some(Location {
                    span: *span,
                    precision: Precision::Construct,
                }),
            ),
            markdown::Error::UnsafeUrl(span) => (
                "unsafe or unsupported URL",
                Some(Location {
                    span: *span,
                    precision: Precision::Construct,
                }),
            ),
            markdown::Error::Summary { span, reason }
            | markdown::Error::Anchor { span, reason }
            | markdown::Error::LinkResolution { span, reason } => (
                *reason,
                Some(Location {
                    span: *span,
                    precision: Precision::Construct,
                }),
            ),
        };
        let mut row = self.row(error.code(), message);
        if let Some(location) = location_map(location) {
            row.location = Some(self.locate(location.span, parent, location.precision)?);
        }
        self.push(row)
    }

    fn file(&mut self, error: &'a corpus::FileError) -> Result<(), DiagnosticError> {
        match error {
            corpus::FileError::Utf8(_) => {
                self.push(self.row("corpus_utf8", "content source is not UTF-8"))
            }
            corpus::FileError::Markdown(error) => self.markdown(error, self.parent(0)?),
            corpus::FileError::Document(document::Error::Metadata { error, offset }) => {
                self.toml(error, *offset)
            }
            corpus::FileError::Document(document::Error::Markdown { error, offset }) => {
                self.markdown(error, self.parent(*offset)?)
            }
            corpus::FileError::Document(error) => {
                let message = match error {
                    document::Error::Framing(reason) => *reason,
                    _ => "complete document byte ceiling exceeded",
                };
                self.push(self.row(error.code(), message))
            }
            corpus::FileError::Landing(error) => {
                use gazette_compiler::landing::Error;
                let (message, span) = match error {
                    Error::Yaml(error) => (error.message.as_str(), error.span),
                    Error::Invalid { reason, span } => (*reason, *span),
                    Error::Markdown(_) => {
                        ("normalized landing body failed Markdown validation", None)
                    }
                };
                let mut row = self.row(error.code(), message);
                if let Some(span) = span {
                    row.location =
                        Some(self.locate(span, self.parent(0)?, Precision::Construct)?);
                }
                self.push(row)
            }
            corpus::FileError::Notebook(error) => {
                // JSON coordinates may be decoder-local, not full-file byte spans.
                self.push(self.row(
                    error.code(),
                    "notebook syntax or structural constraint failed",
                ))
            }
        }
    }

    fn authored(
        &mut self,
        error: &'a authored::Error,
        analysis_path: &EntryPath,
    ) -> Result<(), DiagnosticError> {
        match error {
            authored::Error::AssetMetadata { path, reason } => {
                let mut row = self.row("authored_asset_metadata", reason);
                row.selection_wide = path != analysis_path;
                self.push(row)
            }
            authored::Error::RetainedOnly { path, reason } => {
                let mut row = self.row("authored_retained_only", reason);
                // Exact queried input identity is already checked by diagnostics_for.
                // Absent/foreign dependencies have no span in this selected file.
                row.selection_wide = path != analysis_path;
                self.push(row)
            }
            authored::Error::Artwork(error) => {
                if let Some(origin) = &error.origin {
                    if &origin.path != self.path {
                        return Ok(());
                    }
                    if &origin.source_sha256 != self.digest {
                        return Err(DiagnosticError::InvalidSource);
                    }
                }
                let mut row = self.row(error.code(), &error.reason);
                row.source = Some(&error.content);
                if let Some(span) = error.origin.as_ref().and_then(|origin| origin.span) {
                    row.location =
                        Some(self.locate(span, self.parent(0)?, Precision::Construct)?);
                } else {
                    row.selection_wide = true;
                }
                self.push(row)
            }
            authored::Error::PublicationMetadata(error) => {
                let mut row = self.row(error.code(), error.reason);
                row.source = Some(&error.site);
                row.target = Some(&error.id);
                // A workspace-bound interface may fail against the selected
                // corpus without a Markdown declaration or exact source span.
                row.selection_wide = true;
                self.push(row)
            }
            authored::Error::LegalInput { reason, .. } => {
                let mut row = self.row("legal_source_input", reason);
                // An absent/unframed selected legal entry has no declaration
                // origin. Do not attribute it to the currently queried file.
                row.selection_wide = true;
                self.push(row)
            }
            authored::Error::Files(failures) => {
                let path = self.path;
                for failure in failures
                    .iter()
                    .filter(|failure| &failure.origin.path == path)
                {
                    if &failure.origin.source_sha256 != self.digest {
                        return Err(DiagnosticError::InvalidSource);
                    }
                    let (code, message) = match &failure.error {
                        authored::FileError::Metadata(error) => (
                            error.code(),
                            match error {
                                toml::Error::Limit(resource) => *resource,
                                toml::Error::Invalid { message, .. } => message.as_str(),
                            },
                        ),
                        authored::FileError::YamlMetadata(error) => {
                            (error.code, error.message.as_str())
                        }
                        authored::FileError::NativeBindingRequired => (
                            "native_binding_required",
                            "plain Markdown requires an explicit native-document binding",
                        ),
                        authored::FileError::SiteBindingRequired => (
                            "authored_site_binding_required",
                            "omitted content site requires a configured source binding",
                        ),
                        authored::FileError::Native(authored::NativeError::Framing) => (
                            "native_framing",
                            "unsupported or ambiguous native frontmatter",
                        ),
                        authored::FileError::Native(authored::NativeError::Coordinate) => (
                            "native_coordinate",
                            "native document has invalid source coordinates",
                        ),
                        authored::FileError::Native(authored::NativeError::Route(_)) => (
                            "native_route",
                            "native spelling violates the public-route contract",
                        ),
                        authored::FileError::Native(authored::NativeError::Markdown(error)) => {
                            // Native lowering receives only File::Markdown: its
                            // original body is the whole file, with the same
                            // digest checked above. Framed document errors use
                            // their distinct offset/origin branches instead.
                            self.markdown(error, self.parent(0)?)?;
                            continue;
                        }
                        authored::FileError::Resource(error) => (
                            error.code(),
                            match error {
                                gazette_core::resource::Error::Input { reason, .. } => *reason,
                                gazette_core::resource::Error::Limit(resource) => *resource,
                                gazette_core::resource::Error::Compile(message) => message.as_str(),
                                gazette_core::resource::Error::Source(_) => {
                                    "resource source observation failed"
                                }
                            },
                        ),
                        authored::FileError::Legal(error) => (
                            error.code(),
                            match error {
                                authored::LegalSourceError::Metadata { error, .. } => match error {
                                    toml::Error::Limit(resource) => *resource,
                                    toml::Error::Invalid { message, .. } => message.as_str(),
                                },
                                authored::LegalSourceError::Identity { .. } => {
                                    "legal source identity differs from the selected owner identity"
                                }
                            },
                        ),
                    };
                    let mut row = self.row(code, message);
                    let span = match &failure.error {
                        authored::FileError::Legal(
                            authored::LegalSourceError::Metadata { span, .. }
                            | authored::LegalSourceError::Identity { span, .. },
                        ) => *span,
                        _ => failure.origin.span,
                    };
                    if let Some(span) = span {
                        // Authored origins are already full-document coordinates.
                        row.location =
                            Some(self.locate(span, self.parent(0)?, Precision::Construct)?);
                    }
                    self.push(row)?;
                }
                Ok(())
            }
            authored::Error::Structure {
                error: content::Error::Invalid(errors),
                declarations,
            } => {
                let authoring = authored::AuthoredAuthoring::parse(self.source, Default::default())
                    .ok()
                    .flatten();
                for error in errors {
                    let origins = declarations.get(&error.source);
                    if let Some(origins) = origins {
                        let path = self.path;
                        for origin in origins.iter().filter(|origin| &origin.path == path) {
                            if &origin.source_sha256 != self.digest {
                                return Err(DiagnosticError::InvalidSource);
                            }
                            let message = match error.code {
                                "content_reference" => {
                                    "This reference does not name existing content."
                                }
                                "content_reference_kind" => {
                                    "This reference names the wrong kind of content."
                                }
                                "collection_purpose_conflict" => {
                                    "A nested collection must keep its containing collection's purpose: editorial or documentation."
                                }
                                "collection_navigation_purpose" => {
                                    "Only a documentation collection can declare a documentation outline."
                                }
                                "collection_navigation_empty" => {
                                    "A documentation outline and each group must contain at least one destination."
                                }
                                "collection_navigation_label" => {
                                    "A navigation group needs a nonempty plain-text label without control characters."
                                }
                                "collection_navigation_reference" => {
                                    "This documentation destination does not name an existing content node."
                                }
                                "collection_navigation_outside" => {
                                    "A documentation destination must belong to the declaring collection's subtree."
                                }
                                "collection_navigation_duplicate" => {
                                    "This destination appears more than once after navigation containers are expanded."
                                }
                                "collection_order_reference" => {
                                    "This ordered child does not name an existing content node."
                                }
                                "collection_order_not_child" => {
                                    "An ordered child must already belong directly to this collection."
                                }
                                "collection_order_duplicate" => {
                                    "This child appears more than once in the collection's member order."
                                }
                                "collection_navigation_missing" => {
                                    "The complete documentation outline omits this contained destination."
                                }
                                "content_route_slug" => {
                                    "Add a readable content.slug or an explicit route for this publication's address rules."
                                }
                                "content_route_rule" => {
                                    "Configure a root route prefix for this content kind or supply an explicit route."
                                }
                                "content_route_parent" => {
                                    "The containing collection or journey needs a resolved route before this page can inherit its address."
                                }
                                "content_route_parent_directory" => {
                                    "A derived child address requires a directory route on its containing collection or journey."
                                }
                                _ => "authored content constraint failed",
                            };
                            let mut row = self.row(error.code, message);
                            row.source = Some(&error.source);
                            row.target = error.target.as_ref();
                            if let Some(span) = origin.span {
                                row.location = Some(self.locate(
                                    span,
                                    self.parent(0)?,
                                    Precision::Construct,
                                )?);
                            }
                            let reference_field = match error.code {
                                "collection_navigation_reference"
                                | "collection_navigation_outside" => {
                                    Some(authored::EditorialField::DocumentationNavigation)
                                }
                                "collection_order_reference" | "collection_order_not_child" => {
                                    Some(authored::EditorialField::CollectionOrder)
                                }
                                _ => None,
                            };
                            if matches!(
                                error.code,
                                "content_reference"
                                    | "content_reference_kind"
                                    | "collection_navigation_reference"
                                    | "collection_navigation_outside"
                                    | "collection_order_reference"
                                    | "collection_order_not_child"
                            ) && let (Some(authoring), Some(target)) =
                                (&authoring, &error.target)
                                && let Ok(Some(location)) = authoring.content_reference_location(
                                    self.source,
                                    target,
                                    reference_field,
                                )
                            {
                                row.location = Some(location);
                            }
                            self.push(row)?;
                        }
                    } else {
                        let mut row = self.row(
                            error.code,
                            "authored content constraint has no selected declaration origin",
                        );
                        row.source = Some(&error.source);
                        row.target = error.target.as_ref();
                        row.selection_wide = true;
                        self.push(row)?;
                    }
                }
                Ok(())
            }
            authored::Error::Structure {
                error: content::Error::Limit(resource),
                ..
            } => {
                let mut row = self.row("content_limit", resource);
                row.selection_wide = true;
                self.push(row)
            }
            authored::Error::Notebook(error) => {
                let (message, key) = match error {
                    authored::NotebookError::Input {
                        content, reason, ..
                    } => (*reason, content),
                    authored::NotebookError::Source(authored::SourceBindingError::Missing(
                        content,
                    )) => ("content has no configured source binding", content),
                    authored::NotebookError::Source(authored::SourceBindingError::Home {
                        content,
                        ..
                    }) => (
                        "content home conflicts with its configured source binding",
                        content,
                    ),
                    authored::NotebookError::Source(
                        authored::SourceBindingError::DocumentationMount { landing, reason },
                    ) => (*reason, landing),
                };
                let mut row = self.row(error.code(), message);
                row.source = Some(key);
                if matches!(
                    error,
                    authored::NotebookError::Source(
                        authored::SourceBindingError::DocumentationMount { .. }
                    )
                ) {
                    row.source = None;
                    row.target = Some(key);
                }
                // This failure carries a logical content key but no retained
                // declaration-origin map. Do not guess it from a notebook path.
                row.selection_wide = true;
                self.push(row)
            }
        }
    }
}
