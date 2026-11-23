//! Runs one explicitly selected command as a direct child with bounded input, for the
//! operations that need an external program.
//!
//! This adapter builds on `gazette-core` and the capability traits in `gazette-ports`;
//! `gazette-sdk` builds on it. Neither cleared environment nor an explicit working
//! directory is a sandbox. Native spawn itself is not interruptible here; the deadline
//! begins once spawn returns. Failure cleanup allows at most another second for
//! direct-child reaping and never certifies descendants.

#[cfg(unix)]
mod executable;
#[cfg(unix)]
mod unix;

pub mod social_card;

use gazette_core::process::{
    CaptureRequest, Captured, Error, Exit, IsolatedCaptureRequest, Request,
};
use gazette_core::{
    process::{Cleanup, Failure},
    source::{Content, EntryPath, TreeInput},
};
use std::{
    collections::BTreeSet,
    fs,
    fs::{File, Metadata},
    io::{self, Read},
    path::{Path, PathBuf},
};

#[cfg(unix)]
use std::{ffi::OsStr, os::unix::ffi::OsStrExt, os::unix::fs::MetadataExt};

#[cfg(unix)]
use rtepl_filesystem::unix::FileIdentity;

#[cfg(unix)]
use rustix::fs::{Dir, Mode, OFlags, open, openat};

/// Native command runner, without shell expansion or inherited environment.
/// Optional executable pins add bounded file reads before spawn and after reaping;
/// those filesystem observations are not covered by the child-execution deadline.
pub struct Runner;

impl gazette_ports::process::Runner for Runner {
    fn run(&self, request: Request<'_>) -> Result<Exit, Error> {
        request.options.validate(request.command, request.input)?;
        #[cfg(unix)]
        {
            unix::execute(request, None, None).map(|captured| captured.exit)
        }
        #[cfg(not(unix))]
        {
            Err(Error::before_start(
                gazette_core::process::Failure::Unsupported,
            ))
        }
    }
}

impl gazette_ports::process::CaptureRunner for Runner {
    fn capture_segmented(
        &self,
        request: gazette_core::process::SegmentedCaptureRequest<'_>,
    ) -> Result<Captured, Error> {
        request.validate()?;
        #[cfg(unix)]
        {
            unix::execute_segments(
                request.command,
                request.segments,
                request.timeout,
                request.max_output_bytes,
                request.max_stderr_bytes,
            )
        }
        #[cfg(not(unix))]
        {
            Err(Error::before_start(Failure::Unsupported))
        }
    }

    fn capture(&self, request: CaptureRequest<'_>) -> Result<Captured, Error> {
        request.validate()?;
        #[cfg(unix)]
        {
            unix::execute(
                request.request,
                Some(request.max_output_bytes),
                request.max_stderr_bytes,
            )
        }
        #[cfg(not(unix))]
        {
            Err(Error::before_start(
                gazette_core::process::Failure::Unsupported,
            ))
        }
    }
}

impl gazette_ports::process::IsolatedCaptureRunner for Runner {
    fn capture_isolated(&self, request: IsolatedCaptureRequest<'_>) -> Result<Captured, Error> {
        request.validate()?;
        let temporary = tempfile::Builder::new()
            .prefix("gazette-isolated-command-")
            .tempdir()
            .map_err(resource_error)?;
        let directory = temporary
            .path()
            .to_str()
            .ok_or_else(|| resource_error(io::Error::other("temporary path is not UTF-8")))?;
        let command = request.request.command.as_command(directory);
        let captured = <Self as gazette_ports::process::CaptureRunner>::capture(
            self,
            CaptureRequest {
                request: Request {
                    command: &command,
                    input: request.request.input,
                    options: request.request.options,
                },
                max_output_bytes: request.max_output_bytes,
                max_stderr_bytes: request.max_stderr_bytes,
            },
        );
        let verification = verify_empty(temporary.path());
        let cleanup = temporary.close();
        match captured {
            Err(mut error) => {
                if let Err(source) = cleanup {
                    error.cleanup = Cleanup::Uncertain;
                    error.source = Some(source);
                }
                Err(error)
            }
            Ok(captured) => match (verification, cleanup) {
                (Ok(()), Ok(())) => Ok(captured),
                (Err(source), Ok(())) => Err(completed_resource_error(source)),
                (_, Err(source)) => Err(Error {
                    failure: Failure::Command,
                    cleanup: Cleanup::Uncertain,
                    source: Some(source),
                }),
            },
        }
    }
}

