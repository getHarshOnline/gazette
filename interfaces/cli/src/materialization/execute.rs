//! One-shot private CLI transport over one held native writer and one complete render.

use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, Read, Write},
    path::PathBuf,
    process::ExitCode,
};

use gazette_sdk::{build, render::LimitedWriter, source, store, toml, workspace::Key};
use serde::Serialize;

use crate::command::{Kernel, publication, store_session as command};

#[derive(clap::Args)]
pub(crate) struct Args {
    /// Existing private initialized journal directory; never created or migrated.
    root: PathBuf,
    /// Independently retained complete header; no observation-to-authority rebinding.
    #[arg(long, value_parser = crate::journal::parse_header)]
    expected_header: store::JournalHeader,
    /// Required acknowledgement of private configured local output, not a release.
    #[arg(long, required = true)]
    local: bool,
    /// Permit configured native source and journal reads independently of writes.
    #[arg(long)]
    allow_read: bool,
    /// Permit guarded writes to every configured output; not public admission.
    #[arg(long)]
    allow_write_output: bool,
    /// Permit durable intent/completion recording independently of output effects.
    #[arg(long)]
    allow_record_history: bool,
    /// Permit one exact local journal generation increment; lost delivery is not rollback.
    #[arg(long)]
    advance_writer_generation: bool,
    /// Hold cooperating local journal custody; not a cross-host destination fence.
    #[arg(long)]
    reserve_local_coordination: bool,
    /// Exact per-site operation coordinate; repeat once for every selected site.
    #[arg(long = "operation", required = true, value_parser = parse_operation)]
    operations: Vec<Operation>,
    /// Optional bounded JSON file with explicit repository setup for every selected site.
    #[arg(long)]
    checkouts: Option<PathBuf>,
    /// Complete native recipe JSON stdin byte ceiling, before native acquisition.
    #[arg(long, default_value_t = toml::Limits::default().source_bytes)]
    max_source_bytes: usize,
    /// Maximum selected site count and operation-coordinate count.
    #[arg(long, default_value_t = build::PublicationSetAcquisitionLimits::default().sites)]
    max_sites: usize,
    /// Aggregate retained render and observed-plus-proposed output entry ceiling.
    #[arg(long, default_value_t = build::LocalRenderLimits::default().entries)]
    max_total_output_entries: usize,
    /// Aggregate retained render and observed-plus-proposed output byte ceiling.
    #[arg(long, default_value_t = build::LocalRenderLimits::default().bytes)]
    max_total_output_bytes: u64,
    /// Maximum bytes per declared resource input; aggregate and decoder bounds stay separate.
    #[arg(long, default_value_t = build::LocalRenderLimits::default().publication.output.resources.inputs.file_bytes)]
    max_resource_file_bytes: u64,
    /// Complete-set durable private acquisition/composition/render evidence ceiling.
    #[arg(long, default_value_t = build::MaterializationOptions::default().evidence_bytes)]
    max_evidence_bytes: usize,
    /// Maximum encoded bytes per JSONL response frame; compact custody is reserved first.
    #[arg(long, default_value_t = 1 << 20)]
    max_response_bytes: usize,
    /// Per-site generated output bounds, separate from complete-set bounds.
    #[command(flatten)]
    output: crate::SourceLimits,
    #[command(flatten)]
    read: crate::store::ReadArgs,
}

#[derive(Clone)]
struct Operation {
    site: Key,
    id: source::Sha256Digest,
}

fn parse_operation(value: &str) -> Result<Operation, String> {
    let (site, id) = value.rsplit_once('=').ok_or("use SITE=lowercase-sha256")?;
    let site: Key = site.parse().map_err(|_| "invalid native site identity")?;
    if !site.as_str().starts_with("site:") {
        return Err("operation requires a site: identity".into());
    }
    let id = id
        .parse()
        .map_err(|_| "use an exact lowercase SHA-256 operation ID")?;
    Ok(Operation { site, id })
}

fn operation_ids(
    operations: Vec<Operation>,
) -> Result<BTreeMap<Key, source::Sha256Digest>, &'static str> {
    let mut ids = BTreeMap::new();
    let mut distinct = BTreeSet::new();
    for operation in operations {
        if ids.insert(operation.site, operation.id).is_some() || !distinct.insert(operation.id) {
            return Err("materialization_operation_duplicate");
        }
    }
    Ok(ids)
}

