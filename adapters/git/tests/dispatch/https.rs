//! Real TLS plus native git-http-backend, using one explicitly selected fixture Git installation.
//! The wrapper selects only the disposable CA; Gazette retains TLS verification and redirect rules.
//! This is not a product CA-selection API or immutable arbitrary CA-path qualification.

use super::*;
use serde_json::Value;
use std::{
    process::{Child, Command, Stdio},
    thread,
    time::Instant,
};

struct Server {
    child: Child,
    root: PathBuf,
    endpoint: String,
    program: PathBuf,
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

impl Server {
    fn start(fixture: &Fixture) -> Self {
        let root = fs::canonicalize(fixture.root.path()).expect("private fixture root");
        let config = root.join("openssl.cnf");
        fs::write(&config, "[req]\ndistinguished_name=subject\nx509_extensions=extensions\nprompt=no\n[subject]\nCN=localhost\n[extensions]\nsubjectAltName=DNS:localhost\nbasicConstraints=critical,CA:TRUE\nkeyUsage=critical,keyCertSign,digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n").expect("fixture certificate configuration");
        let mut certificate = Command::new(native::openssl());
        certificate
            .env_clear()
            .args([
                "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-sha256", "-days", "1", "-config",
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
                Duration::from_secs(20)
            )
            .success()
        );
        fs::set_permissions(root.join("key.pem"), fs::Permissions::from_mode(0o600))
            .expect("private fixture key");
        fs::create_dir(root.join("server-home")).expect("isolated native backend home");
        fs::write(root.join("mode"), "normal").expect("initial endpoint behavior");
        fs::write(root.join("server.py"), include_bytes!("https/server.py"))
            .expect("owned fixture server");
        fixture
            .remote
            .config()
            .expect("fixture receiver config")
            .set_bool("http.receivepack", true)
            .expect("explicit fixture anonymous receive-pack");
        let ca = format!("http.sslCAInfo={}", root.join("certificate.pem").display());
        let program = fixture.executable(
            "trusted-fixture-git",
            &format!(
                "exec {} -c '{}' \"$@\"",
                native::shell(native::git()),
                ca.replace('\'', "'\\''")
            ),
        );
        let stderr =
            fs::File::create(root.join("server.stderr")).expect("private server diagnostics");
        let child = Command::new(native::python())
            .env_clear()
            .args(["-I", "-B"])
            .arg(root.join("server.py"))
            .arg(&root)
            .arg(native::git_http_backend())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(stderr)
            .spawn()
            .expect("native Python TLS fixture");
        let mut server = Self {
            child,
            root,
            endpoint: String::new(),
            program,
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Ok(endpoint) = fs::read_to_string(server.root.join("endpoint")) {
                server.endpoint = endpoint;
                break;
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
        server
    }

    fn mode(&self, mode: &str) {
        fs::write(self.root.join("mode"), mode).expect("selected fixture behavior");
    }

    fn credential_helper(&self) -> PathBuf {
        let path = self.root.join("credential-helper");
        executable(
            &path,
            r#"
test -z "$GH_TOKEN$GITHUB_TOKEN$GIT_ASKPASS$SSH_ASKPASS" || exit 91
case "$1" in
  get)
    cat > /dev/null
    printf 'username=fixture\npassword=PRIVATE_HELPER_SECRET\n\n'
    ;;
  store|erase)
    cat > /dev/null
    ;;
  *) exit 92 ;;
esac
"#,
        );
        path
    }

