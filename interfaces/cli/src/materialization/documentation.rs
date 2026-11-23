//! One held writer, original native documentation custody, and explicit CLI close.

use std::{
    collections::BTreeMap,
    io::{self, Read, Write},
    path::PathBuf,
    process::ExitCode,
};

use gazette_sdk::{build, source, store, toml, workspace::Key};
use serde::Serialize;
use serde_json::value::RawValue;

use super::execute::{
    Close, Custody, Frame, close, command_code, deliver, emit, encode, failure, native_failure,
};
use crate::command::{Kernel, documentation, store_session as command};

#[derive(clap::Args)]
pub(crate) struct Args {
    /// Existing private initialized journal directory; never created or migrated.
    root: PathBuf,
    /// Independently retained complete journal header.
    #[arg(long, value_parser=crate::journal::parse_header)]
    expected_header: store::JournalHeader,
    /// Exact caller-selected lowercase SHA-256 operation; not inferred from content.
    #[arg(long)]
    operation: source::Sha256Digest,
    /// Configured brand-owned documentation publication.
    #[arg(long)]
    site: Key,
    /// Required local configured-output acknowledgement; not publication approval.
    #[arg(long, required = true)]
    local: bool,
    /// Permit configured native source and journal reads independently of writes.
    #[arg(long)]
    allow_read: bool,
    /// Permit fresh native generator stages, independently of execution/output/history.
    #[arg(long)]
    allow_stage: bool,
    /// Permit configured native generator execution; not sandboxed.
    #[arg(long, requires = "generator_parent")]
    allow_execute: bool,
    /// Existing private parent for fresh native generator input/output stages.
    #[arg(long)]
    generator_parent: Option<PathBuf>,
    /// Per-command deadline including complete stdout capture.
    #[arg(long, default_value_t = 30_000)]
    generator_timeout_ms: u64,
    /// Maximum native version/generator stdout per invocation.
    #[arg(long, default_value_t=1<<20)]
    max_generator_capture_bytes: usize,
    /// Complete configured native generator-job ceiling.
    #[arg(long, default_value_t = 32)]
    max_generator_jobs: usize,
    /// Permit guarded writes to the configured documentation repository.
    #[arg(long)]
    allow_write_output: bool,
    /// Permit durable intent/completion independently of output writes.
    #[arg(long)]
    allow_record_history: bool,
    /// Permit one exact journal generation advance.
    #[arg(long)]
    advance_writer_generation: bool,
    /// Hold cooperating local journal custody; not a cross-host output fence.
    #[arg(long)]
    reserve_local_coordination: bool,
    /// Read the shared supplied/local `WorkspaceInput` JSON instead of TOML stdin.
    #[arg(long)]
    workspace_json: bool,
    /// Explicit absolute peer roots protected in addition to native inputs and journal.
    #[arg(long = "protected-root")]
    protected: Vec<PathBuf>,
    /// Complete supplied workspace stdin ceiling, before native acquisition.
    #[arg(long, default_value_t=toml::Limits::default().source_bytes)]
    max_source_bytes: usize,
    /// Complete private materialization evidence bound; never expands delivery capacity.
    #[arg(long, default_value_t=build::MaterializationOptions::default().evidence_bytes)]
    max_evidence_bytes: usize,
    /// Maximum encoded JSONL frame bytes; combined native custody reserved first.
    #[arg(long, default_value_t=16<<20)]
    max_response_bytes: usize,
    /// Complete assembly and observed/proposed configured output limits.
    #[command(flatten)]
    output: crate::SourceLimits,
    #[command(flatten)]
    read: crate::store::ReadArgs,
}

#[derive(Serialize)]
struct Detail<'a, T> {
    /// Original native JSON; no remapping, truncation or double stringification.
    documentation: Option<&'a RawValue>,
    outcome: T,
}

// CLI framing is private, but the actual history/output/error views are owned
// by the shared native model and serialized by reference under LimitedWriter.
#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum Outcome<'a> {
    Materialized {
        materialization: build::MaterializationInspection<'a>,
    },
    PlanFailed {
        failure: build::MaterializationErrorInspection<'a>,
    },
    WriteFailed {
        failure: build::MaterializationErrorInspection<'a>,
    },
    NotAttempted {
        failure: DocumentationFailure<'a>,
    },
}

