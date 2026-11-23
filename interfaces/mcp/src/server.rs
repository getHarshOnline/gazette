//! Bounded protocol admission and dispatch; all configuration meaning stays in SDK.

use std::{borrow::Cow, sync::Arc};

use gazette_cli::{
    command::{Kernel, metadata},
    sdk::source::Sha256Digest,
};
use rmcp::model::{
    CacheScope, CallToolRequestParams, CallToolResponse, ErrorData, Implementation,
    ListToolsResult, MetaObject, PaginatedRequestParams, RequestMetaObject, ServerCapabilities,
    ServerInfo, Tool,
};
use rmcp::service::RequestContext;
use rmcp::{RoleServer, ServerHandler};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::{
    authoring, preview, protocol, recovery_session, release_session, store_session, tools,
};

const TOOL_PAGE_SIZE: usize = 8;
const CURSOR_VERSION: &str = "g2";
const DISCOVERY_SELECTION: &str = "io.rtepl.gazette/discovery";
const DISPATCH_DISPOSITION: &str = "io.rtepl.gazette/dispatch";

#[derive(Clone, Copy)]
enum DispatchDisposition {
    NotDispatched,
    OutcomeUnknown,
}

impl DispatchDisposition {
    const fn key(self) -> &'static str {
        match self {
            Self::NotDispatched => "not-dispatched",
            Self::OutcomeUnknown => "outcome-unknown",
        }
    }
}

fn classify(mut error: ErrorData, disposition: DispatchDisposition) -> ErrorData {
    let value = serde_json::Value::String(disposition.key().to_owned());
    let data = match error.data.take() {
        Some(serde_json::Value::Object(mut data)) => {
            let prior = data.insert(DISPATCH_DISPOSITION.to_owned(), value.clone());
            assert!(
                prior.is_none_or(|prior| prior == value),
                "MCP dispatch disposition must have one owner"
            );
            serde_json::Value::Object(data)
        }
        Some(detail) => {
            let mut data = serde_json::Map::new();
            data.insert(DISPATCH_DISPOSITION.to_owned(), value.clone());
            data.insert("detail".to_owned(), detail);
            serde_json::Value::Object(data)
        }
        None => {
            let mut data = serde_json::Map::new();
            data.insert(DISPATCH_DISPOSITION.to_owned(), value);
            serde_json::Value::Object(data)
        }
    };
    error.data = Some(data);
    error
}

fn not_dispatched(error: ErrorData) -> ErrorData {
    classify(error, DispatchDisposition::NotDispatched)
}

fn outcome_unknown(error: ErrorData) -> ErrorData {
    classify(error, DispatchDisposition::OutcomeUnknown)
}

struct Catalog {
    tools: Vec<Tool>,
    digest: Sha256Digest,
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct DiscoverySelection(u16);

impl DiscoverySelection {
    fn all() -> Self {
        Self(metadata::operations().iter().fold(0, |mask, operation| {
            mask | (1 << operation.capability.discovery_rank())
        }))
    }

    fn contains(self, capability: metadata::Capability) -> bool {
        self.0 & (1 << capability.discovery_rank()) != 0
    }

    fn capabilities(self) -> Vec<&'static str> {
        metadata::operations()
            .iter()
            .map(|operation| operation.capability)
            .filter(|capability| self.contains(*capability))
            .map(metadata::Capability::key)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect()
    }
}

#[derive(Clone, Copy)]
struct CatalogError;

fn invalid_cursor() -> ErrorData {
    ErrorData::invalid_params("Invalid or expired Gazette tool cursor", None)
}

fn build_catalog() -> Result<Catalog, CatalogError> {
    let tools = tools::Route::all()
        .into_iter()
        .map(|route| route.descriptor())
        .collect::<Vec<_>>();
    let bytes = serde_json::to_vec(&tools).map_err(|_| CatalogError)?;
    Ok(Catalog {
        tools,
        digest: Sha256Digest::of(&bytes),
    })
}

