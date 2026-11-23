//! Retained-input Sass compilation exercised through the actual resource port.

use std::collections::{BTreeMap, BTreeSet};

use gazette_adapter_styles::Compiler;
use gazette_core::{
    resource::{Artifact, Error, Limits, Recipe},
    source::{CapturedSource, Content, EntryPath, InventoryBuilder, Sha256Digest, TreeInput},
};
use gazette_ports::resource::ResourceCompiler;

#[path = "../src/guard.rs"]
mod guard;

#[test]
fn a_provider_panic_returns_a_compile_error_and_the_next_request_can_run() {
    let error =
        guard::run::<Artifact>(|| panic!("provider panic with private implementation detail"))
            .expect_err("provider unwind must reject the artifact");
    assert!(matches!(error, Error::Compile(ref message)
        if message == "stylesheet compiler failed while processing the retained input"));
    let source = capture(&[("main.scss", b".card { color: red; }")]);
    let artifact = stylesheet(&source, "main.scss", Limits::default())
        .expect("a rejected request leaves no compiler state for the next one");
    assert!(text(&artifact).contains("red"));
}

fn path(value: &str) -> EntryPath {
    value.parse().expect("logical fixture path")
}

fn capture(files: &[(&str, &[u8])]) -> CapturedSource {
    let mut directories = BTreeSet::new();
    let mut payloads = BTreeMap::new();
    for (name, bytes) in files {
        let mut ancestor = *name;
        while let Some((parent, _)) = ancestor.rsplit_once('/') {
            directories.insert(parent);
            ancestor = parent;
        }
        payloads.insert(path(name), bytes.to_vec());
    }
    let mut builder = InventoryBuilder::new(Default::default());
    for directory in directories {
        builder
            .insert(path(directory), Content::Directory)
            .expect("directory");
    }
    for (path, bytes) in &payloads {
        builder
            .insert(
                path.clone(),
                Content::File {
                    bytes: bytes.len() as u64,
                    sha256: Sha256Digest::of(bytes),
                },
            )
            .expect("file");
    }
    CapturedSource::from_parts(builder.finish(), payloads).expect("capture")
}

fn compile(source: &CapturedSource, recipe: Recipe, limits: Limits) -> Result<Artifact, Error> {
    let files = source
        .inventory()
        .entries()
        .iter()
        .filter_map(|entry| {
            source
                .file(&entry.path)
                .map(|bytes| (entry.path.clone(), bytes))
        })
        .collect();
    let input = TreeInput::new(source.inventory(), files).expect("checked input");
    let compiler: &dyn ResourceCompiler = &Compiler;
    compiler.compile(&input, &recipe, &Default::default(), limits)
}

fn stylesheet(source: &CapturedSource, entry: &str, limits: Limits) -> Result<Artifact, Error> {
    compile(source, Recipe::Stylesheet { entry: path(entry) }, limits)
}

fn text(artifact: &Artifact) -> &str {
    std::str::from_utf8(&artifact.bytes).expect("CSS UTF-8")
}

#[test]
fn compact_css_keeps_loud_notices_strings_custom_properties_and_calc_spacing() {
    let code = r#"/*! Example permission and warranty */
        /* An ordinary comment. */
        .card {
            --label: "two  spaces";
            width: calc(100% - 2rem);
            content: "/* literal, not a comment */";
            background: url("images/card.png");
        }
    "#;
    let artifact = stylesheet(
        &capture(&[("main.scss", code.as_bytes())]),
        "main.scss",
        Limits::default(),
    )
    .expect("compact CSS");
    let css = text(&artifact);
    assert!(artifact.bytes.len() < code.len() * 2 / 3);
    assert_eq!(css.matches("Example permission and warranty").count(), 1);
    assert!(!css.contains("An ordinary comment"));
    assert!(css.contains("two  spaces"));
    assert!(css.contains("calc(100% - 2rem)"));
    assert!(css.contains("/* literal, not a comment */"));
    assert_eq!(
        artifact.references,
        BTreeSet::from(["images/card.png".into()])
    );
}

