//! Native loopback transport over exact retained bytes; no browser or deployment claims.

use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Write},
    net::{Shutdown, SocketAddr, TcpListener, TcpStream},
    time::{Duration, Instant},
};

use gazette_adapter_http::{Http, Session};
use gazette_core::{
    preview::{Binding, Options, Poll, Site},
    publication::{Key, routing::PublicBaseUrl},
    render::MediaType,
    source::{CapturedSource, Content, EntryPath, InventoryBuilder, Limits, Sha256Digest},
};
use gazette_ports::preview::{PreviewReservation, PreviewServer, PreviewSession};

static UNTYPED: BTreeMap<EntryPath, MediaType> = BTreeMap::new();

#[path = "preview/compression.rs"]
mod compression;

fn capture(files: &[(&str, &[u8])]) -> CapturedSource {
    capture_with_limits(files, Limits::default())
}

fn capture_with_limits(files: &[(&str, &[u8])], limits: Limits) -> CapturedSource {
    let mut directories = BTreeSet::new();
    let mut payloads = BTreeMap::new();
    for (name, bytes) in files {
        let mut ancestor = *name;
        while let Some((parent, _)) = ancestor.rsplit_once('/') {
            directories.insert(parent);
            ancestor = parent;
        }
        payloads.insert(
            name.parse::<EntryPath>().expect("fixture entry"),
            bytes.to_vec(),
        );
    }
    let mut builder = InventoryBuilder::new(limits);
    for directory in directories {
        builder
            .insert(
                directory.parse().expect("fixture directory"),
                Content::Directory,
            )
            .expect("directory admission");
    }
    for (path, bytes) in &payloads {
        builder
            .insert(
                path.clone(),
                Content::File {
                    bytes: bytes.len() as u64,
                    sha256: Sha256Digest::of(bytes),
                },
            )
            .expect("file admission");
    }
    CapturedSource::from_parts(builder.finish(), payloads).expect("retained fixture")
}

fn key(value: &str) -> Key {
    value.parse().expect("site key")
}
fn options() -> Options {
    Options {
        allow_listeners: true,
        ..Default::default()
    }
}
fn ephemeral() -> SocketAddr {
    "127.0.0.1:0".parse().expect("loopback")
}

fn bind<'a>(output: &'a CapturedSource, base: &'a PublicBaseUrl, options: Options) -> Session<'a> {
    Http.bind(
        BTreeMap::from([(
            key("site:one"),
            Site {
                output,
                base_url: base,
                media_types: &UNTYPED,
                search_page: None,
            },
        )]),
        BTreeMap::from([(key("site:one"), ephemeral())]),
        options,
    )
    .expect("complete listener set")
}

fn address(session: &Session<'_>) -> SocketAddr {
    *session.addresses().values().next().expect("one listener")
}

fn progress(session: &mut Session<'_>) -> (Poll, Duration) {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let started = Instant::now();
        let outcome = session.poll().expect("listener remains usable");
        if outcome != Poll::Idle {
            return (outcome, started.elapsed());
        }
        assert!(
            Instant::now() < deadline,
            "connected client never became an accepted request"
        );
        // Connect completion does not guarantee immediate nonblocking accept readiness.
        // Poll real progress instead of sleeping or treating Idle as a successful response.
        std::thread::yield_now();
    }
}

fn exchange(session: &mut Session<'_>, request: &[u8]) -> Vec<u8> {
    let mut client = TcpStream::connect(address(session)).expect("connect native listener");
    client
        .set_read_timeout(Some(Duration::from_secs(3)))
        .expect("bounded client read");
    client.write_all(request).expect("send request");
    client
        .shutdown(Shutdown::Write)
        .expect("complete client input");
    assert_eq!(progress(session).0, Poll::Responded);
    let mut response = Vec::new();
    client
        .read_to_end(&mut response)
        .expect("complete close-delimited response");
    response
}

fn get(session: &mut Session<'_>, method: &str, path: &str) -> Vec<u8> {
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {}\r\n\r\n",
        address(session)
    );
    exchange(session, request.as_bytes())
}

fn split(response: &[u8]) -> (&str, &[u8]) {
    let split = response
        .windows(4)
        .position(|bytes| bytes == b"\r\n\r\n")
        .expect("header terminator");
    (
        std::str::from_utf8(&response[..split]).expect("ASCII response header"),
        &response[split + 4..],
    )
}