fn discovery_selection(meta: &RequestMetaObject) -> Result<Option<DiscoverySelection>, ErrorData> {
    let Some(value) = meta.get(DISCOVERY_SELECTION) else {
        return Ok(None);
    };
    let object = value.as_object().ok_or_else(invalid_discovery_selection)?;
    if object.len() != 1 || !object.contains_key("capabilities") {
        return Err(invalid_discovery_selection());
    }
    let capabilities = object["capabilities"]
        .as_array()
        .ok_or_else(invalid_discovery_selection)?;
    if capabilities.is_empty()
        || capabilities.len() > DiscoverySelection::all().capabilities().len()
    {
        return Err(invalid_discovery_selection());
    }
    let mut mask = 0_u16;
    for capability in capabilities {
        let key = capability
            .as_str()
            .ok_or_else(invalid_discovery_selection)?;
        let capability = metadata::operations()
            .iter()
            .map(|operation| operation.capability)
            .find(|capability| capability.key() == key)
            .ok_or_else(invalid_discovery_selection)?;
        let bit = 1 << capability.discovery_rank();
        if mask & bit != 0 {
            return Err(invalid_discovery_selection());
        }
        mask |= bit;
    }
    Ok(Some(DiscoverySelection(mask)))
}

fn invalid_discovery_selection() -> ErrorData {
    ErrorData::invalid_params("Invalid Gazette tool discovery capability selection", None)
}

fn page_cursor(digest: Sha256Digest, selection: DiscoverySelection, offset: usize) -> String {
    format!(
        "{CURSOR_VERSION}.{digest}.{:03x}.{offset:016x}",
        selection.0
    )
}

fn page_position(
    cursor: Option<&str>,
    requested: Option<DiscoverySelection>,
    digest: Sha256Digest,
    catalog: &Catalog,
) -> Result<(DiscoverySelection, usize), ErrorData> {
    let Some(cursor) = cursor else {
        return Ok((requested.unwrap_or_else(DiscoverySelection::all), 0));
    };
    let mut fields = cursor.split('.');
    let version = fields.next().ok_or_else(invalid_cursor)?;
    let digest_field = fields.next().ok_or_else(invalid_cursor)?;
    let selection_field = fields.next().ok_or_else(invalid_cursor)?;
    let offset_field = fields.next().ok_or_else(invalid_cursor)?;
    if fields.next().is_some()
        || version != CURSOR_VERSION
        || digest_field.len() != 64
        || selection_field.len() != 3
        || offset_field.len() != 16
    {
        return Err(invalid_cursor());
    }
    let supplied_digest = digest_field
        .parse::<Sha256Digest>()
        .map_err(|_| invalid_cursor())?;
    let selection =
        DiscoverySelection(u16::from_str_radix(selection_field, 16).map_err(|_| invalid_cursor())?);
    let offset_u64 = u64::from_str_radix(offset_field, 16).map_err(|_| invalid_cursor())?;
    let offset = usize::try_from(offset_u64).map_err(|_| invalid_cursor())?;
    let catalog_length = catalog
        .tools
        .iter()
        .filter(|tool| {
            selection.contains(
                metadata::operation(tool.name.as_ref())
                    .expect("catalog tools have canonical metadata")
                    .capability,
            )
        })
        .count();
    if supplied_digest != digest
        || selection.0 == 0
        || selection.0 & !DiscoverySelection::all().0 != 0
        || requested.is_some_and(|requested| requested != selection)
        || offset == 0
        || offset >= catalog_length
        || !offset.is_multiple_of(TOOL_PAGE_SIZE)
        || page_cursor(supplied_digest, selection, offset) != cursor
    {
        return Err(invalid_cursor());
    }
    Ok((selection, offset))
}

#[derive(Clone, Copy)]
pub(super) struct Limits {
    pub(super) frame_bytes: usize,
    pub(super) output_bytes: usize,
    pub(super) workers: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            frame_bytes: 1024 * 1024,
            output_bytes: 256 * 1024,
            workers: 2,
        }
    }
}

