use super::*;
use gazette_core::markdown::{ImageCandidate, ImageSources};

#[test]
fn responsive_attributes_escape_urls_and_keep_the_default_markdown_reference() {
    let document = parse("![Figure](full.webp)\n");
    let mut selected = plan(document.syntax());
    selected.links[0].image_address = Some("image:one".into());
    selected.links[0].image_sources = Some(ImageSources {
        sizes: "(max-width: 40rem) 100vw, 50vw".into(),
        candidates: vec![
            ImageCandidate {
                url: "/small.webp?x=1&y=2".into(),
                width: 320.try_into().unwrap(),
            },
            ImageCandidate {
                url: "/full.webp".into(),
                width: 640.try_into().unwrap(),
            },
        ],
        dark: None,
    });
    let rendered = document.render(&selected, None).unwrap();
    assert!(rendered.body.html.contains("src=\"full.webp\""));
    assert!(
        rendered
            .body
            .html
            .contains("srcset=\"/small.webp?x=1&amp;y=2 320w, /full.webp 640w\"")
    );
    assert!(
        rendered
            .body
            .html
            .contains("sizes=\"(max-width: 40rem) 100vw, 50vw\"")
    );
    assert_eq!(
        document.render_markdown(&selected, None).unwrap().trim(),
        "![Figure](full.webp)"
    );
    for invalid in ["", "100vw; color:red", "100vw\n", "<script>"] {
        selected.links[0].image_sources.as_mut().unwrap().sizes = invalid.into();
        assert_eq!(document.render(&selected, None), Err(Error::InvalidPlan));
    }
    selected.links[0].image_sources.as_mut().unwrap().sizes = "100vw".into();
    selected.links[0]
        .image_sources
        .as_mut()
        .unwrap()
        .candidates
        .reverse();
    assert_eq!(document.render(&selected, None), Err(Error::InvalidPlan));
    selected.links[0]
        .image_sources
        .as_mut()
        .unwrap()
        .candidates
        .reverse();
    selected.links[0].image_sources.as_mut().unwrap().candidates[0].url =
        "/ambiguous,name.webp".into();
    assert_eq!(document.render(&selected, None), Err(Error::InvalidPlan));
}
