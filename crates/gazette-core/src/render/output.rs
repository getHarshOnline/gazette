//! Shared emitted-byte ceiling, checked before forwarding each complete write request.

use std::io::{self, Write};

/// Limit accepted bytes without truncating an oversized write request.
///
/// This bounds forwarded output, not producer CPU or intermediate allocations.
/// Native partial writes and errors retain their normal `Write` semantics. An
/// oversized request is rejected before touching the inner writer and permanently
/// marks this operation as exceeded, even if later smaller writes succeed.
pub struct LimitedWriter<W> {
    inner: W,
    used: usize,
    maximum: usize,
    exceeded: bool,
}

impl<W> LimitedWriter<W> {
    /// Start a fresh byte allowance around the supplied writer.
    pub fn new(inner: W, maximum: usize) -> Self {
        Self {
            inner,
            used: 0,
            maximum,
            exceeded: false,
        }
    }

    /// Whether any write request exceeded the remaining allowance.
    ///
    /// Native I/O errors do not set this flag. Successful later operations cannot
    /// clear it; consumers must check it before reporting successful generation.
    pub fn exceeded(&self) -> bool {
        self.exceeded
    }

    /// Recover the writer, including any partial output; this does not imply success.
    ///
    /// Consumers must first check both the producer result and `exceeded()`.
    pub fn into_inner(self) -> W {
        self.inner
    }

    /// Borrow the writer so far, for example to record a fragment's byte offsets.
    pub fn get_ref(&self) -> &W {
        &self.inner
    }
}

impl<W: Write> Write for LimitedWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.maximum.saturating_sub(self.used) {
            self.exceeded = true;
            return Err(io::Error::other("rendering byte ceiling exceeded"));
        }
        let written = self.inner.write(bytes)?;
        self.used += written;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
