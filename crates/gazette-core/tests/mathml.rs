//! The shared `MathML` invariant is exercised directly, without coupling it to
//! any particular XML or HTML parser.

use gazette_core::mathml::{Attribute, Error, Validator};

fn start<'a>(validator: &mut Validator, tag: &str, attributes: &'a [Attribute<'a>]) {
    validator.start(tag, attributes).expect(tag);
}

#[test]
fn exact_provider_binding_and_safe_inspection_share_one_structure() {
    let no_attributes = [];
    let annotation = [Attribute {
        name: "encoding",
        value: "application/x-tex",
    }];
    let display = [Attribute {
        name: "display",
        value: "block",
    }];

    for (expected_tex, expected_display) in [(Some("x^2"), Some(true)), (None, None)] {
        let mut validator = Validator::new(expected_tex, expected_display);
        start(&mut validator, "math", &display);
        start(&mut validator, "semantics", &no_attributes);
        start(&mut validator, "msup", &no_attributes);
        start(&mut validator, "mi", &no_attributes);
        validator.text("x").unwrap();
        validator.end("mi").unwrap();
        start(&mut validator, "mn", &no_attributes);
        validator.text("2").unwrap();
        validator.end("mn").unwrap();
        validator.end("msup").unwrap();
        start(&mut validator, "annotation", &annotation);
        validator.text("x^2").unwrap();
        validator.end("annotation").unwrap();
        validator.end("semantics").unwrap();
        validator.end("math").unwrap();
        assert!(validator.root_closed());
        validator.finish().unwrap();
    }
}

#[test]
fn structure_and_provider_request_mismatches_fail_closed() {
    let display = [Attribute {
        name: "display",
        value: "block",
    }];
    assert_eq!(
        Validator::new(None, Some(false)).start("math", &display),
        Err(Error::Invalid)
    );

    let mut validator = Validator::new(None, None);
    validator.start("math", &[]).unwrap();
    assert_eq!(
        validator.start(
            "mi",
            &[Attribute {
                name: "onclick",
                value: "run()",
            }],
        ),
        Err(Error::Invalid)
    );

    let mut validator = Validator::new(None, None);
    validator.start("math", &[]).unwrap();
    validator.start("semantics", &[]).unwrap();
    validator.start("mfrac", &[]).unwrap();
    validator.start("mi", &[]).unwrap();
    validator.text("x").unwrap();
    validator.end("mi").unwrap();
    assert_eq!(validator.end("mfrac"), Err(Error::Invalid));
}