#[test]
fn real_partial_forward_use_mixins_and_nested_rules_share_one_captured_closure() {
    let source = capture(&[
        ("ui/main.scss", b"@use '../shared/tokens'; @use '../shared/mixins'; .card { color: tokens.$color; @include mixins.size; &:hover { opacity: .5; } }"),
        ("shared/_tokens.scss", b"@forward 'palette';"),
        ("shared/_palette.scss", b"$color: #123456;"),
        ("shared/_mixins.scss", b"@mixin size { padding: 2px * 3; }"),
        ("unused.scss", b"not valid Sass {"),
    ]);
    let artifact =
        stylesheet(&source, "ui/main.scss", Limits::default()).expect("real Sass compiler");
    assert_eq!(artifact.input, source.inventory().identity());
    assert_eq!(
        artifact.dependencies,
        BTreeSet::from([
            path("ui/main.scss"),
            path("shared/_tokens.scss"),
            path("shared/_palette.scss"),
            path("shared/_mixins.scss"),
        ])
    );
    assert!(text(&artifact).contains("#123456"));
    assert!(text(&artifact).contains("padding:6px"));
    assert!(text(&artifact).contains(".card:hover"));
    assert!(!text(&artifact).contains("@use"));
    assert!(!text(&artifact).contains("/Users/"));
    assert!(!text(&artifact).contains("sourceMappingURL"));
    assert_eq!(
        artifact.tool,
        "grass@0.13.4+parser-repairs.2+lightningcss@1.0.0-alpha.70/captured-css-v6-minified"
    );
    let css = capture(&[("generated.css", &artifact.bytes)]);
    stylesheet(&css, "generated.css", Limits::default()).expect("valid emitted CSS");
}

#[test]
fn supports_indented_sass_css_and_ordinary_css_functions() {
    for (entry, code) in [
        ("main.sass", "$width: 5px\n.card\n  width: $width * 2\n"),
        (
            "main.css",
            ".card { width: 10px; --space: 1rem; padding: var(--space); background: linear-gradient(red, blue); }",
        ),
        (
            "main.scss",
            "@use 'sass:math'; .card { width: math.div(20px, 2); }",
        ),
    ] {
        let artifact = stylesheet(
            &capture(&[(entry, code.as_bytes())]),
            entry,
            Limits::default(),
        )
        .expect(entry);
        assert!(text(&artifact).contains("width:10px"));
        assert_eq!(artifact.dependencies, BTreeSet::from([path(entry)]));
        assert!(artifact.references.is_empty());
    }
}

#[test]
fn nested_selector_fallback_does_not_turn_later_grid_spans_into_division() {
    let source = capture(&[(
        "main.scss",
        br"@layer expression {
          .scene { a:not(:where(.action a)) { color: inherit; } }
          .opening { grid-column: 5 / -1; grid-row: 1 / -1; }
          .comparison { grid-area: 1 / 2 / -1 / -1; }
          .arithmetic { width: (12px / 3); height: 2px * 3; }
        }",
    )]);
    let artifact = stylesheet(&source, "main.scss", Limits::default()).expect("valid Sass");
    let css = text(&artifact);
    assert!(css.contains(".scene a:not(:where(.action a))"), "{css}");
    assert!(css.contains("grid-column:5/-1"), "{css}");
    assert!(css.contains("grid-row:1/-1"), "{css}");
    assert!(css.contains("grid-area:1/2/-1/-1"), "{css}");
    assert!(css.contains("width:4px"), "{css}");
    assert!(css.contains("height:6px"), "{css}");
    assert_eq!(artifact.dependencies, BTreeSet::from([path("main.scss")]));
    assert!(artifact.references.is_empty());
}

