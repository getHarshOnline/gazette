//! Native documentation custody and configured output under one existing writer handle.

use std::path::PathBuf;

use gazette_cli::command::{Kernel, store_session as command};
use gazette_cli::sdk::{build, render::LimitedWriter, source, store};
use rmcp::model::{CallToolResult, ErrorData, JsonObject, Tool, ToolAnnotations};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use serde_json::{Value, value::RawValue};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "DocumentationMaterializationInput")]
pub(super) struct Input {
    /// Existing ordinary writer custody; never acquired or closed by this operation.
    #[schemars(regex(pattern = "^[0-9a-f]{64}$"))]
    session: String,
    /// The same exact supplied/local documentation input and native generator mapping.
    #[serde(deserialize_with = "decode_documentation")]
    documentation: crate::documentation::Input,
    /// Independently selected immutable operation coordinate, not inferred from content.
    id: source::Sha256Digest,
    /// Independent permission for configured native output transitions.
    #[serde(default)]
    allow_write: bool,
    /// Independent durable intent and completion permission.
    #[serde(default)]
    allow_record: bool,
    /// Explicit absolute peer roots, protected in addition to acquired inputs and journal.
    #[serde(default)]
    #[schemars(schema_with = "protected_schema")]
    protected: Vec<PathBuf>,
    /// Complete observed/proposed configured output bounds, distinct from assembly output.
    #[serde(default)]
    limits: source::Limits,
    /// Complete private materialization evidence bound; does not expand response capacity.
    #[serde(default = "evidence_bytes")]
    #[schemars(range(max = usize::MAX))]
    evidence_bytes: usize,
    #[serde(skip)]
    #[schemars(skip)]
    digest: Option<source::Sha256Digest>,
}

fn decode_documentation<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<crate::documentation::Input, D::Error> {
    let Value::Object(arguments) = Value::deserialize(deserializer)? else {
        return Err(D::Error::custom(
            "documentation must be the shared named input",
        ));
    };
    crate::documentation::decode(arguments).map_err(D::Error::custom)
}

fn protected_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({"type":"array","items":{"type":"string","pattern":"^/"}})
}

fn evidence_bytes() -> usize {
    build::MaterializationOptions::default().evidence_bytes
}

fn native_documentation_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
    // Embed the SDK's actual full/compact transport schema, not an arbitrary-JSON
    // permission. Its references must close over this outer schema's definitions.
    let mut native = build::documentation_output::schema();
    if let Some(Value::Object(definitions)) = native.remove("$defs") {
        for (name, definition) in definitions {
            if let Some(existing) = generator.definitions_mut().get(&name) {
                assert_eq!(
                    existing, &definition,
                    "native documentation schema collision"
                );
            } else {
                generator.definitions_mut().insert(name, definition);
            }
        }
    }
    schemars::json_schema!({"anyOf":[native,{"type":"null"}]})
}

#[derive(Serialize, JsonSchema)]
struct DocumentationMaterializationReport<'a> {
    session: &'a str,
    /// Actual retained custody, not a claim that a failed writer is healthy.
    active: bool,
    acquired_header: Option<store::JournalHeader>,
    request_sha256: source::Sha256Digest,
    id: source::Sha256Digest,
    /// Original native encoded JSON, embedded without stringification or source reconstruction.
    #[schemars(schema_with = "native_documentation_schema")]
    documentation: Option<&'a RawValue>,
    #[serde(flatten)]
    outcome: DocumentationMaterializationOutcome<'a>,
}

#[derive(Serialize, JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
enum DocumentationMaterializationOutcome<'a> {
    Materialized {
        materialization: build::MaterializationInspection<'a>,
    },
    /// Configured output was not dispatched; native documentation may retain earlier jobs.
    NotAttempted {
        failure: DocumentationMaterializationFailure<'a>,
    },
    PlanFailed {
        failure: build::MaterializationErrorInspection<'a>,
    },
    WriteFailed {
        failure: build::MaterializationErrorInspection<'a>,
    },
    /// Full materialization evidence unavailable; original documentation custody stays complete.
    Delivery {
        code: &'static str,
        complete_evidence: bool,
        may_have_effects: bool,
    },
}

#[derive(Serialize, JsonSchema)]
#[serde(tag = "phase", rename_all = "snake_case")]
enum DocumentationMaterializationFailure<'a> {
    Request {
        code: &'a str,
        reason: String,
    },
    Before {
        code: &'a str,
        reason: String,
    },
    DocumentationDelivery {
        code: &'static str,
    },
    MissingAssembly {
        code: &'static str,
    },
    Permission {
        failure: build::MaterializationErrorInspection<'a>,
    },
}

