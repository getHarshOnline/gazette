//! Exact one-layer byte decoding, bounded independently of caller content scanning.

use gazette_compiler::encoded::{Decoded, Error, Kind, Limits, PROFILE, decode};

fn decoded(value: &[u8]) -> Decoded {
    decode(value, Limits::default())
        .expect("supported bounded encoding")
        .expect("actual transformation")
}

#[test]
fn percent_decoding_is_non_lossy_one_layer_and_not_form_decoding() {
    for (input, expected) in [
        (
            b"plain%20text+plus".as_slice(),
            b"plain text+plus".as_slice(),
        ),
        (b"%E2%82%AC", "€".as_bytes()),
        (b"%ff%00%80", b"\xff\0\x80"),
        (b"\xff%41", b"\xffA"),
        (b"bad%GG%2%41%", b"bad%GG%2A%"),
        (b"%2520", b"%20"),
        (b"%64ata%3A%2Cx", b"data:,x"),
    ] {
        assert_eq!(
            decoded(input),
            Decoded {
                bytes: expected.to_vec(),
                kind: Kind::Percent
            }
        );
    }
    for value in [
        b"".as_slice(),
        b"plain+text",
        b"%",
        b"%0",
        b"%GG",
        b"c2VjcmV0",
        b"prefix data:,x",
    ] {
        assert_eq!(
            decode(value, Limits::default()).expect("no transformation"),
            None
        );
    }
}

#[test]
fn data_url_structure_is_selected_before_decoding_with_native_fragment_and_query_handling() {
    for (input, expected) in [
        (
            b"data:,one%2Ctwo;three+four".as_slice(),
            b"one,two;three+four".as_slice(),
        ),
        (b" \r\nDaTa:,hello%20world#not-body\t", b"hello world"),
        (b"da\tta:,x?query#fragment", b"x?query"),
        (b"data:,x%23y", b"x#y"),
        (b"data:,%2520", b"%20"),
        (b"data:,%ff%00", b"\xff\0"),
        (b"data:,", b""),
    ] {
        assert_eq!(
            decoded(input),
            Decoded {
                bytes: expected.to_vec(),
                kind: Kind::Data {
                    media_type: "text/plain".to_owned(),
                    charset: Some("US-ASCII".to_owned())
                },
            }
        );
    }
    assert_eq!(
        decode(b"data:text/plain%2Cpayload", Limits::default()),
        Err(Error::Malformed)
    );
    assert_eq!(
        decode(b"data:raw without comma", Limits::default()),
        Err(Error::Malformed)
    );
    assert_eq!(
        decode(b"data:,\xff", Limits::default()),
        Err(Error::Malformed)
    );
}

#[test]
fn forgiving_base64_accepts_the_web_profile_not_only_canonical_padding() {
    for body in [
        "Zg==",
        "Zg",
        "Zh==",
        "Zh",
        "Z%67%3D%3D",
        "Z g%09=%0A=%0D%0C",
    ] {
        let value = format!("data:;base64,{body}");
        assert_eq!(decoded(value.as_bytes()).bytes, b"f", "{body}");
    }
    for (body, expected) in [
        ("Zm8=", b"fo".as_slice()),
        ("Zm9", b"fo"),
        ("Zm9v", b"foo"),
        ("", b""),
    ] {
        assert_eq!(
            decoded(format!("data:; BASE64,{body}").as_bytes()).bytes,
            expected
        );
    }
    assert_eq!(
        decoded(b"data:application/octet-stream;base64,/w==").bytes,
        [255]
    );
}

#[test]
fn invalid_base64_alphabet_padding_and_whitespace_are_explicit_failures() {
    for body in [
        "Z", "Zg=", "Zg===", "=Zg=", "Z=g=", "====", "Zg-_", "Zg%0B==", "Zg%FF==", "Zg%GG==",
    ] {
        assert_eq!(
            decode(format!("data:;base64,{body}").as_bytes(), Limits::default()),
            Err(Error::Malformed),
            "{body}"
        );
    }
}

