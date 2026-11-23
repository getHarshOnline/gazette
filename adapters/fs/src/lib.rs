//! Gives Gazette its local files on Unix: reading selected sources, and writing
//! destinations and fresh private stages.
//!
//! This adapter builds on `gazette-core` and the capability traits in `gazette-ports`;
//! `gazette-sdk` builds on it. It realizes the source, destination and staging ports.
//! Descendants are opened relative to held directory handles without following
//! symlinks. Detected changes fail; this is not an atomic snapshot.

#[cfg(not(unix))]
compile_error!("Gazette's current filesystem adapter requires Unix directory-handle operations");

use std::collections::BTreeMap;
use std::fs::{File, Metadata};
use std::io::{self, Read};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use gazette_core::source::{
    CapturedSource, Content, EntryPath, Error, Inventory, InventoryBuilder, Limits, Sha256Digest,
    Subdirectory, entry_name,
};
use gazette_ports::source::{SelectedSourceCapture, SourceCapture, SourceReader};
use rustix::fs::{self, AtFlags, Dir, FileType, Mode, OFlags};
use sha2::{Digest, Sha256};

mod destination;
mod listing;
mod output;
mod selection;
mod walk;
use selection::Selection;
mod stage;
mod synchronization;
pub use gazette_core::stage::RetainedStage;
pub use stage::{PlannedStage, StagedTree};

/// Local directory-handle adapter; holds no ambient workspace or state.
#[derive(Clone, Copy, Debug, Default)]
pub struct FileSystem;

impl SourceReader for FileSystem {
    fn read(
        &self,
        root: &Path,
        subdirectory: Option<&Subdirectory>,
        limits: Limits,
    ) -> Result<Inventory, Error> {
        read_source(root, subdirectory, limits, None)
    }
}

impl SourceCapture for FileSystem {
    fn list_filtered(
        &self,
        root: &Path,
        subdirectory: Option<&Subdirectory>,
        filter: &dyn gazette_ports::source::SourceFilter,
        limits: gazette_core::source::ListingLimits,
    ) -> Result<gazette_core::source::Listing, Error> {
        let directory = open_selected_directory(root, subdirectory)?;
        listing::list(
            &directory,
            Selection::Filtered {
                exclude: &[],
                filter,
            },
            limits,
        )
    }

    fn capture_filtered(
        &self,
        root: &Path,
        subdirectory: Option<&Subdirectory>,
        exclude: &[EntryPath],
        filter: &dyn gazette_ports::source::SourceFilter,
        limits: Limits,
    ) -> Result<CapturedSource, Error> {
        if !exclude.is_empty() {
            selection::validate(root, exclude)?;
        }
        let directory = open_selected_directory(root, subdirectory)?;
        let mut files = BTreeMap::new();
        let mut inventory = InventoryBuilder::new(limits);
        visit(
            &directory,
            "",
            &mut inventory,
            Some(&mut files),
            ReadProfile::Source,
            None,
            Selection::Filtered { exclude, filter },
        )?;
        CapturedSource::from_parts(inventory.finish(), files)
    }

    fn capture(
        &self,
        root: &Path,
        subdirectory: Option<&Subdirectory>,
        limits: Limits,
    ) -> Result<CapturedSource, Error> {
        let mut files = BTreeMap::new();
        let inventory = read_source(root, subdirectory, limits, Some(&mut files))?;
        CapturedSource::from_parts(inventory, files)
    }
    fn capture_including(
        &self,
        root: &Path,
        include: &[EntryPath],
        limits: Limits,
    ) -> Result<CapturedSource, Error> {
        selection::validate(root, include)?;
        let directory = open_directory(root)?;
        let mut files = BTreeMap::new();
        let mut inventory = InventoryBuilder::new(limits);
        visit(
            &directory,
            "",
            &mut inventory,
            Some(&mut files),
            ReadProfile::Source,
            None,
            Selection::Including(include),
        )?;
        let inventory = inventory.finish();
        for selected in include {
            if inventory
                .entries()
                .binary_search_by(|entry| entry.path.cmp(selected))
                .is_err()
            {
                return Err(Error::InvalidInventory {
                    path: PathBuf::from(selected.as_str()),
                    reason: "required source selection is missing",
                });
            }
        }
        CapturedSource::from_parts(inventory, files)
    }

