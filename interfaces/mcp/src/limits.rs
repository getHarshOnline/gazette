//! Explicit finite carrier budgets, independent of native analysis permissions.

/// Validated per-frame ingress and tool-result encoding ceilings.
///
/// Raising these ceilings grants no source access or native effect permission,
/// and does not change workers, session lifetimes or SDK analysis limits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransportLimits {
    frame_bytes: usize,
    response_bytes: usize,
}

/// A requested carrier ceiling is outside the supported finite range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportLimitsError {
    /// Frame bytes must be between 4 KiB and 16 MiB, inclusive.
    FrameBytes,
    /// Tool-result bytes must be between 1 KiB and 16 MiB, inclusive.
    ResponseBytes,
}

impl std::fmt::Display for TransportLimitsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::FrameBytes => "MCP max-frame-bytes must be between 4096 and 16777216",
            Self::ResponseBytes => "MCP max-response-bytes must be between 1024 and 16777216",
        })
    }
}
impl std::error::Error for TransportLimitsError {}

impl TransportLimits {
    /// Validate explicit byte ceilings before carrier startup or any tool dispatch.
    ///
    /// # Errors
    /// Refuses zero, undersized and unbounded/oversized budgets.
    pub fn new(
        max_frame_bytes: usize,
        max_response_bytes: usize,
    ) -> Result<Self, TransportLimitsError> {
        if !(4096..=16 * 1024 * 1024).contains(&max_frame_bytes) {
            return Err(TransportLimitsError::FrameBytes);
        }
        if !(1024..=16 * 1024 * 1024).contains(&max_response_bytes) {
            return Err(TransportLimitsError::ResponseBytes);
        }
        Ok(Self {
            frame_bytes: max_frame_bytes,
            response_bytes: max_response_bytes,
        })
    }

    /// Maximum bytes before a frame's newline; no resynchronization after refusal.
    #[must_use]
    pub fn max_frame_bytes(self) -> usize {
        self.frame_bytes
    }

    /// Maximum encoded native tool-result bytes, not a whole catalog/wire-frame bound.
    #[must_use]
    pub fn max_response_bytes(self) -> usize {
        self.response_bytes
    }
}
impl Default for TransportLimits {
    fn default() -> Self {
        let native = crate::server::Limits::default();
        Self {
            frame_bytes: native.frame_bytes,
            response_bytes: native.output_bytes,
        }
    }
}