#[test]
fn exact_files_mime_head_and_no_store_are_native_responses() {
    let output = capture(&[
        ("index.html", b"<a href=\"https://public.test/\">exact</a>"),
        ("café.bin", &[0, 255, 10]),
        ("app.mjs", b"export const x = 1;"),
        (
            "captions.vtt",
            b"WEBVTT\n\n00:00.000 --> 00:01.000\nA caption.\n",
        ),
    ]);
    let original = output.inventory().identity();
    let base = "https://public.test/".parse().expect("base");
    let mut session = bind(&output, &base, options());
    assert_eq!(session.poll().expect("idle listener"), Poll::Idle);
    let response = get(&mut session, "GET", "/");
    let (headers, body) = split(&response);
    assert!(headers.starts_with("HTTP/1.1 200 "));
    assert!(headers.contains("Content-Type: text/html; charset=utf-8"));
    assert!(headers.contains("Cache-Control: no-store"));
    assert!(headers.contains("X-Content-Type-Options: nosniff"));
    assert!(!headers.to_ascii_lowercase().contains("access-control"));
    assert_eq!(body, b"<a href=\"https://public.test/\">exact</a>");
    let response = get(&mut session, "GET", "/caf%C3%A9.bin");
    assert_eq!(split(&response).1, [0, 255, 10]);
    assert!(split(&response).0.contains("application/octet-stream"));
    let response = get(&mut session, "HEAD", "/caf%C3%A9.bin");
    assert!(split(&response).0.contains("Content-Length: 3"));
    assert!(split(&response).1.is_empty());
    assert!(
        split(&get(&mut session, "GET", "/app.mjs"))
            .0
            .contains("text/javascript")
    );
    let response = get(&mut session, "GET", "/captions.vtt");
    assert!(
        split(&response)
            .0
            .contains("Content-Type: text/vtt; charset=utf-8")
    );
    assert_eq!(
        split(&response).1,
        b"WEBVTT\n\n00:00.000 --> 00:01.000\nA caption.\n"
    );
    assert_eq!(output.inventory().identity(), original);
}

#[test]
fn producer_media_types_override_extensionless_and_misleading_names() {
    let cases = [
        ("about", MediaType::Html, "text/html; charset=utf-8"),
        ("page.bin", MediaType::Html, "text/html; charset=utf-8"),
        (
            "code",
            MediaType::JavaScript,
            "text/javascript; charset=utf-8",
        ),
        ("data", MediaType::Json, "application/json"),
        ("linked.html", MediaType::JsonLd, "application/ld+json"),
        ("sitemap", MediaType::Xml, "application/xml"),
        ("news", MediaType::Rss, "application/rss+xml"),
        ("updates.json", MediaType::Atom, "application/atom+xml"),
        ("llms", MediaType::Text, "text/plain; charset=utf-8"),
        (
            "alternate",
            MediaType::Markdown,
            "text/markdown; charset=utf-8",
        ),
        (
            "alternate.html",
            MediaType::Markdown,
            "text/markdown; charset=utf-8",
        ),
        ("plain.md", MediaType::Text, "text/plain; charset=utf-8"),
        ("style", MediaType::Css, "text/css; charset=utf-8"),
        ("raster.html", MediaType::Png, "image/png"),
        ("photo", MediaType::Jpeg, "image/jpeg"),
        ("lossless", MediaType::Webp, "image/webp"),
        ("mark", MediaType::Svg, "image/svg+xml"),
        ("font.bin", MediaType::Woff2, "font/woff2"),
    ];
    // Identical opaque bytes deliberately cannot reveal which media type applies.
    let output = capture(
        &cases
            .iter()
            .map(|(path, _, _)| (*path, b"opaque".as_slice()))
            .collect::<Vec<_>>(),
    );
    let media_types = cases
        .iter()
        .map(|(path, kind, _)| (path.parse().expect("output path"), *kind))
        .collect();
    let base = "https://public.test/".parse().expect("base");
    let mut session = Http
        .bind(
            BTreeMap::from([(
                key("site:one"),
                Site {
                    output: &output,
                    base_url: &base,
                    media_types: &media_types,
                    search_page: None,
                },
            )]),
            BTreeMap::from([(key("site:one"), ephemeral())]),
            options(),
        )
        .expect("typed retained output listener");
    for (path, _, expected) in cases {
        let response = get(&mut session, "GET", &format!("/{path}"));
        let (headers, body) = split(&response);
        assert!(headers.starts_with("HTTP/1.1 200 "), "{path}");
        assert!(
            headers.contains(&format!("Content-Type: {expected}\r\n")),
            "{path}: {headers}"
        );
        assert_eq!(body, b"opaque");
    }
}

#[test]
fn configured_mount_and_directory_indices_do_not_flatten_or_fallback() {
    let output = capture(&[("index.html", b"root"), ("guide/index.html", b"guide")]);
    let base = "https://public.test/prefix/".parse().expect("base");
    let mut session = bind(&output, &base, options());
    assert_eq!(
        session.urls()[&key("site:one")],
        format!("http://{}/prefix/", address(&session))
    );
    assert_eq!(split(&get(&mut session, "GET", "/prefix/")).1, b"root");
    assert_eq!(
        split(&get(&mut session, "GET", "/prefix/guide/?a=b")).1,
        b"guide"
    );
    for (path, location) in [
        ("/prefix?x=1", "/prefix/?x=1"),
        ("/prefix/guide?q=x", "/prefix/guide/?q=x"),
    ] {
        let response = get(&mut session, "GET", path);
        assert!(split(&response).0.starts_with("HTTP/1.1 308 "));
        assert!(
            split(&response)
                .0
                .contains(&format!("Location: {location}"))
        );
    }
    for path in [
        "/",
        "/guide/",
        "/prefix-other/",
        "/prefix/missing",
        "/prefix/guide/missing",
    ] {
        assert!(
            split(&get(&mut session, "GET", path))
                .0
                .starts_with("HTTP/1.1 404 "),
            "{path}"
        );
    }
}

