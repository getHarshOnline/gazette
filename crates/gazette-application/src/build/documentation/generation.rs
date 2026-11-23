//! Effectful generation precedes the shared, pure documentation target assembly.

use super::{
    AcquiredInputs, DocumentationAssembly, DocumentationError, DocumentationInput,
    DocumentationNavigation, selection, target_plan,
};
use gazette_core::{documentation::generator, process, source, stage};
use gazette_ports::{
    documentation::{DocumentationTarget, Generator},
    stage::{StageRetention, StageWriter},
};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    fmt,
    path::{Path, PathBuf},
};

/// Explicit native work bounds; per-job limits do not imply a sandbox.
#[derive(Clone, Copy, Debug)]
pub struct DocumentationGenerationOptions {
    /// Per-input limits and complete repeated input-stage byte/entry allowance.
    pub input: source::Limits,
    /// Per-file limits and complete generated-set byte/entry allowance, decremented per job.
    pub output: source::Limits,
    /// Explicit execution permission and shared per-invocation deadline.
    pub execution: process::Options,
    /// Complete stdout ceiling per version/generation invocation.
    pub max_capture_bytes: usize,
    /// Complete selected job ceiling, checked before staging; at most 128.
    pub max_jobs: usize,
    /// Positive canonical stage path-byte bound, at most one MiB, checked before creation.
    pub directory_bytes: usize,
    /// Positive selected artifact entry-byte bound, at most one MiB, checked before staging.
    pub entry_bytes: usize,
}

/// Native locators and exact execution evidence, not target compatibility or publication approval.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct DocumentationJob {
    /// Configuration module position, preserving navigation order.
    pub module: usize,
    /// Configuration reference position within the module.
    pub reference: usize,
    /// Retained source stage; no cleanup occurs when this evidence is dropped.
    pub input: PathBuf,
    /// Retained output stage, including companion files.
    pub output: PathBuf,
    /// Complete captured companion-tree identity, retained even if a later job fails.
    pub output_identity: source::TreeIdentity,
    /// Exact selected file or directory within that captured tree.
    pub selection: gazette_core::documentation::Selection,
    /// Native file size/digest or directory kind, bound to the complete output identity.
    pub content: source::Content,
    /// Original execution observations, separate from portable artifact identity.
    pub evidence: generator::Evidence,
}

/// Completed generation bound to the same acquired configuration and captured sources.
#[derive(Debug)]
pub struct DocumentationGeneration<'a> {
    inputs: &'a AcquiredInputs,
    generated: BTreeMap<(usize, usize), generator::Generated>,
    jobs: Vec<DocumentationJob>,
}

impl DocumentationGeneration<'_> {
    /// Exact completed native jobs; imported inputs have no synthetic job.
    pub fn jobs(&self) -> &[DocumentationJob] {
        &self.jobs
    }

    /// Use the same target assembly as imported artifacts, without rerunning generators.
    ///
    /// # Errors
    /// Rejects unsupported formats, invalid artifacts and complete target limits.
    pub fn assemble_documentation(
        &self,
        target: &impl DocumentationTarget,
        limits: source::Limits,
    ) -> Result<DocumentationAssembly<'_>, DocumentationError> {
        self.inputs.assemble_documentation_inputs(
            target,
            limits,
            Some(&self.generated),
            self.jobs
                .iter()
                .flat_map(|job| [job.input.clone(), job.output.clone()])
                .collect(),
        )
    }
}

/// Failed generation preserves every completed job and known retained native locator.
#[derive(Debug)]
pub struct DocumentationGenerationError {
    /// Completed prefix; not a successful partial publication.
    pub completed: Vec<DocumentationJob>,
    /// All stages retained before failure, including the active job if available.
    pub retained: Vec<PathBuf>,
    /// Exact native failure; never an instruction to retry or delete residue.
    pub cause: DocumentationGenerationCause,
}

/// Original failure owner at the generation boundary.
#[derive(Debug)]
pub enum DocumentationGenerationCause {
    /// Configuration/acquisition mismatch or missing selection.
    Selection(Box<DocumentationError>),
    /// Native execution permission or command bounds failed before staging.
    Process(process::Error),
    /// Portable generator paths could not bind before any stage was created.
    Binding(crate::configuration::RuntimeBindingError),
    /// Whole-job ceiling exceeded before staging.
    Limit,
    /// Fresh staging or retention failed; may include additional residue.
    Stage(Box<stage::Error>),
    /// Native generator refused or failed; original process custody is retained.
    Generator(Box<generator::Error>),
}