#[derive(Serialize)]
struct RequestIdentity<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    checkouts_sha256: Option<source::Sha256Digest>,
    root: &'a std::path::Path,
    expected_header: store::JournalHeader,
    recipe_sha256: source::Sha256Digest,
    operations: &'a BTreeMap<Key, source::Sha256Digest>,
    local: bool,
    allow_read: bool,
    allow_write_output: bool,
    allow_record_history: bool,
    writer: store::WriterOptions,
    output: source::Limits,
    max_source_bytes: usize,
    max_sites: usize,
    max_total_output_entries: usize,
    max_total_output_bytes: u64,
    max_resource_file_bytes: u64,
    max_evidence_bytes: usize,
    max_response_bytes: usize,
}

#[derive(Serialize)]
pub(super) struct Custody<'a> {
    pub(super) root: &'a std::path::Path,
    pub(super) expected_header: store::JournalHeader,
    pub(super) acquired_header: Option<store::JournalHeader>,
    pub(super) request_sha256: source::Sha256Digest,
    pub(super) operations: &'a BTreeMap<Key, source::Sha256Digest>,
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(super) enum Close {
    NotAcquired,
    Held,
    Closed,
    Failed { code: &'static str },
    Unavailable { code: &'static str },
}

#[derive(Serialize)]
pub(super) struct Frame<'a, T> {
    pub(super) event: &'static str,
    pub(super) custody: &'a Custody<'a>,
    pub(super) native_status: &'static str,
    pub(super) details_complete: bool,
    pub(super) detail: T,
    pub(super) close: &'a Close,
}

#[derive(Serialize)]
pub(super) struct NativeFailure<'a> {
    phase: &'static str,
    code: &'static str,
    generation: Option<store::GenerationEffect>,
    close_error: Option<&'static str>,
    diagnostic: &'a str,
}

pub(super) fn native_failure(failure: &command::NativeFailure) -> NativeFailure<'static> {
    NativeFailure {
        phase: match failure.phase {
            command::NativeFailurePhase::Acquisition => "acquisition",
            command::NativeFailurePhase::Readiness => "readiness",
            command::NativeFailurePhase::Inspection => "inspection",
            command::NativeFailurePhase::Close => "close",
        },
        code: failure.code,
        generation: failure.generation,
        close_error: failure.close_error,
        diagnostic: "Preserve journal and output evidence; failure or delivery loss is not rollback or retry authority",
    }
}

pub(super) fn encode(value: &impl Serialize, maximum: usize) -> Result<Vec<u8>, serde_json::Error> {
    let mut writer = LimitedWriter::new(Vec::new(), maximum.saturating_sub(1));
    serde_json::to_writer(&mut writer, value)?;
    let mut bytes = writer.into_inner();
    bytes.push(b'\n');
    Ok(bytes)
}

pub(super) fn emit(bytes: &[u8]) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    stdout.write_all(bytes)?;
    stdout.flush()
}

// Reserve the largest compact carrier before any generation/output effect.
// The original root, header, IDs and digest survive a bounded detail overflow.
fn reserve(custody: &Custody<'_>, maximum: usize) -> Result<(), serde_json::Error> {
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
    encode(
        &Frame {
            event: "delivery_incomplete",
            custody: &worst,
            native_status: "materialization_not_started",
            details_complete: false,
            detail: (),
            close: &Close::Unavailable {
                code: "materialization_session_handle_collision",
            },
        },
        maximum,
    )
    .map(|_| ())
}

pub(super) fn close(kernel: &Kernel, handle: String) -> Close {
    match kernel.execute(command::Operate {
        handle,
        close: true,
        // Close must remain available even after response encoding/delivery failed.
        reserve: |_: &str, _: &command::Selection, _: Option<store::JournalHeader>, _: bool| {
            Ok::<_, std::convert::Infallible>(())
        },
    }) {
        Ok(report) => match report.outcome {
            command::Outcome::Closed => Close::Closed,
            command::Outcome::Failed { failure, .. } => Close::Failed { code: failure.code },
            _ => Close::Unavailable {
                code: "materialization_session_protocol",
            },
        },
        Err(error) => Close::Unavailable {
            code: command_code(&error),
        },
    }
}

pub(super) fn command_code<E>(failure: &command::Failure<E>) -> &'static str {
    match failure {
        command::Failure::Busy => "materialization_session_busy",
        command::Failure::Unavailable => "materialization_session_unavailable",
        command::Failure::Capacity => "materialization_session_capacity",
        command::Failure::Unknown => "materialization_session_unknown",
        command::Failure::HandleCollision => "materialization_session_handle_collision",
        command::Failure::Selection => "materialization_session_selection",
        command::Failure::NotWriter => "materialization_session_not_writer",
        command::Failure::Reservation(_) => "materialization_response_reservation",
    }
}

