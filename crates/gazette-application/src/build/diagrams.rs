//! Explicit native DOT acquisition; pure resource rendering consumes retained checked PNG only.

mod dot;

use super::{AcquiredPublicationSet, programs::Program};
use gazette_compiler::{
    corpus,
    workspace::{ResourceSelection, ResourceUse},
};
use gazette_core::{
    process, raster, resource,
    source::{EntryPath, Sha256Digest, TreeIdentity},
};
use gazette_ports::{markdown::Markdown, process::IsolatedCaptureRunner};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

/// Explicit native effect permission and complete capture ceilings, not a sandbox.
#[derive(Clone, Copy, Debug)]
pub struct DiagramGenerationOptions {
    /// Complete DOT stdin ceiling and native deadline; execution defaults denied.
    pub execution: process::Options,
    /// Complete unique resource-job ceiling, at most 256.
    pub diagrams: usize,
    /// Aggregate retained PNG ceiling, at most 64 MiB.
    pub output_bytes: usize,
    /// Per-process stderr ceiling, at most 64 KiB; any nonempty diagnostics refuse success.
    pub diagnostic_bytes: usize,
    /// Existing decoded pixel and encoded-image limits.
    pub raster: raster::Limits,
}

impl Default for DiagramGenerationOptions {
    fn default() -> Self {
        Self {
            execution: Default::default(),
            diagrams: 256,
            output_bytes: 64 << 20,
            diagnostic_bytes: 64 << 10,
            raster: Default::default(),
        }
    }
}

/// Complete private native results; source records remain owned by their original captures.
/// Renderer executable pins do not attest shared libraries, plugins, fonts or ambient dependencies.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct DiagramCapture {
    workspace_sha256: Sha256Digest,
    records: Vec<Record>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
struct Record {
    input: TreeIdentity,
    entry: EntryPath,
    sources: BTreeSet<gazette_core::publication::Key>,
    command_sha256: Sha256Digest,
    request: Vec<u8>,
    response: Vec<u8>,
    diagnostics: Vec<u8>,
    exit: process::Exit,
    output_sha256: Sha256Digest,
    geometry: raster::Geometry,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
struct Attempt {
    input: TreeIdentity,
    entry: EntryPath,
    command_sha256: Sha256Digest,
    request: Vec<u8>,
    response: Option<Vec<u8>>,
    diagnostics: Option<Vec<u8>>,
    exit: Option<process::Exit>,
}

/// Refused acquisition preserves the completed prefix, active bytes and original process custody.
/// Diagnostics are private; Display exposes the stable class and safe corpus budget facts.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct DiagramGenerationError {
    code: &'static str,
    completed: Vec<Record>,
    active: Option<Attempt>,
    #[serde(skip)]
    #[schemars(skip)]
    process: Option<process::Error>,
    #[serde(skip)]
    #[schemars(skip)]
    native: Option<Box<dyn std::error::Error + Send + Sync>>,
    process_failure: Option<process::Failure>,
    cleanup: Option<process::Cleanup>,
}

impl DiagramGenerationError {
    fn before(code: &'static str) -> Self {
        Self {
            code,
            completed: Vec::new(),
            active: None,
            process: None,
            native: None,
            process_failure: None,
            cleanup: None,
        }
    }
    fn caused(code: &'static str, error: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self {
            native: Some(Box::new(error)),
            ..Self::before(code)
        }
    }
    /// Stable native classification without DOT text, native coordinates or diagnostics.
    pub fn code(&self) -> &'static str {
        self.code
    }
}
impl fmt::Display for DiagramGenerationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code)?;
        if let Some(limit) = self
            .native
            .as_ref()
            .and_then(|error| error.downcast_ref::<super::PublicationInputError>())
            .and_then(super::PublicationInputError::source_bytes_limit)
        {
            write!(f, "; before diagram execution: {limit}")?;
        }
        Ok(())
    }
}
impl std::error::Error for DiagramGenerationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.process
            .as_ref()
            .map(|error| error as _)
            .or_else(|| self.native.as_deref().map(|error| error as _))
    }
}

struct Job<'a> {
    input: TreeIdentity,
    entry: EntryPath,
    sources: BTreeSet<gazette_core::publication::Key>,
    bytes: &'a [u8],
}

