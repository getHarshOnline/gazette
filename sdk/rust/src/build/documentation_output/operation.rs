//! One SDK-owned documentation application sequence.

use super::*;

/// Explicit native generation choice. Omission never invokes a configured tool.
#[derive(Clone, Copy, Debug)]
pub struct GenerationSelection<'a> {
    /// Existing private parent for fresh generator input/output stages.
    pub parent: &'a Path,
    /// Exact execution permissions and complete generation bounds.
    pub options: DocumentationGenerationOptions,
}

/// One optional post-assembly local effect. Remote publication is not represented.
#[derive(Clone, Copy, Debug, Default)]
pub enum RetentionSelection<'a> {
    /// Return assembled bytes without retaining a target.
    #[default]
    None,
    /// Retain one fresh private target below the selected parent.
    Stage {
        /// Existing private parent; configured publication destinations are not inferred.
        parent: &'a Path,
        /// Canonical path-byte ceiling checked by the native stage owner.
        directory_bytes: usize,
    },
    /// Durably retain the exact documentation build inputs in an existing journal.
    Preparation {
        /// Caller-selected immutable preparation coordinates and permissions.
        selection: &'a PreparationSelection,
        /// Optional local coordination selection; omission does not discover one.
        coordinator: Option<&'a crate::store::coordinator::LocalCoordinatorSelection>,
        /// Separate permission for the selected coordinator anchor write.
        allow_anchor_write: bool,
    },
}

/// Complete documentation operation owned by the native SDK.
///
/// Interfaces decode this request and deliver [`Execution`]; they do not reproduce phase order.
pub struct Operation<'a> {
    /// Explicit preview-only ordinary manual hyperlink bases.
    pub preview_link_bases: Option<&'a gazette_core::publication::routing::PreviewLinkBases>,
    /// Existing shared supplied/local selection; no discovery or process-directory fallback.
    pub input: &'a super::super::WorkspaceInput,
    /// Exact configured publication site.
    pub site: &'a crate::workspace::Key,
    /// Supplied workspace syntax ceilings; local selection carries its complete-set limits.
    pub syntax: crate::toml::Limits,
    /// Complete selected-source acquisition bounds.
    pub acquisition: crate::source::PreparationLimits,
    /// Complete generated/assembled output bounds.
    pub output: crate::source::Limits,
    /// Optional explicit native generation.
    pub generation: Option<GenerationSelection<'a>>,
    /// Optional local target retention or durable preparation.
    pub retention: RetentionSelection<'a>,
    /// Complete response ceiling, excluding one transport newline.
    pub response_bytes: usize,
    /// Reserve and serialize a read-only result natively. Effectful selections always do this.
    pub reserve_read_only_response: bool,
}

/// Failure before any generator, target-stage or preparation effect starts.
#[derive(Debug)]
pub enum BeforeError {
    /// Complete response custody could not be reserved.
    Response(&'static str),
    /// Exact shared configuration acquisition or admission failure, before selected-source reads.
    Configuration(crate::configuration::WorkspaceLoadError),
    /// Configured inputs could not be acquired.
    Acquisition(DocumentationError),
    /// Retained inputs could not produce one complete target before any selected effect.
    Assembly(DocumentationError),
    /// Effect selections could not form one internally consistent bounded operation.
    Selection(&'static str),
}

impl BeforeError {
    /// Stable native failure class shared by command and protocol transports.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Response(code) | Self::Selection(code) => code,
            Self::Configuration(error) => error.code(),
            Self::Acquisition(error) | Self::Assembly(error) => error.code(),
        }
    }
}

impl fmt::Display for BeforeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Response(code) | Self::Selection(code) => write!(formatter, "{code}"),
            Self::Configuration(error) => error.fmt(formatter),
            Self::Acquisition(error) | Self::Assembly(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for BeforeError {}

/// One native documentation result projected by CLI, MCP and language SDK transports.
pub enum Execution<'a> {
    /// No generator, target-stage or preparation effect started.
    Before(BeforeError),
    /// Read-only assembly for transport-owned bounded projection; no effect custody exists.
    Complete {
        /// One SDK-owned typed report with the closed native status vocabulary.
        report: Report<'a>,
        /// Retained native assembly, borrowed only within this owning callback.
        assembly: &'a DocumentationAssembly<'a>,
    },
    /// Bounded full report or compact exact custody. Output bytes exist only for full success.
    Delivered {
        /// Pre-reserved native JSON bytes.
        response: Encoded,
        /// Present only for a fully delivered success, never a compact or failed response.
        assembly: Option<&'a DocumentationAssembly<'a>>,
        /// Exact completed native generation jobs; imported references invent none.
        jobs: &'a [DocumentationJob],
    },
    /// A caller/native bounds invariant failed after work; original typed custody survives.
    Contract {
        /// Original native results, never reconstructed from paths or mutable state.
        custody: Custody<'a>,
    },
}

