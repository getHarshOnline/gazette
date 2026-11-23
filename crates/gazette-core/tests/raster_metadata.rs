//! Raster metadata report binding and resource-accounting checks, not native extraction proof.

use gazette_core::{
    raster::{Error, Format, Metadata, MetadataLimits, unavailable_metadata},
    source::Sha256Digest,
};

fn report() -> Metadata {
    Metadata {
        format: Format::Png,
        bytes: 4,
        sha256: Sha256Digest::of(b"data"),
        tool: "fixture.metadata/1",
        decoded_bytes: 7,
        fields: vec!["é".into(), "text".into()],
    }
}

#[test]
fn exact_byte_binding_and_profile_are_required_independently_of_fields() {
    let limits = MetadataLimits::default();
    report()
        .validate(Format::Png, b"data", limits)
        .expect("exact declared evidence binding");
    assert!(matches!(
        report().validate(Format::Jpeg, b"data", limits),
        Err(Error::Binding)
    ));
    assert!(matches!(
        report().validate(Format::Png, b"date", limits),
        Err(Error::Binding)
    ));
    let mut changed = report();
    changed.bytes = 3;
    assert!(matches!(
        changed.validate(Format::Png, b"data", limits),
        Err(Error::Binding)
    ));
    for tool in [
        "",
        "profile with spaces",
        "profile\nsecret",
        "profilé",
        concat!(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "a"
        ),
    ] {
        let mut changed = report();
        changed.tool = tool;
        assert!(matches!(
            changed.validate(Format::Png, b"data", limits),
            Err(Error::Binding)
        ));
    }
}

#[test]
fn field_and_decompression_accounting_share_exact_byte_and_value_bounds() {
    let exact = MetadataLimits {
        encoded_bytes: 4,
        decoded_bytes: 7,
        values: 2,
        ..Default::default()
    };
    report()
        .validate(Format::Png, b"data", exact)
        .expect("exact logical bounds include extra decompression byte");
    for limits in [
        MetadataLimits {
            encoded_bytes: 3,
            ..exact
        },
        MetadataLimits {
            decoded_bytes: 6,
            ..exact
        },
        MetadataLimits { values: 1, ..exact },
    ] {
        assert!(matches!(
            report().validate(Format::Png, b"data", limits),
            Err(Error::Limit(_))
        ));
    }
    let mut underreported = report();
    underreported.decoded_bytes = 5;
    assert!(matches!(
        underreported.validate(Format::Png, b"data", exact),
        Err(Error::Binding)
    ));
    let mut empty = report();
    empty.fields = vec![String::new(); 3];
    empty.decoded_bytes = 0;
    assert!(matches!(
        empty.validate(Format::Png, b"data", exact),
        Err(Error::Limit(_))
    ));
}

#[test]
fn absent_metadata_capability_never_substitutes_pixel_evidence() {
    assert!(matches!(
        unavailable_metadata(Format::Png, b"data", MetadataLimits::default()),
        Err(Error::Unsupported(_))
    ));
}
