//! Bounded request identity over one pre-card model and its already resolved assets.

use super::{
    SocialCardGenerationOptions,
    capture::{Capture, SocialCardGenerationError as Error},
};
use gazette_compiler::{
    theme::SocialCardRecipe,
    workspace::{Key, SocialCardRenderer},
};
use gazette_core::source::{CapturedSource, Content, EntryPath, InventoryBuilder, Sha256Digest};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy)]
pub(crate) struct RequestLimits {
    pub bytes: usize,
    pub assets: u64,
    pub entries: usize,
    pub total: u64,
}
impl Default for RequestLimits {
    fn default() -> Self {
        let options = SocialCardGenerationOptions::default();
        Self {
            bytes: options.request_bytes,
            assets: options.assets_bytes,
            entries: 10_000,
            total: u64::MAX,
        }
    }
}

pub(crate) struct AssetInput<'a> {
    pub url: &'a str,
    pub bytes: &'a [u8],
    pub media_type: &'static str,
    pub role: &'static str,
}

#[derive(Debug, Serialize)]
pub(super) struct Asset {
    pub url: String,
    pub entry: EntryPath,
    pub media_type: &'static str,
    pub role: &'static str,
    pub bytes: u64,
    pub sha256: Sha256Digest,
}

/// Only the native image binding and its declared alternatives enter capture.
#[derive(Debug, Deserialize, Serialize)]
pub(super) struct Image {
    address: String,
    destination: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    variants: Option<ImageVariants>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ImageVariants {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    light: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dark: Option<String>,
}

/// One owned, bounded capture request. It contains private rendered data and retained bytes.
/// A pure renderer returns this requirement without invoking a native program.
#[derive(Debug)]
pub struct SocialCardCaptureRequest {
    pub(super) site: Key,
    pub(super) identity: Sha256Digest,
    pub(super) renderer: SocialCardRenderer,
    pub(super) html: String,
    pub(super) title: String,
    pub(super) document_url: String,
    pub(super) recipe: SocialCardRecipe,
    pub(super) assets: Vec<Asset>,
    pub(super) image_variants: Vec<Image>,
    pub(super) payload: CapturedSource,
    pub(super) request_bytes: usize,
}
impl SocialCardCaptureRequest {
    /// Exact publication home whose selected model requires capture.
    pub fn site(&self) -> &Key {
        &self.site
    }
    /// Identity of card HTML, pre-card model, media settings, pins and asset closure.
    pub fn identity(&self) -> Sha256Digest {
        self.identity
    }
    /// Retained request text/metadata and payload bytes, for whole-set accounting.
    pub fn retained_bytes(&self) -> u64 {
        self.payload
            .inventory()
            .total_bytes()
            .saturating_add(self.request_bytes as u64)
    }
    /// Retained file/directory inventory, for whole-set accounting.
    pub fn retained_entries(&self) -> usize {
        self.payload.inventory().entries().len()
    }
}

#[derive(Serialize)]
struct Identity<'a> {
    site: &'a Key,
    model: Sha256Digest,
    html: &'a str,
    initial_title: &'a str,
    document_url: &'a str,
    recipe: &'a SocialCardRecipe,
    renderer: &'a SocialCardRenderer,
    assets: &'a [Asset],
    image_variants: &'a [Image],
}

