//! Shared GET contract and real pinned-curl loopback qualification; no external provider requests.

use gazette_adapter_http::{Client, Curl};
use gazette_core::{
    http::{Error, Options, Request},
    process,
    source::Sha256Digest,
};
use gazette_ports::{http::Reader, process::CaptureRunner};
use std::{
    cell::RefCell,
    fs,
    io::{Read, Write},
    net::TcpListener,
    thread,
    time::Duration,
};

use gazette_test_support as native;

const URL: &str = "https://api.github.com/repos/owner/publication/pages/builds/42";
const HEADERS: &[(&str, &str)] = &[
    ("Accept", "application/vnd.github+json"),
    ("X-GitHub-Api-Version", "2026-03-10"),
    ("Cache-Control", "no-cache"),
];

struct Runner {
    result: RefCell<Option<Result<process::Captured, process::Error>>>,
    commands: RefCell<Vec<process::Command>>,
}

impl Runner {
    fn bytes(stdout: Vec<u8>, code: Option<i32>) -> Self {
        Self {
            result: RefCell::new(Some(Ok(process::Captured {
                stdout,
                stderr: None,
                exit: process::Exit { code },
            }))),
            commands: RefCell::default(),
        }
    }
}

impl CaptureRunner for Runner {
    fn capture(
        &self,
        request: process::CaptureRequest<'_>,
    ) -> Result<process::Captured, process::Error> {
        request.validate().expect("bounded native request");
        assert!(request.request.input.is_empty());
        assert_eq!(request.request.options.max_input_bytes, 0);
        assert_eq!(
            request.request.options.timeout,
            Duration::from_millis(30000)
        );
        assert_eq!(request.max_output_bytes, options().response_bytes + 4);
        self.commands
            .borrow_mut()
            .push(request.request.command.clone());
        self.result
            .borrow_mut()
            .take()
            .expect("one capture, no retry")
    }
}

fn curl() -> Curl {
    Curl {
        program: "/selected/curl".into(),
        directory: "/selected/work".into(),
        executable: process::ExecutablePin {
            bytes: 1,
            sha256: Sha256Digest::of(b"x"),
        },
    }
}

fn options() -> Options {
    Options {
        allow_read: true,
        allow_execute: true,
        allow_network: true,
        ..Default::default()
    }
}

fn request(url: &str) -> Request<'_> {
    Request {
        url,
        headers: HEADERS,
        options: options(),
    }
}

#[test]
fn shared_client_preserves_exact_get_command_and_complete_body() {
    let runner = Runner::bytes(b"a\0b\n200".to_vec(), Some(0));
    let selected = curl();
    let response = Client {
        runner: &runner,
        curl: &selected,
    }
    .get(request(URL))
    .expect("complete HTTP response");
    assert_eq!(response.status, 200);
    assert_eq!(response.body, b"a\0b");
    let commands = runner.commands.borrow();
    assert_eq!(commands.len(), 1);
    let command = &commands[0];
    assert_eq!(command.program, selected.program);
    assert_eq!(command.directory, selected.directory);
    assert_eq!(command.executable, Some(selected.executable));
    assert!(command.environment.is_empty());
    assert_eq!(
        command.arguments,
        [
            "--disable",
            "--silent",
            "--globoff",
            "--proto",
            "=https",
            "--proxy",
            "",
            "--request",
            "GET",
            "--header",
            "Accept: application/vnd.github+json",
            "--header",
            "X-GitHub-Api-Version: 2026-03-10",
            "--header",
            "Cache-Control: no-cache",
            "--user-agent",
            "Gazette/0.1",
            "--write-out",
            "\n%{http_code}",
            "--url",
            URL,
        ]
    );
}