#[derive(Clone)]
pub(super) struct Server {
    permits: Arc<Semaphore>,
    limits: Limits,
    authoring: Arc<authoring::Sessions>,
    release: Arc<release_session::Sessions>,
    recovery: Arc<recovery_session::Sessions>,
    store: Arc<store_session::Sessions>,
    preview: Arc<preview::Sessions>,
    catalog: Arc<Result<Catalog, CatalogError>>,
}

impl Server {
    pub(super) fn new(limits: Limits) -> Self {
        let kernel = Arc::new(Kernel::new());
        Self {
            permits: Arc::new(Semaphore::new(limits.workers)),
            limits,
            authoring: Arc::new(authoring::Sessions::new(Arc::clone(&kernel))),
            release: Arc::new(release_session::Sessions::default()),
            recovery: Arc::new(recovery_session::Sessions::default()),
            store: Arc::new(store_session::Sessions::default()),
            preview: Arc::new(preview::Sessions::new(kernel)),
            catalog: Arc::new(build_catalog()),
        }
    }

    fn reserve(&self) -> Result<OwnedSemaphorePermit, ErrorData> {
        Arc::clone(&self.permits).try_acquire_owned().map_err(|_| {
            ErrorData::internal_error(
                "Gazette MCP is busy; retry the bounded request",
                Some(serde_json::json!({"code":"mcp_busy","retryable":true})),
            )
        })
    }
}

