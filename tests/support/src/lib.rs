//! Helps Gazette's integration tests find the native tools they run on any host.
//!
//! This crate builds on no other Gazette crate and is used only as a dev-dependency by
//! the test suites of the adapters, SDK and interfaces that run native tools. Gazette's
//! product contracts continue to require explicit absolute executable selections. Tests
//! may discover a host installation so a clean checkout is not coupled to one package
//! manager or Unix filesystem layout.

// This source is included by heterogeneous integration-test crates. Each crate
// intentionally compiles only the native-tool subset exercised by that fixture.
#![allow(dead_code)]

use std::{
    env, fs,
    io::Read,
    os::{
        fd::OwnedFd,
        unix::{fs::PermissionsExt, net::UnixStream},
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::OnceLock,
    thread,
    time::{Duration, Instant},
};

static GIT: OnceLock<PathBuf> = OnceLock::new();
static OPENSSL: OnceLock<PathBuf> = OnceLock::new();
static PYTHON: OnceLock<PathBuf> = OnceLock::new();
static CURL: OnceLock<PathBuf> = OnceLock::new();
static NODE: OnceLock<PathBuf> = OnceLock::new();
static GIT_HTTP_BACKEND: OnceLock<PathBuf> = OnceLock::new();

/// Exact Git executable selected for native integration fixtures.
pub fn git() -> &'static Path {
    GIT.get_or_init(|| resolve("GAZETTE_TEST_GIT", "git"))
}

/// Exact OpenSSL executable selected for the local TLS fixture.
pub fn openssl() -> &'static Path {
    OPENSSL.get_or_init(|| resolve("GAZETTE_TEST_OPENSSL", "openssl"))
}

/// Exact Python executable selected for the local HTTP-backend fixture.
pub fn python() -> &'static Path {
    PYTHON.get_or_init(resolve_python)
}

/// Exact curl executable selected for native loopback HTTP fixtures.
pub fn curl() -> &'static Path {
    CURL.get_or_init(|| resolve("GAZETTE_TEST_CURL", "curl"))
}

/// Exact Node executable meeting the default suite's documented Node 22.18+ baseline.
/// The selected override or first PATH tool is checked; no fallback, skip or installation occurs.
///
/// # Panics
/// Fails with the exact selected path and version when the runtime is unsupported or unobservable.
pub fn node() -> &'static Path {
    NODE.get_or_init(|| {
        let selected = resolve("GAZETTE_TEST_NODE", "node");
        qualify_node_runtime(&selected).unwrap_or_else(|reason| {
            panic!("GAZETTE_TEST_NODE selected {selected:?}: {reason}; the default suite requires Node 22.18.0 or newer; select an exact supported executable with GAZETTE_TEST_NODE")
        });
        selected
    })
}

fn qualify_node_runtime(program: &Path) -> Result<(), String> {
    let version = node_version(program)?;
    if supported_node_version(&version) {
        Ok(())
    } else {
        Err(format!("reported unsupported Node version {version:?}"))
    }
}

fn supported_node_version(version: &str) -> bool {
    let Some(version) = version.strip_prefix('v') else {
        return false;
    };
    let mut parts = version.split('.');
    let parsed = (parts.next(), parts.next(), parts.next(), parts.next());
    let (Some(major), Some(minor), Some(patch), None) = parsed else {
        return false;
    };
    let (Ok(major), Ok(minor), Ok(patch)) = (
        major.parse::<u64>(),
        minor.parse::<u64>(),
        patch.parse::<u64>(),
    ) else {
        return false;
    };
    (major, minor, patch) >= (22, 18, 0)
}

fn node_version(program: &Path) -> Result<String, String> {
    let (mut output, writer) = UnixStream::pair()
        .map_err(|error| format!("version probe output could not be bounded: {error}"))?;
    let writer: OwnedFd = writer.into();
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut child = Command::new(program)
        .arg("--version")
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::from(writer))
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("version probe could not start: {error}"))?;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            result => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(match result {
                    Err(error) => format!("version probe could not be observed: {error}"),
                    _ => "version probe exceeded its two-second deadline".into(),
                });
            }
        }
    };
    if !status.success() {
        return Err(format!("version probe failed with {status}"));
    }
    read_node_version(&mut output, deadline)
}

