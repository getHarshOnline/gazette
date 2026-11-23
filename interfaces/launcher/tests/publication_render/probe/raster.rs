//! Real retained raster containers: metadata coverage without rewriting pixels or bytes.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use gazette_sdk::raster::{self, Format};

use super::{RetainedOutput, build, fs, policy::policy, rules, source};

// Fixed 1×1 RGBA PNGs with complete zlib streams and native chunk CRCs. The
// compressed Comment field differs; the IDAT/pixel bytes are identical.
pub(in super::super) const CLEAN: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAAIHpUWHRDb21tZW50AAB4nEtKzctMz1PITS1JTEksSQQALnIF1R9Frg8AAAANSURBVHicY/jPwPAfAAUAAf+JmT0dAAAAAElFTkSuQmCC";
const SECRET: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAAJnpUWHRDb21tZW50AAB4nAsI8gxzDHGND3Z1DnINiTcztLQwMgcAROAF4Xcp5cgAAAANSURBVHicY/jPwPAfAAUAAf+JmT0dAAAAAElFTkSuQmCC";
const UNKNOWN: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAAFHZwQWdvcGFxdWUgcHJpdmF0ZSBieXRlc9SEiIMAAAANSURBVHicY/jPwPAfAAUAAf+JmT0dAAAAAElFTkSuQmCC";
const EXIF: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAACmVYSWZJSSoACAAAAAAAC5zIQAAAAA1JREFUeJxj+M/A8B8ABQAB/4mZPR0AAAAASUVORK5CYII=";
// Same previously qualified 2×1 RGB fixture used by the notebook pixel path.
const JPEG: &str = "/9j/4AAQSkZJRgABAgAAAQABAAD/wAARCAABAAIDAREAAhEBAxEB/9sAQwADAgIDAgIDAwMDBAMDBAUIBQUEBAUKBwcGCAwKDAwLCgsLDQ4SEA0OEQ4LCxAWEBETFBUVFQwPFxgWFBgSFBUU/9sAQwEDBAQFBAUJBQUJFA0LDRQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQU/8QAHwAAAQUBAQEBAQEAAAAAAAAAAAECAwQFBgcICQoL/8QAtRAAAgEDAwIEAwUFBAQAAAF9AQIDAAQRBRIhMUEGE1FhByJxFDKBkaEII0KxwRVS0fAkM2JyggkKFhcYGRolJicoKSo0NTY3ODk6Q0RFRkdISUpTVFVWV1hZWmNkZWZnaGlqc3R1dnd4eXqDhIWGh4iJipKTlJWWl5iZmqKjpKWmp6ipqrKztLW2t7i5usLDxMXGx8jJytLT1NXW19jZ2uHi4+Tl5ufo6erx8vP09fb3+Pn6/8QAHwEAAwEBAQEBAQEBAQAAAAAAAAECAwQFBgcICQoL/8QAtREAAgECBAQDBAcFBAQAAQJ3AAECAxEEBSExBhJBUQdhcRMiMoEIFEKRobHBCSMzUvAVYnLRChYkNOEl8RcYGRomJygpKjU2Nzg5OkNERUZHSElKU1RVVldYWVpjZGVmZ2hpanN0dXZ3eHl6goOEhYaHiImKkpOUlZaXmJmaoqOkpaanqKmqsrO0tba3uLm6wsPExcbHyMnK0tPU1dbX2Nna4uPk5ebn6Onq8vP09fb3+Pn6/9oADAMBAAIRAxEAPwDovBn/ACJ+hf8AXhB/6LWv4tx3+91f8UvzZ/GHij/yXuf/APYZif8A09M//9k=";

pub(in super::super) fn decode(value: &str) -> Vec<u8> {
    STANDARD
        .decode(value)
        .expect("fixed complete image fixture")
}

fn jpeg_with_secret_comment() -> Vec<u8> {
    let secret = b"PRIVATE_SECRET_619827";
    let mut jpeg = decode(JPEG);
    let mut comment = vec![0xff, 0xfe];
    comment.extend_from_slice(
        &u16::try_from(secret.len() + 2)
            .expect("length")
            .to_be_bytes(),
    );
    comment.extend_from_slice(secret);
    jpeg.splice(2..2, comment);
    jpeg
}

