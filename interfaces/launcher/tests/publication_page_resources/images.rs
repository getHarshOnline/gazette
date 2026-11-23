//! Configured image transformations reach the same native publication and retained output.

use super::*;
use base64::Engine as _;
use gazette_sdk::raster::{self, Format};

#[test]
fn responsive_images_use_verified_widths_mode_bindings_and_serving_bases() {
    // A synthetic 7×5 raster makes proportional rounding observable in the encoded output.
    let original = base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAcAAAAFCAYAAACJmvbYAAAAEklEQVR4nGNw6jjzHxdmGABJAG6HWmDsGfmpAAAAAElFTkSuQmCC").unwrap();
    let mut declarations = String::from(RESOURCES);
    for mode in ["light", "dark"] {
        for (name, role, resize) in [
            ("full", "rendition", "operation='fit-width',width=14"),
            ("narrow", "responsive", "operation='fit-width',width=3"),
            ("bound", "responsive", "operation='fit',width=20,height=3"),
        ] {
            declarations.push_str(&format!("[[resources]]\ninputs=['{mode}.png']\n{role}=true\noutput='/images/{mode}-{name}.webp'\nbuild={{kind='raster-image',entry='{mode}.png',input_format='png',apply_orientation=false,resize={{{resize}}},encoding={{format='webp'}}}}\n"));
        }
    }
    declarations.push_str("[[images]]\ndefault='light.png'\nlight='light.png'\ndark='dark.png'\nsizes='(max-width: 40rem) 100vw, 50vw'\nloading='eager'\nfetch_priority='high'\n");
    let page = format!(
        "{}\n![Figure](light.png?view=1)\n",
        page("one", "/one/", &declarations)
    );
    let fixture = Fixture::new(
        false,
        &[
            ("content/one.md", page.as_bytes()),
            ("content/light.png", &original),
            ("content/dark.png", &original),
        ],
    );
    for base in ["https://one.example/mount/", "http://127.0.0.1:12345/"] {
        let config = fixture
            .configuration()
            .replace("https://one.example/mount/", base)
            .replace(
                "target='authored'",
                "target='authored',static_assets={route_prefix='/'}",
            );
        let inputs = acquire(&config);
        let publication = render(&inputs).expect("native responsive image compilation");
        let output = publication.site().output();
        let model: Value = serde_json::from_slice(bytes(output, "model.json")).unwrap();
        let image = &model["pages"]["content:one"]["images"][0];
        assert_eq!(
            image["destination"],
            format!("{base}images/light-full.webp?view=1")
        );
        assert_eq!(image["intrinsic"], json!({"width":14,"height":10}));
        for mode in ["default", "light", "dark"] {
            let file_mode = if mode == "default" { "light" } else { mode };
            assert_eq!(
                image["responsive"][mode],
                json!([
                    {"url":format!("{base}images/{file_mode}-narrow.webp?view=1"),"width":3},
                    {"url":format!("{base}images/{file_mode}-bound.webp?view=1"),"width":4},
                    {"url":format!("{base}images/{file_mode}-full.webp?view=1"),"width":14},
                ])
            );
        }
        let html = std::str::from_utf8(bytes(output, "one/index.html")).unwrap();
        assert!(html.contains(&format!("srcset=\"{base}images/light-narrow.webp?view=1 3w, {base}images/light-bound.webp?view=1 4w, {base}images/light-full.webp?view=1 14w\"")), "{html}");
        assert!(html.contains("sizes=\"(max-width: 40rem) 100vw, 50vw\""));
        assert!(html.contains("width=\"14\" height=\"10\""));
        let resources = model["pages"]["content:one"]["presentation"]["resources"]
            .as_array()
            .unwrap();
        for mode in ["light", "dark"] {
            let bound = resources
                .iter()
                .find(|resource| resource["url"] == format!("{base}images/{mode}-bound.webp"))
                .unwrap();
            assert_eq!(bound["image"]["output"], json!({"width":4,"height":3}));
            assert_eq!(bound["responsive_of"], format!("{base}{mode}.png"));
            assert!(bound.get("rendition_of").is_none());
            assert!(bytes(output, &format!("images/{mode}-bound.webp")).starts_with(b"RIFF"));
        }
        let parent = tempdir().unwrap();
        let result = cli(&config, parent.path());
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let receipt: Value = serde_json::from_slice(&result.stdout).unwrap();
        let directory = Path::new(receipt["directory"].as_str().unwrap());
        assert_eq!(
            fs::read(directory.join("one/index.html")).unwrap(),
            bytes(output, "one/index.html")
        );
    }
}

