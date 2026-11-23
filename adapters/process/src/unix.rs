//! Nonblocking input, optional output capture and status polling share one deadline.

use std::{
    io::{self, Read, Write},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use gazette_core::process::{Captured, Cleanup, Error, Exit, Failure, Request};
use rustix::fs::{OFlags, fcntl_getfl, fcntl_setfl};

const TICK: Duration = Duration::from_millis(1);
const REAP_GRACE: Duration = Duration::from_secs(1);

pub(super) fn execute(
    request: Request<'_>,
    maximum: Option<usize>,
    stderr_maximum: Option<usize>,
) -> Result<Captured, Error> {
    execute_segments(
        request.command,
        &[request.input],
        request.options.timeout,
        maximum,
        stderr_maximum,
    )
}

pub(super) fn execute_segments(
    selected: &gazette_core::process::Command,
    segments: &[&[u8]],
    timeout: Duration,
    maximum: Option<usize>,
    stderr_maximum: Option<usize>,
) -> Result<Captured, Error> {
    let mut input = Segments {
        parts: segments,
        offset: 0,
    };

    let mut executable = selected
        .executable
        .as_ref()
        .map(|pin| super::executable::Executable::open(&selected.program, pin))
        .transpose()
        .map_err(|source| Error {
            failure: Failure::Executable,
            cleanup: Cleanup::NotStarted,
            source: Some(source),
        })?;
    let mut child = Command::new(&selected.program)
        .args(&selected.arguments)
        .current_dir(&selected.directory)
        .env_clear()
        .envs(&selected.environment)
        .stdin(Stdio::piped())
        .stdout(if maximum.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stderr(if stderr_maximum.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .spawn()
        .map_err(|source| Error {
            failure: Failure::Spawn,
            cleanup: Cleanup::NotStarted,
            source: Some(source),
        })?;
    let deadline = Instant::now() + timeout;
    let Some(stdin) = child.stdin.take() else {
        return Err(failed(&mut child, Failure::Input, None));
    };
    let configured =
        fcntl_getfl(&stdin).and_then(|flags| fcntl_setfl(&stdin, flags | OFlags::NONBLOCK));
    if let Err(error) = configured {
        drop(stdin);
        return Err(failed(&mut child, Failure::Input, Some(error.into())));
    }
    let mut stdin = Some(stdin);
    let mut stdout = Output::new(child.stdout.take(), maximum)
        .map_err(|source| failed(&mut child, Failure::Output, Some(source)))?;
    let mut stderr = Output::new(child.stderr.take(), stderr_maximum)
        .map_err(|source| failed(&mut child, Failure::Output, Some(source)))?;
    let mut status = None;
    loop {
        // Close the only parent writer promptly, including for empty input. Successful writes
        // prove transport delivery, not that a checker interpreted or accepted the payload.
        if input.remaining().is_empty() {
            drop(stdin.take());
        }
        if Instant::now() >= deadline {
            drop(stdin);
            return Err(failed(&mut child, Failure::Timeout, None));
        }
        if status.is_none() {
            match child.try_wait() {
                Ok(Some(exit_status)) => {
                    if !input.remaining().is_empty() {
                        return Err(Error {
                            failure: Failure::Input,
                            cleanup: Cleanup::Reaped,
                            source: None,
                        });
                    }
                    // Keep draining after exit: an inherited descendant pipe still requires EOF.
                    status = Some(exit_status);
                }
                Ok(None) => {}
                Err(source) => {
                    drop(stdin);
                    return Err(failed(&mut child, Failure::Wait, Some(source)));
                }
            }
        }
        if let Some(status) = status.as_ref()
            && stdout.reader.is_none()
            && stderr.reader.is_none()
        {
            if let Some(executable) = executable.as_mut() {
                executable.verify().map_err(|source| Error {
                    failure: Failure::ExecutableChanged,
                    cleanup: Cleanup::Reaped,
                    source: Some(source),
                })?;
            }
            return Ok(Captured {
                exit: Exit {
                    code: status.code(),
                },
                stdout: stdout.bytes,
                stderr: stderr_maximum.map(|_| stderr.bytes),
            });
        }
        let mut progress = stdout
            .drain()
            .map_err(|(failure, source)| failed(&mut child, failure, source))?;
        progress |= stderr
            .drain()
            .map_err(|(failure, source)| failed(&mut child, failure, source))?;
        if let Some(writer) = stdin.as_mut() {
            match writer.write(input.remaining()) {
                Ok(0) => {
                    drop(stdin);
                    return Err(failed(&mut child, Failure::Input, None));
                }
                Ok(count) => {
                    input.offset += count;
                    progress = true;
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => progress = true,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Err(source) => {
                    drop(stdin);
                    return Err(failed(&mut child, Failure::Input, Some(source)));
                }
            }
        }
        if !progress {
            pause(deadline);
        }
    }
}

struct Segments<'a> {
    parts: &'a [&'a [u8]],
    offset: usize,
}

impl Segments<'_> {
    fn remaining(&mut self) -> &[u8] {
        while self
            .parts
            .first()
            .is_some_and(|part| self.offset == part.len())
        {
            self.parts = &self.parts[1..];
            self.offset = 0;
        }
        self.parts.first().map_or(&[], |part| &part[self.offset..])
    }
}

/// One shared bounded reader for either native output stream. Each poll reads at most one chunk,
/// so a noisy stream cannot starve its sibling, stdin delivery or the shared deadline.
struct Output<R> {
    reader: Option<R>,
    maximum: usize,
    bytes: Vec<u8>,
}

impl<R: Read + std::os::fd::AsFd> Output<R> {
    fn new(reader: Option<R>, maximum: Option<usize>) -> io::Result<Self> {
        if maximum.is_some() != reader.is_some() {
            return Err(io::Error::other(
                "selected native output pipe is unavailable",
            ));
        }
        if let Some(reader) = reader.as_ref() {
            let flags = fcntl_getfl(reader)?;
            fcntl_setfl(reader, flags | OFlags::NONBLOCK)?;
        }
        Ok(Self {
            reader,
            maximum: maximum.unwrap_or(0),
            bytes: Vec::new(),
        })
    }

    fn drain(&mut self) -> Result<bool, (Failure, Option<io::Error>)> {
        let Some(reader) = self.reader.as_mut() else {
            return Ok(false);
        };
        let mut buffer = [0u8; 8192];
        let available = self.maximum.saturating_sub(self.bytes.len());
        let bound = buffer.len().min(available.saturating_add(1));
        match reader.read(&mut buffer[..bound]) {
            Ok(0) => {
                drop(self.reader.take());
                Ok(true)
            }
            Ok(count) if count > available => Err((Failure::OutputLimit, None)),
            Ok(count) => {
                self.bytes.extend_from_slice(&buffer[..count]);
                Ok(true)
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(false),
            Err(source) => Err((Failure::Output, Some(source))),
        }
    }
}

fn pause(deadline: Instant) {
    thread::sleep(TICK.min(deadline.saturating_duration_since(Instant::now())));
}

fn failed(child: &mut Child, failure: Failure, source: Option<io::Error>) -> Error {
    let cleanup = match child.try_wait() {
        Ok(Some(_)) => Cleanup::Reaped,
        // An external reaper or changed SIGCHLD handling can invalidate PID custody. Without
        // a successful live-child observation, do not signal a potentially recycled PID.
        Err(_) => Cleanup::Uncertain,
        Ok(None) => {
            // A kill error can race with normal exit. Only a successful status read proves reaping.
            let _ = child.kill();
            let deadline = Instant::now() + REAP_GRACE;
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => break Cleanup::Reaped,
                    Err(_) => break Cleanup::Uncertain,
                    Ok(None) if Instant::now() >= deadline => break Cleanup::Uncertain,
                    Ok(None) => pause(deadline),
                }
            }
        }
    };
    Error {
        failure,
        cleanup,
        source,
    }
}