#[test]
fn qualified_png_metadata_binds_retained_bytes_without_promoting_diagnostic_coverage() {
    let png = decode(CLEAN);
    let metadata = raster::metadata(Format::Png, &png, Default::default())
        .expect("native complete PNG metadata extraction");
    assert_eq!(metadata.fields, ["Comment", "benign metadata"]);
    assert_eq!(metadata.bytes, png.len());
    assert_eq!(metadata.sha256, source::Sha256Digest::of(&png));
    assert_eq!(
        metadata.sha256.to_string(),
        "4218da1beb9ec2d883cb96d9feed32e5a7f1e55e4af2fa8c9aea33b047bd2540"
    );
    let pixels = raster::inspect(Format::Png, &png, Default::default())
        .expect("same exact retained container decodes real pixels");
    assert_eq!((pixels.width, pixels.height), (1, 1));
    let fixture = RetainedOutput::new(&[("pixel.PNG", &png)]);
    let identity = fixture.output().inventory().identity();
    fs::write(
        fixture.directory.path().join("pixel.PNG"),
        b"not reacquired",
    )
    .expect("later native source change");
    let report = build::check_disclosure_policy(
        fixture.output(),
        &fixture.media_types,
        &policy(),
        Default::default(),
    )
    .expect("required scan uses retained complete container");
    assert!(report.findings().is_empty());
    assert!(matches!(report.files()[0].decoding,
        build::DisclosureDecoding::RasterMetadata { format: Format::Png, profile }
        if profile == metadata.tool));
    let json = serde_json::to_value(&report).expect("native report");
    assert_eq!(json["status"], "required-supported-profile-evidence");
    assert_eq!(fixture.output().inventory().identity(), identity);
    assert_eq!(
        fixture.output().file(&"pixel.PNG".parse().expect("path")),
        Some(png.as_slice())
    );
    let selected = rules(&[("rule:text", "benign metadata")]);
    let diagnostic = fixture
        .probe_disclosure(&selected, Default::default())
        .expect("native diagnostic retains decoded metadata findings without certification");
    assert_eq!(diagnostic.findings().len(), 1);
    let finding = &diagnostic.findings()[0];
    assert_eq!(finding.path.as_str(), "pixel.PNG");
    assert_eq!(finding.rule.as_str(), "rule:text");
    assert_eq!(finding.class, None);
    assert_eq!(
        finding.representation,
        build::DisclosureRepresentation::RasterMetadata
    );
    assert!(matches!(diagnostic.files()[0].decoding,
        build::DisclosureDecoding::RasterMetadata { format: Format::Png, profile }
        if profile == metadata.tool));
    let expected =
        serde_json::to_value(diagnostic.findings()).expect("decoded diagnostic findings");
    let diagnostic = serde_json::to_value(diagnostic).expect("diagnostic report");
    assert_eq!(diagnostic["status"], "partial-evidence");
    for unsupported_claim in [
        "policy_sha256",
        "certified",
        "approved",
        "admitted",
        "release",
    ] {
        assert!(diagnostic.get(unsupported_claim).is_none());
    }
    let error = build::check_disclosure(
        fixture.output(),
        &fixture.media_types,
        &selected,
        Default::default(),
    )
    .expect_err("the same actual metadata findings refuse required qualification");
    let build::DisclosureProbeError::Findings { findings } = error else {
        panic!("supported metadata findings remain distinct from malformed or missing coverage");
    };
    assert_eq!(
        serde_json::to_value(findings).expect("required decoded findings"),
        expected
    );
}

#[test]
fn clean_png_data_payload_records_exact_native_coverage_but_binary_charset_is_refused() {
    let png = decode(CLEAN);
    let metadata =
        raster::metadata(Format::Png, &png, Default::default()).expect("native metadata identity");
    let html = format!("<img src=\"data:image/png;base64,{CLEAN}\" alt=\"pixel\">");
    let fixture = RetainedOutput::new(&[("index.html", html.as_bytes())]);
    let report = build::check_disclosure_policy(
        fixture.output(),
        &fixture.media_types,
        &policy(),
        Default::default(),
    )
    .expect("PNG binary payload is not incorrectly required to be UTF-8");
    let value = serde_json::to_value(report).expect("native embedded coverage");
    assert!(
        value["embedded"]
            .as_array()
            .expect("embedded results")
            .iter()
            .any(|entry| entry["path"] == "index.html"
                && entry["sha256"] == metadata.sha256.to_string()
                && entry["bytes"] == png.len()
                && entry["media_type"] == "image/png"
                && entry["decoding"]["raster-metadata"]["profile"] == metadata.tool)
    );
    for charset in ["utf-8", "us-ascii", "iso-8859-1"] {
        let html = format!("<img src=\"data:image/png;charset={charset};base64,{CLEAN}\">");
        let fixture = RetainedOutput::new(&[("index.html", html.as_bytes())]);
        let error = build::check_disclosure_policy(
            fixture.output(),
            &fixture.media_types,
            &policy(),
            Default::default(),
        )
        .expect_err("a text charset never qualifies a binary PNG payload");
        assert!(matches!(
            error,
            build::DisclosureProbeError::Encoded {
                reason: "binary charset is unsupported",
                ..
            }
        ));
    }
}