#[test]
fn bound_webp_rendition_reaches_native_html_geometry_hints_and_resource_mapping() {
    for (width, html_width, html_height) in [(Some(360), 360, 180), (None, 2, 1)] {
        let declarations = "[[resources]]\ninputs=['pixel.png']\nrendition=true\noutput='/images/display.webp'\nbuild={kind='raster-image',entry='pixel.png',input_format='png',apply_orientation=false,resize={operation='scale',width=2,height=1},encoding={format='webp'}}\n[[images]]\ndefault='pixel.png'\nlight='pixel.png'\nwidth=360\nloading='eager'\nfetch_priority='high'\n";
        let declarations = declarations.replace(
            "width=360\n",
            &width.map_or_else(String::new, |value| format!("width={value}\n")),
        );
        let text = format!(
            "{}\n![Figure](pixel.png?view=1)\n",
            page("one", "/one/", &format!("{RESOURCES}{declarations}"))
        );
        let other = format!(
            "{}\n![Original](pixel.png)\n",
            page(
                "two",
                "/two/",
                "[[images]]\ndefault='pixel.png'\nwidth=360\n"
            )
        );
        let original = png();
        let fixture = Fixture::new(
            false,
            &[
                ("content/one.md", text.as_bytes()),
                ("content/two.md", other.as_bytes()),
                ("content/pixel.png", &original),
            ],
        );
        let config = fixture.configuration().replace(
            "target='authored'",
            "target='authored',static_assets={route_prefix='/'}",
        );
        let inputs = acquire(&config);
        let publication = render(&inputs).unwrap();
        let output = publication.site().output();
        let model: Value = serde_json::from_slice(bytes(output, "model.json")).unwrap();
        let image = &model["pages"]["content:one"]["images"][0];
        assert_eq!(
            image["destination"],
            "https://one.example/mount/images/display.webp?view=1"
        );
        assert_eq!(image["intrinsic"], json!({"width":2,"height":1}));
        assert_eq!(image["width"], json!(width));
        assert_eq!(image["loading"], "eager");
        assert_eq!(image["fetch_priority"], "high");
        let html = std::str::from_utf8(bytes(output, "one/index.html")).unwrap();
        assert!(html.contains(&format!("width=\"{html_width}\"")), "{html}");
        assert!(
            html.contains(&format!("height=\"{html_height}\"")),
            "{html}"
        );
        assert_eq!(html.contains("data-gazette-image-sized"), width.is_some());
        for attribute in ["loading=\"eager\"", "fetchpriority=\"high\""] {
            assert!(html.contains(attribute), "{html}");
        }
        let second = std::str::from_utf8(bytes(output, "two/index.html")).unwrap();
        assert!(second.contains("width=\"360\""));
        assert!(second.contains("height=\"360\""));
        assert!(second.contains("data-gazette-image-sized"));
        assert!(!second.contains("loading="));
        assert!(!second.contains("display.webp"));
        assert_eq!(bytes(output, "pixel.png"), original);
        let resource = model["pages"]["content:one"]["presentation"]["resources"]
            .as_array()
            .unwrap()
            .iter()
            .find(|resource| resource["kind"] == "webp-image")
            .unwrap();
        assert_eq!(resource["kind"], "webp-image");
        assert_eq!(
            resource["rendition_of"],
            "https://one.example/mount/pixel.png"
        );
        assert_eq!(
            resource["url"],
            "https://one.example/mount/images/display.webp"
        );
        assert!(bytes(output, "images/display.webp").starts_with(b"RIFF"));
        if width.is_none() {
            let retained = bytes(output, "images/display.webp");
            let page = format!(
                "{}\n![Copied image](still.webp)\n",
                page("copied", "/copied/", "")
            );
            let copied = Fixture::new(
                false,
                &[
                    ("content/copied/index.md", page.as_bytes()),
                    ("content/copied/still.webp", retained),
                ],
            );
            let captured = acquire(&copied.configuration());
            let rendered = render(&captured).expect("ordinary copied WebP");
            let copied_output = rendered.site().output();
            let html = std::str::from_utf8(bytes(copied_output, "copied/index.html")).unwrap();
            assert!(html.contains("width=\"2\" height=\"1\""), "{html}");
            assert!(!html.contains("data-gazette-image-sized"));
            assert_eq!(bytes(copied_output, "copied/still.webp"), retained);
        }
    }
}

#[test]
fn declared_body_variants_reach_cli_html_model_and_validated_asset_bytes() {
    let light = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 2 2"><rect width="2" height="2" fill="#123456"/></svg>"##;
    let dark = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 2 2"><rect width="2" height="2" fill="#abcdef"/></svg>"##;
    let declarations = "[[images]]\ndefault='figure.svg'\nlight='figure.svg'\ndark='dark.svg'\nwidth=480\n[[images]]\ndefault='plot.png'\ndark='plot-dark.png'\nwidth=360\n";
    let page = format!(
        "{}\n![Vector](figure.svg \"Vector title\")\n\nIts caption.\n\n![Plot](plot.png)\n",
        page("paired", "/paired/", declarations)
    );
    let raster = png();
    let fixture = Fixture::new(
        false,
        &[
            ("content/paired/index.md", page.as_bytes()),
            ("content/paired/figure.svg", light),
            ("content/paired/dark.svg", dark),
            ("content/paired/plot.png", &raster),
            ("content/paired/plot-dark.png", &raster),
        ],
    );
    let config = fixture.configuration();
    let inputs = acquire(&config);
    let publication = render(&inputs).expect("compile declared vector and raster renditions");
    let output = publication.site().output();
    let model: Value = serde_json::from_slice(bytes(output, "model.json")).unwrap();
    let images = &model["pages"]["content:paired"]["images"];
    assert_eq!(images.as_array().unwrap().len(), 2);
    for (index, default, dark, width) in [
        (0, "figure.svg", "dark.svg", 480),
        (1, "plot.png", "plot-dark.png", 360),
    ] {
        assert_eq!(
            images[index]["destination"],
            format!("https://one.example/mount/paired/{default}")
        );
        assert_eq!(
            images[index]["variants"]["dark"],
            format!("https://one.example/mount/paired/{dark}")
        );
        assert_eq!(images[index]["width"], width);
    }
    let html = std::str::from_utf8(bytes(output, "paired/index.html")).unwrap();
    assert_eq!(html.matches("<img ").count(), 2);
    for image in images.as_array().unwrap() {
        assert!(html.contains(&format!(
            "src=\"{}\"",
            image["destination"].as_str().unwrap()
        )));
        assert!(html.contains(&format!(
            "data-gazette-image=\"{}\"",
            image["address"].as_str().unwrap()
        )));
    }
    assert!(html.contains("Its caption."));
    assert_eq!(bytes(output, "paired/figure.svg"), light);
    assert_eq!(bytes(output, "paired/dark.svg"), dark);
    assert_eq!(bytes(output, "paired/plot-dark.png"), raster);
    let parent = tempdir().unwrap();
    let result = cli(&config, parent.path());
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let receipt: Value = serde_json::from_slice(&result.stdout).unwrap();
    let directory = Path::new(receipt["directory"].as_str().unwrap());
    for path in [
        "model.json",
        "paired/index.html",
        "paired/figure.svg",
        "paired/dark.svg",
        "paired/plot.png",
        "paired/plot-dark.png",
    ] {
        assert_eq!(fs::read(directory.join(path)).unwrap(), bytes(output, path));
    }
}