#[test]
fn malformed_routes_use_the_shared_strict_codec() {
    let output = capture(&[("index.html", b"safe")]);
    let base = "https://public.test/".parse().expect("base");
    let mut session = bind(&output, &base, options());
    for path in [
        "//host/",
        "/../",
        "/%2e%2e/",
        "/a%2Fb",
        "/a%5cb",
        "/%252e",
        "/%",
        "/%GG",
        "/%FF",
        "/a#fragment",
        "/a\\b",
        "/%00",
        "/?x=%GG",
        "https://public.test/",
    ] {
        assert!(
            split(&get(&mut session, "GET", path))
                .0
                .starts_with("HTTP/1.1 400 "),
            "{path}"
        );
    }
}

#[test]
fn authority_methods_versions_and_body_protocols_are_explicitly_refused() {
    let output = capture(&[("index.html", b"safe")]);
    let base = "https://public.test/".parse().expect("base");
    let mut session = bind(&output, &base, options());
    let host = address(&session);
    for headers in [
        String::new(),
        "Host: localhost\r\n".into(),
        format!("Host: {host}\r\nHost: {host}\r\n"),
        format!("Host: {host}\r\nContent-Length: 0\r\n"),
        format!("Host: {host}\r\nTransfer-Encoding: chunked\r\n"),
        format!("Host: {host}\r\nUpgrade: websocket\r\n"),
        format!("Host: {host}\r\nRange: bytes=0-1\r\n"),
    ] {
        let response = exchange(
            &mut session,
            format!("GET / HTTP/1.1\r\n{headers}\r\n").as_bytes(),
        );
        assert!(split(&response).0.starts_with("HTTP/1.1 400 "));
    }
    assert!(
        split(&get(&mut session, "POST", "/"))
            .0
            .starts_with("HTTP/1.1 405 ")
    );
    let response = exchange(
        &mut session,
        format!("GET / HTTP/1.0\r\nHost: {host}\r\n\r\n").as_bytes(),
    );
    assert!(split(&response).0.starts_with("HTTP/1.1 505 "));
    let response = exchange(
        &mut session,
        format!("GET / HTTP/1.1\r\nHost: {host}\r\n\r\nGET / HTTP/1.1\r\n\r\n").as_bytes(),
    );
    assert!(split(&response).0.starts_with("HTTP/1.1 400 "));
}

#[test]
fn complete_set_bind_failure_releases_prior_listeners_and_validates_permission() {
    let output = capture(&[("index.html", b"safe")]);
    let base = "https://public.test/".parse().expect("base");
    let sites = || {
        BTreeMap::from([
            (
                key("site:a"),
                Site {
                    output: &output,
                    base_url: &base,
                    media_types: &UNTYPED,
                    search_page: None,
                },
            ),
            (
                key("site:b"),
                Site {
                    output: &output,
                    base_url: &base,
                    media_types: &UNTYPED,
                    search_page: None,
                },
            ),
        ])
    };
    let first = TcpListener::bind(ephemeral()).expect("reserve disposable first port");
    let first_address = first.local_addr().expect("first address");
    drop(first);
    let busy = TcpListener::bind(ephemeral()).expect("busy second port");
    let busy_address = busy.local_addr().expect("busy address");
    assert!(
        Http.bind(
            sites(),
            BTreeMap::from([
                (key("site:a"), first_address),
                (key("site:b"), busy_address)
            ]),
            options()
        )
        .is_err()
    );
    let released = TcpListener::bind(first_address)
        .expect("failed complete-set startup released first listener");
    drop(released);
    for (addresses, settings) in [
        (
            BTreeMap::from([(key("site:a"), ephemeral()), (key("site:b"), ephemeral())]),
            Options::default(),
        ),
        (BTreeMap::from([(key("site:a"), ephemeral())]), options()),
        (
            BTreeMap::from([
                (key("site:a"), first_address),
                (key("site:b"), first_address),
            ]),
            options(),
        ),
        (
            BTreeMap::from([
                (
                    key("site:a"),
                    "0.0.0.0:0".parse().expect("unspecified address"),
                ),
                (key("site:b"), ephemeral()),
            ]),
            options(),
        ),
        (
            BTreeMap::from([(key("site:a"), ephemeral()), (key("site:b"), ephemeral())]),
            Options {
                max_sites: 1,
                ..options()
            },
        ),
    ] {
        assert!(Http.bind(sites(), addresses, settings).is_err());
    }
}