pub(super) fn descriptor() -> Tool {
    Tool::new(
        "materialize_docs",
        "Use materialize_docs to write a site's documentation build input to its configured output under a writer session you already hold. One call assembles the exact configured documentation repository once through the shared supplied/local workspace and optional native generator sequence, then materializes its complete retained build-input tree under that ordinary writer session. Requires documentation.local=true and documentation.allow_read=true, independent allow_write=true and allow_record=true; generation retains its independent stage/execute permissions. Fresh output_parent/preparation retention is incompatible with configured output. Relative documentation_repository.path requires a loaded local workspace owner, not the MCP process directory. No writer acquisition, close, retry, rollback, hosted compatibility or publication approval is implied. Complete original documentation JSON, including every generator job and retained path, is preserved in full and compact delivery. Combined custody is reserved before source/provider/history/output work. Missing full materialization evidence is an error retaining original handle/header/request digest/id plus native documentation custody and may-have-effects, never a successful partial result. Native violated-contract custody is retained losslessly without a bounded-delivery guarantee. Explicit inspect_store_session/close_store_session remain the lifetime owner; history is not a current filesystem observation or write permission. Publication approval stays with the publisher.",
        schemars::schema_for!(Input).ensure_object().clone(),
    )
    .with_raw_output_schema(crate::tools::output_schema::<DocumentationMaterializationReport<'static>>())
    .with_annotations(ToolAnnotations::new().read_only(false).destructive(true).idempotent(false).open_world(true))
}

pub(super) fn decode(arguments: JsonObject) -> Result<Input, ErrorData> {
    let value = Value::Object(arguments);
    if value
        .get("limits")
        .is_some_and(|limits| !limits.is_object())
    {
        return Err(invalid());
    }
    let mut bytes = LimitedWriter::new(Vec::new(), 4 << 20);
    serde_json::to_writer(&mut bytes, &value).map_err(|_| invalid())?;
    let digest = source::Sha256Digest::of(&bytes.into_inner());
    let mut input: Input = serde_json::from_value(value).map_err(|_| invalid())?;
    if input.session.len() != 64
        || !input
            .session
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        || input.protected.iter().any(|path| !path.is_absolute())
    {
        return Err(invalid());
    }
    input.digest = Some(digest);
    Ok(input)
}

fn invalid() -> ErrorData {
    ErrorData::invalid_params(
        "Use an exact ordinary writer session, shared documentation input, native operation digest and absolute protected roots; no documentation operation started",
        None,
    )
}

fn allowance(
    session: &str,
    digest: source::Sha256Digest,
    id: source::Sha256Digest,
    maximum: usize,
) -> Result<usize, ErrorData> {
    let witness = DocumentationMaterializationReport {
        session,
        active: true,
        acquired_header: Some(store::JournalHeader {
            schema_version: u32::MAX,
            journal_id: [255; 32],
            journal_epoch: u64::MAX,
            writer_fence: u64::MAX,
        }),
        request_sha256: digest,
        id,
        documentation: None,
        outcome: DocumentationMaterializationOutcome::Delivery {
            code: "documentation_materialization_response_unavailable",
            complete_evidence: false,
            may_have_effects: true,
        },
    };
    let mut bytes = LimitedWriter::new(Vec::new(), maximum);
    serde_json::to_writer(&mut bytes, &witness).map_err(|_| capacity())?;
    // Charge the null as well: substituting an admitted raw JSON response can
    // never expand beyond this measured outer overhead plus native allowance.
    maximum
        .checked_sub(bytes.into_inner().len())
        .filter(|bytes| *bytes > 0)
        .ok_or_else(capacity)
}

fn capacity() -> ErrorData {
    ErrorData::invalid_params(
        "Combined documentation/output custody cannot fit the response ceiling; no source, provider, history or output work started",
        None,
    )
}

impl Input {
    pub(super) fn execute(
        self,
        kernel: &Kernel,
        maximum: usize,
    ) -> Result<CallToolResult, ErrorData> {
        let digest = self.digest.expect("shared decoder binds exact request");
        let native_bytes = allowance(&self.session, digest, self.id, maximum)?;
        let result = kernel.execute(command::MaterializeDocumentation {
            handle: self.session,
            documentation: self.documentation.into_command(native_bytes),
            id: self.id,
            protected: self.protected,
            limits: self.limits,
            options: build::MaterializationOptions {
                allow_write: self.allow_write,
                history: store::MaterializationWriteOptions {
                    allow_record: self.allow_record,
                },
                evidence_bytes: self.evidence_bytes,
            },
            reserve: |handle: &str, _: &command::Selection, _: Option<store::JournalHeader>| {
                // Reprove combined custody under the original held writer before
                // admitting the shared native documentation operation's quota.
                if allowance(handle, digest, self.id, maximum)? < native_bytes {
                    return Err(capacity());
                }
                Ok::<(), ErrorData>(())
            },
        });
        let report = match result {
            Ok(report) => report,
            Err(error) => return crate::store_session::command_failure(error, maximum),
        };
        if let command::DocumentationMaterializationOutcome::Contract { evidence } = &report.outcome
        {
            let evidence = match evidence {
                Ok(evidence) => evidence.clone(),
                Err(error) => serde_json::json!({
                    "serialization_error":error.cause.to_string(),"custody":error.custody,
                }),
            };
            return Err(ErrorData::internal_error(
                "Native documentation custody contract violated after work; exact SDK custody follows without a bounded-delivery guarantee; no configured-output dispatch or retry is implied",
                Some(
                    serde_json::json!({"session":report.session,"active":report.active,
                    "acquired_header":report.acquired_header,"request_sha256":digest,
                    "id":report.id,"native_sdk_custody":evidence}),
                ),
            ));
        }
        let documentation = report.documentation.as_ref().map(|response| {
            serde_json::from_slice::<&RawValue>(&response.bytes)
        }).transpose().map_err(|_| ErrorData::internal_error(
            "Native encoded documentation JSON invalid after work; exact bytes and original custody follow without a bounded-delivery guarantee; no retry",
            Some(serde_json::json!({"session":report.session,"active":report.active,
                "acquired_header":report.acquired_header,"request_sha256":digest,"id":report.id,
                "documentation_bytes":report.documentation.as_ref().map(|response| &response.bytes)})),
        ))?;
        if documentation.is_none()
            && matches!(
                &report.outcome,
                command::DocumentationMaterializationOutcome::Materialized(_)
            )
        {
            return Err(ErrorData::internal_error(
                "Native materialization omitted original documentation custody after work; no complete delivery or retry is implied",
                Some(
                    serde_json::json!({"session":report.session,"active":report.active,
                    "acquired_header":report.acquired_header,"request_sha256":digest,"id":report.id}),
                ),
            ));
        }
        use command::{
            DocumentationMaterializationFailure as F, DocumentationMaterializationOutcome as O,
        };
        let outcome = match &report.outcome {
            O::Materialized(result) => DocumentationMaterializationOutcome::Materialized {
                materialization: result.inspect(),
            },
            O::PlanError(error) => DocumentationMaterializationOutcome::PlanFailed {
                failure: error.inspect(),
            },
            O::WriteError(error) => DocumentationMaterializationOutcome::WriteFailed {
                failure: error.inspect(),
            },
            O::NotAttempted(error) => DocumentationMaterializationOutcome::NotAttempted {
                failure: match error {
                    F::Request(error) => DocumentationMaterializationFailure::Request {
                        code: error.code(),
                        reason: error.message().to_owned(),
                    },
                    F::Before(error) => DocumentationMaterializationFailure::Before {
                        code: error.code(),
                        reason: error.to_string(),
                    },
                    F::Delivery => DocumentationMaterializationFailure::DocumentationDelivery {
                        code: "documentation_delivery_failed",
                    },
                    F::MissingAssembly => DocumentationMaterializationFailure::MissingAssembly {
                        code: "documentation_assembly_missing",
                    },
                    F::Permission(error) => DocumentationMaterializationFailure::Permission {
                        failure: error.inspect(),
                    },
                },
            },
            O::Contract { .. } => unreachable!("lossless exceptional custody handled above"),
        };
        let full = DocumentationMaterializationReport {
            session: &report.session,
            active: report.active,
            acquired_header: report.acquired_header,
            request_sha256: digest,
            id: report.id,
            documentation,
            outcome,
        };
        if let Ok(encoded) = crate::tools::encode_complete(
            &full,
            !matches!(&report.outcome, O::Materialized(_)),
            maximum,
        ) {
            return Ok(encoded);
        }
        let fallback = DocumentationMaterializationReport {
            session: &report.session,
            active: report.active,
            acquired_header: report.acquired_header,
            request_sha256: digest,
            id: report.id,
            documentation,
            outcome: DocumentationMaterializationOutcome::Delivery {
                code: "documentation_materialization_response_unavailable",
                complete_evidence: false,
                may_have_effects: true,
            },
        };
        crate::tools::encode_complete(&fallback, true, maximum).map_err(|_| ErrorData::internal_error(
            "Reserved combined documentation custody violated after work; original native response and coordinates follow without a bounded-delivery guarantee; no retry",
            Some(serde_json::to_value(&fallback).expect("same SDK-owned raw JSON and native fixed coordinates")),
        ))
    }
}