#[test]
fn a_dark_variant_cannot_bypass_inert_svg_compilation() {
    let page = format!(
        "{}\n![Safe default](figure.svg)\n",
        page(
            "paired",
            "/paired/",
            "[[images]]\ndefault='figure.svg'\ndark='dark.svg'\n"
        )
    );
    let fixture = Fixture::new(
        false,
        &[
            ("content/paired/index.md", page.as_bytes()),
            (
                "content/paired/figure.svg",
                br#"<svg xmlns="http://www.w3.org/2000/svg"/>"#,
            ),
            (
                "content/paired/dark.svg",
                br#"<svg xmlns="http://www.w3.org/2000/svg"><script/></svg>"#,
            ),
        ],
    );
    assert!(matches!(
        render(&acquire(&fixture.configuration())),
        Err(build::PublicationRenderError::Resource(_))
    ));
}

#[test]
fn retained_sibling_svg_bundle_images_and_notes_reach_exact_validated_output() {
    // Synthetic drawings, not copies of owner-held creative illustrations.
    let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 2 2"><rect width="2" height="2" fill="#123456"/></svg>"##;
    let template = "{{ body_html | safe }}{% if notes is defined and notes %}<aside>{{ notes.body_html | safe }}</aside>{% endif %}";
    let roots = [
        (
            "diagram-a",
            "collections/india-ai/testing-indias-ai-crossing/tracing-the-infrastructure-story",
            "operating-and-payment.svg",
        ),
        (
            "diagram-b",
            "blog/information-architecture-for-a-story",
            "three-orders.svg",
        ),
    ];
    let pages: Vec<_> = roots
        .iter()
        .map(|(id, directory, filename)| {
            (
                format!("content/{directory}/index.md"),
                format!(
                    "{}\n![Retained diagram]({filename})\n[Diagram file]({filename})\n",
                    page(id, &format!("/{directory}/"), "")
                ),
            )
        })
        .collect();
    let image_paths: Vec<_> = roots
        .iter()
        .map(|(_, directory, filename)| format!("content/{directory}/{filename}"))
        .collect();
    let notes_path = format!("content/{}/notes.md", roots[0].1);
    let notes = format!("## Commentary\n![Same retained diagram]({})\n", roots[0].2);
    let mut overrides: Vec<(&str, &[u8])> = pages
        .iter()
        .map(|(path, page)| (path.as_str(), page.as_bytes()))
        .collect();
    overrides.extend(
        image_paths
            .iter()
            .map(|path| (path.as_str(), svg.as_slice())),
    );
    overrides.extend([
        (notes_path.as_str(), notes.as_bytes()),
        ("theme/templates/page.html", template.as_bytes()),
        ("theme/templates/alias.html", template.as_bytes()),
    ]);
    let fixture = Fixture::new(false, &overrides);
    let config = fixture.configuration();
    let inputs = acquire(&config);
    let publication = render(&inputs).expect("existing inert SVG compiler owns implicit images");
    let output = publication.site().output();
    let model: Value = serde_json::from_slice(bytes(output, "model.json")).expect("shared model");
    let evidence =
        serde_json::to_value(publication.site().evidence()).expect("native resource provenance");
    for (id, directory, filename) in roots {
        let path = format!("{directory}/{filename}");
        assert_eq!(bytes(output, &path), svg);
        assert_eq!(
            publication.site().media_types().get(path.as_str()),
            Some(&gazette_sdk::render::MediaType::Svg)
        );
        let html = std::str::from_utf8(bytes(output, &format!("{directory}/index.html"))).unwrap();
        let address = format!("https://one.example/mount/{path}");
        assert!(html.contains(&format!("src=\"{address}\"")), "{html}");
        assert!(html.contains(&format!("href=\"{address}\"")), "{html}");
        assert!(
            html.contains("data-gazette-destination-kind=\"resource\""),
            "{html}"
        );
        let resource = &model["pages"][format!("content:{id}")]["presentation"]["resources"][0];
        assert_eq!(resource["kind"], "svg-image");
        assert_eq!(resource["url"], address);
        assert_eq!(resource["sha256"], json!(source::Sha256Digest::of(svg)));
        let proof = &evidence["page_resources"][format!("content:{id}")][0];
        assert_eq!(proof["source"], "source:content");
        assert_eq!(proof["recipe"], json!({"kind":"svg-image","entry":path}));
        assert_eq!(
            proof["dependencies"],
            json!({path.clone(): source::Sha256Digest::of(svg)})
        );
        assert_eq!(proof["output"], path);
        assert_eq!(proof["output_sha256"], json!(source::Sha256Digest::of(svg)));
    }
    let first_html =
        std::str::from_utf8(bytes(output, &format!("{}/index.html", roots[0].1))).unwrap();
    assert_eq!(
        first_html.matches("<img ").count(),
        2,
        "notes use the article resource, not another output"
    );
    assert_eq!(
        model["pages"]["content:diagram-a"]["presentation"]["resources"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    fs::write(
        fixture.root.path().join(&image_paths[0]),
        b"later source drift",
    )
    .expect("disposable live drift");
    let again = render(&inputs).expect("replay uses retained images");
    assert_eq!(
        again.site().output().inventory().identity(),
        output.inventory().identity()
    );
}

#[test]
fn implicit_svg_bundle_resources_refuse_active_payloads_before_publication() {
    let page = format!(
        "{}\n![Diagram](three-orders.svg)\n",
        page("unsafe-vector", "/unsafe-vector/", "")
    );
    for svg in [
        br#"<svg xmlns="http://www.w3.org/2000/svg"><script/></svg>"#.as_slice(),
        br#"<svg xmlns="http://www.w3.org/2000/svg" onload="run()"/>"#.as_slice(),
    ] {
        let fixture = Fixture::new(
            false,
            &[
                ("content/unsafe-vector/index.md", page.as_bytes()),
                ("content/unsafe-vector/three-orders.svg", svg),
            ],
        );
        let inputs = acquire(&fixture.configuration());
        assert!(
            matches!(
                render(&inputs),
                Err(build::PublicationRenderError::Resource(_))
            ),
            "implicit lowering must preserve native vector admission"
        );
    }
}

// The existing notebook fixture's complete, checksum-valid one-pixel red PNG.
const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==";
const IMAGE_TEMPLATE: &str = r#"{{ body_html | safe }}{% for resource in presentation.resources %}{% if resource.kind == "png-image" or resource.kind == "jpeg-image" %}<img src="{{ resource.url }}" width="{{ resource.image.output.width }}" height="{{ resource.image.output.height }}" data-kind="{{ resource.kind }}" data-sha256="{{ resource.sha256 }}" alt="Fixture red pixel">{% endif %}{% endfor %}"#;

fn png() -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(PNG)
        .expect("complete retained PNG fixture")
}

#[test]
fn page_styles_reference_the_same_transformed_output_without_another_asset_owner() {
    let original = png();
    let resources = format!(
        "{RESOURCES}{}",
        image_resource(2, 2, "{format='png'}", "/images/transformed.png")
    );
    let first = page("one", "/one/", &resources);
    let fixture = Fixture::new(
        false,
        &[
            ("content/one.md", first.as_bytes()),
            ("content/pixel.png", &original),
            (
                "content/main.scss",
                b"@use 'tokens'; .card { color: tokens.$ink; background: url('../images/transformed.png'); }",
            ),
        ],
    );
    let config = fixture.configuration();
    let inputs = acquire(&config);
    let built = render(&inputs).expect("stylesheet and image share output assembly");
    let tree = built.site().output();
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).expect("model");
    assert_eq!(
        model["pages"]["content:one"]["presentation"]["resources"][0]["references"],
        json!({"../images/transformed.png":"images/transformed.png"})
    );
    assert!(!bytes(tree, "images/transformed.png").is_empty());
    let parent = tempdir().expect("stage");
    let output = cli(&config, parent.path());
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value = serde_json::from_slice(&output.stdout).expect("receipt");
    assert_eq!(
        source::inspect(
            Path::new(receipt["directory"].as_str().expect("stage")),
            None,
            source::Limits::default()
        )
        .expect("read-back")
        .identity(),
        tree.inventory().identity()
    );
}

