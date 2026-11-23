//! Recreate a discovered editor selection from saved files and only its unsaved text.

use super::*;
use gazette_core::source::Sha256Digest;
use serde::{Deserialize, Serialize};

/// Saved discovery choices and configuration pins; this data grants no read permission.
#[derive(Clone, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    /// Absolute workspace directory selected by the editor.
    pub root: PathBuf,
    /// Configuration document relative to the workspace directory.
    pub workspace: EntryPath,
    /// Explicit manual support files in addition to configured navigation.
    pub manuals: Vec<ManualSelection>,
    /// Saved configuration and machine-binding bytes captured by discovery.
    pub configuration: BTreeMap<EntryPath, Sha256Digest>,
}

/// Complete source metadata, shared by native reconstruction and transport verification.
#[derive(Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Editor generation; independent of the receiving session's generation.
    pub generation: u64,
    /// Current analysis status, including invalid text retained for repair.
    pub structurally_valid: bool,
    /// Whether the configuration has an explicit included-document set.
    pub modular: bool,
    /// Every retained source, whether or not its text is unsaved.
    pub sources: Vec<Identity>,
    /// Opaque availability observations, without captured bytes or content hashes.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub asset_observations: BTreeMap<String, Asset>,
}

/// One retained source's current and saved identities.
#[derive(Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    /// Client identity, mapped from native paths by the receiving transport.
    pub id: String,
    /// Logical compiler path.
    pub path: EntryPath,
    /// Accepted open version, including a clean open document.
    #[serde(deserialize_with = "Deserialize::deserialize")]
    pub version: Option<i32>,
    /// Current byte count.
    pub bytes: usize,
    /// Digest of current text or retained opaque bytes.
    pub source_sha256: Sha256Digest,
    /// Required for native replay; absent only in older self-contained exports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_sha256: Option<Sha256Digest>,
    /// Native source role; checked against rediscovery rather than trusted as a locator.
    #[schemars(with = "SourceRole<'static>")]
    pub role: serde_json::Value,
}

/// Opaque file availability, not a claim about its contents.
#[derive(Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    /// Logical source coordinate.
    pub path: EntryPath,
    /// Observed file length.
    pub observed_bytes: u64,
}

/// Current text of one changed open document.
#[derive(Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Overlay {
    /// Selected client identity.
    pub id: String,
    /// Accepted editor version.
    pub version: i32,
    /// Complete unsaved text, not a patch to apply to unknown bytes.
    pub text: String,
}

impl Workspace {
    /// Describe retained discovery inputs without reading files or granting permission.
    pub fn handoff_selection(&self) -> Selection {
        Selection {
            root: self.root.clone(),
            workspace: self.path.clone(),
            manuals: self.manuals.clone(),
            configuration: self.configuration_baselines.clone(),
        }
    }
}

