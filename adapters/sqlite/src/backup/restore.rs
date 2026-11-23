//! Byte-exact detached restoration; no selected live-store path or activation effect.

use super::{
    DATABASE_FILE, check_copy, fingerprint, new_file, open_unchanged, output::OutputDirectory,
    read::resolve_directory, same_file_state,
};
use crate::{Sqlite, regular_file};
use gazette_core::{
    source::Sha256Digest,
    store::{Error, OutputError, RestoreOptions, RestorePreparation, RestoreReceipt},
};
use gazette_ports::store::{BackupReader, RestorePreparer};
use std::{
    io::{self, Read},
    path::Path,
};

impl RestorePreparer for Sqlite {
    fn prepare_restore(
        &self,
        backup: &Path,
        manifest_sha256: Sha256Digest,
        parent: &Path,
        options: RestoreOptions,
    ) -> Result<RestoreReceipt, OutputError> {
        options.validate()?;
        let backup = resolve_directory(backup)?;
        let parent = parent.canonicalize().map_err(Error::Io)?;
        if parent.starts_with(&backup) {
            return Err(Error::UnsafePath.into());
        }
        let inspection = self.verify_backup(&backup, manifest_sha256, options.verification)?;
        let snapshot = &inspection.snapshot;
        let read_limit = snapshot.bytes.checked_add(1).ok_or(Error::InvalidLimits)?;
        let source = backup.join(DATABASE_FILE);
        let source_identity = regular_file(&source)?;
        let source_file = open_unchanged(&source, &source_identity)?;
        let output = OutputDirectory::create(&parent, "gazette-restore-", options.output)?;
        let result = (|| {
            let path = output.path().join(DATABASE_FILE);
            let mut copied = new_file(&path)?;
            let identity = copied.metadata().map_err(Error::Io)?;
            let bytes =
                io::copy(&mut source_file.take(read_limit), &mut copied).map_err(Error::Io)?;
            if bytes != snapshot.bytes {
                return Err(Error::BackupVerification);
            }
            copied.sync_all().map_err(Error::Io)?;
            drop(copied);
            if !same_file_state(&source_identity, &regular_file(&source)?) {
                return Err(Error::Changed);
            }
            let (file, bytes, sha256) =
                fingerprint(&path, &identity, options.verification.read.storage_bytes)?;
            drop(file);
            if bytes != snapshot.bytes || sha256 != snapshot.sha256 {
                return Err(Error::BackupVerification);
            }
            let verifier = check_copy(&path, &snapshot.source, options.verification.read)?;
            let (file, after_bytes, after_hash) =
                fingerprint(&path, &identity, options.verification.read.storage_bytes)?;
            drop(file);
            if after_bytes != bytes || after_hash != sha256 {
                return Err(Error::Changed);
            }
            // No live SQLite handle overlaps a filesystem read/close of its database.
            self.verify_backup(&backup, manifest_sha256, options.verification)?;
            let preparation = RestorePreparation {
                format: RestorePreparation::FORMAT.into(),
                source_manifest_sha256: manifest_sha256,
                database: DATABASE_FILE.into(),
                bytes,
                sha256,
                verifier,
            };
            let receipt_sha256 = output.seal("restore.json", &preparation)?;
            Ok(RestoreReceipt {
                directory: output.path().to_path_buf(),
                preparation,
                receipt_sha256,
            })
        })();
        result.map_err(|cause| output.failure(cause))
    }
}
