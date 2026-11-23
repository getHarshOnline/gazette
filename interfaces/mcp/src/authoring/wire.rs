//! Named authoring requests, not an opaque LSP protocol tunnel.

use gazette_cli::command::authoring::{EditorContextInput, Selection};
use gazette_cli::sdk::authoring::session;
use rmcp::model::{ErrorData, JsonObject, Tool, ToolAnnotations};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;

use super::{Acknowledgement, queries};
use crate::tools::output_schema;

#[derive(Clone, Copy)]
pub(crate) enum Route {
    Start,
    Import,
    Status,
    Open,
    Change,
    Close,
    End,
    Diagnostics,
    Symbols,
    Search,
    Definition,
    References,
    Hover,
    Assistance,
    ComponentAssistance,
    Completion,
    Repair,
    HeadingCompletion,
    PagePackage,
    Origin,
}

impl Route {
    pub(crate) const ALL: [Self; 20] = [
        Self::Start,
        Self::Import,
        Self::Status,
        Self::Open,
        Self::Change,
        Self::Close,
        Self::End,
        Self::Diagnostics,
        Self::Symbols,
        Self::Search,
        Self::Definition,
        Self::References,
        Self::Hover,
        Self::Assistance,
        Self::ComponentAssistance,
        Self::Completion,
        Self::Repair,
        Self::HeadingCompletion,
        Self::PagePackage,
        Self::Origin,
    ];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Start => "authoring_start",
            Self::Import => "authoring_import_editor_context",
            Self::Status => "authoring_status",
            Self::Open => "authoring_open_document",
            Self::Change => "authoring_change_document",
            Self::Close => "authoring_close_document",
            Self::End => "authoring_close_session",
            Self::Diagnostics => "authoring_diagnostics",
            Self::Symbols => "authoring_document_symbols",
            Self::Search => "authoring_search_symbols",
            Self::Definition => "authoring_definition",
            Self::References => "authoring_references",
            Self::Hover => "authoring_hover",
            Self::Assistance => "authoring_assistance",
            Self::ComponentAssistance => "authoring_component_assistance",
            Self::Completion => "authoring_complete_reference",
            Self::Repair => "authoring_review_repair",
            Self::HeadingCompletion => "authoring_complete_heading",
            Self::PagePackage => "authoring_page_package",
            Self::Origin => "authoring_origin",
        }
    }

    pub(crate) fn descriptor(self) -> Tool {
        let (description, schema) = match self {
            Self::Assistance => (
                "Use authoring_assistance to learn which fields a workspace or authored frontmatter is missing and what its schema allows at a cursor position. One call inspects compiler-owned tolerant workspace or authored-frontmatter structure and Rust-schema facts at an exact current UTF-8 offset. Returns missing-field or closed-literal replacements and schema hover facts under the exact session generation, source digest and overlay version. Strict parsing remains the sole publication-validity owner; no stale facts, native reads or edits. Pass the session's exact current generation; take it from authoring_status when unsure. This tool edits nothing; whether to apply a proposal stays with the author.",
                schema::<Position>(),
            ),
            Self::ComponentAssistance => (
                "Use authoring_component_assistance to learn which Theme components and arguments are valid at a position in Markdown or landing YAML. One call inspects the effective selected Theme's component-name catalog or schema-derived argument property and closed-literal candidates at a current Markdown directive or original landing YAML component offset. Exact UTF-8 replacement ranges and optional data-only argument-schema hover are bound to supplied workspace, authored and typed package baselines plus session generation and digest. No stale result, native read, template execution, interaction detail, edit or publication approval. This tool edits nothing; whether to apply a proposal stays with the author.",
                schema::<Position>(),
            ),
            Self::HeadingCompletion => (
                "Use authoring_complete_heading to complete the heading fragment of a same-document Markdown link. One call proposes the complete compiler-allocated heading set for a parsed literal same-document Markdown link destination. Exact full-source destination preimage, digest, version and generation bind the read-only result. No partial list, incomplete-syntax guessing, unselected reads or native edits; apply the URL-ready fragment verbatim and revalidate the changed source. This tool edits nothing; whether to apply a proposal stays with the author.",
                schema::<Position>(),
            ),
            Self::Origin => (
                "Use authoring_origin to learn where a field's value comes from, explicit or inherited. One call resolves an explicit field or winning inherited composition field against the current retained workspace. Requires exact generation; returns native source digest and UTF-8 span, selected document and overlay version. Invalid current workspace never falls back to old success; unrelated Markdown failures do not hide valid workspace facts. No native reads or writes.",
                schema::<Origin>(),
            ),
            Self::PagePackage => (
                "Use authoring_page_package to learn which page package a document belongs to and what companions it carries. One call inspects compiler-resolved page-package ownership for one exact selected document or opaque retained input. Returns the owning index document, content identity, package root, complete ordered companions, kinds and retained digests. Uses only supplied session bytes; no path inference, native reads, publication admission or edit.",
                schema::<Document>(),
            ),
            Self::Search => (
                "Use authoring_search_symbols to find a declaration or heading by name. One call searches current selected workspace declarations and Markdown headings using Unicode lowercase substring comparison; empty query returns all. Complete source-bound results in logical-path order, or an explicit failure: no ranking truncation, stale facts, native reads or identity normalization. Requires the exact session generation.",
                schema::<Search>(),
            ),
            Self::Completion => (
                "Use authoring_complete_reference to complete a reference while someone is writing it. One call proposes complete native replacements for workspace references; authored site, attribution-principal, related-content and editorial references; or AST-recognized symbolic site-link destinations, including unresolved typed values. Native field kinds select candidates; symbolic page, heading and article-owned notes targets retain exact selected definition evidence without replacing the visible label. Exact source preimage, digest, version and generation bind the read-only result; no stale model, partial list, unselected reads or native edit. Proposed changes require complete native reanalysis for every constraint. This tool edits nothing; whether to apply a proposal stays with the author.",
                schema::<Position>(),
            ),
            Self::Repair => (
                "Use authoring_review_repair to get a proposed fix for one diagnostic. One call returns review-only replacement proposals for one exact current Gazette diagnostic. The request must bind the selected document, generation, overlay version, complete source digest and the byte-for-byte diagnostic emitted by authoring_diagnostics. Stale, altered, invented or unassisted diagnostics are refused or return no proposal. The shared SDK analysis owns candidates; this tool performs no edit, command, native read or automatic mutation. If the diagnostic is stale, altered or unassisted, no proposal comes back: run authoring_diagnostics again. This tool edits nothing; whether to apply a proposal stays with the author.",
                repair_schema(),
            ),
            Self::Import => (
                "Use authoring_import_editor_context to start a session from an editor's exported context. One call validates a complete explicit gazette-editor-context/1 export and returns a native authoring session. Supply either an inline context or one explicit local file with allow_read=true and its complete SHA-256. A failed import creates no usable handle. Close successful imports with authoring_close_session.",
                schema::<EditorContextInput>(),
            ),
            Self::Start => (
                "Use authoring_start to begin an authoring session over workspace, content and manual text you supply. One call selects exact supplied workspace, publication-content and authored-manual baselines for authoring. Manuals receive Markdown editor analysis without publication-content resolution, Theme binding or documentation-target selection. Returns an opaque process-local session and generation. Four sessions maximum; absolute five-minute lifetime, no native reads. Identifiers never authorize filesystem access. When a session has expired or four are open, close the ones you no longer need and start a new one.",
                schema::<Selection>(),
            ),
            Self::Status => (
                "Use authoring_status to check what a session holds before you call anything else on it. One call inspects current selected input identities, overlay versions, analysis state and generation without exposing retained source text. Does not extend the session's lifetime.",
                schema::<Handle>(),
            ),
            Self::Open => (
                "Use authoring_open_document to put a document's edited text into the session. One call opens a full-text overlay for an already selected document. Requires exact session generation; already-open or unknown documents return changed=false. No native file writes.",
                schema::<Overlay>(),
            ),
            Self::Change => (
                "Use authoring_change_document to tell the session that a document's text has changed. One call replaces an open overlay with full text at a strictly increasing editor version and exact session generation. Stale versions return changed=false. Failed current analysis replaces prior success.",
                schema::<Overlay>(),
            ),
            Self::Close => (
                "Use authoring_close_document to drop a document's edited text from the session. One call closes an overlay, restoring its explicitly selected baseline, not rereading disk. Requires exact generation. Unknown/not-open documents return changed=false.",
                schema::<Document>(),
            ),
            Self::End => (
                "Use authoring_close_session when you finish authoring, to release the session and its buffers. One call releases an explicit authoring session and its supplied buffers. Requires exact generation. No file or native operation is affected.",
                schema::<Versioned>(),
            ),
            Self::Diagnostics => (
                "Use authoring_diagnostics to see what is wrong in one document as it stands in the session. One call returns current SDK diagnostics for one exact selected document and generation. Locations are full-file UTF-8 byte spans; selection-wide findings remain explicitly marked. Not publication admission. Report each diagnostic's file, line and identity, then fix the source; authoring_review_repair proposes replacements for one diagnostic.",
                schema::<Document>(),
            ),
            Self::Symbols => (
                "Use authoring_document_symbols to outline a workspace or a Markdown document. One call returns current selected-workspace declaration symbols or Markdown headings with full-file UTF-8 construct spans. These are neither rename edit ranges nor whole-section extents.",
                schema::<Document>(),
            ),
            Self::Definition => (
                "Use authoring_definition to find where something is defined. One call resolves a current selected-workspace declaration/reference, selected-document Markdown heading/footnote, or authored page-resource input/entry at an exact UTF-8 byte offset. Text targets carry selected document, version, logical path, digest and full-file location. A page resource carries its explicitly selected opaque companion URI/path/digest and compiled destinations without fabricated source coordinates. Returns null when no native association exists; never reads unselected files.",
                schema::<Position>(),
            ),
            Self::References => (
                "Use authoring_references to find every place something is used. One call resolves current workspace TOML or selected-document Markdown references at a UTF-8 byte offset, with explicit declaration inclusion and per-target source/version binding. Uses the same native associations as LSP; no unselected source reads.",
                schema::<References>(),
            ),
            Self::Hover => (
                "Use authoring_hover to learn what an identity or typed reference means at a position. One call inspects a current selected-workspace identity declaration or typed reference at an exact UTF-8 byte offset. Returns the original compiler symbol, source digest and occurrence precision under the current session generation. Site identities additionally return their compiler-resolved theme/capability composition and winning declaration layers, not build readiness. Names, arbitrary fields and selected Markdown return null; invalid current workspace analysis never borrows prior success. No source reads, edits, field-documentation catalog or publication approval.",
                schema::<Position>(),
            ),
        };
        let output = match self {
            Self::Assistance => output_schema::<
                queries::Report<'static, Option<gazette_cli::sdk::authoring::Assistance<'static>>>,
            >(),
            Self::ComponentAssistance => output_schema::<
                queries::Report<
                    'static,
                    Option<gazette_cli::sdk::authoring::ComponentAssistance<'static>>,
                >,
            >(),
            Self::HeadingCompletion => output_schema::<
                queries::Report<
                    'static,
                    Option<gazette_cli::sdk::authoring::HeadingCompletion<'static>>,
                >,
            >(),
            Self::Origin => output_schema::<session::Origin<'static>>(),
            Self::PagePackage => output_schema::<
                queries::Report<
                    'static,
                    Option<gazette_cli::sdk::authoring::PagePackageOwnership<'static>>,
                >,
            >(),
            Self::Search => output_schema::<gazette_cli::sdk::authoring::SymbolSearch<'static>>(),
            Self::Completion => output_schema::<
                queries::Report<'static, Option<queries::ReferenceProposal<'static>>>,
            >(),
            Self::Repair => {
                output_schema::<queries::Report<'static, Option<queries::RepairReview<'static>>>>()
            }
            Self::Start | Self::Import | Self::Open | Self::Change | Self::Close | Self::End => {
                output_schema::<Acknowledgement<'static>>()
            }
            Self::Status => output_schema::<queries::Status<'static>>(),
            Self::Diagnostics => output_schema::<
                queries::Report<'static, Vec<gazette_cli::sdk::authoring::Diagnostic<'static>>>,
            >(),
            Self::Symbols => {
                output_schema::<queries::Report<'static, Vec<queries::Symbol<'static>>>>()
            }
            Self::Definition => output_schema::<
                queries::Report<'static, Option<queries::SelectedDefinition<'static>>>,
            >(),
            Self::References => {
                output_schema::<queries::Report<'static, Vec<queries::SelectedLocation<'static>>>>()
            }
            Self::Hover => output_schema::<
                queries::Report<
                    'static,
                    Option<gazette_cli::sdk::authoring::WorkspaceIdentity<'static>>,
                >,
            >(),
        };
        Tool::new(self.name(), description, schema)
            .with_raw_output_schema(output)
            .with_annotations(
                ToolAnnotations::new()
                    .read_only(matches!(
                        self,
                        Self::Status
                            | Self::Diagnostics
                            | Self::Symbols
                            | Self::Search
                            | Self::Definition
                            | Self::References
                            | Self::Hover
                            | Self::Assistance
                            | Self::ComponentAssistance
                            | Self::Completion
                            | Self::Repair
                            | Self::HeadingCompletion
                            | Self::Origin
                            | Self::PagePackage
                    ))
                    .destructive(matches!(self, Self::Change | Self::Close | Self::End))
                    .idempotent(matches!(
                        self,
                        Self::Status
                            | Self::Diagnostics
                            | Self::Symbols
                            | Self::Search
                            | Self::Definition
                            | Self::References
                            | Self::Hover
                            | Self::Assistance
                            | Self::ComponentAssistance
                            | Self::Completion
                            | Self::Repair
                            | Self::HeadingCompletion
                            | Self::Origin
                            | Self::PagePackage
                    ))
                    .open_world(matches!(self, Self::Import)),
            )
    }

    pub(crate) fn decode(self, arguments: JsonObject) -> Result<Input, ErrorData> {
        let value = Value::Object(arguments);
        Ok(match self {
            Self::Import => Input::Import(decode(value)?),
            Self::Start => {
                if !value.get("workspace").is_some_and(Value::is_object)
                    || !value
                        .get("documents")
                        .and_then(Value::as_array)
                        .is_some_and(|documents| documents.iter().all(Value::is_object))
                    || value.get("manuals").is_some_and(|manuals| {
                        !manuals
                            .as_array()
                            .is_some_and(|manuals| manuals.iter().all(Value::is_object))
                    })
                    || value.get("page_inputs").is_some_and(|inputs| {
                        !inputs
                            .as_array()
                            .is_some_and(|inputs| inputs.iter().all(Value::is_object))
                    })
                {
                    return Err(ErrorData::invalid_params(
                        "Selected sources must be named objects",
                        None,
                    ));
                }
                Input::Start(decode(value)?)
            }
            Self::Status => Input::Status(decode(value)?),
            Self::Open => Input::Open(decode(value)?),
            Self::Change => Input::Change(decode(value)?),
            Self::Close => Input::Close(decode(value)?),
            Self::End => Input::End(decode(value)?),
            Self::Diagnostics => Input::Diagnostics(decode(value)?),
            Self::Symbols => Input::Symbols(decode(value)?),
            Self::Search => Input::Search(decode(value)?),
            Self::Definition => Input::Definition(decode(value)?),
            Self::References => Input::References(decode(value)?),
            Self::Hover => Input::Hover(decode(value)?),
            Self::Assistance => Input::Assistance(decode(value)?),
            Self::ComponentAssistance => Input::ComponentAssistance(decode(value)?),
            Self::Completion => Input::Completion(decode(value)?),
            Self::Repair => {
                let diagnostic = value.get("diagnostic").and_then(Value::as_object);
                if value.get("version").is_none()
                    || diagnostic.is_none_or(|diagnostic| {
                        [
                            "path",
                            "source_sha256",
                            "code",
                            "message",
                            "field",
                            "key",
                            "source",
                            "target",
                            "location",
                            "selection_wide",
                        ]
                        .iter()
                        .any(|field| !diagnostic.contains_key(*field))
                    })
                {
                    return Err(ErrorData::invalid_params(
                        "Review repair requires an explicit version (including null) and one complete typed diagnostic",
                        None,
                    ));
                }
                Input::Repair(Box::new(decode(value)?))
            }
            Self::HeadingCompletion => Input::HeadingCompletion(decode(value)?),
            Self::PagePackage => Input::PagePackage(decode(value)?),
            Self::Origin => {
                if !value.pointer("/query/query").is_some_and(Value::is_object) {
                    return Err(ErrorData::invalid_params(
                        "Origin requires a named native query",
                        None,
                    ));
                }
                Input::Origin(decode(value)?)
            }
        })
    }
}

