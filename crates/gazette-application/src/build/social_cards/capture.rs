//! One explicit native capture, validated before the same semantic owner resumes.

use super::{
    SocialCardGeneration, SocialCardGenerationOptions,
    request::{RequestLimits, SocialCardCaptureRequest},
};
use crate::build::{PublicationRender, PublicationRenderError, SiteRenderError};
use base64::Engine;
use gazette_compiler::workspace::Key;
use gazette_core::{
    process, raster,
    source::{Content, InventoryBuilder, Sha256Digest, TreeIdentity, TreeInput},
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt};

/// Private evidence of the exact request, retained helper and native image response.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct SocialCardCaptureEvidence {
    /// Exact consuming publication.
    pub site: Key,
    /// Card model, HTML, media and selected runtime request identity.
    pub request_sha256: Sha256Digest,
    /// Complete helper and payload tree executed by the retained process port.
    pub helper_input: TreeIdentity,
    /// Selected Node executable bytes, independently checked by the process port.
    pub node: process::ExecutablePin,
    /// Selected browser bytes, independently checked by the fixed helper.
    pub browser: process::ExecutablePin,
    /// Complete helper response identity.
    pub response_sha256: Sha256Digest,
    /// Title returned by the fixed helper after authored H1 selection.
    pub title: String,
    /// Native decoded image facts bound to the exact output bytes.
    pub image: raster::Inspection,
    /// Complete successful process exit after retained-tree verification.
    pub exit: process::Exit,
}

/// Capture failed without a successful card; native failures retain child-custody evidence.
#[derive(Debug)]
pub enum SocialCardGenerationError {
    /// An exact request/capture or response contract did not match.
    Binding(&'static str),
    /// A configured aggregate or per-capture bound was exceeded.
    Limit(&'static str),
    /// The selected workspace omitted pinned native capture programs.
    Unconfigured,
    /// Native invocation failed, including permission or custody checks.
    Native(process::Error),
    /// The complete helper exited unsuccessfully; only its bounded diagnostic code is shown.
    Exit {
        /// Actual process status.
        exit: process::Exit,
        /// Fixed helper code, excluding arbitrary process output or private payloads.
        reason: Option<String>,
    },
    /// Retained payload/helper tree construction failed.
    Source(gazette_core::source::Error),
    /// Native full-image decoding or its evidence failed.
    Raster(raster::Error),
    /// Bounded template rendering or serialization failed.
    Render(gazette_core::render::Error),
}
impl SocialCardGenerationError {
    /// Stable diagnostic code without private request payloads.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Binding(_) => "social_card_binding",
            Self::Limit(_) => "social_card_limit",
            Self::Unconfigured => "social_card_unconfigured",
            Self::Native(error) => error.code(),
            Self::Exit { .. } => "social_card_exit",
            Self::Source(_) => "social_card_source",
            Self::Raster(_) => "social_card_png",
            Self::Render(error) => error.code(),
        }
    }
}
impl fmt::Display for SocialCardGenerationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Binding(reason) => write!(f, "social card binding failed: {reason}"),
            Self::Limit(reason) => write!(f, "social card exceeds {reason}"),
            Self::Unconfigured => {
                f.write_str("social card requires explicit pinned Node and browser selection")
            }
            Self::Native(error) => error.fmt(f),
            Self::Exit { exit, reason } => {
                write!(f, "social card capture exited {:?}", exit.code)?;
                if let Some(reason) = reason {
                    write!(f, ": {reason}")?;
                }
                f.write_str("; no retry")
            }
            Self::Source(error) => error.fmt(f),
            Self::Raster(error) => error.fmt(f),
            Self::Render(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for SocialCardGenerationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Native(e) => Some(e),
            Self::Source(e) => Some(e),
            Self::Raster(e) => Some(e),
            Self::Render(e) => Some(e),
            _ => None,
        }
    }
}

pub(crate) struct Capture {
    pub bytes: Vec<u8>,
    pub evidence: SocialCardCaptureEvidence,
}

pub(crate) struct Budget {
    remaining: SocialCardGenerationOptions,
}
impl Budget {
    pub(crate) fn new(options: SocialCardGenerationOptions) -> Self {
        Self { remaining: options }
    }
    pub(crate) fn limits(&self) -> RequestLimits {
        RequestLimits {
            bytes: if self.remaining.cards == 0 {
                0
            } else {
                self.remaining
                    .request_bytes
                    .min(self.remaining.execution.max_input_bytes)
            },
            assets: self.remaining.assets_bytes,
            entries: 10_000,
            total: u64::MAX,
        }
    }
    fn reserve(
        &mut self,
        request: &SocialCardCaptureRequest,
    ) -> Result<(), SocialCardGenerationError> {
        self.remaining.cards = self
            .remaining
            .cards
            .checked_sub(1)
            .ok_or(SocialCardGenerationError::Limit("capture count"))?;
        self.remaining.assets_bytes = self
            .remaining
            .assets_bytes
            .checked_sub(request.payload.inventory().total_bytes())
            .ok_or(SocialCardGenerationError::Limit("aggregate assets"))?;
        Ok(())
    }
}

