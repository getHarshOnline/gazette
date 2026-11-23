//! Site-scoped byte transport through actual configuration and retained corpus APIs.

use gazette_compiler::{
    authored::{self, AssetError},
    corpus::{Input, ParsedCorpus},
    workspace,
};
use gazette_core::{
    publication::{Key, content::Limits},
    source::{EntryPath, Sha256Digest},
};

fn key(value: &str) -> Key {
    value.parse().expect("exact fixture identity")
}

fn path(value: &str) -> EntryPath {
    value.parse().expect("literal retained entry")
}

fn configuration(assets: &str) -> String {
    format!(
        "[[principals]]\nid='principal:publisher'\nkind='person'\n\
         [[sites]]\nid='site:one'\npublisher='principal:publisher'\ndeployable=false\n\
         [[sites]]\nid='site:two'\npublisher='principal:publisher'\ndeployable=false\n\
         [[sources]]\nid='source:private'\nlocation={{kind='repository',root='/private/native',commit='1111111111111111111111111111111111111111',subdirectory='docs'}}\n\
         [[brands]]\nid='brand:publication'\nname='Publication'\n\
         [[brands.publications]]\nsite='site:one'\npublic_base_url='https://example.test/mount/'\n\
         authored=[{{source='source:private',target='authored/one'{assets}}}]\n\
         [[brands.publications]]\nsite='site:two'\npublic_base_url='https://other.test/'\n\
         authored=[{{source='source:private',target='authored/two',static_assets={{route_prefix='/foreign/'}}}}]\n"
    )
}

const STATIC: &str = ",static_assets={route_prefix='/media/'}";

fn compile(source: &str) -> workspace::Compilation {
    workspace::compile(source, Default::default()).expect("real workspace compiler")
}

fn corpus<'a>(files: &[(&str, &'a [u8])]) -> ParsedCorpus<'a> {
    ParsedCorpus::parse(
        files.iter().map(|(name, bytes)| Input {
            path: path(name),
            bytes,
        }),
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("retained bytes without filesystem access")
}

#[test]
fn encoded_download_extensions_do_not_bypass_missing_source_or_target_checks() {
    let workspace = compile(&configuration(STATIC));
    for source_path in ["authored/one/page.md", "unbound/page.md"] {
        for destination in ["missing.pdf", "missing.p%64f", "missing.%70df"] {
            let text = format!(
                "+++\n[content]\nid='content:page'\nsite='site:one'\ntitle='Page'\n\
                 lifecycle='draft'\nstructure={{kind='page',family={{kind='article'}}}}\n+++\n\
                 [download]({destination})\n"
            );
            let input = corpus(&[(source_path, text.as_bytes())]);
            let resolved = authored::resolve(&workspace, &input, Limits::default())
                .expect("retained framed document");
            let error = resolved
                .project_page(
                    &key("content:page"),
                    &gazette_core::raster::unavailable,
                    &gazette_adapter_markdown::Comrak,
                )
                .expect_err("equivalent asset spelling must fail closed");
            let authored::ProjectionError::Document { origin, error } = error else {
                panic!("expected source-bound Markdown failure");
            };
            assert_eq!(origin.path, path(source_path));
            let gazette_compiler::document::Error::Markdown { error, .. } = *error else {
                panic!("expected Markdown reference diagnostic");
            };
            assert_eq!(error.code(), "markdown_link_resolution");
            let gazette_compiler::markdown::Error::LinkResolution { reason, .. } = error else {
                panic!("expected exact link failure");
            };
            assert_eq!(
                reason,
                if source_path.starts_with("unbound/") {
                    "static reference has no configured source binding"
                } else {
                    "static target is absent or uncompiled in its configured source"
                }
            );
        }
        for destination in ["https://external.test/missing.p%64f", "/missing.%70df"] {
            let text = format!(
                "+++\n[content]\nid='content:page'\nsite='site:one'\ntitle='Page'\n\
                 lifecycle='draft'\nstructure={{kind='page',family={{kind='article'}}}}\n+++\n\
                 [download]({destination})\n"
            );
            let input = corpus(&[(source_path, text.as_bytes())]);
            let resolved =
                authored::resolve(&workspace, &input, Limits::default()).expect("retained source");
            let page = resolved
                .project_page(
                    &key("content:page"),
                    &gazette_core::raster::unavailable,
                    &gazette_adapter_markdown::Comrak,
                )
                .expect("external and root-relative references remain outside this resolver");
            assert!(page.body_html.contains(&format!("href=\"{destination}\"")));
            assert!(page.data.assets.is_empty());
        }
    }
}

