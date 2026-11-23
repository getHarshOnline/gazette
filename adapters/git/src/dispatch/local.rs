//! Synchronous local object transfer and one locked ref update; no receiver process or hooks.
//!
//! Trusted exclusive namespace custody is required, as for the existing native object adapter.
//! Deadlines are cooperative phase checks, not interruption of native filesystem calls.

use std::{collections::BTreeSet, fs, io::Read, path::Path, time::Instant};

use gazette_core::{
    process::Cleanup,
    release::{
        DispatchEffect, DispatchError, DispatchFailure, DispatchObservation, DispatchOptions,
        DispatchRequest, DispatchStatus, ParentSource, RemoteState,
    },
    source::{MetadataBudget, Sha256Digest},
};
use gazette_ports::release::ReleaseWorktrees;
use git2::{Config, ErrorCode, Oid, Repository, Signature, Time};
use url::Url;

use crate::{
    Git,
    release::{self, SelectedRepository, custody, history},
    remote,
};

pub(super) fn dispatch(
    request: DispatchRequest<'_>,
    options: DispatchOptions,
) -> Result<DispatchObservation, DispatchError> {
    let mut effect = DispatchEffect::NotStarted;
    let result = (|| {
        if request.target.remote.len() > 16 * 1024 || request.target.branch.len() > 4096 {
            return Err(DispatchFailure::Limit);
        }
        let deadline = Instant::now() + options.timeout;
        let url = remote::target(&request.target.remote, false)
            .map_err(|_| DispatchFailure::Unsupported)?;
        let root = Url::parse(&url)
            .ok()
            .and_then(|url| url.to_file_path().ok())
            .ok_or(DispatchFailure::Unsupported)?;
        let configuration = profile(&root)?;
        let mut budget = MetadataBudget::new(options.worktree.output.metadata_bytes);
        let selected =
            SelectedRepository::open(&root, &mut budget).map_err(|_| DispatchFailure::Isolation)?;
        // Only the checked local profile informs native writes; no ambient identity or policy.
        let config = Config::open(&selected.root.join("config"))
            .map_err(|_| DispatchFailure::Unsupported)?;
        selected
            .repo
            .set_config(&config)
            .map_err(|_| DispatchFailure::Unsupported)?;
        let storage_bytes = options.worktree.history.bytes;
        custody::inspect_storage(&selected.root, storage_bytes, &mut budget)
            .map_err(|_| DispatchFailure::Limit)?;
        Git.inspect_worktree(
            request.repository,
            &request.target.local_commit_oid,
            request.output,
            request.metadata,
            options.worktree,
        )
        .map_err(|_| DispatchFailure::LocalState)?;
        let captured = history::capture(
            &ParentSource::Worktree {
                path: request.repository.to_owned(),
            },
            &selected.root,
            &request.target.local_commit_oid,
            options.worktree.history,
            storage_bytes,
            &mut budget,
        )
        .map_err(|_| DispatchFailure::LocalState)?;
        let reference = format!("refs/heads/{}", request.target.branch);
        let desired = Oid::from_str(request.target.local_commit_oid.as_str())
            .map_err(|_| DispatchFailure::Selection)?;
        // Check every possible receipt size before lock/object/ref effects.
        observation(
            request,
            DispatchStatus::AlreadyPresent,
            Some(desired),
            options.max_response_bytes,
        )?;
        elapsed(deadline)?;
        selected.recheck().map_err(|_| DispatchFailure::Isolation)?;
        if profile(&selected.root)? != configuration {
            return Err(DispatchFailure::Isolation);
        }
        let mut transaction = selected
            .repo
            .transaction()
            .map_err(|_| DispatchFailure::Transport)?;
        // A failed native lock operation can leave administrative residue after interruption.
        effect = DispatchEffect::Unverified;
        transaction
            .lock_ref(&reference)
            .map_err(|_| DispatchFailure::Transport)?;
        let before = current(&selected.repo, &reference)?;
        if before == Some(desired) {
            return observation(
                request,
                DispatchStatus::AlreadyPresent,
                before,
                options.max_response_bytes,
            );
        }
        let expected = match &request.target.expected_remote_oid_or_unborn {
            RemoteState::Unborn => None,
            RemoteState::Commit { oid } => {
                Some(Oid::from_str(oid.as_str()).map_err(|_| DispatchFailure::Selection)?)
            }
        };
        if before != expected {
            return observation(
                request,
                DispatchStatus::Rejected,
                before,
                options.max_response_bytes,
            );
        }
        elapsed(deadline)?;
        captured
            .write(&selected.repo)
            .map_err(|_| DispatchFailure::Transport)?;
        release::inspect(
            &selected.repo,
            &request.target.local_commit_oid,
            request.output,
            request.metadata,
            options.worktree.output,
            &mut budget,
        )
        .map_err(|_| DispatchFailure::LocalState)?;
        // Reachable objects are durable before the publication ref is changed.
        custody::synchronize(&selected.root, storage_bytes, &mut budget)
            .map_err(|_| DispatchFailure::Transport)?;
        selected.recheck().map_err(|_| DispatchFailure::Isolation)?;
        if profile(&selected.root)? != configuration {
            return Err(DispatchFailure::Isolation);
        }
        elapsed(deadline)?;
        let signer = &request.metadata.committer;
        let signature = Signature::new(
            &signer.name,
            &signer.email,
            &Time::new(signer.seconds, signer.offset_minutes),
        )
        .map_err(|_| DispatchFailure::Selection)?;
        transaction
            .set_target(
                &reference,
                desired,
                Some(&signature),
                "Gazette local transaction",
            )
            .map_err(|_| DispatchFailure::Transport)?;
        transaction
            .commit()
            .map_err(|_| DispatchFailure::Transport)?;
        custody::synchronize(&selected.root, storage_bytes, &mut budget)
            .map_err(|_| DispatchFailure::Transport)?;
        selected.recheck().map_err(|_| DispatchFailure::Isolation)?;
        let observed = current(&selected.repo, &reference)?;
        if observed != Some(desired) {
            return Err(DispatchFailure::LocalState);
        }
        if profile(&selected.root)? != configuration {
            return Err(DispatchFailure::Isolation);
        }
        elapsed(deadline)?;
        observation(
            request,
            DispatchStatus::Updated,
            observed,
            options.max_response_bytes,
        )
    })();
    result.map_err(|failure| DispatchError {
        failure,
        effect,
        cleanup: Cleanup::NotStarted,
    })
}

