//! Gives editors Gazette's diagnostics, symbols and navigation over the Language Server
//! Protocol.
//!
//! This crate builds on `gazette-sdk`; `gazette-launcher` builds it into the `gazette`
//! executable. Complete supplied buffers or an explicitly permitted, confined native
//! selection enter the same bounded workspace and content compiler used by the other
//! interfaces. The server owns framing, protocol lifecycle and UTF-16 coordinates.
//! Selected baselines, versions and current analysis belong to the SDK. No URI is
//! treated as implicit permission, and no build or publication effect is performed.

mod code_actions;
mod completion;
mod context;
mod diagnostics;
mod documents;
mod hover;
mod navigation;
mod origin;
mod page_package;
mod positions;
mod scheduling;
mod server;
mod snapshot;
mod symbols;
mod transport;

use std::{fmt, io};

/// Independent transport, editor-state and compiler ceilings.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Aggregate header bytes per incoming frame, including CRLF separators.
    pub header_bytes: usize,
    /// Complete incoming JSON bytes, checked before allocating its body or decoding.
    pub message_bytes: usize,
    /// Complete outgoing JSON bytes, checked before writing any frame bytes.
    pub output_bytes: usize,
    /// Complete selected source count, including the workspace declaration.
    pub documents: usize,
    /// Aggregate bytes of selected URI identifiers.
    pub uri_bytes: usize,
    /// Aggregate retained UTF-8 baseline and unsaved overlay bytes.
    pub source_bytes: usize,
    /// Complete diagnostic count per document; overflow is not truncated success.
    pub diagnostics: usize,
    /// Shared SDK source-byte, node and nesting limits for each complete buffer.
    pub syntax: gazette_sdk::toml::Limits,
    /// Shared content/heading analysis ceilings; `syntax` owns workspace parsing.
    pub authoring: gazette_sdk::authoring::Limits,
}

impl Default for Limits {
    fn default() -> Self {
        let session = gazette_sdk::authoring::session::Limits::default();
        Self {
            header_bytes: 8192,
            message_bytes: 1 << 20,
            output_bytes: 2 << 20,
            documents: session.documents,
            uri_bytes: session.identifier_bytes,
            source_bytes: session.source_bytes,
            diagnostics: 256,
            syntax: gazette_sdk::toml::Limits::default(),
            authoring: session.analysis,
        }
    }
}

/// A failed protocol session. Messages omit the rejected editor payload.
#[derive(Debug)]
pub enum Error {
    /// Native stream read/write failure.
    Io(io::Error),
    /// No complete operation fits the named resource ceiling.
    Limit(&'static str),
    /// Malformed framing/message or invalid terminal lifecycle.
    Protocol(&'static str),
    /// An internal typed protocol projection could not be serialized.
    Encoding(serde_json::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "LSP stream: {error}"),
            Self::Limit(resource) => write!(formatter, "LSP {resource} ceiling exceeded"),
            Self::Protocol(reason) => write!(formatter, "LSP protocol: {reason}"),
            Self::Encoding(_) => formatter.write_str("LSP response encoding failed"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Encoding(error) => Some(error),
            Self::Limit(_) | Self::Protocol(_) => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Serve the selected-source LSP on process standard input/output.
///
/// Only protocol frames are written to stdout. The calling CLI reports terminal
/// errors on stderr. One bounded process-local worker serializes selected-set
/// analysis and protocol queries. Native reads occur only through the explicit
/// initialization mode and shared SDK capture boundary.
///
/// # Errors
/// Propagates [`serve`] errors. Clean termination requires shutdown followed by exit.
pub fn run() -> Result<(), Error> {
    use std::{io::Write, os::fd::AsFd};
    let mut stdout = io::stdout().lock();
    stdout.flush()?;
    // StdoutRaw suppresses EBADF. Retain its lock but write through the cloned
    // native handle so a lost response cannot be reported as delivered.
    let output = std::fs::File::from(stdout.as_fd().try_clone_to_owned()?);
    serve(io::stdin().lock(), output, Limits::default())
}

/// Run the same scheduled protocol over caller-owned streams.
///
/// Initialization supplies one workspace and a complete selected source baseline.
/// In supplied-buffer mode URI identifiers never become native paths; native mode
/// separately binds one standard file root and explicit logical paths. Versioned overlays replace only
/// selected buffers. Closing restores the captured or explicitly acknowledged saved baseline, never an implicit disk read.
/// A newer buffer replaces failed analysis; malformed/stale protocol cannot
/// silently replace an accepted version.
///
/// # Errors
/// Rejects malformed/oversized frames, output/diagnostic ceilings, stream failures,
/// premature EOF and exit without shutdown. No partial frame is emitted on an
/// encoding/size failure; native I/O failures may still interrupt a frame write.
pub fn serve(
    mut reader: impl io::BufRead,
    mut writer: impl io::Write + Send,
    limits: Limits,
) -> Result<(), Error> {
    use std::sync::mpsc;

    let (sender, receiver) = mpsc::sync_channel(scheduling::PENDING_MESSAGES);
    std::thread::scope(|scope| {
        let worker = scope.spawn(move || server::serve(&receiver, &mut writer, limits));
        loop {
            let input = match transport::read(&mut reader, limits) {
                Ok(Some(message)) => {
                    let terminal = matches!(
                        &message,
                        lsp_server::Message::Notification(notification)
                            if notification.method == "exit"
                    );
                    if sender.send(scheduling::Input::Message(message)).is_err() {
                        break;
                    }
                    if terminal {
                        break;
                    }
                    continue;
                }
                Ok(None) => scheduling::Input::Eof,
                Err(error) => scheduling::Input::ReadError(error),
            };
            let _ = sender.send(input);
            break;
        }
        drop(sender);
        worker
            .join()
            .map_err(|_| Error::Protocol("LSP analysis worker panicked"))?
    })
}
