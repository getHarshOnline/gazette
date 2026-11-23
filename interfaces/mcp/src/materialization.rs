//! Configured output effects through the same already-held ordinary writer command.

use std::collections::BTreeMap;

use gazette_cli::command::{Kernel, store_session as command};
use gazette_cli::sdk::{build, render::LimitedWriter, source, store, workspace::Key};
use rmcp::model::{CallToolResult, ErrorData, JsonObject, Tool, ToolAnnotations};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Input {
    /// An existing ordinary writer handle from `open_store_session`; never reacquired here.
    #[schemars(regex(pattern = "^[0-9a-f]{64}$"))]
    session: String,
    /// The exact same permission-checked publication producer as `render_sites`.
    #[schemars(schema_with = "crate::publication::input_schema")]
    publication: crate::publication::Input,
    /// One distinct caller-retained operation coordinate for every selected site.
    ids: BTreeMap<Key, source::Sha256Digest>,
    /// Optional complete-set configured repository setup, with independent native permissions.
    #[serde(default)]
    #[schemars(with = "BTreeMap<Key, store::CheckoutSelection>")]
    checkouts: Option<BTreeMap<Key, store::CheckoutSelection>>,
    /// Independent permission for configured native output transitions.
    #[serde(default)]
    allow_write: bool,
    /// Independent permission to persist intent and exact read-back completion.
    #[serde(default)]
    allow_record: bool,
    /// Complete-set existing/proposed output limits, distinct from render limits.
    #[serde(default)]
    limits: source::Limits,
    /// Aggregate retained private build evidence ceiling; never expands transport capacity.
    #[serde(default = "evidence_bytes")]
    #[schemars(range(max = usize::MAX))]
    evidence_bytes: usize,
    #[serde(skip)]
    #[schemars(skip)]
    digest: Option<source::Sha256Digest>,
}

fn evidence_bytes() -> usize {
    build::MaterializationOptions::default().evidence_bytes
}

#[derive(Serialize, JsonSchema)]
struct Report<'a> {
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    checkouts: &'a BTreeMap<Key, gazette_cli::sdk::release::OutputCheckoutReport>,
    session: &'a str,
    /// Custody, not a guarantee that a failed writer remains usable.
    active: bool,
    acquired_header: Option<store::JournalHeader>,
    request_sha256: source::Sha256Digest,
    #[serde(flatten)]
    outcome: Outcome<'a>,
}

#[derive(Serialize, JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
enum Outcome<'a> {
    Materialized {
        sites: BTreeMap<&'a Key, build::MaterializationInspection<'a>>,
    },
    Failed {
        failure: Failure<'a>,
    },
    /// Native execution finished, but this carrier cannot deliver complete evidence.
    /// Original coordinates remain available for explicit inspection; no retry is implied.
    Delivery {
        code: &'static str,
        ids: &'a BTreeMap<Key, source::Sha256Digest>,
        complete_evidence: bool,
        may_have_effects: bool,
    },
}

#[derive(Serialize, JsonSchema)]
#[serde(tag = "phase", rename_all = "snake_case")]
enum Failure<'a> {
    Checkout {
        evidence: &'a store::CheckoutSetError,
    },
    Render {
        code: &'a str,
        reason: &'a str,
    },
    Plan {
        evidence: build::MaterializationSetPlanInspection<'a>,
    },
    Write {
        evidence: build::MaterializationSetWriteInspection<'a>,
    },
}