fn image_resource(width: u32, height: u32, encoding: &str, output: &str) -> String {
    image_resource_from(width, height, encoding, output, "png")
}

fn image_resource_from(
    width: u32,
    height: u32,
    encoding: &str,
    output: &str,
    input_format: &str,
) -> String {
    format!(
        "[[resources]]\ninputs=['pixel.{input_format}']\nbuild={{kind='raster-image',entry='pixel.{input_format}',input_format='{input_format}',resize={{operation='scale',width={width},height={height}}},encoding={encoding},apply_orientation=true}}\noutput='{output}'\n"
    )
}

const RESPONSIVE_STYLE: &str = "@use 'tokens'; .card { color: tokens.$ink; background-image: image-set('../images/normal.png' 1x type('image/png'), url('../images/dense.jpg') 2x type('image/jpeg')); }";

fn responsive_fixture(include_dense: bool) -> Fixture {
    let mut resources = format!(
        "{RESOURCES}{}",
        image_resource(2, 2, "{format='png'}", "/images/normal.png")
    );
    if include_dense {
        resources.push_str(&image_resource(
            4,
            4,
            "{format='jpeg',quality=90,background=[255,255,255]}",
            "/images/dense.jpg",
        ));
    }
    let first = page("one", "/one/", &resources);
    let template = format!(
        "{IMAGE_TEMPLATE}{{% for resource in presentation.resources %}}{{% if resource.kind == 'stylesheet' %}}<link rel=\"stylesheet\" href=\"{{{{ resource.url }}}}\">{{% for reference, target in resource.references %}}<i>{{{{ reference }}}}:{{{{ target }}}}</i>{{% endfor %}}{{% endif %}}{{% endfor %}}"
    );
    Fixture::new(
        false,
        &[
            ("content/one.md", first.as_bytes()),
            ("content/pixel.png", &png()),
            ("content/main.scss", RESPONSIVE_STYLE.as_bytes()),
            ("theme/templates/page.html", template.as_bytes()),
            ("theme/templates/alias.html", template.as_bytes()),
        ],
    )
}

