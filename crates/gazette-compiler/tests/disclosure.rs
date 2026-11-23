//! Native lexical matching and strict declaration shape, separate from disclosure approval.

use gazette_compiler::disclosure::{Pattern, contains_literal};

fn checked(pattern: Pattern, yes: &[&str], no: &[&str]) {
    pattern.validate().expect("valid selected pattern");
    for text in yes {
        assert!(
            pattern.matches(text.as_bytes()),
            "missing match in {text:?}"
        );
    }
    for text in no {
        assert!(!pattern.matches(text.as_bytes()), "false match in {text:?}");
    }
}

fn network(address: &str, prefix: u8) -> Pattern {
    Pattern::IpNetwork {
        address: address.parse().expect("native fixture IP address"),
        prefix,
    }
}

#[test]
fn literals_are_exact_bytes_without_case_or_unicode_normalization() {
    let pattern = Pattern::Literal {
        value: "café".into(),
    };
    checked(
        pattern.clone(),
        &["prefix café suffix"],
        &["CAFÉ", "cafe\u{301}"],
    );
    assert!(pattern.matches(b"\xffcaf\xc3\xa9\x00"));
    assert!(!contains_literal(b"anything", ""));
    assert!(!contains_literal(b"", "anything"));
    let error = Pattern::Literal {
        value: String::new(),
    }
    .validate()
    .expect_err("empty literal refused");
    assert_eq!(error.to_string(), "invalid disclosure pattern");
}

#[test]
fn path_trees_keep_exact_spelling_and_component_boundaries() {
    checked(
        Pattern::PathTree {
            root: "/private/native".into(),
        },
        &[
            "/private/native",
            "root=/private/native/file",
            "file:///private/native/file",
            "'/private/native'",
            "/private/native/../other",
        ],
        &[
            "/private/native-other",
            "/private/native=other",
            "/a/private/native",
            "/PRIVATE/native",
            "/private%2Fnative",
        ],
    );
    checked(
        Pattern::PathTree {
            root: r"C:\Private\Native".into(),
        },
        &[
            r"C:\Private\Native",
            r"C:\Private\Native\file",
            r"C:\Private\Native/file",
        ],
        &[
            r"c:\Private\Native",
            "C:/Private/Native",
            r"C:\Private\NativeExtra",
        ],
    );
    checked(
        Pattern::PathTree {
            root: r"\\server\share".into(),
        },
        &[r"\\server\share\file", r"\\server\share"],
        &[r"\\server\share-other", "//server/share/file"],
    );
    checked(
        Pattern::PathTree { root: "/".into() },
        &["/", "/file"],
        &["relative"],
    );
    checked(
        Pattern::PathTree { root: "C:/".into() },
        &["C:/file"],
        &["D:/file"],
    );
    for root in [
        "",
        "relative",
        "C:relative",
        "//server",
        r"\\server",
        "/a/../b",
        "/a/./b",
        "/a//b",
        "/a/",
        "/a\nsecret",
    ] {
        assert!(
            Pattern::PathTree { root: root.into() }.validate().is_err(),
            "accepted {root:?}"
        );
    }
}

#[test]
fn hostname_tokens_are_exact_ascii_labels_not_subdomain_wildcards() {
    checked(
        Pattern::Host {
            name: "internal.example".into(),
        },
        &[
            "internal.example",
            "https://INTERNAL.Example:8080/path",
            "name@internal.example",
            "internal.example.",
        ],
        &[
            "sub.internal.example",
            "internal.example.evil",
            "prefixinternal.example",
            "internal.example_suffix",
            "éinternal.example",
            "internal.exampleé",
            "internal.example..",
        ],
    );
    let longest = [
        "a".repeat(63),
        "b".repeat(63),
        "c".repeat(63),
        "d".repeat(61),
    ]
    .join(".");
    assert_eq!(longest.len(), 253);
    Pattern::Host {
        name: longest.clone(),
    }
    .validate()
    .expect("maximum DNS textual hostname");
    for name in [
        String::new(),
        "*.example".into(),
        "example.".into(),
        "a..b".into(),
        "-a.example".into(),
        "a-.example".into(),
        "a_b.example".into(),
        "é.example".into(),
        "example:80".into(),
        "a".repeat(64),
        format!("{longest}a"),
    ] {
        assert!(Pattern::Host { name }.validate().is_err());
    }
}