#[test]
fn input_order_and_repeated_compilation_are_deterministic_and_capture_is_immutable() {
    let files: [(&str, &[u8]); 2] = [
        ("main.scss", b"@use 'color'; .card { color: color.$value; }"),
        ("_color.scss", b"$value: red;"),
    ];
    let first = stylesheet(&capture(&files), "main.scss", Limits::default()).expect("first");
    let mut reversed = files;
    reversed.reverse();
    for _ in 0..4 {
        let next = stylesheet(&capture(&reversed), "main.scss", Limits::default()).expect("repeat");
        assert_eq!(first.bytes, next.bytes);
        assert_eq!(first.dependencies, next.dependencies);
        assert_eq!(first.references, next.references);
        assert_eq!(first.input, next.input);
    }
    let mut body = b".card { color: blue; }".to_vec();
    let retained = capture(&[("main.scss", &body)]);
    body.fill(b'x');
    let artifact = stylesheet(&retained, "main.scss", Limits::default()).expect("retained bytes");
    assert!(text(&artifact).contains("blue"));
    assert_ne!(first.bytes, artifact.bytes);
}

#[test]
fn missing_escaping_absolute_ambient_and_unsupported_sources_fail_closed() {
    for import in [
        "missing",
        "../outside",
        "/tmp/outside",
        "https://example.test/style",
        "@theme/style",
        "present%2Escss",
        "present.scss?raw",
        "present.scss#fragment",
        "present.unsupported",
    ] {
        let code = format!("@use '{import}'; .card {{ color: red; }}");
        let source = capture(&[
            ("main.scss", code.as_bytes()),
            ("present.scss", b"$color: red;"),
        ]);
        assert!(
            stylesheet(&source, "main.scss", Limits::default()).is_err(),
            "{import}"
        );
    }
    for (entry, code) in [
        ("main.scss", b".card { color: ; invalid {".as_slice()),
        ("main.scss", &[0xff]),
        ("main.txt", b".card { color: red; }"),
    ] {
        assert!(
            stylesheet(&capture(&[(entry, code)]), entry, Limits::default()).is_err(),
            "{entry}"
        );
    }
    let source = capture(&[("main.scss", b".card { color: red; }")]);
    assert!(matches!(
        stylesheet(&source, "missing.scss", Limits::default()),
        Err(Error::Input { .. })
    ));
    assert!(matches!(
        compile(
            &source,
            Recipe::BrowserModule {
                entry: path("main.scss"),
                imports: Default::default(),
            },
            Limits::default()
        ),
        Err(Error::Input { .. })
    ));
}

#[test]
fn source_import_validation_uses_decoded_ast_not_comments_or_css_strings() {
    let source = capture(&[
        (
            "main.scss",
            br#"// @use 'missing.scss?raw';
            /* @forward 'missing.unsupported'; */
            @use 'present' as renamed;
            .card { content: "@use 'missing.scss?raw';"; color: renamed.$color; }"#,
        ),
        ("_present.scss", b"$color: red;"),
    ]);
    let artifact =
        stylesheet(&source, "main.scss", Limits::default()).expect("actual parsed imports only");
    assert!(text(&artifact).contains("color:red"));
    assert!(text(&artifact).contains("missing.scss?raw"));
    assert_eq!(
        artifact.dependencies,
        BTreeSet::from([path("main.scss"), path("_present.scss")])
    );

    for code in [
        r"@use 'present.scss\3f raw' as renamed;",
        r"@forward 'present.scss\23 fragment';",
        "@use 'sass:meta' as renamed; @include renamed.load-css('present.scss?raw');",
        "@use 'sass:meta' as *; @include load-css('present.scss?raw');",
    ] {
        let source = capture(&[
            ("main.scss", code.as_bytes()),
            ("present.scss", b".card { color: red; }"),
        ]);
        assert!(
            stylesheet(&source, "main.scss", Limits::default()).is_err(),
            "{code}"
        );
    }
}

