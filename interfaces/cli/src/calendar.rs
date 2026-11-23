//! Capture presentation-only calendar context at an invocation boundary, never inside templates.
use chrono::Datelike;
use gazette_sdk::authored::RenderYear;

/// Preserve an explicit year or capture the current UTC year once for this invocation.
///
/// # Errors
/// Refuses a host clock outside the supported presentation-year range.
pub fn resolve(explicit: Option<RenderYear>) -> Result<RenderYear, &'static str> {
    explicit.map_or_else(|| year_at(std::time::SystemTime::now()), Ok)
}

fn year_at(time: std::time::SystemTime) -> Result<RenderYear, &'static str> {
    let date = chrono::DateTime::<chrono::Utc>::from(time);
    let year =
        u16::try_from(date.year()).map_err(|_| "current UTC year is outside supported range")?;
    RenderYear::new(year)
}

#[cfg(test)]
#[path = "../tests/unit/calendar.rs"]
mod tests;