#[test]
fn two_ephemeral_sites_remain_distinct_round_robin_and_close_releases_all() {
    let a = capture(&[("index.html", b"a")]);
    let b = capture(&[("index.html", b"b")]);
    let base = "https://public.test/".parse().expect("base");
    let mut session = Http
        .bind(
            BTreeMap::from([
                (
                    key("site:a"),
                    Site {
                        output: &a,
                        base_url: &base,
                        media_types: &UNTYPED,
                        search_page: None,
                    },
                ),
                (
                    key("site:b"),
                    Site {
                        output: &b,
                        base_url: &base,
                        media_types: &UNTYPED,
                        search_page: None,
                    },
                ),
            ]),
            BTreeMap::from([(key("site:a"), ephemeral()), (key("site:b"), ephemeral())]),
            options(),
        )
        .expect("both listeners");
    let addresses: Vec<_> = session.addresses().values().copied().collect();
    assert_ne!(addresses[0], addresses[1]);
    let mut clients = Vec::new();
    for address in &addresses {
        let mut client = TcpStream::connect(address).expect("site connection");
        client
            .set_read_timeout(Some(Duration::from_secs(3)))
            .expect("bounded read");
        write!(client, "GET / HTTP/1.1\r\nHost: {address}\r\n\r\n").expect("request");
        client.shutdown(Shutdown::Write).expect("complete request");
        clients.push(client);
    }
    assert_eq!(progress(&mut session).0, Poll::Responded);
    assert_eq!(progress(&mut session).0, Poll::Responded);
    for (mut client, expected) in clients.into_iter().zip([b"a", b"b"]) {
        let mut bytes = Vec::new();
        client.read_to_end(&mut bytes).expect("native response");
        assert_eq!(split(&bytes).1, expected);
    }
    session.close();
    for address in addresses {
        TcpListener::bind(address).expect("close released exact listener");
    }
}

#[test]
fn header_ceilings_and_client_eof_do_not_claim_successful_content() {
    let output = capture(&[("index.html", b"safe")]);
    let base = "https://public.test/".parse().expect("base");
    let mut session = bind(
        &output,
        &base,
        Options {
            max_headers: 1,
            ..options()
        },
    );
    let host = address(&session);
    let response = exchange(
        &mut session,
        format!("GET / HTTP/1.1\r\nHost: {host}\r\nOther: x\r\n\r\n").as_bytes(),
    );
    assert!(split(&response).0.starts_with("HTTP/1.1 431 "));
    let mut bounded = bind(
        &output,
        &base,
        Options {
            max_header_bytes: 16,
            ..options()
        },
    );
    let response = exchange(&mut bounded, b"GET / HTTP/1.1\r\n");
    assert!(split(&response).0.starts_with("HTTP/1.1 431 "));
    let client = TcpStream::connect(address(&session)).expect("empty client");
    client
        .shutdown(Shutdown::Both)
        .expect("client ends before request");
    assert_eq!(progress(&mut session).0, Poll::Disconnected);
    assert_eq!(split(&get(&mut session, "GET", "/")).1, b"safe");
}

#[test]
fn fractional_progress_does_not_renew_the_absolute_request_deadline() {
    let output = capture(&[("index.html", b"safe")]);
    let base = "https://public.test/".parse().expect("base");
    let mut session = bind(
        &output,
        &base,
        Options {
            request_timeout: Duration::from_millis(200),
            ..options()
        },
    );
    let mut client = TcpStream::connect(address(&session)).expect("slow client");
    client.write_all(b"G").expect("initial byte queued");
    std::thread::scope(|scope| {
        scope.spawn(move || {
            for byte in b"ET / HTTP/1.1\r\n" {
                std::thread::sleep(Duration::from_millis(40));
                if client.write_all(&[*byte]).is_err() {
                    break;
                }
            }
        });
        let (outcome, elapsed) = progress(&mut session);
        assert_eq!(outcome, Poll::Disconnected);
        assert!(
            elapsed >= Duration::from_millis(150),
            "the accepted request must actually wait for its deadline, not fail before progress"
        );
        assert!(
            elapsed < Duration::from_millis(550),
            "ongoing progress must not renew a 200ms request deadline"
        );
    });
}

