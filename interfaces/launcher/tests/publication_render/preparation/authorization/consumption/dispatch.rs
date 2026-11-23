//! Complete same-holder attempts over genuinely prepared and consumed local targets.

use super::{Local, Repository, consume_options, fs, read, release, store, verification};
use std::{os::unix::fs::PermissionsExt, path::Path, time::Duration};

struct Fixture {
    local: Local,
    writer: store::Writer,
    bytes: Vec<u8>,
    binding: release::Binding,
    consumed: store::ReleaseConsumption,
    id: String,
}

impl Fixture {
    fn new() -> Self {
        let mut local = Local::new(true);
        let (mut writer, bytes, binding) = local.register(false);
        let mut authority = local.authority(&binding, &binding);
        let consumed = writer.consume_release_authorization(
            &mut authority, &local.git(), local.request(&bytes), consume_options(),
        ).expect("actual signed pending registration, anchored writer and complete native consumption").record;
        assert_eq!(authority.calls, 2);
        let id = consumed.authorization_id.clone();
        Self {
            local,
            writer,
            bytes,
            binding,
            consumed,
            id,
        }
    }

    fn history(&self) -> store::ReleaseAttemptInspection {
        store::inspect_release_attempts(
            &self.local.prepared.database,
            &self.id,
            Some(&self.local.prepared.header),
            read(),
        )
        .expect("independent durable attempt inspection")
    }

    fn dispatch(
        &mut self,
        number: u64,
    ) -> Result<store::ReleaseAttempt, release::DispatchAttemptError> {
        let mut authority = self.local.authority(&self.binding, &self.binding);
        self.writer.dispatch_release_attempt(
            &mut authority,
            &self.local.git(),
            request(&self.local, &self.bytes, number),
            options(),
        )
    }

    fn assert_targets(&self, first_updated: bool) {
        for (ordinal, target) in self.binding.ordered_targets.iter().enumerate() {
            let expected = if first_updated && ordinal == 0 {
                &target.local_commit_oid
            } else {
                let release::RemoteState::Commit { oid } = &target.expected_remote_oid_or_unborn
                else {
                    panic!("fixture has actual advertised parents");
                };
                oid
            };
            let repo = Repository::open_bare(&self.local.paths[ordinal])
                .expect("independent receiver inspection");
            assert_eq!(
                repo.find_reference("refs/heads/publication")
                    .expect("actual target branch")
                    .target()
                    .expect("direct branch")
                    .to_string(),
                expected.as_str()
            );
            assert_eq!(
                repo.references()
                    .expect("complete native destination refs")
                    .count(),
                1
            );
        }
    }
}

fn request<'a>(
    local: &'a Local,
    bytes: &'a [u8],
    number: u64,
) -> release::DispatchAttemptRequest<'a> {
    release::DispatchAttemptRequest {
        authorization: bytes,
        preparations: local.prepared.request(&local.prepared.selections),
        ordinal: 0,
        number,
    }
}

fn options() -> release::DispatchAttemptOptions {
    release::DispatchAttemptOptions {
        verification: verification(),
        read: release::RemoteReadOptions {
            allow_read: true,
            timeout: Duration::from_secs(10),
            ..Default::default()
        },
        record: store::ReleaseAttemptOptions { allow_record: true },
        dispatch: release::DispatchOptions {
            allow_dispatch: true,
            timeout: Duration::from_secs(10),
            ..Default::default()
        },
    }
}

fn wrapper(path: &Path, body: &str) {
    fs::write(path, format!("#!/bin/sh\n{body}\n")).expect("disposable real-Git wrapper");
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .expect("private fixture executable");
}