pub(super) fn deliver<T: Serialize>(
    custody: &Custody<'_>,
    status: &'static str,
    detail: T,
    closed: &Close,
    maximum: usize,
) -> bool {
    let full = encode(
        &Frame {
            event: "result",
            custody,
            native_status: status,
            details_complete: true,
            detail,
            close: closed,
        },
        maximum,
    );
    let (bytes, complete) = match full {
        Ok(bytes) => (bytes, true),
        Err(_) => match encode(
            &Frame {
                event: "delivery_incomplete",
                custody,
                native_status: status,
                details_complete: false,
                detail: (),
                close: closed,
            },
            maximum,
        ) {
            Ok(bytes) => (bytes, false),
            Err(_) => return false,
        },
    };
    if emit(&bytes).is_err() {
        // Stdout loss is not rollback. Retain recovery coordinates on the
        // independent diagnostic carrier, without claiming the full result arrived.
        if let Ok(fallback) = encode(
            &Frame {
                event: "delivery_incomplete",
                custody,
                native_status: status,
                details_complete: false,
                detail: (),
                close: closed,
            },
            maximum,
        ) {
            let _ = io::stderr().lock().write_all(&fallback);
        }
        return false;
    }
    complete
}

pub(super) fn failure(code: &str) -> ExitCode {
    let _ = writeln!(
        io::stderr().lock(),
        "gazette: {code}; no writer acquired or configured output operation started"
    );
    ExitCode::FAILURE
}

fn deliver_with_checkouts<T: Serialize>(
    custody: &Custody<'_>,
    status: &'static str,
    detail: T,
    checkouts: &BTreeMap<Key, gazette_sdk::release::OutputCheckoutReport>,
    closed: &Close,
    maximum: usize,
) -> bool {
    #[derive(Serialize)]
    struct Detail<'a, T> {
        checkouts: &'a BTreeMap<Key, gazette_sdk::release::OutputCheckoutReport>,
        output: T,
    }
    if checkouts.is_empty() {
        deliver(custody, status, detail, closed, maximum)
    } else {
        deliver(
            custody,
            status,
            Detail {
                checkouts,
                output: detail,
            },
            closed,
            maximum,
        )
    }
}

