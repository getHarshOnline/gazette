use super::*;
use std::collections::BTreeMap;

const INLINE: &str = "[[images]]\ndefault='figure.svg'\ninline=true\nwidth=480\n";
const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 200 100" role="img" aria-labelledby="title desc"><title id="title">A &amp; B</title><desc id="desc">Two systems.</desc><g fill="var(--color-ink, #292722)" font-family="var(--type-family-reading, sans-serif)"><text x="10" y="20">A &#x26; B &lt;script&gt;</text><path d="M0 0L100 50" stroke="currentColor"/></g></svg>"##;

#[test]
fn inline_svg_keeps_occurrence_metadata_and_scopes_repeated_body_note_and_cell_ids() {
    let mut files = files(INLINE);
    files[1].1 = SVG.as_bytes().to_vec();
    let corpus = parse(&files);
    let compilation = compilation();
    let resolved = resolve(&compilation, &corpus);
    let page = resolved
        .project_page(
            &key("content:guide"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    assert_eq!(page.data.images.len(), 3);
    let note = page.data.notes.as_ref().unwrap();
    assert_eq!(
        note.images.len(),
        3,
        "the notes image and two notebook uses"
    );
    let notebook = notebook_html(note);
    let uses = [
        (&page.data.images[0], page.body_html.as_str()),
        (&page.data.images[1], page.body_html.as_str()),
        (&note.images[1], notebook.as_str()),
        (&note.images[2], notebook.as_str()),
    ];
    for (image, html) in uses {
        let address = image.address.as_ref().unwrap();
        assert!(html.contains(&format!("data-gazette-image-ref=\"{address}\"")));
        assert!(html.contains(&format!("<img hidden data-gazette-image=\"{address}\"")));
        let scoped = format!("gazette-svg-{}-title", Sha256Digest::of(address.as_bytes()));
        assert_eq!(html.matches(&format!("id=\"{scoped}\"")).count(), 1);
        assert!(html.contains(&format!("aria-labelledby=\"{scoped} ")));
        assert_eq!(image.destination, "https://example.test/guide/figure.svg");
    }
    assert!(
        page.body_html
            .contains("var(--type-family-reading, sans-serif)")
    );
    assert!(page.body_html.contains("A &amp; B &lt;script&gt;"));
    assert!(
        page.body_html
            .contains("alt=\"First\" title=\"First title\"")
    );
    assert!(!page.body_html.contains("<script>"));
    assert_eq!(page.data.images[0].alt, "First");
    assert!(note.body_html.contains("data-gazette-image-ref="));
    assert_ne!(note.images[0].address, page.data.images[0].address);
}

#[test]
fn inline_svg_refuses_active_foreign_ambiguous_and_unbounded_input() {
    for svg in [
        SVG.replace("<g fill=", "<g onclick='run()' fill="),
        SVG.replace("<g fill=", "<g ONLOAD='run()' fill="),
        SVG.replace("<g fill=", "<g style='fill:red' fill="),
        SVG.replace("<g fill=", "<g xmlns='http://www.w3.org/1999/xhtml' fill="),
        SVG.replace("<g fill=", "<g xmlns:x='http://www.w3.org/2000/svg' fill="),
        SVG.replace("<g fill=", "<g href='#title' fill="),
        SVG.replace("<g fill=", "<g xlink:href='#title' fill="),
        SVG.replace("<g fill=", "<g class='selected' fill="),
        SVG.replace("<g fill=", "<g fill='red' fill="),
        SVG.replace("currentColor", "url(https://example.test/paint.svg)"),
        SVG.replace("currentColor", "u&#114;l(#title)"),
        SVG.replace("currentColor", "var(--paint, url(#title))"),
        SVG.replace("<text ", "<TEXT ")
            .replace("</text>", "</TEXT>"),
        SVG.replace("<text ", "<foreignObject ")
            .replace("</text>", "</foreignObject>"),
        SVG.replace("id=\"desc\"", "id=\"title\""),
        SVG.replace("title desc\"", "title missing\""),
        SVG.replace("A &amp; B", "&unknown;"),
        SVG.replace("A &amp; B", "<text>Nested markup</text>"),
        SVG.replace("A &amp; B", "&#0;"),
        format!("<?xml version='1.0'?>{SVG}"),
        format!("<!DOCTYPE svg>{SVG}"),
        SVG.replace("A &amp; B", "<![CDATA[text]]>"),
        SVG.replace("<path ", "<svg "),
        SVG.replace("A &amp; B", &"x".repeat((1 << 20) + 1)),
    ] {
        let mut files = files(INLINE);
        files[1].1 = svg.into_bytes();
        let corpus = parse(&files);
        let compilation = compilation();
        assert!(authored::resolve(&compilation, &corpus, Default::default()).is_err());
    }
    for declaration in [
        format!("{INLINE}dark='dark.svg'\n"),
        "[[images]]\ndefault='plain.png'\ninline=true\n".into(),
    ] {
        let files = files(&declaration);
        let corpus = parse(&files);
        let compilation = compilation();
        assert!(authored::resolve(&compilation, &corpus, Default::default()).is_err());
    }
}

/// Joins every notebook cell's HTML in a page's Notes.
fn notebook_html(notes: &authored::ArticleNotes) -> String {
    let mut html = String::new();
    for notebook in &notes.notebooks {
        for cell in &notebook.cells {
            match cell {
                gazette_compiler::notebook::NotebookCell::Markdown { html: cell, .. } => {
                    html.push_str(cell);
                }
                gazette_compiler::notebook::NotebookCell::Code {
                    source_html,
                    outputs_html,
                    ..
                } => {
                    html.push_str(source_html);
                    html.push_str(outputs_html);
                }
            }
        }
    }
    html
}

fn files(declarations: &str) -> Vec<(String, Vec<u8>)> {
    vec![
        ("authored/one/guide/index.md".into(), format!("{}\n![First](./figure.svg \"First title\")\n\nSame caption.\n\n![Second](%66igure.svg)\n\n![](plain.png)\n", page_with("content:guide", "/guide/", declarations)).into_bytes()),
        ("authored/one/guide/figure.svg".into(), b"<svg xmlns='http://www.w3.org/2000/svg'/>".to_vec()),
        ("authored/one/guide/dark.svg".into(), b"<svg xmlns='http://www.w3.org/2000/svg'/>".to_vec()),
        ("authored/one/guide/plain.png".into(), b"retained raster".to_vec()),
        ("authored/one/guide/notes.md".into(), b"![Note](figure.svg)".to_vec()),
        ("authored/one/guide/calculations/plot.ipynb".into(), br#"{"nbformat":4,"nbformat_minor":5,"metadata":{},"cells":[{"cell_type":"markdown","id":"one","metadata":{},"source":"![Notebook](../figure.svg)"},{"cell_type":"markdown","id":"two","metadata":{},"source":"![Other cell](../figure.svg)"}]}"#.to_vec()),
    ]
}

#[test]
fn inline_svg_requires_a_resource_owner_and_whole_image_occurrence() {
    let mut undeclared = files(INLINE);
    undeclared.retain(|(name, _)| name.ends_with("index.md") || name.ends_with("figure.svg"));
    undeclared[0].0 = "authored/one/guide/page.md".into();
    let corpus = parse(&undeclared);
    let compilation = compilation();
    assert!(authored::resolve(&compilation, &corpus, Default::default()).is_err());

    for suffix in ["#view", "?mode=dark"] {
        let mut selected = files(INLINE);
        selected[0].1 = format!(
            "{}\n![Part](figure.svg{suffix})\n",
            page_with("content:guide", "/guide/", INLINE)
        )
        .into_bytes();
        let corpus = parse(&selected);
        let resolved = resolve(&compilation, &corpus);
        assert!(
            resolved
                .project_page(
                    &key("content:guide"),
                    &gazette_core::raster::unavailable,
                    &gazette_adapter_markdown::Comrak
                )
                .is_err()
        );
    }
}

const DECLARATION: &str =
    "[[images]]\ndefault='figure.svg'\nlight='./figure.svg'\ndark='dark.svg'\nwidth=480\n";

#[test]
fn explicit_svg_outputs_match_source_files_without_pairing_external_lookalikes() {
    let declarations = format!(
        "{DECLARATION}[[resources]]\ninputs=['guide/figure.svg']\nbuild={{kind='svg-image',entry='guide/figure.svg'}}\noutput='/diagrams/selected.svg'\n"
    );
    let mut files = files(&declarations);
    files.retain(|(name, _)| !name.ends_with(".ipynb") && !name.ends_with("notes.md"));
    files[0].1 = format!(
        "{}\n![View](figure.svg#view)\n\n![Remote](https://example.test/diagrams/selected.svg)\n",
        page_with("content:guide", "/guide/", &declarations)
    )
    .into_bytes();
    let corpus = parse(&files);
    let compilation = compilation();
    let resolved = resolve(&compilation, &corpus);
    let page = resolved
        .project_page(
            &key("content:guide"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    assert_eq!(
        page.data.images[0].destination,
        "https://example.test/diagrams/selected.svg#view"
    );
    assert_eq!(
        page.data.images[0]
            .variants
            .as_ref()
            .unwrap()
            .dark
            .as_deref(),
        Some("https://example.test/guide/dark.svg#view")
    );
    assert!(page.data.images[1].variants.is_none() && page.data.images[1].width.is_none());
}

#[test]
fn image_variants_keep_one_occurrence_and_resolve_body_notes_and_notebook_bases() {
    let files = files(DECLARATION);
    let corpus = parse(&files);
    let compilation = compilation_with_base("https://example.test/press/");
    for base in [
        "https://example.test/press/",
        "http://127.0.0.1:4198/press/",
    ] {
        let mut resolved = resolve(&compilation, &corpus);
        if base.starts_with("http:") {
            resolved = resolved
                .with_preview_urls(BTreeMap::from([(key("site:one"), base.parse().unwrap())]))
                .unwrap();
        }
        let page = resolved
            .project_page(
                &key("content:guide"),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .unwrap();
        let images = &page.data.images;
        assert_eq!(
            images
                .iter()
                .map(|image| image.alt.as_str())
                .collect::<Vec<_>>(),
            ["First", "Second", ""],
            "two repeated body uses and the plain image"
        );
        let notes = page.data.notes.as_ref().unwrap();
        // Notebook images join Notes after the notes.md image.
        assert_eq!(
            notes
                .images
                .iter()
                .map(|image| image.alt.as_str())
                .collect::<Vec<_>>(),
            ["Note", "Notebook", "Other cell"]
        );
        assert_eq!(images[0].title.as_deref(), Some("First title"));
        assert!(images[1].title.is_none());
        assert!(images[2].variants.is_none() && images[2].width.is_none());
        let notebook = notebook_html(notes);
        let uses = [
            (&images[0], page.body_html.as_str()),
            (&images[1], page.body_html.as_str()),
            (&notes.images[1], notebook.as_str()),
            (&notes.images[2], notebook.as_str()),
        ];
        for (image, html) in uses {
            assert_eq!(image.destination, format!("{base}guide/figure.svg"));
            let variants = image.variants.as_ref().unwrap();
            assert_eq!(variants.light.as_deref(), Some(image.destination.as_str()));
            assert_eq!(variants.dark, Some(format!("{base}guide/dark.svg")));
            assert_eq!(image.width.unwrap().get(), 480);
            let address = image.address.as_ref().unwrap();
            assert!(html.contains(&format!("data-gazette-image=\"{address}\"")));
        }
        assert_ne!(images[0].address, images[1].address);
        assert_ne!(images[0].address, notes.images[1].address);
        assert_ne!(notes.images[1].address, notes.images[2].address);
        assert!(
            page.body_html
                .contains(&format!("src=\"{base}guide/figure.svg\""))
        );
        // The dark rendition appears only as the source ahead of the default image.
        assert_eq!(
            page.body_html.matches("dark.svg").count(),
            page.body_html
                .matches(&format!(
                    "<source data-gazette-image-mode=\"dark\" media=\"(prefers-color-scheme: dark)\" srcset=\"{base}guide/dark.svg"
                ))
                .count()
        );
        assert!(
            page.body_html
                .contains("<picture data-gazette-image-modes>")
        );
        assert!(page.body_text.contains("Same caption."));
        let note = &page.data.notes.as_ref().unwrap().images[0];
        assert_eq!(note.variants, images[0].variants);
        assert_ne!(note.address, images[0].address);
        assert!(
            page.data
                .notes
                .as_ref()
                .unwrap()
                .body_html
                .contains(note.address.as_ref().unwrap())
        );
        let json = serde_json::to_value(&page.data).unwrap();
        assert!(json["images"][0]["variants"].get("default").is_none());
        assert_eq!(json["images"][0]["width"], 480);
        assert!(json["images"][2].get("variants").is_none());
    }
}

#[test]
fn image_declarations_reject_ambiguous_missing_unsafe_or_non_image_sources() {
    for declaration in [
        format!("{DECLARATION}[[images]]\ndefault='./figure.svg'\n"),
        "[[images]]\ndefault='figure.svg'\ndark='absent.svg'\n".into(),
        "[[images]]\ndefault='figure.svg'\ndark='https://remote.test/dark.svg'\n".into(),
        "[[images]]\ndefault='figure.svg'\ndark='../../outside.svg'\n".into(),
        "[[images]]\ndefault='figure.svg'\ndark='../../one-other/dark.png'\n".into(),
        "[[images]]\ndefault='figure.svg'\ndark='/guide/dark.svg'\n".into(),
        "[[images]]\ndefault='figure.svg'\ndark='dark.svg?mode=dark'\n".into(),
        "[[images]]\ndefault='figure.svg'\ndark='dark.svg#view'\n".into(),
        "[[images]]\ndefault='figure.svg'\ndark='%2e%2e/dark.svg'\n".into(),
        "[[images]]\ndefault='figure.svg'\ndark='notes.md'\n".into(),
        "[[images]]\ndefault='figure.svg'\nwidth=0\n".into(),
        "[[images]]\ndefault='figure.svg'\nwidth=-1\n".into(),
        "[[images]]\ndefault='figure.svg'\nwidth=1.5\n".into(),
    ] {
        let mut files = files(&declaration);
        files.push((
            "authored/one-other/dark.png".into(),
            b"retained in a different configured source".to_vec(),
        ));
        let corpus = parse(&files);
        let compilation = compilation();
        assert!(
            authored::resolve(&compilation, &corpus, Default::default()).is_err(),
            "{declaration}"
        );
    }
}

#[test]
fn unused_declarations_select_inputs_but_never_create_occurrences() {
    // A non-package owner proves selection is due to the declaration, not sibling discovery.
    let mut files = files("[[images]]\ndefault='plain.png'\nwidth=360\n");
    files.retain(|(name, _)| name.ends_with("index.md") || name.ends_with("plain.png"));
    files[0] = (
        "authored/one/guide/page.md".into(),
        page_with(
            "content:guide",
            "/guide/",
            "[[images]]\ndefault='plain.png'\nwidth=360\n",
        )
        .into_bytes(),
    );
    let corpus = parse(&files);
    let compilation = compilation();
    let resolved = resolve(&compilation, &corpus);
    let selected = resolved.non_draft_selection().unwrap();
    assert_eq!(
        selected.assets[&path("authored/one/guide/plain.png")],
        Disposition::Include
    );
    let page = resolved
        .project_page(
            &key("content:guide"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    assert!(page.data.images.is_empty());
    let mut excluded = selected;
    excluded
        .assets
        .insert(path("authored/one/guide/plain.png"), Disposition::Exclude);
    assert!(resolved.into_selected(&excluded).is_err());
}

#[test]
fn yaml_images_and_width_only_transparent_images_share_the_native_contract() {
    let mut files = files("");
    files[0] = ("authored/one/guide/index.landing.yaml".into(), b"format: gazette-landing/v1\nimages:\n- default: figure.svg\n  dark: dark.svg\n  width: 480\n- default: plain.png\n  width: 360\ncontent:\n  id: content:guide\n  site: site:one\n  title: Page\n  lifecycle: published\n  route: {path: /guide/}\n  structure: {kind: page, family: {kind: article}}\nsections:\n- body: |\n    ![Figure](figure.svg)\n\n    ![Raster](plain.png)\n".to_vec());
    let corpus = parse(&files);
    let compilation = compilation();
    let resolved = resolve(&compilation, &corpus);
    let page = resolved
        .project_page(
            &key("content:guide"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    assert!(
        page.data.images[0]
            .variants
            .as_ref()
            .unwrap()
            .dark
            .is_some()
    );
    assert!(page.data.images[1].variants.is_none());
    assert_eq!(page.data.images[1].width.unwrap().get(), 360);
    let schema = serde_json::to_string(&authored::declaration_schema()).unwrap();
    assert!(
        schema.contains("images")
            && schema.contains("ImageDeclaration")
            && schema.contains("width")
    );
}

#[test]
fn the_theme_starting_mode_sets_every_dark_source_media() {
    struct Starting(gazette_core::markdown::InitialColorMode);
    impl gazette_ports::markdown::ComponentRenderer for Starting {
        fn render(
            &self,
            _: &gazette_core::markdown::ComponentBinding,
            _: &gazette_core::markdown::Fragment,
            _: bool,
        ) -> Result<String, gazette_core::markdown::Error> {
            Err(gazette_core::markdown::Error::InvalidPlan)
        }
        fn initial_color_mode(&self) -> gazette_core::markdown::InitialColorMode {
            self.0
        }
    }
    let manifest = gazette_compiler::theme_package::ParsedManifest::parse(
        "package='theme:one'\nnamespace='one'\ntemplates='templates'\nrender={page='page.html'}\n",
        Default::default(),
    )
    .unwrap();
    let files = files(DECLARATION);
    let corpus = parse(&files);
    let compilation = compilation();
    let resolved = resolve(&compilation, &corpus);
    use gazette_core::markdown::InitialColorMode;
    for (mode, media) in [
        (InitialColorMode::System, "(prefers-color-scheme: dark)"),
        (InitialColorMode::Light, "not all"),
        (InitialColorMode::Dark, "all"),
    ] {
        let page = resolved
            .project_page_with(
                &key("content:guide"),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
                None,
                Some(gazette_compiler::markdown::ComponentPresentation {
                    definitions: manifest.components(),
                    renderer: &Starting(mode),
                }),
            )
            .unwrap();
        let sources = page.body_html.matches("<source ").count();
        assert!(sources > 0);
        assert_eq!(
            page.body_html
                .matches(&format!(
                    "data-gazette-image-mode=\"dark\" media=\"{media}\""
                ))
                .count(),
            sources,
            "{mode:?}"
        );
    }
}