#[test]
fn compressed_png_canaries_are_found_in_files_and_nested_data_urls_without_rewrite() {
    let png = decode(SECRET);
    let canary = b"PRIVATE_SECRET_619827";
    assert!(!png.windows(canary.len()).any(|part| part == canary));
    let metadata =
        raster::metadata(Format::Png, &png, Default::default()).expect("native zTXt decoder");
    assert_eq!(metadata.fields, ["Comment", "PRIVATE_SECRET_619827"]);
    let html = format!("<img src=\"data:image/png;base64,{SECRET}\" alt=\"pixel\">");
    for (path, bytes) in [
        ("pixel.png", png.as_slice()),
        ("index.html", html.as_bytes()),
    ] {
        let fixture = RetainedOutput::new(&[(path, bytes)]);
        let identity = fixture.output().inventory().identity();
        let error = build::check_disclosure_policy(
            fixture.output(),
            &fixture.media_types,
            &policy(),
            Default::default(),
        )
        .expect_err("compressed private metadata is a required finding, never a successful report");
        assert_eq!(error.code(), "disclosure_findings");
        let build::DisclosureProbeError::Findings { findings } = error else {
            panic!("actual findings expected")
        };
        assert!(findings.iter().any(|finding| finding.path.as_str() == path
            && finding.rule.as_str() == "rule:secret"
            && finding.class == Some(build::DisclosureClass::Secret)
            && finding.representation == build::DisclosureRepresentation::RasterMetadata));
        assert_eq!(fixture.output().inventory().identity(), identity);
        assert_eq!(
            fs::read(fixture.directory.path().join(path)).expect("unchanged native bytes"),
            bytes
        );
    }
}

#[test]
fn unsupported_or_malformed_raster_containers_cannot_become_zero_finding_success() {
    let mut jpeg = decode(JPEG);
    let pixels =
        raster::inspect(Format::Jpeg, &jpeg, Default::default()).expect("valid JPEG pixel input");
    assert_eq!((pixels.width, pixels.height), (2, 1));
    // An unqualified APP container stays unsupported even when pixels decode.
    jpeg.splice(
        2..2,
        [0xff, 0xe1, 0, 9, b'p', b'r', b'i', b'v', b'a', b't', b'e'],
    );
    for (path, bytes) in [
        ("photo.jpeg", jpeg),
        ("unknown.png", decode(UNKNOWN)),
        ("exif.png", decode(EXIF)),
    ] {
        let fixture = RetainedOutput::new(&[(path, &bytes)]);
        let error = build::check_disclosure_policy(
            fixture.output(),
            &fixture.media_types,
            &policy(),
            Default::default(),
        )
        .expect_err("unqualified container is not implicitly supported");
        assert!(matches!(
            error,
            build::DisclosureProbeError::Unsupported {
                kind: "raster metadata",
                ..
            }
        ));
        assert_eq!(error.code(), "disclosure_unsupported");
        assert!(!error.to_string().contains(path));
    }
    let mut truncated = decode(CLEAN);
    truncated.truncate(truncated.len() - 1);
    let fixture = RetainedOutput::new(&[("first.txt", b"clean text"), ("late.png", &truncated)]);
    let error = build::check_disclosure_policy(
        fixture.output(),
        &fixture.media_types,
        &policy(),
        Default::default(),
    )
    .expect_err("late invalid container returns no partial report");
    assert_eq!(error.code(), "disclosure_probe_raster");
}

