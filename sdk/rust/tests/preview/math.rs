//! Real preview effects: failed input is held, not implicitly retried by polling.

use super::*;
use gazette_sdk::{
    preview::watch::{BuildError, Control, Event},
    process,
};

fn configured(fail_generation: bool) -> (Fixture, std::path::PathBuf) {
    // Provider prerequisites must fail before Fixture owns the shared port mutex.
    // Otherwise one unsupported runtime poisons unrelated native preview tests.
    let program = super::native::node().to_path_buf();
    let mut fixture = Fixture::new();
    let counter = fixture.directory.path().join("math-attempts.txt");
    let executable = fs::read(&program).expect("runtime pin bytes");
    let arguments = vec![counter.to_string_lossy().into_owned()];
    let entrypoint = if fail_generation {
        "fail.mjs"
    } else {
        "wrapper.mjs"
    };
    let commit = git(fixture.directory.path(), &["rev-parse", "HEAD"]);
    fixture.config = format!(
        "math_typesetter={{program={},executable={{bytes={},sha256='{}'}},arguments={},directory={},environment={{}},source='source:math',entrypoint={}}}\n[[sources]]\nid='source:math'\nlocation={{kind='repository',root={},commit='{}',subdirectory='math'}}\n{}",
        serde_json::to_string(&program).expect("program string"),
        executable.len(),
        Sha256Digest::of(&executable),
        serde_json::to_string(&arguments).expect("literal arguments"),
        serde_json::to_string(fixture.directory.path()).expect("working directory"),
        serde_json::to_string(entrypoint).expect("entrypoint"),
        serde_json::to_string(fixture.directory.path()).expect("repository root"),
        commit.trim(),
        fixture.config.replace(
            "authored=[{source='source:one',target='one'}]",
            "authored=[{source='source:one',target='one',markdown_profile='publication-math'}]"
        ),
    );
    (fixture, counter)
}

#[test]
fn failed_math_generation_and_later_render_failure_hold_exact_inputs_without_reexecution() {
    for fail_generation in [true, false] {
        let (fixture, counter) = configured(fail_generation);
        let source = fixture.directory.path().join("one/page.md");
        let template = fixture.directory.path().join("theme/templates/page.html");
        let original = fs::read_to_string(&source).expect("original source");
        let original_template = fs::read_to_string(&template).expect("original template");
        let failed_source = format!("{original}\n$x$\n");
        let failed_template = if fail_generation {
            original_template.as_str()
        } else {
            "{% if %}"
        };
        let mut prior = None;
        let mut failed_at = None;
        let mut first_error = None;
        let mut failures = 0;
        let mut restored = false;
        let mut recovered = false;
        let started = Instant::now();
        // Bound polling without progress, not the complete native lifecycle:
        // the provider has its own child deadline and exact executable-byte
        // checks outside it. Those checks must not be weakened to fit this guard.
        let mut progress_deadline = started + Duration::from_secs(15);
        super::watch::run_with_math(
            &fixture,
            true,
            Some(build::MathGenerationOptions {
                execution: process::Options {
                    allow_execute: true,
                    ..Default::default()
                },
                ..Default::default()
            }),
            |event| {
                let (phase, error_code) = match &event {
                    Event::Poll => ("poll", None),
                    Event::Ready { .. } => ("ready", None),
                    Event::Rebuilt { .. } => ("rebuilt", None),
                    Event::Recovered { .. } => ("recovered", None),
                    Event::RebuildFailed { error, .. } => ("failed", Some(error.code())),
                    Event::Closed => ("closed", None),
                };
                if phase != "poll" {
                    eprintln!(
                        "math watch: provider_failure={fail_generation} phase={phase} elapsed={:?} failures={failures} restored={restored} recovered={recovered} error={error_code:?}",
                        started.elapsed()
                    );
                }
                if matches!(&event, Event::Poll) {
                    assert!(
                        Instant::now() < progress_deadline,
                        "bounded native watch progress: provider_failure={fail_generation} phase={phase} elapsed={:?} failures={failures} restored={restored} recovered={recovered} error={error_code:?} attempts={:?}",
                        started.elapsed(),
                        fs::read_to_string(&counter)
                    );
                } else if matches!(
                    &event,
                    Event::Ready { .. } | Event::RebuildFailed { .. } | Event::Recovered { .. }
                ) {
                    progress_deadline = Instant::now() + Duration::from_secs(15);
                }
                match event {
                    Event::Ready {
                        generation,
                        current,
                        ..
                    } => {
                        assert_eq!(generation, 1);
                        assert!(prior.is_none(), "initial ready is delivered exactly once");
                        assert!(
                            !counter.exists(),
                            "selected provider alone does not execute without math"
                        );
                        prior = Some((
                            current.input_identity().clone(),
                            current.sites()[&key("site:one")]
                                .output()
                                .inventory()
                                .identity(),
                        ));
                        fs::write(&source, &failed_source)?;
                        fs::write(&template, failed_template)?;
                    }
                    Event::RebuildFailed { generation, error } => {
                        failures += 1;
                        assert_eq!(
                            generation, 1,
                            "failed candidate does not replace retained output"
                        );
                        assert_eq!(
                            fs::read_to_string(&counter)?,
                            "attempt\n",
                            "same capture must never repeat the native effect"
                        );
                        if fail_generation {
                            let BuildError::Publication(build::PublicationSetError::Math(error)) =
                                error
                            else {
                                panic!("expected retained native math failure: {error:?}");
                            };
                            assert_eq!(error.code(), "math_exit");
                            assert_eq!(error.active_exit().and_then(|exit| exit.code), Some(7));
                            assert_eq!(
                                error.active_response(),
                                Some(b"native-failure\n".as_slice())
                            );
                            let request: serde_json::Value = serde_json::from_str(
                                error.active_request().expect("exact invocation request"),
                            )
                            .expect("native JSON");
                            assert_eq!(
                                request,
                                serde_json::json!({"formulas":[{"tex":"x","display":false}]})
                            );
                            assert_eq!(error.completed().count(), 0);
                        } else {
                            assert_eq!(
                                error.code(),
                                "preview_incomplete_generation",
                                "typesetting succeeds before the invalid Tera fails"
                            );
                        }
                        if failures == 1 {
                            first_error = Some(error.to_string());
                            failed_at = Some(Instant::now());
                        } else {
                            assert_eq!(failures, 2);
                            assert!(
                                recovered,
                                "polling identical failed inputs must not deliver repeated failures"
                            );
                            assert_eq!(Some(error.to_string()), first_error);
                            return Ok(Control::Stop);
                        }
                    }
                    Event::Poll => {
                        if failed_at
                            .is_some_and(|time| time.elapsed() >= Duration::from_millis(350))
                            && !restored
                        {
                            assert_eq!(fs::read_to_string(&counter)?, "attempt\n");
                            fs::write(&source, &original)?;
                            fs::write(&template, &original_template)?;
                            restored = true;
                        }
                    }
                    Event::Recovered {
                        generation,
                        current,
                        ..
                    } => {
                        assert_eq!(generation, 1);
                        assert!(restored && !recovered, "one recovery follows restoration");
                        let (identity, output) = prior.as_ref().expect("prior complete generation");
                        assert_eq!(current.input_identity(), identity);
                        assert_eq!(
                            current.sites()[&key("site:one")]
                                .output()
                                .inventory()
                                .identity(),
                            *output
                        );
                        assert_eq!(fs::read_to_string(&counter)?, "attempt\n");
                        recovered = true;
                        fs::write(&source, &failed_source)?;
                        fs::write(&template, failed_template)?;
                    }
                    Event::Rebuilt { .. } => {
                        panic!("failed candidate must never become a complete generation")
                    }
                    Event::Closed => (),
                }
                Ok(Control::Continue)
            },
        )
        .expect("SDK watch holds native failures and retained output");
        assert_eq!(failures, 2);
        assert!(recovered);
        assert_eq!(
            fs::read_to_string(counter).expect("native invocation count"),
            "attempt\n"
        );
    }
}