#[test]
fn transitive_and_nested_import_literals_are_checked_before_grass_extension_probing() {
    for code in [
        "@forward 'present.unsupported';",
        ".nested { @import 'present.scss?raw'; }",
        "@mixin style { @import 'present.unsupported'; } @include style;",
        "$source: 'present'; @import '#{$source}';",
    ] {
        let source = capture(&[
            ("main.scss", b"@use 'nested/module';"),
            ("nested/_module.scss", code.as_bytes()),
            ("nested/present.scss", b".card { color: red; }"),
        ]);
        let error = stylesheet(&source, "main.scss", Limits::default()).expect_err(code);
        assert_eq!(error.code(), "resource_compile", "{code}: {error}");
    }
    let source = capture(&[
        (
            "main.scss",
            b".first { @import 'present'; } .second { @import 'present'; }",
        ),
        ("_present.scss", b".child { color: red; }"),
    ]);
    let artifact =
        stylesheet(&source, "main.scss", Limits::default()).expect("repeated validated import");
    assert!(text(&artifact).contains(".first .child"));
    assert!(text(&artifact).contains(".second .child"));
    assert_eq!(
        artifact.dependencies,
        BTreeSet::from([path("main.scss"), path("_present.scss")])
    );
}

#[test]
fn declaration_only_partial_is_rejected_by_the_selected_grass_compiler_itself() {
    // Grass 0.13.4 parses imported files as standalone stylesheets, even when
    // their evaluation is nested. Do not wrap source to invent wider Sass support.
    const ENTRY: &[u8] = b".parent { @import 'partial'; }";
    const PARTIAL: &[u8] = b"color: red;";
    #[derive(Debug)]
    struct NativeControl;
    impl grass::Fs for NativeControl {
        fn is_dir(&self, _: &std::path::Path) -> bool {
            false
        }
        fn is_file(&self, path: &std::path::Path) -> bool {
            matches!(path.to_str(), Some("main.scss" | "_partial.scss"))
        }
        fn read(&self, path: &std::path::Path) -> std::io::Result<Vec<u8>> {
            match path.to_str() {
                Some("main.scss") => Ok(ENTRY.to_vec()),
                Some("_partial.scss") => Ok(PARTIAL.to_vec()),
                _ => Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "not retained",
                )),
            }
        }
    }
    let options = grass::Options::default().fs(&NativeControl).quiet(true);
    let native = grass::from_path("main.scss", &options).expect_err("compiler limitation control");
    assert!(native.to_string().contains("expected"));
    let source = capture(&[("main.scss", ENTRY), ("_partial.scss", PARTIAL)]);
    let error =
        stylesheet(&source, "main.scss", Limits::default()).expect_err("same bounded profile");
    assert_eq!(error.code(), "resource_compile");
    assert!(error.to_string().contains("_partial.scss"));
}

#[test]
fn refused_transitive_bytes_return_original_diagnostics_without_partial_artifacts() {
    for (dependency, bytes, reason) in [
        ("broken.scss", &[0xff][..], "stylesheet is not UTF-8"),
        ("broken.sass", &[0xff][..], "stylesheet is not UTF-8"),
        ("broken.css", &[0xff][..], "stylesheet is not UTF-8"),
        ("broken.scss", b".broken {".as_slice(), "expected"),
    ] {
        let code = format!("@use '{dependency}'; .entry {{ color: red; }}");
        let source = capture(&[("main.scss", code.as_bytes()), (dependency, bytes)]);
        let error = stylesheet(&source, "main.scss", Limits::default()).expect_err(dependency);
        assert_eq!(error.code(), "resource_compile");
        let message = error.to_string();
        assert!(message.contains(reason), "{dependency}: {message}");
        assert!(message.contains(dependency), "{dependency}: {message}");
        assert!(!message.contains("captured stylesheet rejected"));
    }
}

