//! Gives agents Gazette's checks, builds, previews and releases as MCP tools they can
//! discover and call.
//!
//! This crate builds on `gazette-cli`, whose typed commands it projects over the shared
//! SDK; `gazette-launcher` builds it into the `gazette` executable. Supplied-source
//! tools do not open declared paths. Explicit local-read tools call SDK-owned confined
//! source operations; discovery identifies their effects. The transport owns neither a
//! compiler nor publication authority. Authoring handles retain supplied buffers only;
//! their semantic and version state belongs to the same SDK owner as the LSP.

use std::sync::Arc;

use tokio::io::{AsyncRead, AsyncWrite};

mod artifact;
mod authoring;
mod commit_write;
mod composition;
mod configuration;
mod coordinator;
mod delivery;
mod documentation;
mod documentation_check;
mod documentation_materialization;
mod github;
mod inspect;
mod journal_commit;
mod limits;
mod materialization;
mod observation;
mod preparation;
mod preparation_history;
mod preparation_inspection;
mod preview;
mod probe_output;
mod protocol;
mod publication;
mod publication_check;
mod qualification;
mod readback;
mod recovery_session;
mod release;
mod release_session;
mod rendered_site;
mod server;
mod site_documents;
mod source;
mod stage;
mod stage_check;
mod storage;
mod store_lifecycle;
mod store_session;
mod tools;
mod transport;

pub use limits::{TransportLimits, TransportLimitsError};

/// Transport startup or termination failure; not a native-effect receipt.
pub type Error = Box<dyn std::error::Error + Send + Sync>;

/// Serve the discoverable operation catalog on the process's standard streams.
///
/// # Errors
/// Returns runtime, transport or service failure. A successful return does not
/// certify that a caller received or accepted a particular tool response.
pub fn run() -> Result<(), Error> {
    run_with_limits(TransportLimits::default())
}

/// Serve standard streams with explicitly validated carrier budgets.
/// Native permissions, worker count and session lifetimes are unchanged.
///
/// # Errors
/// Returns the same startup/termination failures as [`run`].
pub fn run_with_limits(limits: TransportLimits) -> Result<(), Error> {
    // Use exact owned OS handles: standard-stream wrappers can translate a bad
    // descriptor into apparent EOF/success. Tokio's file carrier keeps blocking
    // I/O off the async reactor without that standard-stream special case.
    let (input, output) = standard_streams()?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()?;
    let result = runtime.block_on(serve_with_transport_limits(
        tokio::fs::File::from_std(input),
        tokio::fs::File::from_std(output),
        limits,
    ));
    // A failed output can leave a blocking input read waiting on an open pipe.
    // This process launcher must return its failure without waiting indefinitely
    // for that producer. Timeout is not proof the read or SDK work was cancelled;
    // embedded callers supply their own carrier/runtime through `serve` instead.
    runtime.shutdown_timeout(std::time::Duration::from_millis(100));
    result
}

fn standard_streams() -> std::io::Result<(std::fs::File, std::fs::File)> {
    use std::io::Write;
    std::io::stdout().lock().flush()?;
    #[cfg(unix)]
    {
        use std::os::fd::AsFd;
        Ok((
            std::io::stdin().as_fd().try_clone_to_owned()?.into(),
            std::io::stdout().as_fd().try_clone_to_owned()?.into(),
        ))
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsHandle;
        Ok((
            std::io::stdin().as_handle().try_clone_to_owned()?.into(),
            std::io::stdout().as_handle().try_clone_to_owned()?.into(),
        ))
    }
}

/// Serve MCP over an explicitly supplied byte carrier; startup opens no native sources.
///
/// This entry point also permits protocol clients to exercise the production
/// service in memory. Current stateless requests and the admitted initialized
/// session revisions share one server, catalog and application dispatch.
/// Blocking SDK work is bounded and cannot be undone by caller cancellation.
///
/// # Errors
/// Returns an error on observed carrier failure, a frame ceiling, or failed
/// service tasks. EOF is ordinary termination, not proof of response delivery.
pub async fn serve<R, W>(reader: R, writer: W) -> Result<(), Error>
where
    R: AsyncRead + Send + Unpin + 'static,
    W: AsyncWrite + Send + Unpin + 'static,
{
    serve_with_transport_limits(reader, writer, TransportLimits::default()).await
}

/// Serve an explicit carrier using finite caller-selected byte budgets.
///
/// Only ingress and native tool-result encoding ceilings change. This does not
/// broaden source selection, native analysis, permissions or session custody.
///
/// # Errors
/// Returns the same observed carrier/service failures as [`serve`].
pub async fn serve_with_transport_limits<R, W>(
    reader: R,
    writer: W,
    limits: TransportLimits,
) -> Result<(), Error>
where
    R: AsyncRead + Send + Unpin + 'static,
    W: AsyncWrite + Send + Unpin + 'static,
{
    serve_with_limits(
        reader,
        writer,
        server::Limits {
            frame_bytes: limits.max_frame_bytes(),
            output_bytes: limits.max_response_bytes(),
            ..server::Limits::default()
        },
    )
    .await
}

async fn serve_with_limits<R, W>(reader: R, writer: W, limits: server::Limits) -> Result<(), Error>
where
    R: AsyncRead + Send + Unpin + 'static,
    W: AsyncWrite + Send + Unpin + 'static,
{
    let (failed, failure) = transport::Failure::new();
    let input = transport::BoundedLines::new(reader, limits.frame_bytes, Arc::clone(&failed));
    let output = transport::ObservedWriter::new(writer, Arc::clone(&failed));
    let service = rmcp::service::serve_directly::<rmcp::RoleServer, _, _, _, _>(
        protocol::Lifecycle::new(server::Server::new(limits)),
        (input, output),
        None,
    );
    let cancellation = service.cancellation_token();
    let monitor = tokio::spawn(async move {
        if failure.await.is_ok() {
            cancellation.cancel();
        }
    });
    let result = service.waiting().await;
    monitor.abort();
    let reason = result?;
    if failed.observed() {
        return Err(
            std::io::Error::other("MCP carrier failed or exceeded its frame ceiling").into(),
        );
    }
    match reason {
        rmcp::service::QuitReason::Closed => Ok(()),
        other => Err(std::io::Error::other(format!("MCP service stopped: {other:?}")).into()),
    }
}

#[cfg(test)]
#[path = "../tests/internal/protocol.rs"]
mod tests;

#[cfg(test)]
use gazette_test_support as native;

#[cfg(test)]
#[path = "../tests/internal/configuration.rs"]
mod configuration_tests;
