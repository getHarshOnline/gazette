//! Explicit browser capture composed around the same pure publication renderer.

use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

/// Invocation permission and bounds for selected Theme share-card recipes.
/// Executables remain pinned workspace inputs; no caller script or browser flags are accepted.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct SocialCardGeneration {
    /// Permit the selected native interpreter and browser to run.
    pub allow_execute: bool,
    /// Maximum bytes of each complete request.
    #[schemars(range(min = 1, max = 67108864))]
    pub max_input_bytes: usize,
    /// Per-capture native deadline, including bounded cleanup.
    #[schemars(range(min = 3001, max = 300000))]
    pub timeout_ms: u64,
    /// Maximum complete captures across the selected sites.
    #[schemars(range(min = 1, max = 128))]
    pub cards: usize,
    /// Aggregate serialized request ceiling.
    #[schemars(range(min = 1, max = 67108864))]
    pub request_bytes: usize,
    /// Aggregate retained media and executable resource ceiling.
    #[schemars(range(min = 1, max = 1073741824))]
    pub assets_bytes: u64,
    /// Aggregate response and decoded PNG ceiling.
    #[schemars(range(min = 1, max = 67108864))]
    pub output_bytes: usize,
}

impl Default for SocialCardGeneration {
    fn default() -> Self {
        Self {
            allow_execute: false,
            max_input_bytes: 16 << 20,
            timeout_ms: 30_000,
            cards: 128,
            request_bytes: 64 << 20,
            assets_bytes: 1 << 30,
            output_bytes: 64 << 20,
        }
    }
}

impl SocialCardGeneration {
    /// Workspace configuration selects the renderer, never execution permission.
    pub fn resolve(
        workspace: &crate::workspace::Workspace,
        generation: Option<Self>,
        allow_execute: bool,
    ) -> Option<Self> {
        if generation.is_none() && workspace.social_card_renderer().is_none() {
            return None;
        }
        let mut selected = generation.unwrap_or_default();
        selected.allow_execute |= allow_execute;
        Some(selected)
    }

    pub(super) fn validate(self) -> Result<(), &'static str> {
        if !(1..=64 << 20).contains(&self.max_input_bytes)
            || !(3001..=300_000).contains(&self.timeout_ms)
            || !(1..=128).contains(&self.cards)
            || !(1..=64 << 20).contains(&self.request_bytes)
            || !(1..=1 << 30).contains(&self.assets_bytes)
            || !(1..=64 << 20).contains(&self.output_bytes)
        {
            return Err(
                "social-card generation requires bounded requests, assets, outputs and native execution",
            );
        }
        Ok(())
    }

    /// Native options retain the caller's permission independently from provider selection.
    pub fn options(self) -> super::SocialCardGenerationOptions {
        super::SocialCardGenerationOptions {
            execution: gazette_core::process::Options {
                allow_execute: self.allow_execute,
                max_input_bytes: self.max_input_bytes,
                timeout: Duration::from_millis(self.timeout_ms),
            },
            cards: self.cards,
            request_bytes: self.request_bytes,
            assets_bytes: self.assets_bytes,
            output_bytes: self.output_bytes,
            ..Default::default()
        }
    }
}

/// Construct only the embedded engine helper; this performs no filesystem reads or effects.
pub(crate) fn helper()
-> Result<(crate::source::CapturedSource, crate::source::EntryPath), crate::source::Error> {
    use crate::source::{CapturedSource, Content, EntryPath, InventoryBuilder, Sha256Digest};
    let mut builder = InventoryBuilder::new(crate::source::Limits {
        entries: 32,
        file_bytes: 128 << 10,
        total_bytes: 512 << 10,
        ..Default::default()
    });
    let mut directories = BTreeSet::new();
    let mut files = BTreeMap::new();
    for (name, bytes) in gazette_adapter_process::social_card::resources() {
        let path: EntryPath = name.parse()?;
        for (offset, _) in name.match_indices('/') {
            let directory: EntryPath = name[..offset].parse()?;
            if directories.insert(directory.clone()) {
                builder.insert(directory, Content::Directory)?;
            }
        }
        builder.insert(
            path.clone(),
            Content::File {
                bytes: bytes.len() as u64,
                sha256: Sha256Digest::of(bytes),
            },
        )?;
        files.insert(path, bytes.to_vec());
    }
    Ok((
        CapturedSource::from_parts(builder.finish(), files)?,
        "social-card.mjs".parse()?,
    ))
}

/// Render with explicit social-card execution after semantic selection.
/// Omission keeps the pure renderer and its capture-required refusal unchanged.
///
/// # Errors
/// Preserves selection, rendering, bounded process and capture failures without retry.
pub fn render_publications_with_generations<'a>(
    inputs: &'a super::AcquiredPublicationSet,
    options: super::PublicationSetRenderOptions<'_>,
    limits: super::PublicationSetRenderLimits,
    generation: Option<super::SocialCardGenerationOptions>,
) -> Result<super::RenderedPublicationSet<'a>, super::PublicationSetError> {
    let Some(generation) = generation else {
        return super::render_publications(inputs, options, limits);
    };
    let (helper, entrypoint) = helper().map_err(|_| {
        super::PublicationSetError::Selection("embedded social-card helper is invalid")
    })?;
    let resources = gazette_application::build::ResourceCache::new(
        super::resources::Compiler,
        Default::default(),
    );
    inputs.render_local_with_generations(
        &gazette_adapter_tera::Compiler,
        &resources,
        &crate::raster::inspect,
        options,
        limits,
        &gazette_adapter_markdown::Comrak,
        Some(&gazette_integration_github::HostFiles),
        gazette_application::build::SocialCardGeneration {
            runner: &gazette_adapter_process::Runner,
            helper: &helper,
            entrypoint: &entrypoint,
            options: generation,
        },
    )
}

/// Check the candidate once, then capture and render within that same admitted selection.
///
/// # Errors
/// A failed capture never repeats a checker or produces a successful partial site.
pub fn check_and_render_publications_with_generations<'a>(
    inputs: &'a super::AcquiredPublicationSet,
    options: super::PublicationSetRenderOptions<'_>,
    limits: super::PublicationSetRenderLimits,
    policy: super::CandidateCheckOptions<'_>,
    generation: Option<super::SocialCardGenerationOptions>,
) -> Result<super::RenderedPublicationSet<'a>, super::PublicationSetError> {
    let Some(generation) = generation else {
        return super::check_and_render_publications(inputs, options, limits, policy);
    };
    let (helper, entrypoint) = helper().map_err(|_| {
        super::PublicationSetError::Selection("embedded social-card helper is invalid")
    })?;
    let resources = gazette_application::build::ResourceCache::new(
        super::resources::Compiler,
        Default::default(),
    );
    inputs.check_and_render_local_with_generations(
        &gazette_adapter_tera::Compiler,
        &resources,
        &crate::raster::inspect,
        options,
        limits,
        &gazette_adapter_markdown::Comrak,
        &gazette_adapter_process::Runner,
        policy,
        Some(&gazette_integration_github::HostFiles),
        gazette_application::build::SocialCardGeneration {
            runner: &gazette_adapter_process::Runner,
            helper: &helper,
            entrypoint: &entrypoint,
            options: generation,
        },
    )
}