#[derive(Serialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
enum DocumentationFailure<'a> {
    Request {
        code: &'static str,
        reason: &'a str,
    },
    Before {
        code: &'static str,
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

#[derive(Serialize)]
struct Incomplete {
    code: &'static str,
    complete_evidence: bool,
    may_have_effects: bool,
}

fn incomplete() -> Incomplete {
    Incomplete {
        code: "documentation_materialization_response_unavailable",
        complete_evidence: false,
        may_have_effects: true,
    }
}

fn allowance(custody: &Custody<'_>, maximum: usize) -> Result<usize, serde_json::Error> {
    let worst = Custody {
        root: custody.root,
        expected_header: custody.expected_header,
        acquired_header: Some(store::JournalHeader {
            schema_version: u32::MAX,
            journal_id: [255; 32],
            journal_epoch: u64::MAX,
            writer_fence: u64::MAX,
        }),
        request_sha256: custody.request_sha256,
        operations: custody.operations,
    };
    let bytes = encode(
        &Frame {
            event: "delivery_incomplete",
            custody: &worst,
            native_status: "documentation_materialization",
            details_complete: false,
            detail: Detail {
                documentation: None,
                outcome: incomplete(),
            },
            close: &Close::Unavailable {
                code: "materialization_session_handle_collision",
            },
        },
        maximum,
    )?;
    // Charge the null and framing byte as well. Substituting an admitted native
    // RawValue is bounded by this exact outer carrier plus the SDK allowance.
    Ok(maximum.saturating_sub(bytes.len()))
}

fn deliver_documentation<T: Serialize>(
    custody: &Custody<'_>,
    documentation: Option<&RawValue>,
    outcome: T,
    closed: &Close,
    maximum: usize,
) -> bool {
    let full = encode(
        &Frame {
            event: "result",
            custody,
            native_status: "documentation_materialization",
            details_complete: true,
            detail: Detail {
                documentation,
                outcome,
            },
            close: closed,
        },
        maximum,
    );
    let fallback = || {
        encode(
            &Frame {
                event: "delivery_incomplete",
                custody,
                native_status: "documentation_materialization",
                details_complete: false,
                detail: Detail {
                    documentation,
                    outcome: incomplete(),
                },
                close: closed,
            },
            maximum,
        )
    };
    let (bytes, complete) = match full {
        Ok(bytes) => (bytes, true),
        Err(_) => match fallback() {
            Ok(bytes) => (bytes, false),
            Err(_) => {
                // A violated reserved native contract cannot silently discard custody.
                let _ = writeln!(
                    io::stderr().lock(),
                    "gazette: documentation_materialization_delivery_contract; exact custody follows without a bounded-delivery guarantee; no retry"
                );
                if let Ok(bytes) = serde_json::to_vec(&Frame {
                    event: "delivery_incomplete",
                    custody,
                    native_status: "documentation_materialization",
                    details_complete: false,
                    detail: Detail {
                        documentation,
                        outcome: incomplete(),
                    },
                    close: closed,
                }) {
                    let mut stderr = io::stderr().lock();
                    let _ = stderr.write_all(&bytes);
                    let _ = stderr.write_all(b"\n");
                }
                return false;
            }
        },
    };
    if emit(&bytes).is_err() {
        // Lost stdout is not rollback: the diagnostic carrier retains the same
        // original native documentation response, coordinates and close result.
        if let Ok(bytes) = fallback() {
            let _ = io::stderr().lock().write_all(&bytes);
        }
        return false;
    }
    complete
}

pub(crate) fn run(args: Args) -> ExitCode {
    if !args.local
        || !args.allow_read
        || !args.allow_write_output
        || !args.allow_record_history
        || args.generator_parent.is_some() && (!args.allow_stage || !args.allow_execute)
    {
        return failure("documentation_materialization_permission");
    }
    if !args.root.is_absolute()
        || !args.site.as_str().starts_with("site:")
        || args.protected.iter().any(|root| !root.is_absolute())
        || args
            .generator_parent
            .as_ref()
            .is_some_and(|parent| !parent.is_absolute())
    {
        return failure("documentation_materialization_input");
    }
    let writer_options = store::WriterOptions {
        allow_generation_advance: args.advance_writer_generation,
        session: store::SessionOptions {
            allow_local_reservation: args.reserve_local_coordination,
            read: args.read.options(),
        },
    };
    if let Err(error) = writer_options.validate() {
        return failure(error.code());
    }
    let mut bytes = Vec::new();
    if io::stdin()
        .lock()
        .take((args.max_source_bytes as u64).saturating_add(1))
        .read_to_end(&mut bytes)
        .is_err()
        || bytes.len() > args.max_source_bytes
    {
        return failure("documentation_materialization_input_limit");
    }
    let Ok(text) = std::str::from_utf8(&bytes) else {
        return failure("workspace_utf8");
    };
    let input = if args.workspace_json {
        match serde_json::from_str::<crate::json_input::Object<build::WorkspaceInput>>(text) {
            Ok(input) => input.0,
            Err(_) => return failure("workspace_selection"),
        }
    } else {
        build::WorkspaceInput::Supplied {
            source: text.to_owned(),
        }
    };
    let output = args.output.into_limits();
    let ids = BTreeMap::from([(args.site.clone(), args.operation)]);
    let identity = serde_json::json!({"root":args.root,"expected_header":args.expected_header,"workspace_sha256":source::Sha256Digest::of(&bytes),
        "operations":ids,"workspace_json":args.workspace_json,"local":args.local,"allow_read":args.allow_read,"allow_stage":args.allow_stage,
        "allow_execute":args.allow_execute,"generator_parent":args.generator_parent,"generator_timeout_ms":args.generator_timeout_ms,
        "max_generator_capture_bytes":args.max_generator_capture_bytes,"max_generator_jobs":args.max_generator_jobs,
        "allow_write_output":args.allow_write_output,"allow_record_history":args.allow_record_history,"writer":writer_options,
        "protected":args.protected,"output":output,"max_source_bytes":args.max_source_bytes,
        "max_evidence_bytes":args.max_evidence_bytes,"max_response_bytes":args.max_response_bytes});
    let identity = match encode(&identity, args.max_response_bytes) {
        Ok(bytes) => bytes,
        Err(_) => return failure("materialization_response_reservation"),
    };
    let mut custody = Custody {
        root: &args.root,
        expected_header: args.expected_header,
        acquired_header: None,
        request_sha256: source::Sha256Digest::of(&identity),
        operations: &ids,
    };
    let native_bytes = match allowance(&custody, args.max_response_bytes) {
        Ok(bytes) if bytes > 0 => bytes,
        _ => return failure("materialization_response_reservation"),
    };
    let reserved = encode(
        &Frame {
            event: "reserved",
            custody: &custody,
            native_status: "not_started",
            details_complete: true,
            detail: (),
            close: &Close::NotAcquired,
        },
        args.max_response_bytes,
    );
    if !matches!(reserved,Ok(ref bytes) if emit(bytes).is_ok()) {
        return failure("materialization_reserved_delivery");
    }
    let kernel = Kernel::new();
    let opened = kernel.execute(command::Open {
        handle: custody.request_sha256.to_string(),
        selection: command::Selection::Writer {
            root: args.root.clone(),
            expected_header: args.expected_header,
            allow_native_reads: args.allow_read,
            options: writer_options,
        },
        reserve: |_: &str, _: &command::Selection| {
            allowance(&custody, args.max_response_bytes).map(|_| ())
        },
    });
    let opened = match opened {
        Ok(report) => report,
        Err(error) => {
            deliver(
                &custody,
                "acquisition_refused",
                command_code(&error),
                &Close::NotAcquired,
                args.max_response_bytes,
            );
            return ExitCode::FAILURE;
        }
    };
    custody.acquired_header = opened.acquired_header;
    match &opened.outcome {
        command::Outcome::Acquired { .. } => {}
        command::Outcome::Failed { failure, .. } => {
            deliver(
                &custody,
                "acquisition_failed",
                native_failure(failure),
                &Close::NotAcquired,
                args.max_response_bytes,
            );
            return ExitCode::FAILURE;
        }
        _ => {
            let closed = close(&kernel, opened.session);
            deliver(
                &custody,
                "acquisition_failed",
                "materialization_session_protocol",
                &closed,
                args.max_response_bytes,
            );
            return ExitCode::FAILURE;
        }
    }
    let acquired = encode(
        &Frame {
            event: "acquired",
            custody: &custody,
            native_status: "writer_acquired",
            details_complete: true,
            detail: (),
            close: &Close::Held,
        },
        args.max_response_bytes,
    );
    if !matches!(acquired,Ok(ref bytes) if emit(bytes).is_ok()) {
        let closed = close(&kernel, opened.session);
        deliver(
            &custody,
            "documentation_not_started",
            "materialization_acquired_delivery",
            &closed,
            args.max_response_bytes,
        );
        return ExitCode::FAILURE;
    }
    let result = kernel.execute(command::MaterializeDocumentation {
        handle: opened.session.clone(),
        documentation: documentation::Assemble {
            preview_link_bases: None,
            input,
            site: args.site,
            local: args.local,
            allow_read: args.allow_read,
            allow_stage: args.allow_stage,
            syntax: toml::Limits {
                source_bytes: args.max_source_bytes,
                ..Default::default()
            },
            acquisition: source::PreparationLimits {
                sources: 256,
                entries: source::Limits::default().entries,
                bytes: source::Limits::default().total_bytes,
                per_source: source::RevisionLimits::default(),
            },
            output,
            generation: args
                .generator_parent
                .map(|parent| documentation::Generation {
                    parent,
                    input: source::Limits::default(),
                    output,
                    allow_execute: args.allow_execute,
                    timeout: std::time::Duration::from_millis(args.generator_timeout_ms),
                    max_capture_bytes: args.max_generator_capture_bytes,
                    max_jobs: args.max_generator_jobs,
                    directory_bytes: 16 << 10,
                    entry_bytes: 4 << 10,
                }),
            retention: documentation::Retention::None,
            response_bytes: native_bytes,
        },
        id: args.operation,
        protected: args.protected,
        limits: output,
        options: build::MaterializationOptions {
            allow_write: args.allow_write_output,
            history: store::MaterializationWriteOptions {
                allow_record: args.allow_record_history,
            },
            evidence_bytes: args.max_evidence_bytes,
        },
        reserve: |_: &str, _: &command::Selection, _: Option<store::JournalHeader>| {
            allowance(&custody, args.max_response_bytes).map(|_| ())
        },
    });
    let closed = close(&kernel, opened.session);
    let report = match result {
        Ok(report) => report,
        Err(error) => {
            deliver(
                &custody,
                "documentation_materialization_refused",
                command_code(&error),
                &closed,
                args.max_response_bytes,
            );
            return ExitCode::FAILURE;
        }
    };
    use command::{
        DocumentationMaterializationFailure as F, DocumentationMaterializationOutcome as O,
    };
    if let O::Contract { evidence } = &report.outcome {
        let evidence = match evidence {
            Ok(evidence) => evidence.clone(),
            Err(error) => {
                serde_json::json!({"serialization_error":error.cause.to_string(),"custody":error.custody})
            }
        };
        let _ = writeln!(
            io::stderr().lock(),
            "gazette: native_documentation_contract; SDK custody follows losslessly without bounded delivery; configured output not dispatched, earlier generators may have effects; no retry"
        );
        let frame = Frame {
            event: "native_contract",
            custody: &custody,
            native_status: "documentation_materialization",
            details_complete: false,
            detail: evidence,
            close: &closed,
        };
        if let Ok(bytes) = serde_json::to_vec(&frame) {
            let mut stderr = io::stderr().lock();
            let _ = stderr.write_all(&bytes);
            let _ = stderr.write_all(b"\n");
        }
        return ExitCode::FAILURE;
    }
    let native = match report
        .documentation
        .as_ref()
        .map(|response| serde_json::from_slice::<&RawValue>(&response.bytes))
        .transpose()
    {
        Ok(native) => native,
        Err(_) => {
            let _ = writeln!(
                io::stderr().lock(),
                "gazette: native_documentation_json; original bytes and custody follow without bounded delivery; no retry"
            );
            let frame = Frame {
                event: "native_contract",
                custody: &custody,
                native_status: "documentation_materialization",
                details_complete: false,
                detail: report
                    .documentation
                    .as_ref()
                    .map(|response| &response.bytes),
                close: &closed,
            };
            if let Ok(bytes) = serde_json::to_vec(&frame) {
                let mut stderr = io::stderr().lock();
                let _ = stderr.write_all(&bytes);
                let _ = stderr.write_all(b"\n");
            }
            return ExitCode::FAILURE;
        }
    };
    let success = matches!(&report.outcome, O::Materialized(_))
        && native.is_some()
        && matches!(closed, Close::Closed);
    if native.is_none() && matches!(&report.outcome, O::Materialized(_)) {
        deliver_documentation(
            &custody,
            None,
            "native_materialization_missing_documentation_custody",
            &closed,
            args.max_response_bytes,
        );
        return ExitCode::FAILURE;
    }
    let outcome = match &report.outcome {
        O::Materialized(result) => Outcome::Materialized {
            materialization: result.inspect(),
        },
        O::PlanError(error) => Outcome::PlanFailed {
            failure: error.inspect(),
        },
        O::WriteError(error) => Outcome::WriteFailed {
            failure: error.inspect(),
        },
        O::NotAttempted(error) => {
            let failure = match error {
                F::Request(error) => DocumentationFailure::Request {
                    code: error.code(),
                    reason: error.message(),
                },
                F::Before(error) => DocumentationFailure::Before {
                    code: error.code(),
                    reason: error.to_string(),
                },
                F::Delivery => DocumentationFailure::DocumentationDelivery {
                    code: "documentation_delivery_failed",
                },
                F::MissingAssembly => DocumentationFailure::MissingAssembly {
                    code: "documentation_assembly_missing",
                },
                F::Permission(error) => DocumentationFailure::Permission {
                    failure: error.inspect(),
                },
            };
            Outcome::NotAttempted { failure }
        }
        O::Contract { .. } => unreachable!("lossless native exceptional custody handled above"),
    };
    let delivered =
        deliver_documentation(&custody, native, outcome, &closed, args.max_response_bytes);
    if success && delivered {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
