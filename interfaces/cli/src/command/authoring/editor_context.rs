//! Import an explicit editor export through the existing selected-buffer session.

use std::{collections::BTreeMap, path::PathBuf};

use gazette_sdk::authoring::session::native::handoff::{self, Manifest, Overlay};
use gazette_sdk::{
    authoring::session,
    source::{EntryPath, Sha256Digest},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{Acknowledgement, Failure, Kernel, Selection, check_capacity, expire, lock, retain};
use crate::command::Command;

/// Maximum serialized handoff before decoding stdin, including JSON escaping.
pub const MAX_FRAME_BYTES: usize = 32 * 1024 * 1024;
const MAX_SOURCE_BYTES: usize = 12 * 1024 * 1024;

/// Explicit editor selection; native locators require separate receiving-side permission.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditorContext {
    format: String,
    workspace: String,
    generation: u64,
    active: Active,
    manifest: Manifest,
    start: Option<Selection>,
    native_selection: Option<handoff::Selection>,
    overlays: Vec<Overlay>,
    #[serde(skip)]
    allow_native_reads: bool,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Active {
    document: String,
    offset: usize,
    #[serde(deserialize_with = "Deserialize::deserialize")]
    version: Option<i32>,
    source_sha256: Sha256Digest,
}

/// One explicit handoff carrier. A file requires permission and an independently retained digest.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditorContextInput {
    /// Complete supplied context. Use exactly one of context and file.
    context: Option<EditorContext>,
    /// Exact local export. Use exactly one of context and file.
    file: Option<ContextFile>,
    /// Explicit permission to reconstruct a native handoff from its saved workspace.
    #[serde(default)]
    allow_native_reads: bool,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ContextFile {
    /// Absolute path to the exact private Gazette editor export.
    path: PathBuf,
    /// SHA-256 of the entire exported JSON file, before decoding.
    sha256: Sha256Digest,
    /// Must be true to read the explicitly selected file.
    allow_read: bool,
}

impl EditorContextInput {
    /// Read only the explicitly permitted and digest-pinned export, or consume inline buffers.
    ///
    /// # Errors
    /// Refuses missing permission, unsafe file selection and unreadable, changed or invalid exports.
    pub fn into_context(self) -> Result<EditorContext, session::Error> {
        use session::Error::Selection;
        match (self.context, self.file) {
            (Some(mut context), None) => {
                context.allow_native_reads = self.allow_native_reads;
                Ok(context)
            }
            (None, Some(file)) if file.allow_read && file.path.is_absolute() => {
                let (Some(parent), Some(entry)) = (
                    file.path.parent(),
                    file.path
                        .file_name()
                        .and_then(std::ffi::OsStr::to_str)
                        .and_then(|name| name.parse::<EntryPath>().ok()),
                ) else {
                    return Err(Selection("invalid editor context file path"));
                };
                let limits = gazette_sdk::source::Limits {
                    entries: 1,
                    depth: 1,
                    file_bytes: MAX_FRAME_BYTES as u64,
                    total_bytes: MAX_FRAME_BYTES as u64,
                };
                let capture = gazette_sdk::configuration::capture_selected(
                    parent,
                    std::slice::from_ref(&entry),
                    limits,
                )
                .map_err(|_| {
                    Selection("could not capture the exact bounded editor context file")
                })?;
                let bytes = capture
                    .file(&entry)
                    .ok_or(Selection("editor context file is missing"))?;
                if Sha256Digest::of(bytes) != file.sha256 {
                    return Err(Selection(
                        "editor context file digest differs from the selected export",
                    ));
                }
                serde_json::from_slice::<crate::json_input::Object<EditorContext>>(bytes)
                    .map(|crate::json_input::Object(mut context)| {
                        context.allow_native_reads = self.allow_native_reads;
                        context
                    })
                    .map_err(|_| Selection("file must contain one complete editor context object"))
            }
            _ => Err(Selection(
                "supply exactly one inline context or one permitted absolute context file",
            )),
        }
    }
}

/// Retain an imported context only after native analysis and parity checks finish.
pub struct ImportEditorContext {
    /// Transport-owned opaque process-local identifier.
    pub handle: String,
    /// Complete supplied editor export.
    pub context: EditorContext,
}

impl Command for ImportEditorContext {
    type Outcome = Acknowledgement;
    type Failure = Failure;

    fn execute(self, kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        let mut entries = lock(&kernel.authoring)?;
        expire(&mut entries);
        check_capacity(&entries, &self.handle)?;
        let (session, _, _) = self.context.verify().map_err(Failure::Native)?;
        Ok(retain(&mut entries, self.handle, session))
    }
}

impl EditorContext {
    /// Verify the export in memory and return an inspectable receipt, without retaining a handle.
    ///
    /// # Errors
    /// Refuses unsupported or inconsistent editor captures and exceeded handoff bounds.
    pub fn inspect(self) -> Result<impl Serialize, session::Error> {
        self.inspect_with_native_reads(false)
    }

    /// Inspect a native handoff only with permission supplied by the receiving caller.
    ///
    /// # Errors
    /// Refuses unavailable or changed saved inputs and inconsistent editor metadata.
    pub fn inspect_with_native_reads(
        mut self,
        allow_read: bool,
    ) -> Result<impl Serialize, session::Error> {
        self.allow_native_reads = allow_read;
        let (session, active, editor_generation) = self.verify()?;
        #[derive(Serialize)]
        struct Receipt<'a> {
            editor_generation: u64,
            active: Active,
            manifest: session::Snapshot<'a>,
        }
        serde_json::to_value(Receipt {
            editor_generation,
            active,
            manifest: session.snapshot(),
        })
        .map_err(|_| session::Error::Selection("cannot encode editor context receipt"))
    }

    fn verify(self) -> Result<(session::Session, Active, u64), session::Error> {
        use session::Error::{Limit, Selection};
        if self.format == "gazette-editor-context/2" {
            if !self.allow_native_reads || self.start.is_some() {
                return Err(Selection(
                    "native editor reconstruction requires explicit receiving-side read permission",
                ));
            }
            if self.generation == 0 || self.generation != self.manifest.generation {
                return Err(Selection("editor context generations disagree"));
            }
            let selected = self
                .native_selection
                .ok_or(Selection("native editor selection is missing"))?;
            if url::Url::from_directory_path(&selected.root)
                .ok()
                .map(|url| url.to_string().trim_end_matches('/').to_owned())
                != Some(self.workspace.trim_end_matches('/').to_owned())
            {
                return Err(Selection("editor workspace differs from native selection"));
            }
            let session = handoff::restore(
                selected,
                &self.manifest,
                self.overlays,
                file_identifier,
                session::Limits::default(),
            )?;
            check_active(&session, &self.active)?;
            return Ok((session, self.active, self.generation));
        }
        if self.format != "gazette-editor-context/1" || self.native_selection.is_some() {
            return Err(Selection("unsupported editor context format"));
        }
        if self.generation == 0 || self.generation != self.manifest.generation {
            return Err(Selection("editor context generations disagree"));
        }
        if !self.manifest.asset_observations.is_empty() {
            return Err(Selection(
                "opaque native observations require a native editor handoff",
            ));
        }
        let limits = session::Limits::default();
        let mut expected = BTreeMap::new();
        let mut bytes = 0usize;
        if self.manifest.sources.len() > limits.documents {
            return Err(Limit("editor context source count"));
        }
        for source in self.manifest.sources {
            bytes = bytes
                .checked_add(source.bytes)
                .filter(|total| *total <= MAX_SOURCE_BYTES)
                .ok_or(Limit("editor context source bytes"))?;
            if expected.insert(source.id.clone(), source).is_some() {
                return Err(Selection("duplicate editor context source identity"));
            }
        }
        let start = self
            .start
            .ok_or(Selection("supplied editor inputs are missing"))?
            .into_start(
                String::new(),
                session::Limits {
                    source_bytes: MAX_SOURCE_BYTES * 2,
                    ..limits
                },
            )?;
        let root_matches = url::Url::parse(&self.workspace)
            .ok()
            .is_some_and(|mut root| {
                if root.scheme() != "file" || root.query().is_some() || root.fragment().is_some() {
                    return false;
                }
                let Ok(mut segments) = root.path_segments_mut() else {
                    return false;
                };
                segments
                    .pop_if_empty()
                    .extend(start.workspace.path.as_str().split('/'));
                drop(segments);
                url::Url::parse(&start.workspace.id).is_ok_and(|id| id == root)
            });
        if !root_matches {
            return Err(Selection(
                "editor workspace does not own the supplied configuration locator",
            ));
        }
        let mut session = start.into_session()?;
        for overlay in self.overlays {
            if session
                .files()
                .get(&overlay.id)
                .is_none_or(|document| document.text() != overlay.text)
            {
                return Err(Selection(
                    "editor overlay does not match the exported current bytes",
                ));
            }
            if !session.open(&overlay.id, overlay.version, overlay.text)? {
                return Err(Selection("duplicate or unselected editor overlay"));
            }
        }
        let snapshot = session.snapshot();
        if snapshot.modular != self.manifest.modular
            || snapshot.structurally_valid != self.manifest.structurally_valid
            || snapshot.sources.len() != expected.len()
        {
            return Err(Selection(
                "native session does not match the complete editor manifest",
            ));
        }
        for source in snapshot.sources {
            let matched = expected.get(source.id).is_some_and(|expected| {
                source.path == &expected.path
                    && source.version == expected.version
                    && source.source_sha256 == expected.source_sha256
                    && source.bytes == expected.bytes
                    && serde_json::to_value(source.role).is_ok_and(|role| role == expected.role)
            });
            if !matched {
                return Err(Selection(
                    "native source role, version, size or digest differs from the editor manifest",
                ));
            }
        }
        check_active(&session, &self.active)?;
        Ok((session, self.active, self.generation))
    }
}

fn file_identifier(path: &std::path::Path) -> Result<String, &'static str> {
    url::Url::from_file_path(path)
        .map(String::from)
        .map_err(|_| "invalid native file identity")
}

fn check_active(session: &session::Session, selected: &Active) -> Result<(), session::Error> {
    use session::Error::Selection;
    let active = session
        .files()
        .get(&selected.document)
        .ok_or(Selection("active editor is not a selected text source"))?;
    if active.version() != selected.version
        || Sha256Digest::of(active.text().as_bytes()) != selected.source_sha256
        || !active.text().is_char_boundary(selected.offset)
    {
        return Err(Selection(
            "active editor digest, version or UTF-8 cursor does not match",
        ));
    }
    Ok(())
}
