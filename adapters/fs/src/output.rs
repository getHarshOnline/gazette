//! Exact local output effects through held directories, with explicit partial failures.

use std::{
    fs::{File, Metadata},
    io::Write,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

use gazette_core::{
    destination::{Error, OutputMutation, OutputObservation, OutputWriteError},
    source::{Content, EntryPath, Error as SourceError},
};
use gazette_ports::destination::OutputWriter;
use rustix::fs::{self, AtFlags, Mode, OFlags, RenameFlags};

use super::{
    FileSystem, ReadProfile,
    destination::{DestinationRoots, OutputAdministration},
    hash_file, io_error, read_directory_profile,
    stage::parent_and_name,
};

impl OutputWriter for FileSystem {
    fn write_output(
        &self,
        mutation: OutputMutation<'_>,
        allow_write: bool,
    ) -> Result<OutputObservation, OutputWriteError> {
        let mut effects = Effects::default();
        let result = if allow_write {
            materialize(&mutation, &mut effects)
        } else {
            Err(SourceError::InvalidInventory {
                path: mutation.root().to_owned(),
                reason: "explicit local output-write permission required",
            }
            .into())
        };
        result.map_err(|cause| OutputWriteError {
            cause,
            attempted: effects.attempted,
            residue: effects.residue,
        })
    }
}

#[derive(Default)]
struct Effects {
    attempted: Vec<EntryPath>,
    residue: Option<PathBuf>,
}

fn materialize(
    mutation: &OutputMutation<'_>,
    effects: &mut Effects,
) -> Result<OutputObservation, Error> {
    let roots = DestinationRoots::observe_output(mutation.root(), mutation.protected())?;
    let administration = OutputAdministration::observe(roots.directory())?;
    if &roots.binding(&administration)? != mutation.observed().binding() {
        return Err(SourceError::Changed(mutation.root().to_owned()).into());
    }
    let before = read_directory_profile(
        roots.directory(),
        mutation.limits(),
        None,
        ReadProfile::Output,
    )?;
    if &before != mutation.observed().inventory() {
        return Err(SourceError::Changed(mutation.root().to_owned()).into());
    }
    roots.recheck()?;
    administration.recheck(roots.directory())?;
    for change in mutation.changes() {
        roots.recheck()?;
        administration.recheck(roots.directory())?;
        let (parent, name) = parent_and_name(roots.directory(), &change.path)?;
        let original = match change.before {
            Some(content) => Some(verify_file(&parent, name, content)?),
            None => {
                match fs::statat(&parent, name, AtFlags::SYMLINK_NOFOLLOW) {
                    Err(rustix::io::Errno::NOENT) => (),
                    _ => return Err(SourceError::Changed(change.path.as_str().into()).into()),
                }
                None
            }
        };
        // Every later failure reports this path as possibly affected, even if
        // the syscall returned an error. No cleanup or retry is implicit.
        effects.attempted.push(change.path.clone());
        match change.after {
            Some(Content::Directory) => {
                fs::mkdirat(&parent, name, Mode::RUSR | Mode::WUSR | Mode::XUSR).map_err(
                    |error| {
                        io_error(
                            "create output directory",
                            Path::new(change.path.as_str()),
                            error,
                        )
                    },
                )?;
            }
            Some(content @ Content::File { .. }) => {
                let bytes = mutation
                    .file(&change.path)
                    .ok_or_else(|| SourceError::ContentMismatch(change.path.as_str().into()))?;
                let temporary = temporary_name()?;
                let mut file = File::from(
                    fs::openat(
                        &parent,
                        temporary.as_str(),
                        OFlags::WRONLY
                            | OFlags::CREATE
                            | OFlags::EXCL
                            | OFlags::NOFOLLOW
                            | OFlags::CLOEXEC,
                        Mode::RUSR | Mode::WUSR,
                    )
                    .map_err(|error| {
                        io_error(
                            "create exact output temporary",
                            Path::new(change.path.as_str()),
                            error,
                        )
                    })?,
                );
                effects.residue = Some(
                    roots
                        .canonical()
                        .join(change.path.as_str())
                        .with_file_name(&temporary),
                );
                let temporary_identity = file.metadata().map_err(|error| {
                    io_error(
                        "inspect output temporary",
                        Path::new(change.path.as_str()),
                        error,
                    )
                })?;
                file.write_all(bytes)
                    .and_then(|()| file.flush())
                    .and_then(|()| file.sync_all())
                    .map_err(|error| {
                        io_error(
                            "write and synchronize output temporary",
                            Path::new(change.path.as_str()),
                            error,
                        )
                    })?;
                // Hold the same native root/source/admin boundaries up to the
                // effect. The caller must exclusively control this namespace.
                roots.recheck()?;
                administration.recheck(roots.directory())?;
                check_parent(roots.directory(), &change.path, &parent)?;
                check_identity(&parent, &temporary, &temporary_identity)?;
                if let Some((_, metadata)) = &original {
                    check_identity(&parent, name, metadata)?;
                    exchange(&parent, &temporary, name)?;
                    let (_, displaced) = verify_file(
                        &parent,
                        &temporary,
                        change
                            .before
                            .ok_or_else(|| SourceError::Changed(change.path.as_str().into()))?,
                    )?;
                    if identity(&displaced) != identity(metadata) {
                        return Err(SourceError::Changed(change.path.as_str().into()).into());
                    }
                    check_identity(&parent, &temporary, &displaced)?;
                    fs::unlinkat(&parent, temporary.as_str(), AtFlags::empty()).map_err(
                        |error| {
                            io_error(
                                "remove verified prior output",
                                Path::new(change.path.as_str()),
                                error,
                            )
                        },
                    )?;
                } else {
                    move_absent(&parent, &temporary, name)?;
                }
                effects.residue = None;
                let (_, installed) = verify_file(&parent, name, content)?;
                if identity(&installed) != identity(&temporary_identity) {
                    return Err(SourceError::Changed(change.path.as_str().into()).into());
                }
            }
            None => {
                let (_, metadata) = original
                    .as_ref()
                    .ok_or_else(|| SourceError::Changed(change.path.as_str().into()))?;
                let temporary = temporary_name()?;
                roots.recheck()?;
                administration.recheck(roots.directory())?;
                check_parent(roots.directory(), &change.path, &parent)?;
                check_identity(&parent, name, metadata)?;
                move_absent(&parent, name, &temporary)?;
                effects.residue = Some(
                    roots
                        .canonical()
                        .join(change.path.as_str())
                        .with_file_name(&temporary),
                );
                let (_, displaced) = verify_file(
                    &parent,
                    &temporary,
                    change
                        .before
                        .ok_or_else(|| SourceError::Changed(change.path.as_str().into()))?,
                )?;
                if identity(&displaced) != identity(metadata) {
                    return Err(SourceError::Changed(change.path.as_str().into()).into());
                }
                check_identity(&parent, &temporary, &displaced)?;
                fs::unlinkat(&parent, temporary.as_str(), AtFlags::empty()).map_err(|error| {
                    io_error(
                        "remove verified obsolete output",
                        Path::new(change.path.as_str()),
                        error,
                    )
                })?;
                effects.residue = None;
            }
        }
        parent.sync_all().map_err(|error| {
            io_error(
                "synchronize output parent",
                Path::new(change.path.as_str()),
                error,
            )
        })?;
    }
    roots.recheck()?;
    administration.recheck(roots.directory())?;
    roots
        .directory()
        .sync_all()
        .map_err(|error| io_error("synchronize output root", mutation.root(), error))?;
    let after = read_directory_profile(
        roots.directory(),
        mutation.limits(),
        None,
        ReadProfile::Output,
    )?;
    if &after != mutation.expected() {
        return Err(SourceError::ContentMismatch(mutation.root().to_owned()).into());
    }
    roots.recheck()?;
    administration.recheck(roots.directory())?;
    Ok(OutputObservation::new(
        after,
        roots.binding(&administration)?,
    ))
}

fn verify_file(
    parent: &File,
    name: &str,
    expected: Content,
) -> Result<(File, Metadata), SourceError> {
    let Content::File { bytes, sha256 } = expected else {
        return Err(SourceError::Unsupported(name.into()));
    };
    let declared = fs::statat(parent, name, AtFlags::SYMLINK_NOFOLLOW)
        .map_err(|error| io_error("inspect output file", Path::new(name), error))?;
    let mut file = File::from(
        fs::openat(
            parent,
            name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| io_error("hold output file", Path::new(name), error))?,
    );
    let metadata = file
        .metadata()
        .map_err(|error| io_error("inspect held output file", Path::new(name), error))?;
    if !metadata.is_file()
        || identity(&metadata) != (declared.st_dev as u64, declared.st_ino as u64)
    {
        return Err(SourceError::Changed(name.into()));
    }
    let actual = hash_file(&mut file, Path::new(name), &metadata, bytes, None)?;
    if actual != (bytes, sha256) {
        return Err(SourceError::ContentMismatch(name.into()));
    }
    Ok((file, metadata))
}

fn identity(metadata: &Metadata) -> (u64, u64) {
    (metadata.dev(), metadata.ino())
}

fn check_parent(root: &File, path: &EntryPath, held: &File) -> Result<(), SourceError> {
    let (current, _) = parent_and_name(root, path)?;
    let before = held.metadata().map_err(|error| {
        io_error(
            "inspect held output parent",
            Path::new(path.as_str()),
            error,
        )
    })?;
    let after = current
        .metadata()
        .map_err(|error| io_error("recheck output parent", Path::new(path.as_str()), error))?;
    if identity(&before) != identity(&after) {
        return Err(SourceError::Changed(path.as_str().into()));
    }
    Ok(())
}

fn check_identity(parent: &File, name: &str, expected: &Metadata) -> Result<(), SourceError> {
    let current = fs::statat(parent, name, AtFlags::SYMLINK_NOFOLLOW)
        .map_err(|error| io_error("recheck output identity", Path::new(name), error))?;
    if identity(expected) != (current.st_dev as u64, current.st_ino as u64) {
        return Err(SourceError::Changed(name.into()));
    }
    Ok(())
}

fn temporary_name() -> Result<String, SourceError> {
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random).map_err(|error| {
        io_error(
            "choose output temporary",
            Path::new(""),
            std::io::Error::other(error.to_string()),
        )
    })?;
    Ok(format!(
        ".gazette-write-{}",
        random
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    ))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn rename(parent: &File, from: &str, to: &str, flags: RenameFlags) -> Result<(), SourceError> {
    fs::renameat_with(parent, from, parent, to, flags)
        .map_err(|error| io_error("install exact output transition", Path::new(to), error))
}
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn rename(_: &File, _: &str, to: &str, _: RenameFlags) -> Result<(), SourceError> {
    Err(SourceError::Unsupported(to.into()))
}
fn move_absent(parent: &File, from: &str, to: &str) -> Result<(), SourceError> {
    rename(parent, from, to, RenameFlags::NOREPLACE)
}
fn exchange(parent: &File, from: &str, to: &str) -> Result<(), SourceError> {
    rename(parent, from, to, RenameFlags::EXCHANGE)
}

#[cfg(test)]
#[path = "../tests/internal/output.rs"]
mod tests;
