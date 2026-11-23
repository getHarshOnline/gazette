use super::*;
use gazette_core::{raster::Dimensions, resource::ImageLayout};
use std::collections::BTreeMap;

fn declaration(role: &str, name: &str, resize: &str) -> String {
    format!(
        "[[resources]]\n{role}=true\ninputs=['image.png']\noutput='/images/{name}.webp'\nbuild={{kind='raster-image',entry='image.png',input_format='png',apply_orientation=false,resize={{{resize}}},encoding={{format='webp'}}}}\n"
    )
}

fn declarations() -> String {
    format!(
        "{}{}[[images]]\ndefault='image.png'\nsizes='100vw'\n",
        declaration("rendition", "full", "operation='fit-width',width=7"),
        declaration("responsive", "small", "operation='fit-width',width=3")
    )
}

fn files(declarations: &str) -> Vec<(String, Vec<u8>)> {
    vec![
        (
            "authored/one/page.md".into(),
            format!(
                "{}\n![Figure](image.png)\n",
                page_with("content:page", "/page/", declarations)
            )
            .into_bytes(),
        ),
        (
            "authored/one/image.png".into(),
            b"native geometry supplied separately".to_vec(),
        ),
    ]
}

#[test]
fn responsive_declarations_refuse_missing_fallback_crop_orientation_and_ambiguous_roles() {
    let valid = declarations();
    for invalid in [
        valid.replace("rendition=true", "rendition=false"),
        valid.replace("responsive=true", "responsive=true\nrendition=true"),
        valid.replacen("apply_orientation=false", "apply_orientation=true", 1),
        valid.replace(
            "operation='fit-width',width=3",
            "operation='fill',width=3,height=3",
        ),
        valid.replace(
            "operation='fit-width',width=7",
            "operation='scale',width=7,height=5",
        ),
        valid.replace("sizes='100vw'", "sizes='<script>'"),
    ] {
        let corpus_files = files(&invalid);
        let corpus = parse(&corpus_files);
        let compilation = compilation();
        assert!(
            authored::resolve(&compilation, &corpus, Default::default()).is_err(),
            "{invalid}"
        );
    }
}

#[test]
fn responsive_projection_requires_sizes_distinct_verified_widths_and_preserves_rounding() {
    for (sizes, small, expected) in [
        (true, Some((3, 2)), true),
        (false, Some((3, 2)), false),
        (true, None, false),
        (true, Some((7, 5)), false),
    ] {
        let declarations = declarations();
        let declarations = if sizes {
            declarations
        } else {
            declarations.replace("sizes='100vw'\n", "")
        };
        let files = files(&declarations);
        let corpus = parse(&files);
        let compilation = compilation();
        let resolved = resolve(&compilation, &corpus);
        let mut dimensions = BTreeMap::from([(
            "https://example.test/images/full.webp".into(),
            ImageLayout::Raster(Dimensions {
                width: 7,
                height: 5,
            }),
        )]);
        if let Some((width, height)) = small {
            dimensions.insert(
                "https://example.test/images/small.webp".into(),
                ImageLayout::Raster(Dimensions { width, height }),
            );
        }
        let page = resolved.project_page_with_renditions(
            &key("content:page"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
            None,
            None,
            &dimensions,
        );
        assert_eq!(page.is_ok(), expected);
        if let Ok(page) = page {
            assert!(
                page.body_html
                    .contains("small.webp 3w, https://example.test/images/full.webp 7w")
            );
            assert!(page.body_html.contains("width=\"7\" height=\"5\""));
        }
    }
}
