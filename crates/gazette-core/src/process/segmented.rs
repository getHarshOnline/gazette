//! Borrowed finite input segments, without assembling a second publication-sized buffer.

use std::time::Duration;

use super::{Command, Error, Failure, Options};

/// Retained source ceilings permit 512 MiB of file bytes and 64 MiB of metadata,
/// plus one eight-byte frame per entry and a small protocol prefix. This is a
/// separate transport contract; ordinary buffered requests retain their 64 MiB cap.
pub const SEGMENTED_INPUT_BYTES: usize = (576 << 20) + 10_000 * 8 + 64;

/// One supervised native invocation over immutable, already retained segments.
/// Segments are delivered in order followed by EOF; none are read from paths.
/// At most 20,003 segments of at most 64 MiB each are supported. Only borrowed
/// descriptors and the selected bounded stdout/stderr are allocated by the runner.
pub struct SegmentedCaptureRequest<'a> {
    /// Exact workspace-selected command and executable pin.
    pub command: &'a Command,
    /// Complete ordered input, including any caller-owned framing.
    pub segments: &'a [&'a [u8]],
    /// Explicit native execution consent.
    pub allow_execute: bool,
    /// Complete aggregate input ceiling, at most `SEGMENTED_INPUT_BYTES`.
    pub max_input_bytes: usize,
    /// Shared input/capture/child deadline after spawn, from 1 ms to 5 minutes.
    pub timeout: Duration,
    /// Selected stdout ceiling, at most 64 MiB. None discards stdout.
    pub max_output_bytes: Option<usize>,
    /// Selected stderr ceiling, at most 64 MiB. None discards stderr.
    pub max_stderr_bytes: Option<usize>,
}

impl SegmentedCaptureRequest<'_> {
    /// Check permission, command, deadline and every aggregate bound before spawn.
    ///
    /// # Errors
    /// Refuses unbounded input, segment/count overflow and unsupported capture limits.
    pub fn validate(&self) -> Result<(), Error> {
        Options {
            allow_execute: self.allow_execute,
            max_input_bytes: 0,
            timeout: self.timeout,
        }
        .validate(self.command, &[])?;
        if self.max_input_bytes > SEGMENTED_INPUT_BYTES || self.segments.len() > 20_003 {
            return Err(Error::before_start(Failure::Limit));
        }
        let mut total = 0usize;
        for segment in self.segments {
            total = total
                .checked_add(segment.len())
                .filter(|&n| n <= self.max_input_bytes && segment.len() <= 64 << 20)
                .ok_or_else(|| Error::before_start(Failure::Limit))?;
        }
        if self.max_output_bytes.is_some_and(|n| n > 64 << 20)
            || self.max_stderr_bytes.is_some_and(|n| n > 64 << 20)
        {
            return Err(Error::before_start(Failure::OutputLimit));
        }
        Ok(())
    }
}
