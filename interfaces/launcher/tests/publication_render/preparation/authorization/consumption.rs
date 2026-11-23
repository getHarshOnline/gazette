//! Actual prepared parented sites → anchored custody → native refs → one-use intent.

#[path = "consumption/dispatch.rs"]
mod dispatch;
#[path = "consumption/reviewed.rs"]
mod reviewed;
#[path = "consumption/session.rs"]
mod session;
#[path = "consumption/settlement.rs"]
mod settlement;

use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

use git2::{Repository, Signature};
use tempfile::TempDir;

use super::{
    Prepared, Sha256Digest, batch_options, declaration, fs, options, private_directory, read,
    release, sign, store, worktree,
};

struct Local {
    prepared: Prepared,
    _remotes: TempDir,
    paths: Vec<PathBuf>,
    isolation: TempDir,
    control: TempDir,
    program: PathBuf,
    calls: PathBuf,
}

fn commit(path: &Path, message: &str) -> gazette_sdk::source::CommitId {
    let repo = Repository::open_bare(path).expect("actual disposable bare repository");
    let tree_id = repo
        .treebuilder(None)
        .expect("native empty tree builder")
        .write()
        .expect("persist native parent tree");
    let tree = repo.find_tree(tree_id).expect("read actual parent tree");
    let signature = Signature::new("Fixture", "fixture@example.invalid", &git2::Time::new(0, 0))
        .expect("deterministic fixture Git identity");
    let oid = repo
        .commit(None, &signature, &signature, message, &tree, &[])
        .expect("persist real native fixture commit");
    repo.reference(
        "refs/heads/publication",
        oid,
        true,
        "explicit fixture setup",
    )
    .expect("advertise exact fixture commit on publication branch");
    oid.to_string()
        .parse()
        .expect("native Git OID satisfies shared commit grammar")
}

impl Local {
    fn git(&self) -> release::GitSelection {
        release::GitSelection {
            program: self.program.clone(),
            isolation: self.isolation.path().to_path_buf(),
            credentials: None,
        }
    }

    fn new(coordinated: bool) -> Self {
        Self::with_prestate(coordinated, true)
    }

    fn unborn() -> Self {
        Self::with_prestate(true, false)
    }

    fn with_prestate(coordinated: bool, parented: bool) -> Self {
        Self::with_profile(coordinated, parented, true, true)
    }

    fn qualified() -> Self {
        Self::with_profile(true, true, true, false)
    }

    fn candidate_qualified() -> Self {
        Self::with_profile(true, true, true, true)
    }

    fn seven_site_candidate_qualified() -> Self {
        let (fixture, configuration, sites) = crate::sites::seven_site_fixture();
        let remotes = private_directory();
        let paths = sites
            .iter()
            .map(|site| {
                remotes.path().join(format!(
                    "{}.git",
                    site.strip_prefix("site:").expect("fixture site prefix")
                ))
            })
            .collect::<Vec<_>>();
        for path in &paths {
            Repository::init_bare(path).expect("real parent source and local destination");
        }
        let parents = paths
            .iter()
            .enumerate()
            .map(|(ordinal, path)| {
                let oid = commit(path, &format!("reviewed parent {ordinal}"));
                (worktree::ParentSource::Bare { path: path.clone() }, oid)
            })
            .collect::<Vec<_>>();
        let prepared = Prepared::seven_site_candidate_qualified(
            fixture,
            configuration,
            &sites,
            &paths,
            &parents,
        );
        Self::with_prepared(prepared, remotes, paths)
    }

    fn with_profile(coordinated: bool, parented: bool, qualified: bool, candidate: bool) -> Self {
        let remotes = private_directory();
        let paths = vec![
            remotes.path().join("one.git"),
            remotes.path().join("other.git"),
        ];
        for path in &paths {
            Repository::init_bare(path).expect("real parent source and local destination");
        }
        let parents = parented.then(|| {
            std::array::from_fn(|ordinal| {
                let path = &paths[ordinal];
                let oid = commit(&paths[ordinal], &format!("reviewed parent {ordinal}"));
                (worktree::ParentSource::Bare { path: path.clone() }, oid)
            })
        });
        let destinations = paths
            .iter()
            .map(|path| path.to_str().expect("fixture destination UTF-8"))
            .collect::<Vec<_>>();
        let prepared = Prepared::with_candidate_qualification(
            true,
            Some([destinations[0], destinations[1]]),
            coordinated.then_some("coordinator:fixture"),
            parents.as_ref(),
            qualified,
            candidate,
        );
        Self::with_prepared(prepared, remotes, paths)
    }

