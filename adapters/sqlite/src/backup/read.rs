//! Retained-backup inspection reuses creation's native integrity and byte checks.

use super::{
    DATABASE_FILE, RECEIPT_FILE, check_copy, check_directory, fingerprint, open_unchanged,
    same_file_state,
};
use crate::{Sqlite, regular_file};
use gazette_core::{
    source::Sha256Digest,
    store::{BackupInspection, BackupSnapshot, BackupVerifyOptions, Error},
};
use gazette_ports::store::BackupReader;
use std::{
    fs::{self, Metadata},
    io::Read,
    path::{Path, PathBuf},
};

impl BackupReader for Sqlite {
    fn verify_backup(
        &self,
        directory: &Path,
        manifest_sha256: Sha256Digest,
        options: BackupVerifyOptions,
    ) -> Result<BackupInspection, Error> {
        options.validate()?;
        let directory = resolve_directory(directory)?;
        let identity = fs::symlink_metadata(&directory).map_err(Error::Io)?;
        if !identity.is_dir() {
            return Err(Error::UnsafePath);
        }
        check_directory(&directory, &identity, &[DATABASE_FILE, RECEIPT_FILE])?;
        let receipt_path = directory.join(RECEIPT_FILE);
        let (manifest_identity, manifest) = read_manifest(&receipt_path, options.manifest_bytes)?;
        if Sha256Digest::of(&manifest) != manifest_sha256 {
            return Err(Error::BackupVerification);
        }
        // A typed decode rejects duplicate and unknown fields at every declared level.
        let snapshot: BackupSnapshot =
            serde_json::from_slice(&manifest).map_err(|_| Error::BackupManifest)?;
        snapshot.validate()?;
        if snapshot.bytes > options.read.storage_bytes {
            return Err(Error::BackupLimit);
        }
        let path = directory.join(DATABASE_FILE);
        let database_identity = regular_file(&path)?;
        check_fingerprint(&path, &database_identity, &snapshot, options)?;
        let verifier = check_copy(&path, &snapshot.source, options.read)?;
        // Each hashing handle is closed before the SQLite read to preserve POSIX locks.
        // A later read checks bytes again; no cached result is an effect capability.
        check_fingerprint(&path, &database_identity, &snapshot, options)?;
        let (after, read_back) = read_manifest(&receipt_path, options.manifest_bytes)?;
        if !same_file_state(&manifest_identity, &after) || read_back != manifest {
            return Err(Error::Changed);
        }
        check_directory(&directory, &identity, &[DATABASE_FILE, RECEIPT_FILE])?;
        Ok(BackupInspection {
            snapshot,
            manifest_sha256,
            verifier,
        })
    }
}

pub(super) fn resolve_directory(directory: &Path) -> Result<PathBuf, Error> {
    let absolute = if directory.is_absolute() {
        directory.to_owned()
    } else {
        std::env::current_dir().map_err(Error::Io)?.join(directory)
    };
    // Reconstruct the final component so a trailing slash cannot hide a symlink.
    let parent = absolute
        .parent()
        .ok_or(Error::UnsafePath)?
        .canonicalize()
        .map_err(Error::Io)?;
    let directory = parent.join(absolute.file_name().ok_or(Error::UnsafePath)?);
    if !fs::symlink_metadata(&directory)
        .map_err(Error::Io)?
        .is_dir()
    {
        return Err(Error::UnsafePath);
    }
    Ok(directory)
}

fn check_fingerprint(
    path: &Path,
    identity: &Metadata,
    snapshot: &BackupSnapshot,
    options: BackupVerifyOptions,
) -> Result<(), Error> {
    if !same_file_state(identity, &regular_file(path)?) {
        return Err(Error::Changed);
    }
    let (file, bytes, hash) = fingerprint(path, identity, options.read.storage_bytes)?;
    drop(file);
    if !same_file_state(identity, &regular_file(path)?) {
        return Err(Error::Changed);
    }
    if bytes != snapshot.bytes || hash != snapshot.sha256 {
        return Err(Error::BackupVerification);
    }
    Ok(())
}

pub(crate) fn read_manifest(path: &Path, limit: u64) -> Result<(Metadata, Vec<u8>), Error> {
    let identity = regular_file(path)?;
    if identity.len() > limit {
        return Err(Error::BackupManifestLimit);
    }
    let file = open_unchanged(path, &identity)?;
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(Error::Io)?;
    if bytes.len() as u64 > limit {
        return Err(Error::BackupManifestLimit);
    }
    if !same_file_state(&identity, &regular_file(path)?) || identity.len() != bytes.len() as u64 {
        return Err(Error::Changed);
    }
    Ok((identity, bytes))
}