pub(super) fn descriptor() -> Tool {
    let mut schema = schemars::schema_for!(Input);
    schema.ensure_object()["properties"]["ids"]["minProperties"] = 1.into();
    schema.ensure_object()["properties"]["ids"]["maxProperties"] = 64.into();
    schema.ensure_object()["properties"]["checkouts"]["minProperties"] = 1.into();
    schema.ensure_object()["properties"]["checkouts"]["maxProperties"] = 64.into();
    Tool::new(
        "materialize_sites",
        "Use materialize_sites to write a publication's complete configured output to its output directories, once, under a writer session you already hold. One call acquires and renders the exact publication once and materializes its complete configured output set under that ordinary writer session. No writer reacquisition, per-site interface loop, automatic retry or release. Requires publication.local=true and allow_read=true, independent allow_write=true and allow_record=true, and exactly one distinct ids coordinate per selected site. Complete-set preflight protects source roots, journal custody, .git and unrelated user bytes. Durable intent precedes filesystem effects; only exact read-back completion advances generated ownership. Historical replay has output=null and proves no current destination contents. Failure retains requested/completed/failed and native effect evidence. Response custody is reserved before source/provider/history/output effects; an oversized result is a delivery error with original handle/header/request/ids, never success or rollback. Preserve that request and inspect history before further work. The original session remains held for explicit inspect_store_session/close_store_session; carrier loss or process exit is not successful close, settlement, publication permission or proof that an effect did not occur. Releasing or publishing the result stays with the publisher.",
        schema.ensure_object().clone(),
    )
    .with_raw_output_schema(crate::tools::output_schema::<Report<'static>>())
    .with_annotations(ToolAnnotations::new().read_only(false).destructive(true).idempotent(false).open_world(true))
}

pub(super) fn decode(arguments: JsonObject) -> Result<Input, ErrorData> {
    let value = Value::Object(arguments);
    crate::publication::validate_shape(value.get("publication").ok_or_else(invalid)?)?;
    if let Some(checkouts) = value.get("checkouts") {
        let bytes = serde_json::to_vec(checkouts).map_err(|_| invalid())?;
        store::parse_output_checkouts(&bytes).map_err(|_| invalid())?;
    }
    if !value.get("ids").is_some_and(Value::is_object)
        || value.get("limits").is_some_and(|item| !item.is_object())
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
        || input.ids.is_empty()
        || input.ids.len() > 64
        || input
            .ids
            .values()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != input.ids.len()
    {
        return Err(invalid());
    }
    input.digest = Some(digest);
    Ok(input)
}

fn invalid() -> ErrorData {
    ErrorData::invalid_params(
        "Invalid configured-output request; use the exact ordinary writer handle, named native objects and distinct complete per-site ids; no operation started",
        None,
    )
}

impl Input {
    pub(super) fn execute(
        self,
        kernel: &Kernel,
        maximum: usize,
    ) -> Result<CallToolResult, ErrorData> {
        if !self.allow_write || !self.allow_record {
            return crate::tools::failure(
                "materialization_permission",
                "Independent configured output and durable-history permissions required; no source, provider, history or output operation started",
                maximum,
            );
        }
        let digest = self.digest.expect("decoder binds exact request");
        let ids = self.ids;
        let mut reserved = None;
        let result = kernel.execute(command::Materialize {
            handle: self.session,
            render: self.publication.into_command()?,
            ids: ids.clone(),
            checkouts: self.checkouts,
            limits: self.limits,
            options: build::MaterializationOptions {
                allow_write: self.allow_write,
                history: store::MaterializationWriteOptions { allow_record: self.allow_record },
                evidence_bytes: self.evidence_bytes,
            },
            reserve: |handle: &str, _: &command::Selection, header: Option<store::JournalHeader>| {
                let witness = Report {
                    checkouts: &BTreeMap::new(),
                    session: handle,
                    active: true,
                    acquired_header: Some(store::JournalHeader {
                        schema_version: u32::MAX,
                        journal_id: [255; 32],
                        journal_epoch: u64::MAX,
                        writer_fence: u64::MAX,
                    }),
                    request_sha256: digest,
                    outcome: Outcome::Delivery { code: "materialization_response_unavailable", ids: &ids, complete_evidence: false, may_have_effects: true },
                };
                crate::tools::encode_complete(&witness, true, maximum).map_err(|_| {
                    ErrorData::invalid_params("Response capacity cannot retain configured-output custody; no source/provider/history/output work started", None)
                })?;
                reserved = header;
                Ok::<(), ErrorData>(())
            },
        });
        let report = match result {
            Ok(report) => report,
            Err(error) => return crate::store_session::command_failure(error, maximum),
        };
        let outcome = match &report.outcome {
            Err(command::MaterializationFailure::Checkout(error)) => Outcome::Failed {
                failure: Failure::Checkout { evidence: error },
            },
            Ok(sites) => Outcome::Materialized {
                sites: sites
                    .iter()
                    .map(|(site, result)| (site, result.inspect()))
                    .collect(),
            },
            Err(command::MaterializationFailure::Render(error)) => Outcome::Failed {
                failure: Failure::Render {
                    code: error.code(),
                    reason: error.message(),
                },
            },
            Err(command::MaterializationFailure::Plan(error)) => Outcome::Failed {
                failure: Failure::Plan {
                    evidence: error.inspect(),
                },
            },
            Err(command::MaterializationFailure::Write(error)) => Outcome::Failed {
                failure: Failure::Write {
                    evidence: error.inspect(),
                },
            },
        };
        let full = Report {
            checkouts: &report.checkouts,
            session: &report.session,
            active: report.active,
            acquired_header: report.acquired_header,
            request_sha256: digest,
            outcome,
        };
        if let Ok(encoded) = crate::tools::encode_complete(&full, report.outcome.is_err(), maximum)
        {
            return Ok(encoded);
        }
        crate::tools::encode_complete(
            &Report {
                checkouts: &BTreeMap::new(),
                session: &report.session,
                active: report.active,
                acquired_header: reserved,
                request_sha256: digest,
                outcome: Outcome::Delivery {
                    code: "materialization_response_unavailable",
                    ids: &ids,
                    complete_evidence: false,
                    may_have_effects: true,
                },
            },
            true,
            maximum,
        )
        .map_err(|_| {
            ErrorData::internal_error(
                "Reserved materialization custody could not be encoded; preserve the original handle, request and operation ids, and inspect durable history before further effects",
                None,
            )
        })
    }
}