#[test]
fn responsive_image_set_connects_both_transformed_variants_to_retained_output() {
    let fixture = responsive_fixture(true);
    let config = fixture.configuration();
    let inputs = acquire(&config);
    let publication = render(&inputs).expect("native image-set and both generated variants");
    let tree = publication.site().output();
    let identity = tree.inventory().identity();
    let evidence = serde_json::to_value(publication.site().evidence()).expect("build evidence");
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).expect("shared model");
    let resources = &model["pages"]["content:one"]["presentation"]["resources"];
    assert_eq!(resources.as_array().expect("ordered resources").len(), 4);
    assert_eq!(
        resources[0]["references"],
        json!({
            "../images/normal.png": "images/normal.png",
            "../images/dense.jpg": "images/dense.jpg",
        })
    );
    let css = std::str::from_utf8(bytes(tree, "assets/z-first.css")).expect("native CSS");
    assert!(css.contains("image-set("), "{css}");
    for value in [
        "../images/normal.png",
        "../images/dense.jpg",
        "1x",
        "2x",
        "image/png",
        "image/jpeg",
    ] {
        assert!(css.contains(value), "{value}: {css}");
    }
    for (index, format, path, size) in [
        (2, Format::Png, "images/normal.png", 2),
        (3, Format::Jpeg, "images/dense.jpg", 4),
    ] {
        let pixels = raster::inspect(format, bytes(tree, path), Default::default())
            .expect("decode actual transformed variant");
        assert_eq!((pixels.width, pixels.height), (size, size));
        assert_eq!(
            resources[index]["image"]["output"],
            json!({"width": size, "height": size})
        );
        assert_eq!(resources[index]["sha256"], json!(pixels.sha256));
        assert_eq!(resources[index]["output"], path);
        assert_eq!(
            resources[index]["url"],
            format!("https://one.example/mount/{path}")
        );
        for page in ["one/index.html", "old.html"] {
            let html = std::str::from_utf8(bytes(tree, page))
                .expect("same resource facts reach Theme")
                .replace("&#x2F;", "/");
            assert!(html.contains(&format!("../{path}:{path}")), "{html}");
            assert!(html.contains("href=\"https://one.example/mount/assets/z-first.css\""));
            assert!(
                html.contains(&format!(
                    "src=\"https://one.example/mount/{path}\" width=\"{size}\" height=\"{size}\""
                )),
                "{html}"
            );
        }
    }
    assert_eq!(
        model["pages"]["content:two"]["presentation"]["resources"],
        json!([])
    );

    let parent = tempdir().expect("fresh CLI stage parent");
    let result = cli(&config, parent.path());
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let receipt: Value = serde_json::from_slice(&result.stdout).expect("CLI staging receipt");
    let directory = Path::new(receipt["directory"].as_str().expect("stage directory"));
    assert_eq!(
        source::inspect(directory, None, Default::default())
            .expect("native read-back")
            .identity(),
        identity
    );
    for entry in tree.inventory().entries() {
        if matches!(entry.content, source::Content::File { .. }) {
            assert_eq!(
                fs::read(directory.join(entry.path.as_str())).expect("staged file"),
                tree.file(&entry.path).expect("SDK output")
            );
        }
    }
    fs::write(
        fixture.root.path().join("content/pixel.png"),
        b"changed after capture",
    )
    .expect("disposable native image drift");
    fs::write(
        fixture.root.path().join("content/main.scss"),
        b"invalid later stylesheet {",
    )
    .expect("disposable native stylesheet drift");
    let again = render(&inputs).expect("both compilation paths use frozen input");
    assert_eq!(again.site().output().inventory().identity(), identity);
    assert_eq!(
        serde_json::to_value(again.site().evidence()).expect("repeat evidence"),
        evidence
    );
}

#[test]
fn responsive_image_set_cannot_omit_a_missing_density_candidate_before_staging() {
    let fixture = responsive_fixture(false);
    // The lower-resolution image is valid. The browser's ability to choose it
    // cannot discharge the missing second candidate in the complete output set.
    failure(&fixture.configuration(), "source_inventory");
}

fn fixture(width: u32, height: u32, image: &[u8]) -> Fixture {
    fixture_from(width, height, image, "png")
}

fn fixture_from(width: u32, height: u32, image: &[u8], input_format: &str) -> Fixture {
    let resources = format!(
        "{RESOURCES}{}{}",
        image_resource_from(
            width,
            height,
            "{format='png'}",
            "/assets/transformed.png",
            input_format
        ),
        image_resource_from(
            width,
            height,
            "{format='jpeg',quality=90,background=[255,255,255]}",
            "/assets/transformed.jpg",
            input_format,
        ),
    );
    let first = page("one", "/one/", &resources);
    Fixture::new(
        false,
        &[
            ("content/one.md", first.as_bytes()),
            (&format!("content/pixel.{input_format}"), image),
            ("theme/templates/page.html", IMAGE_TEMPLATE.as_bytes()),
            ("theme/templates/alias.html", IMAGE_TEMPLATE.as_bytes()),
        ],
    )
}

#[test]
fn retained_images_reach_matching_theme_model_and_staged_bytes() {
    retained_image_outputs(png(), Format::Png, "png");
}

#[test]
fn retained_webp_reaches_png_and_jpeg_theme_model_and_staged_bytes() {
    // A complete one-pixel red lossless WebP encoded with image-webp 0.2.4.
    let original = vec![
        0x52, 0x49, 0x46, 0x46, 0x1a, 0, 0, 0, 0x57, 0x45, 0x42, 0x50, 0x56, 0x50, 0x38, 0x4c,
        0x0e, 0, 0, 0, 0x2f, 0, 0, 0, 0, 0xcd, 0x55, 0x20, 0x22, 0x02, 0xd1, 0xff, 0x88, 0x04,
    ];
    retained_image_outputs(original, Format::Webp, "webp");
}