    fn with_prepared(prepared: Prepared, remotes: TempDir, paths: Vec<PathBuf>) -> Self {
        let control = private_directory();
        let program = control.path().join("observed-git");
        let calls = control.path().join("git-calls");
        fs::write(
            &program,
            format!(
                "#!/bin/sh\nprintf x >> '{}'\nexec {} \"$@\"\n",
                calls.display(),
                crate::native::shell(crate::native::git())
            ),
        )
        .expect("write counting wrapper that delegates to real Git");
        fs::set_permissions(&program, fs::Permissions::from_mode(0o700))
            .expect("make fixture Git wrapper privately executable");
        Self {
            prepared,
            _remotes: remotes,
            paths,
            isolation: private_directory(),
            control,
            program,
            calls,
        }
    }

    fn signed(&mut self, wrong_coordinator: bool) -> (store::Writer, Vec<u8>, release::Binding) {
        let mut writer = match &self.prepared.coordinator {
            Some((_, selected)) => store::coordinator::acquire_writer(
                &selected.anchor_root,
                &selected.binding,
                self.prepared.header,
                options().writer,
                true,
            ),
            None => store::acquire_writer(
                self.prepared._journal_root.path(),
                self.prepared.header,
                options().writer,
            ),
        }
        .expect("native writer held before constructing signed coordinates");
        self.prepared.header = writer
            .inspect()
            .expect("inspect exact held generation before signing")
            .header;
        let (mut body, mut binding) = declaration(&self.prepared.inspect());
        let coordinator = if wrong_coordinator {
            "coordinator:other"
        } else {
            "coordinator:fixture"
        };
        body.coordinator_id = coordinator.into();
        binding.coordinator_id = coordinator.into();
        let (bytes, binding) = sign(body, binding);
        (writer, bytes, binding)
    }

    fn register(&mut self, wrong_coordinator: bool) -> (store::Writer, Vec<u8>, release::Binding) {
        let (mut writer, bytes, binding) = self.signed(wrong_coordinator);
        writer
            .register_release_authorization(
                release::PreparedAuthorizationRequest {
                    authorization: &bytes,
                    binding: &binding,
                    preparations: self.prepared.request(&self.prepared.selections),
                },
                release::RegisterAuthorizationOptions {
                    verification: verification(),
                    record: store::ReleaseAuthorizationWriteOptions { allow_record: true },
                },
            )
            .expect("actual verification and native two-target pending registration");
        (writer, bytes, binding)
    }

    fn request<'a>(&'a self, authorization: &'a [u8]) -> release::ConsumeAuthorizationRequest<'a> {
        release::ConsumeAuthorizationRequest {
            authorization,
            preparations: self.prepared.request(&self.prepared.selections),
        }
    }

    fn reads(&self) -> usize {
        fs::read(&self.calls).map_or(0, |bytes| bytes.len())
    }

    fn authority(&self, initial: &release::Binding, refreshed: &release::Binding) -> Authority {
        let paths = [
            self.control.path().join("initial-public-authority.json"),
            self.control.path().join("refreshed-public-authority.json"),
        ];
        for (path, binding) in paths.iter().zip([initial, refreshed]) {
            fs::write(
                path,
                serde_json::to_vec(binding).expect("encode independent fixture public authority"),
            )
            .expect("retain selected public authority observation");
        }
        Authority { paths, calls: 0 }
    }