#[test]
fn nonreading_client_exhausts_the_response_deadline_without_poisoning_listeners() {
    const BODY_BYTES: usize = 32 << 20;
    let large = vec![b'x'; BODY_BYTES];
    let output = capture_with_limits(
        &[("large.bin", &large), ("index.html", b"still usable")],
        Limits {
            entries: 2,
            depth: 1,
            file_bytes: BODY_BYTES as u64,
            total_bytes: BODY_BYTES as u64 + 12,
        },
    );
    drop(large);
    let base = "https://public.test/".parse().expect("base");
    let mut session = bind(
        &output,
        &base,
        Options {
            response_timeout: Duration::from_millis(250),
            ..options()
        },
    );
    let address = address(&session);
    let mut client = TcpStream::connect(address).expect("nonreading client");
    client
        .set_read_timeout(Some(Duration::from_secs(3)))
        .expect("bounded eventual drain");
    write!(client, "GET /large.bin HTTP/1.1\r\nHost: {address}\r\n\r\n").expect("complete request");
    client
        .shutdown(Shutdown::Write)
        .expect("no more request bytes");
    // Keep the read half open but do not consume any bytes until the write attempt ends.
    let (outcome, elapsed) = progress(&mut session);
    assert_eq!(
        outcome,
        Poll::Disconnected,
        "a partial response is not successful delivery"
    );
    assert!(
        elapsed >= Duration::from_millis(200),
        "not an immediate refusal or disconnected peer"
    );
    assert!(
        elapsed < Duration::from_millis(450),
        "partial socket writes must not renew the 250ms total response deadline"
    );
    let mut response = Vec::new();
    (&mut client)
        .take(BODY_BYTES as u64 + 1024)
        .read_to_end(&mut response)
        .expect("drain bytes written before timeout");
    let (headers, body) = split(&response);
    assert!(headers.starts_with("HTTP/1.1 200 "));
    assert!(headers.contains(&format!("Content-Length: {BODY_BYTES}\r\n")));
    assert!(
        !body.is_empty(),
        "the response body must actually have begun"
    );
    assert!(
        body.len() < BODY_BYTES,
        "fixture must establish native backpressure, not fit wholly in socket buffers"
    );
    assert!(body.iter().all(|byte| *byte == b'x'));
    drop(client);
    assert_eq!(split(&get(&mut session, "GET", "/")).1, b"still usable");
}

#[test]
fn reservation_preserves_native_sockets_and_typed_urls_through_attachment() {
    let base: PublicBaseUrl = "https://public.test/prefix/".parse().expect("public mount");
    let reservation = Http
        .reserve(
            BTreeMap::from([(
                key("site:one"),
                Binding {
                    address: ephemeral(),
                    base_url: &base,
                },
            )]),
            options(),
        )
        .expect("reserve before rendering");
    let address = reservation.addresses()[&key("site:one")];
    let local = reservation.urls()[&key("site:one")].clone();
    assert_eq!(local.as_str(), format!("http://{address}/prefix/"));
    assert!(
        TcpListener::bind(address).is_err(),
        "reservation continuously holds the chosen port"
    );
    let mut queued = TcpStream::connect(address).expect("queue request on the reserved socket");
    queued
        .set_read_timeout(Some(Duration::from_secs(3)))
        .expect("bounded read");
    write!(queued, "GET /prefix/ HTTP/1.1\r\nHost: {address}\r\n\r\n")
        .expect("queue complete request");
    queued
        .shutdown(Shutdown::Write)
        .expect("complete queued input");
    // Output is created only after ports exist and remains separately owned.
    let output = capture(&[("index.html", local.as_str().as_bytes())]);
    let mut session = reservation
        .attach(BTreeMap::from([(
            key("site:one"),
            Site {
                output: &output,
                media_types: &UNTYPED,
                base_url: &local,
                search_page: None,
            },
        )]))
        .expect("attach local-mode output at the same mount");
    assert_eq!(session.addresses()[&key("site:one")], address);
    assert_eq!(session.urls()[&key("site:one")], local.as_str());
    assert!(
        TcpListener::bind(address).is_err(),
        "attachment must not release the port"
    );
    assert_eq!(progress(&mut session).0, Poll::Responded);
    let mut response = Vec::new();
    queued
        .read_to_end(&mut response)
        .expect("pre-attachment connection survives into serving");
    assert_eq!(split(&response).1, local.as_str().as_bytes());
    session.close();
    TcpListener::bind(address).expect("close releases reserved socket");
}

#[test]
fn attachment_set_or_mount_failure_releases_the_entire_reservation() {
    let base: PublicBaseUrl = "https://public.test/prefix/".parse().expect("base");
    let wrong: PublicBaseUrl = "https://public.test/other/"
        .parse()
        .expect("different mount");
    let output = capture(&[("index.html", b"content")]);
    for mode in 0..3 {
        let reservation = Http
            .reserve(
                BTreeMap::from([
                    (
                        key("site:a"),
                        Binding {
                            address: ephemeral(),
                            base_url: &base,
                        },
                    ),
                    (
                        key("site:b"),
                        Binding {
                            address: ephemeral(),
                            base_url: &base,
                        },
                    ),
                ]),
                options(),
            )
            .expect("reserve exact two-site set");
        let addresses: Vec<_> = reservation.addresses().values().copied().collect();
        let mut sites = BTreeMap::from([(
            key("site:a"),
            Site {
                output: &output,
                media_types: &UNTYPED,
                base_url: &base,
                search_page: None,
            },
        )]);
        match mode {
            0 => (), // Missing second site.
            1 => {
                sites.insert(
                    key("site:c"),
                    Site {
                        output: &output,
                        media_types: &UNTYPED,
                        base_url: &base,
                        search_page: None,
                    },
                );
            }
            _ => {
                sites.insert(
                    key("site:b"),
                    Site {
                        output: &output,
                        media_types: &UNTYPED,
                        base_url: &wrong,
                        search_page: None,
                    },
                );
            }
        }
        assert!(
            reservation.attach(sites).is_err(),
            "no reduced or remounted attachment"
        );
        for address in addresses {
            TcpListener::bind(address).expect("attachment failure releases every socket");
        }
    }
}

