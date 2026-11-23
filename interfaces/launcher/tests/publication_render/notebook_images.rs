//! Stored raster results through retained inputs, the real decoder and shared publication output.

use serde_json::json;

use super::{
    Fixture, PAGE, RENDER_ARGUMENTS, Value, acquire, assert_cli_failure_preserves_parent, bytes,
    cli, fs, render, tempdir,
};

// A synthetic 1×1 RGBA red pixel: PNG chunks have real CRCs and a complete zlib stream.
const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==";
const PNG_SHA256: &str = "4ff6ab670a58c14270e034e2090d9a432caa263a14e0a25785386b0c12f880b5";
const TRUNCATED_PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQQ==";
// Synthetic 2×1 RGB red/green pixels, encoded once with image 0.25.10 at quality 90.
const JPEG: &str = "/9j/4AAQSkZJRgABAgAAAQABAAD/wAARCAABAAIDAREAAhEBAxEB/9sAQwADAgIDAgIDAwMDBAMDBAUIBQUEBAUKBwcGCAwKDAwLCgsLDQ4SEA0OEQ4LCxAWEBETFBUVFQwPFxgWFBgSFBUU/9sAQwEDBAQFBAUJBQUJFA0LDRQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQU/8QAHwAAAQUBAQEBAQEAAAAAAAAAAAECAwQFBgcICQoL/8QAtRAAAgEDAwIEAwUFBAQAAAF9AQIDAAQRBRIhMUEGE1FhByJxFDKBkaEII0KxwRVS0fAkM2JyggkKFhcYGRolJicoKSo0NTY3ODk6Q0RFRkdISUpTVFVWV1hZWmNkZWZnaGlqc3R1dnd4eXqDhIWGh4iJipKTlJWWl5iZmqKjpKWmp6ipqrKztLW2t7i5usLDxMXGx8jJytLT1NXW19jZ2uHi4+Tl5ufo6erx8vP09fb3+Pn6/8QAHwEAAwEBAQEBAQEBAQAAAAAAAAECAwQFBgcICQoL/8QAtREAAgECBAQDBAcFBAQAAQJ3AAECAxEEBSExBhJBUQdhcRMiMoEIFEKRobHBCSMzUvAVYnLRChYkNOEl8RcYGRomJygpKjU2Nzg5OkNERUZHSElKU1RVVldYWVpjZGVmZ2hpanN0dXZ3eHl6goOEhYaHiImKkpOUlZaXmJmaoqOkpaanqKmqsrO0tba3uLm6wsPExcbHyMnK0tPU1dbX2Nna4uPk5ebn6Onq8vP09fb3+Pn6/9oADAMBAAIRAxEAPwDovBn/ACJ+hf8AXhB/6LWv4tx3+91f8UvzZ/GHij/yXuf/APYZif8A09M//9k=";
const JPEG_SHA256: &str = "dfddaa565fdcb271d5bc0b0bfb657077a4f5c2abd64f182c4dfb9854c17dade9";

fn fixture(outputs: Vec<Value>) -> Fixture {
    cells_fixture(vec![
        json!({"cell_type":"code","id":"stored-images","metadata":{},
        "source":"raise RuntimeError('never execute stored image generation')",
        "execution_count":null,"outputs":outputs}),
    ])
}

fn cells_fixture(cells: Vec<Value>) -> Fixture {
    let notebook = serde_json::to_vec(&json!({
        "nbformat":4,"nbformat_minor":5,"metadata":{"private_image_notes":"NOT PUBLIC"},
        "cells":cells
    }))
    .expect("valid retained notebook syntax");
    let page = PAGE.replacen("+++\n", "+++\nnotebooks=['analysis.ipynb']\n", 1);
    Fixture::new(&[
        ("one/page.md", page.as_bytes()),
        ("one/analysis.ipynb", &notebook),
    ])
}