    fn assert_pending(&self, bytes: &[u8]) {
        let body: release::Authorization =
            serde_json::from_slice(bytes).expect("decode original fixture authorization identity");
        assert!(
            store::inspect_release_consumption(
                &self.prepared.database,
                &body.record_id,
                Some(&self.prepared.header),
                read()
            )
            .expect("independent native inspection proves no consumed intent")
            .record
            .is_none()
        );
        let pending = store::inspect_release_authorization(
            &self.prepared.database,
            &body.record_id,
            Some(&self.prepared.header),
            read(),
        )
        .expect("inspect original pending custody")
        .record
        .expect("original pending authorization remains present");
        assert_eq!(pending.authorization.as_bytes(), bytes);
    }
}

// Fixture-owned independent public authority observations. Each callback opens
// its selected file afresh; no expected field is derived from received body bytes.
struct Authority {
    paths: [PathBuf; 2],
    calls: usize,
}
impl release::AuthoritySource for Authority {
    fn acquire_authority(&mut self) -> Result<release::Binding, release::AuthorityError> {
        let index = self.calls.min(1);
        self.calls += 1;
        let bytes =
            fs::read(&self.paths[index]).map_err(|_| release::AuthorityError::Unavailable)?;
        serde_json::from_slice(&bytes).map_err(|_| release::AuthorityError::Invalid)
    }
}

fn verification() -> release::PreparedAuthorizationOptions {
    release::PreparedAuthorizationOptions {
        preparations: batch_options(),
        ..Default::default()
    }
}
fn consume_options() -> release::ConsumeAuthorizationOptions {
    release::ConsumeAuthorizationOptions {
        verification: verification(),
        read: release::RemoteReadOptions {
            allow_read: true,
            allow_network: false,
            ..Default::default()
        },
        leases: store::ReleaseLeaseOptions {
            allow_acquire: true,
        },
        consume: store::ReleaseConsumptionOptions {
            allow_consume: true,
        },
    }
}

#[test]
fn complete_parented_publications_consume_once_and_reopen_without_renewing_authority() {
    let mut local = Local::new(true);
    let (mut writer, bytes, initial) = local.register(false);
    let original = store::inspect_release_authorization(
        &local.prepared.database,
        &serde_json::from_slice::<release::Authorization>(&bytes)
            .expect("decode original signed fixture record ID")
            .record_id,
        Some(&local.prepared.header),
        read(),
    )
    .expect("inspect actual registration before consumption")
    .record
    .expect("actual pending registration exists");
    let mut refreshed = initial.clone();
    refreshed.observed_at += 1;
    refreshed.authority_frontier_sha256 =
        Sha256Digest::of(b"independent current authority observation");
    let mut authority = local.authority(&initial, &refreshed);
    let result = writer
        .consume_release_authorization(
            &mut authority,
            &local.git(),
            local.request(&bytes),
            consume_options(),
        )
        .expect("complete native local consumption, no dispatch");
    assert_eq!(authority.calls, 2);
    assert_eq!(local.reads(), 2);
    assert_eq!(result.record.authorization_sha256, Sha256Digest::of(&bytes));
    assert_eq!(
        result.record.preparations_sha256,
        original.preparations_sha256
    );
    assert_eq!(result.record.binding.as_signed(), Some(&refreshed));
    assert_eq!(result.record.created, local.prepared.header);
    assert_eq!(result.record.targets.len(), 2);
    assert_eq!(result.record.leases.targets.len(), 2);
    for (ordinal, (target, original_target)) in result
        .record
        .targets
        .iter()
        .zip(&initial.ordered_targets)
        .enumerate()
    {
        assert_eq!(&target.target, original_target);
        assert!(target.lease_generation > 0);
        assert_eq!(result.record.readbacks[ordinal].ordinal, ordinal as u32);
        assert!(matches!(
            result.record.readbacks[ordinal].readback,
            release::TargetReadback::Observed {
                comparison: release::RefComparison::Prestate,
                ..
            }
        ));
    }
    let snapshot = local.prepared.snapshot();
    let duplicate = writer
        .consume_release_authorization(
            &mut authority,
            &local.git(),
            local.request(&bytes),
            consume_options(),
        )
        .expect_err("one use cannot be repeated");
    assert!(matches!(
        duplicate.cause,
        release::ConsumeFailure::Store(store::Error::ReleaseAlreadyConsumed)
    ));
    assert!(duplicate.leases.is_none());
    assert!(duplicate.observations.is_none());
    assert_eq!(local.reads(), 2, "duplicate cannot sample providers again");
    assert_eq!(
        authority.calls, 3,
        "duplicate rechecks authority once but cannot progress to leases or destination reads"
    );
    local.prepared.unchanged(&snapshot);
    writer
        .close()
        .expect("complete coordinated close seals same-generation consumption");
    let body: release::Authorization =
        serde_json::from_slice(&bytes).expect("decode original authorization expiry");
    let mut expired = refreshed;
    expired.observed_at = body.expires_at;
    assert!(matches!(
        release::verify(&bytes, &expired, Default::default()),
        Err(release::Error::Time)
    ));
    let historical = store::inspect_release_consumption(
        &local.prepared.database,
        &body.record_id,
        Some(&local.prepared.header),
        read(),
    )
    .expect("clock-free independent complete intent reopening");
    assert_eq!(historical.record, Some(result.record));
    assert_eq!(
        store::inspect_release_authorization(
            &local.prepared.database,
            &body.record_id,
            Some(&local.prepared.header),
            read()
        )
        .expect("reopen original authorization after consumption")
        .record,
        Some(original)
    );
    assert_eq!(local.reads(), 2, "reopen never contacts destinations");
}