#[test]
fn unresolved_imports_and_opaque_resource_rules_remain_refused() {
    for code in [
        "@import 'https://example.test/base.css';",
        "@import 'local.css';",
        ".card { background: image-set(var(--image) 1x); }",
        ".card { --asset: image-set('image.png' 1x); }",
        ".card { --asset: image('image.png'); }",
        "@unknown { .card { color: red; } }",
        ".card { --asset: url(var(--path)); }",
    ] {
        let error = stylesheet(
            &capture(&[("main.scss", code.as_bytes())]),
            "main.scss",
            Limits::default(),
        )
        .expect_err(code);
        assert_eq!(error.code(), "resource_compile", "{code}: {error}");
    }
}

#[test]
fn bounded_font_face_collects_one_explicit_woff2_reference() {
    let code = "@font-face { font-family: 'Example'; src: url('../fonts/example.woff2') format('woff2'); font-style: normal; font-weight: 100 900; font-stretch: 75% 125%; unicode-range: U+0000-00FF; FONT-DISPLAY: SWAP; size-adjust: 102%; ascent-override: 92%; descent-override: normal; line-gap-override: 0%; } .card { font-family: 'Example'; }";
    let artifact = stylesheet(
        &capture(&[("main.css", code.as_bytes())]),
        "main.css",
        Limits::default(),
    )
    .expect("closed WOFF2 font-face profile");
    assert_eq!(
        artifact.references,
        BTreeSet::from(["../fonts/example.woff2".to_owned()])
    );
    assert_eq!(
        stylesheet(
            &capture(&[("main.css", code.as_bytes())]),
            "main.css",
            Limits::default(),
        )
        .expect("deterministic repeat")
        .bytes,
        artifact.bytes
    );
}

#[test]
fn font_face_refuses_ambiguous_sources_and_invalid_descriptors() {
    for code in [
        "@font-face { font-family: Example; src: local(Example), url('font.woff2') format('woff2'); }",
        "@font-face { font-family: Example; src: local(serif); }",
        "@font-face { font-family: Example; src: local(''); }",
        "@font-face { font-family: Example; src: local('   '); }",
        r"@font-face { font-family: Example; src: local('bad\1 name'); }",
        r"@font-face { font-family: Example; src: local('bad\7f name'); }",
        "@font-face { font-family: Example; src: url('font.woff2'); }",
        "@font-face { font-family: Example; src: url('font.woff') format('woff'); }",
        "@font-face { font-family: Example; src: url('a.woff2') format('woff2'), url('b.woff2') format('woff2'); }",
        "@font-face { font-family: Example; src: url('font.woff2') format('woff2') tech(variations); }",
        "@font-face { font-family: Example; src: url('font.woff2') format('woff2'); font-display: invented; }",
        "@font-face { font-family: Example; src: url('font.woff2') format('woff2'); font-display: swap; font-display: block; }",
        "@font-face { font-family: Example; src: url('font.woff2') format('woff2'); size-adjust: normal; }",
        "@font-face { font-family: Example; src: url('font.woff2') format('woff2'); size-adjust: -1%; }",
        "@font-face { font-family: Example; src: url('font.woff2') format('woff2'); descent-override: 10%; descent-override: normal; }",
        "@font-face { font-family: Example; src: url('font.woff2') format('woff2'); line-gap-override: auto; }",
        "@font-face { font-family: Example; font-family: Other; src: url('font.woff2') format('woff2'); }",
        "@font-face { font-family: Example; src: url('font.woff2') format('woff2'); src: url('other.woff2') format('woff2'); }",
        "@namespace svg url('http://www.w3.org/2000/svg');",
        "@unknown font('font.woff2');",
    ] {
        let error = stylesheet(
            &capture(&[("main.css", code.as_bytes())]),
            "main.css",
            Limits::default(),
        )
        .expect_err(code);
        assert_eq!(error.code(), "resource_compile", "{code}: {error}");
    }
}