#[test]
fn preparation_report_retains_the_exact_native_math_failure_without_reexecution() {
    let (fixture, counter) = configured(true);
    let source = fixture.directory.path().join("one/page.md");
    let original = fs::read_to_string(&source).expect("source");
    fs::write(&source, format!("{original}\n$x$\n")).expect("selected math before startup");
    let error = super::watch::run_with_math(
        &fixture,
        false,
        Some(build::MathGenerationOptions {
            execution: process::Options {
                allow_execute: true,
                ..Default::default()
            },
            ..Default::default()
        }),
        |_| panic!("failed startup cannot publish preview events"),
    )
    .expect_err("actual native failure precedes listeners");
    let gazette_sdk::preview::watch::Error::Build(BuildError::Publication(
        build::PublicationSetError::Math(error),
    )) = error
    else {
        panic!("expected native math error");
    };
    let request = error.active_request().expect("native request").to_owned();
    let command = error
        .active_command_sha256()
        .expect("native command identity");
    let result = Err(build::LocalPreparationRunError::Math(error));
    let report = build::LocalPreparationRunReport::new(&result);
    let first = serde_json::to_value(&report).expect("shared preparation evidence");
    assert_eq!(first["status"], "math-generation-failed");
    assert_eq!(first["evidence"]["code"], "math_exit");
    assert_eq!(
        first["evidence"]["active_command_sha256"],
        serde_json::json!(command)
    );
    assert_eq!(first["evidence"]["active_request"], request);
    assert_eq!(
        first["evidence"]["active_response"],
        serde_json::json!(b"native-failure\n".as_slice())
    );
    assert_eq!(first["evidence"]["active_exit"]["code"], 7);
    assert_eq!(first["evidence"]["completed"], serde_json::json!([]));
    assert_eq!(
        first,
        serde_json::to_value(&report).expect("same retained evidence")
    );
    let schema = schemars::schema_for!(build::LocalPreparationRunReport<'_>);
    assert!(
        jsonschema::validator_for(schema.as_value())
            .expect("native report schema")
            .is_valid(&first)
    );
    assert_eq!(
        fs::read_to_string(counter).expect("native effect count"),
        "attempt\n"
    );
}