#[test]
fn jpeg_metadata_reaches_retained_policy_and_nested_data_urls_without_reencoding() {
    let clean = decode(JPEG);
    let metadata =
        raster::metadata(Format::Jpeg, &clean, Default::default()).expect("native JPEG metadata");
    assert!(metadata.fields.is_empty());
    assert_eq!(metadata.decoded_bytes, 2 * 3);
    let fixture = RetainedOutput::new(&[("photo.jpeg", &clean)]);
    let report = build::check_disclosure_policy(
        fixture.output(),
        &fixture.media_types,
        &policy(),
        Default::default(),
    )
    .expect("clean supported JPEG");
    assert!(
        matches!(report.files()[0].decoding, build::DisclosureDecoding::RasterMetadata { format: Format::Jpeg, profile } if profile == metadata.tool)
    );
    assert_eq!(
        serde_json::to_value(report).expect("report")["status"],
        "required-supported-profile-evidence"
    );
    let jpeg = jpeg_with_secret_comment();
    let html = format!(
        "<img src=\"data:image/jpeg;base64,{}\" alt=\"pixel\">",
        STANDARD.encode(&jpeg)
    );
    for (path, bytes) in [
        ("photo.jpeg", jpeg.as_slice()),
        ("index.html", html.as_bytes()),
    ] {
        let fixture = RetainedOutput::new(&[(path, bytes)]);
        let identity = fixture.output().inventory().identity();
        let error = build::check_disclosure_policy(
            fixture.output(),
            &fixture.media_types,
            &policy(),
            Default::default(),
        )
        .expect_err("private JPEG comment");
        let build::DisclosureProbeError::Findings { findings } = error else {
            panic!("actual metadata findings expected")
        };
        assert!(findings.iter().any(|finding| finding.path.as_str() == path
            && finding.rule.as_str() == "rule:secret"
            && finding.representation == build::DisclosureRepresentation::RasterMetadata));
        assert_eq!(fixture.output().inventory().identity(), identity);
        assert_eq!(
            fixture.output().file(&path.parse().expect("path")),
            Some(bytes)
        );
    }
}

#[test]
fn one_immutable_canary_matrix_reaches_every_supported_output_surface_without_removal() {
    let secret = "PRIVATE_SECRET_619827";
    let nested = format!(
        "<p data-percent=\"PRIVATE%255fSECRET_619827\" data-payload=\"data:text/plain;base64,{}\"></p>",
        STANDARD.encode(secret)
    );
    let files = vec![
        ("page.html", b"<p>PRIVATE&#95;SECRET_619827</p>".to_vec()),
        (
            "site.jsonld",
            br#"{"value":"PRIVATE\u005fSECRET_619827"}"#.to_vec(),
        ),
        (
            "search.json",
            br#"[{"value":"PRIVATE\u005fSECRET_619827"}]"#.to_vec(),
        ),
        ("llms.txt", secret.as_bytes().to_vec()),
        (
            "sitemap.xml",
            b"<urlset><loc>PRIVATE&#95;SECRET_619827</loc></urlset>".to_vec(),
        ),
        (
            "feed.atom",
            b"<feed><title>PRIVATE&#95;SECRET_619827</title></feed>".to_vec(),
        ),
        (
            "image.svg",
            b"<svg xmlns=\"http://www.w3.org/2000/svg\"><text>PRIVATE&#95;SECRET_619827</text></svg>"
                .to_vec(),
        ),
        (
            "module.mjs",
            br#"export const value = "PRIVATE\u005fSECRET_619827";"#.to_vec(),
        ),
        (
            "style.css",
            br#"p { content: "PRIVATE\5f SECRET_619827"; }"#.to_vec(),
        ),
        (
            "module.js.map",
            br#"{"version":3,"sourcesContent":["PRIVATE\u005fSECRET_619827"]}"#.to_vec(),
        ),
        ("PRIVATE_SECRET_619827.txt", b"clean body".to_vec()),
        ("pixel.png", decode(SECRET)),
        ("photo.jpeg", jpeg_with_secret_comment()),
        ("nested.html", nested.into_bytes()),
    ];
    let selected = files
        .iter()
        .map(|(path, bytes)| (*path, bytes.as_slice()))
        .collect::<Vec<_>>();
    let fixture = RetainedOutput::new(&selected);
    let identity = fixture.output().inventory().identity();
    let error = build::check_disclosure_policy(
        fixture.output(),
        &fixture.media_types,
        &policy(),
        Default::default(),
    )
    .expect_err("every supported surface retains its canary");
    let build::DisclosureProbeError::Findings { findings } = error else {
        panic!("supported canary matrix must return complete findings")
    };
    for (path, representation) in [
        ("page.html", build::DisclosureRepresentation::HtmlText),
        ("site.jsonld", build::DisclosureRepresentation::JsonString),
        ("search.json", build::DisclosureRepresentation::JsonString),
        ("llms.txt", build::DisclosureRepresentation::Text),
        ("sitemap.xml", build::DisclosureRepresentation::XmlText),
        ("feed.atom", build::DisclosureRepresentation::XmlText),
        ("image.svg", build::DisclosureRepresentation::XmlText),
        (
            "module.mjs",
            build::DisclosureRepresentation::JavascriptString,
        ),
        ("style.css", build::DisclosureRepresentation::CssString),
        ("module.js.map", build::DisclosureRepresentation::JsonString),
        (
            "PRIVATE_SECRET_619827.txt",
            build::DisclosureRepresentation::Path,
        ),
        ("pixel.png", build::DisclosureRepresentation::RasterMetadata),
        (
            "photo.jpeg",
            build::DisclosureRepresentation::RasterMetadata,
        ),
        (
            "nested.html",
            build::DisclosureRepresentation::PercentDecoded,
        ),
        ("nested.html", build::DisclosureRepresentation::DataPayload),
    ] {
        assert!(
            findings.iter().any(|finding| finding.path.as_str() == path
                && finding.rule.as_str() == "rule:secret"
                && finding.class == Some(build::DisclosureClass::Secret)
                && finding.representation == representation),
            "missing {path}/{representation:?}: {findings:?}"
        );
    }
    assert_eq!(fixture.output().inventory().identity(), identity);
    for (path, bytes) in selected {
        assert_eq!(
            fixture.output().file(&path.parse().expect("matrix path")),
            Some(bytes)
        );
        assert_eq!(
            fs::read(fixture.directory.path().join(path)).expect("unchanged matrix file"),
            bytes
        );
    }
}