impl ServerHandler for Server {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_protocol_version(protocol::VERSION)
            .with_server_info(Implementation::new("gazette", env!("CARGO_PKG_VERSION")))
            .with_instructions("Use Gazette's tools to check, preview, build and release a publication. Reach for them when you need what a publication declares, whether its sources and links resolve, or what a build, preview or release produced, rather than inferring it from files. Discover named operations through tools/list. A client may put io.rtepl.gazette/discovery with an exact capabilities array in request _meta to page only the relevant workspace, authoring, build, preview, documentation, store, release, observation or recovery neighborhoods; omitting it returns the complete catalog. Intent, effect, prominence, relation, closure and terminal-result metadata guide selection and closure only; exact schemas, annotations, permissions and returned evidence remain authoritative. Supplied-source tools perform no native I/O. inspect_source, inspect_toml and load_workspace read explicitly selected absolute local roots and require caller/host permission; annotations are not authorization. load_workspace reads only its exact root-relative workspace document, declared non-recursive includes and necessary ancestor directories; it performs no cwd or ancestor discovery. Authoring sessions retain only explicitly supplied baselines and overlays, expire after five minutes, and require exact generations for changes. Coordinates are UTF-8 bytes, not UTF-16 editor positions. Each call returns typed results with their evidence; if a call returns a diagnostic, report its file, line and identity and fix the source rather than retrying it unchanged. No build, signing or release permission is implied: whether a site is published stays with its publisher.")
    }

    fn supported_protocol_versions(&self) -> Cow<'static, [rmcp::model::ProtocolVersion]> {
        Cow::Borrowed(&protocol::VERSIONS)
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        tools::Route::named(name).map(tools::Route::descriptor)
    }

    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let catalog = self.catalog.as_ref().as_ref().map_err(|_| {
            ErrorData::internal_error("Gazette tool catalog serialization failed", None)
        })?;
        let requested = discovery_selection(&context.meta)?;
        // Broad hosts such as Codex materialize only the first tools/list result and
        // keep their own bounded search index. The unqualified standard request must
        // therefore expose the complete callable surface in that one result. Clients
        // that explicitly select capability neighborhoods opt into Gazette's smaller
        // paginated surface below.
        let complete = requested.is_none()
            && request
                .as_ref()
                .and_then(|request| request.cursor.as_deref())
                .is_none();
        let (selection, offset) = page_position(
            request
                .as_ref()
                .and_then(|request| request.cursor.as_deref()),
            requested,
            catalog.digest,
            catalog,
        )?;
        let selected = catalog
            .tools
            .iter()
            .filter(|tool| {
                selection.contains(
                    metadata::operation(tool.name.as_ref())
                        .expect("catalog tools have canonical metadata")
                        .capability,
                )
            })
            .collect::<Vec<_>>();
        let end = if complete {
            selected.len()
        } else {
            offset.saturating_add(TOOL_PAGE_SIZE).min(selected.len())
        };
        let mut result = ListToolsResult::with_all_items(
            selected[offset..end]
                .iter()
                .map(|tool| (*tool).clone())
                .collect(),
        )
        .with_ttl_ms(0)
        .with_cache_scope(CacheScope::Private);
        let mut meta = MetaObject::new();
        meta.0.insert(
            DISCOVERY_SELECTION.to_owned(),
            serde_json::json!({
                "capabilities": selection.capabilities(),
                "tools": selected.len(),
            }),
        );
        result.meta = Some(meta);
        result.next_cursor = (!complete && end < selected.len())
            .then(|| page_cursor(catalog.digest, selection, end));
        Ok(result)
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        if request.input_responses.is_some() || request.request_state.is_some() {
            return Err(not_dispatched(ErrorData::invalid_params(
                "Gazette tools do not use resumable request state",
                None,
            )));
        }
        let route = tools::Route::named(&request.name).ok_or_else(|| {
            not_dispatched(ErrorData::invalid_params("Unknown Gazette tool", None))
        })?;
        // Reserve before decoding source/query payloads. There is no pending work queue.
        let permit = self.reserve().map_err(not_dispatched)?;
        let mut input = route
            .decode(request.arguments.unwrap_or_default())
            .map_err(not_dispatched)?;
        if let tools::Input::Configuration(crate::configuration::Input::Check {
            workspace: None,
            client,
        }) = &mut input
        {
            *client = client_folder(&context).await.map_err(not_dispatched)?;
        }
        let output_bytes = self.limits.output_bytes;
        let authoring = Arc::clone(&self.authoring);
        let release = Arc::clone(&self.release);
        let recovery = Arc::clone(&self.recovery);
        let store = Arc::clone(&self.store);
        let preview = Arc::clone(&self.preview);
        let result = tokio::task::spawn_blocking(move || {
            // Dropping the caller does not stop admitted work; retain its slot
            // until both the native computation and bounded result encoding finish.
            let _permit = permit;
            input
                .execute(
                    &authoring,
                    &release,
                    &recovery,
                    &store,
                    &preview,
                    output_bytes,
                )
                .map(Into::into)
        })
        .await
        .map_err(|_| {
            outcome_unknown(ErrorData::internal_error("Gazette MCP worker failed", None))
        })?;
        result.map_err(outcome_unknown)
    }
}

#[allow(deprecated)] // Older MCP hosts still provide their opened folder through roots/list.
async fn client_folder(
    context: &RequestContext<RoleServer>,
) -> Result<Option<std::path::PathBuf>, ErrorData> {
    if context
        .peer
        .peer_info()
        .is_none_or(|info| info.capabilities.roots.is_none())
    {
        return Ok(None);
    }
    let roots = tokio::time::timeout(std::time::Duration::from_secs(5), context.peer.list_roots())
        .await
        .map_err(|_| ErrorData::internal_error("The client did not return its workspace folders. Pass workspace to check_workspace.", None))?
        .map_err(|error| ErrorData::internal_error(format!("Could not read the client's workspace folders: {error}"), None))?;
    roots.roots.first().map(|root| {
        url::Url::parse(&root.uri).ok().and_then(|uri| uri.to_file_path().ok())
            .ok_or_else(|| ErrorData::invalid_params("The client workspace must be a local file URI. Pass workspace to select a local folder.", None))
    }).transpose()
}

#[cfg(test)]
#[path = "../tests/internal/server.rs"]
mod tests;