impl DocumentationGenerationError {
    /// Stable failure class without disclosing source bytes or command arguments.
    pub fn code(&self) -> &'static str {
        match &self.cause {
            DocumentationGenerationCause::Selection(error) => error.code(),
            DocumentationGenerationCause::Process(error) => error.code(),
            DocumentationGenerationCause::Binding(error) => error.code(),
            DocumentationGenerationCause::Limit => "documentation_generation_limit",
            DocumentationGenerationCause::Stage(_) => "documentation_generation_stage",
            DocumentationGenerationCause::Generator(error) => error.code(),
        }
    }
}
impl fmt::Display for DocumentationGenerationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}; {} completed jobs; retained stages {:?}; native cause {:?}; no automatic retry or cleanup",
            self.code(),
            self.completed.len(),
            self.retained,
            self.cause
        )?;
        write!(f, "; completed job evidence {:?}", self.completed)
    }
}
impl std::error::Error for DocumentationGenerationError {}

impl AcquiredInputs {
    /// Materialize exact captures, retain private stages, invoke configured tools and recapture output.
    /// All recipes and execution bounds are checked before any stage creation. Generation runs in
    /// configured order and stops at the first failure without returning a successful subset.
    /// Native commands are not sandboxed; executable pins do not pin tool imports or dependencies.
    ///
    /// # Errors
    /// Preserves completed jobs, retained stages and native failure/custody without cleanup or retry.
    pub fn generate_documentation<S: StageWriter>(
        &self,
        writer: &S,
        generator: &impl Generator,
        target: &impl DocumentationTarget,
        parent: &Path,
        options: DocumentationGenerationOptions,
    ) -> Result<DocumentationGeneration<'_>, DocumentationGenerationError>
    where
        S::Stage: StageRetention,
    {
        self.generate_documentation_with_protected(writer, generator, target, parent, options, &[])
    }