#[test]
fn supported_metadata_preserves_declarations_without_charset_conversion() {
    for (value, media_type, charset) in [
        ("data:Text/PLAIN,hello", "text/plain", None),
        ("data:;charset=UTF-8,%C3%A9", "text/plain", Some("UTF-8")),
        (
            "data:text/plain;charset=\"iso-8859-1\",%E9",
            "text/plain",
            Some("iso-8859-1"),
        ),
        (
            "data:application/ld+json;base64,e30=",
            "application/ld+json",
            None,
        ),
        ("data: text/plain ;base64,Zg==", "text/plain", None),
    ] {
        let result = decoded(value.as_bytes());
        assert_eq!(
            result.kind,
            Kind::Data {
                media_type: media_type.to_owned(),
                charset: charset.map(str::to_owned)
            }
        );
    }
    assert_eq!(
        decoded(b"data:text/plain;charset=iso-8859-1,%E9").bytes,
        [0xe9]
    );
    assert_eq!(
        decoded(b"data:text/plain%3Bbase64,Zm9v"),
        Decoded {
            bytes: b"Zm9v".to_vec(),
            kind: Kind::Data {
                media_type: "text/plain%3bbase64".to_owned(),
                charset: None
            },
        },
        "an escaped semicolon is part of the MIME token, never a Base64 flag"
    );
}

#[test]
fn unsupported_metadata_never_silently_changes_the_decode_profile() {
    for value in [
        "data:text/plain;name=file.txt,x",
        "data:text/plain;charset=utf-8;charset=ascii,x",
        "data:text/plain;charset=\"utf\\-8\",x",
        "data:text/plain;charset=,x",
        "data:text/plain;BASE64=yes,Zm9v",
        "data:text/plain;%62ase64,Zm9v",
        "data:text%2Fplain,x",
        "data:not-a-mime,x",
        "data:text/plain;unknown;base64,Zm9v",
    ] {
        assert_eq!(
            decode(value.as_bytes(), Limits::default()),
            Err(Error::Unsupported),
            "{value}"
        );
    }
}

#[test]
fn exact_decoded_bounds_do_not_charge_base64_padding_or_percent_expansion_as_output() {
    for value in [
        b"%41".as_slice(),
        b"data:,%41",
        b"data:;base64,YQ==",
        b"data:;base64,YQ",
        b"data:;base64,Y Q%3D%3D",
    ] {
        let expected = if value.starts_with(b"data:;base64") {
            b"a"
        } else {
            b"A"
        };
        assert_eq!(
            decode(
                value,
                Limits {
                    input_bytes: value.len(),
                    decoded_bytes: 1
                }
            )
            .expect("exact output bound")
            .expect("decoded")
            .bytes,
            expected
        );
        assert_eq!(
            decode(
                value,
                Limits {
                    input_bytes: value.len(),
                    decoded_bytes: 0
                }
            ),
            Err(Error::Limit("decoded bytes"))
        );
        assert_eq!(
            decode(
                value,
                Limits {
                    input_bytes: value.len() - 1,
                    decoded_bytes: 100
                }
            ),
            Err(Error::Limit("input bytes"))
        );
    }
    assert_eq!(
        decode(
            b"data:,",
            Limits {
                input_bytes: 6,
                decoded_bytes: 0
            }
        )
        .expect("empty body fits")
        .expect("data URL")
        .bytes,
        b""
    );
    assert_eq!(
        decode(
            b"plain",
            Limits {
                input_bytes: 5,
                decoded_bytes: 0
            }
        )
        .expect("untransformed input needs no output"),
        None
    );
}

#[test]
fn failure_diagnostics_are_redacted_and_profile_is_explicit() {
    assert_eq!(PROFILE, "gazette-encoded/1");
    for (input, limits) in [
        (b"data:;base64,secret!".as_slice(), Limits::default()),
        (b"data:secret,x", Limits::default()),
        (
            b"%73ecret",
            Limits {
                decoded_bytes: 0,
                ..Limits::default()
            },
        ),
    ] {
        let error = decode(input, limits).expect_err("rejected sensitive input");
        assert!(!error.to_string().contains("secret"));
        assert!(!format!("{error:?}").contains("secret"));
    }
}
