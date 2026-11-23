//! JSONL-controlled loopback preview; the render request and compilation stay shared.

use std::{
    collections::BTreeMap,
    io::{self, Read, Write},
    net::SocketAddr,
    process::ExitCode,
    sync::mpsc::{self, TryRecvError},
    thread,
    time::Duration,
};

use gazette_sdk::{
    preview::{self, runner},
    production_preview,
    workspace::Key,
};
use serde::Serialize;

use crate::{
    command::{Kernel, preview as command},
    render_sites,
};

#[derive(clap::Args)]
#[group(id = "Preview")]
pub(super) struct Args {
    #[command(flatten)]
    render: render_sites::Args,
    /// Permit retained private output to become reachable on loopback listeners.
    #[arg(long)]
    allow_listeners: bool,
    /// Render session-local cross-site URLs from the exact reserved listeners.
    #[arg(long)]
    local_urls: bool,
    /// Map public documentation bases to actual loopback previews without changing the request file.
    #[arg(long, requires = "local_urls")]
    preview_link_bases_json: Option<String>,
    /// Print the exact pre-execution effect plan and exit before native work.
    #[arg(long)]
    plan: bool,
    /// Rebuild from dirty/content-addressed inputs under the fixed initial workspace and Theme bindings.
    #[arg(long)]
    watch: bool,
    /// Watch inspection interval (100..=3600000 ms); default 500 ms. Does not watch workspace configuration.
    #[arg(long, requires = "watch")]
    watch_interval_ms: Option<u64>,
    /// Exact site=loopback-address binding; repeat for every selected site. Port zero is allowed.
    #[arg(long, required = true)]
    listen: Vec<String>,
    #[arg(long, default_value_t = preview::Options::default().max_sites)]
    max_sites: usize,
    #[arg(long, default_value_t = preview::Options::default().max_header_bytes)]
    max_header_bytes: usize,
    #[arg(long, default_value_t = preview::Options::default().max_headers)]
    max_headers: usize,
    /// Complete request-header deadline, in milliseconds; incoming bytes do not renew it.
    #[arg(long, default_value_t = 2000)]
    request_timeout_ms: u64,
    /// Complete response-write deadline, in milliseconds.
    #[arg(long, default_value_t = 2000)]
    response_timeout_ms: u64,
}

#[derive(clap::Args)]
#[group(id = "ProductionPreview")]
pub(super) struct ProductionArgs {
    #[command(flatten)]
    render: render_sites::Args,
    /// Explicitly keep the private registry outside this first same-model slice.
    #[arg(long, value_name = "unconfigured", required = true)]
    registry: production_preview::RegistryMode,
    /// Render fresh retained inputs exactly once; no staged or prepared artifact is accepted.
    #[arg(long, value_name = "fresh-render", required = true)]
    artifact: production_preview::ArtifactMode,
    /// Print the exact no-push effect plan and exit before native work.
    #[arg(long)]
    plan: bool,
    /// Permit retained private output to become reachable on loopback listeners.
    #[arg(long)]
    allow_listeners: bool,
    /// Exact site=loopback-address binding; repeat for every selected site. Port zero is allowed.
    #[arg(long, required = true)]
    listen: Vec<String>,
    #[arg(long, default_value_t = preview::Options::default().max_sites)]
    max_sites: usize,
    #[arg(long, default_value_t = preview::Options::default().max_header_bytes)]
    max_header_bytes: usize,
    #[arg(long, default_value_t = preview::Options::default().max_headers)]
    max_headers: usize,
    /// Complete request-header deadline, in milliseconds.
    #[arg(long, default_value_t = 2000)]
    request_timeout_ms: u64,
    /// Complete response-write deadline, in milliseconds.
    #[arg(long, default_value_t = 2000)]
    response_timeout_ms: u64,
}

