//! Explicit pixel operations and encoding choices, independent of provider and output paths.

use serde::{Deserialize, Serialize};

/// Observed pixel extent, not a requested resize bound or a CSS display size.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct Dimensions {
    /// Actual pixel width.
    #[schemars(range(min = 1, max = u32::MAX))]
    pub width: u32,
    /// Actual pixel height.
    #[schemars(range(min = 1, max = u32::MAX))]
    pub height: u32,
}

/// Geometry observed along one retained raster transformation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct Geometry {
    /// Stored source pixels before orientation handling.
    pub source: Dimensions,
    /// Pixels after the recipe's selected orientation, before resize/crop.
    pub oriented: Dimensions,
    /// Final encoded pixels, independently decoded before artifact acceptance.
    pub output: Dimensions,
}

/// Resize after the recipe's selected orientation handling. Dimensions are pixels;
/// providers reject zero or out-of-bounds dimensions before allocating output.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "operation", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Resize {
    /// Produce exact dimensions without preserving aspect ratio.
    Scale {
        /// Target pixel width.
        width: u32,
        /// Target pixel height.
        height: u32,
    },
    /// Preserve aspect ratio at the requested width, including upscaling.
    FitWidth {
        /// Target pixel width.
        width: u32,
    },
    /// Preserve aspect ratio at the requested height, including upscaling.
    FitHeight {
        /// Target pixel height.
        height: u32,
    },
    /// Fit within both dimensions, preserving aspect ratio and never upscaling.
    Fit {
        /// Maximum output width.
        width: u32,
        /// Maximum output height.
        height: u32,
    },
    /// Center-crop to fill exact dimensions while preserving aspect ratio.
    Fill {
        /// Target pixel width.
        width: u32,
        /// Target pixel height.
        height: u32,
    },
}

/// Encoded output representation. JPEG's loss and alpha handling are explicit;
/// none of these choices asserts sanitation, rights or publication readiness.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "format", rename_all = "lowercase", deny_unknown_fields)]
pub enum Encoding {
    /// Lossless WebP with eight-bit RGBA channels, retaining transformed alpha.
    Webp,
    /// PNG with eight-bit RGBA channels, retaining transformed alpha.
    Png,
    /// JPEG with an explicit quality and opaque RGB background for alpha composition.
    Jpeg {
        /// Encoder quality in the inclusive range 1–100.
        #[schemars(range(min = 1, max = 100))]
        quality: u8,
        /// RGB background used when the input has transparency.
        background: [u8; 3],
    },
}
