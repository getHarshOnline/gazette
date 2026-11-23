//! SVG viewport evidence interpreted from already admitted root attributes.
use serde::Serialize;

/// Finite intrinsic lengths and aspect ratio, distinct from decoded raster pixels.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, schemars::JsonSchema)]
pub struct SvgLayout {
    #[serde(skip_serializing_if = "Option::is_none")]
    width: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    height: Option<f64>,
    height_per_width: f64,
}
// Construction excludes NaN and infinity; equality is total for admitted values.
impl Eq for SvgLayout {}
impl SvgLayout {
    /// Interpret root sizing after XML admission; this does not admit XML or CSS.
    /// Absolute dimensions take precedence over a valid viewBox ratio.
    pub fn from_root(
        width: Option<&str>,
        height: Option<&str>,
        view_box: Option<&str>,
    ) -> Option<Self> {
        let width = width.and_then(length);
        let height = height.and_then(length);
        let height_per_width = width
            .zip(height)
            .map(|(width, height)| height / width)
            .or_else(|| view_box.and_then(view_box_ratio))?;
        (height_per_width.is_finite() && height_per_width > 0.0).then_some(Self {
            width,
            height,
            height_per_width,
        })
    }
    /// Explicit absolute root width in CSS pixels, when present.
    pub fn width(self) -> Option<f64> {
        self.width
    }
    /// Explicit absolute root height in CSS pixels, when present.
    pub fn height(self) -> Option<f64> {
        self.height
    }
    /// Verified viewport height divided by width; viewBox offsets do not change it.
    pub fn height_per_width(self) -> f64 {
        self.height_per_width
    }
    /// Resolve a known intrinsic width, without inventing one from viewBox units.
    pub fn html_width(self) -> Option<u32> {
        self.width
            .or_else(|| self.height.map(|height| height / self.height_per_width))
            .and_then(html_length)
    }
    /// Scale a known display width, retaining fractional ratio until HTML rounding.
    pub fn html_height(self, width: u32) -> Option<u32> {
        html_length(f64::from(width) * self.height_per_width)
    }
    /// Whether two admitted images reserve the same aspect ratio.
    pub fn same_ratio(self, other: Self) -> bool {
        self.height_per_width == other.height_per_width
    }
}
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn html_length(value: f64) -> Option<u32> {
    let rounded = value.round().max(1.0);
    (value.is_finite() && value > 0.0 && rounded <= f64::from(u32::MAX)).then_some(rounded as u32)
}
fn length(value: &str) -> Option<f64> {
    let lower = value.trim().to_ascii_lowercase();
    let (number, factor) = [
        ("px", 1.0),
        ("cm", 96.0 / 2.54),
        ("mm", 96.0 / 25.4),
        ("q", 96.0 / 101.6),
        ("in", 96.0),
        ("pt", 96.0 / 72.0),
        ("pc", 16.0),
    ]
    .into_iter()
    .find_map(|(unit, factor)| lower.strip_suffix(unit).map(|number| (number, factor)))
    .unwrap_or((&lower, 1.0));
    let number = number.parse::<f64>().ok()? * factor;
    (number.is_finite() && number > 0.0).then_some(number)
}
fn view_box_ratio(value: &str) -> Option<f64> {
    if value.split(',').any(|part| part.trim().is_empty()) {
        return None;
    }
    let values = value
        .split(|c: char| c.is_ascii_whitespace() || c == ',')
        .filter(|part| !part.is_empty())
        .map(str::parse::<f64>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    (values.len() == 4
        && values.iter().all(|value| value.is_finite())
        && values[2] > 0.0
        && values[3] > 0.0)
        .then(|| values[3] / values[2])
}
/// Verified image layout supplied to occurrence projection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageLayout {
    /// Verified raster display extent, including retained-image orientation.
    Raster(crate::raster::Dimensions),
    /// Admitted SVG root sizing, without a decoded-pixel claim.
    Svg(SvgLayout),
}