impl gazette_ports::process::RetainedCaptureRunner for Runner {
    fn capture_retained(
        &self,
        request: CaptureRequest<'_>,
        resources: TreeInput<'_>,
        entrypoint: &EntryPath,
    ) -> Result<Captured, Error> {
        if resources.file(entrypoint).is_none() {
            return Err(Error::before_start(Failure::Command));
        }
        let temporary = tempfile::Builder::new()
            .prefix("gazette-math-resource-")
            .tempdir()
            .map_err(resource_error)?;
        materialize(temporary.path(), &resources).map_err(resource_error)?;
        let entrypoint = temporary.path().join(entrypoint.as_str());
        let entrypoint = entrypoint
            .to_str()
            .ok_or_else(|| resource_error(io::Error::other("temporary path is not UTF-8")))?;
        let mut command = request.request.command.clone();
        command.arguments.insert(0, entrypoint.to_owned());
        let rebound = CaptureRequest {
            request: Request {
                command: &command,
                input: request.request.input,
                options: request.request.options,
            },
            max_output_bytes: request.max_output_bytes,
            max_stderr_bytes: request.max_stderr_bytes,
        };
        let captured = <Self as gazette_ports::process::CaptureRunner>::capture(self, rebound);
        let verification = verify(temporary.path(), &resources);
        let cleanup = temporary.close();
        if let Err(source) = cleanup {
            return Err(Error {
                failure: Failure::Command,
                cleanup: Cleanup::Uncertain,
                source: Some(source),
            });
        }
        verification.map_err(completed_resource_error)?;
        captured
    }
}

fn resource_error(source: io::Error) -> Error {
    Error {
        failure: Failure::Command,
        cleanup: Cleanup::NotStarted,
        source: Some(source),
    }
}

fn completed_resource_error(source: io::Error) -> Error {
    Error {
        failure: Failure::Command,
        cleanup: Cleanup::Reaped,
        source: Some(source),
    }
}

fn materialize(root: &Path, input: &TreeInput<'_>) -> io::Result<()> {
    for entry in input.inventory().entries() {
        let path = root.join(entry.path.as_str());
        match entry.content {
            Content::Directory => fs::create_dir(&path)?,
            Content::File { .. } => {
                let bytes = input
                    .file(&entry.path)
                    .ok_or_else(|| io::Error::other("retained resource file is absent"))?;
                fs::write(path, bytes)?;
            }
        }
    }
    Ok(())
}

fn verify(root: &Path, input: &TreeInput<'_>) -> io::Result<()> {
    #[cfg(unix)]
    {
        verify_unix(root, input)
    }
    #[cfg(not(unix))]
    {
        let _ = (root, input);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "retained resource verification requires no-follow file descriptors",
        ))
    }
}

