//! Inline drawings keep image bindings, local IDs and output budgets intact.
use super::*;
use gazette_core::markdown::{InlineSvg, SvgEvent};

fn drawing() -> InlineSvg {
    InlineSvg::new(vec![
        SvgEvent::Start {
            name: "svg".into(),
            attributes: vec![
                ("xmlns".into(), "http://www.w3.org/2000/svg".into()),
                ("id".into(), "drawing".into()),
            ],
        },
        SvgEvent::End("svg".into()),
    ])
    .unwrap()
}

#[test]
fn inline_svg_requires_image_identity_and_refuses_host_id_collisions() {
    let document = parse("# Title\n\n![Alt](figure.svg)\n");
    let mut selected = plan(document.syntax());
    selected.links[0].inline_svg = Some(drawing());
    assert_eq!(document.render(&selected, None), Err(Error::InvalidPlan));
    selected.links[0].image_address = Some("image:one".into());
    let rendered = document.render(&selected, None).unwrap();
    assert!(rendered.body.html.contains("</svg></span><img hidden "));
    let id = drawing().scoped_ids("image:one").next().unwrap();
    selected.headings[0].id = id;
    assert_eq!(document.render(&selected, None), Err(Error::InvalidPlan));
}

#[test]
fn inline_svg_empty_alt_is_decorative_and_expanded_output_is_bounded() {
    let source = "![](figure.svg)\n";
    let document = parse(source);
    let mut selected = plan(document.syntax());
    selected.links[0].inline_svg = Some(drawing());
    selected.links[0].image_address = Some("image:one".into());
    let rendered = document.render(&selected, None).unwrap();
    assert!(rendered.body.html.contains("aria-hidden=\"true\""));
    assert!(!rendered.body.html.contains("role=\"img\""));
    assert_eq!(
        document.render_markdown(&selected, None).unwrap().trim(),
        "![](figure.svg)"
    );
    let bounded = Comrak
        .parse(
            source,
            Profile::Publication,
            Limits {
                output_bytes: rendered.body.html.len() - 1,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(matches!(
        bounded.render(&selected, None),
        Err(Error::Limit(_))
    ));
}