impl DiagramCapture {
    pub(super) fn generate(
        inputs: &AcquiredPublicationSet,
        runner: &dyn IsolatedCaptureRunner,
        inspect: &raster::Inspect<'_>,
        corpus_limits: corpus::Limits,
        markdown: &dyn Markdown,
        options: DiagramGenerationOptions,
    ) -> Result<Self, DiagramGenerationError> {
        let refused = DiagramGenerationError::before;
        if inputs.diagram_capture().is_some() {
            return Err(refused("diagram_already_captured"));
        }
        if options.diagrams == 0
            || options.diagrams > 256
            || options.output_bytes == 0
            || options.output_bytes > 64 << 20
            || options.diagnostic_bytes > 64 << 10
            || options.execution.max_input_bytes == 0
            || options.execution.max_input_bytes > 64 << 20
            || options.execution.timeout < std::time::Duration::from_millis(1)
            || options.execution.timeout > std::time::Duration::from_secs(300)
            || options.raster.encoded_bytes == 0
            || options.raster.decoded_bytes == 0
            || options.raster.width == 0
            || options.raster.height == 0
        {
            return Err(refused("diagram_limit"));
        }
        // The same parsed declarations feed normal rendering. No DOT or Markdown re-parser.
        let mut jobs: BTreeMap<([u8; 32], EntryPath), Job<'_>> = BTreeMap::new();
        let mut admit = |source: &gazette_core::publication::Key,
                         entry: &EntryPath|
         -> Result<(), DiagramGenerationError> {
            let capture = inputs
                .sources()
                .get(source)
                .ok_or_else(|| refused("diagram_binding"))?;
            let bytes = capture
                .file(entry)
                .ok_or_else(|| refused("diagram_binding"))?;
            if bytes.is_empty() || bytes.len() > options.execution.max_input_bytes {
                return Err(refused("diagram_limit"));
            }
            dot::validate(bytes).map_err(|_| refused("diagram_profile"))?;
            let input = capture.inventory().identity();
            let job = jobs
                .entry((*input.sha256().as_bytes(), entry.clone()))
                .or_insert_with(|| Job {
                    input,
                    entry: entry.clone(),
                    sources: BTreeSet::new(),
                    bytes,
                });
            job.sources.insert(source.clone());
            if jobs.len() > options.diagrams {
                return Err(refused("diagram_limit"));
            }
            Ok(())
        };
        for (site, acquired) in inputs.sites() {
            let acquired = acquired.as_ref().map_err(|_| refused("diagram_input"))?;
            let parsed = acquired
                .parse_publication(corpus_limits, markdown)
                .map_err(|error| DiagramGenerationError::caused("diagram_input", error))?;
            let resolved = parsed
                .resolve(Default::default())
                .map_err(|error| DiagramGenerationError::caused("diagram_input", error))?;
            for node in resolved
                .authored()
                .content()
                .graph()
                .nodes()
                .filter(|node| &node.site == site)
            {
                let declarations = resolved.authored().resources(&node.id);
                if declarations.is_empty() {
                    continue;
                }
                let binding = resolved
                    .authored()
                    .source_binding(&node.id)
                    .map_err(|_| refused("diagram_binding"))?;
                for declaration in declarations {
                    if let Some(entry) = diagram_entry(&declaration.inputs, &declaration.build)? {
                        admit(binding.source_key(), entry)?;
                    }
                }
            }
            let metadata = acquired
                .check_metadata(Default::default())
                .map_err(|error| DiagramGenerationError::caused("diagram_input", error))?;
            let composition = inputs
                .compilation()
                .workspace
                .composition_for(site)
                .ok_or_else(|| refused("diagram_binding"))?;
            for selected in composition.resources().values() {
                let ResourceSelection::Enabled {
                    resource,
                    use_: ResourceUse::Site,
                    ..
                } = selected.value()
                else {
                    continue;
                };
                let admitted = metadata
                    .resource(resource)
                    .ok_or_else(|| refused("diagram_binding"))?;
                if let Some(entry) = diagram_entry(admitted.inputs, admitted.build)? {
                    let origins =
                        super::resource_inputs::resource_origins(&admitted).map_err(|error| {
                            DiagramGenerationError::caused("diagram_binding", error)
                        })?;
                    let origin = origins
                        .get(entry)
                        .ok_or_else(|| refused("diagram_binding"))?;
                    admit(&origin.source, &origin.entry)?;
                }
            }
        }
        let mut capture = Self {
            workspace_sha256: inputs.compilation().source_sha256,
            records: Vec::new(),
        };
        let mut input_bytes = 0usize;
        for job in jobs.values() {
            input_bytes = input_bytes
                .checked_add(job.bytes.len())
                .filter(|bytes| *bytes <= 64 << 20)
                .ok_or_else(|| refused("diagram_limit"))?;
        }
        if jobs.is_empty() {
            return Ok(capture);
        }
        let renderer = inputs
            .compilation()
            .workspace
            .dot_renderer()
            .ok_or_else(|| refused("diagram_unconfigured"))?;
        if !options.execution.allow_execute {
            return Err(refused("diagram_permission"));
        }
        renderer
            .executable
            .validate()
            .map_err(|_| refused("diagram_command"))?;
        let command = process::IsolatedCommand {
            executable: renderer.executable.clone(),
            arguments: vec!["-Kdot".into(), "-Tpng:cairo:cairo".into()],
            environment: BTreeMap::new(),
        };
        let command_sha256 = Sha256Digest::of(
            &serde_json::to_vec(&command).map_err(|_| refused("diagram_command"))?,
        );
        let mut remaining = options.output_bytes;
        for (_, job) in jobs {
            let mut active = Attempt {
                input: job.input,
                entry: job.entry.clone(),
                command_sha256,
                request: job.bytes.to_vec(),
                response: None,
                diagnostics: None,
                exit: None,
            };
            if remaining == 0 {
                return Err(DiagramGenerationError {
                    code: "diagram_limit",
                    completed: capture.records,
                    active: Some(active),
                    process: None,
                    native: None,
                    process_failure: None,
                    cleanup: None,
                });
            }
            let result = runner.capture_isolated(process::IsolatedCaptureRequest {
                request: process::IsolatedRequest {
                    command: &command,
                    input: job.bytes,
                    options: options.execution,
                },
                max_output_bytes: remaining.min(options.raster.encoded_bytes),
                max_stderr_bytes: Some(options.diagnostic_bytes),
            });
            let captured = match result {
                Ok(captured) => captured,
                Err(error) => {
                    return Err(DiagramGenerationError {
                        code: error.code(),
                        completed: capture.records,
                        active: Some(active),
                        process_failure: Some(error.failure),
                        cleanup: Some(error.cleanup),
                        process: Some(error),
                        native: None,
                    });
                }
            };
            active.exit = Some(captured.exit);
            active.response = Some(captured.stdout);
            active.diagnostics = captured.stderr;
            let response = active.response.as_deref().expect("captured stdout");
            let failure = if active.exit.is_none_or(|exit| exit.code != Some(0)) {
                Some("diagram_exit")
            } else if active
                .diagnostics
                .as_ref()
                .is_none_or(|bytes| !bytes.is_empty())
            {
                Some("diagram_diagnostics")
            } else if response.len() > remaining || response.len() > options.raster.encoded_bytes {
                Some("diagram_limit")
            } else {
                None
            };
            let mut native: Option<Box<dyn std::error::Error + Send + Sync>> = None;
            let inspection = if failure.is_none() {
                match inspect(raster::Format::Png, response, options.raster).and_then(|observed| {
                    observed.validate(raster::Format::Png, response, options.raster)?;
                    Ok(observed)
                }) {
                    Ok(observed) => Some(observed),
                    Err(error) => {
                        native = Some(Box::new(error));
                        None
                    }
                }
            } else {
                None
            };
            if let Some(code) = failure.or_else(|| inspection.is_none().then_some("diagram_raster"))
            {
                return Err(DiagramGenerationError {
                    code,
                    completed: capture.records,
                    active: Some(active),
                    process: None,
                    native,
                    process_failure: None,
                    cleanup: Some(process::Cleanup::Reaped),
                });
            }
            let image = inspection.expect("checked native pixels");
            let size = raster::Dimensions {
                width: image.width,
                height: image.height,
            };
            remaining -= response.len();
            capture.records.push(Record {
                input: job.input,
                entry: job.entry,
                sources: job.sources,
                command_sha256,
                request: active.request,
                response: active.response.expect("complete PNG"),
                diagnostics: active.diagnostics.expect("requested diagnostics"),
                exit: active.exit.expect("native exit"),
                output_sha256: image.sha256,
                geometry: raster::Geometry {
                    source: size,
                    oriented: size,
                    output: size,
                },
            });
        }
        Ok(capture)
    }

