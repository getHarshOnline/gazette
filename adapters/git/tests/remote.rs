//! Actual Git advertisements and bounded native malformed-provider fixtures.

use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use gazette_adapter_git::RemoteReader;
use gazette_adapter_process::Runner;
use gazette_core::{
    process::Cleanup,
    release::{GitSelection, RefAdvertisement, RemoteReadFailure, RemoteReadOptions},
};
use gazette_ports::release::RemoteReader as _;
use git2::{Repository, RepositoryInitOptions, Signature, Time};
use tempfile::{TempDir, tempdir};

use gazette_test_support as native;

struct Fixture {
    root: TempDir,
    isolation: PathBuf,
    repository: Repository,
}

impl Fixture {
    fn new() -> Self {
        let root = tempdir().expect("private fixture root");
        let isolation = root.path().join("isolated");
        fs::create_dir(&isolation).expect("explicit pre-existing isolation");
        fs::set_permissions(&isolation, fs::Permissions::from_mode(0o700))
            .expect("private isolation mode");
        let mut options = RepositoryInitOptions::new();
        options.bare(true).external_template(false);
        let repository = Repository::init_opts(root.path().join("destination.git"), &options)
            .expect("controlled bare destination");
        Self {
            root,
            isolation,
            repository,
        }
    }

    fn commit(&self, branch: &str) -> gazette_core::source::CommitId {
        let tree = self
            .repository
            .treebuilder(None)
            .expect("native empty tree")
            .write()
            .expect("write fixture tree");
        let tree = self.repository.find_tree(tree).expect("fixture tree");
        let signature = Signature::new("Fixture", "fixture@example.invalid", &Time::new(1, 0))
            .expect("fixed fixture signature");
        self.repository
            .commit(
                Some(&format!("refs/heads/{branch}")),
                &signature,
                &signature,
                "fixture",
                &tree,
                &[],
            )
            .expect("native controlled branch")
            .to_string()
            .parse()
            .expect("complete OID")
    }

    fn read(
        &self,
        program: &Path,
        remote: &str,
        branch: &str,
        options: RemoteReadOptions,
    ) -> Result<RefAdvertisement, gazette_core::release::RemoteReadError> {
        let selection = GitSelection {
            program: program.to_owned(),
            isolation: self.isolation.clone(),
            credentials: None,
        };
        RemoteReader {
            runner: &Runner,
            selection: &selection,
        }
        .observe_remote(remote, branch, options)
    }

    fn remote(&self) -> &str {
        self.repository.path().to_str().expect("fixture UTF-8 path")
    }

    fn executable(&self, body: &str) -> PathBuf {
        let path = self.root.path().join("provider");
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("disposable executable fixture");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
            .expect("private executable mode");
        path
    }
}

fn options() -> RemoteReadOptions {
    // Ordinary output/environment cases use the native finite default;
    // the explicit deadline case below independently selects 20 ms.
    RemoteReadOptions {
        allow_read: true,
        ..Default::default()
    }
}

#[test]
fn qualified_local_absence_is_distinct_from_hidden_advertisements() {
    let fixture = Fixture::new();
    let head = fs::read(fixture.repository.path().join("HEAD")).expect("original HEAD");
    assert_eq!(
        fixture
            .read(native::git(), fixture.remote(), "main", options())
            .expect("exact bare namespace absence"),
        RefAdvertisement::Absent
    );
    let oid = fixture.commit("main");
    let expected = RefAdvertisement::Commit { oid };
    assert_eq!(
        fixture
            .read(native::git(), fixture.remote(), "main", options())
            .expect("native branch"),
        expected
    );
    let url = url::Url::from_file_path(fixture.repository.path()).expect("explicit file URL");
    assert_eq!(
        fixture
            .read(native::git(), url.as_str(), "main", options())
            .expect("same URL branch"),
        expected
    );
    fixture
        .repository
        .config()
        .expect("fixture server config")
        .set_str("uploadpack.hideRefs", "refs/heads/main")
        .expect("server hides existing ref");
    assert_eq!(
        fixture
            .read(native::git(), fixture.remote(), "main", options())
            .expect("hidden advertisement"),
        RefAdvertisement::NotAdvertised
    );
    assert!(
        fixture.repository.find_reference("refs/heads/main").is_ok(),
        "hidden is not absent"
    );
    assert_eq!(
        fs::read(fixture.repository.path().join("HEAD")).expect("unchanged HEAD"),
        head
    );
    assert!(!fixture.repository.path().join("FETCH_HEAD").exists());
    assert!(
        fs::read_dir(&fixture.isolation)
            .expect("isolation directory")
            .next()
            .is_none()
    );
}