#[expect(
    clippy::too_many_arguments,
    reason = "Exact resolved model, capture binding and independent byte ceilings"
)]
pub(crate) fn prepare(
    theme: &crate::build::CompiledTheme,
    site: &Key,
    model: &mut Value,
    recipe: &SocialCardRecipe,
    renderer: &SocialCardRenderer,
    document_url: &str,
    initial_title: &str,
    assets: &[AssetInput<'_>],
    limits: RequestLimits,
    captured: Option<&Capture>,
    resources: &crate::build::resources::CompiledResources,
) -> Result<Option<SocialCardCaptureRequest>, Error> {
    if initial_title.is_empty() || initial_title.len() > 1024 || document_url.len() > 8192 {
        return Err(Error::Limit("card title or document URL"));
    }
    renderer.node.validate().map_err(Error::Native)?;
    renderer.browser.validate().map_err(Error::Native)?;
    let maximum = limits
        .bytes
        .min(theme.evidence().limits.context_bytes)
        .min(64 << 20);
    // Remove only our own derived field; every other page/presentation fact remains bound.
    let previous = take_social_card(model)?;
    let encoded = crate::build::serialization::encode(model, maximum, "social card model")
        .map_err(Error::Render)?;
    let model_hash = Sha256Digest::of(&encoded);
    let image_variants = match model.get("images") {
        Some(value) => Vec::<Image>::deserialize(value)
            .map_err(|_| Error::Binding("native image declarations"))?
            .into_iter()
            .filter(|image| image.variants.is_some())
            .collect::<Vec<_>>(),
        None => Vec::new(),
    };
    if image_variants.len() > limits.entries.min(10_000) {
        return Err(Error::Limit("card image declarations"));
    }
    model
        .as_object_mut()
        .ok_or(Error::Binding("page model"))?
        .insert(
            "card".into(),
            serde_json::json!({"width":recipe.width,"height":recipe.height}),
        );
    let original_presentation = model["presentation"].clone();
    let card_resources = model["presentation"]["theme_resources"]
        .as_array_mut()
        .ok_or(Error::Binding("theme resource projection"))?;
    for id in &recipe.resource_requirements {
        if card_resources
            .iter()
            .any(|value| value.get("id").and_then(Value::as_str) == Some(id.as_str()))
        {
            continue;
        }
        let resource = resources
            .model
            .get(id)
            .ok_or(Error::Binding("card resource selection"))?;
        card_resources.push(
            serde_json::to_value(crate::build::ThemeResourceProjection {
                id: id.clone(),
                resource: resource.clone(),
            })
            .map_err(|_| Error::Binding("card resource projection"))?,
        );
    }
    // The compiled import map covers every selected dependency, without adding page roots.
    let mut pending: BTreeSet<_> = recipe.resource_requirements.iter().cloned().collect();
    let mut reachable = BTreeSet::new();
    while let Some(id) = pending.pop_first() {
        if !reachable.insert(id.clone()) {
            continue;
        }
        if let Some(module) = resources.browser.modules.get(&id) {
            pending.extend(module.dependencies.iter().cloned());
        }
    }
    let mut preloads = model["presentation"]["browser_modules"]["preloads"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    for id in reachable {
        if let Some(module) = resources.browser.modules.get(&id)
            && !preloads.iter().any(|value| {
                value.get("url").and_then(Value::as_str) == Some(module.resource.url.as_str())
            })
        {
            preloads
                .push(serde_json::to_value(module).map_err(|_| Error::Binding("card preload"))?);
        }
    }
    model["presentation"]["browser_modules"] =
        serde_json::json!({"import_map":resources.browser.import_map,"preloads":preloads});
    let rendered = theme.render(&recipe.template, model).map_err(Error::Render);
    restore_card_context(model, original_presentation, previous)?;
    let html = String::from_utf8(rendered?).map_err(|_| Error::Binding("card HTML UTF-8"))?;
    if html.len() > 8 << 20 || assets.len() > limits.entries.min(10_000) {
        return Err(Error::Limit("card HTML or assets"));
    }
    let mut remaining = limits
        .bytes
        .checked_sub(html.len())
        .ok_or(Error::Limit("card request bytes"))?;
    let mut asset_bytes = 0u64;
    let mut seen = BTreeSet::new();
    let mut descriptors = Vec::new();
    for (index, asset) in assets.iter().enumerate() {
        if asset.url == document_url || !seen.insert(asset.url) {
            return Err(Error::Binding("duplicate card asset URL"));
        }
        if asset.url.len() > 8192 || asset.bytes.len() > 256 << 20 {
            return Err(Error::Limit("card asset"));
        }
        remaining = remaining
            .checked_sub(asset.url.len().saturating_add(512))
            .ok_or(Error::Limit("card asset metadata"))?;
        asset_bytes = asset_bytes
            .checked_add(asset.bytes.len() as u64)
            .filter(|bytes| *bytes <= limits.assets.min(1 << 30))
            .ok_or(Error::Limit("card asset bytes"))?;
        descriptors.push(Asset {
            url: asset.url.to_owned(),
            entry: format!("payload/{index}").parse().map_err(Error::Source)?,
            media_type: asset.media_type,
            role: asset.role,
            bytes: asset.bytes.len() as u64,
            sha256: Sha256Digest::of(asset.bytes),
        });
    }
    let encoded = crate::build::serialization::encode(
        &Identity {
            site,
            model: model_hash,
            html: &html,
            initial_title,
            document_url,
            recipe,
            renderer,
            assets: &descriptors,
            image_variants: &image_variants,
        },
        limits.bytes.min(64 << 20),
        "card request identity",
    )
    .map_err(Error::Render)?;
    if asset_bytes
        .checked_add(encoded.len() as u64)
        .is_none_or(|bytes| bytes > limits.total)
    {
        return Err(Error::Limit("aggregate retained request"));
    }
    let identity = Sha256Digest::of(&encoded);
    if let Some(captured) = captured {
        if captured.evidence.request_sha256 != identity || captured.evidence.site != *site {
            return Err(Error::Binding("retained card differs from exact request"));
        }
        return Ok(None);
    }
    // All request and aggregate payload ceilings precede the first payload clone.
    let mut builder = InventoryBuilder::new(gazette_core::source::Limits {
        entries: descriptors.len().saturating_add(1),
        depth: 2,
        file_bytes: 256 << 20,
        total_bytes: asset_bytes,
    });
    builder
        .insert(
            "payload".parse().map_err(Error::Source)?,
            Content::Directory,
        )
        .map_err(Error::Source)?;
    let mut files = BTreeMap::new();
    for (descriptor, asset) in descriptors.iter().zip(assets) {
        builder
            .insert(
                descriptor.entry.clone(),
                Content::File {
                    bytes: descriptor.bytes,
                    sha256: descriptor.sha256,
                },
            )
            .map_err(Error::Source)?;
        files.insert(descriptor.entry.clone(), asset.bytes.to_vec());
    }
    Ok(Some(SocialCardCaptureRequest {
        site: site.clone(),
        identity,
        renderer: renderer.clone(),
        html,
        title: initial_title.to_owned(),
        document_url: document_url.to_owned(),
        recipe: recipe.clone(),
        assets: descriptors,
        image_variants,
        payload: CapturedSource::from_parts(builder.finish(), files).map_err(Error::Source)?,
        request_bytes: encoded.len(),
    }))
}

// Static transport labels follow the supported file formats; the browser still decodes
// the media and the recipe independently declares image versus video readiness.
pub(crate) fn static_media(path: &EntryPath) -> Option<(&'static str, &'static str)> {
    match path
        .as_str()
        .rsplit('.')
        .next()?
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => Some(("image/png", "media")),
        "jpg" | "jpeg" => Some(("image/jpeg", "media")),
        "webp" => Some(("image/webp", "media")),
        "gif" => Some(("image/gif", "media")),
        "svg" => Some(("image/svg+xml", "media")),
        "avif" => Some(("image/avif", "media")),
        "mp4" => Some(("video/mp4", "media")),
        "webm" => Some(("video/webm", "media")),
        "woff2" => Some(("font/woff2", "font")),
        "woff" => Some(("font/woff", "font")),
        _ => None,
    }
}

// Map removal may swap the final entry into the removed position when Serde's
// preserve_order feature is enabled. Keep the existing page-context ordering.
fn take_social_card(model: &mut Value) -> Result<Option<Value>, Error> {
    let presentation = model
        .get_mut("presentation")
        .and_then(Value::as_object_mut)
        .ok_or(Error::Binding("page presentation"))?;
    let previous = presentation.remove("social_card");
    presentation.sort_keys();
    Ok(previous)
}

fn restore_card_context(
    model: &mut Value,
    presentation: Value,
    previous: Option<Value>,
) -> Result<(), Error> {
    model["presentation"] = presentation;
    model
        .as_object_mut()
        .ok_or(Error::Binding("page model"))?
        .remove("card");
    if let Some(previous) = previous {
        model["presentation"]
            .as_object_mut()
            .ok_or(Error::Binding("page presentation"))?
            .insert("social_card".into(), previous);
    }
    model.sort_all_objects();
    Ok(())
}

#[cfg(test)]
#[path = "../../../tests/internal/social_card_context.rs"]
mod context_tests;