#[cfg(unix)]
fn verify_unix(root: &Path, input: &TreeInput<'_>) -> io::Result<()> {
    let expected = input
        .inventory()
        .entries()
        .iter()
        .map(|entry| entry.path.as_str().to_owned())
        .collect::<BTreeSet<_>>();
    let root_directory = File::from(open(
        root,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?);
    let root_identity = root_directory.metadata()?;
    let mut observed = BTreeSet::new();
    let mut visited = 0usize;
    observe(
        &root_directory,
        Path::new(""),
        &mut observed,
        &mut visited,
        expected.len(),
    )?;
    if observed != expected {
        return Err(io::Error::other(
            "retained resource tree changed during execution",
        ));
    }
    for entry in input.inventory().entries() {
        match entry.content {
            Content::Directory => {
                let directory = open_beneath(&root_directory, &entry.path, true)?;
                if !directory.metadata()?.is_dir() {
                    return Err(shape_changed());
                }
            }
            Content::File { .. } => {
                let expected = input
                    .file(&entry.path)
                    .ok_or_else(|| io::Error::other("retained resource file is absent"))?;
                verify_file(&root_directory, &entry.path, expected)?;
            }
        }
    }
    let rebound_root = File::from(open(
        root,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?);
    if !same_node(&root_identity, &root_directory.metadata()?)
        || !same_node(&root_identity, &rebound_root.metadata()?)
    {
        return Err(changed_resource());
    }
    Ok(())
}

#[cfg(unix)]
fn verify_file(root: &File, relative: &EntryPath, expected: &[u8]) -> io::Result<()> {
    let expected_length = u64::try_from(expected.len())
        .map_err(|_| io::Error::other("retained resource length exceeds native limits"))?;
    let mut file = open_beneath(root, relative, false)?;
    let opened = file.metadata()?;
    if !opened.is_file() || opened.len() != expected_length {
        return Err(changed_resource());
    }

    let limit = expected_length
        .checked_add(1)
        .ok_or_else(|| io::Error::other("retained resource length exceeds native limits"))?;
    let mut reader = (&mut file).take(limit);
    let mut buffer = [0u8; 16 * 1024];
    let mut offset = 0usize;
    loop {
        let count = match reader.read(&mut buffer) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if count == 0 {
            break;
        }
        let end = offset
            .checked_add(count)
            .ok_or_else(|| io::Error::other("retained resource length exceeds native limits"))?;
        if expected.get(offset..end) != Some(&buffer[..count]) {
            return Err(changed_resource());
        }
        offset = end;
    }
    if offset != expected.len() || !same_file(&opened, &file.metadata()?) {
        return Err(changed_resource());
    }

    // Re-resolve every component without following links. This binds the held file back to the
    // selected retained name after reading and detects final- or ancestor-component replacement.
    let rebound = open_beneath(root, relative, false)?;
    if !same_file(&opened, &rebound.metadata()?) {
        return Err(changed_resource());
    }
    Ok(())
}

#[cfg(unix)]
fn open_beneath(root: &File, relative: &EntryPath, directory_target: bool) -> io::Result<File> {
    let directory_flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut directory = root.try_clone()?;
    let mut components = relative.as_str().split('/').peekable();
    while let Some(component) = components.next() {
        if components.peek().is_none() {
            let target_flags = if directory_target {
                directory_flags
            } else {
                OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC
            };
            return Ok(File::from(openat(
                &directory,
                component,
                target_flags,
                Mode::empty(),
            )?));
        }
        directory = File::from(openat(
            &directory,
            component,
            directory_flags,
            Mode::empty(),
        )?);
    }
    Err(io::Error::other("retained resource path is empty"))
}

#[cfg(unix)]
pub(crate) fn same_file(before: &Metadata, after: &Metadata) -> bool {
    before.is_file() && after.is_file() && same_node(before, after)
}

#[cfg(unix)]
fn same_node(before: &Metadata, after: &Metadata) -> bool {
    before.is_file() == after.is_file()
        && before.is_dir() == after.is_dir()
        && before.file_type().is_symlink() == after.file_type().is_symlink()
        && FileIdentity::from(before) == FileIdentity::from(after)
        && before.uid() == after.uid()
        && before.gid() == after.gid()
        && before.nlink() == after.nlink()
}

fn changed_resource() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "retained resource changed during execution",
    )
}

fn shape_changed() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "retained resource shape changed during execution",
    )
}

fn verify_empty(root: &Path) -> io::Result<()> {
    if fs::read_dir(root)?.next().transpose()?.is_some() {
        return Err(io::Error::other(
            "isolated command changed its disposable working directory",
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn observe(
    directory: &File,
    relative: &Path,
    observed: &mut BTreeSet<String>,
    visited: &mut usize,
    maximum: usize,
) -> io::Result<()> {
    let identity = directory.metadata()?;
    if !identity.is_dir() {
        return Err(shape_changed());
    }
    let mut entries = Dir::read_from(directory)?;
    while let Some(child) = entries.read() {
        let child = child?;
        let name = child.file_name().to_bytes();
        if name == b"." || name == b".." {
            continue;
        }
        *visited = visited
            .checked_add(1)
            .ok_or_else(|| io::Error::other("retained resource tree exceeds native limits"))?;
        if *visited > maximum {
            return Err(io::Error::other(
                "retained resource tree changed during execution",
            ));
        }
        let name = OsStr::from_bytes(name);
        let child_relative = relative.join(name);
        let portable = portable_path(&child_relative)?;
        let held = File::from(openat(
            directory,
            name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )?);
        let child_identity = held.metadata()?;
        if !(child_identity.is_file() || child_identity.is_dir()) {
            return Err(io::Error::other(
                "retained resource tree contains a special file",
            ));
        }
        observed.insert(portable);
        if child_identity.is_dir() {
            observe(&held, &child_relative, observed, visited, maximum)?;
        }
        let rebound = File::from(openat(
            directory,
            name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )?);
        if !same_node(&child_identity, &held.metadata()?)
            || !same_node(&child_identity, &rebound.metadata()?)
        {
            return Err(changed_resource());
        }
    }
    if !same_node(&identity, &directory.metadata()?) {
        return Err(changed_resource());
    }
    Ok(())
}

fn portable_path(path: &Path) -> io::Result<String> {
    let mut result = PathBuf::new();
    for component in path.components() {
        let component = component
            .as_os_str()
            .to_str()
            .ok_or_else(|| io::Error::other("retained resource path is not UTF-8"))?;
        result.push(component);
    }
    Ok(result
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/"))
}
