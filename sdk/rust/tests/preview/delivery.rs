//! Served-byte observations use native curl, actual loopback responses and retained output.

use super::*;
use gazette_sdk::{delivery, process, source::CapturedSource};

fn curl(directory: &Path) -> delivery::Curl {
    let program = super::native::curl()
        .to_str()
        .expect("selected curl path is UTF-8")
        .to_owned();
    let bytes = fs::read(&program).expect("selected curl executable remains readable");
    delivery::Curl {
        program,
        executable: process::ExecutablePin {
            bytes: bytes.len() as u64,
            sha256: Sha256Digest::of(&bytes),
        },
        directory: directory.to_str().unwrap().into(),
    }
}

fn options() -> delivery::Options {
    let mut options = delivery::Options::default();
    options.http.allow_read = true;
    options.http.allow_execute = true;
    options.http.allow_network = true;
    options.http.timeout_ms = 3000;
    options
}

fn selection(base: &str, routes: &[&str]) -> delivery::Selection {
    delivery::Selection {
        base_url: base.parse().unwrap(),
        routes: routes.iter().map(|route| route.parse().unwrap()).collect(),
    }
}

#[test]
fn exact_selected_preview_routes_match_retained_publication_despite_source_mutation() {
    let fixture = Fixture::linked();
    let inputs = fixture.acquire(&SITES);
    let reservation = preview::reserve(&inputs, addresses(&SITES), permission()).unwrap();
    let urls = reservation.urls().clone();
    fs::write(
        fixture.directory.path().join("one/page.md"),
        "not the captured document",
    )
    .unwrap();
    fs::remove_file(fixture.directory.path().join("theme/templates/page.html")).unwrap();
    let rendered = render_using(&inputs, false, Some(&reservation));
    assert!(rendered.is_complete());
    let tree = rendered.sites()[&key("site:one")]
        .as_ref()
        .unwrap()
        .site()
        .output();
    let identity = tree.inventory().identity();
    let selection = delivery::Selection {
        base_url: urls[&key("site:one")].clone(),
        routes: ["/model.json", "/"]
            .into_iter()
            .map(|route| route.parse().unwrap())
            .collect(),
    };
    let curl = curl(fixture.directory.path());
    let mut session = preview::attach(reservation, &rendered).unwrap();
    let report = std::thread::scope(|scope| {
        let observer = scope.spawn(|| delivery::observe(tree, &curl, &selection, options()));
        let deadline = Instant::now() + Duration::from_secs(10);
        while !observer.is_finished() {
            assert!(
                Instant::now() < deadline,
                "bounded complete delivery observation"
            );
            session.poll().expect("native preview response");
            std::thread::yield_now();
        }
        observer
            .join()
            .expect("observer thread")
            .expect("complete selected report")
    });
    assert!(report.matches_selected_routes());
    assert_eq!(report.output, identity);
    assert_eq!(report.base_url, selection.base_url);
    assert_eq!(report.observations.len(), 2);
    for (observed, route) in report.observations.iter().zip(&selection.routes) {
        assert_eq!(&observed.route, route);
        assert_eq!(observed.output, *route.output());
        assert_eq!(observed.url, selection.base_url.resolve(route));
        assert!(observed.url.contains("/pr%C3%A9fix/"));
        let expected = tree.file(route.output()).unwrap();
        assert_eq!(observed.expected.sha256, Sha256Digest::of(expected));
        assert_eq!(observed.expected.bytes, expected.len() as u64);
        assert!(
            matches!(observed.outcome, delivery::Outcome::Matches {observed: body} if body == observed.expected)
        );
    }
    assert_eq!(
        tree.inventory().identity(),
        identity,
        "observations do not alter artifact identity"
    );
    session.close();
}