fn elapsed(deadline: Instant) -> Result<(), DispatchFailure> {
    if Instant::now() >= deadline {
        Err(DispatchFailure::Timeout)
    } else {
        Ok(())
    }
}

fn current(repository: &Repository, reference: &str) -> Result<Option<Oid>, DispatchFailure> {
    match repository.find_reference(reference) {
        Ok(value) => value.target().map(Some).ok_or(DispatchFailure::Unsupported),
        Err(error) if error.code() == ErrorCode::NotFound => Ok(None),
        Err(_) => Err(DispatchFailure::LocalState),
    }
}

// Deliberately a tiny accepted native initialization profile, not a Git config parser.
// Anything else (including includes, hooksPath, receive policy or a custom ref backend)
// requires receive-pack qualification and is refused without rewriting the destination.
fn profile(root: &Path) -> Result<Vec<u8>, DispatchFailure> {
    let metadata = fs::symlink_metadata(root).map_err(|_| DispatchFailure::Isolation)?;
    if !metadata.is_dir() {
        return Err(DispatchFailure::Isolation);
    }
    let path = root.join("config");
    let metadata = fs::symlink_metadata(&path).map_err(|_| DispatchFailure::Unsupported)?;
    if !metadata.is_file() || metadata.len() > 4096 {
        return Err(DispatchFailure::Unsupported);
    }
    let mut bytes = Vec::new();
    fs::File::open(&path)
        .map_err(|_| DispatchFailure::Unsupported)?
        .take(4097)
        .read_to_end(&mut bytes)
        .map_err(|_| DispatchFailure::Unsupported)?;
    if bytes.len() > 4096 {
        return Err(DispatchFailure::Unsupported);
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| DispatchFailure::Unsupported)?;
    let mut lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
    if lines.next() != Some("[core]") {
        return Err(DispatchFailure::Unsupported);
    }
    let mut keys = BTreeSet::new();
    for line in lines {
        let (key, value) = line.split_once('=').ok_or(DispatchFailure::Unsupported)?;
        let (key, value) = (key.trim(), value.trim());
        if !matches!(
            (key, value),
            ("bare", "true")
                | (
                    "filemode" | "ignorecase" | "precomposeunicode" | "symlinks",
                    "true" | "false"
                )
                | ("repositoryformatversion", "0")
                | ("logallrefupdates", "false")
        ) || !keys.insert(key)
        {
            return Err(DispatchFailure::Unsupported);
        }
    }
    // Native initialization records filesystem capabilities. These do not select
    // receiver policy; this path writes objects/refs and never checks out files.
    if !["bare", "filemode", "repositoryformatversion"]
        .iter()
        .all(|key| keys.contains(key))
    {
        return Err(DispatchFailure::Unsupported);
    }
    match fs::symlink_metadata(root.join("config.worktree")) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
        _ => return Err(DispatchFailure::Unsupported),
    }
    match fs::read_dir(root.join("hooks")) {
        Ok(entries) => {
            for (index, entry) in entries.enumerate() {
                if index >= 64 {
                    return Err(DispatchFailure::Limit);
                }
                let entry = entry.map_err(|_| DispatchFailure::Unsupported)?;
                if !entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.ends_with(".sample"))
                {
                    return Err(DispatchFailure::Unsupported);
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
        Err(_) => return Err(DispatchFailure::Unsupported),
    }
    Ok(bytes)
}

fn observation(
    request: DispatchRequest<'_>,
    status: DispatchStatus,
    observed: Option<Oid>,
    maximum: usize,
) -> Result<DispatchObservation, DispatchFailure> {
    let code = match status {
        DispatchStatus::Updated => "updated",
        DispatchStatus::AlreadyPresent => "already-present",
        DispatchStatus::Rejected => "rejected",
    };
    let expected = match &request.target.expected_remote_oid_or_unborn {
        RemoteState::Unborn => "unborn",
        RemoteState::Commit { oid } => oid.as_str(),
    };
    let actual = observed.map_or_else(|| "unborn".to_owned(), |oid| oid.to_string());
    let output = request.output.sha256().to_string();
    let fields = [
        "gazette.local-transaction/1",
        code,
        &request.target.remote,
        &request.target.branch,
        expected,
        request.target.local_commit_oid.as_str(),
        &actual,
        &output,
    ];
    let size = fields
        .iter()
        .try_fold(0usize, |total, field| {
            total.checked_add(8)?.checked_add(field.len())
        })
        .filter(|size| *size <= maximum)
        .ok_or(DispatchFailure::Limit)?;
    let mut bytes = Vec::with_capacity(size);
    for field in fields {
        bytes.extend_from_slice(&(field.len() as u64).to_be_bytes());
        bytes.extend_from_slice(field.as_bytes());
    }
    Ok(DispatchObservation {
        status,
        exit_code: None,
        response_sha256: Sha256Digest::of(&bytes),
        cleanup: Cleanup::NotStarted,
    })
}