pub(super) fn run(args: Args) -> ExitCode {
    // Permissions and transport-only bounds are checked before even reading stdin.
    if !args.render.local {
        return failure(
            "publication_mode",
            "--local is required; public admission is not implemented",
        );
    }
    let preview_link_bases = match args.preview_link_bases_json.as_deref() {
        Some(encoded) if encoded.len() <= 65536 => {
            if gazette_sdk::json::parse(encoded.as_bytes()).is_err() {
                return failure(
                    "preview_link_bases",
                    "use one JSON object without duplicate keys",
                );
            }
            match serde_json::from_str::<
                crate::json_input::Object<gazette_sdk::publication::routing::PreviewLinkBases>,
            >(encoded)
            {
                Ok(value) => Some(value.0),
                Err(_) => {
                    return failure(
                        "preview_link_bases",
                        "map public bases to matching literal loopback bases",
                    );
                }
            }
        }
        Some(_) => return failure("preview_link_bases", "preview link map exceeds 64 KiB"),
        None => None,
    };
    let options = preview::Options {
        allow_listeners: args.allow_listeners,
        max_sites: args.max_sites,
        max_header_bytes: args.max_header_bytes,
        max_headers: args.max_headers,
        request_timeout: Duration::from_millis(args.request_timeout_ms),
        response_timeout: Duration::from_millis(args.response_timeout_ms),
    };
    if let Err(error) = options.validate() {
        return failure(error.code(), &error.to_string());
    }
    let mode = if args.watch {
        runner::Mode::Development {
            interval: Duration::from_millis(args.watch_interval_ms.unwrap_or(500)),
        }
    } else {
        runner::Mode::Pinned
    };
    let mut runner_options = runner::Options {
        mode,
        local_urls: args.local_urls,
        listeners: options,
        ..Default::default()
    };
    if let Err(error) = runner_options.validate() {
        return failure(error.code(), &error.to_string());
    }
    let addresses = match bindings(&args.listen, options.max_sites) {
        Ok(addresses) => addresses,
        Err(reason) => return failure("preview_refused", reason),
    };
    let request = match args.render.request(true, options.max_sites) {
        Ok(request) => {
            let year = match crate::calendar::resolve(args.render.render_year) {
                Ok(year) => year,
                Err(reason) => return failure("render_year", reason),
            };
            let request = request.with_default_render_year(year);
            match preview_link_bases {
                Some(bases) => match request.with_preview_link_bases(bases) {
                    Ok(request) => request,
                    Err(reason) => return failure("preview_link_bases", reason),
                },
                None => request,
            }
        }
        Err((code, reason)) => return failure(code, reason),
    };
    let (output, render, syntax, acquisition) = args.render.limits();
    runner_options.acquisition = acquisition;
    runner_options.output = output;
    runner_options.render = render;
    runner_options.syntax = syntax;
    let plan = match runner::plan(&request, &addresses, &runner_options) {
        Ok(plan) => plan,
        Err(error) => return failure(error.code(), &error.to_string()),
    };
    if args.plan {
        return super::write_json(&plan);
    }
    let mut control = match CloseInput::open() {
        Ok(control) => control,
        Err(_) => {
            return failure(
                "preview_control",
                "could not start bounded stdin close watcher",
            );
        }
    };
    match Kernel::new().execute(command::RunWithDocumentation {
        recipe: request,
        addresses,
        options: runner_options,
        observe: emit_runner,
        documentation: emit_documentation,
        control: || {
            Ok(if control.poll()? {
                preview::watch::Control::Stop
            } else {
                preview::watch::Control::Continue
            })
        },
    }) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => failure(
            error.code(),
            &format!("{error}; preview terminated; all listeners closed"),
        ),
    }
}

