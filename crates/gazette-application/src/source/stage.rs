//! Compose captured authored bytes and prepared project bytes before a fresh write.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use gazette_core::source::{Content, Error as SourceError, Inventory, Limits, TreeInput};
use gazette_core::stage::{Error, Location};
use gazette_ports::destination::RootSeparation;
use gazette_ports::source::{SourceCapture, SourceReader};
use gazette_ports::stage::StageWriter;

use super::destination::{compose, protected_roots};
use super::{PreparedSourceSet, StageContext, materialization};

/// Check the complete logical stage and its actual output parent without writes.
///
/// Uses staging's composition and native parent-admission rules. Authored content
/// and prepared projects are inputs, which may overlap; the staging parent must
/// be disjoint from all of them. No payloads or parent children are retained.
/// Success is observational, not a lease: staging repeats admission and captures
/// current authored bytes. This does not certify content or write readiness.
///
/// # Errors
///
/// Propagates source, context, layout, limit and native parent-admission failures.
pub fn check_stage(
    reader: &impl SourceReader,
    destination: &impl RootSeparation,
    prepared: &PreparedSourceSet,
    context: &StageContext,
    authored_root: &Path,
    staging_parent: &Path,
    limits: Limits,
) -> Result<Inventory, Error> {
    let generated = materialization::generate(prepared, context)?;
    let base = reader.read(authored_root, None, limits)?;
    let inventory = compose(&base, prepared, &generated, limits)?;
    destination.check_root_separation(staging_parent, &input_roots(prepared, authored_root))?;
    Ok(inventory)
}

/// Capture authored content and stage the complete composition in a fresh tree.
///
/// Layout/collision checks and byte binding complete before the writer is called.
/// The base capture and proposed output each obey `limits`; already prepared
/// project payloads retain their earlier acquisition limits. Payloads are borrowed
/// into the writer, not copied into a second in-memory aggregate. No source is
/// reopened to copy its bytes. Source meaning and publication gates remain outside
/// this structural operation; success grants no replacement or release authority.
///
/// # Errors
///
/// Propagates capture, layout, byte-binding, native staging and cleanup failures.
pub fn stage<S: StageWriter>(
    reader: &impl SourceCapture,
    writer: &S,
    prepared: &PreparedSourceSet,
    context: &StageContext,
    authored_root: &Path,
    location: Location<'_>,
    limits: Limits,
) -> Result<S::Stage, Error> {
    let generated = materialization::generate(prepared, context)?;
    let (base, contents) = reader.capture(authored_root, None, limits)?.into_parts();
    let inventory = compose(&base, prepared, &generated, limits)?;
    let mut files = BTreeMap::new();
    for entry in base.entries() {
        if matches!(entry.content, Content::File { .. }) {
            let bytes = contents
                .get(&entry.path)
                .ok_or_else(|| SourceError::ContentMismatch(entry.path.as_str().into()))?;
            files.insert(entry.path.clone(), bytes);
        }
    }
    for placement in prepared.placements() {
        for entry in placement.source().inventory().entries() {
            if matches!(entry.content, Content::File { .. }) {
                let path = format!("{}/{}", placement.request().target, entry.path).parse()?;
                let bytes = placement
                    .source()
                    .file(&entry.path)
                    .ok_or_else(|| SourceError::ContentMismatch(entry.path.as_str().into()))?;
                files.insert(path, bytes);
            }
        }
    }
    for (path, bytes) in &generated {
        files.insert(path.clone(), bytes.as_slice());
    }
    let input = TreeInput::new(&inventory, files)?;
    writer.stage(
        location,
        &input_roots(prepared, authored_root),
        input,
        limits,
    )
}

fn input_roots(prepared: &PreparedSourceSet, authored_root: &Path) -> Vec<PathBuf> {
    let mut roots = protected_roots(prepared);
    roots.push(authored_root.to_owned());
    roots.sort();
    roots.dedup();
    roots
}
