//! Qualified WOFF2 behavior over one provenance-documented neutral conformance fixture.

use std::collections::{BTreeMap, BTreeSet};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use gazette_adapter_fonts::{Compiler, Inspector, PROFILE};
use gazette_core::{
    font,
    resource::{Error as ResourceError, Limits, Recipe},
    source::{Content, EntryPath, InventoryBuilder, Sha256Digest, TreeInput},
};
use gazette_ports::{font::FontInspector, resource::ResourceCompiler};

fn fixture() -> Vec<u8> {
    STANDARD
        .decode(
            include_str!("fixtures/valid-001.woff2.b64")
                .split_whitespace()
                .collect::<String>(),
        )
        .expect("pinned WPT WOFF2 fixture")
}

fn path(value: &str) -> EntryPath {
    value.parse().expect("logical path")
}

fn compile(
    files: &[(&str, Vec<u8>)],
    limits: Limits,
) -> Result<gazette_core::resource::Artifact, ResourceError> {
    let mut builder = InventoryBuilder::new(limits.inputs);
    let mut retained = BTreeMap::new();
    for (name, bytes) in files {
        let path = path(name);
        builder
            .insert(
                path.clone(),
                Content::File {
                    bytes: bytes.len() as u64,
                    sha256: Sha256Digest::of(bytes),
                },
            )
            .expect("bounded fixture");
        retained.insert(path, bytes.as_slice());
    }
    let inventory = builder.finish();
    let input = TreeInput::new(&inventory, retained).expect("checked fixture tree");
    Compiler.compile(
        &input,
        &Recipe::Woff2Font {
            entry: path("font.woff2"),
        },
        &Default::default(),
        limits,
    )
}

#[test]
fn complete_decode_binds_exact_retained_and_reconstructed_bytes() {
    let encoded = fixture();
    assert_eq!(encoded.len(), 980);
    assert_eq!(
        Sha256Digest::of(&encoded).to_string(),
        "92cd7b916e02cb77a9b21fce3b89f588ba060eda31784c693093438a1a9a8a69"
    );
    let decoded = Inspector
        .inspect(&encoded, font::Limits::default())
        .expect("qualified WOFF2 decode");
    decoded
        .validate(&encoded, font::Limits::default())
        .expect("evidence binds both representations");
    assert_eq!(decoded.inspection.encoded_bytes, 980);
    assert_eq!(decoded.inspection.tables, 9);
    assert!(decoded.inspection.glyphs > 0);
    assert_eq!(decoded.inspection.tool, PROFILE);
    assert_eq!(decoded.inspection.decoded_bytes, decoded.bytes.len());
    assert_eq!(
        decoded.inspection.decoded_sha256,
        Sha256Digest::of(&decoded.bytes)
    );
}

#[test]
fn compilation_is_deterministic_exact_byte_delivery_with_closed_inputs() {
    let encoded = fixture();
    let first = compile(&[("font.woff2", encoded.clone())], Limits::default())
        .expect("validated font resource");
    let second = compile(&[("font.woff2", encoded.clone())], Limits::default())
        .expect("repeat font resource");
    assert_eq!(first.bytes, encoded);
    assert_eq!(first.bytes, second.bytes);
    assert_eq!(first.input, second.input);
    assert_eq!(first.dependencies, BTreeSet::from([path("font.woff2")]));
    assert!(first.references.is_empty());
    assert!(first.image.is_none());
    assert_eq!(first.tool, PROFILE);

    assert!(matches!(
        compile(
            &[
                ("font.woff2", fixture()),
                ("ambient.bin", b"not admitted".to_vec())
            ],
            Limits::default()
        ),
        Err(ResourceError::Input { .. })
    ));
}

#[test]
fn malformed_truncated_signature_length_and_unsupported_flavors_fail_closed() {
    let encoded = fixture();
    let mut cases = Vec::new();
    cases.push(encoded[..47].to_vec());
    let mut signature = encoded.clone();
    signature[0..4].copy_from_slice(b"wOFF");
    cases.push(signature);
    let mut length = encoded.clone();
    length[8..12].copy_from_slice(&1_u32.to_be_bytes());
    cases.push(length);
    let mut reserved = encoded.clone();
    reserved[14] = 1;
    cases.push(reserved);
    let mut collection = encoded.clone();
    collection[4..8].copy_from_slice(b"ttcf");
    cases.push(collection);
    let mut unsupported = encoded.clone();
    unsupported[4..8].copy_from_slice(b"true");
    cases.push(unsupported);
    let mut compressed_size = encoded;
    compressed_size[20..24].copy_from_slice(&1_u32.to_be_bytes());
    cases.push(compressed_size);

    for malformed in cases {
        assert!(
            Inspector
                .inspect(&malformed, font::Limits::default())
                .is_err(),
            "malformed font was accepted"
        );
    }
}

#[test]
fn encoded_decoded_table_and_output_limits_are_enforced() {
    let encoded = fixture();
    for limits in [
        font::Limits {
            encoded_bytes: encoded.len() - 1,
            ..Default::default()
        },
        font::Limits {
            decoded_bytes: 1,
            ..Default::default()
        },
        font::Limits {
            tables: 8,
            ..Default::default()
        },
    ] {
        assert!(matches!(
            Inspector.inspect(&encoded, limits),
            Err(font::Error::Limit(_))
        ));
    }
    let limits = Limits {
        output_bytes: encoded.len() - 1,
        ..Default::default()
    };
    assert!(matches!(
        compile(&[("font.woff2", encoded)], limits),
        Err(ResourceError::Limit("WOFF2 output bytes"))
    ));
}
