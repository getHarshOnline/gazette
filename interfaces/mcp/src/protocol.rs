//! Exact MCP lifecycle revisions admitted by the Gazette server.

use std::borrow::Cow;

use rmcp::model::{
    ClientNotification, ClientRequest, ErrorData, ProtocolVersion, ServerInfo, ServerResult,
};
use rmcp::service::{NotificationContext, RequestContext};
use rmcp::{RoleServer, Service};

/// Stateless requests retain the current self-describing lifecycle.
pub(super) const VERSION: ProtocolVersion = ProtocolVersion::V_2026_07_28;

/// Initialized sessions admit the two standard pre-stateless revisions in active use.
///
/// The `rmcp` service owns lifecycle negotiation and request validation. Gazette
/// narrows its advertised set here instead of implementing a parallel handshake.
pub(super) static VERSIONS: [ProtocolVersion; 3] = [
    ProtocolVersion::V_2025_06_18,
    ProtocolVersion::V_2025_11_25,
    VERSION,
];

/// Preserve stateless inline negotiation while admitting the standard initialized lifecycle.
pub(super) struct Lifecycle<S>(S);

impl<S> Lifecycle<S> {
    pub(super) fn new(service: S) -> Self {
        Self(service)
    }
}

impl<S: Service<RoleServer>> Service<RoleServer> for Lifecycle<S> {
    async fn handle_request(
        &self,
        request: ClientRequest,
        context: RequestContext<RoleServer>,
    ) -> Result<ServerResult, ErrorData> {
        if !matches!(request, ClientRequest::InitializeRequest(_)) {
            if let Some(inline) = context.meta.protocol_version() {
                if !context.meta.missing_required_keys(&VERSION).is_empty() {
                    return Err(ErrorData::invalid_params(
                        "Required protocolVersion/clientCapabilities metadata is missing or malformed",
                        None,
                    ));
                }
                if inline != VERSION {
                    return Err(ErrorData::unsupported_protocol_version(
                        inline,
                        std::slice::from_ref(&VERSION),
                    ));
                }
            } else {
                let Some(negotiated) = context.protocol_version() else {
                    return Err(ErrorData::invalid_params(
                        "Initialize the MCP session or supply stateless request metadata",
                        None,
                    ));
                };
                if !VERSIONS.contains(&negotiated) {
                    return Err(ErrorData::unsupported_protocol_version(
                        negotiated, &VERSIONS,
                    ));
                }
            }
        }
        self.0.handle_request(request, context).await
    }

    async fn handle_notification(
        &self,
        notification: ClientNotification,
        context: NotificationContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        self.0.handle_notification(notification, context).await
    }

    fn get_info(&self) -> ServerInfo {
        self.0.get_info()
    }

    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Borrowed(&VERSIONS)
    }
}