    fn capture_excluding(
        &self,
        root: &Path,
        exclude: &[EntryPath],
        limits: Limits,
    ) -> Result<CapturedSource, Error> {
        selection::validate(root, exclude)?;
        let directory = open_directory(root)?;
        let mut files = BTreeMap::new();
        let mut inventory = InventoryBuilder::new(limits);
        visit(
            &directory,
            "",
            &mut inventory,
            Some(&mut files),
            ReadProfile::Source,
            None,
            Selection::Excluding(exclude),
        )?;
        CapturedSource::from_parts(inventory.finish(), files)
    }
}

impl SelectedSourceCapture for FileSystem {
    fn capture_selected(
        &self,
        root: &Path,
        paths: &[EntryPath],
        limits: Limits,
    ) -> Result<CapturedSource, Error> {
        let directory = open_directory(root)?;
        let before = directory
            .metadata()
            .map_err(|error| io_error("inspect directory", root, error))?;
        let mut selected = paths.to_vec();
        selected.sort();
        if let Some(path) = selected
            .windows(2)
            .find_map(|pair| (pair[0] == pair[1]).then(|| pair[0].clone()))
        {
            return Err(Error::InvalidInventory {
                path: PathBuf::from(path.as_str()),
                reason: "duplicate selected path",
            });
        }
        let mut inventory = InventoryBuilder::new(limits);
        let mut files = BTreeMap::new();
        let mut directories = std::collections::BTreeSet::new();
        let mut held_files = Vec::new();
        for path in &selected {
            read_selected_file(
                &directory,
                path,
                &mut inventory,
                &mut directories,
                &mut files,
                &mut held_files,
            )?;
        }
        for (file, before, path) in held_files {
            let after = file
                .metadata()
                .map_err(|error| io_error("recheck selected file", &path, error))?;
            if changed(&before, &after) {
                return Err(Error::Changed(path));
            }
        }
        let after = directory
            .metadata()
            .map_err(|error| io_error("recheck directory", root, error))?;
        if changed(&before, &after) {
            return Err(Error::Changed(root.to_owned()));
        }
        CapturedSource::from_parts(inventory.finish(), files)
    }
}