/// Read the chosen workspace and reproduce the exact current editor selection.
///
/// The caller must obtain explicit native-read permission before invoking this function.
/// No file is written, asset payload opened, or stale/partial selection returned.
///
/// # Errors
/// Refuses changed baselines, membership, roles, observations or versions; malformed
/// overlays; unavailable discovery; and exhausted native session bounds.
pub fn restore(
    selection: Selection,
    manifest: &Manifest,
    overlays: Vec<Overlay>,
    identifier: Identifier,
    limits: Limits,
) -> Result<Session, Error> {
    let invalid = Error::Selection("native editor handoff differs from its saved workspace");
    if manifest.generation == 0
        || !selection.root.is_absolute()
        || selection.root.components().any(|part| {
            matches!(
                part,
                std::path::Component::CurDir | std::path::Component::ParentDir
            )
        })
        || manifest
            .sources
            .len()
            .saturating_add(manifest.asset_observations.len())
            > limits.documents
        || selection.configuration.is_empty()
        || selection.configuration.len() > limits.documents
        || !selection.configuration.contains_key(&selection.workspace)
        || selection.manuals.len() > limits.documents
    {
        return Err(invalid);
    }
    let expected: BTreeMap<_, _> = manifest
        .sources
        .iter()
        .map(|source| (source.id.as_str(), source))
        .collect();
    if expected.len() != manifest.sources.len()
        || expected
            .values()
            .any(|source| source.baseline_sha256.is_none())
    {
        return Err(invalid);
    }
    let mut changed = BTreeMap::new();
    let mut transferred = 0usize;
    for overlay in overlays {
        transferred = transferred
            .checked_add(overlay.text.len())
            .filter(|bytes| *bytes <= limits.source_bytes)
            .ok_or(Error::Limit("unsaved editor text bytes"))?;
        let source = expected.get(overlay.id.as_str()).ok_or(invalid)?;
        if source.version != Some(overlay.version)
            || source.bytes != overlay.text.len()
            || source.source_sha256 != Sha256Digest::of(overlay.text.as_bytes())
            || source.baseline_sha256 == Some(source.source_sha256)
            || source.role["kind"] == "page_input"
            || changed.insert(overlay.id.clone(), overlay).is_some()
        {
            return Err(invalid);
        }
    }
    if expected.values().any(|source| {
        (source.baseline_sha256 != Some(source.source_sha256)) != changed.contains_key(&source.id)
    }) {
        return Err(invalid);
    }
    // Check the original configuration and machine binding before discovery can
    // follow their paths. The exported data does not authorize these reads.
    let paths: Vec<_> = selection.configuration.keys().cloned().collect();
    let captured =
        crate::configuration::capture_selected(&selection.root, &paths, source_limits(limits))
            .map_err(|_| invalid)?;
    for (path, digest) in &selection.configuration {
        if captured
            .file(path)
            .is_none_or(|bytes| Sha256Digest::of(bytes) != *digest)
        {
            return Err(invalid);
        }
    }
    let mut workspace = Workspace::open_workspace(
        selection.root,
        selection.workspace,
        selection.manuals,
        identifier,
        limits,
    )?;
    // Configuration overlays can introduce another included document. Replay
    // only already-selected configuration until the whole declared set is ready.
    let mut configuration: BTreeSet<_> = expected
        .values()
        .filter(|source| {
            matches!(
                source.role["kind"].as_str(),
                Some("workspace" | "workspace_document")
            )
        })
        .map(|source| source.id.as_str())
        .collect();
    while !configuration.is_empty() {
        let mut progressed = false;
        for id in configuration.clone() {
            if workspace.state.files.contains_key(id) {
                replay(&mut workspace, expected[id], &mut changed)?;
                configuration.remove(id);
                progressed = true;
            }
        }
        if !progressed {
            return Err(invalid);
        }
    }
    for source in expected.values().filter(|source| {
        !matches!(
            source.role["kind"].as_str(),
            Some("workspace" | "workspace_document")
        )
    }) {
        replay(&mut workspace, source, &mut changed)?;
    }
    if !changed.is_empty() || workspace.configuration_baselines != selection.configuration {
        return Err(invalid);
    }
    let actual = workspace.session().snapshot();
    if actual.modular != manifest.modular
        || actual.structurally_valid != manifest.structurally_valid
        || actual.sources.len() != expected.len()
        || actual.asset_observations.len() != manifest.asset_observations.len()
    {
        return Err(invalid);
    }
    for source in actual.sources {
        let pin = expected.get(source.id).ok_or(invalid)?;
        if source.path != &pin.path
            || source.version != pin.version
            || source.bytes != pin.bytes
            || source.source_sha256 != pin.source_sha256
            || Some(source.baseline_sha256) != pin.baseline_sha256
            || serde_json::to_value(source.role).map_err(|_| invalid)? != pin.role
        {
            return Err(invalid);
        }
    }
    for (id, observed) in actual.asset_observations {
        let pin = manifest.asset_observations.get(id).ok_or(invalid)?;
        if observed.path() != &pin.path || observed.observed_bytes() != pin.observed_bytes {
            return Err(invalid);
        }
    }
    Ok(workspace.state)
}

fn replay(
    workspace: &mut Workspace,
    source: &Identity,
    changed: &mut BTreeMap<String, Overlay>,
) -> Result<(), Error> {
    let invalid = Error::Selection("native editor source baseline changed or is unavailable");
    if source.role["kind"] == "page_input" {
        return Ok(());
    }
    let document = workspace.state.files.get(&source.id).ok_or(invalid)?;
    if Some(Sha256Digest::of(document.baseline.as_bytes())) != source.baseline_sha256 {
        return Err(invalid);
    }
    if let Some(version) = source.version {
        let text = changed
            .remove(&source.id)
            .map_or_else(|| document.text().to_owned(), |overlay| overlay.text);
        let path = workspace.paths.get(&source.id).ok_or(invalid)?.clone();
        if !workspace.open(&source.id, &path, version, text)? {
            return Err(invalid);
        }
    } else if changed.contains_key(&source.id) {
        return Err(invalid);
    }
    Ok(())
}
