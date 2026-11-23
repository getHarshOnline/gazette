//! Named protocol choices over the shared publication recipe and native preview bounds.

use gazette_cli::sdk::{
    preview::{self, runner, session},
    production_preview,
    source::Sha256Digest,
    workspace::Key,
};
use rmcp::model::{ErrorData, JsonObject};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
use std::{collections::BTreeMap, net::SocketAddr, time::Duration};

use crate::publication;
use gazette_cli::command::publication as publication_command;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Start {
    /// Same complete private recipe, read acknowledgement and source/render ceilings as `render_sites`.
    #[schemars(schema_with = "publication::input_schema")]
    pub(super) publication: publication::Input,
    /// Complete selected site set, not permission to omit a failed member.
    pub(super) listen: Vec<Listener>,
    /// Required independently of `publication.local` and `publication.allow_read`.
    allow_listeners: bool,
    /// Use addresses from the actual held sockets for cross-site output URLs.
    #[serde(default)]
    local_urls: bool,
    /// Pinned capture or explicitly selected development working-tree capture.
    mode: Mode,
    #[serde(default = "max_sites")]
    #[schemars(range(max = usize::MAX))]
    max_sites: usize,
    #[serde(default = "max_header_bytes")]
    #[schemars(range(max = usize::MAX))]
    max_header_bytes: usize,
    #[serde(default = "max_headers")]
    #[schemars(range(max = usize::MAX))]
    max_headers: usize,
    /// Total request-header deadline, not renewed by incoming bytes.
    #[serde(default = "request_timeout_ms")]
    #[schemars(range(max = u64::MAX))]
    request_timeout_ms: u64,
    /// Total response-write deadline.
    #[serde(default = "response_timeout_ms")]
    #[schemars(range(max = u64::MAX))]
    response_timeout_ms: u64,
    /// Bounded native observation history, separate from MCP response capacity.
    #[serde(default)]
    pub(super) history: session::Limits,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Production {
    /// Same complete private recipe, read acknowledgement and source/render ceilings as `render_sites`.
    #[schemars(schema_with = "publication::input_schema")]
    pub(super) publication: publication::Input,
    /// Complete selected site set, not permission to omit a failed member.
    pub(super) listen: Vec<Listener>,
    /// Registry reconciliation is explicitly unavailable in this same-model first slice.
    registry: production_preview::RegistryMode,
    /// Only a fresh in-memory render is accepted.
    artifact: production_preview::ArtifactMode,
    /// Required independently of `publication.local` and `publication.allow_read`.
    allow_listeners: bool,
    #[serde(default = "max_sites")]
    #[schemars(range(max = usize::MAX))]
    max_sites: usize,
    #[serde(default = "max_header_bytes")]
    #[schemars(range(max = usize::MAX))]
    max_header_bytes: usize,
    #[serde(default = "max_headers")]
    #[schemars(range(max = usize::MAX))]
    max_headers: usize,
    #[serde(default = "request_timeout_ms")]
    #[schemars(range(max = u64::MAX))]
    request_timeout_ms: u64,
    #[serde(default = "response_timeout_ms")]
    #[schemars(range(max = u64::MAX))]
    response_timeout_ms: u64,
    /// Bounded native observation history, separate from MCP response capacity.
    #[serde(default)]
    pub(super) history: session::Limits,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Listener {
    site: Key,
    address: SocketAddr,
}

#[derive(Clone, Copy, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Mode {
    Pinned {},
    Development {
        /// Native watch interval; never selects a different workspace or source binding.
        #[schemars(range(max = u64::MAX))]
        interval_ms: u64,
    },
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Handle {
    #[schemars(regex(pattern = "^[0-9a-f]{64}$"))]
    pub(super) session: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Poll {
    #[schemars(regex(pattern = "^[0-9a-f]{64}$"))]
    pub(super) session: String,
    /// Last observation accepted by the caller; zero requests the retained beginning.
    #[schemars(range(max = u64::MAX))]
    pub(super) after: u64,
    /// Maximum records to return; no HTTP serving or rebuild is driven by this call.
    #[schemars(range(max = usize::MAX))]
    pub(super) maximum: usize,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Stop {
    #[schemars(regex(pattern = "^[0-9a-f]{64}$"))]
    pub(super) session: String,
    /// Remove only an actually finished worker. While stopping, retain its handle.
    #[serde(default)]
    pub(super) release_handle: bool,
}

pub(crate) enum Input {
    Plan(Box<Start>),
    ProductionPlan(Box<Production>),
    Start {
        request: Box<Start>,
        digest: Sha256Digest,
    },
    ProductionStart {
        request: Box<Production>,
        digest: Sha256Digest,
    },
    Inspect(Handle),
    Poll(Poll),
    Stop(Stop),
}

pub(super) struct ProductionParts {
    pub(super) publication: publication_command::Render,
    pub(super) registry: production_preview::RegistryMode,
    pub(super) artifact: production_preview::ArtifactMode,
    pub(super) addresses: BTreeMap<Key, SocketAddr>,
    pub(super) options: production_preview::Options,
    pub(super) history: session::Limits,
}

fn max_sites() -> usize {
    preview::Options::default().max_sites
}
fn max_header_bytes() -> usize {
    preview::Options::default().max_header_bytes
}
fn max_headers() -> usize {
    preview::Options::default().max_headers
}
fn request_timeout_ms() -> u64 {
    preview::Options::default()
        .request_timeout
        .as_millis()
        .try_into()
        .expect("native deadline")
}
fn response_timeout_ms() -> u64 {
    preview::Options::default()
        .response_timeout
        .as_millis()
        .try_into()
        .expect("native deadline")
}

pub(super) fn decode(route: super::Route, arguments: JsonObject) -> Result<Input, ErrorData> {
    let value = Value::Object(arguments);
    match route {
        super::Route::Plan
        | super::Route::Start
        | super::Route::ProductionPlan
        | super::Route::ProductionStart => {
            publication::validate_shape(value.get("publication").ok_or_else(invalid)?)?;
            for name in ["mode", "history"] {
                if value.get(name).is_some_and(|v| !v.is_object()) {
                    return Err(invalid());
                }
            }
            if !value
                .get("listen")
                .and_then(Value::as_array)
                .is_some_and(|items| items.iter().all(Value::is_object))
            {
                return Err(invalid());
            }
            let mut bytes = gazette_cli::sdk::render::LimitedWriter::new(Vec::new(), 1 << 20);
            serde_json::to_writer(&mut bytes, &value).map_err(|_| invalid())?;
            let digest = Sha256Digest::of(&bytes.into_inner());
            if matches!(
                route,
                super::Route::ProductionPlan | super::Route::ProductionStart
            ) {
                let request: Production = serde_json::from_value(value).map_err(|_| invalid())?;
                if matches!(route, super::Route::ProductionPlan) {
                    Ok(Input::ProductionPlan(Box::new(request)))
                } else {
                    Ok(Input::ProductionStart {
                        request: Box::new(request),
                        digest,
                    })
                }
            } else {
                let request: Start = serde_json::from_value(value).map_err(|_| invalid())?;
                if matches!(route, super::Route::Plan) {
                    Ok(Input::Plan(Box::new(request)))
                } else {
                    Ok(Input::Start {
                        request: Box::new(request),
                        digest,
                    })
                }
            }
        }
        super::Route::Inspect => {
            let request: Handle = serde_json::from_value(value).map_err(|_| invalid())?;
            handle(&request.session)?;
            Ok(Input::Inspect(request))
        }
        super::Route::Poll => {
            let request: Poll = serde_json::from_value(value).map_err(|_| invalid())?;
            handle(&request.session)?;
            Ok(Input::Poll(request))
        }
        super::Route::Stop => {
            let request: Stop = serde_json::from_value(value).map_err(|_| invalid())?;
            handle(&request.session)?;
            Ok(Input::Stop(request))
        }
    }
}

fn handle(value: &str) -> Result<(), ErrorData> {
    if value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        Ok(())
    } else {
        Err(invalid())
    }
}

fn invalid() -> ErrorData {
    ErrorData::invalid_params(
        "Invalid preview selection; use complete named objects and explicit permissions; no operation started",
        None,
    )
}

impl Start {
    pub(super) fn into_command_parts(
        self,
    ) -> Result<
        (
            publication_command::Render,
            BTreeMap<Key, SocketAddr>,
            runner::Options,
            session::Limits,
        ),
        ErrorData,
    > {
        let addresses = self.addresses()?;
        let publication = self.publication.into_command()?;
        let options = runner::Options {
            mode: match self.mode {
                Mode::Pinned {} => runner::Mode::Pinned,
                Mode::Development { interval_ms } => runner::Mode::Development {
                    interval: Duration::from_millis(interval_ms),
                },
            },
            local_urls: self.local_urls,
            listeners: preview::Options {
                allow_listeners: self.allow_listeners,
                max_sites: self.max_sites,
                max_header_bytes: self.max_header_bytes,
                max_headers: self.max_headers,
                request_timeout: Duration::from_millis(self.request_timeout_ms),
                response_timeout: Duration::from_millis(self.response_timeout_ms),
            },
            syntax: publication.syntax,
            acquisition: publication.acquisition,
            output: publication.output,
            render: publication.limits,
        };
        Ok((publication, addresses, options, self.history))
    }

    pub(super) fn addresses(&self) -> Result<BTreeMap<Key, SocketAddr>, ErrorData> {
        let mut addresses = BTreeMap::new();
        for binding in &self.listen {
            if addresses
                .insert(binding.site.clone(), binding.address)
                .is_some()
            {
                return Err(invalid());
            }
        }
        Ok(addresses)
    }
}

impl Production {
    pub(super) fn into_command_parts(self) -> Result<ProductionParts, ErrorData> {
        let addresses = self.addresses()?;
        let publication = self.publication.into_command()?;
        let options = production_preview::Options {
            listeners: preview::Options {
                allow_listeners: self.allow_listeners,
                max_sites: self.max_sites,
                max_header_bytes: self.max_header_bytes,
                max_headers: self.max_headers,
                request_timeout: Duration::from_millis(self.request_timeout_ms),
                response_timeout: Duration::from_millis(self.response_timeout_ms),
            },
            syntax: publication.syntax,
            acquisition: publication.acquisition,
            output: publication.output,
            render: publication.limits,
        };
        Ok(ProductionParts {
            publication,
            registry: self.registry,
            artifact: self.artifact,
            addresses,
            options,
            history: self.history,
        })
    }

    fn addresses(&self) -> Result<BTreeMap<Key, SocketAddr>, ErrorData> {
        let mut addresses = BTreeMap::new();
        for binding in &self.listen {
            if addresses
                .insert(binding.site.clone(), binding.address)
                .is_some()
            {
                return Err(invalid());
            }
        }
        Ok(addresses)
    }
}
