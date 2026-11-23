//! Current-buffer origins reuse the native parser/resolver, never an editor-side merge.

use super::Session;
use crate::{
    render::LimitedWriter,
    source::{EntryPath, Span},
    workspace,
};
use serde::{Deserialize, Serialize};

/// Select one existing native query; transports do not define another field catalog.
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum OriginQuery {
    /// An explicitly declared object field.
    Field {
        /// Existing native declaration query, including its exact key/index path.
        query: workspace::FieldQuery,
    },
    /// The winning inherited or overridden composition field.
    Composition {
        /// Existing native selection query, including its relative key/index path.
        query: workspace::CompositionOriginQuery,
    },
}

/// Exact compiler-owned origin evidence.
#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", content = "origin", rename_all = "kebab-case")]
pub enum OriginValue {
    /// Explicit native declaration coordinates.
    Field(workspace::FieldOrigin),
    /// Winning native composition coordinates and declaration origin.
    Composition(workspace::CompositionFieldOrigin),
    /// Explicit native declaration coordinates in one selected workspace document.
    DocumentField(workspace::DocumentFieldOrigin),
    /// Winning native composition coordinates and containing workspace document.
    DocumentComposition(workspace::DocumentCompositionFieldOrigin),
}

impl OriginValue {
    /// Half-open UTF-8 coordinates; protocol adapters may project but never reinterpret them.
    pub fn span(&self) -> Span {
        match self {
            Self::Field(v) => v.span,
            Self::Composition(v) => v.span,
            Self::DocumentField(v) => v.span,
            Self::DocumentComposition(v) => v.span,
        }
    }
}

/// Complete origin bound to the selected current workspace and session generation.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct Origin<'a> {
    /// Opaque selected client identifier, never a filesystem read request.
    pub document: &'a str,
    /// Original logical source path.
    pub path: &'a EntryPath,
    /// Exact generation at which the query ran.
    #[schemars(range(max = u64::MAX))]
    pub generation: u64,
    /// Current overlay version, or null for the retained baseline.
    pub version: Option<i32>,
    /// Existing compiler evidence, including the complete source digest.
    pub value: OriginValue,
}

/// Redacted query refusal; neither prior success nor a partial origin escapes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OriginError {
    /// The caller's generation is not current.
    Generation,
    /// Current workspace parsing or semantic resolution failed.
    Workspace,
    /// The requested native declaration identity does not exist.
    UnknownIdentity,
    /// No explicit field exists at the requested native path.
    NoExplicitField,
    /// Complete query/result exceeds the session's navigation ceiling.
    Limit,
}
impl OriginError {
    /// Stable shared diagnostic code, never rejected source text.
    pub fn code(self) -> &'static str {
        match self {
            Self::Generation => "authoring_stale_generation",
            Self::Workspace => "authoring_origin_workspace",
            Self::UnknownIdentity => "authoring_origin_identity",
            Self::NoExplicitField => "authoring_origin_field",
            Self::Limit => "authoring_origin_limit",
        }
    }
}
impl std::fmt::Display for OriginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code())
    }
}
impl std::error::Error for OriginError {}

fn bounded(value: &impl Serialize, maximum: usize) -> Result<(), OriginError> {
    let mut output = LimitedWriter::new(std::io::sink(), maximum);
    serde_json::to_writer(&mut output, value).map_err(|_| OriginError::Limit)?;
    if output.exceeded() {
        return Err(OriginError::Limit);
    }
    Ok(())
}

impl Session {
    /// Resolve an origin against the exact retained current workspace, without native I/O.
    ///
    /// Ephemeral parsing uses the existing workspace source/node/depth limits. No borrowed
    /// AST is cached and no second inheritance algorithm exists. Unrelated corpus failures
    /// do not prevent valid workspace queries. Query and complete result use the session's
    /// navigation-byte ceiling; callers may apply smaller transport output bounds.
    ///
    /// # Errors
    /// Rejects stale generations, invalid current workspace, absent declarations/fields and bounds.
    pub fn origin(
        &self,
        expected_generation: u64,
        query: &OriginQuery,
    ) -> Result<Origin<'_>, OriginError> {
        if expected_generation != self.generation {
            return Err(OriginError::Generation);
        }
        bounded(query, self.limits.analysis.navigation_bytes)?;
        let root = self
            .files
            .get(&self.workspace)
            .ok_or(OriginError::Workspace)?;
        let map_error = |error| match error {
            workspace::OriginError::UnknownIdentity => OriginError::UnknownIdentity,
            workspace::OriginError::NoExplicitField => OriginError::NoExplicitField,
        };
        let (document, value) = match &self.workspace_documents {
            None => {
                let parsed =
                    workspace::ParsedWorkspace::parse(root.text(), self.limits.analysis.workspace)
                        .map_err(|_| OriginError::Workspace)?;
                let value = match query {
                    OriginQuery::Field { query } => parsed.origin(query).map(OriginValue::Field),
                    OriginQuery::Composition { query } => parsed
                        .composition_origin(query)
                        .map(OriginValue::Composition),
                }
                .map_err(map_error)?;
                ((self.workspace.as_str(), root), value)
            }
            Some(members) => {
                let parsed = workspace::ParsedWorkspaceDocuments::parse(
                    root.path(),
                    members.iter().filter_map(|id| {
                        self.files
                            .get(id)
                            .map(|document| (document.path(), document.text().as_bytes()))
                    }),
                    self.limits.analysis.workspace_documents,
                )
                .map_err(|_| OriginError::Workspace)?;
                let value = match query {
                    OriginQuery::Field { query } => {
                        parsed.origin(query).map(OriginValue::DocumentField)
                    }
                    OriginQuery::Composition { query } => parsed
                        .composition_origin(query)
                        .map(OriginValue::DocumentComposition),
                }
                .map_err(map_error)?;
                let origin_path = match &value {
                    OriginValue::DocumentField(origin) => &origin.document,
                    OriginValue::DocumentComposition(origin) => &origin.document,
                    OriginValue::Field(_) | OriginValue::Composition(_) => unreachable!(),
                };
                let document = members
                    .iter()
                    .filter_map(|id| self.files.get_key_value(id))
                    .find(|(_, document)| document.path() == origin_path)
                    .ok_or(OriginError::Workspace)?;
                ((document.0.as_str(), document.1), value)
            }
        };
        let result = Origin {
            document: document.0,
            path: document.1.path(),
            generation: self.generation,
            version: document.1.version(),
            value,
        };
        bounded(&result, self.limits.analysis.navigation_bytes)?;
        Ok(result)
    }
}