fn read_node_version(output: &mut UnixStream, deadline: Instant) -> Result<String, String> {
    // The child may already have closed its socket. Nonblocking reads avoid
    // setting peer-sensitive socket timeouts while retaining one overall deadline.
    output
        .set_nonblocking(true)
        .map_err(|error| format!("version probe output could not be bounded: {error}"))?;
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 129];
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or("version probe exceeded its two-second deadline")?;
        let maximum = buffer.len() - bytes.len();
        let count = match output.read(&mut buffer[..maximum]) {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(remaining.min(Duration::from_millis(10)));
                continue;
            }
            value => {
                value.map_err(|error| format!("version probe output could not be read: {error}"))?
            }
        };
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..count]);
        if bytes.len() > 128 {
            return Err("version probe exceeded its 128-byte output ceiling".into());
        }
    }
    String::from_utf8(bytes)
        .map(|version| version.trim().to_owned())
        .map_err(|_| "version probe returned non-UTF-8 output".into())
}

/// Git's own HTTP backend paired with the selected Git installation.
///
/// # Panics
///
/// Panics when the selected Git executable cannot report a regular executable
/// `git-http-backend` path suitable for the native integration fixture.
pub fn git_http_backend() -> &'static Path {
    GIT_HTTP_BACKEND.get_or_init(|| {
        if let Some(selected) = env::var_os("GAZETTE_TEST_GIT_HTTP_BACKEND") {
            return qualify(PathBuf::from(selected), "GAZETTE_TEST_GIT_HTTP_BACKEND");
        }
        let output = Command::new(git())
            .arg("--exec-path")
            .output()
            .expect("selected Git must report its native executable directory");
        assert!(
            output.status.success(),
            "selected Git did not report its native executable directory: {output:?}"
        );
        let directory = std::str::from_utf8(&output.stdout)
            .expect("Git executable directory must be UTF-8")
            .trim();
        assert!(!directory.is_empty(), "Git executable directory is empty");
        qualify(
            Path::new(directory).join("git-http-backend"),
            "selected Git HTTP backend",
        )
    })
}

/// Quote an exact selected native path for a fixture-owned POSIX shell script.
///
/// # Panics
///
/// Panics when the already-qualified fixture path is not valid UTF-8.
pub fn shell(path: &Path) -> String {
    let path = path.to_str().expect("selected native path must be UTF-8");
    format!("'{}'", path.replace('\'', "'\"'\"'"))
}

fn resolve(variable: &str, name: &str) -> PathBuf {
    if let Some(selected) = env::var_os(variable) {
        return qualify(PathBuf::from(selected), variable);
    }
    let path = env::var_os("PATH").expect("PATH is required to discover test-only native tools");
    let selected = env::split_paths(&path)
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file())
        .unwrap_or_else(|| {
            panic!(
                "test-only native tool {name:?} was not found; set {variable} to an absolute executable"
            )
        });
    qualify(selected, name)
}

fn resolve_python() -> PathBuf {
    const VARIABLE: &str = "GAZETTE_TEST_PYTHON";
    if let Some(selected) = env::var_os(VARIABLE) {
        let selected = qualify(PathBuf::from(selected), VARIABLE);
        assert!(
            python_fixture_capable(&selected),
            "{VARIABLE} must select a Python interpreter capable of a loopback HTTP fixture"
        );
        return selected;
    }

    let path = env::var_os("PATH").expect("PATH is required to discover test-only native tools");
    let mut inspected = Vec::new();
    for candidate in env::split_paths(&path).map(|directory| directory.join("python3")) {
        if !candidate.is_file() {
            continue;
        }
        let candidate = qualify(candidate, "python3");
        if inspected.contains(&candidate) {
            continue;
        }
        inspected.push(candidate.clone());
        if python_fixture_capable(&candidate) {
            return candidate;
        }
    }
    panic!(
        "no discovered Python interpreter can run the loopback HTTP fixture; set {VARIABLE} to an absolute compatible executable"
    );
}

fn python_fixture_capable(program: &Path) -> bool {
    let mut child = match Command::new(program)
        .env_clear()
        .args([
            "-I",
            "-B",
            "-c",
            "import http.server; server=http.server.HTTPServer(('127.0.0.1',0), http.server.BaseHTTPRequestHandler); server.server_close()",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return false,
    };
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

fn qualify(path: PathBuf, label: &str) -> PathBuf {
    assert!(path.is_absolute(), "{label} must select an absolute path");
    let path = fs::canonicalize(&path)
        .unwrap_or_else(|error| panic!("cannot resolve {label} at {path:?}: {error}"));
    let metadata = fs::metadata(&path)
        .unwrap_or_else(|error| panic!("cannot inspect {label} at {path:?}: {error}"));
    assert!(metadata.is_file(), "{label} must select a regular file");
    assert_ne!(
        metadata.permissions().mode() & 0o111,
        0,
        "{label} must select an executable file"
    );
    path
}

#[cfg(test)]
#[path = "../tests/node/mod.rs"]
mod node_tests;