#[test]
fn one_moved_or_unavailable_destination_refuses_the_entire_pending_batch() {
    for unavailable in [false, true] {
        let mut local = Local::new(true);
        let (mut writer, bytes, binding) = local.register(false);
        if unavailable {
            fs::rename(&local.paths[0], local.paths[0].with_extension("withheld"))
                .expect("withhold disposable first destination without deleting history");
        } else {
            commit(&local.paths[0], "independent later destination movement");
        }
        let mut authority = local.authority(&binding, &binding);
        let error = writer
            .consume_release_authorization(
                &mut authority,
                &local.git(),
                local.request(&bytes),
                consume_options(),
            )
            .expect_err("no shrinking around failed target");
        assert!(
            matches!(
                error.cause,
                release::ConsumeFailure::Store(store::Error::ReleasePrestate)
            ),
            "{error}"
        );
        assert_eq!(
            error
                .leases
                .as_ref()
                .expect("complete acquired local leases survive refusal")
                .targets
                .len(),
            2
        );
        let observations = &error
            .observations
            .as_ref()
            .expect("full native read set retained")
            .targets;
        assert_eq!(observations.len(), 2);
        assert_eq!(observations[0].ordinal, 0);
        assert_eq!(observations[1].ordinal, 1);
        if unavailable {
            assert!(matches!(
                observations[0].readback,
                release::TargetReadback::Unavailable { .. }
            ));
        } else {
            assert!(matches!(
                observations[0].readback,
                release::TargetReadback::Observed {
                    comparison: release::RefComparison::Conflict,
                    ..
                }
            ));
        }
        assert!(
            matches!(
                observations[1].readback,
                release::TargetReadback::Observed {
                    comparison: release::RefComparison::Prestate,
                    ..
                }
            ),
            "unavailable={unavailable}; complete observations: {observations:?}"
        );
        assert!(
            local.reads() >= 1,
            "real configured provider called for remaining available target"
        );
        local.assert_pending(&bytes);
        writer
            .close()
            .expect("prestate refusal does not claim consumption or erase acquired leases");
    }
}

