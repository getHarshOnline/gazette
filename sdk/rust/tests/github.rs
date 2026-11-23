//! Native process wiring with an owned pinned transport executable; no network/provider claim.
#![cfg(unix)]

use gazette_sdk::{github, process, source::Sha256Digest};
use std::{fs, os::unix::fs::PermissionsExt};

const COMMIT: &str = "0123456789012345678901234567890123456789";

struct Fixture {
    root: tempfile::TempDir,
    curl: github::Curl,
}

impl Fixture {
    fn new(body: &str, trailer: &str, exit: i32) -> Self {
        let root = tempfile::tempdir().expect("owned native fixture");
        let program = root.path().join("transport");
        let script = format!(
            "#!/bin/sh\n[ -z \"${{HOME+x}}\" ] || exit 91\n[ -z \"${{GITHUB_TOKEN+x}}\" ] || exit 92\n[ -z \"${{HTTPS_PROXY+x}}\" ] || exit 93\n[ \"$1\" = --disable ] || exit 94\n[ \"$#\" = 21 ] || exit 95\nfor argument do last=$argument; done\n[ \"$last\" = https://api.github.com/repos/owner/publication/pages/builds/42 ] || exit 96\nprintf '%s' '{body}'\nprintf '\\n%s' '{trailer}'\nexit {exit}\n"
        );
        fs::write(&program, &script).expect("owned executable bytes");
        fs::set_permissions(&program, fs::Permissions::from_mode(0o700))
            .expect("executable fixture");
        let curl = github::Curl {
            program: program.to_str().expect("native path").into(),
            directory: root.path().to_str().expect("native directory").into(),
            executable: process::ExecutablePin {
                bytes: script.len() as u64,
                sha256: Sha256Digest::of(script.as_bytes()),
            },
        };
        Self { root, curl }
    }

    fn read(&self, options: github::Options) -> Result<github::PagesObservation, github::Error> {
        github::observe_pages_build(
            &self.curl,
            &github::PagesBuild {
                remote: "https://github.com/owner/publication.git".into(),
                build_id: 42,
                expected_commit: COMMIT.parse().expect("commit"),
            },
            options,
        )
    }
}

fn options() -> github::Options {
    github::Options {
        allow_read: true,
        allow_execute: true,
        allow_network: true,
        ..Default::default()
    }
}

fn body() -> String {
    format!(
        r#"{{"url":"https://api.github.com/repos/owner/publication/pages/builds/42","commit":"{COMMIT}","status":"built"}}"#
    )
}

#[test]
fn sdk_reads_complete_pinned_native_response_without_an_ambient_environment() {
    let fixture = Fixture::new(&body(), "200", 0);
    let observed = fixture.read(options()).expect("native fixture transport");
    assert_eq!(
        observed.response_sha256,
        Sha256Digest::of(body().as_bytes())
    );
    assert_eq!(observed.response_bytes, body().len());
    let github::HttpOutcome::Observed { build } = observed.outcome else {
        panic!("native record")
    };
    assert_eq!(build.state, github::BuildState::Built);
    assert!(build.matches_expected_commit);
    assert_eq!(
        fs::read_dir(fixture.root.path())
            .expect("owned fixture tree")
            .count(),
        1
    );
}

#[test]
fn sdk_refuses_permission_changed_executable_and_unsuccessful_process_evidence() {
    let fixture = Fixture::new(&body(), "200", 0);
    assert_eq!(
        fixture.read(github::Options::default()).unwrap_err(),
        github::Error::Permission
    );
    fs::write(&fixture.curl.program, b"#!/bin/sh\nexit 0\n")
        .expect("change owned executable after selection");
    assert_eq!(
        fixture.read(options()).unwrap_err(),
        github::Error::Transport {
            failure: process::Failure::Executable,
            cleanup: process::Cleanup::NotStarted,
        }
    );
    let fixture = Fixture::new(&body(), "200", 8);
    assert_eq!(
        fixture.read(options()).unwrap_err(),
        github::Error::Response
    );
    let fixture = Fixture::new(&body(), "20x", 0);
    assert_eq!(
        fixture.read(options()).unwrap_err(),
        github::Error::Response
    );
}

#[test]
fn sdk_retains_http_denial_and_native_output_limit_without_partial_observation() {
    let fixture = Fixture::new("private access denial", "403", 0);
    let observation = fixture
        .read(options())
        .expect("complete denied HTTP response");
    assert!(matches!(
        observation.outcome,
        github::HttpOutcome::ForbiddenOrRateLimited
    ));
    let fixture = Fixture::new(&body(), "200", 0);
    assert_eq!(
        fixture
            .read(github::Options {
                response_bytes: 8,
                ..options()
            })
            .unwrap_err(),
        github::Error::Transport {
            failure: process::Failure::OutputLimit,
            cleanup: process::Cleanup::Reaped,
        }
    );
}
