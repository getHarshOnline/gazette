//! Explicit social-card capture between checked semantic selection and complete site output.

use gazette_core::{
    process, raster,
    source::{CapturedSource, EntryPath},
};
use gazette_ports::process::RetainedCaptureRunner;

/// Explicit permission and whole-operation capture bounds, separate from site output limits.
#[derive(Clone, Copy, Debug)]
pub struct SocialCardGenerationOptions {
    /// Native process permission, input bound and deadline for each capture.
    pub execution: process::Options,
    /// Maximum number of browser captures across the selected sites.
    pub cards: usize,
    /// Aggregate serialized capture requests, including rendered card HTML.
    pub request_bytes: usize,
    /// Aggregate retained media/program/style/font bytes supplied to capture.
    pub assets_bytes: u64,
    /// Aggregate capture response bytes and decoded PNG bytes.
    pub output_bytes: usize,
    /// Complete diagnostic stream bound for each invocation.
    pub diagnostic_bytes: usize,
    /// Native PNG decoding bounds; dimensions must also equal the selected recipe.
    pub raster: raster::Limits,
}

impl Default for SocialCardGenerationOptions {
    fn default() -> Self {
        Self {
            execution: process::Options::default(),
            cards: 128,
            request_bytes: 64 << 20,
            assets_bytes: 1 << 30,
            output_bytes: 64 << 20,
            diagnostic_bytes: 64 << 10,
            raster: raster::Limits::default(),
        }
    }
}

/// Caller-supplied native port and engine-owned retained helper, never workspace script selection.
#[derive(Clone, Copy)]
pub struct SocialCardGeneration<'a> {
    /// Selected retained-resource process implementation.
    pub runner: &'a dyn RetainedCaptureRunner,
    /// Exact embedded helper tree, supplied by the native adapter composition.
    pub helper: &'a CapturedSource,
    /// Exact fixed helper entry within that tree.
    pub entrypoint: &'a EntryPath,
    /// Explicit effect permission and aggregate bounds.
    pub options: SocialCardGenerationOptions,
}

mod capture;
mod request;
pub(super) use capture::{Budget, Capture, resume};
pub use capture::{SocialCardCaptureEvidence, SocialCardGenerationError};
pub use request::SocialCardCaptureRequest;
pub(super) use request::{AssetInput, RequestLimits, prepare, static_media};

/// Public facts for one generated card; executable and source evidence stay private.
#[derive(Clone, Debug, serde::Serialize, schemars::JsonSchema)]
pub struct SocialCardProjection {
    /// Serving URL resolved beneath this publication's selected public or preview base.
    pub url: String,
    /// Actual native-decoded output width.
    pub width: u32,
    /// Actual native-decoded output height.
    pub height: u32,
    /// Exact generated representation.
    pub media_type: &'static str,
    /// Captured title after the fixed helper's authored-heading selection.
    pub title: String,
    /// Exact generated PNG bytes.
    pub sha256: gazette_core::source::Sha256Digest,
}
