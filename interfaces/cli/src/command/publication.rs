//! Publication-set commands shared by CLI and MCP.

use super::{Command, Kernel};
use gazette_sdk::{build, process, render::LimitedWriter, source, toml};
use serde_json::Value;

/// Stable transport-neutral failure before a publication-family result exists.
#[derive(Debug)]
pub struct Failure {
    code: &'static str,
    message: String,
}

impl Failure {
    fn new(code: &'static str, message: impl ToString) -> Self {
        Self {
            code,
            message: message.to_string(),
        }
    }

    /// Stable native failure class.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.code
    }

    /// Redacted operator-facing diagnostic.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for Failure {}

/// Exact local render request. Permissions are part of the application command,
/// not inferred from the CLI or protocol transport.
pub struct Render {
    /// Boundary-captured presentation context; an explicit recipe year takes precedence.
    pub default_render_year: Option<gazette_sdk::authored::RenderYear>,
    /// Complete native recipe bytes.
    pub recipe_json: Vec<u8>,
    /// Explicit math override; absence preserves the request or workspace selection.
    pub math: Option<bool>,
    /// This call permits the selected math provider to execute.
    pub allow_math_execution: bool,
    /// Explicit local-operation acknowledgement.
    pub local: bool,
    /// Permission for configured native source reads.
    pub allow_read: bool,
    /// Complete recipe byte ceiling.
    pub max_input_bytes: usize,
    /// Workspace syntax ceilings.
    pub syntax: toml::Limits,
    /// Complete-set acquisition ceilings.
    pub acquisition: build::PublicationSetAcquisitionLimits,
    /// Per-site generated-output ceilings.
    pub output: source::Limits,
    /// Aggregate render ceilings.
    pub limits: build::LocalRenderLimits,
}

/// Owned publication inspection captured before retained inputs are released.
pub struct RenderOutcome {
    report: Value,
    complete: bool,
}

impl Render {
    pub(crate) fn parse(self) -> Result<ParsedRender, Failure> {
        if !self.local || !self.allow_read {
            return Err(Failure::new(
                "publication_permission",
                "explicit local mode and configured-source read permission required; no source acquisition started",
            ));
        }
        let mut recipe = build::LocalPublicationRecipe::parse_json(
            &self.recipe_json,
            self.max_input_bytes,
            self.acquisition.sites,
        )
        .map_err(|error| Failure::new("publication_set_input", error))?;
        recipe = recipe
            .with_math_execution(self.math, self.allow_math_execution)
            .map_err(|error| Failure::new("publication_set_input", error))?;
        if let Some(year) = self.default_render_year {
            recipe = recipe.with_default_render_year(year);
        }
        Ok(ParsedRender {
            recipe,
            syntax: self.syntax,
            acquisition: self.acquisition,
            output: self.output,
            limits: self.limits,
        })
    }

    /// Execute the shared acquisition/render pipeline once and consume the
    /// borrowed native result inside its owning stack frame.
    ///
    /// # Errors
    /// Returns the typed permission, parsing, acquisition, render or consumer
    /// failure without transport conversion.
    pub fn with_rendered<T>(
        self,
        consume: impl FnOnce(&build::RenderedPublicationSet<'_>) -> Result<T, Failure>,
    ) -> Result<T, Failure> {
        self.parse()?.consume_rendered(consume)?
    }
}

/// Validated request whose configuration and content acquisition have not started.
pub(crate) struct ParsedRender {
    pub(crate) recipe: build::LocalPublicationRecipe,
    syntax: toml::Limits,
    acquisition: build::PublicationSetAcquisitionLimits,
    output: source::Limits,
    limits: build::LocalRenderLimits,
}

/// One permission-checked retained configuration and its resolved site intent.
pub(crate) struct ResolvedRender {
    pub(crate) recipe: build::ResolvedLocalPublicationRecipe,
    acquisition: build::PublicationSetAcquisitionLimits,
    output: source::Limits,
    limits: build::LocalRenderLimits,
}

impl ParsedRender {
    pub(crate) fn resolve(self) -> Result<ResolvedRender, Failure> {
        Ok(ResolvedRender {
            recipe: self
                .recipe
                .resolve(self.syntax)
                .map_err(|error| Failure::new(error.code(), error))?,
            acquisition: self.acquisition,
            output: self.output,
            limits: self.limits,
        })
    }

