use super::*;
use gazette_core::markdown::{DarkImageSource, ImageCandidate, ImageSources, InitialColorMode};

fn candidate(url: &str, width: u32) -> ImageCandidate {
    ImageCandidate {
        url: url.into(),
        width: width.try_into().unwrap(),
    }
}

fn dark(mode: InitialColorMode, url: &str, candidates: Vec<ImageCandidate>) -> DarkImageSource {
    DarkImageSource {
        mode,
        url: url.into(),
        candidates,
    }
}

#[test]
fn images_without_a_dark_alternative_keep_their_unwrapped_markup() {
    let document = parse("![Figure](full.webp)\n");
    let mut selected = plan(document.syntax());
    selected.links[0].image_address = Some("image:one".into());
    assert_eq!(
        document.render(&selected, None).unwrap().body.html,
        "<div data-gazette-image-block>\n<p><img data-gazette-image=\"image:one\" src=\"full.webp\" alt=\"Figure\" /></p>\n</div>\n"
    );
    selected.links[0].image_sources = Some(ImageSources {
        sizes: "100vw".into(),
        candidates: vec![candidate("/small.webp", 320), candidate("/full.webp", 640)],
        dark: None,
    });
    assert_eq!(
        document.render(&selected, None).unwrap().body.html,
        "<div data-gazette-image-block>\n<p><img data-gazette-image=\"image:one\" src=\"full.webp\" srcset=\"/small.webp 320w, /full.webp 640w\" sizes=\"100vw\" alt=\"Figure\" /></p>\n</div>\n"
    );
}

#[test]
fn a_single_dark_rendition_precedes_the_default_image_for_each_starting_mode() {
    let document = parse("![Figure](full.webp \"Title\")\n");
    let mut selected = plan(document.syntax());
    selected.links[0].image_address = Some("image:one".into());
    for (mode, media) in [
        (InitialColorMode::System, "(prefers-color-scheme: dark)"),
        (InitialColorMode::Light, "not all"),
        (InitialColorMode::Dark, "all"),
    ] {
        selected.links[0].image_sources = Some(ImageSources {
            sizes: String::new(),
            candidates: Vec::new(),
            dark: Some(dark(mode, "/dark.webp?x=1&y=\"2\"", Vec::new())),
        });
        let html = document.render(&selected, None).unwrap().body.html;
        assert_eq!(
            html,
            format!(
                "<div data-gazette-image-block>\n<p><picture data-gazette-image-modes><source data-gazette-image-mode=\"dark\" media=\"{media}\" srcset=\"/dark.webp?x=1&amp;y=&quot;2&quot;\"><img data-gazette-image=\"image:one\" src=\"full.webp\" alt=\"Figure\" title=\"Title\" /></picture></p>\n</div>\n"
            )
        );
    }
    assert_eq!(
        document.render_markdown(&selected, None).unwrap().trim(),
        "![Figure](full.webp \"Title\")"
    );
}

#[test]
fn responsive_dark_candidates_share_the_declared_sizes() {
    let document = parse("Text ![Figure](full.webp) after\n");
    let mut selected = plan(document.syntax());
    selected.links[0].image_address = Some("image:one".into());
    selected.links[0].image_sources = Some(ImageSources {
        sizes: "(max-width: 40rem) 100vw, 50vw".into(),
        candidates: vec![candidate("/small.webp", 320), candidate("/full.webp", 640)],
        dark: Some(dark(
            InitialColorMode::System,
            "/full-dark.webp",
            vec![
                candidate("/small-dark.webp", 320),
                candidate("/full-dark.webp", 640),
            ],
        )),
    });
    let html = document.render(&selected, None).unwrap().body.html;
    assert_eq!(
        html,
        "<p>Text <picture data-gazette-image-modes><source data-gazette-image-mode=\"dark\" media=\"(prefers-color-scheme: dark)\" srcset=\"/small-dark.webp 320w, /full-dark.webp 640w\" sizes=\"(max-width: 40rem) 100vw, 50vw\"><img data-gazette-image=\"image:one\" src=\"full.webp\" srcset=\"/small.webp 320w, /full.webp 640w\" sizes=\"(max-width: 40rem) 100vw, 50vw\" alt=\"Figure\" /></picture> after</p>\n"
    );
}

#[test]
fn dark_sources_refuse_ambiguous_or_incomplete_selections() {
    let document = parse("![Figure](full.webp)\n");
    let mut selected = plan(document.syntax());
    selected.links[0].image_address = Some("image:one".into());
    let mut refuse = |sources: ImageSources| {
        selected.links[0].image_sources = Some(sources);
        assert_eq!(document.render(&selected, None), Err(Error::InvalidPlan));
    };
    // Neither responsive candidates nor a dark source.
    refuse(ImageSources {
        sizes: String::new(),
        candidates: Vec::new(),
        dark: None,
    });
    // Layout sizes without candidates to apply them to.
    refuse(ImageSources {
        sizes: "100vw".into(),
        candidates: Vec::new(),
        dark: Some(dark(InitialColorMode::System, "/dark.webp", Vec::new())),
    });
    // Dark candidates require the image's own responsive layout.
    refuse(ImageSources {
        sizes: String::new(),
        candidates: Vec::new(),
        dark: Some(dark(
            InitialColorMode::System,
            "/dark.webp",
            vec![candidate("/dark.webp", 640)],
        )),
    });
    for url in ["", "/dark one.webp", "/dark.webp,", "javascript:alert(1)"] {
        refuse(ImageSources {
            sizes: String::new(),
            candidates: Vec::new(),
            dark: Some(dark(InitialColorMode::System, url, Vec::new())),
        });
    }
    refuse(ImageSources {
        sizes: "100vw".into(),
        candidates: vec![candidate("/full.webp", 640)],
        dark: Some(dark(
            InitialColorMode::System,
            "/full-dark.webp",
            vec![
                candidate("/full-dark.webp", 640),
                candidate("/small-dark.webp", 320),
            ],
        )),
    });
}