#[test]
fn metadata_limits_and_repeated_images_consume_one_whole_output_budget() {
    let png = decode(CLEAN);
    let metadata = raster::metadata(Format::Png, &png, Default::default())
        .expect("native measured logical work");
    let one = RetainedOutput::new(&[("one.png", &png)]);
    let two = RetainedOutput::new(&[("one.png", &png), ("two.png", &png)]);
    let mut limits = build::DisclosureProbeLimits {
        encoded_bytes: metadata.decoded_bytes,
        ..Default::default()
    };
    build::check_disclosure_policy(one.output(), &one.media_types, &policy(), limits)
        .expect("one image fits its actual complete native accounting");
    let error = build::check_disclosure_policy(two.output(), &two.media_types, &policy(), limits)
        .expect_err("repeated image must not evade aggregate decompression accounting");
    assert!(matches!(
        error,
        build::DisclosureProbeError::Limit("raster metadata")
    ));
    limits = build::DisclosureProbeLimits {
        encoded_values: metadata.fields.len(),
        ..Default::default()
    };
    build::check_disclosure_policy(one.output(), &one.media_types, &policy(), limits)
        .expect("one metadata field set");
    assert!(matches!(
        build::check_disclosure_policy(two.output(), &two.media_types, &policy(), limits),
        Err(build::DisclosureProbeError::Limit("raster metadata"))
    ));
    for field in [
        "encoded_bytes",
        "chunk_bytes",
        "decoded_bytes",
        "values",
        "chunks",
    ] {
        let mut limits = build::DisclosureProbeLimits::default();
        match field {
            "encoded_bytes" => limits.raster.encoded_bytes = png.len() - 1,
            "chunk_bytes" => limits.raster.chunk_bytes = 1,
            "decoded_bytes" => limits.raster.decoded_bytes = 1,
            "values" => limits.raster.values = 0,
            "chunks" => limits.raster.chunks = 0,
            _ => unreachable!("closed native field cases"),
        }
        assert!(
            matches!(
                build::check_disclosure_policy(one.output(), &one.media_types, &policy(), limits),
                Err(build::DisclosureProbeError::Limit("raster metadata"))
            ),
            "{field}"
        );
    }
}