    pub(crate) fn consume_rendered<T>(
        self,
        consume: impl FnOnce(&build::RenderedPublicationSet<'_>) -> T,
    ) -> Result<T, Failure> {
        self.resolve()?.consume_rendered(consume)
    }
}

impl ResolvedRender {
    pub(crate) fn consume_rendered_documentation<T>(
        self,
        consume: impl FnOnce(build::DocumentationRenderExecution<'_>) -> T,
    ) -> Result<T, Failure> {
        self.recipe
            .with_rendered_documentation(self.acquisition, self.output, self.limits, consume)
            .map_err(|error| Failure::new(error.code(), error))
    }
    /// Per-site output limits retained from the validated request.
    pub(crate) const fn output_limits(&self) -> source::Limits {
        self.output
    }

    /// Acquire and render once, then consume the borrowed native result in place.
    pub(crate) fn consume_rendered<T>(
        self,
        consume: impl FnOnce(&build::RenderedPublicationSet<'_>) -> T,
    ) -> Result<T, Failure> {
        self.recipe
            .with_rendered(self.acquisition, self.output, self.limits, consume)
            .map_err(|error| Failure::new(error.code(), error))
    }
}

impl RenderOutcome {
    /// Complete owned private inspection.
    #[must_use]
    pub fn report(&self) -> &Value {
        &self.report
    }

    /// Whether every explicitly requested site rendered.
    #[must_use]
    pub const fn is_complete(&self) -> bool {
        self.complete
    }
}

impl Command for Render {
    type Outcome = RenderOutcome;
    type Failure = Failure;

    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        render_outcome(self.parse()?.resolve()?)
    }
}

fn render_outcome(resolved: ResolvedRender) -> Result<RenderOutcome, Failure> {
    resolved.consume_rendered_documentation(|execution| match execution {
            build::DocumentationRenderExecution::Rendered {rendered,receipts}=> {
                let mut report=serde_json::to_value(rendered.inspect()).map_err(|error|Failure::new("publication_response",error))?;
                if !receipts.is_empty() {
                    report["documentation"]=serde_json::to_value(receipts).map_err(|error|Failure::new("documentation_response",error))?;
                }
                Ok(RenderOutcome {report,complete:rendered.is_complete()})
            }
            build::DocumentationRenderExecution::Refused {receipts,error}=> {
                if receipts.is_empty() {return Err(Failure::new(error.code(),error));}
                Ok(RenderOutcome {report:serde_json::json!({"status":"documentation-publication-failed","documentation":receipts,"code":error.code(),"error":error.to_string()}),complete:false})
            }
            build::DocumentationRenderExecution::Native {site,receipts,execution}=> {
                documentation_failure(site,receipts,execution).map(|report|RenderOutcome {report,complete:false})
            }
        })?
}

/// Invocation-owned documentation custody capacity for a bounded transport.
/// Configuration may be read to measure exact owner labels; source acquisition and effects follow reservation.
pub struct BoundedRender {
    /// Original native command, including this transport's execution permission.
    pub command: Render,
    /// Complete compact native report budget, after transport framing/escaping capacity is reserved.
    pub response_bytes: usize,
}
impl Command for BoundedRender {
    type Outcome = RenderOutcome;
    type Failure = Failure;
    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        let allow_execute = self.command.allow_math_execution;
        let mut resolved = self.command.parse()?.resolve()?;
        if resolved.recipe.documentation_generation().is_some() {
            let sites = resolved.recipe.sites().collect::<Vec<_>>();
            let largest = sites.iter().max_by_key(|site| {
                serde_json::to_vec(site)
                    .expect("serializable owner key")
                    .len()
            });
            let witness = serde_json::json!({
                "documentation": sites.iter().map(|site| serde_json::json!({
                    "site":site,"response":null,"is_error":false
                })).collect::<Vec<_>>(),
                "site":largest,"current":null,"custody":null,"is_error":false
            });
            let overhead = serde_json::to_vec(&documentation_reconciliation(&witness))
                .map_err(|error| Failure::new("documentation_response", error))?
                .len();
            resolved
                .recipe
                .restrict_documentation_delivery(
                    self.response_bytes.saturating_sub(overhead),
                    allow_execute,
                )
                .map_err(|error| Failure::new("documentation_response_limit", error))?;
        }
        render_outcome(resolved)
    }
}

/// Smaller failed delivery preserves every original native receipt and exceptional custody.
/// It does not claim the omitted publication inspection was delivered or invite a retry.
pub fn documentation_reconciliation(report: &Value) -> Value {
    let mut result = serde_json::json!({
        "status":"documentation-publication-output-limit",
        "code":"documentation_publication_response_limit",
        "documentation":report.get("documentation").cloned().unwrap_or_else(||serde_json::json!([])),
    });
    for field in ["site", "current", "custody", "is_error"] {
        if let Some(value) = report.get(field) {
            result[field] = value.clone();
        }
    }
    result
}

/// Complete-set technical qualification request.
pub struct Qualify {
    /// Complete native qualification recipe bytes.
    pub recipe_json: Vec<u8>,
    /// Complete recipe byte ceiling.
    pub max_input_bytes: usize,
    /// Permission for configured native source reads.
    pub allow_read: bool,
    /// Permission for configured candidate-policy execution.
    pub allow_policy_execution: bool,
    /// Per-site generated-output ceilings.
    pub output: source::Limits,
}

impl Command for Qualify {
    type Outcome = build::QualificationReport;
    type Failure = Failure;

    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        let options = build::QualificationOptions {
            allow_read: self.allow_read,
            policy: process::Options {
                allow_execute: self.allow_policy_execution,
                ..Default::default()
            },
            output: self.output,
            ..Default::default()
        };
        let recipe = build::QualificationRecipe::parse_json(
            &self.recipe_json,
            self.max_input_bytes,
            options.acquisition.sites,
        )
        .map_err(|error| Failure::new("qualification_input", error))?;
        Ok(recipe.execute(options))
    }
}

/// Exact local preparation request with pre-reserved bounded custody delivery.
pub struct Prepare {
    /// Complete native preparation recipe bytes.
    pub recipe_json: Vec<u8>,
    /// Complete recipe byte ceiling.
    pub max_input_bytes: usize,
    /// Complete custody-response byte ceiling.
    pub max_output_bytes: usize,
    /// Explicit local-operation acknowledgement.
    pub local: bool,
    /// Native preparation permissions and bounds.
    pub options: build::LocalPreparationRunOptions,
}

/// Native bounded preparation report. Encoding is part of the SDK custody
/// contract because a smaller reconciliation receipt must survive effects.
pub struct Prepared {
    bytes: Vec<u8>,
    failed: bool,
}

impl Prepared {
    /// Pre-reserved native report or reconciliation receipt bytes.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Whether the encoded native result represents failure custody.
    #[must_use]
    pub const fn is_failure(&self) -> bool {
        self.failed
    }
}

impl Command for Prepare {
    type Outcome = Prepared;
    type Failure = Failure;

    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        if !self.local {
            return Err(Failure::new(
                "local_preparation_input",
                "--local is required",
            ));
        }
        let recipe = build::LocalPreparationRecipe::parse_json(
            &self.recipe_json,
            self.max_input_bytes,
            self.options.acquisition.sites,
        )
        .map_err(|error| Failure::new("local_preparation_input", error))?;
        recipe.execute_with_delivery(self.options, |report, failed| {
            let mut writer = LimitedWriter::new(Vec::new(), self.max_output_bytes);
            serde_json::to_writer(&mut writer, report)
                .map_err(|error| Failure::new("local_preparation_response", error))?;
            Ok(Prepared {
                bytes: writer.into_inner(),
                failed,
            })
        })
    }
}