fn retained_image_outputs(original: Vec<u8>, input_format: Format, extension: &str) {
    let fixture = fixture_from(3, 2, &original, extension);
    let entry = format!("pixel.{extension}");
    let config = fixture.configuration();
    let inputs = acquire(&config);
    let publication = render(&inputs).expect("real configured raster compilation");
    let output = publication.site().output();
    let identity = output.inventory().identity();
    let model: Value = serde_json::from_slice(bytes(output, "model.json")).expect("shared model");
    let resources = model["pages"]["content:one"]["presentation"]["resources"]
        .as_array()
        .expect("ordered resource projection");
    assert_eq!(
        resources.len(),
        4,
        "existing styles/module and two image outputs"
    );
    for resource in &resources[..2] {
        assert!(
            resource.get("image").is_none(),
            "scripts and styles have no image geometry"
        );
    }
    let source_pixels = raster::inspect(input_format, &original, Default::default())
        .expect("independently decode retained original");
    assert_eq!((source_pixels.width, source_pixels.height), (1, 1));
    let evidence = serde_json::to_value(publication.site().evidence()).expect("native evidence");
    for (index, format, media_type, kind, path) in [
        (
            2,
            Format::Png,
            gazette_sdk::render::MediaType::Png,
            "png-image",
            "assets/transformed.png",
        ),
        (
            3,
            Format::Jpeg,
            gazette_sdk::render::MediaType::Jpeg,
            "jpeg-image",
            "assets/transformed.jpg",
        ),
    ] {
        let encoded = bytes(output, path);
        let decoded = raster::inspect(format, encoded, Default::default())
            .expect("actual emitted pixels decode through the independent native inspector");
        assert_eq!((decoded.width, decoded.height), (3, 2));
        assert_eq!(
            resources[index]["image"],
            json!({
                "source": {"width": source_pixels.width, "height": source_pixels.height},
                "oriented": {"width": 1, "height": 1},
                "output": {"width": decoded.width, "height": decoded.height},
            })
        );
        assert_ne!(
            encoded, original,
            "transformed output is not a renamed source"
        );
        assert_eq!(resources[index]["kind"], kind);
        assert_eq!(
            publication.site().media_types().get(path),
            Some(&media_type),
        );
        assert_eq!(resources[index]["output"], path);
        assert_eq!(resources[index]["bytes"], encoded.len());
        assert_eq!(resources[index]["sha256"], json!(decoded.sha256));
        assert_eq!(
            resources[index]["url"],
            format!("https://one.example/mount/{path}")
        );
        let private = &evidence["page_resources"]["content:one"][index];
        assert_eq!(private["source"], "source:content");
        assert_eq!(private["recipe"]["kind"], "raster-image");
        assert_eq!(private["recipe"]["entry"], entry);
        assert_eq!(private["recipe"]["input_format"], extension);
        assert_eq!(private["recipe"]["resize"]["width"], 3);
        assert_eq!(
            private["dependencies"],
            json!({&entry: source::Sha256Digest::of(&original)})
        );
        assert_eq!(private["output_sha256"], resources[index]["sha256"]);
        assert!(
            !private["tool"]
                .as_str()
                .expect("native compiler identity")
                .is_empty()
        );
        for page in ["one/index.html", "old.html"] {
            let html = std::str::from_utf8(bytes(output, page))
                .expect("Theme output")
                .replace("&#x2F;", "/");
            assert!(
                html.contains(&format!(
                    "src=\"https://one.example/mount/{path}\" width=\"{}\" height=\"{}\"",
                    decoded.width, decoded.height
                )),
                "{html}"
            );
            assert!(html.contains(&format!("data-kind=\"{kind}\"")), "{html}");
            assert!(
                html.contains(&format!("data-sha256=\"{}\"", decoded.sha256)),
                "{html}"
            );
        }
    }
    assert!(
        !std::str::from_utf8(bytes(output, "two/index.html"))
            .expect("unrelated page")
            .contains("<img")
    );
    let public = std::str::from_utf8(bytes(output, "model.json")).expect("public model");
    assert!(!public.contains(fixture.root.path().to_str().expect("native root")));
    assert!(
        !public.contains("\"recipe\""),
        "private transform recipe is not a public field"
    );

    let parent = tempdir().expect("private staging parent");
    let result = cli(&config, parent.path());
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let receipt: Value = serde_json::from_slice(&result.stdout).expect("native staging receipt");
    let directory = Path::new(receipt["directory"].as_str().expect("retained output path"));
    assert_eq!(
        source::inspect(directory, None, Default::default())
            .expect("fresh retained inventory")
            .identity(),
        identity
    );
    for entry in output.inventory().entries() {
        if matches!(entry.content, source::Content::File { .. }) {
            assert_eq!(
                fs::read(directory.join(entry.path.as_str())).expect("staged file"),
                output.file(&entry.path).expect("same SDK artifact")
            );
        }
    }
    assert_eq!(
        fs::read(fixture.root.path().join("content").join(&entry)).expect("original source"),
        original
    );
    fs::write(
        fixture.root.path().join("content").join(&entry),
        b"later invalid native image",
    )
    .expect("disposable fixture drift after capture");
    let again = render(&inputs).expect("rebuild uses retained image, not live source");
    assert_eq!(again.site().output().inventory().identity(), identity);
    assert_eq!(
        serde_json::to_value(again.site().evidence()).expect("repeat evidence"),
        evidence
    );
}