#[test]
fn image_set_candidates_share_native_url_collection_without_selecting_a_variant() {
    for function in ["image-set", "-webkit-image-set"] {
        let code = format!(
            r#".card {{ background-image: {function}("images/p\6cot.png" 1x type("image/png"), url('images/dense.jpg') 192dpi type("image/jpeg"), "images/future.bin" 3dppx type("image/x-future")); }}
            .duplicate {{ background-image: {function}(url('images/plot.png')); }}"#
        );
        let source = capture(&[("main.css", code.as_bytes())]);
        let artifact = stylesheet(&source, "main.css", Limits::default())
            .expect("every native image option is accounted for");
        assert_eq!(
            artifact.references,
            BTreeSet::from([
                "images/plot.png".to_owned(),
                "images/dense.jpg".to_owned(),
                "images/future.bin".to_owned(),
            ])
        );
        assert_eq!(artifact.dependencies, BTreeSet::from([path("main.css")]));
        let native = grass::from_string(
            code,
            &grass::Options::default()
                .quiet(true)
                .allows_charset(false)
                .style(grass::OutputStyle::Compressed),
        )
        .expect("native compiler output");
        assert_eq!(
            artifact.bytes,
            native.as_bytes(),
            "no second serializer or variant filtering"
        );
        let repeated = stylesheet(&source, "main.css", Limits::default()).expect("retained repeat");
        assert_eq!(repeated.bytes, artifact.bytes);
        assert_eq!(repeated.references, artifact.references);
        assert!(matches!(
            stylesheet(
                &source,
                "main.css",
                Limits {
                    output_bytes: artifact.bytes.len() - 1,
                    ..Limits::default()
                }
            ),
            Err(Error::Limit("stylesheet output bytes"))
        ));
    }
    let artifact = stylesheet(&capture(&[("main.css",
        b".card { background: image-set(linear-gradient(red, blue) 1x, 'zero.png' 0x, 'print.png' 40dpcm); }")]),
        "main.css", Limits::default()).expect("native gradient and nonnegative resolution");
    assert_eq!(
        artifact.references,
        BTreeSet::from(["zero.png".to_owned(), "print.png".to_owned()])
    );
}

#[test]
fn image_set_invalid_or_unresolved_options_never_return_a_partial_artifact() {
    for options in [
        "'good.png' 1x, '' 2x",
        r"'good.png' 1x, 'bad\a .png' 2x",
        "'good.png' 1x, 'bad.png' -1x",
        "'good.png' 1x, image-set('nested.png' 1x) 2x",
        "'good.png' 1x, none 2x",
        "'good.png' 1x, var(--other) 2x",
    ] {
        let code = format!(".card {{ background: image-set({options}); }}");
        let error = stylesheet(
            &capture(&[("main.css", code.as_bytes())]),
            "main.css",
            Limits::default(),
        )
        .expect_err(options);
        assert_eq!(error.code(), "resource_compile", "{options}: {error}");
    }
}