#[test]
fn request_scope_headers_permissions_and_native_selection_fail_before_capture() {
    let runner = Runner::bytes(b"unused\n200".to_vec(), Some(0));
    let selected = curl();
    let client = Client {
        runner: &runner,
        curl: &selected,
    };
    for url in [
        "http://example.com/a",
        "http://localhost/a",
        "http://127.1/a",
        "http://2130706433/a",
        "http://0x7f000001/a",
        "http://[::ffff:127.0.0.1]/a",
        "http://192.0.2.1/a",
        "https://user:secret@example.com/a",
        "https://@example.com/a",
        "https://example.com/a#fragment",
        "https://example.com/\r\nx",
        "https://example.com/a b",
        "https://example.com\\other/a",
        "file:///tmp/content",
        "ftp://example.com/a",
        "https:///example.com/a",
    ] {
        assert_eq!(
            client.get(request(url)).unwrap_err(),
            Error::Selection,
            "{url}"
        );
    }
    for url in ["http://127.0.0.1:8123/a", "http://[::1]:8123/a", URL] {
        request(url).validate().expect("explicit allowed URL");
    }
    for headers in [
        vec![("X-Test", "bad\r\nInjected: value")],
        vec![("X-Test\n", "value")],
        vec![("Authorization", "Bearer secret")],
        vec![("Cookie", "secret")],
        vec![("Host", "other.invalid")],
        vec![("Content-Length", "8")],
        vec![("Proxy-Authorization", "secret")],
        vec![("Transfer-Encoding", "chunked")],
        vec![("Accept", "a"), ("accept", "b")],
        vec![("", "value")],
    ] {
        assert_eq!(
            client
                .get(Request {
                    headers: &headers,
                    ..request(URL)
                })
                .unwrap_err(),
            Error::Selection
        );
    }
    for options in [
        Options::default(),
        Options {
            allow_read: false,
            ..options()
        },
        Options {
            allow_execute: false,
            ..options()
        },
        Options {
            allow_network: false,
            ..options()
        },
    ] {
        assert_eq!(
            client
                .get(Request {
                    options,
                    ..request(URL)
                })
                .unwrap_err(),
            Error::Permission
        );
    }
    for (response_bytes, timeout_ms) in [
        (0, 30000),
        ((16 << 20) + 1, 30000),
        (1024, 0),
        (1024, 300001),
    ] {
        assert_eq!(
            client
                .get(Request {
                    options: Options {
                        response_bytes,
                        timeout_ms,
                        ..options()
                    },
                    ..request(URL)
                })
                .unwrap_err(),
            Error::Limit
        );
    }
    let long_url = format!("https://example.com/{}", "a".repeat(8192));
    assert_eq!(client.get(request(&long_url)).unwrap_err(), Error::Limit);
    let long_header = "v".repeat(8192);
    assert_eq!(
        client
            .get(Request {
                headers: &[("X-Test", &long_header)],
                ..request(URL)
            })
            .unwrap_err(),
        Error::Limit
    );
    for selected in [
        Curl {
            program: "curl".into(),
            ..curl()
        },
        Curl {
            directory: "relative".into(),
            ..curl()
        },
        Curl {
            executable: process::ExecutablePin {
                bytes: 0,
                ..curl().executable
            },
            ..curl()
        },
    ] {
        assert_eq!(
            Client {
                runner: &runner,
                curl: &selected
            }
            .get(request(URL))
            .unwrap_err(),
            Error::Selection
        );
    }
    assert!(runner.commands.borrow().is_empty());
}

#[test]
fn partial_capture_exit_overflow_and_invalid_trailers_never_return_a_prefix() {
    let selected = curl();
    for (bytes, code) in [
        (b"{}\n200".to_vec(), Some(1)),
        (b"{}\n200".to_vec(), None),
        (b"".to_vec(), Some(0)),
        (b"{} 200".to_vec(), Some(0)),
        (b"{}\n20x".to_vec(), Some(0)),
        (b"{}\n000".to_vec(), Some(0)),
        (b"{}\n600".to_vec(), Some(0)),
        (b"{}\n200\n".to_vec(), Some(0)),
    ] {
        let runner = Runner::bytes(bytes, code);
        assert_eq!(
            Client {
                runner: &runner,
                curl: &selected
            }
            .get(request(URL))
            .unwrap_err(),
            Error::Response
        );
    }
    let runner = Runner::bytes(vec![b'x'; options().response_bytes + 5], Some(0));
    assert_eq!(
        Client {
            runner: &runner,
            curl: &selected
        }
        .get(request(URL))
        .unwrap_err(),
        Error::Limit
    );
    for (failure, cleanup) in [
        (process::Failure::Timeout, process::Cleanup::Reaped),
        (process::Failure::Output, process::Cleanup::Uncertain),
        (process::Failure::OutputLimit, process::Cleanup::Reaped),
    ] {
        let runner = Runner {
            result: RefCell::new(Some(Err(process::Error {
                failure,
                cleanup,
                source: None,
            }))),
            commands: RefCell::default(),
        };
        assert_eq!(
            Client {
                runner: &runner,
                curl: &selected
            }
            .get(request(URL))
            .unwrap_err(),
            Error::Transport { failure, cleanup }
        );
    }
}

fn native_curl(root: &std::path::Path) -> Curl {
    let program = native::curl().to_owned();
    let bytes = fs::read(&program).expect("native executable pin");
    Curl {
        program: program.to_str().expect("UTF-8 curl path").into(),
        executable: process::ExecutablePin {
            bytes: bytes.len() as u64,
            sha256: Sha256Digest::of(&bytes),
        },
        directory: root.to_str().expect("UTF-8 owned directory").into(),
    }
}

