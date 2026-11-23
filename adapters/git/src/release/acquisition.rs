//! Native bounded acquisition into a fresh private bare source, with exact remote read-back.

use super::{SelectedRepository, custody, history, setup};
use crate::remote::{Isolation, git_command, target};
use gazette_core::{process, release::*, source::MetadataBudget};
use gazette_ports::{
    process::CaptureRunner,
    release::{ReleaseParentAcquisition, RemoteReader as _},
};
use git2::{ObjectFormat, Repository, RepositoryInitOptions};
use std::{fs, path::Path};

const CONFIGURATION: &[u8] =
    b"[core]\n\trepositoryformatversion = 0\n\tbare = true\n\tlogallrefupdates = false\n";

/// Explicitly selected native Git and bounded process provider; never ambient credentials.
pub struct ParentAcquirer<'a, R> {
    /// Shared bounded native capture provider.
    pub runner: &'a R,
    /// Explicit executable, private isolation and optional HTTPS credential helper.
    pub selection: &'a GitSelection,
}

impl<R: CaptureRunner> ReleaseParentAcquisition for ParentAcquirer<'_, R> {
    fn acquire_parent(
        &self,
        request: ParentAcquisitionRequest<'_>,
        options: ParentAcquisitionOptions,
    ) -> Result<ParentAcquisitionReport, RepositorySetupError> {
        let mut effect = RepositorySetupEffect::NotAttempted;
        let mut cleanup = None;
        (|| {
            if !options.allow_acquire {
                return Err("explicit parent acquisition permission is required");
            }
            options
                .read
                .validate()
                .map_err(|_| "parent acquisition read permission or bounds refused")?;
            if !self.selection.is_valid() || options.limits.storage_bytes == 0 {
                return Err("parent acquisition selection or bounds refused");
            }
            validate_git_destination(request.remote, request.branch)
                .map_err(|_| "parent destination is invalid")?;
            let remote = target(request.remote, options.read.allow_network)
                .map_err(|_| "parent transport selection refused")?;
            let isolation = Isolation::open(&self.selection.isolation)
                .map_err(|_| "parent acquisition isolation refused")?;
            let program = self
                .selection
                .program
                .to_str()
                .ok_or("Git executable is not text")?;
            let directory = isolation
                .path
                .to_str()
                .filter(|p| !p.contains(':'))
                .ok_or("Git isolation path is unsupported")?;
            let root = setup::coordinate(request.root)?;
            setup::separate(&root, request.protected_roots)?;
            setup::separate(&root, std::slice::from_ref(&isolation.path))?;
            if Path::new(request.remote).is_absolute() {
                setup::separate(&root, &[request.remote.into()])?;
            } else if let Ok(url) = url::Url::parse(request.remote)
                && let Ok(path) = url.to_file_path()
            {
                setup::separate(&root, &[path])?;
            }
            setup::absent(&root)?;
            let mut command = git_command(
                program,
                directory,
                &remote,
                self.selection.credentials.as_ref(),
            )
            .map_err(|_| "parent credential selection refused")?;
            command.arguments.extend([
                "-c".into(),
                "gc.auto=0".into(),
                "-c".into(),
                "maintenance.auto=false".into(),
                "-c".into(),
                "fetch.writeCommitGraph=false".into(),
                "-c".into(),
                "core.hooksPath=/dev/null".into(),
                format!(
                    "--git-dir={}",
                    root.to_str().ok_or("parent repository path is not text")?
                ),
                "fetch".into(),
                "--no-tags".into(),
                "--no-write-fetch-head".into(),
                "--no-recurse-submodules".into(),
                "--no-auto-maintenance".into(),
                "--no-progress".into(),
                "--".into(),
                remote,
                format!("refs/heads/{}", request.branch),
            ]);
            command
                .validate()
                .map_err(|_| "parent acquisition command refused")?;
            let reader = crate::RemoteReader {
                runner: self.runner,
                selection: self.selection,
            };
            let before = reader
                .observe_remote(request.remote, request.branch, options.read)
                .map_err(|e| {
                    cleanup = e.cleanup;
                    "parent remote observation failed"
                })?;
            if before
                != (RefAdvertisement::Commit {
                    oid: request.expected.clone(),
                })
            {
                return Err("parent remote does not match the selected commit");
            }
            effect = RepositorySetupEffect::LocalStateMayRemain;
            let held = setup::Directory::create(&root)?;
            let mut initialization = RepositoryInitOptions::new();
            initialization
                .bare(true)
                .no_reinit(true)
                .mkdir(false)
                .mkpath(false)
                .external_template(false)
                .template_path(Path::new(""))
                .object_format(ObjectFormat::Sha1);
            Repository::init_opts(&root, &initialization)
                .map_err(|_| "parent repository initialization failed")?;
            fs::write(root.join("config"), CONFIGURATION)
                .map_err(|_| "parent repository configuration failed")?;
            held.recheck()?;
            let captured = self
                .runner
                .capture(process::CaptureRequest {
                    request: process::Request {
                        command: &command,
                        input: &[],
                        options: process::Options {
                            allow_execute: true,
                            max_input_bytes: 0,
                            timeout: options.read.timeout,
                        },
                    },
                    max_output_bytes: options.read.max_response_bytes,
                    max_stderr_bytes: Some(options.read.max_response_bytes),
                })
                .map_err(|error| {
                    cleanup = Some(error.cleanup);
                    "parent fetch failed or exceeded process bounds"
                })?;
            cleanup = Some(process::Cleanup::Reaped);
            if captured.exit.code != Some(0) {
                return Err("parent fetch did not complete successfully");
            }
            isolation
                .recheck()
                .map_err(|_| "parent acquisition isolation changed")?;
            held.recheck()?;
            let mut budget = MetadataBudget::new(options.limits.metadata_bytes);
            if setup::read(&root.join("config"), &mut budget)? != CONFIGURATION {
                return Err("parent acquisition configuration changed");
            }
            custody::inspect_storage(&root, options.limits.storage_bytes, &mut budget)?;
            let selected = SelectedRepository::open(&root, &mut budget)?;
            history::verify(
                &selected.repo,
                request.expected,
                options.limits.history,
                &mut budget,
            )?;
            if selected
                .repo
                .references()
                .map_err(|_| "parent refs unavailable")?
                .next()
                .is_some()
            {
                return Err("parent acquisition unexpectedly created refs");
            }
            setup::absent(&root.join("FETCH_HEAD"))?;
            let after = reader
                .observe_remote(request.remote, request.branch, options.read)
                .map_err(|e| {
                    cleanup = e.cleanup;
                    "independent parent remote read-back failed"
                })?;
            if after != before {
                return Err("parent remote changed during acquisition");
            }
            custody::synchronize(&root, options.limits.storage_bytes, &mut budget)?;
            let reopened = SelectedRepository::open(&root, &mut budget)?;
            history::verify(
                &reopened.repo,
                request.expected,
                options.limits.history,
                &mut budget,
            )?;
            selected.recheck()?;
            reopened.recheck()?;
            held.synchronize()?;
            Ok(ParentAcquisitionReport {
                source: ParentSource::Bare { path: root },
                parent: request.expected.clone(),
            })
        })()
        .map_err(|reason| RepositorySetupError {
            reason,
            effect,
            cleanup,
        })
    }
}
