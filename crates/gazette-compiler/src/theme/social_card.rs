//! Theme-owned capture facts; native execution and publication placement belong to the application.

use gazette_core::source::EntryPath;
use serde::{Deserialize, Serialize};

/// The exact media element the fixed capture helper must wait for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SocialCardMediaKind {
    /// Wait for the selected image's decoded pixels.
    Image,
    /// Wait for the selected video's requested frame.
    Video,
}

/// One complete inherited Theme recipe for a publication-home link preview.
/// A dependent package replaces the whole recipe rather than mixing capture settings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SocialCardRecipe {
    /// Package-local template, qualified in its declaring namespace during graph resolution.
    pub template: EntryPath,
    /// Independently enabled resources loaded only by the card template, in declared order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resource_requirements: Vec<gazette_core::resource::ResourceId>,
    /// Output width in CSS pixels, from 1 through 4096.
    #[schemars(range(min = 1, max = 4096))]
    pub width: u32,
    /// Output height in CSS pixels, from 1 through 4096.
    #[schemars(range(min = 1, max = 4096))]
    pub height: u32,
    /// One nonempty CSS selector, at most 1024 UTF-8 bytes, without controls or outer whitespace.
    /// Native capture checks selector syntax, cardinality and the selected element's kind.
    #[schemars(length(min = 1, max = 1024))]
    pub media_selector: String,
    /// Required media kind; never inferred from a filename or selector.
    pub media_kind: SocialCardMediaKind,
    /// Optional video seek from 0 through 600000 milliseconds; forbidden for images.
    /// Absence leaves the fixed capture helper's initial-frame default intact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 0, max = 600000))]
    pub video_time_ms: Option<u32>,
}

impl SocialCardRecipe {
    pub(crate) fn validate(&self) -> Result<(), (&'static str, &'static str)> {
        let mut seen = std::collections::BTreeSet::new();
        for resource in &self.resource_requirements {
            if !seen.insert(resource) {
                return Err((
                    "render.social_card.resource_requirements",
                    "duplicate logical resource requirement",
                ));
            }
        }
        for (field, dimension) in [
            ("render.social_card.width", self.width),
            ("render.social_card.height", self.height),
        ] {
            if !(1..=4096).contains(&dimension) {
                return Err((field, "capture dimension must be between 1 and 4096 pixels"));
            }
        }
        if self.media_selector.is_empty()
            || self.media_selector.len() > 1024
            || self.media_selector.trim() != self.media_selector
            || self.media_selector.chars().any(char::is_control)
        {
            return Err((
                "render.social_card.media_selector",
                "selector must contain 1..=1024 UTF-8 bytes without controls or outer whitespace",
            ));
        }
        if let Some(time) = self.video_time_ms {
            if self.media_kind != SocialCardMediaKind::Video {
                return Err((
                    "render.social_card.video_time_ms",
                    "seek is only valid for video media",
                ));
            }
            if time > 600_000 {
                return Err((
                    "render.social_card.video_time_ms",
                    "video seek must not exceed 600000 milliseconds",
                ));
            }
        }
        Ok(())
    }
}