fn response_report(
    tree: &CapturedSource,
    curl: &delivery::Curl,
    response: &[u8],
) -> delivery::Report {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let selection = selection(
        &format!("http://{}/mount/", listener.local_addr().unwrap()),
        &["/"],
    );
    std::thread::scope(|scope| {
        let server = scope.spawn(|| {
            let deadline = Instant::now() + Duration::from_secs(8);
            let (mut stream, _) = loop {
                match listener.accept() {
                    Ok(client) => break client,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "native client did not connect");
                        std::thread::yield_now();
                    }
                    Err(error) => panic!("native accept: {error}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                assert!(request.len() < 16384);
                let mut byte = [0];
                stream
                    .read_exact(&mut byte)
                    .expect("complete native GET headers");
                request.push(byte[0]);
            }
            let request = std::str::from_utf8(&request).unwrap();
            assert!(request.starts_with("GET /mount/ HTTP/1.1\r\n"), "{request}");
            let request = request.to_ascii_lowercase();
            assert!(request.contains("accept-encoding: identity\r\n"));
            assert!(request.contains("cache-control: no-cache\r\n"));
            // Oversized replies may be disconnected by the bounded native reader.
            let _ = stream.write_all(response);
        });
        let report = delivery::observe(tree, curl, &selection, options())
            .expect("selection stays valid when serving fails");
        server.join().expect("bounded native response fixture");
        report
    })
}

#[test]
fn changed_missing_redirected_oversized_and_truncated_responses_remain_explicit() {
    let fixture = Fixture::new();
    let inputs = fixture.acquire(&SITES[..1]);
    let rendered = render(&inputs, false);
    let tree = rendered.sites()[&key("site:one")]
        .as_ref()
        .unwrap()
        .site()
        .output();
    let curl = curl(fixture.directory.path());
    let large = vec![b'x'; options().http.response_bytes + 1];
    let mut oversized = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        large.len()
    )
    .into_bytes();
    oversized.extend(large);
    let cases: [(&[u8], &str); 5] = [
        (b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\nchanged", "differs"),
        (b"HTTP/1.1 404 Not Found\r\nContent-Length: 7\r\nConnection: close\r\n\r\nmissing", "missing"),
        (b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/never-follow\r\nContent-Length: 5\r\nConnection: close\r\n\r\nmoved", "redirect"),
        (&oversized, "oversized"),
        (b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\nshort", "truncated"),
    ];
    for (wire, case) in cases {
        let report = response_report(tree, &curl, wire);
        assert!(!report.matches_selected_routes(), "{case}");
        assert_eq!(report.output, tree.inventory().identity());
        assert_eq!(
            report.observations.len(),
            1,
            "failure never removes its selected route"
        );
        let observed = &report.observations[0];
        assert_eq!(observed.route.as_str(), "/");
        assert_eq!(
            observed.expected.sha256,
            Sha256Digest::of(tree.file(&"index.html".parse().unwrap()).unwrap())
        );
        match (case, &observed.outcome) {
            ("differs", delivery::Outcome::Differs { observed }) => {
                assert_eq!(observed.sha256, Sha256Digest::of(b"changed"))
            }
            (
                "missing",
                delivery::Outcome::HttpFailure {
                    http_status: 404,
                    observed,
                },
            ) => assert_eq!(observed.sha256, Sha256Digest::of(b"missing")),
            (
                "redirect",
                delivery::Outcome::HttpFailure {
                    http_status: 302,
                    observed,
                },
            ) => assert_eq!(observed.sha256, Sha256Digest::of(b"moved")),
            ("oversized" | "truncated", delivery::Outcome::Unavailable { .. }) => (),
            (_, unexpected) => panic!("{case}: unexpected outcome {unexpected:?}"),
        }
    }
}

#[test]
fn complete_selection_validation_refuses_before_first_native_get() {
    let fixture = Fixture::new();
    let inputs = fixture.acquire(&SITES[..1]);
    let rendered = render(&inputs, false);
    let tree = rendered.sites()[&key("site:one")]
        .as_ref()
        .unwrap()
        .site()
        .output();
    let curl = curl(fixture.directory.path());
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let base = format!("http://{}/mount/", listener.local_addr().unwrap());
    for routes in [&["/", "/absent.html"][..], &["/", "/"][..], &[][..]] {
        let failure =
            delivery::observe(tree, &curl, &selection(&base, routes), options()).unwrap_err();
        assert_eq!(failure.code(), "delivery_selection");
        assert!(
            matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
            "invalid late selection must not GET even its valid prefix"
        );
    }
    let mut bounded = options();
    bounded.max_routes = 1;
    assert_eq!(
        delivery::observe(
            tree,
            &curl,
            &selection(&base, &["/", "/model.json"]),
            bounded
        )
        .unwrap_err()
        .code(),
        "delivery_limit"
    );
    assert!(
        matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
    );
}