#[test]
fn cell_local_attachments_reach_cli_and_shared_outputs_from_retained_pixels() {
    let fixture = cells_fixture(vec![
        json!({"cell_type":"markdown","id":"png-cell","metadata":{},
            "source":"![Red attachment](attachment:plot)",
            "attachments":{"plot":{"image/png":PNG}}}),
        json!({"cell_type":"markdown","id":"jpeg-cell","metadata":{},
            "source":"![Red and green attachment](attachment:plot)",
            "attachments":{"plot":{"image/jpeg":JPEG}}}),
    ]);
    let config = fixture.config().replace(
        "public_base_url=",
        "machine={search='lexical'}\npublic_base_url=",
    );
    let inputs = acquire(&config);
    let rendered = render(&inputs).expect("cell-local retained attachments");
    let tree = rendered.site().output();
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).expect("shared model");
    let page = &model["pages"]["content:one"];
    let body = &super::notebooks::notebook_sections(page);
    let html = std::str::from_utf8(bytes(tree, "guide/index.html")).expect("Theme HTML");
    assert!(html.contains(body));
    for (id, mime, encoded, other) in [
        ("png-cell", "image/png", PNG, JPEG),
        ("jpeg-cell", "image/jpeg", JPEG, PNG),
    ] {
        let cell = body
            .split_once(&format!("data-cell-id=\"{id}\""))
            .expect("distinct owning Markdown cell")
            .1
            .split_once("</div>")
            .expect("complete cell")
            .0;
        assert!(cell.contains(&format!("src=\"data:{mime};base64,{encoded}\"")));
        assert!(
            !cell.contains(other),
            "same name must not borrow another cell's image"
        );
    }
    let text = page["notes"]["body_text"]
        .as_str()
        .expect("searchable image alternatives");
    assert!(text.contains("Red attachment"));
    assert!(text.contains("Red and green attachment"));
    assert!(!text.contains(PNG));
    assert!(!text.contains(JPEG));
    let search: Vec<Value> = serde_json::from_slice(bytes(tree, "search.json")).expect("search");
    let record = search
        .iter()
        .find(|record| record["content"]["id"] == "content:one")
        .expect("same page search record");
    assert_eq!(record["notes"]["body_text"], text);
    let evidence = serde_json::to_value(rendered.site().evidence()).expect("private evidence");
    let images = evidence["pages"]["content:one"]["notebooks"][0]["images"]
        .as_array()
        .expect("raster evidence");
    assert_eq!(images.len(), 2);
    for (image, (format, digest, width, size)) in images
        .iter()
        .zip([("png", PNG_SHA256, 1, 70), ("jpeg", JPEG_SHA256, 2, 668)])
    {
        assert_eq!(image["format"], format);
        assert_eq!(image["sha256"], digest);
        assert_eq!(image["width"], width);
        assert_eq!(image["height"], 1);
        assert_eq!(image["bytes"], size);
    }
    assert!(!html.contains("NOT PUBLIC"));
    let parent = tempdir().expect("CLI stage parent");
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments.extend([
        "--output-parent",
        parent.path().to_str().expect("parent path"),
    ]);
    let result = cli(&config, &arguments);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let receipt: Value = serde_json::from_slice(&result.stdout).expect("CLI receipt");
    assert_eq!(
        receipt["inventory"],
        serde_json::to_value(tree.inventory()).expect("SDK inventory")
    );
    let directory = std::path::Path::new(receipt["directory"].as_str().expect("stage"));
    for path in ["guide/index.html", "model.json", "search.json"] {
        assert_eq!(
            fs::read(directory.join(path)).expect("native read-back"),
            bytes(tree, path)
        );
    }
    fs::write(
        fixture.directory.path().join("one/analysis.ipynb"),
        b"changed after capture",
    )
    .expect("disposable source mutation");
    assert_eq!(
        render(&inputs)
            .expect("retained attachment rebuild")
            .site()
            .output()
            .inventory()
            .identity(),
        tree.inventory().identity()
    );
    assert_cli_failure_preserves_parent(&config, "source_content_mismatch");
}

#[test]
fn missing_or_invalid_cell_attachments_fail_before_cli_staging() {
    for (attachments, expected) in [
        (json!({}), "markdown_unsafe_url"),
        (json!({"plot":{"image/png":TRUNCATED_PNG}}), "raster_decode"),
    ] {
        let fixture = cells_fixture(vec![json!({
            "cell_type":"markdown","id":"bad-attachment","metadata":{},
            "source":"![No fallback](attachment:plot)","attachments":attachments
        })]);
        let config = fixture.config();
        assert_eq!(
            render(&acquire(&config))
                .expect_err("no missing or corrupt image fallback")
                .code(),
            expected
        );
        assert_cli_failure_preserves_parent(&config, expected);
    }
}

fn output(mime: &str, data: Value, description: &str) -> Value {
    let mut bundle = serde_json::Map::new();
    bundle.insert(mime.into(), data);
    bundle.insert("text/plain".into(), json!(description));
    json!({"output_type":"display_data","metadata":{},"data":bundle})
}

