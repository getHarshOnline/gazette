//! Bounded ingress before JSON decoding, with observable carrier failures.

use std::{
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll},
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// First carrier failure both survives provider closure and wakes the service.
pub(super) struct Failure {
    failed: AtomicBool,
    sender: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}

impl Failure {
    pub(super) fn new() -> (Arc<Self>, tokio::sync::oneshot::Receiver<()>) {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        (
            Arc::new(Self {
                failed: AtomicBool::new(false),
                sender: Mutex::new(Some(sender)),
            }),
            receiver,
        )
    }

    pub(super) fn observed(&self) -> bool {
        self.failed.load(Ordering::Relaxed)
    }

    fn record(&self) {
        self.failed.store(true, Ordering::Relaxed);
        if let Some(sender) = self
            .sender
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
        {
            let _ = sender.send(());
        }
    }
}

pub(super) struct BoundedLines<R> {
    inner: R,
    bytes: usize,
    maximum: usize,
    rejected: bool,
    failed: Arc<Failure>,
}

impl<R> BoundedLines<R> {
    pub(super) fn new(inner: R, maximum: usize, failed: Arc<Failure>) -> Self {
        Self {
            inner,
            bytes: 0,
            maximum,
            rejected: false,
            failed,
        }
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for BoundedLines<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        output: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        if self.rejected {
            return Poll::Ready(Err(std::io::Error::other("MCP frame ceiling")));
        }
        if output.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        let mut bytes = [0_u8; 8192];
        let capacity = output.remaining().min(bytes.len());
        let mut buffer = ReadBuf::new(&mut bytes[..capacity]);
        match Pin::new(&mut self.inner).poll_read(context, &mut buffer) {
            Poll::Ready(Ok(())) => {
                let mut accepted = 0;
                for byte in buffer.filled() {
                    if *byte == b'\n' {
                        self.bytes = 0;
                    } else {
                        self.bytes = self.bytes.saturating_add(1);
                        if self.bytes > self.maximum {
                            self.failed.record();
                            self.rejected = true;
                            break;
                        }
                    }
                    accepted += 1;
                }
                if accepted > 0 {
                    output.put_slice(&buffer.filled()[..accepted]);
                    Poll::Ready(Ok(()))
                } else if self.rejected {
                    Poll::Ready(Err(std::io::Error::other("MCP frame ceiling")))
                } else {
                    Poll::Ready(Ok(()))
                }
            }
            Poll::Ready(Err(error)) => {
                self.failed.record();
                Poll::Ready(Err(error))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

pub(super) struct ObservedWriter<W> {
    inner: W,
    failed: Arc<Failure>,
}

impl<W> ObservedWriter<W> {
    pub(super) fn new(inner: W, failed: Arc<Failure>) -> Self {
        Self { inner, failed }
    }

    fn observe<T>(&self, result: Poll<std::io::Result<T>>) -> Poll<std::io::Result<T>> {
        if matches!(&result, Poll::Ready(Err(_))) {
            self.failed.record();
        }
        result
    }
}

impl<W: AsyncWrite + Unpin> AsyncWrite for ObservedWriter<W> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let result = Pin::new(&mut self.inner).poll_write(context, bytes);
        if matches!(result, Poll::Ready(Ok(0))) && !bytes.is_empty() {
            return self.observe(Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::WriteZero,
                "MCP carrier accepted no bytes",
            ))));
        }
        self.observe(result)
    }
    fn poll_flush(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        let result = Pin::new(&mut self.inner).poll_flush(context);
        self.observe(result)
    }
    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        let result = Pin::new(&mut self.inner).poll_shutdown(context);
        self.observe(result)
    }
}

#[cfg(test)]
#[path = "../tests/internal/transport.rs"]
mod tests;