#[test]
fn fit_width_projects_actual_aspect_preserved_dimensions_through_theme_and_cli() {
    // Reuse the existing native fixture/compiler to obtain a real non-square
    // retained source; no invented PNG header or second image generator.
    let original = png();
    let seed = fixture(3, 2, &original);
    let seed_inputs = acquire(&seed.configuration());
    let seed_publication = render(&seed_inputs).expect("native non-square fixture source");
    let source_image = bytes(seed_publication.site().output(), "assets/transformed.png");
    let source_pixels = raster::inspect(Format::Png, source_image, Default::default())
        .expect("exact retained source pixels");
    assert_eq!((source_pixels.width, source_pixels.height), (3, 2));
    let resources = format!(
        "{RESOURCES}[[resources]]\ninputs=['pixel.png']\nbuild={{kind='raster-image',entry='pixel.png',input_format='png',resize={{operation='fit-width',width=6}},encoding={{format='png'}},apply_orientation=true}}\noutput='/assets/fitted.png'\n"
    );
    let first = page("one", "/one/", &resources);
    let fixture = Fixture::new(
        false,
        &[
            ("content/one.md", first.as_bytes()),
            ("content/pixel.png", source_image),
            ("theme/templates/page.html", IMAGE_TEMPLATE.as_bytes()),
            ("theme/templates/alias.html", IMAGE_TEMPLATE.as_bytes()),
        ],
    );
    let config = fixture.configuration();
    let inputs = acquire(&config);
    let publication = render(&inputs).expect("fit-width connected publication");
    let tree = publication.site().output();
    let pixels = raster::inspect(
        Format::Png,
        bytes(tree, "assets/fitted.png"),
        Default::default(),
    )
    .expect("decode actual final artifact");
    assert_eq!((pixels.width, pixels.height), (6, 4));
    let model: Value =
        serde_json::from_slice(bytes(tree, "model.json")).expect("shared machine model");
    let resources = model["pages"]["content:one"]["presentation"]["resources"]
        .as_array()
        .expect("complete resources");
    assert_eq!(resources.len(), 3);
    assert!(resources[0].get("image").is_none());
    assert!(resources[1].get("image").is_none());
    assert_eq!(
        resources[2]["image"],
        json!({
            "source": {"width": 3, "height": 2},
            "oriented": {"width": 3, "height": 2},
            "output": {"width": pixels.width, "height": pixels.height},
        })
    );
    assert_eq!(resources[2]["sha256"], json!(pixels.sha256));
    for path in ["one/index.html", "old.html"] {
        let html = std::str::from_utf8(bytes(tree, path))
            .expect("Theme output")
            .replace("&#x2F;", "/");
        assert!(
            html.contains(
                "src=\"https://one.example/mount/assets/fitted.png\" width=\"6\" height=\"4\""
            ),
            "{html}"
        );
    }
    let parent = tempdir().expect("fresh private stage");
    let result = cli(&config, parent.path());
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let receipt: Value = serde_json::from_slice(&result.stdout).expect("CLI receipt");
    let directory = Path::new(receipt["directory"].as_str().expect("retained stage"));
    assert_eq!(
        source::inspect(directory, None, Default::default())
            .expect("read-back")
            .identity(),
        tree.inventory().identity()
    );
    for path in [
        "model.json",
        "one/index.html",
        "old.html",
        "assets/fitted.png",
    ] {
        assert_eq!(
            fs::read(directory.join(path)).expect("retained bytes"),
            bytes(tree, path)
        );
    }
    assert_eq!(
        fs::read(fixture.root.path().join("content/pixel.png")).expect("source retained"),
        source_image
    );
    assert_eq!(
        fs::read(seed.root.path().join("content/pixel.png")).expect("original seed retained"),
        original
    );
}

#[test]
fn a_changed_image_recipe_changes_the_actual_pixels_output_and_retained_recipe() {
    let original = png();
    let first = fixture(3, 2, &original);
    let second = fixture(5, 4, &original);
    let first_inputs = acquire(&first.configuration());
    let second_inputs = acquire(&second.configuration());
    let first = render(&first_inputs).expect("first transform");
    let second = render(&second_inputs).expect("changed transform");
    assert_ne!(
        first.site().output().inventory().identity(),
        second.site().output().inventory().identity()
    );
    for (format, path) in [
        (Format::Png, "assets/transformed.png"),
        (Format::Jpeg, "assets/transformed.jpg"),
    ] {
        let first_bytes = bytes(first.site().output(), path);
        let second_bytes = bytes(second.site().output(), path);
        assert_ne!(first_bytes, second_bytes);
        let first_pixels =
            raster::inspect(format, first_bytes, Default::default()).expect("first pixels");
        let second_pixels =
            raster::inspect(format, second_bytes, Default::default()).expect("second pixels");
        assert_eq!((first_pixels.width, first_pixels.height), (3, 2));
        assert_eq!((second_pixels.width, second_pixels.height), (5, 4));
    }
    let first = serde_json::to_value(first.site().evidence()).expect("first evidence");
    let second = serde_json::to_value(second.site().evidence()).expect("second evidence");
    let first = &first["page_resources"]["content:one"][2];
    let second = &second["page_resources"]["content:one"][2];
    assert_eq!(
        first["input"], second["input"],
        "the exact same input image"
    );
    assert_ne!(first["recipe"], second["recipe"]);
    assert_ne!(first["output_sha256"], second["output_sha256"]);
}

