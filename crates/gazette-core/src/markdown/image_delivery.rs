//! Author-selected browser image delivery hints, independent of image meaning and geometry.

use serde::{Deserialize, Serialize};

/// When a browser should load this image occurrence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ImageLoading {
    /// Load without deferring until the image approaches the viewport.
    Eager,
    /// Let the browser defer an offscreen image.
    Lazy,
}

impl ImageLoading {
    /// Native HTML attribute value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Eager => "eager",
            Self::Lazy => "lazy",
        }
    }
}

/// Relative browser request priority; not a guarantee of scheduling or completion.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ImageFetchPriority {
    /// Give this image higher relative priority.
    High,
    /// Give this image lower relative priority.
    Low,
    /// Leave relative priority to the browser.
    Auto,
}

impl ImageFetchPriority {
    /// Native HTML attribute value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Low => "low",
            Self::Auto => "auto",
        }
    }
}