fn schema<T: JsonSchema>() -> JsonObject {
    schemars::schema_for!(T)
        .as_object()
        .expect("request object schema")
        .clone()
}

fn repair_schema() -> JsonObject {
    let mut schema = schema::<Repair>();
    let required = schema
        .entry("required")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .expect("request required fields are an array");
    if !required.iter().any(|field| field == "version") {
        required.push(Value::String("version".into()));
    }
    schema
}

fn decode<T: DeserializeOwned>(value: Value) -> Result<T, ErrorData> {
    serde_json::from_value(value).map_err(|error| {
        ErrorData::invalid_params(format!("Invalid authoring arguments: {error}"), None)
    })
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Handle {
    /// Opaque handle returned by `authoring_start`; scoped to this server process.
    pub(super) session: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Versioned {
    pub(super) session: String,
    /// Exact current session generation; stale requests cannot change state.
    #[schemars(range(max = u64::MAX))]
    pub(super) generation: u64,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Document {
    pub(super) session: String,
    #[schemars(range(max = u64::MAX))]
    pub(super) generation: u64,
    /// Identifier of a selected baseline, not an implicit source lookup.
    pub(super) document: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Overlay {
    pub(super) session: String,
    #[schemars(range(max = u64::MAX))]
    pub(super) generation: u64,
    pub(super) document: String,
    /// Editor overlay version, strictly increasing for changes.
    #[schemars(range(min = i32::MIN, max = i32::MAX))]
    pub(super) version: i32,
    /// Complete current text, not a patch or native write.
    pub(super) text: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Position {
    pub(super) session: String,
    #[schemars(range(max = u64::MAX))]
    pub(super) generation: u64,
    pub(super) document: String,
    /// UTF-8 byte offset in the current complete source, not a UTF-16 position.
    #[schemars(range(max = usize::MAX))]
    pub(super) offset: usize,
}

/// Exact transport projection previously returned by `authoring_diagnostics`.
#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct BoundDiagnostic {
    pub(super) path: gazette_cli::sdk::source::EntryPath,
    pub(super) source_sha256: gazette_cli::sdk::source::Sha256Digest,
    pub(super) code: String,
    pub(super) message: String,
    pub(super) field: RequiredNullable<String>,
    pub(super) key: RequiredNullable<String>,
    pub(super) source: RequiredNullable<String>,
    pub(super) target: RequiredNullable<String>,
    pub(super) location: RequiredNullable<BoundLocation>,
    pub(super) selection_wide: bool,
}

/// A required JSON property whose value may explicitly be null.
#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(transparent)]
pub(super) struct RequiredNullable<T>(pub(super) Option<T>);

#[derive(Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct BoundLocation {
    pub(super) span: BoundSpan,
    pub(super) precision: BoundPrecision,
}

#[derive(Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct BoundSpan {
    #[schemars(range(max = usize::MAX))]
    pub(super) start: usize,
    #[schemars(range(max = usize::MAX))]
    pub(super) end: usize,
}

#[derive(Clone, Copy, Deserialize, Serialize, JsonSchema)]
pub(super) enum BoundPrecision {
    Construct,
    Exact,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Repair {
    pub(super) session: String,
    #[schemars(range(max = u64::MAX))]
    pub(super) generation: u64,
    pub(super) document: String,
    /// Exact current overlay version; null denotes the retained baseline.
    #[schemars(range(min = i32::MIN, max = i32::MAX))]
    pub(super) version: Option<i32>,
    pub(super) source_sha256: gazette_cli::sdk::source::Sha256Digest,
    /// UTF-8 byte offset where a shared replacement is requested.
    #[schemars(range(max = usize::MAX))]
    pub(super) offset: usize,
    pub(super) diagnostic: BoundDiagnostic,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct References {
    pub(super) session: String,
    #[schemars(range(max = u64::MAX))]
    pub(super) generation: u64,
    pub(super) document: String,
    #[schemars(range(max = usize::MAX))]
    pub(super) offset: usize,
    pub(super) include_declaration: bool,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Search {
    pub(super) session: String,
    /// Exact selected-source generation; no stale success substitution.
    #[schemars(range(max = u64::MAX))]
    pub(super) generation: u64,
    /// Unicode lowercase substring match; empty means the complete selected symbol set.
    pub(super) query: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Origin {
    pub(super) session: String,
    #[schemars(range(max = u64::MAX))]
    pub(super) generation: u64,
    pub(super) query: session::OriginQuery,
}

pub(crate) enum Input {
    Start(Selection),
    Import(Box<EditorContextInput>),
    Status(Handle),
    Open(Overlay),
    Change(Overlay),
    Close(Document),
    End(Versioned),
    Diagnostics(Document),
    Symbols(Document),
    Search(Search),
    Definition(Position),
    References(References),
    Hover(Position),
    Assistance(Position),
    ComponentAssistance(Position),
    Completion(Position),
    Repair(Box<Repair>),
    HeadingCompletion(Position),
    PagePackage(Document),
    Origin(Origin),
}

impl Input {
    pub(super) fn handle(&self) -> &str {
        match self {
            Self::Start(_) | Self::Import(_) => unreachable!("start has no handle"),
            Self::Status(i) => &i.session,
            Self::Open(i) | Self::Change(i) => &i.session,
            Self::Close(i) | Self::Diagnostics(i) | Self::Symbols(i) => &i.session,
            Self::End(i) => &i.session,
            Self::Definition(i)
            | Self::Hover(i)
            | Self::Assistance(i)
            | Self::ComponentAssistance(i)
            | Self::Completion(i)
            | Self::HeadingCompletion(i) => &i.session,
            Self::PagePackage(i) => &i.session,
            Self::Repair(i) => &i.session,
            Self::References(i) => &i.session,
            Self::Search(i) => &i.session,
            Self::Origin(i) => &i.session,
        }
    }

    pub(super) fn generation(&self) -> Option<u64> {
        Some(match self {
            Self::Start(_) | Self::Import(_) | Self::Status(_) => return None,
            Self::Open(i) | Self::Change(i) => i.generation,
            Self::Close(i) | Self::Diagnostics(i) | Self::Symbols(i) => i.generation,
            Self::End(i) => i.generation,
            Self::Definition(i)
            | Self::Hover(i)
            | Self::Assistance(i)
            | Self::ComponentAssistance(i)
            | Self::Completion(i)
            | Self::HeadingCompletion(i) => i.generation,
            Self::PagePackage(i) => i.generation,
            Self::Repair(i) => i.generation,
            Self::References(i) => i.generation,
            Self::Search(i) => i.generation,
            Self::Origin(i) => i.generation,
        })
    }

    pub(super) fn mutates(&self) -> bool {
        matches!(
            self,
            Self::Start(_)
                | Self::Import(_)
                | Self::Open(_)
                | Self::Change(_)
                | Self::Close(_)
                | Self::End(_)
        )
    }
}