#[test]
fn invalid_or_oversized_image_resources_refuse_the_complete_publication_without_staging() {
    let invalid = fixture(3, 2, b"not a PNG");
    failure(&invalid.configuration(), "resource_compile");
    let oversized = fixture(u32::MAX, u32::MAX, &png());
    failure(&oversized.configuration(), "resource_limit");

    let valid = fixture(3, 2, &png());
    let inputs = acquire(&valid.configuration());
    for dimensions in [true, false] {
        let mut limits = build::PublicationRenderLimits::default();
        if dimensions {
            limits.output.resources.raster.width = 2;
        } else {
            limits.output.resources.raster.decoded_bytes = 16;
        }
        let error = render_with_limits(&inputs, limits)
            .expect_err("raster-specific limits refuse the complete site after earlier programs");
        assert_eq!(error.code(), "resource_limit", "{error}");
    }
    render(&inputs).expect("bounded attempts did not damage the retained source");
}

#[test]
fn automatic_svg_companions_reserve_verified_layout_before_loading() {
    for (attributes, width, suffix, dark_attributes, expected) in [
        (
            r#"width="360" height="300""#,
            Some(360),
            "",
            None,
            Some((360, 300)),
        ),
        (
            r#"viewBox="0 0 360 300""#,
            Some(360),
            "",
            None,
            Some((360, 300)),
        ),
        (
            r#"width="480" height="166" viewBox="0 258 480 166""#,
            None,
            "",
            None,
            Some((480, 166)),
        ),
        (r#"viewBox="0 0 480 166""#, None, "", None, None),
        (
            r#"width="360" height="300""#,
            Some(360),
            "#other-view",
            None,
            None,
        ),
        (
            r#"width="360" height="300""#,
            Some(360),
            "",
            Some(r#"width="360" height="100""#),
            None,
        ),
    ] {
        let svg = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" {attributes}/>"#);
        let dark = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" {}/>"#,
            dark_attributes.unwrap_or(attributes)
        );
        let declarations = width.map_or_else(String::new, |width| {
            format!(
                "[[images]]\ndefault='figure.svg'\nwidth={width}\n{}",
                if dark_attributes.is_some() {
                    "dark='dark.svg'\n"
                } else {
                    ""
                }
            )
        });
        let page = format!(
            "{}\n![Diagram](figure.svg{suffix})\n",
            page("geometry", "/geometry/", &declarations)
        );
        let fixture = Fixture::new(
            false,
            &[
                ("content/geometry/index.md", page.as_bytes()),
                ("content/geometry/figure.svg", svg.as_bytes()),
                ("content/geometry/dark.svg", dark.as_bytes()),
            ],
        );
        let inputs = acquire(&fixture.configuration());
        let publication = render(&inputs).expect("automatic vector companion");
        let output = publication.site().output();
        let html = std::str::from_utf8(bytes(output, "geometry/index.html")).unwrap();
        assert_eq!(bytes(output, "geometry/figure.svg"), svg.as_bytes());
        assert_eq!(
            html.contains("data-gazette-image-sized"),
            width.is_some(),
            "{html}"
        );
        if let Some((width, height)) = expected {
            assert!(html.contains(&format!("width=\"{width}\"")), "{html}");
            assert!(html.contains(&format!("height=\"{height}\"")), "{html}");
        } else {
            assert!(!html.contains("height="), "{html}");
        }
        let model: Value = serde_json::from_slice(bytes(output, "model.json")).unwrap();
        assert!(model["pages"]["content:geometry"]["images"][0]["intrinsic"].is_null());
    }
}

#[test]
fn raster_variants_need_matching_verified_ratios_before_reserving_geometry() {
    for (suffix, dark_recipe) in [
        (
            "",
            "[[resources]]\ninputs=['dark.png']\nrendition=true\noutput='/images/dark.webp'\nbuild={kind='raster-image',entry='dark.png',input_format='png',apply_orientation=false,resize={operation='scale',width=1,height=2},encoding={format='webp'}}\n",
        ),
        ("", ""),
        (
            "#xywh=0,0,1,1",
            "[[resources]]\ninputs=['dark.png']\nrendition=true\noutput='/images/dark.webp'\nbuild={kind='raster-image',entry='dark.png',input_format='png',apply_orientation=false,resize={operation='scale',width=2,height=1},encoding={format='webp'}}\n",
        ),
    ] {
        let declarations = format!(
            "[[resources]]\ninputs=['pixel.png']\nrendition=true\noutput='/images/default.webp'\nbuild={{kind='raster-image',entry='pixel.png',input_format='png',apply_orientation=false,resize={{operation='scale',width=2,height=1}},encoding={{format='webp'}}}}\n{dark_recipe}[[images]]\ndefault='pixel.png'\ndark='dark.png'\nwidth=360\n"
        );
        let page = format!(
            "{}\n![Variant](pixel.png{suffix})\n",
            page("variants", "/variants/", &declarations)
        );
        let original = png();
        let fixture = Fixture::new(
            false,
            &[
                ("content/variants.md", page.as_bytes()),
                ("content/pixel.png", &original),
                ("content/dark.png", &original),
            ],
        );
        let config = fixture.configuration().replace(
            "target='authored'",
            "target='authored',static_assets={route_prefix='/'}",
        );
        let inputs = acquire(&config);
        let publication = render(&inputs).expect("variant output");
        let output = publication.site().output();
        let html = std::str::from_utf8(bytes(output, "variants/index.html")).unwrap();
        assert!(html.contains("width=\"360\""), "{html}");
        assert!(!html.contains("height="), "{html}");
        let model: Value = serde_json::from_slice(bytes(output, "model.json")).unwrap();
        assert!(model["pages"]["content:variants"]["images"][0]["intrinsic"].is_null());
    }
}