#[test]
fn abandoning_unattached_reservation_releases_every_ephemeral_port() {
    let base: PublicBaseUrl = "https://public.test/".parse().expect("base");
    let reservation = Http
        .reserve(
            BTreeMap::from([
                (
                    key("site:a"),
                    Binding {
                        address: ephemeral(),
                        base_url: &base,
                    },
                ),
                (
                    key("site:b"),
                    Binding {
                        address: ephemeral(),
                        base_url: &base,
                    },
                ),
            ]),
            options(),
        )
        .expect("reserve exact set");
    let addresses: Vec<_> = reservation.addresses().values().copied().collect();
    assert_ne!(addresses[0], addresses[1]);
    for address in &addresses {
        assert!(TcpListener::bind(address).is_err());
    }
    drop(reservation);
    for address in addresses {
        TcpListener::bind(address).expect("abandoning reservation releases all ports");
    }
}

#[test]
fn detached_session_replaces_output_on_the_same_socket_without_stale_routes() {
    let base: PublicBaseUrl = "https://public.test/prefix/".parse().expect("public mount");
    let old = capture(&[("index.html", b"old page"), ("old.txt", b"retired")]);
    let mut session = bind(
        &old,
        &base,
        Options {
            max_header_bytes: 128,
            ..options()
        },
    );
    let held = address(&session);
    let urls = session.urls();
    assert_eq!(split(&get(&mut session, "GET", "/prefix/")).1, b"old page");
    let reservation = session.detach();
    drop(old);
    assert_eq!(reservation.addresses()[&key("site:one")], held);
    assert_eq!(
        reservation.urls()[&key("site:one")].as_str(),
        urls[&key("site:one")]
    );
    assert!(
        TcpListener::bind(held).is_err(),
        "detachment retains native socket custody"
    );
    let mut queued = TcpStream::connect(held).expect("queue while no output is attached");
    queued
        .set_read_timeout(Some(Duration::from_secs(3)))
        .expect("bounded queued read");
    write!(queued, "GET /prefix/ HTTP/1.1\r\nHost: {held}\r\n\r\n").expect("queue exact request");
    queued
        .shutdown(Shutdown::Write)
        .expect("complete queued input");
    let new = capture(&[("index.html", b"new page"), ("fresh.bin", b"new resource")]);
    let typed = BTreeMap::from([("fresh.bin".parse().expect("resource path"), MediaType::Css)]);
    let mut session = reservation
        .attach(BTreeMap::from([(
            key("site:one"),
            Site {
                output: &new,
                media_types: &typed,
                base_url: &base,
                search_page: None,
            },
        )]))
        .expect("attach replacement retained output");
    assert_eq!(address(&session), held);
    assert_eq!(session.urls(), urls);
    assert!(
        TcpListener::bind(held).is_err(),
        "replacement does not reopen listener"
    );
    assert_eq!(progress(&mut session).0, Poll::Responded);
    let mut response = Vec::new();
    queued
        .read_to_end(&mut response)
        .expect("connection survives detached interval");
    assert_eq!(split(&response).1, b"new page");
    assert!(
        split(&get(&mut session, "GET", "/prefix/old.txt"))
            .0
            .starts_with("HTTP/1.1 404 ")
    );
    let response = get(&mut session, "GET", "/prefix/fresh.bin");
    assert_eq!(split(&response).1, b"new resource");
    assert!(
        split(&response)
            .0
            .contains("Content-Type: text/css; charset=utf-8")
    );
    // Exhaust the exact retained header ceiling without leaving an unread TCP
    // tail, which can legitimately reset the connection after a rejection.
    let mut exhausted = format!("GET /prefix/ HTTP/1.1\r\nHost: {held}\r\nX-Padding: ");
    exhausted.push_str(&"x".repeat(128 - exhausted.len()));
    assert_eq!(exhausted.len(), 128);
    let response = exchange(&mut session, exhausted.as_bytes());
    let (headers, body) = split(&response);
    assert!(
        headers.starts_with("HTTP/1.1 431 "),
        "the original request ceiling survives detachment"
    );
    assert_eq!(body, b"Preview request refused.\n");
    assert!(
        headers
            .lines()
            .any(|line| line == format!("Content-Length: {}", body.len()))
    );
    session.close();
    TcpListener::bind(held).expect("close releases original listener");
}

