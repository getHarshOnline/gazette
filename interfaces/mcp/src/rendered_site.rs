//! One-site consumers of the existing retained publication producer.

use crate::{publication, stage, tools};
use gazette_cli::command::{Kernel, rendered_output as command};
use gazette_cli::sdk::{
    build::{self, disclosure_input::ScanLimits},
    source, toml,
};
use rmcp::model::{CallToolResult, ErrorData, JsonObject, Tool, ToolAnnotations};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy)]
pub(super) enum Route {
    Stage,
    Probe,
}

impl Route {
    pub(super) const ALL: [Self; 2] = [Self::Stage, Self::Probe];

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Stage => "stage_rendered_site",
            Self::Probe => "probe_rendered_site",
        }
    }

    pub(super) fn descriptor(self) -> Tool {
        let (meaning, mut schema, output) = match self {
            Self::Stage => (
                "Use stage_rendered_site to render one site and keep its output as a fresh private tree. One call renders exactly one explicitly selected site through render_sites' shared SDK producer, then creates and synchronizes one fresh private output tree. Full evidence retains original source/render/inventory observations; compact error delivery keeps the exact native retention identity or residue path. No replacement, store, Git, certification, approval or publication. Reservation precedes native writes; caller loss is not rollback, cleanup permission or permission to retry. Approval and publication stay with the publisher.",
                schemars::schema_for!(Request<Stage>),
                tools::output_schema::<stage::Report<'static, Retained<'static>>>(),
            ),
            Self::Probe => (
                "Use probe_rendered_site to look for private literals in the output of one rendered site. One call renders exactly one explicitly selected site through render_sites' shared SDK producer, then runs the native private literal probe on that original output and its producer media declarations. No imported-tree recapture, staging, writes, browser execution, certification or publication. Every result is partial-evidence, including zero findings; exhausted bounds return no successful subset. Rules, identifiers, paths and results may be confidential. Report findings to the publisher, who decides whether the output may be published. No findings is not clearance.",
                schemars::schema_for!(Request<Probe>),
                tools::output_schema::<build::DisclosureProbe<'static>>(),
            ),
        };
        if matches!(self, Self::Probe) {
            let property = &mut schema.ensure_object()["properties"]["rules_json"];
            property["contentMediaType"] = "application/json".into();
            property["contentSchema"] = build::disclosure_input::output_schema().into();
        }
        let mutation = matches!(self, Self::Stage);
        Tool::new(self.name(), meaning, schema.ensure_object().clone())
            .with_raw_output_schema(output)
            .with_annotations(
                ToolAnnotations::new()
                    .read_only(!mutation)
                    .destructive(mutation)
                    .idempotent(!mutation)
                    .open_world(true),
            )
    }

    pub(super) fn decode(self, arguments: JsonObject) -> Result<Input, ErrorData> {
        let value = Value::Object(arguments);
        publication::validate_shape(value.get("publication").ok_or_else(invalid)?)?;
        if value.get("scan_limits").is_some_and(|v| !v.is_object()) {
            return Err(invalid());
        }
        let input = match self {
            Self::Stage => serde_json::from_value(value).map(Input::Stage),
            Self::Probe => serde_json::from_value(value).map(Input::Probe),
        }
        .map_err(|_| invalid())?;
        if let Input::Stage(r) = &input
            && (!r.mode.create_stage
                || !r.mode.output_parent.is_absolute()
                || !(1..=1 << 20).contains(&r.mode.directory_bytes))
        {
            return Err(invalid());
        }
        Ok(input)
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Request<Mode> {
    /// Same exact native recipe, permissions and phase budgets as `render_sites`; exactly one site.
    #[schemars(schema_with = "publication::input_schema")]
    publication: publication::Input,
    #[serde(flatten)]
    mode: Mode,
}

#[derive(Deserialize, JsonSchema)]
pub(super) struct Stage {
    create_stage: bool,
    output_parent: PathBuf,
    #[schemars(range(min = 1, max = 1048576))]
    directory_bytes: usize,
}

#[derive(Deserialize, JsonSchema)]
pub(super) struct Probe {
    rules_json: String,
    #[serde(default = "rules_bytes")]
    #[schemars(range(max=usize::MAX))]
    max_rules_bytes: usize,
    #[serde(default)]
    scan_limits: ScanLimits,
}
fn rules_bytes() -> usize {
    toml::Limits::default().source_bytes
}

#[derive(Serialize, JsonSchema)]
struct Retained<'a> {
    status: Status,
    inputs: build::CompositionInspection<'a>,
    evidence: &'a build::SiteRenderEvidence,
    inventory: &'a source::Inventory,
    directory: &'a Path,
}
#[derive(Serialize, JsonSchema)]
enum Status {
    #[expect(
        dead_code,
        reason = "schema-only projection; the command owner supplies the runtime report"
    )]
    #[serde(rename = "local_retained")]
    LocalRetained,
}

pub(super) enum Input {
    Stage(Request<Stage>),
    Probe(Request<Probe>),
}

impl Input {
    pub(super) fn execute(self, maximum: usize) -> Result<CallToolResult, ErrorData> {
        match self {
            Self::Stage(r) => {
                let reservation = stage::Reservation::new(r.mode.directory_bytes, maximum)
                    .map_err(|_| invalid())?;
                match Kernel::new().execute(command::StageRenderedSite {
                    publication: r.publication.into_command()?,
                    output_parent: r.mode.output_parent,
                    directory_bytes: r.mode.directory_bytes,
                }) {
                    Ok(outcome) => {
                        let (report, retained) = outcome.into_parts();
                        let retained = retained.expect("stage command retains one output");
                        stage::deliver_with(reservation, &Ok(retained), |_| &report)
                    }
                    Err(error) => match error.into_stage_error() {
                        Ok(error) => stage::deliver_with(
                            reservation,
                            &Err::<source::RetainedStage, _>(error),
                            |_| &Value::Null,
                        ),
                        Err(error) => tools::failure(error.code(), error.message(), maximum),
                    },
                }
            }
            Self::Probe(r) => {
                match Kernel::new().execute(command::ProbeRenderedSite {
                    publication: r.publication.into_command()?,
                    rules_json: r.mode.rules_json,
                    max_rules_bytes: r.mode.max_rules_bytes,
                    scan_limits: r.mode.scan_limits.native(),
                }) {
                    Ok(report) => tools::encode(&report, false, maximum),
                    Err(error) => tools::failure(error.code(), error.message(), maximum),
                }
            }
        }
    }
}

fn invalid() -> ErrorData {
    ErrorData::invalid_params(
        "Invalid rendered-site request: named publication/options, independent create permission, absolute parent and bounded directory/response capacity required before staging",
        None,
    )
}

#[cfg(test)]
#[path = "../tests/internal/rendered_site.rs"]
mod tests;