#[test]
fn native_start_probe() {
    let Some(database) = std::env::var_os("GAZETTE_ATTEMPT_PROBE_DATABASE") else {
        return;
    };
    let id = std::env::var("GAZETTE_ATTEMPT_PROBE_ID").expect("selected original authorization ID");
    let marker = std::env::var_os("GAZETTE_ATTEMPT_PROBE_MARKER")
        .expect("disposable probe completion marker");
    let inspected = store::inspect_release_attempts(Path::new(&database), &id, None, read())
        .expect("durable read in separate process before Git starts");
    assert_eq!(inspected.attempts.len(), 1);
    assert_eq!(inspected.attempts[0].start.key.ordinal, 0);
    assert_eq!(inspected.attempts[0].start.key.number, 1);
    assert!(
        inspected.attempts[0].result.is_none(),
        "provider has not started yet"
    );
    fs::write(marker, b"native durable start observed before dispatch")
        .expect("probe actually ran");
}

#[test]
fn durable_start_precedes_real_push_and_exact_replay_never_dispatches_again() {
    let mut fixture = Fixture::new();
    let marker = fixture.local.control.path().join("before-push-probe");
    let test = std::env::current_exe().expect("current native test executable");
    wrapper(
        &fixture.local.program,
        &format!(
            "printf x >> '{}'\nfor arg do\n if [ \"$arg\" = push ]; then\n GAZETTE_ATTEMPT_PROBE_DATABASE='{}' GAZETTE_ATTEMPT_PROBE_ID='{}' GAZETTE_ATTEMPT_PROBE_MARKER='{}' '{}' --exact preparation::authorization::consumption::dispatch::native_start_probe >/dev/null || exit 8\n test -f '{}' || exit 9\n fi\ndone\nexec {} \"$@\"",
            fixture.local.calls.display(),
            fixture.local.prepared.database.display(),
            fixture.id,
            marker.display(),
            test.display(),
            marker.display(),
            crate::native::shell(crate::native::git())
        ),
    );
    let mut refreshed = fixture.binding.clone();
    refreshed.observed_at += 1;
    let mut authority = fixture.local.authority(&fixture.binding, &refreshed);
    let attempt = fixture
        .writer
        .dispatch_release_attempt(
            &mut authority,
            &fixture.local.git(),
            request(&fixture.local, &fixture.bytes, 1),
            options(),
        )
        .expect("connected exact-parent native push and durable result");
    assert_eq!(authority.calls, 2);
    assert_eq!(attempt.start.binding.as_signed(), Some(&refreshed));
    assert!(
        marker.is_file(),
        "separate process observed durable start before native Git invocation"
    );
    let result = attempt
        .result
        .as_ref()
        .expect("native report and independent read-back retained");
    assert_eq!(
        result
            .transport
            .expect("complete native transport report")
            .status,
        release::DispatchStatus::Updated
    );
    assert!(matches!(
        result.readback,
        release::TargetReadback::Observed {
            comparison: release::RefComparison::Intended,
            ..
        }
    ));
    fixture.assert_targets(true);
    let calls = fixture.local.reads();
    assert_eq!(
        fixture.dispatch(1).expect("exact replay returns history"),
        attempt
    );
    assert_eq!(
        fixture.local.reads(),
        calls,
        "history replay must not invoke the provider"
    );
    let history = fixture.history();
    assert_eq!(history.attempts, vec![attempt.clone()]);
    assert_eq!(
        history.consumption,
        Some(fixture.consumed.clone()),
        "dispatching one target cannot shrink or rewrite two-target consumed intent"
    );
    fixture
        .writer
        .close()
        .expect("seal completed same-holder attempt");
    let history = store::inspect_release_attempts(
        &fixture.local.prepared.database,
        &fixture.id,
        Some(&fixture.local.prepared.header),
        read(),
    )
    .expect("clock-free history survives coordinated close");
    assert_eq!(history.attempts, vec![attempt]);
}