fn serve(response: Vec<u8>) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("owned loopback listener");
    let address = listener.local_addr().expect("held address");
    listener.set_nonblocking(true).expect("bounded accept");
    let handle = thread::spawn(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "no native request before deadline"
                    );
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("native accept: {error}"),
            }
        };
        // Accepted sockets may inherit the listener's nonblocking state on Unix.
        // The fixture owns a bounded blocking request phase below on every host.
        stream
            .set_nonblocking(false)
            .expect("blocking accepted fixture socket");
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .expect("read bound");
        stream
            .set_write_timeout(Some(Duration::from_secs(3)))
            .expect("write bound");
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            stream.read_exact(&mut byte).expect("complete HTTP headers");
            request.extend(byte);
            assert!(request.len() <= 8192, "request bound");
        }
        // Overflow tests may close their input pipe before the server finishes.
        let _ = stream.write_all(&response);
        String::from_utf8(request).expect("HTTP request text")
    });
    (format!("http://{address}/mount/page.html"), handle)
}

#[test]
fn native_pinned_curl_reads_exact_loopback_bytes_without_ambient_configuration() {
    let root = tempfile::tempdir().expect("owned native directory");
    fs::write(root.path().join(".curlrc"), "unsupported-option = true\n")
        .expect("ignored owned curl configuration");
    let selected = native_curl(root.path());
    let body = b"<p>retained</p>\0\n200";
    let mut response = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    response.extend(body);
    let (url, server) = serve(response);
    let response = Client {
        runner: &gazette_adapter_process::Runner,
        curl: &selected,
    }
    .get(request(&url))
    .expect("native complete response");
    assert_eq!(response.status, 200);
    assert_eq!(response.body, body);
    let native_request = server.join().expect("server completed");
    assert!(native_request.starts_with("GET /mount/page.html HTTP/1.1\r\n"));
    assert!(native_request.contains("X-GitHub-Api-Version: 2026-03-10\r\n"));
    assert!(
        !native_request
            .to_ascii_lowercase()
            .contains("authorization:")
    );
    assert!(!native_request.to_ascii_lowercase().contains("cookie:"));
}

#[test]
fn native_http_denial_and_redirect_remain_observations_without_following() {
    let root = tempfile::tempdir().expect("owned native directory");
    let selected = native_curl(root.path());
    let forbidden = TcpListener::bind("127.0.0.1:0").expect("redirect detection listener");
    forbidden
        .set_nonblocking(true)
        .expect("no blocking inspection");
    for status in [302, 404] {
        let (url, server) = serve(format!("HTTP/1.1 {status} Test\r\nLocation: http://{}/not-selected\r\nContent-Length: 6\r\nConnection: close\r\n\r\ndenied", forbidden.local_addr().unwrap()).into_bytes());
        let response = Client {
            runner: &gazette_adapter_process::Runner,
            curl: &selected,
        }
        .get(request(&url))
        .expect("complete non-success status");
        assert_eq!(response.status, status);
        assert_eq!(response.body, b"denied");
        server.join().expect("one original request");
        assert_eq!(
            forbidden.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }
}

#[test]
fn native_partial_oversized_and_wrong_pin_fail_without_successful_body() {
    let root = tempfile::tempdir().expect("owned native directory");
    let selected = native_curl(root.path());
    let (url, server) =
        serve(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\nshort".to_vec());
    assert_eq!(
        Client {
            runner: &gazette_adapter_process::Runner,
            curl: &selected
        }
        .get(request(&url))
        .unwrap_err(),
        Error::Response
    );
    server.join().expect("partial response fixture completed");
    let (url, server) = serve(
        b"HTTP/1.1 200 OK\r\nContent-Length: 20\r\nConnection: close\r\n\r\n01234567890123456789"
            .to_vec(),
    );
    assert_eq!(
        Client {
            runner: &gazette_adapter_process::Runner,
            curl: &selected
        }
        .get(Request {
            options: Options {
                response_bytes: 8,
                ..options()
            },
            ..request(&url)
        })
        .unwrap_err(),
        Error::Transport {
            failure: process::Failure::OutputLimit,
            cleanup: process::Cleanup::Reaped
        }
    );
    server.join().expect("oversized response fixture completed");
    let wrong = Curl {
        executable: process::ExecutablePin {
            sha256: Sha256Digest::of(b"wrong"),
            ..selected.executable
        },
        ..selected
    };
    assert_eq!(
        Client {
            runner: &gazette_adapter_process::Runner,
            curl: &wrong
        }
        .get(request("http://127.0.0.1:1/never-connect"))
        .unwrap_err(),
        Error::Transport {
            failure: process::Failure::Executable,
            cleanup: process::Cleanup::NotStarted
        }
    );
}