pub(super) fn run_production(args: ProductionArgs) -> ExitCode {
    if !args.render.local {
        return failure(
            "publication_mode",
            "--local is required; production preview is noncertified private serving",
        );
    }
    let listeners = preview::Options {
        allow_listeners: args.allow_listeners,
        max_sites: args.max_sites,
        max_header_bytes: args.max_header_bytes,
        max_headers: args.max_headers,
        request_timeout: Duration::from_millis(args.request_timeout_ms),
        response_timeout: Duration::from_millis(args.response_timeout_ms),
    };
    if let Err(error) = listeners.validate() {
        return failure(error.code(), &error.to_string());
    }
    let addresses = match bindings(&args.listen, listeners.max_sites) {
        Ok(addresses) => addresses,
        Err(reason) => return failure("preview_refused", reason),
    };
    let recipe = match args.render.request(true, listeners.max_sites) {
        Ok(recipe) => {
            let year = match crate::calendar::resolve(args.render.render_year) {
                Ok(year) => year,
                Err(reason) => return failure("render_year", reason),
            };
            recipe.with_default_render_year(year)
        }
        Err((code, reason)) => return failure(code, reason),
    };
    let (output, render, syntax, acquisition) = args.render.limits();
    let options = production_preview::Options {
        acquisition,
        listeners,
        syntax,
        output,
        render,
    };
    let request = production_preview::Request::new(recipe, args.registry, args.artifact, addresses);
    let plan = match production_preview::plan(&request, options) {
        Ok(plan) => plan,
        Err(error) => return failure(error.code(), &error.to_string()),
    };
    if args.plan {
        return super::write_json(&plan);
    }
    let mut control = match CloseInput::open() {
        Ok(control) => control,
        Err(_) => {
            return failure(
                "preview_control",
                "could not start bounded stdin close watcher",
            );
        }
    };
    match Kernel::new().execute(command::ProductionRun {
        request,
        options,
        observe: |event| emit(&event),
        control: || {
            Ok(if control.poll()? {
                preview::watch::Control::Stop
            } else {
                preview::watch::Control::Continue
            })
        },
    }) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => failure(
            error.code(),
            "production preview terminated; all listeners closed",
        ),
    }
}

fn bindings(values: &[String], maximum: usize) -> Result<BTreeMap<Key, SocketAddr>, &'static str> {
    if values.is_empty() || values.len() > maximum {
        return Err("listener set is empty or exceeds the site ceiling");
    }
    let mut result = BTreeMap::new();
    for value in values {
        let (site, socket) = value
            .split_once('=')
            .ok_or("expected site:key=loopback-address")?;
        let site = site.parse().map_err(|_| "invalid listener site identity")?;
        let socket: SocketAddr = socket
            .parse()
            .map_err(|_| "invalid explicit socket address")?;
        if !socket.ip().is_loopback() {
            return Err("preview listeners must use explicit loopback addresses");
        }
        if socket.port() != 0 && result.values().any(|existing| *existing == socket) {
            return Err("multiple sites cannot share one explicit listener address");
        }
        if result.insert(site, socket).is_some() {
            return Err("duplicate listener site identity");
        }
    }
    Ok(result)
}

fn emit(event: &impl Serialize) -> io::Result<()> {
    let mut bounded = gazette_sdk::render::LimitedWriter::new(Vec::new(), 1 << 20);
    serde_json::to_writer(&mut bounded, event).map_err(io::Error::other)?;
    bounded.write_all(b"\n")?;
    if bounded.exceeded() {
        return Err(io::Error::other("preview event limit"));
    }
    let mut output = io::stdout().lock();
    output.write_all(&bounded.into_inner())?;
    output.flush()
}

fn runner_documentation(
    event: &mut runner::Event,
) -> Option<&mut Vec<gazette_sdk::build::DocumentationReceipt>> {
    match event {
        runner::Event::Ready { state }
        | runner::Event::Changed { state }
        | runner::Event::Recovered { state } => Some(&mut state.documentation),
        runner::Event::RebuildFailed { documentation, .. } => Some(documentation),
        runner::Event::Closed => None,
    }
}

#[derive(Serialize)]
struct ReceiptHeader<'a> {
    site: &'a Key,
    response: (),
    is_error: bool,
}
#[derive(Serialize)]
struct RunnerMetadata<'a> {
    #[serde(flatten)]
    event: &'a runner::Event,
    documentation: Vec<ReceiptHeader<'a>>,
}

fn emit_runner(mut event: runner::Event) -> io::Result<()> {
    if runner_documentation(&mut event).is_some_and(|receipts| !receipts.is_empty()) {
        return deliver_runner_event(event, crate::deliver_json, &mut io::stderr().lock());
    }
    emit(&event)
}

