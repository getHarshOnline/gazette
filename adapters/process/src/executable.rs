//! Paired executable-file observations in a trusted native namespace, not image attestation.
//!
//! Spawn still uses the selected path. Holding and rechecking this descriptor detects observed
//! changes; it does not close a hostile replacement-and-restore race, pin an interpreter/import,
//! or confine native effects. Byte ceilings bound both streaming passes, not filesystem latency.

use std::{
    fs::{self, File, Metadata},
    io::{self, Read, Seek, SeekFrom},
    path::Path,
};

use gazette_core::{process::ExecutablePin, source::Sha256Digest};
use rtepl_filesystem::unix::open_readonly_nofollow;
use sha2::{Digest, Sha256};

use crate::same_file;

pub(super) struct Executable<'a> {
    file: File,
    path: &'a Path,
    pin: &'a ExecutablePin,
    identity: Metadata,
}

impl<'a> Executable<'a> {
    pub(super) fn open(path: &'a str, pin: &'a ExecutablePin) -> io::Result<Self> {
        let path = Path::new(path);
        let declared = fs::symlink_metadata(path)?;
        if !declared.is_file() || declared.len() != pin.bytes {
            return Err(changed());
        }
        // NONBLOCK prevents a raced FIFO replacement from blocking the pre-spawn check.
        // NOFOLLOW applies to the final component; ancestors remain trusted native custody.
        let file = open_readonly_nofollow(path, true)?;
        let identity = file.metadata()?;
        if !same_file(&declared, &identity) {
            return Err(changed());
        }
        let mut executable = Self {
            file,
            path,
            pin,
            identity,
        };
        executable.verify()?;
        Ok(executable)
    }

    pub(super) fn verify(&mut self) -> io::Result<()> {
        self.binding()?;
        self.file.seek(SeekFrom::Start(0))?;
        let mut remaining = self.pin.bytes;
        let mut hash = Sha256::new();
        let mut buffer = [0u8; 16 * 1024];
        while remaining != 0 {
            let bound = remaining.min(buffer.len() as u64) as usize;
            let count = match self.file.read(&mut buffer[..bound]) {
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                result => result?,
            };
            if count == 0 {
                return Err(changed());
            }
            hash.update(&buffer[..count]);
            remaining -= count as u64;
        }
        // Exact EOF is independent of the observed length; growth cannot hide behind metadata.
        loop {
            match self.file.read(&mut buffer[..1]) {
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Ok(0) => break,
                Ok(_) => return Err(changed()),
                Err(error) => return Err(error),
            }
        }
        let observed = Sha256Digest::from(<[u8; 32]>::from(hash.finalize()));
        if observed != self.pin.sha256 {
            return Err(changed());
        }
        self.binding()
    }

    fn binding(&self) -> io::Result<()> {
        let held = self.file.metadata()?;
        let named = fs::symlink_metadata(self.path)?;
        if !same_file(&self.identity, &held) || !same_file(&self.identity, &named) {
            return Err(changed());
        }
        Ok(())
    }
}

fn changed() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "selected executable could not be verified",
    )
}