pub(crate) fn run(args: Args) -> ExitCode {
    let render_year = match crate::calendar::resolve(None) {
        Ok(year) => year,
        Err(_) => return failure("render_year"),
    };
    if !args.local || !args.allow_read || !args.allow_write_output || !args.allow_record_history {
        return failure("materialization_permission");
    }
    if !args.root.is_absolute() || args.max_sites == 0 || args.operations.len() > args.max_sites {
        return failure("materialization_input");
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
    let ids = match operation_ids(args.operations) {
        Ok(ids) => ids,
        Err(code) => return failure(code),
    };
    let mut bytes = Vec::new();
    if io::stdin()
        .lock()
        .take((args.max_source_bytes as u64).saturating_add(1))
        .read_to_end(&mut bytes)
        .is_err()
    {
        return failure("publication_set_input");
    }
    if bytes.len() > args.max_source_bytes {
        return failure("publication_set_input_limit");
    }
    let checkout_bytes = match args
        .checkouts
        .as_ref()
        .map(|path| {
            let mut bytes = Vec::new();
            std::fs::File::open(path)?
                .take((4 << 20) + 1)
                .read_to_end(&mut bytes)?;
            Ok::<_, std::io::Error>(bytes)
        })
        .transpose()
    {
        Ok(bytes) => bytes,
        Err(_) => return failure("output_checkout_input"),
    };
    let checkouts = match checkout_bytes
        .as_deref()
        .map(store::parse_output_checkouts)
        .transpose()
    {
        Ok(checkouts) => checkouts,
        Err(code) => return failure(code),
    };
    let output = args.output.into_limits();
    let identity = RequestIdentity {
        checkouts_sha256: checkout_bytes.as_deref().map(source::Sha256Digest::of),
        root: &args.root,
        expected_header: args.expected_header,
        recipe_sha256: source::Sha256Digest::of(&bytes),
        operations: &ids,
        local: args.local,
        allow_read: args.allow_read,
        allow_write_output: args.allow_write_output,
        allow_record_history: args.allow_record_history,
        writer: writer_options,
        output,
        max_source_bytes: args.max_source_bytes,
        max_sites: args.max_sites,
        max_total_output_entries: args.max_total_output_entries,
        max_total_output_bytes: args.max_total_output_bytes,
        max_resource_file_bytes: args.max_resource_file_bytes,
        max_evidence_bytes: args.max_evidence_bytes,
        max_response_bytes: args.max_response_bytes,
    };
    let encoded = match encode(&identity, args.max_response_bytes) {
        Ok(bytes) => bytes,
        Err(_) => return failure("materialization_response_reservation"),
    };
    let mut custody = Custody {
        root: &args.root,
        expected_header: args.expected_header,
        acquired_header: None,
        request_sha256: source::Sha256Digest::of(&encoded),
        operations: &ids,
    };
    if reserve(&custody, args.max_response_bytes).is_err() {
        return failure("materialization_response_reservation");
    }
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
    if !matches!(reserved, Ok(ref bytes) if emit(bytes).is_ok()) {
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
        reserve: |_: &str, _: &command::Selection| reserve(&custody, args.max_response_bytes),
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
    if !matches!(acquired, Ok(ref bytes) if emit(bytes).is_ok()) {
        let closed = close(&kernel, opened.session);
        deliver(
            &custody,
            "materialization_not_started",
            "materialization_acquired_delivery",
            &closed,
            args.max_response_bytes,
        );
        return ExitCode::FAILURE;
    }
    let rendered = kernel.execute(command::Materialize {
        handle: opened.session.clone(),
        render: publication::Render {
            math: None,
            allow_math_execution: false,
            default_render_year: Some(render_year),
            recipe_json: bytes,
            local: args.local,
            allow_read: args.allow_read,
            max_input_bytes: args.max_source_bytes,
            syntax: toml::Limits {
                source_bytes: args.max_source_bytes,
                ..Default::default()
            },
            acquisition: build::PublicationSetAcquisitionLimits {
                sites: args.max_sites,
                ..Default::default()
            },
            output,
            limits: build::PublicationSetRenderLimits {
                publication: build::PublicationRenderLimits {
                    output: build::PublicationOutputLimits {
                        resources: gazette_sdk::resource::Limits {
                            inputs: source::Limits {
                                file_bytes: args.max_resource_file_bytes,
                                ..gazette_sdk::resource::Limits::default().inputs
                            },
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    ..Default::default()
                },
                bytes: args.max_total_output_bytes,
                entries: args.max_total_output_entries,
            },
        },
        ids: ids.clone(),
        checkouts,
        limits: source::Limits {
            entries: args.max_total_output_entries,
            total_bytes: args.max_total_output_bytes,
            ..output
        },
        options: build::MaterializationOptions {
            allow_write: args.allow_write_output,
            history: store::MaterializationWriteOptions {
                allow_record: args.allow_record_history,
            },
            evidence_bytes: args.max_evidence_bytes,
        },
        reserve: |_: &str, _: &command::Selection, _: Option<store::JournalHeader>| {
            reserve(&custody, args.max_response_bytes)
        },
    });
    let closed = close(&kernel, opened.session);
    let mut success = matches!(closed, Close::Closed);
    let delivered = match rendered {
        Err(error) => {
            success = false;
            deliver(
                &custody,
                "materialization_refused",
                command_code(&error),
                &closed,
                args.max_response_bytes,
            )
        }
        Ok(report) => match report.outcome {
            Err(command::MaterializationFailure::Checkout(error)) => {
                success = false;
                deliver(
                    &custody,
                    "checkout_failed",
                    &error,
                    &closed,
                    args.max_response_bytes,
                )
            }
            Ok(sites) => {
                let views = sites
                    .iter()
                    .map(|(site, result)| (site, result.inspect()))
                    .collect::<BTreeMap<_, _>>();
                deliver_with_checkouts(
                    &custody,
                    "materialization_completed",
                    &views,
                    &report.checkouts,
                    &closed,
                    args.max_response_bytes,
                )
            }
            Err(command::MaterializationFailure::Render(error)) => {
                success = false;
                deliver(
                    &custody,
                    "render_failed",
                    serde_json::json!({"code":error.code(),"message":error.message()}),
                    &closed,
                    args.max_response_bytes,
                )
            }
            Err(command::MaterializationFailure::Plan(error)) => {
                success = false;
                deliver_with_checkouts(
                    &custody,
                    "plan_failed",
                    error.inspect(),
                    &report.checkouts,
                    &closed,
                    args.max_response_bytes,
                )
            }
            Err(command::MaterializationFailure::Write(error)) => {
                success = false;
                deliver_with_checkouts(
                    &custody,
                    "materialization_failed",
                    error.inspect(),
                    &report.checkouts,
                    &closed,
                    args.max_response_bytes,
                )
            }
        },
    };
    if success && delivered {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

#[cfg(test)]
#[path = "../../tests/internal/materialization.rs"]
mod tests;