    pub(super) fn program(
        &self,
        workspace: Sha256Digest,
        capture: &crate::source::AcquiredInput,
        entry: &EntryPath,
        input: TreeIdentity,
        limits: resource::Limits,
    ) -> Result<Program, resource::Error> {
        let fail = || resource::Error::Input {
            path: entry.clone(),
            reason: "DOT resource has no exact retained PNG binding",
        };
        if self.workspace_sha256 != workspace {
            return Err(fail());
        }
        let record = self
            .records
            .iter()
            .find(|record| record.input == capture.inventory().identity() && &record.entry == entry)
            .ok_or_else(fail)?;
        if capture.file(entry) != Some(record.request.as_slice()) {
            return Err(fail());
        }
        if record.response.len() > limits.output_bytes
            || record.response.len() > limits.raster.encoded_bytes
            || record.geometry.output.width > limits.raster.width
            || record.geometry.output.height > limits.raster.height
            || u64::from(record.geometry.output.width)
                * u64::from(record.geometry.output.height)
                * 4
                > limits.raster.decoded_bytes
        {
            return Err(resource::Error::Limit("retained diagram pixels or bytes"));
        }
        Ok(Program {
            bytes: record.response.clone().into(),
            input,
            dependencies: BTreeMap::from([(entry.clone(), Sha256Digest::of(&record.request))]),
            references: BTreeSet::new(),
            deferred_references: Default::default(),
            svg: None,
            image: Some(record.geometry),
            tool: "graphviz-dot-png/retained-v1",
        })
    }
}

fn diagram_entry<'a>(
    paths: &[EntryPath],
    recipe: &'a resource::Recipe,
) -> Result<Option<&'a EntryPath>, DiagramGenerationError> {
    let resource::Recipe::DotImage { entry } = recipe else {
        return Ok(None);
    };
    if paths != [entry.clone()] {
        return Err(DiagramGenerationError::before("diagram_binding"));
    }
    Ok(Some(entry))
}