fn read_selected_file(
    root: &File,
    path: &EntryPath,
    inventory: &mut InventoryBuilder,
    directories: &mut std::collections::BTreeSet<EntryPath>,
    files: &mut BTreeMap<EntryPath, Vec<u8>>,
    held_files: &mut Vec<(File, Metadata, PathBuf)>,
) -> Result<(), Error> {
    let mut directory = root
        .try_clone()
        .map_err(|error| io_error("clone root directory", Path::new(""), error))?;
    let components: Vec<_> = path.as_str().split('/').collect();
    let mut prefix = String::new();
    let mut held = Vec::new();
    for (index, component) in components.iter().enumerate() {
        if !prefix.is_empty() {
            prefix.push('/');
        }
        prefix.push_str(component);
        let relative: EntryPath = prefix.parse()?;
        require_exact_entry_name(&directory, component, Path::new(&prefix))?;
        let declared = fs::statat(&directory, *component, AtFlags::SYMLINK_NOFOLLOW)
            .map_err(|error| io_error("inspect selected entry", Path::new(&prefix), error))?;
        let kind = FileType::from_raw_mode(declared.st_mode);
        let last = index + 1 == components.len();
        let expected = if last {
            FileType::RegularFile
        } else {
            FileType::Directory
        };
        if kind != expected {
            return Err(Error::Unsupported(PathBuf::from(&prefix)));
        }
        let flags = OFlags::RDONLY
            | OFlags::NOFOLLOW
            | OFlags::CLOEXEC
            | OFlags::NONBLOCK
            | if last {
                OFlags::empty()
            } else {
                OFlags::DIRECTORY
            };
        let opened = fs::openat(&directory, *component, flags, Mode::empty())
            .map_err(|error| io_error("open selected entry", Path::new(&prefix), error))?;
        let mut opened = File::from(opened);
        let metadata = opened.metadata().map_err(|error| {
            io_error("inspect opened selected entry", Path::new(&prefix), error)
        })?;
        if metadata.dev() != declared.st_dev as u64 || metadata.ino() != declared.st_ino as u64 {
            return Err(Error::Changed(PathBuf::from(&prefix)));
        }
        if last {
            let ceiling = inventory.file_ceiling();
            let mut contents = Vec::new();
            let (bytes, sha256) = hash_file(
                &mut opened,
                Path::new(path.as_str()),
                &metadata,
                ceiling,
                Some(&mut contents),
            )?;
            inventory.insert(relative.clone(), Content::File { bytes, sha256 })?;
            files.insert(relative, contents);
            held_files.push((opened, metadata, PathBuf::from(&prefix)));
        } else {
            if directories.insert(relative.clone()) {
                inventory.insert(relative, Content::Directory)?;
            }
            held.push((
                opened.try_clone().map_err(|error| {
                    io_error("retain selected directory", Path::new(&prefix), error)
                })?,
                metadata,
                PathBuf::from(&prefix),
            ));
            directory = opened;
        }
    }
    for (directory, before, path) in held {
        let after = directory
            .metadata()
            .map_err(|error| io_error("recheck selected directory", &path, error))?;
        if changed(&before, &after) {
            return Err(Error::Changed(path));
        }
    }
    Ok(())
}