#[test]
fn invalid_selection_permissions_and_isolation_refuse_before_provider_execution() {
    let fixture = Fixture::new();
    let missing = Path::new("/missing-explicit-git-provider");
    assert_eq!(
        fixture
            .read(missing, fixture.remote(), "main", Default::default())
            .expect_err("read permission required")
            .failure,
        RemoteReadFailure::Permission
    );
    assert_eq!(
        fixture
            .read(
                missing,
                "https://example.invalid/site.git",
                "main",
                options()
            )
            .expect_err("network permission required")
            .failure,
        RemoteReadFailure::Permission
    );
    for remote in [
        "origin",
        "../destination.git",
        "ssh://example.invalid/repo",
        "http://example.invalid/repo",
        "ext::command",
        "file://remote/repo",
        "https:\\example.invalid/repo",
    ] {
        let error = fixture
            .read(missing, remote, "main", options())
            .expect_err("unsupported transport");
        assert_eq!(error.failure, RemoteReadFailure::Unsupported, "{remote}");
        assert_eq!(error.cleanup, None);
    }
    for remote in [
        "https://user:secret@example.invalid/repo",
        "https://example.invalid/repo?secret=1",
        "https://example.invalid/repo#fragment",
    ] {
        assert_eq!(
            fixture
                .read(missing, remote, "main", options())
                .expect_err("nonliteral private URL refused")
                .failure,
            RemoteReadFailure::Selection
        );
    }
    assert_eq!(
        fixture
            .read(missing, fixture.remote(), "main*", options())
            .expect_err("glob branch refused")
            .failure,
        RemoteReadFailure::Selection
    );
    fs::write(fixture.isolation.join(".netrc"), "private fixture")
        .expect("nonempty isolation fixture");
    assert_eq!(
        fixture
            .read(missing, fixture.remote(), "main", options())
            .expect_err("isolation must be empty")
            .failure,
        RemoteReadFailure::Isolation
    );
    let link = fixture.root.path().join("linked");
    symlink(&fixture.isolation, &link).expect("disposable directory link");
    let selection = GitSelection {
        program: missing.to_owned(),
        isolation: link,
        credentials: None,
    };
    let error = RemoteReader {
        runner: &Runner,
        selection: &selection,
    }
    .observe_remote(fixture.remote(), "main", options())
    .expect_err("linked isolation refused");
    assert_eq!(error.failure, RemoteReadFailure::Isolation);
}

#[test]
fn malformed_or_inconsistent_native_output_never_returns_an_advertised_prefix() {
    let fixture = Fixture::new();
    let oid = "a".repeat(40);
    for (body, expected) in [
        (
            format!("printf '%s\\trefs/heads/main\\n' '{oid}'; exit 0"),
            true,
        ),
        ("exit 2".to_owned(), true),
        ("exit 0".to_owned(), false),
        (format!("printf '%s\\trefs/heads/other\\n' '{oid}'"), false),
        (format!("printf '%s\\trefs/heads/main' '{oid}'"), false),
        (
            format!("printf '%s\\trefs/heads/main\\n%s\\trefs/heads/main\\n' '{oid}' '{oid}'"),
            false,
        ),
        (
            format!("printf '%s\\trefs/heads/main\\nextra\\n' '{oid}'"),
            false,
        ),
        (
            "printf '0000000000000000000000000000000000000000\\trefs/heads/main\\n'".to_owned(),
            false,
        ),
        ("printf '\\377\\trefs/heads/main\\n'".to_owned(), false),
        (
            format!("printf '%s\\trefs/heads/main\\n' '{}'", "a".repeat(64)),
            false,
        ),
        (
            format!("printf '%s\\trefs/heads/main\\n' '{}'", "A".repeat(40)),
            false,
        ),
        (
            format!("printf '%s\\trefs/heads/main\\n' '{oid}'; exit 2"),
            false,
        ),
        ("exit 128".to_owned(), false),
    ] {
        let program = fixture.executable(&body);
        let observed = fixture.read(&program, fixture.remote(), "main", options());
        if expected {
            assert!(observed.is_ok(), "{body}: {observed:?}");
        } else {
            let error = observed.expect_err("malformed or failed response");
            assert!(
                matches!(
                    error.failure,
                    RemoteReadFailure::Response | RemoteReadFailure::Transport
                ),
                "{error:?}"
            );
            assert_eq!(error.cleanup, Some(Cleanup::Reaped));
            assert!(!error.to_string().contains(&oid));
        }
    }
}