#[test]
fn native_url_values_are_decoded_deduplicated_and_distinct_from_sass_dependencies() {
    let source = capture(&[
        ("main.scss", br#"@use 'nested/part'; .main { background: url('images/p\6cot.png'); --asset: url('images/plot.png'); content: 'url(not-a-reference.png)'; }"#),
        ("nested/_part.scss", br#"/* url(comment-only.png) */ @media screen { .nested { background: url('images/plot.png'); cursor: url('../cursors/hand.cur'), auto; } }"#),
    ]);
    let artifact = stylesheet(&source, "main.scss", Limits::default())
        .expect("actual Sass and native CSS URL visitation");
    assert_eq!(
        artifact.references,
        BTreeSet::from([
            "../cursors/hand.cur".to_owned(),
            "images/plot.png".to_owned(),
        ])
    );
    assert_eq!(
        artifact.dependencies,
        BTreeSet::from([path("main.scss"), path("nested/_part.scss"),])
    );
    assert!(text(&artifact).contains("url(not-a-reference.png)"));
    assert!(text(&artifact).contains("../cursors/hand.cur"));
    let again = stylesheet(&source, "main.scss", Limits::default()).expect("repeat retained input");
    assert_eq!(again.references, artifact.references);
    assert_eq!(again.bytes, artifact.bytes);
    assert_eq!(again.dependencies, artifact.dependencies);
    assert!(matches!(
        stylesheet(
            &source,
            "main.scss",
            Limits {
                output_bytes: artifact.bytes.len() - 1,
                ..Limits::default()
            }
        ),
        Err(Error::Limit("stylesheet output bytes"))
    ));
}

#[test]
fn url_collection_preserves_compiler_css_without_claiming_destination_admission() {
    let code = ".card { background: url(data:image/png;base64,YQ==); filter: url(#mask); --asset: url(https://example.invalid/image.png); }";
    let artifact = stylesheet(
        &capture(&[("main.scss", code.as_bytes())]),
        "main.scss",
        Limits::default(),
    )
    .expect("references only, no acquisition");
    let native = grass::from_string(
        code.to_owned(),
        &grass::Options::default()
            .quiet(true)
            .allows_charset(false)
            .style(grass::OutputStyle::Compressed),
    )
    .expect("native compiler output control");
    assert_eq!(
        artifact.bytes,
        native.as_bytes(),
        "visitor is not another CSS serializer"
    );
    assert_eq!(
        artifact.references,
        BTreeSet::from([
            "#mask".to_owned(),
            "data:image/png;base64,YQ==".to_owned(),
            "https://example.invalid/image.png".to_owned(),
        ]),
        "application must separately bind or reject every emitted reference"
    );
}

#[test]
fn empty_and_control_bearing_decoded_css_urls_fail_before_artifact_return() {
    for value in ["", "   ", r"image\7f .png", r"image\a .png"] {
        let code = format!(".card {{ background: url('{value}'); }}");
        let error = stylesheet(
            &capture(&[("main.scss", code.as_bytes())]),
            "main.scss",
            Limits::default(),
        )
        .expect_err(value);
        assert_eq!(error.code(), "resource_compile");
    }
}

#[test]
fn random_and_unique_id_never_produce_successful_nondeterministic_css() {
    for code in [
        ".card { z-index: random(100); }",
        ".card { --id: #{unique-id()}; }",
        "@use 'sass:math'; .card { z-index: math.random(100); }",
        "@use 'sass:string'; .card { --id: #{string.unique-id()}; }",
    ] {
        assert!(
            stylesheet(
                &capture(&[("main.scss", code.as_bytes())]),
                "main.scss",
                Limits::default()
            )
            .is_err(),
            "{code}"
        );
    }
}

#[test]
fn all_declared_inputs_and_final_output_share_explicit_limits() {
    let source = capture(&[
        ("main.scss", b".card { color: red; }"),
        ("unused/deep/file.scss", b"unused bytes still count"),
    ]);
    for inputs in [
        gazette_core::source::Limits {
            entries: 1,
            ..Limits::default().inputs
        },
        gazette_core::source::Limits {
            depth: 1,
            ..Limits::default().inputs
        },
        gazette_core::source::Limits {
            file_bytes: 1,
            ..Limits::default().inputs
        },
        gazette_core::source::Limits {
            total_bytes: 1,
            ..Limits::default().inputs
        },
    ] {
        assert!(matches!(
            stylesheet(
                &source,
                "main.scss",
                Limits {
                    inputs,
                    ..Limits::default()
                }
            ),
            Err(Error::Limit(_))
        ));
    }
    let normal = stylesheet(&source, "main.scss", Limits::default()).expect("output");
    let exact = Limits {
        output_bytes: normal.bytes.len(),
        ..Limits::default()
    };
    assert_eq!(
        stylesheet(&source, "main.scss", exact)
            .expect("exact ceiling")
            .bytes,
        normal.bytes
    );
    assert!(matches!(
        stylesheet(
            &source,
            "main.scss",
            Limits {
                output_bytes: normal.bytes.len() - 1,
                ..exact
            }
        ),
        Err(Error::Limit("stylesheet output bytes"))
    ));
}

// Run hostile parser inputs out of process: a regression must fail with a bound,
// rather than hang the entire test runner or unwind across the resource port.
#[test]
fn malformed_stylesheets_fail_without_panicking_or_hanging() {
    const CHILD: &str = "GAZETTE_STYLES_MALFORMED_CHILD";
    if std::env::var_os(CHILD).is_some() {
        for (entry, source) in [
            ("bad.sass", "a\n  x: 1 /*"),
            (
                "bad.scss",
                "@use 'sass:selector'; a { x: selector.simple-selectors('>'); }",
            ),
            (
                "bad.css",
                "@-moz-document url-prefix() { a { color:red; } }",
            ),
        ] {
            assert!(
                stylesheet(
                    &capture(&[(entry, source.as_bytes())]),
                    entry,
                    Limits::default()
                )
                .is_err(),
                "{entry} must fail closed"
            );
        }
        return;
    }
    let mut child = std::process::Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "--exact",
            "malformed_stylesheets_fail_without_panicking_or_hanging",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .spawn()
        .expect("parser worker");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    loop {
        if let Some(status) = child.try_wait().expect("worker status") {
            assert!(status.success(), "malformed input panicked or was accepted");
            break;
        }
        if std::time::Instant::now() >= deadline {
            child.kill().expect("terminate stuck parser");
            child.wait().expect("reap parser");
            panic!("malformed stylesheet parser exceeded deadline");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn local_metric_fallback_faces_preserve_names_without_retained_url_claims() {
    let code = "@font-face { font-family: 'Instrument Sans Fallback'; src: local('Arial'), local('Liberation Sans'); font-style: normal; font-weight: 400; size-adjust: 102.63%; ascent-override: 94.514%; descent-override: 24.359%; line-gap-override: 0%; } @font-face { font-family: 'Instrument Sans Fallback'; src: local('Arial Bold'), local('Liberation Sans Bold'); font-style: normal; font-weight: 500 700; size-adjust: 98.46%; ascent-override: 98.517%; descent-override: 25.391%; line-gap-override: 0%; }";
    let source = capture(&[("fallback.css", code.as_bytes())]);
    let artifact = stylesheet(&source, "fallback.css", Limits::default()).unwrap();
    assert!(artifact.references.is_empty());
    let with_url = format!(
        "{code} @font-face {{ font-family: Actual; src: url('actual.woff2') format('woff2'); }}"
    );
    let mixed_rules = stylesheet(
        &capture(&[("faces.css", with_url.as_bytes())]),
        "faces.css",
        Limits::default(),
    )
    .unwrap();
    assert_eq!(
        mixed_rules.references,
        BTreeSet::from(["actual.woff2".into()])
    );
    let escaped = r"@font-face { font-family: Fallback; src: local(Arial\20 Bold); }";
    assert!(
        stylesheet(
            &capture(&[("escaped.css", escaped.as_bytes())]),
            "escaped.css",
            Limits::default()
        )
        .unwrap()
        .references
        .is_empty()
    );
    let css = text(&artifact);
    for value in [
        "Liberation Sans",
        "Arial Bold",
        "500 700",
        "102.63%",
        "98.46%",
        "94.514%",
        "98.517%",
    ] {
        assert!(css.contains(value), "{css}");
    }
    assert_eq!(
        stylesheet(&source, "fallback.css", Limits::default())
            .unwrap()
            .bytes,
        artifact.bytes
    );
    for names in [
        vec!["local(Example)"; 17].join(","),
        format!("local('{}')", "a".repeat(257)),
        format!("local(' {}')", "a".repeat(256)),
        format!("local('{} ')", "a".repeat(256)),
    ] {
        let code = format!("@font-face {{ font-family: Example; src: {names}; }}");
        assert!(
            stylesheet(
                &capture(&[("main.css", code.as_bytes())]),
                "main.css",
                Limits::default()
            )
            .is_err()
        );
    }
}