    /// Generate through the same native stages while protecting additional publication/configuration roots.
    /// Every actual stage write rechecks the complete union through the existing stage writer.
    ///
    /// # Errors
    /// Preserves the same original generation custody and refuses unsafe native stage ancestry.
    pub fn generate_documentation_with_protected<S: StageWriter>(
        &self,
        writer: &S,
        generator: &impl Generator,
        target: &impl DocumentationTarget,
        parent: &Path,
        options: DocumentationGenerationOptions,
        additional_roots: &[PathBuf],
    ) -> Result<DocumentationGeneration<'_>, DocumentationGenerationError>
    where
        S::Stage: StageRetention,
    {
        let mut jobs = Vec::new();
        let mut retained = Vec::new();
        let mut generated = BTreeMap::new();
        let result = (|| {
            let (brand, _, build) = selection(&self.compilation().workspace, self.site())
                .map_err(|error| DocumentationGenerationCause::Selection(Box::new(error)))?;
            target
                .preflight(target_plan(brand, build))
                .map_err(|error| {
                    DocumentationGenerationCause::Selection(Box::new(DocumentationError::Target(
                        error,
                    )))
                })?;
            let selected: Vec<_> = build
                .modules
                .iter()
                .enumerate()
                .flat_map(|(module, value)| {
                    value
                        .navigation
                        .iter()
                        .enumerate()
                        .filter_map(move |(reference, value)| match value {
                            DocumentationNavigation::Reference { input, .. } => {
                                match input.as_ref() {
                                    DocumentationInput::Generated { source, recipe } => {
                                        Some((module, reference, source, recipe))
                                    }
                                    DocumentationInput::Artifact { .. } => None,
                                }
                            }
                            DocumentationNavigation::Manual { .. } => None,
                        })
                })
                .collect();
            if options.max_jobs > 128
                || !(1..=1 << 20).contains(&options.directory_bytes)
                || !(1..=1 << 20).contains(&options.entry_bytes)
                || selected.len() > options.max_jobs
                || options.max_capture_bytes > 64 << 20
            {
                return Err(DocumentationGenerationCause::Limit);
            }
            let selected: Vec<_> = selected
                .into_iter()
                .map(|(module, reference, source, recipe)| {
                    self.configuration()
                        .documentation_recipe(recipe)
                        .map(|recipe| (module, reference, source, recipe))
                        .map_err(DocumentationGenerationCause::Binding)
                })
                .collect::<Result<_, _>>()?;
            // Preflight the entire configured set before the first native write.
            let mut input_bytes = 0u64;
            let mut input_entries = 0usize;
            for (_, _, key, recipe) in &selected {
                if recipe
                    .output
                    .path()
                    .is_some_and(|path| path.as_str().len() > options.entry_bytes)
                {
                    return Err(DocumentationGenerationCause::Limit);
                }
                recipe
                    .validate()
                    .map_err(DocumentationGenerationCause::Process)?;
                options
                    .execution
                    .validate(
                        &process::Command {
                            program: recipe.program.clone(),
                            executable: Some(recipe.executable),
                            arguments: recipe.version_arguments.clone(),
                            directory: "/".into(),
                            environment: recipe.environment.clone(),
                        },
                        &[],
                    )
                    .map_err(DocumentationGenerationCause::Process)?;
                if !self.sources().contains_key(*key) {
                    return Err(DocumentationGenerationCause::Selection(Box::new(
                        DocumentationError::Input {
                            source: (*key).clone(),
                            selection: recipe.output.clone(),
                        },
                    )));
                }
            }
            for (_, _, key, _) in &selected {
                let inventory = self.sources()[*key].inventory();
                input_bytes = input_bytes
                    .checked_add(inventory.total_bytes())
                    .filter(|&bytes| bytes <= options.input.total_bytes)
                    .ok_or(DocumentationGenerationCause::Limit)?;
                input_entries = input_entries
                    .checked_add(inventory.entries().len())
                    .filter(|&entries| entries <= options.input.entries)
                    .ok_or(DocumentationGenerationCause::Limit)?;
            }
            let mut roots = self
                .source_roots()
                .map_err(|error| DocumentationGenerationCause::Selection(Box::new(error.into())))?;
            roots.extend_from_slice(additional_roots);
            roots.sort();
            roots.dedup();
            let empty = source::CapturedSource::from_parts(
                source::InventoryBuilder::new(options.output).finish(),
                BTreeMap::new(),
            )
            .map_err(|error| DocumentationGenerationCause::Stage(Box::new(error.into())))?;
            let mut remaining = options.output;
            for (module, reference, key, recipe) in selected {
                let capture = &self.sources()[key];
                let payload = capture
                    .as_input()
                    .map_err(|error| DocumentationGenerationCause::Stage(Box::new(error.into())))?;
                let input = writer
                    .stage(
                        stage::Location::FreshBounded {
                            parent,
                            directory_bytes: options.directory_bytes,
                        },
                        &roots,
                        payload,
                        options.input,
                    )
                    .and_then(StageRetention::retain)
                    .map_err(|error| DocumentationGenerationCause::Stage(Box::new(error)))?;
                retained.push(input.path().to_owned());
                let output = writer
                    .stage(
                        stage::Location::FreshBounded {
                            parent,
                            directory_bytes: options.directory_bytes,
                        },
                        &roots,
                        empty.as_input(),
                        options.output,
                    )
                    .and_then(StageRetention::retain)
                    .map_err(|error| DocumentationGenerationCause::Stage(Box::new(error)))?;
                retained.push(output.path().to_owned());
                let artifact = generator
                    .generate(generator::Request {
                        recipe: &recipe,
                        input: &input,
                        output: &output,
                        input_limits: options.input,
                        output_limits: remaining,
                        execution: options.execution,
                        max_capture_bytes: options.max_capture_bytes,
                    })
                    .map_err(|error| DocumentationGenerationCause::Generator(Box::new(error)))?;
                let inventory = artifact.output.inventory();
                // Reapply every caller ceiling at the port boundary, including companions.
                // A valid capture may have been constructed under looser provider limits.
                let mut checked = source::InventoryBuilder::new(remaining);
                for entry in inventory.entries() {
                    checked
                        .insert(entry.path.clone(), entry.content)
                        .map_err(|_| DocumentationGenerationCause::Limit)?;
                }
                remaining.total_bytes = remaining
                    .total_bytes
                    .checked_sub(inventory.total_bytes())
                    .ok_or(DocumentationGenerationCause::Limit)?;
                remaining.entries = remaining
                    .entries
                    .checked_sub(inventory.entries().len())
                    .ok_or(DocumentationGenerationCause::Limit)?;
                if artifact.selection != recipe.output || artifact.format != recipe.format {
                    return Err(DocumentationGenerationCause::Selection(Box::new(
                        DocumentationError::Input {
                            source: key.clone(),
                            selection: recipe.output.clone(),
                        },
                    )));
                }
                let content = recipe
                    .output
                    .content(&artifact.output.as_input())
                    .map_err(|_| {
                        DocumentationGenerationCause::Selection(Box::new(
                            DocumentationError::Input {
                                source: key.clone(),
                                selection: recipe.output.clone(),
                            },
                        ))
                    })?;
                jobs.push(DocumentationJob {
                    module,
                    reference,
                    input: input.path().to_owned(),
                    output: output.path().to_owned(),
                    output_identity: inventory.identity(),
                    selection: recipe.output.clone(),
                    content,
                    evidence: artifact.evidence.clone(),
                });
                generated.insert((module, reference), artifact);
            }
            Ok(())
        })();
        match result {
            Ok(()) => Ok(DocumentationGeneration {
                inputs: self,
                generated,
                jobs,
            }),
            Err(cause) => Err(DocumentationGenerationError {
                completed: jobs,
                retained,
                cause,
            }),
        }
    }
}
