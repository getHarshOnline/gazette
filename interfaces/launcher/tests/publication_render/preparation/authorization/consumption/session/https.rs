//! One admitted held release over authenticated loopback TLS, without provider emulation.

use super::super::{Local, Prepared, commit};
use super::*;
use git2::Repository;
use serde_json::Value;
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const SECRET: &str = "PRIVATE_HELPER_SECRET";

struct Server {
    child: Child,
    root: PathBuf,
    endpoint: String,
}

impl Server {
    fn start(root: &Path, shared_certificate: Option<&Path>) -> Self {
        let root = fs::canonicalize(root).expect("private fixture server root");
        if let Some(source) = shared_certificate {
            fs::copy(source.join("key.pem"), root.join("key.pem")).expect("shared fixture TLS key");
            fs::copy(source.join("certificate.pem"), root.join("certificate.pem"))
                .expect("shared fixture TLS certificate");
        } else {
            let config = root.join("openssl.cnf");
            fs::write(
                &config,
                "[req]\ndistinguished_name=subject\nx509_extensions=extensions\nprompt=no\n[subject]\nCN=localhost\n[extensions]\nsubjectAltName=DNS:localhost\nbasicConstraints=critical,CA:TRUE\nkeyUsage=critical,keyCertSign,digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n",
            )
            .expect("fixture certificate configuration");
            let mut certificate = Command::new(crate::native::openssl());
            certificate
                .env_clear()
                .args([
                    "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-sha256", "-days", "1",
                    "-config",
                ])
                .arg(&config)
                .arg("-keyout")
                .arg(root.join("key.pem"))
                .arg("-out")
                .arg(root.join("certificate.pem"))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            assert!(
                wait(
                    &mut certificate.spawn().expect("native OpenSSL fixture"),
                    Duration::from_secs(20),
                )
                .success()
            );
        }
        fs::set_permissions(root.join("key.pem"), fs::Permissions::from_mode(0o600))
            .expect("private fixture key");
        fs::create_dir(root.join("server-home")).expect("isolated native backend home");
        fs::write(root.join("mode"), "authenticated").expect("authenticated endpoint profile");
        fs::write(
            root.join("server.py"),
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../adapters/git/tests/dispatch/https/server.py"
            )),
        )
        .expect("reuse owned loopback TLS Git backend");
        Repository::open_bare(root.join("destination.git"))
            .expect("fixture receiver")
            .config()
            .expect("fixture receiver config")
            .set_bool("http.receivepack", true)
            .expect("explicit fixture receive-pack");
        let stderr =
            fs::File::create(root.join("server.stderr")).expect("private server diagnostics");
        let child = Command::new(crate::native::python())
            .env_clear()
            .args(["-I", "-B"])
            .arg(root.join("server.py"))
            .arg(&root)
            .arg(crate::native::git_http_backend())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(stderr)
            .spawn()
            .expect("native Python TLS fixture");
        let mut server = Self {
            child,
            root,
            endpoint: String::new(),
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Ok(endpoint) = fs::read_to_string(server.root.join("endpoint")) {
                server.endpoint = endpoint;
                return server;
            }
            assert!(
                server
                    .child
                    .try_wait()
                    .expect("fixture startup status")
                    .is_none(),
                "TLS fixture exited: {}",
                fs::read_to_string(server.root.join("server.stderr")).unwrap_or_default()
            );
            assert!(
                Instant::now() < deadline,
                "TLS fixture did not become ready"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn requests(&self) -> Vec<Value> {
        fs::read_to_string(self.root.join("requests.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).expect("complete fixture observation"))
            .filter(|record: &Value| record["event"] == "request")
            .collect()
    }

    fn assert_authenticated_tls(&self) {
        let requests = self.requests();
        assert!(
            !requests.is_empty(),
            "connected flow reaches each selected server"
        );
        assert!(requests.iter().any(|request| {
            request["authorization"] == "Basic Zml4dHVyZTpQUklWQVRFX0hFTFBFUl9TRUNSRVQ="
        }));
        for request in requests {
            assert!(
                request["authorization"].is_null()
                    || request["authorization"] == "Basic Zml4dHVyZTpQUklWQVRFX0hFTFBFUl9TRUNSRVQ="
            );
            assert!(matches!(
                request["tls"].as_str(),
                Some("TLSv1.2" | "TLSv1.3")
            ));
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = fs::write(self.root.join("stop-server"), b"stop");
        let deadline = Instant::now() + Duration::from_secs(12);
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => break,
                Err(_) => {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    break;
                }
                Ok(None) if Instant::now() >= deadline => {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    break;
                }
                Ok(None) => thread::sleep(Duration::from_millis(10)),
            }
        }
    }
}

fn wait(child: &mut Child, timeout: Duration) -> std::process::ExitStatus {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().expect("inspect fixture child") {
            return status;
        }
        if Instant::now() >= deadline {
            child.kill().expect("stop overdue fixture process");
            let _ = child.wait();
            panic!("fixture process exceeded bounded deadline");
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn executable(path: &Path, body: &str) {
    fs::write(path, format!("#!/bin/sh\n{body}\n")).expect("private fixture executable");
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .expect("private fixture executable mode");
}

fn selected(allow_credentials: bool) -> (Selected, [Server; 2]) {
    let remotes = super::super::private_directory();
    let roots = [remotes.path().join("one"), remotes.path().join("other")];
    for root in &roots {
        fs::create_dir(root).expect("private fixture server directory");
    }
    let paths = roots.each_ref().map(|root| root.join("destination.git"));
    for path in &paths {
        Repository::init_bare(path).expect("real parent source and TLS destination");
    }
    let parents = std::array::from_fn(|ordinal| {
        let oid = commit(&paths[ordinal], &format!("reviewed parent {ordinal}"));
        (
            gazette_sdk::release_commit::ParentSource::Bare {
                path: paths[ordinal].clone(),
            },
            oid,
        )
    });
    let first = Server::start(&roots[0], None);
    let second = Server::start(&roots[1], Some(&roots[0]));
    let servers = [first, second];
    let destinations = [servers[0].endpoint.as_str(), servers[1].endpoint.as_str()];
    let prepared = Prepared::with_candidate_qualification(
        true,
        Some(destinations),
        Some("coordinator:fixture"),
        Some(&parents),
        true,
        true,
    );
    let control = super::super::private_directory();
    let calls = control.path().join("git-calls");
    let trust = control.path().join("trusted-cas.pem");
    fs::copy(roots[0].join("certificate.pem"), &trust).expect("selected fixture trust anchor");
    let program = control.path().join("trusted-fixture-git");
    executable(
        &program,
        &format!(
            "printf x >> '{}'\nexec {} -c 'http.sslCAInfo={}' \"$@\"",
            calls.display(),
            crate::native::shell(crate::native::git()),
            trust.display()
        ),
    );
    let helper = control.path().join("credential-helper");
    executable(
        &helper,
        "case \"$1\" in\n get) cat >/dev/null; printf 'username=fixture\\npassword=PRIVATE_HELPER_SECRET\\n\\n' ;;\n store|erase) cat >/dev/null ;;\n *) exit 92 ;;\nesac",
    );
    let isolation = super::super::private_directory();
    let local = Local {
        prepared,
        _remotes: remotes,
        paths: paths.to_vec(),
        isolation,
        control,
        program: program.clone(),
        calls,
    };
    let git = session::GitSelection {
        program,
        isolation: local.isolation.path().to_path_buf(),
        credentials: Some(release::GitCredentialSelection {
            helper,
            allow_credentials,
        }),
    };
    (Selected::with_local(local, true, git), servers)
}

fn network_consume_options() -> release::ConsumeAuthorizationOptions {
    let mut options = consume_options();
    options.read.allow_network = true;
    options
}

fn dispatch_options() -> release::DispatchAttemptOptions {
    release::DispatchAttemptOptions {
        verification: verification(),
        read: release::RemoteReadOptions {
            allow_read: true,
            allow_network: true,
            timeout: Duration::from_secs(10),
            ..Default::default()
        },
        record: store::ReleaseAttemptOptions { allow_record: true },
        dispatch: release::DispatchOptions {
            allow_dispatch: true,
            allow_network: true,
            timeout: Duration::from_secs(10),
            ..Default::default()
        },
    }
}

fn assert_redacted(value: &impl serde::Serialize) {
    let encoded = serde_json::to_string(value).expect("serialize retained public evidence");
    assert!(
        !encoded.contains(SECRET),
        "credential value cannot enter evidence"
    );
}

#[test]
fn authenticated_https_session_consumes_pushes_reads_back_and_settles_exact_admitted_set() {
    let (selected, servers) = selected(true);
    assert_redacted(&selected.open);
    assert_redacted(&selected.local.prepared.inspect());
    let mut fixture = Fixture::from_selected(selected);
    let header = fixture.local.prepared.header;
    let registered = fixture.register();
    assert_redacted(&registered);
    let consumed = fixture.execute(Operation::Consume {
        expected: header,
        input: fixture.input.clone(),
        options: network_consume_options(),
    });
    assert_redacted(&consumed);
    let Outcome::Consumed { receipt: consumed } = consumed else {
        panic!("authenticated exact pre-state acquisition and consumption: {consumed:?}")
    };
    for ordinal in 0..2 {
        assert!(matches!(
            consumed.record.readbacks[ordinal].readback,
            release::TargetReadback::Observed {
                comparison: release::RefComparison::Prestate,
                ..
            }
        ));
        let dispatched = fixture.execute(Operation::Dispatch {
            expected: header,
            input: fixture.input.clone(),
            ordinal: ordinal as u32,
            number: 1,
            options: dispatch_options(),
        });
        assert_redacted(&dispatched);
        let Outcome::Dispatched { attempt } = dispatched else {
            panic!("exact non-force HTTPS push: {dispatched:?}")
        };
        let result = attempt
            .result
            .expect("effect plus independent authenticated read-back");
        assert_eq!(
            result.transport.expect("complete HTTPS transport").status,
            release::DispatchStatus::Updated
        );
        assert!(matches!(
            result.readback,
            release::TargetReadback::Observed {
                comparison: release::RefComparison::Intended,
                ..
            }
        ));
    }
    let settled = fixture.execute(Operation::Reconcile {
        expected: header,
        authorization_id: consumed.record.authorization_id,
        number: 1,
        options: release::ReconcileReleaseOptions {
            read: release::RemoteReadOptions {
                allow_read: true,
                allow_network: true,
                timeout: Duration::from_secs(10),
                ..Default::default()
            },
            record: store::ReleaseReconciliationOptions {
                allow_record: true,
                ..Default::default()
            },
            ..Default::default()
        },
    });
    assert_redacted(&settled);
    let Outcome::Reconciled { receipt } = settled else {
        panic!("complete authenticated read-back settlement: {settled:?}")
    };
    assert_eq!(
        receipt.record.settlement.state,
        store::ReleaseSettlementState::Succeeded
    );
    for server in &servers {
        server.assert_authenticated_tls();
    }
    assert!(matches!(fixture.execute(Operation::Close), Outcome::Closed));
}

#[test]
fn selected_helper_without_permission_refuses_before_any_https_request_or_ref_effect() {
    let (selected, servers) = selected(false);
    assert_redacted(&selected.open);
    let parents = selected
        .local
        .paths
        .iter()
        .map(|path| {
            Repository::open_bare(path)
                .expect("fixture receiver")
                .refname_to_id("refs/heads/publication")
                .expect("original parent")
        })
        .collect::<Vec<_>>();
    let mut fixture = Fixture::from_selected(selected);
    fixture.register();
    let outcome = fixture.execute(Operation::Consume {
        expected: fixture.local.prepared.header,
        input: fixture.input.clone(),
        options: network_consume_options(),
    });
    assert_redacted(&outcome);
    let diagnostics = format!("{outcome:?}");
    assert!(!diagnostics.contains(SECRET));
    let Outcome::Failed { failure } = outcome else {
        panic!("denied helper must refuse the pre-state acquisition: {outcome:?}")
    };
    assert!(
        matches!(
            failure.cause,
            session::Cause::Store {
                code: "store_release_prestate"
            }
        ),
        "{failure:?}"
    );
    let session::Evidence::Consumption {
        observations: Some(observations),
        ..
    } = failure.evidence
    else {
        panic!("permission refusals retain complete unavailable observations")
    };
    assert!(observations.targets.iter().all(|row| matches!(
        row.readback,
        release::TargetReadback::Unavailable {
            error: release::RemoteReadError {
                failure: release::RemoteReadFailure::Permission,
                ..
            }
        }
    )));
    for (ordinal, server) in servers.iter().enumerate() {
        assert!(server.requests().is_empty(), "no HTTPS request may start");
        assert_eq!(
            Repository::open_bare(&fixture.local.paths[ordinal])
                .expect("fixture receiver")
                .refname_to_id("refs/heads/publication")
                .expect("unchanged parent"),
            parents[ordinal]
        );
    }
    fixture
        .local
        .assert_pending(fixture.input.authorization.as_bytes());
    assert!(matches!(fixture.execute(Operation::Close), Outcome::Closed));
}
