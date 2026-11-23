//! Crawler rules and text and data mining reservations for every host a publication serves.
//!
//! The application writes these files for the authored site, and documentation targets
//! write the same bytes for the publication's documentation host, so both hosts state one
//! policy. The publication's workspace configuration supplies the rules and terms address.
//! Callers choose file placement and enforce their own byte ceilings.
//!
//! These directives request behaviour from crawlers and miners; they restrict no access.

use std::io::{self, Write};

/// One robots.txt user-agent group, in the publisher's declared order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CrawlerGroup<'a> {
    /// One product token, or `*` for crawlers without a more specific group.
    pub user_agent: &'a str,
    /// Allowed path patterns, written before disallowed patterns.
    pub allow: &'a [String],
    /// Disallowed path patterns; an empty string explicitly permits all paths.
    pub disallow: &'a [String],
}

/// A publication's reservation and crawler rules, carried to its documentation host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsagePolicy<'a> {
    /// Absolute terms-of-use address cited as the reservation's policy.
    pub terms_url: String,
    /// Crawler groups to publish; empty when the publication emits no robots policy.
    pub crawlers: Vec<CrawlerGroup<'a>>,
}

/// Write robots.txt groups, then an absolute `Sitemap` line when one exists.
///
/// # Errors
/// Returns the writer's error, including a caller's byte-ceiling refusal.
pub fn write_robots<'a>(
    output: &mut impl Write,
    groups: impl IntoIterator<Item = CrawlerGroup<'a>>,
    sitemap: Option<&str>,
) -> io::Result<()> {
    for group in groups {
        writeln!(output, "User-agent: {}", group.user_agent)?;
        for path in group.allow {
            writeln!(output, "Allow: {path}")?;
        }
        for path in group.disallow {
            writeln!(output, "Disallow: {path}")?;
        }
        writeln!(output)?;
    }
    if let Some(sitemap) = sitemap {
        writeln!(output, "Sitemap: {sitemap}")?;
    }
    Ok(())
}

/// The `/.well-known/tdmrep.json` bytes: one `TDMRep` rule reserving mining rights for
/// every path on the serving host and citing `policy`, followed by a newline.
pub fn tdm_reservation(policy: &str) -> Vec<u8> {
    // Field order is part of the published shape; the policy is escaped as a JSON string.
    format!(
        "[{{\"location\":\"/*\",\"tdm-reservation\":1,\"tdm-policy\":{}}}]\n",
        serde_json::Value::from(policy)
    )
    .into_bytes()
}