/// Consume original native documentation failure inside its owning operation callback.
/// Every earlier receipt and exceptional Contract is preserved as structured private evidence.
pub(crate) fn documentation_failure(
    site: &gazette_sdk::workspace::Key,
    receipts: &[build::DocumentationReceipt],
    execution: build::documentation_output::Execution<'_>,
) -> Result<Value, Failure> {
    let mut report = serde_json::json!({"status":"documentation-publication-failed","site":site,"documentation":receipts});
    match execution {
        build::documentation_output::Execution::Before(error) => {
            report["code"] = error.code().into();
            report["error"] = error.to_string().into();
        }
        build::documentation_output::Execution::Delivered { response, .. } => {
            report["code"] = "documentation_native_delivery".into();
            report["current"] = serde_json::from_slice(&response.bytes)
                .map_err(|error| Failure::new("documentation_response", error))?;
            report["is_error"] = response.is_error.into();
        }
        build::documentation_output::Execution::Contract { custody } => {
            report["code"] = "documentation_response_contract".into();
            report["custody"] = serde_json::to_value(custody)
                .map_err(|error| Failure::new("documentation_response", error))?;
        }
        build::documentation_output::Execution::Complete {
            report: current, ..
        } => {
            report["code"] = "documentation_catalog_unavailable".into();
            report["current"] = serde_json::to_value(current)
                .map_err(|error| Failure::new("documentation_response", error))?;
        }
    }
    Ok(report)
}
