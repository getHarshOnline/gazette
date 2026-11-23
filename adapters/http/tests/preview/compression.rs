//! Negotiated content encoding preserves retained bytes and video range offsets.

use super::*;
use flate2::read::GzDecoder;

fn encoded(session: &mut Session<'_>, method: &str, path: &str, headers: &str) -> Vec<u8> {
    exchange(
        session,
        format!(
            "{method} {path} HTTP/1.1\r\nHost: {}\r\n{headers}\r\n",
            address(session)
        )
        .as_bytes(),
    )
}

#[test]
fn compressed_text_round_trips_and_head_uses_the_same_representation() {
    let source = "<main>Retained publication content.</main>\n".repeat(300);
    let output = capture(&[("index.html", source.as_bytes())]);
    let identity = output.inventory().identity();
    let base = "https://public.test/".parse().unwrap();
    let mut session = bind(&output, &base, options());
    let response = encoded(&mut session, "GET", "/", "Accept-Encoding: br, gzip\r\n");
    let (headers, body) = split(&response);
    assert!(headers.starts_with("HTTP/1.1 200 "));
    assert!(headers.contains("Content-Encoding: gzip"));
    assert!(headers.contains("Vary: Accept-Encoding\r\n"));
    assert!(headers.contains("Cache-Control: no-store\r\n"));
    assert!(headers.contains(&format!("Content-Length: {}\r\n", body.len())));
    assert!(body.len() < source.len() / 4);
    let mut restored = String::new();
    GzDecoder::new(body).read_to_string(&mut restored).unwrap();
    assert_eq!(restored, source);
    assert_eq!(
        response,
        encoded(&mut session, "GET", "/", "Accept-Encoding: br, gzip\r\n")
    );
    let head = encoded(&mut session, "HEAD", "/", "Accept-Encoding: br, gzip\r\n");
    assert_eq!(split(&head), (headers, &[][..]));
    assert_eq!(output.inventory().identity(), identity);
}

#[test]
fn qualities_wildcards_empty_fields_and_refusals_are_respected() {
    let source = "const value = 'published content';\n".repeat(100);
    let output = capture(&[("app.js", source.as_bytes()), ("small.txt", b"tiny")]);
    let base = "https://public.test/".parse().unwrap();
    let mut session = bind(&output, &base, options());
    for (value, gzip, status) in [
        ("", false, 200),
        ("gzip", true, 200),
        ("GZip;q=0.500", true, 200),
        ("x-gzip", true, 200),
        ("br", false, 200),
        ("gzip;q=0", false, 200),
        ("*;q=0.5", true, 200),
        ("gzip;q=0, *;q=1", false, 200),
        ("gzip;q=0.5, identity;q=1", false, 200),
        ("gzip;q=1, identity;q=0.5", true, 200),
        ("gzip;q=0.5, identity;q=0", true, 200),
        ("gzip;q=1, *;q=0", true, 200),
        ("identity;q=1, *;q=0", false, 200),
        ("*;q=0", false, 406),
        ("br, identity;q=0", false, 406),
        ("gzip;q=0, identity;q=0", false, 406),
        ("gzip;q=2", false, 400),
        ("gzip;q=0.0001", false, 400),
        ("gzip;q=.5", false, 400),
        ("gzip;q=1.1", false, 400),
        ("gzip;q=NaN", false, 400),
        ("gzip;q=0.5;q=1", false, 400),
    ] {
        let response = encoded(
            &mut session,
            "GET",
            "/app.js",
            &format!("Accept-Encoding: {value}\r\n"),
        );
        let (headers, body) = split(&response);
        assert!(
            headers.starts_with(&format!("HTTP/1.1 {status} ")),
            "{value}: {headers}"
        );
        assert_eq!(headers.contains("Content-Encoding: gzip"), gzip, "{value}");
        if status == 406 {
            assert!(body.is_empty());
        }
        if status == 200 && !gzip {
            assert_eq!(body, source.as_bytes());
        }
    }
    let duplicates = encoded(
        &mut session,
        "GET",
        "/app.js",
        "Accept-Encoding: gzip;q=0\r\nAccept-Encoding: gzip;q=1\r\n",
    );
    assert!(!split(&duplicates).0.contains("Content-Encoding:"));
    assert_eq!(split(&duplicates).1, source.as_bytes());
    let tiny = encoded(
        &mut session,
        "GET",
        "/small.txt",
        "Accept-Encoding: gzip\r\n",
    );
    assert_eq!(split(&tiny).1, b"tiny");
    let forced = encoded(
        &mut session,
        "GET",
        "/small.txt",
        "Accept-Encoding: gzip, identity;q=0\r\n",
    );
    assert!(split(&forced).0.contains("Content-Encoding: gzip"));
    let mut restored = Vec::new();
    GzDecoder::new(split(&forced).1)
        .read_to_end(&mut restored)
        .unwrap();
    assert_eq!(restored, b"tiny");
}

#[test]
fn binary_and_video_ranges_keep_identity_bytes() {
    let source = vec![42; 2048];
    let output = capture(&[("image.webp", &source), ("film.mp4", b"0123456789")]);
    let base = "https://public.test/".parse().unwrap();
    let mut session = bind(&output, &base, options());
    let image = encoded(
        &mut session,
        "GET",
        "/image.webp",
        "Accept-Encoding: gzip\r\n",
    );
    assert_eq!(split(&image).1, source);
    assert!(!split(&image).0.contains("Content-Encoding:"));
    let range = encoded(
        &mut session,
        "GET",
        "/film.mp4",
        "Accept-Encoding: gzip\r\nRange: bytes=2-5\r\n",
    );
    let (headers, body) = split(&range);
    assert!(headers.starts_with("HTTP/1.1 206 "));
    assert!(headers.contains("Content-Range: bytes 2-5/10"));
    assert!(!headers.contains("Content-Encoding:"));
    assert_eq!(body, b"2345");
    let refused = encoded(
        &mut session,
        "GET",
        "/film.mp4",
        "Accept-Encoding: gzip, identity;q=0\r\nRange: bytes=2-5\r\n",
    );
    assert!(split(&refused).0.starts_with("HTTP/1.1 406 "));
    assert!(!split(&refused).0.contains("Content-Range:"));
    assert!(split(&refused).1.is_empty());
}

#[test]
fn producer_media_type_selects_compression_without_guessing_from_the_filename() {
    let source = "text content and reusable styles\n".repeat(200);
    let output = capture(&[
        ("asset.bin", source.as_bytes()),
        ("opaque.css", source.as_bytes()),
    ]);
    let media = BTreeMap::from([
        ("asset.bin".parse().unwrap(), MediaType::Css),
        ("opaque.css".parse().unwrap(), MediaType::Woff2),
    ]);
    let base = "https://public.test/".parse().unwrap();
    let mut session = Http
        .bind(
            BTreeMap::from([(
                key("site:one"),
                Site {
                    output: &output,
                    base_url: &base,
                    media_types: &media,
                    search_page: None,
                },
            )]),
            BTreeMap::from([(key("site:one"), ephemeral())]),
            options(),
        )
        .unwrap();
    for (path, compressed) in [("/asset.bin", true), ("/opaque.css", false)] {
        let response = encoded(&mut session, "GET", path, "Accept-Encoding: gzip\r\n");
        assert_eq!(
            split(&response).0.contains("Content-Encoding: gzip"),
            compressed
        );
    }
}