    fn records(&self) -> Vec<Value> {
        fs::read_to_string(self.root.join("requests.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).expect("complete fixture observation"))
            .collect()
    }

    fn requests(&self) -> usize {
        self.records()
            .iter()
            .filter(|record| record["event"] == "request")
            .count()
    }

    fn observe(&self, fixture: &Fixture) -> RefAdvertisement {
        self.observe_with(fixture, None)
            .expect("independent verified HTTPS read-back")
    }

    fn observe_with(
        &self,
        fixture: &Fixture,
        credentials: Option<GitCredentialSelection>,
    ) -> Result<RefAdvertisement, gazette_core::release::RemoteReadError> {
        let selection = GitSelection {
            program: self.program.clone(),
            isolation: fixture.isolation.clone(),
            credentials,
        };
        RemoteReader {
            runner: &Runner,
            selection: &selection,
        }
        .observe_remote(
            &fixture.target.remote,
            "main",
            RemoteReadOptions {
                allow_read: true,
                allow_network: true,
                timeout: Duration::from_secs(10),
                ..Default::default()
            },
        )
    }

    fn assert_protocol(&self) {
        for record in self
            .records()
            .iter()
            .filter(|record| record["event"] == "request")
        {
            assert!(record["authorization"].is_null(), "no credential exchange");
            assert!(
                matches!(record["tls"].as_str(), Some("TLSv1.2" | "TLSv1.3")),
                "actual encrypted request"
            );
            assert!(
                !record["path"]
                    .as_str()
                    .expect("request path")
                    .contains("redirect-target")
            );
        }
    }
}

#[test]
fn explicit_helper_authenticates_reads_and_pushes_without_secret_or_ambient_fallback() {
    let fixture = Fixture::new();
    let server = Server::start(&fixture);
    let mut fixture = fixture;
    fixture.target.remote = server.endpoint.clone();
    server.mode("authenticated");
    let helper = server.credential_helper();
    let denied = GitCredentialSelection {
        helper: helper.clone(),
        allow_credentials: false,
    };
    let before = server.requests();
    let read_error = server
        .observe_with(&fixture, Some(denied.clone()))
        .expect_err("helper use requires its own permission");
    assert_eq!(read_error.failure, RemoteReadFailure::Permission);
    let push_error = fixture
        .dispatch_with_credentials(&server.program, Some(denied), network())
        .expect_err("dispatch cannot fall back to anonymous when helper use is denied");
    assert_eq!(push_error.failure, DispatchFailure::Permission);
    assert_eq!(push_error.effect, DispatchEffect::NotStarted);
    assert_eq!(server.requests(), before);

    let credentials = GitCredentialSelection {
        helper,
        allow_credentials: true,
    };
    let selection = GitSelection {
        program: server.program.clone(),
        isolation: fixture.isolation.clone(),
        credentials: Some(credentials.clone()),
    };
    let encoded = serde_json::to_string(&selection).expect("public selection shape");
    assert!(!encoded.contains("PRIVATE_HELPER_SECRET"));
    assert_eq!(
        server
            .observe_with(&fixture, Some(credentials.clone()))
            .expect("selected helper authenticates exact pre-state"),
        RefAdvertisement::Commit {
            oid: fixture.metadata.parent.clone().expect("parent")
        }
    );
    let dispatched = fixture
        .dispatch_with_credentials(&server.program, Some(credentials.clone()), network())
        .expect("selected helper authenticates exact non-force push");
    assert_eq!(dispatched.status, DispatchStatus::Updated);
    assert_eq!(
        server
            .observe_with(&fixture, Some(credentials))
            .expect("same selection authenticates independent read-back"),
        RefAdvertisement::Commit {
            oid: fixture.target.local_commit_oid.clone()
        }
    );
    let requests: Vec<_> = server
        .records()
        .into_iter()
        .filter(|record| record["event"] == "request")
        .collect();
    assert!(requests.iter().any(|record| {
        record["authorization"] == "Basic Zml4dHVyZTpQUklWQVRFX0hFTFBFUl9TRUNSRVQ="
    }));
    assert!(requests.iter().all(|record| {
        record["authorization"].is_null()
            || record["authorization"] == "Basic Zml4dHVyZTpQUklWQVRFX0hFTFBFUl9TRUNSRVQ="
    }));
    assert!(!format!("{push_error}").contains("PRIVATE_HELPER_SECRET"));
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

fn network() -> DispatchOptions {
    DispatchOptions {
        allow_network: true,
        ..options()
    }
}

fn other_refs(fixture: &Fixture) -> BTreeMap<String, Oid> {
    fixture
        .remote
        .references()
        .expect("native refs")
        .map(|reference| {
            let reference = reference.expect("native ref");
            (
                reference.name().expect("ref name").to_owned(),
                reference.target().expect("direct fixture ref"),
            )
        })
        .filter(|(name, _)| name != "refs/heads/main")
        .collect()
}

fn seed_other_ref(fixture: &Fixture) -> BTreeMap<String, Oid> {
    fixture
        .remote
        .reference(
            "refs/heads/unrelated",
            Oid::from_str(fixture.metadata.parent.as_ref().expect("parent").as_str())
                .expect("parent OID"),
            false,
            "unrelated fixture branch",
        )
        .expect("other native ref");
    other_refs(fixture)
}

#[test]
fn verified_https_denies_network_bad_trust_auth_and_redirect_then_updates_exact_ref() {
    let mut fixture = Fixture::new();
    let others = seed_other_ref(&fixture);
    let server = Server::start(&fixture);
    fixture.target.remote = server.endpoint.clone();
    let denied = fixture
        .dispatch(&server.program, options())
        .expect_err("network permission remains independent");
    assert_eq!(denied.failure, DispatchFailure::Permission);
    assert_eq!(denied.effect, DispatchEffect::NotStarted);
    assert_eq!(server.requests(), 0);
    let bad_trust = fixture
        .dispatch(native::git(), network())
        .expect_err("system trust does not admit fixture self-signed certificate");
    assert_eq!(bad_trust.effect, DispatchEffect::Unverified);
    assert_eq!(
        server.requests(),
        0,
        "TLS refusal occurs before HTTP request"
    );
    for mode in ["auth", "redirect"] {
        server.mode(mode);
        let before = server.requests();
        let refused = fixture
            .dispatch(&server.program, network())
            .expect_err("no credentials or redirect following");
        assert_eq!(refused.effect, DispatchEffect::Unverified);
        assert_eq!(
            server.requests() - before,
            1,
            "no prompted/authenticated/redirect request"
        );
        assert_eq!(
            fixture
                .remote
                .refname_to_id("refs/heads/main")
                .expect("unchanged ref")
                .to_string(),
            fixture.metadata.parent.as_ref().expect("parent").as_str()
        );
    }
    server.mode("normal");
    assert_eq!(
        server.observe(&fixture),
        RefAdvertisement::Commit {
            oid: fixture.metadata.parent.clone().expect("original prestate")
        }
    );
    let updated = fixture
        .dispatch(&server.program, network())
        .expect("real HTTPS receive-pack update");
    assert_eq!(updated.status, DispatchStatus::Updated);
    assert_eq!(updated.cleanup, Cleanup::Reaped);
    assert_eq!(
        server.observe(&fixture),
        RefAdvertisement::Commit {
            oid: fixture.target.local_commit_oid.clone()
        }
    );
    assert_eq!(other_refs(&fixture), others);
    server.assert_protocol();
}

#[test]
fn https_stale_state_preserves_concurrent_state_and_all_other_refs() {
    let mut fixture = Fixture::new();
    let others = seed_other_ref(&fixture);
    let server = Server::start(&fixture);
    fixture.target.remote = server.endpoint.clone();
    assert_eq!(
        server.observe(&fixture),
        RefAdvertisement::Commit {
            oid: fixture.metadata.parent.clone().expect("initial state")
        }
    );
    let concurrent = fixture.sibling();
    fixture
        .remote
        .reference(
            "refs/heads/main",
            Oid::from_str(concurrent.as_str()).expect("concurrent OID"),
            true,
            "concurrent owner",
        )
        .expect("real intervening update");
    assert_eq!(
        fixture
            .dispatch(&server.program, network())
            .expect("native stale-state outcome")
            .status,
        DispatchStatus::Rejected
    );
    assert_eq!(
        server.observe(&fixture),
        RefAdvertisement::Commit { oid: concurrent }
    );
    assert_eq!(other_refs(&fixture), others);
    server.assert_protocol();
}

#[test]
fn https_receiver_hook_rejection_is_observed_without_an_update() {
    let mut fixture = Fixture::new();
    let others = seed_other_ref(&fixture);
    let hook = fixture.remote.path().join("hooks/pre-receive");
    fs::create_dir_all(hook.parent().expect("fixture hook directory"))
        .expect("owned receiver hook parent");
    executable(&hook, "exit 1");
    let server = Server::start(&fixture);
    fixture.target.remote = server.endpoint.clone();
    assert_eq!(
        fixture
            .dispatch(&server.program, network())
            .expect("real receiver-owned hook rejection")
            .status,
        DispatchStatus::Rejected
    );
    assert_eq!(
        server.observe(&fixture),
        RefAdvertisement::Commit {
            oid: fixture.metadata.parent.clone().expect("unchanged parent")
        }
    );
    assert_eq!(other_refs(&fixture), others);
    server.assert_protocol();
}

#[test]
fn dropped_https_response_after_native_update_is_uncertain_until_independent_readback() {
    let mut fixture = Fixture::new();
    let others = seed_other_ref(&fixture);
    let server = Server::start(&fixture);
    fixture.target.remote = server.endpoint.clone();
    server.mode("drop");
    let failure = fixture
        .dispatch(&server.program, network())
        .expect_err("lost response is not success or rollback");
    assert_eq!(failure.effect, DispatchEffect::Unverified);
    assert_eq!(failure.cleanup, Cleanup::Reaped);
    assert_eq!(
        fixture
            .remote
            .refname_to_id("refs/heads/main")
            .expect("actual completed native effect")
            .to_string(),
        fixture.target.local_commit_oid.as_str()
    );
    let records = server.records();
    assert_eq!(
        records
            .iter()
            .filter(|record| record["event"] == "request"
                && record["method"] == "POST"
                && record["path"] == "/destination.git/git-receive-pack")
            .count(),
        1,
        "no automatic retry"
    );
    assert!(
        records.iter().any(|record| record["event"] == "backend"
            && record["path"] == "/destination.git/git-receive-pack"
            && record["exit"] == 0),
        "backend was reaped before dropping its real result"
    );
    server.mode("normal");
    assert_eq!(
        server.observe(&fixture),
        RefAdvertisement::Commit {
            oid: fixture.target.local_commit_oid.clone()
        }
    );
    assert_eq!(other_refs(&fixture), others);
    server.assert_protocol();
}