#[test]
fn optional_transport_is_deserialized_once_and_requires_a_directory_mount() {
    let omitted = compile(&configuration(""));
    assert!(
        omitted
            .workspace
            .publication_for(&key("site:one"))
            .expect("publication")
            .authored
            .first()
            .expect("binding")
            .static_assets
            .is_none()
    );
    let configured = compile(&configuration(STATIC));
    let binding = configured
        .workspace
        .publication_for(&key("site:one"))
        .expect("publication")
        .authored
        .first()
        .expect("binding");
    assert_eq!(
        binding
            .static_assets
            .as_ref()
            .expect("explicit transport")
            .route_prefix
            .as_str(),
        "/media/"
    );
    let invalid = workspace::compile(
        &configuration(",static_assets={route_prefix='/media/file.png'}"),
        Default::default(),
    )
    .expect_err("file route cannot be a transport directory");
    assert!(matches!(invalid, workspace::Error::Invalid(errors)
        if errors.iter().any(|error| error.code == "workspace_asset_route")));
    for declaration in [
        ",static_assets={}",
        ",static_assets={route_prefix='/media/',unknown=true}",
    ] {
        assert!(workspace::compile(&configuration(declaration), Default::default()).is_err());
    }
}

#[test]
fn one_cached_site_plan_preserves_bytes_and_excludes_native_coordinates_from_public_fields() {
    // A filename is transport classification, deliberately not image decoding or MIME proof.
    let bytes = b"opaque retained fixture\0\xff";
    let input = corpus(&[
        ("authored/one/使用/first image.PNG", bytes),
        ("authored/two/foreign.pdf", b"foreign"),
    ]);
    let workspace = compile(&configuration(STATIC));
    let resolved =
        authored::resolve(&workspace, &input, Limits::default()).expect("structural result");
    let plan = resolved
        .assets_for(&key("site:one"))
        .expect("selected site plan");
    assert!(std::ptr::eq(
        plan,
        resolved.assets_for(&key("site:one")).expect("cached plan")
    ));
    assert_eq!(plan.entries().len(), 1);
    assert!(plan.uncompiled().is_empty());
    let source = path("authored/one/使用/first image.PNG");
    let asset = plan.get(&source).expect("exact source asset");
    assert_eq!(
        asset.url,
        "https://example.test/mount/media/%E4%BD%BF%E7%94%A8/first%20image.PNG"
    );
    assert_eq!(asset.output.as_str(), "media/使用/first image.PNG");
    assert_eq!(asset.content(), bytes);
    assert_eq!(asset.content().as_ptr(), bytes.as_ptr());
    assert_eq!(asset.bytes, bytes.len() as u64);
    assert_eq!(asset.sha256, Sha256Digest::of(bytes));
    assert_eq!(asset.source().path, source);
    assert_eq!(asset.source().source_sha256, asset.sha256);
    assert!(asset.source().span.is_none());
    let value = serde_json::to_value(asset).expect("public asset projection");
    assert_eq!(value["provenance"]["site"], "site:one");
    assert!(value["provenance"].get("project").is_none());
    for field in ["content", "source"] {
        assert!(value.get(field).is_none());
    }
    let serialized = value.to_string();
    for private in [
        "/private/native",
        "source:private",
        "authored/one",
        "1111111111111111111111111111111111111111",
    ] {
        assert!(!serialized.contains(private), "{private}");
    }
    assert!(plan.get(&path("first image.PNG")).is_none());
    let foreign = resolved
        .assets_for(&key("site:two"))
        .expect("independent foreign plan");
    assert_eq!(foreign.entries().len(), 1);
    assert!(foreign.get(&source).is_none());
}

#[test]
fn unsupported_unconfigured_and_unbound_entries_remain_explicit_obligations() {
    let input = corpus(&[
        ("authored/one/readme.txt", b"download"),
        ("authored/one/custom.js", b"javascript"),
        (
            "authored/one/notebook.ipynb",
            br#"{"nbformat":4,"nbformat_minor":5,"metadata":{},"cells":[]}"#,
        ),
        ("authored/one/active.svg", b"svg"),
        ("authored/two/foreign.js", b"foreign capability"),
        ("unbound/secret.pdf", b"unbound"),
    ]);
    for (configuration, expected) in [
        (
            configuration(STATIC),
            vec![
                "authored/one/active.svg",
                "authored/one/custom.js",
                "authored/one/notebook.ipynb",
                "unbound/secret.pdf",
            ],
        ),
        (
            configuration(""),
            vec![
                "authored/one/active.svg",
                "authored/one/custom.js",
                "authored/one/notebook.ipynb",
                "authored/one/readme.txt",
                "unbound/secret.pdf",
            ],
        ),
    ] {
        let workspace = compile(&configuration);
        let resolved =
            authored::resolve(&workspace, &input, Limits::default()).expect("source evidence");
        let plan = resolved
            .assets_for(&key("site:one"))
            .expect("accounted obligations");
        let uncompiled: Vec<_> = plan.uncompiled().iter().map(EntryPath::as_str).collect();
        assert_eq!(uncompiled, expected);
    }
}

