//! Native JavaScript text reaches the same bounded, immutable-output comparator.

use gazette_compiler::javascript::{self, Error, Goal, TextKind};
use gazette_core::source::EntryPath;

use super::{DisclosureProbeError, DisclosureRepresentation, Scan};

pub(super) fn file_goal(suffix: &str) -> Option<Goal> {
    if suffix.eq_ignore_ascii_case("mjs") {
        Some(Goal::Module)
    } else if suffix.eq_ignore_ascii_case("cjs") {
        Some(Goal::Script)
    } else if suffix.eq_ignore_ascii_case("js") {
        // A suffix alone cannot establish a browser's loading context. Let the
        // native parser identify explicit module syntax; report this weaker goal.
        Some(Goal::Unambiguous)
    } else {
        None
    }
}

pub(super) fn scan(
    scan: &mut Scan<'_>,
    path: &EntryPath,
    bytes: &[u8],
    goal: Goal,
) -> Result<(), DisclosureProbeError> {
    javascript::inspect(bytes, goal, scan.limits.javascript, |kind, value| {
        let representation = match kind {
            TextKind::Identifier => DisclosureRepresentation::JavascriptIdentifier,
            TextKind::String => DisclosureRepresentation::JavascriptString,
            TextKind::Template => DisclosureRepresentation::JavascriptTemplate,
        };
        if kind == TextKind::Identifier {
            scan.search(path, value.as_bytes(), representation)
        } else {
            scan.text(path, value.as_bytes(), representation)
        }
    })
    .map_err(|error| {
        let reason = match error {
            Error::Consumer(error) => return error,
            Error::Limit(resource) => {
                return DisclosureProbeError::Limit(match resource {
                    "input bytes" => "JavaScript input bytes",
                    "nodes" => "JavaScript syntax nodes",
                    "nesting depth" => "JavaScript syntax depth",
                    "decoded bytes" => "JavaScript decoded bytes",
                    _ => "JavaScript decoding",
                });
            }
            Error::Utf8 => "requires UTF-8",
            Error::Malformed => "syntax is malformed",
            Error::Unsupported => "syntax text is outside the decoding profile",
        };
        DisclosureProbeError::Javascript {
            path: path.clone(),
            reason,
        }
    })
}