#[test]
fn native_timeout_output_ceiling_and_changed_isolation_preserve_custody() {
    let fixture = Fixture::new();
    let program = fixture.executable("while :; do :; done");
    let error = fixture
        .read(
            &program,
            fixture.remote(),
            "main",
            RemoteReadOptions {
                timeout: Duration::from_millis(20),
                ..options()
            },
        )
        .expect_err("deadline expires");
    assert_eq!(error.failure, RemoteReadFailure::Timeout);
    assert_eq!(error.cleanup, Some(Cleanup::Reaped));
    let program = fixture.executable("while :; do printf 'TOO_MUCH_OUTPUT'; done");
    let error = fixture
        .read(
            &program,
            fixture.remote(),
            "main",
            RemoteReadOptions {
                max_response_bytes: 8,
                ..options()
            },
        )
        .expect_err("complete response bound");
    assert_eq!(error.failure, RemoteReadFailure::Limit);
    assert_eq!(error.cleanup, Some(Cleanup::Reaped));
    let program = fixture.executable("printf 'residue' > \"$HOME/created\"; exit 2");
    let error = fixture
        .read(&program, fixture.remote(), "main", options())
        .expect_err("provider dirtied isolation");
    assert_eq!(error.failure, RemoteReadFailure::Isolation);
    assert_eq!(error.cleanup, Some(Cleanup::Reaped));
    assert_eq!(
        fs::read(fixture.isolation.join("created")).expect("residue not erased"),
        b"residue"
    );
}

#[test]
fn ambient_configuration_isolation_child() {
    let Some(path) = std::env::var_os("GAZETTE_REMOTE_CHILD_ROOT") else {
        return;
    };
    let root = PathBuf::from(path);
    let selection = GitSelection {
        program: native::git().to_owned(),
        isolation: root.join("isolated"),
        credentials: None,
    };
    let observed = RemoteReader {
        runner: &Runner,
        selection: &selection,
    }
    .observe_remote(
        root.join("destination.git").to_str().expect("native path"),
        "main",
        options(),
    )
    .expect("ambient config cannot redirect selected endpoint");
    assert!(matches!(observed, RefAdvertisement::Commit { .. }));
}

#[test]
fn hostile_parent_environment_cannot_redirect_the_real_native_git_reader() {
    let fixture = Fixture::new();
    fixture.commit("main");
    let poison = fixture.root.path().join("ambient");
    fs::create_dir(&poison).expect("private ambient fixture");
    fs::write(poison.join(".gitconfig"), "[include]\n path = /missing-private-config\n[url \"ext::false \" ]\n insteadOf = file://\n[credential]\n helper = !false\n").expect("poison ambient config");
    fs::write(
        poison.join(".netrc"),
        "machine example.invalid login PRIVATE password PRIVATE\n",
    )
    .expect("ambient netrc canary");
    let output = Command::new(std::env::current_exe().expect("current test executable"))
        .args([
            "--exact",
            "ambient_configuration_isolation_child",
            "--nocapture",
        ])
        .env("GAZETTE_REMOTE_CHILD_ROOT", fixture.root.path())
        .env("HOME", &poison)
        .env("XDG_CONFIG_HOME", &poison)
        .env("GIT_CONFIG_GLOBAL", poison.join(".gitconfig"))
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "url.ext::false .insteadOf")
        .env("GIT_CONFIG_VALUE_0", "file://")
        .env("GIT_ASKPASS", "/missing-private-askpass")
        .env("HTTPS_PROXY", "http://127.0.0.1:1")
        .stdin(Stdio::null())
        .output()
        .expect("isolated nested native test");
    assert!(output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("1 passed"),
        "nested fixture actually ran: {output:?}"
    );
    assert!(
        fs::read_dir(&fixture.isolation)
            .expect("isolation unchanged")
            .next()
            .is_none()
    );
}

#[test]
fn child_environment_is_explicit_and_protocol_specific() {
    let fixture = Fixture::new();
    let program = fixture.executable(
        r#"
test "$HOME" = "$PWD" || exit 9
test "$XDG_CONFIG_HOME" = "$HOME" || exit 9
test "$GIT_CONFIG_GLOBAL" = /dev/null || exit 9
test "$GIT_CONFIG_SYSTEM" = /dev/null || exit 9
test "$GIT_CONFIG_NOSYSTEM" = 1 || exit 9
test "$GIT_TERMINAL_PROMPT" = 0 || exit 9
test "$GIT_NO_LAZY_FETCH" = 1 || exit 9
test "$GIT_ALLOW_PROTOCOL" = file || exit 9
test -z "$GIT_CONFIG_COUNT$GIT_ASKPASS$SSH_ASKPASS$HTTPS_PROXY$GIT_DIR" || exit 9
exit 2
"#,
    );
    assert_eq!(
        fixture
            .read(&program, fixture.remote(), "main", options())
            .expect("fixed child environment and qualified local namespace"),
        RefAdvertisement::Absent
    );
}