fn require_exact_entry_name(directory: &File, component: &str, path: &Path) -> Result<(), Error> {
    let scan = fs::openat(
        directory,
        ".",
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map(File::from)
    .map_err(|error| io_error("open selected parent", path, error))?;
    let entries =
        Dir::read_from(&scan).map_err(|error| io_error("list selected parent", path, error))?;
    for entry in entries {
        let entry = entry.map_err(|error| io_error("read selected parent", path, error))?;
        if entry.file_name().to_bytes() == component.as_bytes() {
            return Ok(());
        }
    }
    Err(Error::InvalidName(path.to_owned()))
}

fn read_source(
    root: &Path,
    subdirectory: Option<&Subdirectory>,
    limits: Limits,
    files: Option<&mut BTreeMap<EntryPath, Vec<u8>>>,
) -> Result<Inventory, Error> {
    let directory = open_selected_directory(root, subdirectory)?;
    read_directory(&directory, limits, files)
}

fn open_selected_directory(
    root: &Path,
    subdirectory: Option<&Subdirectory>,
) -> Result<File, Error> {
    let mut directory = open_directory(root)?;
    if let Some(subdirectory) = subdirectory {
        let mut relative = PathBuf::new();
        for component in subdirectory.components() {
            relative.push(component);
            let fd = fs::openat(
                &directory,
                component,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(|e| io_error("open selected subdirectory", &relative, e))?;
            directory = File::from(fd);
        }
    }
    Ok(directory)
}

fn open_directory(root: &Path) -> Result<File, Error> {
    // Remove trailing separators and `.` so they cannot turn a root symlink
    // into an intermediate component exempt from NOFOLLOW.
    let root: PathBuf = root.components().collect();
    let fd = fs::open(
        &root,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|e| io_error("open directory", &root, e))?;
    Ok(File::from(fd))
}

fn read_directory(
    directory: &File,
    limits: Limits,
    files: Option<&mut BTreeMap<EntryPath, Vec<u8>>>,
) -> Result<Inventory, Error> {
    read_directory_profile(directory, limits, files, ReadProfile::Source)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ReadProfile {
    Source,
    Worktree,
    Output,
}

fn read_directory_profile(
    directory: &File,
    limits: Limits,
    files: Option<&mut BTreeMap<EntryPath, Vec<u8>>>,
    profile: ReadProfile,
) -> Result<Inventory, Error> {
    let mut inventory = InventoryBuilder::new(limits);
    visit(
        directory,
        "",
        &mut inventory,
        files,
        profile,
        None,
        Selection::Excluding(&[]),
    )?;
    Ok(inventory.finish())
}
fn io_error(operation: &'static str, path: &Path, source: impl Into<io::Error>) -> Error {
    Error::Io {
        operation,
        path: path.to_owned(),
        source: source.into(),
    }
}

fn visit(
    directory: &File,
    prefix: &str,
    inventory: &mut InventoryBuilder,
    files: Option<&mut BTreeMap<EntryPath, Vec<u8>>>,
    profile: ReadProfile,
    observed: Option<&mut BTreeMap<EntryPath, Metadata>>,
    selection: Selection<'_>,
) -> Result<(), Error> {
    walk::walk(
        directory,
        prefix,
        &mut CaptureVisitor { inventory, files },
        profile,
        observed,
        selection,
    )
}

struct CaptureVisitor<'a> {
    inventory: &'a mut InventoryBuilder,
    files: Option<&'a mut BTreeMap<EntryPath, Vec<u8>>>,
}

impl walk::Visitor for CaptureVisitor<'_> {
    fn check_path(&self, path: &EntryPath) -> Result<(), Error> {
        self.inventory.check_path(path)
    }

    fn directory(&mut self, path: EntryPath) -> Result<(), Error> {
        self.inventory.insert(path, Content::Directory)
    }

    fn file(&mut self, path: EntryPath, file: &mut File, metadata: &Metadata) -> Result<(), Error> {
        let mut contents = Vec::new();
        let retain = self.files.as_ref().map(|_| &mut contents);
        let (bytes, sha256) = hash_file(
            file,
            Path::new(path.as_str()),
            metadata,
            self.inventory.file_ceiling(),
            retain,
        )?;
        if let Some(files) = self.files.as_deref_mut() {
            files.insert(path.clone(), contents);
        }
        self.inventory.insert(path, Content::File { bytes, sha256 })
    }
}

fn hash_file(
    file: &mut File,
    path: &Path,
    before: &Metadata,
    ceiling: u64,
    mut contents: Option<&mut Vec<u8>>,
) -> Result<(u64, Sha256Digest), Error> {
    let limit_error = || Error::Limit {
        path: path.to_owned(),
        resource: "file or total bytes",
    };
    if before.len() > ceiling {
        return Err(limit_error());
    }
    let mut hash = Sha256::new();
    let mut bytes = 0_u64;
    // Stream content; at the ceiling probe only one byte to detect growth.
    let mut buffer = [0_u8; 32 * 1024];
    loop {
        let request = ceiling
            .saturating_sub(bytes)
            .saturating_add(1)
            .min(buffer.len() as u64) as usize;
        let count = match file.read(&mut buffer[..request]) {
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            result => result.map_err(|e| io_error("read file", path, e))?,
        };
        if count == 0 {
            break;
        }
        bytes = bytes.checked_add(count as u64).ok_or_else(limit_error)?;
        if bytes > ceiling {
            return Err(limit_error());
        }
        hash.update(&buffer[..count]);
        if let Some(contents) = contents.as_deref_mut() {
            contents.extend_from_slice(&buffer[..count]);
        }
    }
    let after = file
        .metadata()
        .map_err(|e| io_error("recheck file", path, e))?;
    if bytes != before.len() || changed(before, &after) {
        return Err(Error::Changed(path.to_owned()));
    }
    let digest: [u8; 32] = hash.finalize().into();
    Ok((bytes, digest.into()))
}

fn changed(before: &Metadata, after: &Metadata) -> bool {
    before.dev() != after.dev()
        || before.ino() != after.ino()
        || before.len() != after.len()
        || before.mtime() != after.mtime()
        || before.mtime_nsec() != after.mtime_nsec()
        || before.ctime() != after.ctime()
        || before.ctime_nsec() != after.ctime_nsec()
}

#[cfg(test)]
#[path = "../tests/internal/source.rs"]
mod tests;