#[test]
fn incomplete_replacement_releases_every_detached_listener() {
    let base: PublicBaseUrl = "https://public.test/".parse().expect("public base");
    let old = capture(&[("index.html", b"old")]);
    let output = ["site:a", "site:b"]
        .into_iter()
        .map(|name| {
            (
                key(name),
                Site {
                    output: &old,
                    media_types: &UNTYPED,
                    base_url: &base,
                    search_page: None,
                },
            )
        })
        .collect();
    let session = Http
        .bind(
            output,
            BTreeMap::from([(key("site:a"), ephemeral()), (key("site:b"), ephemeral())]),
            options(),
        )
        .expect("original complete two-site session");
    let addresses = session.addresses().clone();
    let reservation = session.detach();
    drop(old);
    assert_eq!(reservation.addresses(), &addresses);
    for address in addresses.values() {
        assert!(
            TcpListener::bind(address).is_err(),
            "all detached sockets remain held"
        );
    }
    let new = capture(&[("index.html", b"new")]);
    assert!(
        reservation
            .attach(BTreeMap::from([(
                key("site:a"),
                Site {
                    output: &new,
                    media_types: &UNTYPED,
                    base_url: &base,
                    search_page: None,
                },
            )]))
            .is_err(),
        "replacement cannot silently shrink the reserved site set"
    );
    for address in addresses.values() {
        TcpListener::bind(address).expect("failed replacement drops the entire socket set");
    }
}

#[test]
fn replacement_preserves_round_robin_progress_across_queued_sites() {
    let base: PublicBaseUrl = "https://public.test/".parse().expect("public base");
    let first = capture(&[("index.html", b"site a")]);
    let second = capture(&[("index.html", b"site b")]);
    let output = || {
        BTreeMap::from([
            (
                key("site:a"),
                Site {
                    output: &first,
                    media_types: &UNTYPED,
                    base_url: &base,
                    search_page: None,
                },
            ),
            (
                key("site:b"),
                Site {
                    output: &second,
                    media_types: &UNTYPED,
                    base_url: &base,
                    search_page: None,
                },
            ),
        ])
    };
    let mut session = Http
        .bind(
            output(),
            BTreeMap::from([(key("site:a"), ephemeral()), (key("site:b"), ephemeral())]),
            options(),
        )
        .expect("two-site session");
    let addresses = session.addresses().clone();
    let queue = |site: &str| {
        let address = addresses[&key(site)];
        let mut client = TcpStream::connect_timeout(&address, Duration::from_secs(2))
            .expect("queue native site connection");
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("bounded native read");
        write!(client, "GET / HTTP/1.1\r\nHost: {address}\r\n\r\n")
            .expect("queued complete request");
        client
            .shutdown(Shutdown::Write)
            .expect("complete queued input");
        client
    };
    let mut client_a = queue("site:a");
    let mut client_b = queue("site:b");
    assert_eq!(progress(&mut session).0, Poll::Responded);
    let mut response = Vec::new();
    client_a
        .read_to_end(&mut response)
        .expect("first site's turn completed");
    assert_eq!(split(&response).1, b"site a");
    // A busy first site queues another request before the next attachment.
    // Resetting the cursor would serve it instead of the already waiting B.
    let mut next_a = queue("site:a");
    let reservation = session.detach();
    assert_eq!(reservation.addresses(), &addresses);
    let mut session = reservation
        .attach(output())
        .expect("reattach same listeners");
    assert_eq!(progress(&mut session).0, Poll::Responded);
    response.clear();
    client_b
        .read_to_end(&mut response)
        .expect("second site receives the next turn after replacement");
    assert_eq!(split(&response).1, b"site b");
    assert_eq!(progress(&mut session).0, Poll::Responded);
    response.clear();
    next_a
        .read_to_end(&mut response)
        .expect("first site receives its following turn");
    assert_eq!(split(&response).1, b"site a");
    session.close();
}