#[test]
fn unqualified_namespace_silence_is_never_promoted_to_absence() {
    let fixture = Fixture::new();
    let program = fixture.executable("exit 2");
    // A network provider's empty advertisement supplies no namespace evidence.
    assert_eq!(
        fixture
            .read(
                &program,
                "https://example.invalid/repo",
                "main",
                RemoteReadOptions {
                    allow_network: true,
                    ..options()
                }
            )
            .expect("network silence"),
        RefAdvertisement::NotAdvertised
    );
    // A tiny response allowance cannot certify an unbounded native namespace.
    assert_eq!(
        fixture
            .read(
                &program,
                fixture.remote(),
                "main",
                RemoteReadOptions {
                    max_response_bytes: 0,
                    ..options()
                }
            )
            .expect("bounded silence"),
        RefAdvertisement::NotAdvertised
    );
    fixture
        .repository
        .reference_symbolic(
            "refs/heads/main",
            "refs/heads/missing",
            false,
            "dangling symbolic fixture",
        )
        .expect("native symbolic ref");
    assert_eq!(
        fixture
            .read(&program, fixture.remote(), "main", options())
            .expect("dangling ref"),
        RefAdvertisement::NotAdvertised,
        "unresolved is not absent"
    );
    fixture
        .repository
        .find_reference("refs/heads/main")
        .expect("fixture ref")
        .delete()
        .expect("remove only fixture symbolic reference");
    let link = fixture.root.path().join("linked.git");
    symlink(fixture.repository.path(), &link).expect("fixture link");
    assert_eq!(
        fixture
            .read(
                &program,
                link.to_str().expect("fixture path"),
                "main",
                options()
            )
            .expect("unqualified linked namespace"),
        RefAdvertisement::NotAdvertised
    );
    fs::write(
        fixture.repository.path().join("packed-refs"),
        "broken packed refs\n",
    )
    .expect("malformed native fixture metadata");
    assert_eq!(
        fixture
            .read(&program, fixture.remote(), "main", options())
            .expect("unreadable namespace remains indeterminate"),
        RefAdvertisement::NotAdvertised
    );
}

#[test]
fn advertisement_does_not_hydrate_missing_promisor_objects() {
    let upstream = Fixture::new();
    let oid = upstream.commit("main");
    let fixture = Fixture::new();
    let native_oid = git2::Oid::from_str(oid.as_str()).expect("complete fixture OID");
    // Deliberately incomplete provider state: libgit2's reference constructor
    // correctly refuses a missing object, so write this loose fixture ref directly.
    fs::write(
        fixture.repository.path().join("refs/heads/main"),
        format!("{oid}\n"),
    )
    .expect("controlled missing-object ref");
    let mut config = fixture
        .repository
        .config()
        .expect("local promisor fixture configuration");
    config
        .set_i64("core.repositoryformatversion", 1)
        .expect("extension format");
    config
        .set_str("extensions.partialClone", "partial")
        .expect("selected promisor");
    config
        .set_bool("remote.partial.promisor", true)
        .expect("promisor source");
    config
        .set_str(
            "remote.partial.url",
            url::Url::from_file_path(upstream.repository.path())
                .expect("local promisor URL")
                .as_str(),
        )
        .expect("controlled offline hydration source");
    drop(config);
    let original = fs::read(fixture.repository.path().join("config"))
        .expect("original provider configuration");
    assert!(
        !fixture
            .repository
            .odb()
            .expect("empty object store")
            .exists(native_oid)
    );
    assert_eq!(
        fixture
            .read(native::git(), fixture.remote(), "main", options())
            .expect("advertisement is not object-existence certification"),
        RefAdvertisement::Commit { oid }
    );
    assert!(
        !fixture
            .repository
            .odb()
            .expect("unchanged object store")
            .exists(native_oid),
        "ls-remote must not hydrate even from a reachable local promisor"
    );
    assert_eq!(
        fs::read(fixture.repository.path().join("config")).expect("unchanged configuration"),
        original
    );
    assert!(!fixture.repository.path().join("FETCH_HEAD").exists());
}