#[test]
fn changed_or_unavailable_authority_at_final_refresh_prevents_consumption() {
    for case in ["expiry", "revocation", "stop", "missing", "corrupt"] {
        let mut local = Local::new(true);
        let (mut writer, bytes, binding) = local.register(false);
        let mut refreshed = binding.clone();
        match case {
            "expiry" => {
                refreshed.observed_at = serde_json::from_slice::<release::Authorization>(&bytes)
                    .expect("decode original expiry for final authority observation")
                    .expires_at
            }
            "revocation" => refreshed.publishers[0].state = release::KeyState::Revoked,
            "stop" => refreshed.emergency_stop = true,
            "missing" | "corrupt" => {}
            _ => unreachable!(),
        }
        let mut authority = local.authority(&binding, &refreshed);
        match case {
            "missing" => fs::rename(
                &authority.paths[1],
                authority.paths[1].with_extension("withheld"),
            )
            .expect("withhold selected current authority source without deleting evidence"),
            "corrupt" => fs::write(&authority.paths[1], b"incomplete authority observation")
                .expect("corrupt only selected final fixture authority source"),
            _ => {}
        }
        let error = writer
            .consume_release_authorization(
                &mut authority,
                &local.git(),
                local.request(&bytes),
                consume_options(),
            )
            .expect_err("last current authority observation must gate CAS");
        assert_eq!(authority.calls, 2, "{case}: {error}");
        if matches!(case, "missing" | "corrupt") {
            assert!(
                matches!(error.cause, release::ConsumeFailure::Authority(cause)
                    if cause == if case == "missing" { release::AuthorityError::Unavailable } else { release::AuthorityError::Invalid }),
                "{case}: {error}"
            );
        } else {
            assert!(
                matches!(
                    error.cause,
                    release::ConsumeFailure::Verification(
                        release::PreparedAuthorizationError::Authorization(_)
                    )
                ),
                "{case}: {error}"
            );
        }
        assert_eq!(
            error
                .leases
                .as_ref()
                .expect("leases retained when final authority refuses")
                .targets
                .len(),
            2
        );
        assert_eq!(
            error
                .observations
                .as_ref()
                .expect("complete remote observations retained")
                .targets
                .len(),
            2
        );
        assert_eq!(
            local.reads(),
            2,
            "{case} changes at the final refresh after real complete reads"
        );
        local.assert_pending(&bytes);
        writer
            .close()
            .expect("authority refusal retains pending custody and lease history");
    }
}

#[test]
fn unanchored_wrong_coordinator_permissions_and_changed_output_refuse_before_remote_reads() {
    for case in [
        "plain",
        "coordinator",
        "consume_permission",
        "lease_permission",
        "read_permission",
        "authority",
        "output",
    ] {
        let mut local = Local::new(case != "plain");
        let (mut writer, bytes, binding) = local.register(case == "coordinator");
        let mut settings = consume_options();
        match case {
            "consume_permission" => settings.consume.allow_consume = false,
            "lease_permission" => settings.leases.allow_acquire = false,
            "read_permission" => settings.read.allow_read = false,
            "output" => fs::write(
                local.prepared.records[0]
                    .intent
                    .address
                    .path()
                    .join("model.json"),
                b"changed native output after registration",
            )
            .expect("controlled native output drift after registration"),
            _ => {}
        }
        let mut authority = local.authority(&binding, &binding);
        if case == "authority" {
            fs::write(
                &authority.paths[0],
                b"not a complete public authority binding",
            )
            .expect("replace selected fixture authority with malformed source bytes");
        }
        let error = writer
            .consume_release_authorization(
                &mut authority,
                &local.git(),
                local.request(&bytes),
                settings,
            )
            .expect_err("required gates precede every provider read");
        assert_eq!(local.reads(), 0, "{case}: {error}");
        assert!(error.leases.is_none(), "{case}");
        assert!(error.observations.is_none(), "{case}");
        match case {
            "plain" | "coordinator" => assert!(
                matches!(
                    error.cause,
                    release::ConsumeFailure::Store(store::Error::CoordinatorMismatch)
                ),
                "{case}: {error}"
            ),
            "consume_permission" => assert!(matches!(
                error.cause,
                release::ConsumeFailure::Store(store::Error::ReleaseConsumptionPermission)
            )),
            "lease_permission" => assert!(matches!(
                error.cause,
                release::ConsumeFailure::Store(store::Error::ReleaseLeasePermission)
            )),
            "read_permission" => assert!(matches!(error.cause, release::ConsumeFailure::Read(_))),
            "authority" => assert!(matches!(
                error.cause,
                release::ConsumeFailure::Authority(release::AuthorityError::Invalid)
            )),
            "output" => assert!(matches!(
                error.cause,
                release::ConsumeFailure::Verification(
                    release::PreparedAuthorizationError::Preparation(_)
                )
            )),
            _ => unreachable!(),
        }
        local.assert_pending(&bytes);
        let _closed = writer.close();
    }
}