#[test]
fn unknown_site_missing_base_and_unsafe_literal_routes_fail_without_partial_plan() {
    let input = corpus(&[("authored/one/image.png", b"image")]);
    let workspace = compile(&configuration(STATIC));
    let resolved =
        authored::resolve(&workspace, &input, Limits::default()).expect("resolved corpus");
    assert!(
        matches!(resolved.assets_for(&key("site:absent")), Err(AssetError::Site(site)) if site == key("site:absent"))
    );
    let missing_base = compile(
        &configuration(STATIC).replace("public_base_url='https://example.test/mount/'\n", ""),
    );
    let resolved = authored::resolve(&missing_base, &input, Limits::default())
        .expect("base not needed for empty content graph");
    assert!(
        matches!(resolved.assets_for(&key("site:one")), Err(AssetError::Base(site)) if site == key("site:one"))
    );
    for filename in ["literal%61.png", "query?.png", "hash#.png"] {
        let name = format!("authored/one/{filename}");
        let input = corpus(&[(name.as_str(), b"retained")]);
        let resolved = authored::resolve(&workspace, &input, Limits::default())
            .expect("literal source accepted");
        assert!(
            matches!(resolved.assets_for(&key("site:one")), Err(AssetError::Route(source)) if source == path(&name))
        );
    }
}

#[test]
fn selected_asset_record_and_derived_text_limits_apply_lazily_and_independently() {
    let input = corpus(&[("authored/one/a.txt", b"a"), ("authored/two/b.txt", b"b")]);
    let workspace = compile(&configuration(STATIC));
    for (limits, resource) in [
        (
            Limits {
                nodes: 0,
                ..Limits::default()
            },
            "records",
        ),
        (
            Limits {
                text_bytes: 2,
                ..Limits::default()
            },
            "text bytes",
        ),
    ] {
        let resolved = authored::resolve(&workspace, &input, limits)
            .expect("empty semantic graph within limits");
        assert!(
            matches!(resolved.assets_for(&key("site:one")), Err(AssetError::Limit(actual)) if actual == resource)
        );
    }
    let text_bytes = 2 * "authored/one/a.txt".len()
        + "https://example.test/mount/".len()
        + "/media/a.txt".len()
        + "media/a.txt".len();
    let exact = Limits {
        nodes: 1,
        text_bytes,
        ..Limits::default()
    };
    let resolved = authored::resolve(&workspace, &input, exact).expect("empty content graph");
    assert_eq!(
        resolved
            .assets_for(&key("site:one"))
            .expect("exact asset bound excludes foreign site")
            .entries()
            .len(),
        1
    );
    let below = authored::resolve(
        &workspace,
        &input,
        Limits {
            text_bytes: text_bytes - 1,
            ..exact
        },
    )
    .expect("empty content graph");
    assert!(matches!(
        below.assets_for(&key("site:one")),
        Err(AssetError::Limit("text bytes"))
    ));
}

#[test]
fn media_transport_preserves_bytes_and_resolves_only_configured_retained_media() {
    let workspace = compile(&configuration(STATIC));
    let unselected = compile(&configuration(""));
    // Transport is deliberately opaque: this does not claim media decoding or qualification.
    let bytes = b"retained opaque video bytes";
    for extension in ["mp4", "Mp4", "webm", "WeBm", "vtt", "VtT"] {
        let media = format!("media/film.{extension}");
        let source = format!("authored/one/{media}");
        let text = format!(
            "+++\n[content]\nid='content:page'\nsite='site:one'\ntitle='Page'\nlifecycle='draft'\nstructure={{kind='page',family={{kind='article'}}}}\n+++\n[Film]({media})\n"
        );
        let input = corpus(&[("authored/one/page.md", text.as_bytes()), (&source, bytes)]);
        let resolved = authored::resolve(&workspace, &input, Limits::default()).unwrap();
        let page = resolved
            .project_page(
                &key("content:page"),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .unwrap();
        assert!(
            page.body_html
                .contains(&format!("https://example.test/mount/media/{media}"))
        );
        assert_eq!(page.data.assets.len(), 1);
        assert_eq!(page.data.assets[0].content(), bytes);
        assert_eq!(page.data.assets[0].sha256, Sha256Digest::of(bytes));
        let resolved = authored::resolve(&unselected, &input, Limits::default()).unwrap();
        assert!(
            resolved
                .project_page(
                    &key("content:page"),
                    &gazette_core::raster::unavailable,
                    &gazette_adapter_markdown::Comrak
                )
                .is_err()
        );
    }
}