#[test]
fn video_retained_byte_ranges_head_and_unsatisfied_requests() {
    for (filename, mime) in [
        ("film.mp4", "video/mp4"),
        ("film.Mp4", "video/mp4"),
        ("film.webm", "video/webm"),
        ("film.WeBm", "video/webm"),
    ] {
        let output = capture(&[(filename, b"0123456789")]);
        let identity = output.inventory().identity();
        let base = "https://public.test/".parse().unwrap();
        let mut session = bind(&output, &base, options());
        let host = address(&session);
        for (method, extra, status, expected, content_range, length) in [
            ("GET", "", 200, "0123456789", None, 10),
            (
                "GET",
                "Range: bytes=2-5\r\n",
                206,
                "2345",
                Some("bytes 2-5/10"),
                4,
            ),
            (
                "GET",
                "Range: bytes=7-\r\n",
                206,
                "789",
                Some("bytes 7-9/10"),
                3,
            ),
            (
                "GET",
                "Range: bytes=-3\r\n",
                206,
                "789",
                Some("bytes 7-9/10"),
                3,
            ),
            (
                "GET",
                "Range: bytes=8-99\r\n",
                206,
                "89",
                Some("bytes 8-9/10"),
                2,
            ),
            (
                "GET",
                "Range: bytes=-99\r\n",
                206,
                "0123456789",
                Some("bytes 0-9/10"),
                10,
            ),
            (
                "GET",
                "Range: bytes=10-\r\n",
                416,
                "",
                Some("bytes */10"),
                0,
            ),
            ("GET", "Range: bytes=-0\r\n", 416, "", Some("bytes */10"), 0),
            ("GET", "Range: bytes=3-2\r\n", 200, "0123456789", None, 10),
            (
                "GET",
                "Range: bytes=0-1,3-4\r\n",
                200,
                "0123456789",
                None,
                10,
            ),
            (
                "GET",
                "Range: bytes=999999999999999999999999-\r\n",
                200,
                "0123456789",
                None,
                10,
            ),
            ("GET", "Range: items=1-2\r\n", 200, "0123456789", None, 10),
            (
                "GET",
                "Range: bytes=0-1\r\nIf-Range: \"unknown\"\r\n",
                200,
                "0123456789",
                None,
                10,
            ),
            ("HEAD", "Range: bytes=2-5\r\n", 200, "", None, 10),
        ] {
            let response = exchange(
                &mut session,
                format!("{method} /{filename} HTTP/1.1\r\nHost: {host}\r\n{extra}\r\n").as_bytes(),
            );
            let (headers, body) = split(&response);
            assert!(
                headers.starts_with(&format!("HTTP/1.1 {status} ")),
                "{extra}: {headers}"
            );
            assert!(headers.contains(&format!("Content-Type: {mime}\r\n")));
            assert!(headers.contains("Accept-Ranges: bytes"));
            assert!(headers.contains(&format!("Content-Length: {length}\r\n")));
            assert_eq!(body, expected.as_bytes(), "{extra}");
            match content_range {
                Some(range) => assert!(
                    headers
                        .lines()
                        .any(|line| line == format!("Content-Range: {range}"))
                ),
                None => assert!(!headers.contains("Content-Range:")),
            }
        }
        let duplicate = exchange(
        &mut session,
        format!(
            "GET /{filename} HTTP/1.1\r\nHost: {host}\r\nRange: bytes=0-1\r\nRange: bytes=2-3\r\n\r\n"
        )
        .as_bytes(),
    );
        assert!(split(&duplicate).0.starts_with("HTTP/1.1 400 "));
        assert_eq!(identity, output.inventory().identity());
    }
}

#[test]
fn missing_local_pages_offer_only_retained_mount_relative_recovery() {
    let base: PublicBaseUrl = "https://public.test/mount/".parse().unwrap();
    let output = capture(&[("index.html", b"home"), ("find/index.html", b"search")]);
    let search = "/find/".parse().unwrap();
    let mut session = Http
        .bind(
            BTreeMap::from([(
                key("site:one"),
                Site {
                    output: &output,
                    media_types: &UNTYPED,
                    search_page: Some(&search),
                    base_url: &base,
                },
            )]),
            BTreeMap::from([(key("site:one"), ephemeral())]),
            options(),
        )
        .unwrap();
    let page = get(&mut session, "GET", "/mount/unknown/");
    let (headers, body) = split(&page);
    assert!(headers.starts_with("HTTP/1.1 404 "));
    assert!(headers.contains("Content-Type: text/html"));
    let body = std::str::from_utf8(body).unwrap();
    assert!(body.contains("href=\"/mount/\">Home"));
    assert!(body.contains("href=\"/mount/find/\">Search"));
    let head = get(&mut session, "HEAD", "/mount/unknown/");
    assert_eq!(split(&head), (headers, &[][..]));
    for target in ["/mount/missing.css", "/outside/"] {
        let resource = get(&mut session, "GET", target);
        let (headers, body) = split(&resource);
        assert!(headers.starts_with("HTTP/1.1 404 "));
        assert_eq!(body, b"Preview request refused.\n");
    }
    let address = address(&session);
    let request =
        format!("GET /mount/untyped HTTP/1.1\r\nHost: {address}\r\nSec-Fetch-Dest: script\r\n\r\n");
    let resource = exchange(&mut session, request.as_bytes());
    assert_eq!(split(&resource).1, b"Preview request refused.\n");
    let malformed = get(&mut session, "GET", "/mount/%2e%2e/unknown");
    assert!(split(&malformed).0.starts_with("HTTP/1.1 400 "));
    assert_eq!(split(&malformed).1, b"Preview request refused.\n");
    assert_eq!(split(&get(&mut session, "GET", "/mount/")).1, b"home");
    assert_eq!(
        split(&get(&mut session, "GET", "/mount/find/")).1,
        b"search"
    );
}