fn deliver_runner_event(
    mut event: runner::Event,
    deliver: impl FnOnce(&[u8]) -> io::Result<()>,
    diagnostic: &mut impl Write,
) -> io::Result<()> {
    let receipts = runner_documentation(&mut event)
        .map(std::mem::take)
        .unwrap_or_default();
    let metadata = RunnerMetadata {
        event: &event,
        documentation: receipts
            .iter()
            .map(|receipt| ReceiptHeader {
                site: &receipt.site,
                response: (),
                is_error: receipt.is_error(),
            })
            .collect(),
    };
    let mut bounded = gazette_sdk::render::LimitedWriter::new(io::sink(), 1 << 20);
    let checked = serde_json::to_writer(&mut bounded, &metadata)
        .map_err(io::Error::other)
        .and_then(|()| bounded.write_all(b"\n"));
    if let Err(error) = checked {
        // Refuse oversized state without discarding already retained native receipts.
        let custody = serde_json::json!({"event":"documentation", "status":"preview-metadata-refused", "documentation":receipts});
        return deliver_documentation_event(&custody, |_| Err(error), diagnostic);
    }
    if let Some(documentation) = runner_documentation(&mut event) {
        *documentation = receipts;
    }
    deliver_documentation_event(&event, deliver, diagnostic)
}

// A process-only stdin watcher shared by immutable and rebuilding previews.
// If startup or receipt delivery fails, process exit ends any pending read.
struct CloseInput {
    receiver: mpsc::Receiver<io::Result<()>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl CloseInput {
    fn open() -> io::Result<Self> {
        let (sender, receiver) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("gazette-preview-stdin".into())
            .spawn(move || {
                let result = io::stdin().lock().read(&mut [0]).map(|_| ());
                let _ = sender.send(result);
            })?;
        Ok(Self {
            receiver,
            thread: Some(thread),
        })
    }

    fn poll(&mut self) -> io::Result<bool> {
        match self.receiver.try_recv() {
            Ok(result) => {
                result?;
                if let Some(thread) = self.thread.take() {
                    thread
                        .join()
                        .map_err(|_| io::Error::other("stdin close watcher failed"))?;
                }
                Ok(true)
            }
            Err(TryRecvError::Empty) => Ok(false),
            Err(TryRecvError::Disconnected) => {
                Err(io::Error::other("stdin close watcher disconnected"))
            }
        }
    }
}

fn failure(code: &str, reason: &str) -> ExitCode {
    let _ = writeln!(io::stderr().lock(), "gazette: {code}: {reason}");
    ExitCode::FAILURE
}

fn emit_documentation(delivery: gazette_sdk::build::DocumentationDelivery<'_>) -> io::Result<()> {
    use gazette_sdk::build::DocumentationDelivery;
    let mut value = match delivery {
        DocumentationDelivery::Complete { receipts } => {
            serde_json::json!({"status":"documentation-complete","documentation":receipts})
        }
        DocumentationDelivery::ReplayedFailure { receipts } => {
            serde_json::json!({"status":"documentation-unchanged-failure","documentation":receipts})
        }
        DocumentationDelivery::Refused { receipts, error } => {
            serde_json::json!({"status":"documentation-refused","documentation":receipts,"code":error.code(),"error":error.to_string()})
        }
        DocumentationDelivery::Native {
            site,
            receipts,
            execution,
        } => crate::command::publication::documentation_failure(site, receipts, execution)
            .map_err(io::Error::other)?,
    };
    value["event"] = "documentation".into();
    deliver_documentation_event(&value, crate::deliver_json, &mut io::stderr().lock())
}

// Native receipt capacity was reserved by its operation. The separate preview-state
// event ceiling cannot discard that custody after stages have been created.
fn deliver_documentation_event(
    event: &impl Serialize,
    deliver: impl FnOnce(&[u8]) -> io::Result<()>,
    diagnostic: &mut impl Write,
) -> io::Result<()> {
    let mut bytes = serde_json::to_vec(event).map_err(io::Error::other)?;
    bytes.push(b'\n');
    if let Err(error) = deliver(&bytes) {
        let _ = writeln!(
            diagnostic,
            "gazette: output_io: {error}; original documentation evidence follows; inspect before retrying"
        );
        let _ = diagnostic.write_all(&bytes);
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
#[path = "../tests/unit/preview_documentation.rs"]
mod tests;