/// Run the existing operation over already acquired exact documentation inputs.
/// Selection and source acquisition remain with their original owner.
pub struct AcquiredOperation<'a> {
    /// Immutable configuration and independently captured documentation sources.
    pub inputs: &'a AcquiredInputs,
    /// Additional publication/configuration roots rechecked at every native stage write.
    pub protected_roots: &'a [PathBuf],
    /// Explicit outbound hyperlink projection; this does not start a documentation server.
    pub preview_link_bases: Option<&'a gazette_core::publication::routing::PreviewLinkBases>,
    /// Complete target output limits.
    pub output: crate::source::Limits,
    /// Explicit existing generation selection, including its private stage parent.
    pub generation: Option<GenerationSelection<'a>>,
    /// Optional existing target retention; ordinary publication discovery uses None.
    pub retention: RetentionSelection<'a>,
    /// Complete native receipt ceiling reserved before any effects.
    pub response_bytes: usize,
    /// Reserve the same native response for an imported-only operation.
    pub reserve_read_only_response: bool,
}
fn delivery_bounds(
    generation: Option<GenerationSelection<'_>>,
    retention: RetentionSelection<'_>,
) -> Result<Bounds, BeforeError> {
    let mut directory_bytes = 1;
    let (jobs, entry_bytes) = generation.map_or((0, 1), |generation| {
        directory_bytes = generation.options.directory_bytes;
        (generation.options.max_jobs, generation.options.entry_bytes)
    });
    match retention {
        RetentionSelection::None => {}
        RetentionSelection::Stage {
            directory_bytes: stage_bytes,
            ..
        } => {
            if stage_bytes == 0 {
                return Err(BeforeError::Response("documentation_response_limits"));
            }
            directory_bytes = directory_bytes.max(stage_bytes);
        }
        RetentionSelection::Preparation { selection, .. } => {
            let preparation_bytes = selection
                .required_directory_bytes()
                .map_err(BeforeError::Selection)?;
            selection
                .validate(preparation_bytes)
                .map_err(BeforeError::Selection)?;
            directory_bytes = directory_bytes.max(preparation_bytes);
        }
    }
    Ok(Bounds {
        jobs,
        directory_bytes,
        entry_bytes,
    })
}
fn reserve(
    preview: bool,
    generation: Option<GenerationSelection<'_>>,
    retention: RetentionSelection<'_>,
    response_bytes: usize,
    read_only: bool,
) -> Result<Option<Reservation>, BeforeError> {
    if preview && matches!(retention, RetentionSelection::Preparation { .. }) {
        return Err(BeforeError::Selection("documentation_preview_preparation"));
    }
    let bounds = delivery_bounds(generation, retention)?;
    if read_only || generation.is_some() || !matches!(retention, RetentionSelection::None) {
        Reservation::new(bounds, response_bytes)
            .map(Some)
            .map_err(BeforeError::Response)
    } else {
        Ok(None)
    }
}
impl Operation<'_> {
    /// Run acquisition, optional generation, assembly and optional retention once.
    /// Native response capacity is reserved before effects; original failure custody stays borrowed.
    pub fn execute_with_delivery<T>(self, deliver: impl FnOnce(Execution<'_>) -> T) -> T {
        let reservation = match reserve(
            self.preview_link_bases.is_some(),
            self.generation,
            self.retention,
            self.response_bytes,
            self.reserve_read_only_response,
        ) {
            Ok(value) => value,
            Err(error) => return deliver(Execution::Before(error)),
        };
        let configuration = match self.input.resolve(self.syntax) {
            Ok(configuration) => configuration,
            Err(error) => return deliver(Execution::Before(BeforeError::Configuration(error))),
        };
        let inputs = match gazette_application::build::acquire_resolved_documentation(
            &gazette_adapter_fs::FileSystem,
            &gazette_adapter_git::Git,
            &gazette_adapter_markdown::Comrak,
            configuration,
            self.site,
            self.acquisition,
        ) {
            Ok(inputs) => inputs,
            Err(error) => {
                return deliver(Execution::Before(BeforeError::Acquisition(error)));
            }
        };
        AcquiredOperation {
            inputs: &inputs,
            protected_roots: &[],
            preview_link_bases: self.preview_link_bases,
            output: self.output,
            generation: self.generation,
            retention: self.retention,
            response_bytes: self.response_bytes,
            reserve_read_only_response: self.reserve_read_only_response,
        }
        .execute_reserved(reservation, deliver)
    }
}
impl AcquiredOperation<'_> {
    /// Execute the same native phases without reacquiring configuration or source bytes.
    /// Contract failures reach the callback while their original custody is alive.
    pub fn execute_with_delivery<T>(self, deliver: impl FnOnce(Execution<'_>) -> T) -> T {
        let reservation = match reserve(
            self.preview_link_bases.is_some(),
            self.generation,
            self.retention,
            self.response_bytes,
            self.reserve_read_only_response,
        ) {
            Ok(value) => value,
            Err(error) => return deliver(Execution::Before(error)),
        };
        self.execute_reserved(reservation, deliver)
    }
    fn execute_reserved<T>(
        self,
        reservation: Option<Reservation>,
        deliver: impl FnOnce(Execution<'_>) -> T,
    ) -> T {
        let inputs = self.inputs;
        let generated = match self.generation {
            Some(selection) => {
                match super::super::generate_documentation_with_protected(
                    inputs,
                    selection.parent,
                    selection.options,
                    self.protected_roots,
                ) {
                    Ok(generated) => Some(generated),
                    Err(error) => {
                        return deliver(finish_reserved(
                            reservation,
                            None::<&Report<'_>>,
                            Custody::GenerationFailed(&error),
                            None,
                            &error.completed,
                        ));
                    }
                }
            }
            None => None,
        };
        let jobs = generated
            .as_ref()
            .map_or(&[][..], |generated| generated.jobs());
        let target = gazette_adapter_mintlify::Mintlify::new(&gazette_adapter_markdown::Comrak)
            .with_preview_link_bases(self.preview_link_bases);
        let assembled = match &generated {
            Some(generated) => generated.assemble_documentation(&target, self.output),
            None => inputs.assemble_documentation(&target, self.output),
        };
        let assembled = match assembled {
            Ok(assembled) => assembled,
            Err(error) if generated.is_some() => {
                return deliver(finish_reserved(
                    reservation,
                    None::<&Report<'_>>,
                    Custody::AssemblyFailed {
                        jobs,
                        error: &error,
                    },
                    None,
                    jobs,
                ));
            }
            Err(error) => return deliver(Execution::Before(BeforeError::Assembly(error))),
        };
        let identity = assembled.output().inventory().identity();
        match self.retention {
            RetentionSelection::Preparation {
                selection,
                coordinator,
                allow_anchor_write,
            } => {
                let result = super::prepare_documentation(
                    &assembled,
                    selection.request(),
                    coordinator,
                    allow_anchor_write,
                    selection.options(self.output),
                );
                let report = result.as_ref().ok().map(|receipt| PreparedReport {
                    assembly: Report::from_assembly(
                        Status::PreparedLocalNotCertified,
                        inputs,
                        &assembled,
                        None,
                        jobs,
                    ),
                    preparation: receipt,
                });
                deliver(finish_reserved(
                    reservation,
                    report.as_ref(),
                    Custody::Preparation {
                        jobs,
                        output_identity: identity,
                        selection,
                        result: &result,
                    },
                    result.as_ref().ok().map(|_| &assembled),
                    jobs,
                ))
            }
            retention => {
                let retained = match retention {
                    RetentionSelection::None => None,
                    RetentionSelection::Stage {
                        parent,
                        directory_bytes,
                    } => match super::stage_documentation(
                        &assembled,
                        parent,
                        self.output,
                        directory_bytes,
                    )
                    .and_then(crate::source::StagedTree::retain)
                    {
                        Ok(retained) => Some(retained),
                        Err(error) => {
                            return deliver(finish_reserved(
                                reservation,
                                None::<&Report<'_>>,
                                Custody::StageFailed {
                                    jobs,
                                    output_identity: identity,
                                    error: &error,
                                },
                                None,
                                jobs,
                            ));
                        }
                    },
                    RetentionSelection::Preparation { .. } => unreachable!("handled above"),
                };
                let report = Report::from_assembly(
                    Status::AssembledLocal,
                    inputs,
                    &assembled,
                    retained.as_ref().map(|retained| retained.path()),
                    jobs,
                );
                match reservation {
                    Some(reservation) => deliver(finish(
                        reservation,
                        Some(&report),
                        Custody::Complete {
                            jobs,
                            output_identity: identity,
                            target: retained.as_ref(),
                        },
                        Some(&assembled),
                        jobs,
                    )),
                    None => deliver(Execution::Complete {
                        report,
                        assembly: &assembled,
                    }),
                }
            }
        }
    }
}

fn finish<'a, T: Serialize>(
    reservation: Reservation,
    full: Option<&'a T>,
    custody: Custody<'a>,
    assembly: Option<&'a DocumentationAssembly<'a>>,
    jobs: &'a [DocumentationJob],
) -> Execution<'a> {
    match reservation.finish(full, custody) {
        Ok(response) => Execution::Delivered {
            assembly: (!response.is_error).then_some(assembly).flatten(),
            jobs,
            response,
        },
        Err(error) => Execution::Contract {
            custody: error.custody,
        },
    }
}

fn finish_reserved<'a, T: Serialize>(
    reservation: Option<Reservation>,
    full: Option<&'a T>,
    custody: Custody<'a>,
    assembly: Option<&'a DocumentationAssembly<'a>>,
    jobs: &'a [DocumentationJob],
) -> Execution<'a> {
    match reservation {
        Some(reservation) => finish(reservation, full, custody, assembly, jobs),
        None => Execution::Contract { custody },
    }
}
