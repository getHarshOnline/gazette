//! Capture-relative Rustdoc references; no native reads or target-specific meaning inference.

use crate::{Error, invalid};
use gazette_core::source::EntryPath;

pub(super) enum Destination {
    External(String),
    Local {
        path: EntryPath,
        fragment: Option<String>,
    },
}

pub(super) fn resolve(origin: &str, href: &str) -> Result<Destination, Error> {
    if href
        .chars()
        .any(|ch| ch.is_whitespace() || ch.is_control() || ch == '\\')
    {
        return Err(invalid(origin, "unsafe Rustdoc reference"));
    }
    if href.starts_with("https://") || href.starts_with("http://") {
        let parsed = url::Url::parse(href)
            .map_err(|_| invalid(origin, "invalid external Rustdoc reference"))?;
        if parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            return Err(invalid(origin, "credential-bearing Rustdoc reference"));
        }
        return Ok(Destination::External(href.into()));
    }
    let (path, fragment) = href
        .split_once('#')
        .map_or((href, None), |(path, fragment)| {
            (path, (!fragment.is_empty()).then(|| fragment.to_owned()))
        });
    if path.contains(['?', ':']) || path.starts_with('/') {
        return Err(invalid(
            origin,
            "unsupported Rustdoc reference scheme or query",
        ));
    }
    if path.is_empty() {
        return Ok(Destination::Local {
            path: origin.parse()?,
            fragment,
        });
    }
    let mut segments: Vec<&str> = origin.split('/').collect();
    segments.pop();
    let decoded = percent_encoding::percent_decode_str(path)
        .decode_utf8()
        .map_err(|_| invalid(origin, "non-UTF-8 Rustdoc path"))?;
    // Encoded separators must not create another path topology after admission.
    if path.split('/').count() != decoded.split('/').count() || decoded.contains('\\') {
        return Err(invalid(origin, "encoded Rustdoc path separator"));
    }
    for segment in decoded.split('/') {
        match segment {
            "." => {}
            ".." => {
                segments
                    .pop()
                    .ok_or_else(|| invalid(origin, "Rustdoc reference escapes retained capture"))?;
            }
            "" => return Err(invalid(origin, "empty Rustdoc path component")),
            value => segments.push(value),
        }
    }
    Ok(Destination::Local {
        path: segments.join("/").parse()?,
        fragment,
    })
}

pub(super) fn url(path: &str, fragment: Option<&str>) -> String {
    const SEGMENT: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
        .remove(b'-')
        .remove(b'.')
        .remove(b'_')
        .remove(b'~');
    let mut result = String::new();
    for segment in path.split('/') {
        result.push('/');
        result.push_str(&percent_encoding::utf8_percent_encode(segment, SEGMENT).to_string());
    }
    if let Some(fragment) = fragment {
        result.push('#');
        result.push_str(&percent_encoding::utf8_percent_encode(fragment, SEGMENT).to_string());
    }
    result
}

#[cfg(test)]
#[path = "../../tests/internal/rustdoc_links.rs"]
mod tests;
