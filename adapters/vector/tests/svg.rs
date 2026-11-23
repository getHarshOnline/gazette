//! Strict static-SVG admission and refusal behavior.

use std::collections::{BTreeMap, BTreeSet};

use gazette_core::{
    resource::{Limits, Recipe},
    source::{Content, EntryPath, InventoryBuilder, Sha256Digest, TreeInput},
};
use gazette_ports::resource::ResourceCompiler;

fn compile(svg: &str) -> Result<gazette_core::resource::Artifact, gazette_core::resource::Error> {
    let entry: EntryPath = "icon.svg".parse().expect("entry");
    let bytes = svg.as_bytes();
    let mut inventory = InventoryBuilder::new(Limits::default().inputs);
    inventory
        .insert(
            entry.clone(),
            Content::File {
                bytes: bytes.len() as u64,
                sha256: Sha256Digest::of(bytes),
            },
        )
        .expect("inventory");
    let inventory = inventory.finish();
    let files = BTreeMap::from([(entry.clone(), bytes)]);
    let source = TreeInput::new(&inventory, files).expect("tree");
    gazette_adapter_vector::Compiler.compile(
        &source,
        &Recipe::SvgImage { entry },
        &BTreeMap::new(),
        Limits::default(),
    )
}

#[test]
fn exact_inert_svg_is_retained_deterministically() {
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><defs><linearGradient id="ink"><stop offset="0" stop-color="#000"/></linearGradient><path id="mark" d="M1 1h14v14H1z"/></defs><use href="#mark" fill="url(#ink)"/></svg>"##;
    let first = compile(svg).expect("accepted");
    let second = compile(svg).expect("replayed");
    assert_eq!(first.bytes, svg.as_bytes());
    assert_eq!(first.bytes, second.bytes);
    assert_eq!(first.references, BTreeSet::new());
    assert_eq!(first.tool, gazette_adapter_vector::PROFILE);
}

#[test]
fn active_foreign_and_external_constructs_are_rejected() {
    for svg in [
        r#"<svg xmlns="http://www.w3.org/2000/svg"><script/></svg>"#,
        r#"<svg xmlns="http://www.w3.org/2000/svg"><style/></svg>"#,
        r#"<svg xmlns="http://www.w3.org/2000/svg"><foreignObject/></svg>"#,
        r#"<svg xmlns="http://www.w3.org/2000/svg" onload="run()"/>"#,
        r#"<svg xmlns="http://www.w3.org/2000/svg" style="color:red"/>"#,
        r#"<svg xmlns="http://www.w3.org/2000/svg"><use href="https://example.test/a.svg#x"/></svg>"#,
        r#"<svg xmlns="http://www.w3.org/2000/svg"><image href="data:image/png;base64,AA=="/></svg>"#,
        r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink"><use xlink:href="#x"/></svg>"##,
        r#"<svg xmlns="http://www.w3.org/2000/svg"><rect fill="url(https://example.test/a.svg#x)"/></svg>"#,
        r#"<?xml-stylesheet href="theme.css"?><svg xmlns="http://www.w3.org/2000/svg"/>"#,
        r#"<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg"/>"#,
        r#"<!DOCTYPE svg><svg xmlns="http://www.w3.org/2000/svg"/>"#,
    ] {
        assert!(compile(svg).is_err(), "unexpectedly admitted {svg}");
    }
}

#[test]
fn malformed_namespaced_or_non_svg_documents_are_rejected() {
    for svg in [
        r#"<svg/>"#,
        r#"<html xmlns="http://www.w3.org/2000/svg"/>"#,
        r#"<s:svg xmlns:s="http://www.w3.org/2000/svg"/>"#,
        r#"<svg xmlns="http://www.w3.org/2000/svg"><path></svg>"#,
        r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns="http://example.test"/>"#,
    ] {
        assert!(compile(svg).is_err(), "unexpectedly admitted {svg}");
    }
}

#[test]
fn viewport_layout_uses_admitted_root_lengths_and_preserves_exact_bytes() {
    for (attributes, natural_width, display_width, display_height) in [
        (r#"width="360" height="300""#, Some(360), 360, 300),
        (r#"viewBox="0 0 360 300""#, None, 360, 300),
        (
            r#"width="360" height="300" viewBox="0 0 10 10""#,
            Some(360),
            360,
            300,
        ),
        (
            r#"width="480" height="166" viewBox="0 258 480 166""#,
            Some(480),
            480,
            166,
        ),
        (
            r#"width="100%" height="auto" viewBox="0 44 480 166""#,
            None,
            480,
            166,
        ),
        (r#"width="1in" height="0.5in""#, Some(96), 360, 180),
        (r#"width="2.5" height="1.25""#, Some(3), 360, 180),
    ] {
        let svg = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" {attributes}/>"#);
        let artifact = compile(&svg).expect("admitted SVG");
        let layout = artifact.svg.expect("verified viewport");
        assert_eq!(layout.html_width(), natural_width, "{attributes}");
        assert_eq!(
            layout.html_height(display_width),
            Some(display_height),
            "{attributes}"
        );
        assert!(artifact.image.is_none());
        assert_eq!(artifact.bytes, svg.as_bytes());
    }
    for attributes in [
        "",
        r#"width="100%" height="100%""#,
        r#"viewBox="0 0 0 10""#,
        r#"viewBox="0,,0,10,10""#,
        r#"width="NaN" height="20""#,
        r#"viewBox="0 0 inf 10""#,
    ] {
        let svg = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" {attributes}/>"#);
        assert!(
            compile(&svg)
                .expect("sizing does not change admission")
                .svg
                .is_none()
        );
    }
}