#[test]
fn stored_pixels_reach_cli_html_shared_model_search_and_private_decode_evidence() {
    let fixture = fixture(vec![
        output("image/png", json!(PNG), "A retained red pixel"),
        output(
            "image/jpeg",
            json!([&JPEG[..400], &JPEG[400..]]),
            "Retained red and green pixels",
        ),
    ]);
    let config = fixture.config().replace(
        "public_base_url=",
        "machine={search='lexical'}\npublic_base_url=",
    );
    let inputs = acquire(&config);
    let rendered = render(&inputs).expect("retained raster pixels decoded by the selected adapter");
    let tree = rendered.site().output();
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).expect("shared model");
    let page = &model["pages"]["content:one"];
    let body = &super::notebooks::notebook_sections(page);
    let html = std::str::from_utf8(bytes(tree, "guide/index.html")).expect("Theme output");
    assert!(html.contains(body));
    assert!(body.contains("<img "));
    assert!(body.contains(&format!("src=\"data:image/png;base64,{PNG}\"")));
    assert!(body.contains(&format!("src=\"data:image/jpeg;base64,{JPEG}\"")));
    assert!(body.contains("width=\"1\""));
    assert!(body.contains("width=\"2\""));
    assert!(body.contains("height=\"1\""));
    let text = page["notes"]["body_text"]
        .as_str()
        .expect("searchable text alternative");
    assert!(text.contains("A retained red pixel"));
    assert!(text.contains("Retained red and green pixels"));
    assert!(
        !text.contains(PNG),
        "binary encoding is not searchable prose"
    );
    assert!(!text.contains(JPEG));
    let search: Vec<Value> = serde_json::from_slice(bytes(tree, "search.json")).expect("search");
    let record = search
        .iter()
        .find(|record| record["content"]["id"] == "content:one")
        .expect("same owning page search record");
    assert_eq!(record["notes"]["body_text"], text);
    let evidence = serde_json::to_value(rendered.site().evidence()).expect("private evidence");
    let notebook = &evidence["pages"]["content:one"]["notebooks"][0];
    let images = notebook["images"]
        .as_array()
        .expect("actual pixel decode evidence");
    assert_eq!(images.len(), 2);
    for (image, (format, width, size, digest, tool)) in images.iter().zip([
        (
            "png",
            1,
            70,
            PNG_SHA256,
            "gazette.raster/2;display-orientation;png/0.18.0;single-frame;finish",
        ),
        (
            "jpeg",
            2,
            668,
            JPEG_SHA256,
            "gazette.raster/2;display-orientation;zune-jpeg/0.5.16-rc2;zune-core/0.5.3;strict",
        ),
    ]) {
        assert_eq!(image["format"], format);
        assert_eq!(image["width"], width);
        assert_eq!(image["height"], 1);
        assert_eq!(image["bytes"], size);
        assert_eq!(image["sha256"], digest);
        assert_eq!(image["tool"], tool);
    }
    assert!(
        page.get("notebooks").is_none(),
        "decoder evidence is not public content metadata"
    );
    assert!(!html.contains("NOT PUBLIC"));
    assert!(
        tree.inventory()
            .entries()
            .iter()
            .all(|entry| !entry.path.as_str().ends_with(".ipynb"))
    );

    let parent = tempdir().expect("local CLI staging parent");
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments.extend([
        "--output-parent",
        parent.path().to_str().expect("parent path"),
    ]);
    let result = cli(&config, &arguments);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let receipt: Value = serde_json::from_slice(&result.stdout).expect("direct CLI receipt");
    assert_eq!(
        receipt["inventory"],
        serde_json::to_value(tree.inventory()).expect("SDK inventory")
    );
    let directory = std::path::Path::new(receipt["directory"].as_str().expect("staged directory"));
    assert_eq!(
        fs::read(directory.join("guide/index.html")).expect("native image HTML read-back"),
        bytes(tree, "guide/index.html")
    );
    fs::write(
        fixture.directory.path().join("one/analysis.ipynb"),
        b"native source changed after capture",
    )
    .expect("disposable source changed");
    assert_eq!(
        render(&inputs)
            .expect("retained image recompile")
            .site()
            .output()
            .inventory()
            .identity(),
        tree.inventory().identity()
    );
    assert_cli_failure_preserves_parent(&config, "source_content_mismatch");
}

#[test]
fn malformed_or_wrong_format_pixels_fail_before_stage_despite_a_text_alternative() {
    for (mime, data) in [
        ("image/png", "%%%not-base64%%%"),
        ("image/png", TRUNCATED_PNG),
        ("image/jpeg", PNG),
        ("image/png", JPEG),
        ("image/jpeg", &JPEG[..400]),
    ] {
        let fixture = fixture(vec![output(
            mime,
            json!(data),
            "MUST NOT MASK INVALID IMAGE",
        )]);
        let config = fixture.config();
        let inputs = acquire(&config);
        inputs
            .parse_publication(Default::default(), &gazette_adapter_markdown::Comrak)
            .expect("image representation belongs to render validation, not JSON syntax");
        let error =
            render(&inputs).expect_err("full pixel decoder rejects malformed retained image");
        assert_eq!(error.code(), "raster_decode", "{mime}: {error}");
        assert_cli_failure_preserves_parent(&config, "raster_decode");
    }
}