#[test]
fn changed_output_or_final_stop_refuses_before_durable_start_and_push() {
    for stopped in [false, true] {
        let mut fixture = Fixture::new();
        let mut refreshed = fixture.binding.clone();
        if stopped {
            refreshed.emergency_stop = true;
        } else {
            fs::write(
                fixture.local.prepared.records[0]
                    .intent
                    .address
                    .path()
                    .join("model.json"),
                b"changed after consumption",
            )
            .expect("actual prepared output drift");
        }
        let before = fixture.local.reads();
        let mut authority = fixture.local.authority(&fixture.binding, &refreshed);
        let error = fixture
            .writer
            .dispatch_release_attempt(
                &mut authority,
                &fixture.local.git(),
                request(&fixture.local, &fixture.bytes, 1),
                options(),
            )
            .expect_err("fresh authority and actual prepared bytes precede attempt start");
        assert!(fixture.history().attempts.is_empty());
        assert_eq!(
            fixture.history().consumption,
            Some(fixture.consumed.clone())
        );
        fixture.assert_targets(false);
        if stopped {
            assert_eq!(authority.calls, 2);
            assert!(
                matches!(
                    error.prestate.as_deref(),
                    Some(release::TargetReadback::Observed {
                        comparison: release::RefComparison::Prestate,
                        ..
                    })
                ),
                "the observed prestate survives later authority refusal"
            );
        } else {
            assert_eq!(fixture.local.reads(), before);
            assert!(
                error.prestate.is_none(),
                "output gate failed before destination acquisition"
            );
        }
        fixture
            .writer
            .close()
            .expect("pre-start refusal does not poison held writer");
    }
}

#[test]
fn moved_prestate_after_consumption_retains_native_conflict_without_an_attempt() {
    let mut fixture = Fixture::new();
    let moved = super::commit(
        &fixture.local.paths[0],
        "independent native owner moved after consumption",
    );
    let before = fixture.local.reads();
    let error = fixture
        .dispatch(1)
        .expect_err("consumption is not authority to overwrite later movement");
    assert!(
        matches!(error.prestate.as_deref(), Some(release::TargetReadback::Observed {
        advertisement: release::RefAdvertisement::Commit { oid },
        comparison: release::RefComparison::Conflict,
    }) if oid == &moved)
    );
    assert!(error.started.is_none());
    assert!(error.observed.is_none());
    assert_eq!(
        fixture.local.reads(),
        before + 1,
        "only one fresh prestate read, never a push"
    );
    assert!(fixture.history().attempts.is_empty());
    assert_eq!(
        fixture.history().consumption,
        Some(fixture.consumed.clone())
    );
    fixture
        .writer
        .close()
        .expect("refused prestate preserves current custody and original intent");
}

#[test]
fn lost_response_after_real_push_is_retained_and_never_grants_automatic_retry() {
    let mut fixture = Fixture::new();
    let pushes = fixture.local.control.path().join("push-calls");
    wrapper(
        &fixture.local.program,
        &format!(
            "printf x >> '{}'\nfor arg do\n if [ \"$arg\" = push ]; then\n printf p >> '{}'\n {git} \"$@\" >/dev/null\n exit 7\n fi\ndone\nexec {git} \"$@\"",
            fixture.local.calls.display(),
            pushes.display(),
            git = crate::native::shell(crate::native::git())
        ),
    );
    let attempt = fixture
        .dispatch(1)
        .expect("failed transport with complete independent read-back is durable evidence");
    let result = attempt
        .result
        .as_ref()
        .expect("uncertainty is retained, not a missing result");
    assert!(result.transport.is_err());
    assert_eq!(
        result
            .transport
            .expect_err("lost response is not success")
            .effect,
        release::DispatchEffect::Unverified
    );
    assert!(matches!(
        result.readback,
        release::TargetReadback::Observed {
            comparison: release::RefComparison::Intended,
            ..
        }
    ));
    assert!(!result.permits_retry());
    fixture.assert_targets(true);
    let before = fixture.local.reads();
    assert_eq!(
        fixture.dispatch(1).expect("exact failed-result replay"),
        attempt
    );
    assert_eq!(fixture.local.reads(), before);
    fixture
        .dispatch(2)
        .expect_err("observed intended state forbids a new automatic attempt");
    assert_eq!(
        fs::read(pushes).expect("actual push invocation log"),
        b"p",
        "refused retry never invokes another push; fresh read-only observations are allowed"
    );
    assert_eq!(fixture.history().attempts, vec![attempt]);
    fixture
        .writer
        .close()
        .expect("retain exact transport uncertainty and read-back");
}