pub(crate) fn resume<'s>(
    generation: Option<&SocialCardGeneration<'_>>,
    budget: &mut Budget,
    raster: &raster::Inspect<'_>,
    mut render: impl FnMut(
        Option<&Capture>,
        RequestLimits,
    ) -> Result<PublicationRender<'s>, PublicationRenderError>,
) -> Result<PublicationRender<'s>, PublicationRenderError> {
    let request_limits = budget.limits();
    let first = render(None, request_limits);
    let Some(generation) = generation else {
        if let Err(PublicationRenderError::Site(SiteRenderError::SocialCardCaptureRequired(
            request,
        ))) = &first
        {
            budget.reserve(request).map_err(|error| {
                PublicationRenderError::Site(SiteRenderError::SocialCard(Box::new(error)))
            })?;
            budget.remaining.request_bytes = budget
                .remaining
                .request_bytes
                .saturating_sub(request.request_bytes);
        }
        return first;
    };
    let request = match first {
        Err(PublicationRenderError::Site(SiteRenderError::SocialCardCaptureRequired(request))) => {
            request
        }
        result => return result,
    };
    let capture = capture(&request, generation, budget, raster).map_err(|error| {
        PublicationRenderError::Site(SiteRenderError::SocialCard(Box::new(error)))
    })?;
    match render(Some(&capture), request_limits) {
        Err(PublicationRenderError::Site(SiteRenderError::SocialCardCaptureRequired(_))) => Err(
            PublicationRenderError::Site(SiteRenderError::SocialCard(Box::new(
                SocialCardGenerationError::Binding("resumption requested another capture"),
            ))),
        ),
        result => result,
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Response {
    version: u8,
    request_sha256: Sha256Digest,
    title: String,
    png_base64: Option<String>,
    jpeg_base64: Option<String>,
}

fn capture(
    request: &SocialCardCaptureRequest,
    generation: &SocialCardGeneration<'_>,
    budget: &mut Budget,
    raster: &raster::Inspect<'_>,
) -> Result<Capture, SocialCardGenerationError> {
    use SocialCardGenerationError as Error;
    let helper_timeout = generation
        .options
        .execution
        .timeout
        .checked_sub(std::time::Duration::from_secs(3))
        .filter(|timeout| *timeout >= std::time::Duration::from_millis(1))
        .ok_or(Error::Limit("deadline leaves no browser cleanup margin"))?;
    budget.reserve(request)?;
    let helper = generation.helper;
    if helper.inventory().total_bytes() > 4 << 20
        || helper.inventory().entries().len() > 128
        || helper.file(generation.entrypoint).is_none()
    {
        return Err(Error::Binding("fixed helper tree"));
    }
    let total = helper
        .inventory()
        .total_bytes()
        .checked_add(request.payload.inventory().total_bytes())
        .ok_or(Error::Limit("capture tree"))?;
    let mut builder = InventoryBuilder::new(gazette_core::source::Limits {
        entries: helper.inventory().entries().len() + request.payload.inventory().entries().len(),
        depth: 64,
        file_bytes: 256 << 20,
        total_bytes: total,
    });
    let mut files = BTreeMap::new();
    for tree in [helper, &request.payload] {
        for entry in tree.inventory().entries() {
            let content = match entry.content {
                Content::Directory => Content::Directory,
                Content::File { bytes, sha256 } => {
                    files.insert(
                        entry.path.clone(),
                        tree.file(&entry.path)
                            .ok_or(Error::Binding("capture file"))?,
                    );
                    Content::File { bytes, sha256 }
                }
            };
            builder
                .insert(entry.path.clone(), content)
                .map_err(Error::Source)?;
        }
    }
    let inventory = builder.finish();
    let tree = TreeInput::new(&inventory, files).map_err(Error::Source)?;
    let output_limit = budget.remaining.output_bytes.min(64 << 20);
    let value = serde_json::json!({
        "version":1,"request_sha256":request.identity,"html":request.html,"initial_title":request.title,"document_url":request.document_url,
        "width":request.recipe.width,"height":request.recipe.height,"media_selector":request.recipe.media_selector,"media_kind":request.recipe.media_kind,
        "browser":{"program":request.renderer.browser.program,"bytes":request.renderer.browser.executable.bytes,"sha256":request.renderer.browser.executable.sha256},
        "limits":{"timeout_ms":helper_timeout.as_millis(),"max_output_bytes":output_limit},"assets":request.assets,"image_variants":request.image_variants,
    });
    let mut value = value;
    if let Some(time) = request.recipe.video_time_ms {
        value["video_time_ms"] = time.into();
    }
    if let Some(quality) = request.renderer.jpeg_quality {
        value["jpeg_quality"] = quality.into();
    }
    let input = crate::build::serialization::encode(
        &value,
        budget
            .remaining
            .request_bytes
            .min(generation.options.execution.max_input_bytes)
            .min(64 << 20),
        "social card request bytes",
    )
    .map_err(Error::Render)?;
    budget.remaining.request_bytes = budget
        .remaining
        .request_bytes
        .checked_sub(input.len())
        .ok_or(Error::Limit("aggregate request bytes"))?;
    let command = process::Command {
        program: request.renderer.node.program.clone(),
        executable: Some(request.renderer.node.executable),
        arguments: vec![],
        directory: "/".into(),
        environment: BTreeMap::new(),
    };
    let capture = process::CaptureRequest {
        request: process::Request {
            command: &command,
            input: &input,
            options: generation.options.execution,
        },
        max_output_bytes: output_limit,
        max_stderr_bytes: Some(generation.options.diagnostic_bytes),
    };
    capture.validate().map_err(Error::Native)?;
    let captured = generation
        .runner
        .capture_retained(capture, tree, generation.entrypoint)
        .map_err(Error::Native)?;
    if captured.stdout.len() > output_limit
        || captured
            .stderr
            .as_ref()
            .is_some_and(|bytes| bytes.len() > generation.options.diagnostic_bytes)
    {
        return Err(Error::Limit("capture streams"));
    }
    if captured.exit.code != Some(0) {
        let reason = captured
            .stderr
            .as_deref()
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .and_then(|text| text.strip_prefix("social-card: "))
            .and_then(|text| text.strip_suffix('\n'))
            .filter(|text| {
                !text.is_empty()
                    && text.len() <= 80
                    && text
                        .bytes()
                        .all(|byte| byte.is_ascii_lowercase() || byte == b'-')
            })
            .map(str::to_owned);
        return Err(Error::Exit {
            exit: captured.exit,
            reason,
        });
    }
    budget.remaining.output_bytes = budget
        .remaining
        .output_bytes
        .checked_sub(captured.stdout.len())
        .ok_or(Error::Limit("aggregate response bytes"))?;
    if captured.stdout.is_empty() {
        return Err(Error::Binding("empty capture response"));
    }
    let response: Response = serde_json::from_slice(&captured.stdout).map_err(|error| {
        Error::Binding(match error.classify() {
            serde_json::error::Category::Eof => "incomplete capture response",
            serde_json::error::Category::Data => "capture response fields",
            _ => "capture response JSON",
        })
    })?;
    if response.version != 1
        || response.request_sha256 != request.identity
        || response.title.trim().is_empty()
        || response.title.len() > 1024
        || response.title.contains('\0')
    {
        return Err(Error::Binding("capture response identity or title"));
    }
    let maximum = budget
        .remaining
        .output_bytes
        .min(generation.options.raster.encoded_bytes);
    let (format, encoded) = match (
        request.renderer.jpeg_quality,
        response.png_base64,
        response.jpeg_base64,
    ) {
        (None, Some(encoded), None) => (raster::Format::Png, encoded),
        (Some(_), None, Some(encoded)) => (raster::Format::Jpeg, encoded),
        _ => return Err(Error::Binding("capture response image format")),
    };
    if encoded.len() / 4 * 3 > maximum.saturating_add(2) {
        return Err(Error::Limit("image bytes"));
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&encoded)
        .map_err(|_| Error::Binding("image base64"))?;
    budget.remaining.output_bytes = budget
        .remaining
        .output_bytes
        .checked_sub(bytes.len())
        .ok_or(Error::Limit("aggregate image bytes"))?;
    let limits = raster::Limits {
        encoded_bytes: maximum,
        width: request.recipe.width.min(generation.options.raster.width),
        height: request.recipe.height.min(generation.options.raster.height),
        ..generation.options.raster
    };
    let image = raster(format, &bytes, limits).map_err(Error::Raster)?;
    image
        .validate(format, &bytes, limits)
        .map_err(Error::Raster)?;
    if image.width != request.recipe.width || image.height != request.recipe.height {
        return Err(Error::Binding("image dimensions"));
    }
    Ok(Capture {
        bytes,
        evidence: SocialCardCaptureEvidence {
            site: request.site.clone(),
            request_sha256: request.identity,
            helper_input: inventory.identity(),
            node: request.renderer.node.executable,
            browser: request.renderer.browser.executable,
            response_sha256: Sha256Digest::of(&captured.stdout),
            title: response.title,
            image,
            exit: captured.exit,
        },
    })
}