#[test]
fn ipv4_uses_native_address_and_socket_syntax_with_network_membership() {
    checked(
        network("10.0.0.0", 8),
        &[
            "10.2.3.4",
            "http://10.2.3.4:8080/path",
            "[10.2.3.4]",
            "address=10.255.255.255",
        ],
        &[
            "11.2.3.4",
            "110.2.3.4",
            "10.02.3.4",
            "10.2.3.4:99999",
            "x10.2.3.4",
            "10.2.3.4.example",
            "10.2.3.4_suffix",
            "::ffff:10.2.3.4",
            "10.2.3.4%zone",
        ],
    );
    checked(network("10.2.3.4", 32), &["10.2.3.4"], &["10.2.3.5"]);
    checked(network("0.0.0.0", 0), &["255.255.255.255"], &["::1"]);
    assert!(network("10.2.3.4", 8).validate().is_err());
    assert!(network("10.0.0.0", 33).validate().is_err());
    assert!(network("1.0.0.0", 0).validate().is_err());
}

#[test]
fn ipv6_and_mapped_addresses_do_not_silently_change_family() {
    checked(
        network("fd00::", 8),
        &[
            "fd01::1",
            "FD00:0000:0000:0000:0000:0000:0000:0001",
            "https://[fd00::1]:8443/path",
        ],
        &[
            "fe00::1",
            "fd00::1%eth0",
            "fd00::1%25eth0",
            "prefixfd00::1",
            "10.2.3.4",
        ],
    );
    checked(
        network("::ffff:10.0.0.0", 104),
        &["::ffff:10.2.3.4", "[::ffff:a02:304]:80"],
        &["10.2.3.4", "::ffff:11.2.3.4", "::a02:304"],
    );
    checked(network("::", 0), &["::1", "::ffff:10.2.3.4"], &["10.2.3.4"]);
    checked(network("fd00::1", 128), &["fd00::1"], &["fd00::2"]);
    assert!(network("fd00::1", 8).validate().is_err());
    assert!(network("::", 129).validate().is_err());
    assert!(network("::1", 0).validate().is_err());
}

#[test]
fn oversized_or_malformed_tokens_do_not_degrade_to_address_substrings() {
    let pattern = network("10.0.0.0", 8);
    pattern.validate().expect("network valid");
    for text in [
        format!("{}10.2.3.4", "0".repeat(4096)),
        format!("10.2.3.4{}", "a".repeat(4096)),
        "10.2.3.4:80:90".into(),
    ] {
        assert!(!pattern.matches(text.as_bytes()));
    }
    assert!(pattern.matches(format!("{} 10.2.3.4", "0".repeat(4096)).as_bytes()));
    assert!(pattern.work_factor() >= 2 * 53);
    let literal = Pattern::Literal {
        value: "long selected literal".into(),
    };
    assert!(literal.work_factor() >= "long selected literal".len());
    let path = Pattern::PathTree {
        root: "/long/selected/path".into(),
    };
    assert!(path.work_factor() >= "/long/selected/path".len());
}

#[test]
fn serde_is_map_only_exact_fields_and_native_typed_values() {
    for pattern in [
        Pattern::Literal {
            value: "private".into(),
        },
        Pattern::PathTree {
            root: "/private".into(),
        },
        Pattern::Host {
            name: "private.example".into(),
        },
        network("10.0.0.0", 8),
        network("fd00::", 8),
    ] {
        let encoded = serde_json::to_string(&pattern).expect("serialize typed pattern");
        let decoded: Pattern = serde_json::from_str(&encoded).expect("decode same pattern");
        assert_eq!(decoded, pattern);
    }
    for input in [
        r#"["literal","private"]"#,
        "null",
        "true",
        r#"{}"#,
        r#"{"kind":"literal","value":"a","value":"b"}"#,
        r#"{"kind":"literal","value":"a","\u0076alue":"b"}"#,
        r#"{"kind":"literal","value":"a","root":"/a"}"#,
        r#"{"kind":"literal","value":"a","extra":true}"#,
        r#"{"kind":"literal","value":null}"#,
        r#"{"kind":"host"}"#,
        r#"{"kind":"ip-network","address":[10,0,0,0],"prefix":8}"#,
        r#"{"kind":"ip-network","address":"garbage","prefix":8}"#,
        r#"{"kind":"ip-network","address":"10.0.0.0","prefix":"8"}"#,
        r#"{"kind":"ip-network","address":"10.0.0.0","prefix":8.0}"#,
        r#"{"kind":"ip-network","address":"10.0.0.0","prefix":-1}"#,
        r#"{"kind":"ip-network","address":"10.0.0.0","prefix":8,"prefix":16}"#,
    ] {
        assert!(
            serde_json::from_str::<Pattern>(input).is_err(),
            "accepted {input}"
        );
    }
    for input in [
        r#"{"kind":"path-tree","root":"relative"}"#,
        r#"{"kind":"ip-network","address":"10.1.0.0","prefix":8}"#,
    ] {
        let pattern: Pattern =
            serde_json::from_str(input).expect("shape distinct from semantic validation");
        assert!(pattern.validate().is_err());
    }
}
