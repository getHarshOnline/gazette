//! Retained leaf packages compose existing authored resource owners without filesystem discovery.

#[path = "page_packages/figure_caption.rs"]
mod figure_caption;
#[path = "page_packages/images.rs"]
mod images;
#[path = "page_packages/responsive.rs"]
mod responsive;

use gazette_compiler::{
    authored::{self, PageCompanionKind, ResolvedAuthored, Selection, SelectionError},
    corpus::{File, Input, ParsedCorpus},
    workspace,
};
use gazette_core::{
    publication::{
        Key,
        content::{self, Disposition},
    },
    resource::Recipe,
    source::{EntryPath, Sha256Digest},
};

fn key(value: &str) -> Key {
    value.parse().expect("fixture identity")
}

fn path(value: &str) -> EntryPath {
    value.parse().expect("fixture path")
}

fn compilation() -> workspace::Compilation {
    compilation_with_base("https://example.test/")
}

fn compilation_with_base(public_base_url: &str) -> workspace::Compilation {
    workspace::compile(
        &"[[principals]]\nid='owner:one'\nkind='person'\n\
         [[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n\
         [[sources]]\nid='source:one'\nlocation={kind='repository',root='/fixture/one',commit='1111111111111111111111111111111111111111',subdirectory='input'}\n\
         [[sources]]\nid='source:two'\nlocation={kind='repository',root='/fixture/two',commit='2222222222222222222222222222222222222222',subdirectory='input'}\n\
         [[brands]]\nid='brand:one'\nname='Example'\n\
         [[brands.publications]]\nsite='site:one'\npublic_base_url='https://example.test/'\n\
         authored=[{source='source:one',target='authored/one',static_assets={route_prefix='/media/'}}]\n\
         [[projects]]\nid='project:other'\nlifecycle='active'\nvisibility='public'\navailability='available'\n\
         [[placements]]\nrole='primary'\nsite='site:one'\nproject='project:other'\nrender=true\ndiscoverable=true\n\
         documents={source='source:two',target='authored/one-other',static_assets={route_prefix='/other-media/'}}\n\
         native_markdown={route_prefix='/other-docs/',lifecycle='published'}\n".replace("https://example.test/", public_base_url),
        Default::default(),
    )
    .expect("two exact nonoverlapping content bindings")
}

fn page(id: &str, route: &str) -> String {
    format!(
        "+++\n[content]\nid='{id}'\nsite='site:one'\ntitle='Page'\n\
         lifecycle='published'\nroute={{path='{route}'}}\n\
         structure={{kind='page',family={{kind='article'}}}}\n+++\n# Page\n"
    )
}

fn page_with(id: &str, route: &str, root: &str) -> String {
    format!(
        "+++\n{root}[content]\nid='{id}'\nsite='site:one'\ntitle='Page'\n\
         lifecycle='published'\nroute={{path='{route}'}}\n\
         structure={{kind='page',family={{kind='article'}}}}\n+++\n# Page\n"
    )
}

fn parse<'a>(files: &'a [(String, Vec<u8>)]) -> ParsedCorpus<'a> {
    ParsedCorpus::parse(
        files.iter().map(|(name, bytes)| Input {
            path: path(name),
            bytes,
        }),
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("retained package corpus")
}

fn resolve<'a, 's>(
    compilation: &'a workspace::Compilation,
    corpus: &'a ParsedCorpus<'s>,
) -> ResolvedAuthored<'a, 's> {
    authored::resolve(compilation, corpus, content::Limits::default())
        .expect("resolved retained package")
}

fn selection(resolved: &ResolvedAuthored<'_, '_>, disposition: Disposition) -> Selection {
    Selection {
        content: resolved
            .content()
            .graph()
            .nodes()
            .map(|node| (node.id.clone(), disposition))
            .collect(),
        assets: resolved
            .corpus()
            .files()
            .iter()
            .filter(|(_, file)| matches!(file, File::Asset(_) | File::Notebook(_)))
            .map(|(path, _)| (path.clone(), disposition))
            .collect(),
    }
}

#[test]
fn leaf_package_reuses_resource_notebook_and_static_owners_without_promoting_dependencies() {
    let files = vec![
        (
            "authored/one/guide/index.md".into(),
            page("content:guide", "/guide/").into_bytes(),
        ),
        (
            "authored/one/guide/main.ts".into(),
            b"import './lib/dep.ts';".to_vec(),
        ),
        (
            "authored/one/guide/lib/dep.ts".into(),
            b"export const answer = 42;".to_vec(),
        ),
        (
            "authored/one/guide/main.scss".into(),
            b"@use 'styles/tokens';".to_vec(),
        ),
        (
            "authored/one/guide/styles/_tokens.scss".into(),
            b"$ink: #123456;".to_vec(),
        ),
        (
            "authored/one/guide/analysis.ipynb".into(),
            br#"{"nbformat":4,"nbformat_minor":5,"metadata":{},"cells":[]}"#.to_vec(),
        ),
        (
            "authored/one/guide/image.png".into(),
            b"retained image".to_vec(),
        ),
    ];
    let corpus = parse(&files);
    let compilation = compilation();
    let resolved = resolve(&compilation, &corpus);
    let id = key("content:guide");

    let package = resolved.page_package(&id).expect("leaf package");
    assert_eq!(package.root(), Some(&path("guide")));
    assert_eq!(package.companions().len(), 6);
    assert_eq!(resolved.page_packages().len(), 1);
    assert_eq!(
        package
            .companions()
            .iter()
            .map(|entry| (
                entry.path().as_str(),
                entry.relative().as_str(),
                entry.kind()
            ))
            .collect::<Vec<_>>(),
        [
            (
                "authored/one/guide/analysis.ipynb",
                "analysis.ipynb",
                PageCompanionKind::Notebook
            ),
            (
                "authored/one/guide/image.png",
                "image.png",
                PageCompanionKind::Static
            ),
            (
                "authored/one/guide/lib/dep.ts",
                "lib/dep.ts",
                PageCompanionKind::Program
            ),
            (
                "authored/one/guide/main.scss",
                "main.scss",
                PageCompanionKind::Stylesheet
            ),
            (
                "authored/one/guide/main.ts",
                "main.ts",
                PageCompanionKind::Program
            ),
            (
                "authored/one/guide/styles/_tokens.scss",
                "styles/_tokens.scss",
                PageCompanionKind::Stylesheet
            ),
        ]
    );
    let resources = resolved.resources(&id);
    assert_eq!(resources.len(), 2);
    let script = resources
        .iter()
        .find(|resource| {
            matches!(resource.build, Recipe::BrowserModule { .. })
                && resource.build.entry() == &path("guide/main.ts")
        })
        .expect("browser module");
    assert_eq!(script.output.as_str(), "/guide/main.js");
    assert_eq!(
        script
            .inputs
            .iter()
            .map(EntryPath::as_str)
            .collect::<Vec<_>>(),
        ["guide/lib/dep.ts", "guide/main.ts"]
    );
    assert!(
        resources
            .iter()
            .all(|resource| resource.build.entry() != &path("guide/lib/dep.ts")),
        "the nested dependency remains an owned input, not a public entry"
    );
    let style = resources
        .iter()
        .find(|resource| matches!(resource.build, Recipe::Stylesheet { .. }))
        .expect("stylesheet");
    assert_eq!(style.output.as_str(), "/guide/main.css");
    assert_eq!(
        style
            .inputs
            .iter()
            .map(EntryPath::as_str)
            .collect::<Vec<_>>(),
        ["guide/main.scss", "guide/styles/_tokens.scss"]
    );
    assert_eq!(
        resolved.notebooks(&id).expect("attached retained notebook")[0].path,
        path("authored/one/guide/analysis.ipynb")
    );
    let assets = resolved.assets_for(&key("site:one")).expect("asset plan");
    let image = assets
        .get(&path("authored/one/guide/image.png"))
        .expect("package static transport");
    assert_eq!(image.url, "https://example.test/guide/image.png");
    assert_eq!(image.output.as_str(), "guide/image.png");
    let page = resolved
        .project_page(
            &id,
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("package page projection");
    assert_eq!(page.data.assets.len(), 1);
    assert_eq!(page.data.assets[0].url, image.url);
}

#[test]
fn explicit_page_resource_declarations_remain_authoritative() {
    let metadata = "[[resources]]\ninputs=['guide/main.ts','guide/lib/dep.ts']\n\
        build={kind='browser-module',entry='guide/lib/dep.ts'}\noutput='/custom/page.js'\n";
    let files = vec![
        (
            "authored/one/guide/index.md".into(),
            page_with("content:guide", "/guide/", metadata).into_bytes(),
        ),
        (
            "authored/one/guide/main.ts".into(),
            b"import './lib/dep.ts';".to_vec(),
        ),
        (
            "authored/one/guide/lib/dep.ts".into(),
            b"export const answer = 42;".to_vec(),
        ),
    ];
    let corpus = parse(&files);
    let compilation = compilation();
    let resolved = resolve(&compilation, &corpus);
    let resources = resolved.resources(&key("content:guide"));
    assert_eq!(resources.len(), 1);
    assert_eq!(resources[0].output.as_str(), "/custom/page.js");
    assert_eq!(
        resources[0].build.entry(),
        &path("guide/lib/dep.ts"),
        "an explicit declaration may select an otherwise non-inferred nested entry"
    );
}

#[test]
fn ambiguous_automatic_page_package_entries_require_explicit_declarations() {
    for (first, second) in [
        ("authored/one/guide/main.ts", "authored/one/guide/admin.js"),
        (
            "authored/one/guide/main.scss",
            "authored/one/guide/article.css",
        ),
    ] {
        let files = vec![
            (
                "authored/one/guide/index.md".into(),
                page("content:guide", "/guide/").into_bytes(),
            ),
            (first.into(), b"first candidate".to_vec()),
            (second.into(), b"second candidate".to_vec()),
        ];
        let corpus = parse(&files);
        let compilation = compilation();
        let error = authored::resolve(&compilation, &corpus, content::Limits::default())
            .expect_err("ambiguous zero-config entries require an explicit declaration");
        let authored::Error::Files(failures) = error else {
            panic!("entry ambiguity remains attached to the page-package owner")
        };
        assert!(failures.iter().any(|failure| {
            matches!(
                &failure.error,
                authored::FileError::Resource(gazette_core::resource::Error::Input {
                    reason: "ambiguous automatic page-package entries require an explicit resource declaration",
                    ..
                })
            )
        }));
    }
}

#[test]
fn page_package_fonts_are_classified_but_require_explicit_compilation() {
    let metadata = "[[resources]]\ninputs=['guide/type.woff2']\n\
        build={kind='woff2-font',entry='guide/type.woff2'}\noutput='/guide/type.woff2'\n";
    let files = vec![
        (
            "authored/one/guide/index.md".into(),
            page_with("content:guide", "/guide/", metadata).into_bytes(),
        ),
        (
            "authored/one/guide/type.woff2".into(),
            b"retained font".to_vec(),
        ),
    ];
    let corpus = parse(&files);
    let compilation = compilation();
    let resolved = resolve(&compilation, &corpus);
    let id = key("content:guide");
    let package = resolved.page_package(&id).expect("page package");
    assert_eq!(package.companions()[0].kind(), PageCompanionKind::Font);
    let resources = resolved.resources(&id);
    assert_eq!(
        resources.len(),
        1,
        "classification never creates a second resource"
    );
    assert!(matches!(resources[0].build, Recipe::Woff2Font { .. }));
    assert_eq!(resources[0].output.as_str(), "/guide/type.woff2");
}

#[test]
fn unhandled_package_companions_remain_explicit_build_obligations() {
    let files = vec![
        (
            "authored/one/guide/index.md".into(),
            page("content:guide", "/guide/").into_bytes(),
        ),
        (
            "authored/one/guide/private.bin".into(),
            b"no implicit transport".to_vec(),
        ),
    ];
    let corpus = parse(&files);
    let compilation = compilation();
    let resolved = resolve(&compilation, &corpus);
    let companion = &resolved
        .page_package(&key("content:guide"))
        .expect("page package")
        .companions()[0];
    assert_eq!(companion.kind(), PageCompanionKind::Unhandled);
    assert_eq!(
        companion.source_sha256(),
        Some(Sha256Digest::of(b"no implicit transport"))
    );
    assert_eq!(
        resolved.assets_for(&key("site:one")).unwrap().uncompiled(),
        [path("authored/one/guide/private.bin")]
    );
}

#[test]
fn page_package_inspection_shape_is_stable_and_schema_backed() {
    let files = vec![
        (
            "authored/one/guide/index.md".into(),
            page("content:guide", "/guide/").into_bytes(),
        ),
        ("authored/one/guide/image.png".into(), b"image".to_vec()),
    ];
    let corpus = parse(&files);
    let compilation = compilation();
    let resolved = resolve(&compilation, &corpus);
    let package = resolved.page_package(&key("content:guide")).unwrap();
    let image_sha256 = Sha256Digest::of(b"image");
    assert_eq!(
        serde_json::to_value(package).unwrap(),
        serde_json::json!({
            "document": "authored/one/guide/index.md",
            "root": "guide",
            "companions": [{
                "path": "authored/one/guide/image.png",
                "relative": "image.png",
                "kind": "static",
                "source_sha256": image_sha256
            }]
        })
    );
    let schema = schemars::schema_for!(gazette_compiler::authored::PagePackage);
    assert_eq!(
        schema
            .get("properties")
            .and_then(|value| value.get("companions"))
            .and_then(|value| value.get("type"))
            .and_then(serde_json::Value::as_str),
        Some("array")
    );
}

#[test]
fn page_package_root_requires_the_exact_portable_index_name() {
    for filename in ["INDEX.md", "Index.markdown"] {
        let files = vec![
            (
                format!("authored/one/guide/{filename}"),
                page("content:guide", "/guide/").into_bytes(),
            ),
            ("authored/one/guide/image.png".into(), b"unowned".to_vec()),
        ];
        let corpus = parse(&files);
        let compilation = compilation();
        let resolved = resolve(&compilation, &corpus);
        assert!(resolved.page_package(&key("content:guide")).is_none());
        let image = resolved
            .assets_for(&key("site:one"))
            .unwrap()
            .get(&path("authored/one/guide/image.png"))
            .expect("non-package static asset uses the configured binding transport");
        assert_eq!(image.url, "https://example.test/media/guide/image.png");
    }
}

#[test]
fn standalone_page_does_not_claim_a_similarly_named_directory() {
    let files = vec![
        (
            "authored/one/guide.md".into(),
            page("content:guide", "/guide/").into_bytes(),
        ),
        (
            "authored/one/guide/main.ts".into(),
            b"export const value = 1;".to_vec(),
        ),
    ];
    let corpus = parse(&files);
    let compilation = compilation();
    let resolved = resolve(&compilation, &corpus);
    let id = key("content:guide");
    assert!(resolved.page_package(&id).is_none());
    assert!(resolved.resources(&id).is_empty());
    assert_eq!(
        resolved.assets_for(&key("site:one")).unwrap().uncompiled(),
        [path("authored/one/guide/main.ts")]
    );
}

#[test]
fn package_ownership_stops_at_the_exact_content_binding() {
    let files = vec![
        (
            "authored/one/guide/index.md".into(),
            page("content:guide", "/guide/").into_bytes(),
        ),
        ("authored/one/guide/owned.png".into(), b"owned".to_vec()),
        (
            "authored/one-other/guide/foreign.png".into(),
            b"foreign".to_vec(),
        ),
    ];
    let corpus = parse(&files);
    let compilation = compilation();
    let resolved = resolve(&compilation, &corpus);
    let package = resolved
        .page_package(&key("content:guide"))
        .expect("package");
    assert_eq!(package.companions().len(), 1);
    assert_eq!(
        package.companions()[0].path(),
        &path("authored/one/guide/owned.png")
    );
    let assets = resolved.assets_for(&key("site:one")).unwrap();
    assert_eq!(
        assets
            .get(&path("authored/one-other/guide/foreign.png"))
            .unwrap()
            .url,
        "https://example.test/other-media/guide/foreign.png"
    );
}

#[test]
fn package_companion_selection_is_the_owner_selection_without_orphans() {
    let files = vec![
        (
            "authored/one/guide/index.md".into(),
            page("content:guide", "/guide/").into_bytes(),
        ),
        ("authored/one/guide/image.png".into(), b"retained".to_vec()),
    ];
    let corpus = parse(&files);
    let compilation = compilation();
    for (owner, companion) in [
        (Disposition::Include, Disposition::Exclude),
        (Disposition::Exclude, Disposition::Include),
    ] {
        let resolved = resolve(&compilation, &corpus);
        let mut selection = selection(&resolved, owner);
        selection
            .assets
            .insert(path("authored/one/guide/image.png"), companion);
        assert!(matches!(
            resolved.into_selected(&selection),
            Err(SelectionError::Asset {
                reason: "page package companion disposition must match its owner",
                ..
            })
        ));
    }

    let resolved = resolve(&compilation, &corpus);
    let excluded_selection = selection(&resolved, Disposition::Exclude);
    let excluded = resolved
        .into_selected(&excluded_selection)
        .expect("owner and closure excluded together");
    assert!(excluded.page_package(&key("content:guide")).is_none());
    assert!(
        excluded
            .assets_for(&key("site:one"))
            .expect("empty selected asset plan")
            .entries()
            .next()
            .is_none()
    );
}

#[test]
fn nested_or_competing_markdown_refuses_the_complete_package() {
    for nested in [
        "authored/one/guide/other.md",
        "authored/one/guide/nested/index.md",
    ] {
        let files = vec![
            (
                "authored/one/guide/index.md".into(),
                page("content:guide", "/guide/").into_bytes(),
            ),
            (
                nested.into(),
                page("content:nested", "/nested/").into_bytes(),
            ),
            (
                "authored/one/guide/image.png".into(),
                b"must not escape partial resolution".to_vec(),
            ),
        ];
        let corpus = parse(&files);
        let compilation = compilation();
        let error = authored::resolve(&compilation, &corpus, content::Limits::default())
            .expect_err("one leaf package cannot contain another Markdown owner");
        let authored::Error::Files(failures) = error else {
            panic!("package failure preserves its retained owner")
        };
        assert!(failures.iter().any(|failure| {
            matches!(
                &failure.error,
                authored::FileError::Resource(gazette_core::resource::Error::Input {
                    path,
                    reason: "leaf page package contains another Markdown document"
                }) if path.as_str() == nested
            )
        }));
    }
}

#[test]
fn sibling_svg_images_lower_to_existing_resources_and_notes_share_the_article_destination() {
    let files = vec![
        (
            "authored/one/guide/index.md".into(),
            format!(
                "{}\n![Diagram](operating-and-payment.svg)\n[Diagram source](operating-and-payment.svg#mark)\n",
                page("content:guide", "/guide/")
            )
            .into_bytes(),
        ),
        (
            "authored/one/guide/operating-and-payment.svg".into(),
            br#"<svg xmlns="http://www.w3.org/2000/svg"><rect id="mark" width="1" height="1"/></svg>"#.to_vec(),
        ),
        (
            "authored/one/guide/notes.md".into(),
            b"## Commentary\n![Same diagram](operating-and-payment.svg)\n".to_vec(),
        ),
    ];
    let corpus = parse(&files);
    let compilation = compilation();
    let resolved = resolve(&compilation, &corpus);
    let id = key("content:guide");
    let resource = &resolved.resources(&id)[0];
    assert_eq!(resolved.resources(&id).len(), 1);
    assert_eq!(resource.inputs, [path("guide/operating-and-payment.svg")]);
    assert_eq!(
        resource.build,
        Recipe::SvgImage {
            entry: path("guide/operating-and-payment.svg")
        }
    );
    assert_eq!(resource.output.as_str(), "/guide/operating-and-payment.svg");
    let package = resolved.page_package(&id).expect("article package");
    assert_eq!(
        package
            .companions()
            .iter()
            .find(|companion| companion.relative().as_str() == "operating-and-payment.svg")
            .unwrap()
            .kind(),
        PageCompanionKind::SvgImage
    );
    assert!(
        resolved
            .assets_for(&key("site:one"))
            .unwrap()
            .get(&path("authored/one/guide/operating-and-payment.svg"))
            .is_none(),
        "SVG never enters unchecked static transport"
    );
    let projected = resolved
        .project_page(
            &id,
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("declared resource address");
    let address = "https://example.test/guide/operating-and-payment.svg";
    assert!(projected.body_html.contains(&format!("src=\"{address}\"")));
    assert!(
        projected
            .body_html
            .contains(&format!("href=\"{address}#mark\""))
    );
    assert!(
        projected
            .body_html
            .contains("data-gazette-destination-kind=\"resource\"")
    );
    assert!(
        projected
            .data
            .notes
            .as_ref()
            .unwrap()
            .body_html
            .contains(&format!("src=\"{address}\""))
    );
    assert!(
        projected.data.assets.is_empty(),
        "compiled SVG remains distinct from static assets"
    );
    let image = &projected.data.images[0];
    assert_eq!(image.destination, address);
    assert_eq!(image.alt, "Diagram");
    assert_eq!(image.occurrence, 1);
    assert_eq!(image.address.as_deref(), Some("content:guide#image-1"));
    assert!(
        projected
            .body_html
            .contains("data-gazette-image=\"content:guide#image-1\"")
    );
    let note_image = &projected.data.notes.as_ref().unwrap().images[0];
    assert_eq!(note_image.destination, address);
    assert_eq!(note_image.alt, "Same diagram");
    assert_eq!(
        note_image.address.as_deref(),
        Some("content:guide#image-gazette-notes-1")
    );
    assert!(
        projected
            .data
            .notes
            .as_ref()
            .unwrap()
            .body_html
            .contains("data-gazette-image=\"content:guide#image-gazette-notes-1\"")
    );
    let discovery = resolved.discovery().unwrap();
    let search = serde_json::to_value(
        projected
            .discovery_view(&discovery)
            .unwrap()
            .search()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(search["images"][0]["destination"], address);
    assert_eq!(search["notes"]["images"][0]["alt"], "Same diagram");
    let mut selected = selection(&resolved, Disposition::Include);
    selected.assets.insert(
        path("authored/one/guide/operating-and-payment.svg"),
        Disposition::Exclude,
    );
    assert!(
        matches!(
            resolved.into_selected(&selected),
            Err(SelectionError::Asset { .. })
        ),
        "compiled SVG remains in the required selected page closure"
    );
}

#[test]
fn explicit_svg_destination_overrides_implicit_bundle_output_without_duplicate_compilation() {
    let resources = "[[resources]]\ninputs=['guide/three-orders.svg']\nbuild={kind='svg-image',entry='guide/three-orders.svg'}\noutput='/diagrams/selected.svg'\n";
    let files = vec![
        (
            "authored/one/guide/index.md".into(),
            format!(
                "{}\n![Diagram](three-orders.svg)\n",
                page_with("content:guide", "/guide/", resources)
            )
            .into_bytes(),
        ),
        (
            "authored/one/guide/three-orders.svg".into(),
            b"retained compiler input".to_vec(),
        ),
    ];
    let corpus = parse(&files);
    let compilation = compilation();
    let resolved = resolve(&compilation, &corpus);
    let id = key("content:guide");
    assert_eq!(resolved.resources(&id).len(), 1);
    assert_eq!(
        resolved.resources(&id)[0].output.as_str(),
        "/diagrams/selected.svg"
    );
    let projected = resolved
        .project_page(
            &id,
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("explicit compiled destination");
    assert!(
        projected
            .body_html
            .contains("src=\"https://example.test/diagrams/selected.svg\"")
    );
}

#[test]
fn implicit_svg_cannot_claim_an_explicit_resource_output() {
    let resources = "[[resources]]\ninputs=['guide/program.ts']\nbuild={kind='browser-module',entry='guide/program.ts'}\noutput='/guide/three-orders.svg'\n";
    let files = vec![
        (
            "authored/one/guide/index.md".into(),
            page_with("content:guide", "/guide/", resources).into_bytes(),
        ),
        (
            "authored/one/guide/program.ts".into(),
            b"export {};".to_vec(),
        ),
        (
            "authored/one/guide/three-orders.svg".into(),
            b"retained compiler input".to_vec(),
        ),
    ];
    let corpus = parse(&files);
    let compilation = compilation();
    let error = authored::resolve(&compilation, &corpus, content::Limits::default())
        .expect_err("output ownership remains exclusive");
    assert!(
        error
            .to_string()
            .contains("explicit and automatic page resources claim the same output"),
        "{error}"
    );
}

#[test]
fn relative_svg_reference_refuses_missing_or_ambiguous_compiled_destinations() {
    for resources in [
        "",
        "[[resources]]\ninputs=['guide/three-orders.svg']\nbuild={kind='svg-image',entry='guide/three-orders.svg'}\noutput='/diagrams/one.svg'\n[[resources]]\ninputs=['guide/three-orders.svg']\nbuild={kind='svg-image',entry='guide/three-orders.svg'}\noutput='/diagrams/two.svg'\n",
    ] {
        let mut files = vec![(
            "authored/one/guide/index.md".into(),
            format!(
                "{}\n![Diagram](three-orders.svg)\n",
                page_with("content:guide", "/guide/", resources)
            )
            .into_bytes(),
        )];
        if !resources.is_empty() {
            files.push((
                "authored/one/guide/three-orders.svg".into(),
                b"retained compiler input".to_vec(),
            ));
        }
        let corpus = parse(&files);
        let compilation = compilation();
        let resolved = resolve(&compilation, &corpus);
        let error = resolved
            .project_page(
                &key("content:guide"),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .expect_err("an image needs one exact compiled destination");
        let reason = if resources.is_empty() {
            "static target is absent or uncompiled"
        } else {
            "retained image has multiple compiled publication destinations"
        };
        assert!(error.to_string().contains(reason), "{error}");
    }
}

#[test]
fn observed_assets_resolve_packages_without_fabricating_captured_identity() {
    let files = vec![
        (
            "authored/one/guide/index.md".into(),
            page("content:guide", "/guide/").into_bytes(),
        ),
        ("authored/one/guide/z.png".into(), b"retained".to_vec()),
    ];
    let corpus = parse(&files);
    let config = compilation();
    let observations = std::collections::BTreeMap::from([
        (path("authored/one/guide/a.mp4"), 64_u64 << 30),
        (path("authored/one/guide/main.scss"), 128),
    ]);
    let resolved =
        authored::resolve_with_asset_metadata(&config, &corpus, observations, Default::default())
            .unwrap();
    let package = resolved.page_package(&key("content:guide")).unwrap();
    let companions = package.companions();
    assert_eq!(
        companions
            .iter()
            .map(|c| c.relative().as_str())
            .collect::<Vec<_>>(),
        ["a.mp4", "main.scss", "z.png"]
    );
    assert_eq!(companions[0].source_sha256(), None);
    assert_eq!(companions[0].observed_bytes(), Some(64_u64 << 30));
    assert_eq!(companions[1].kind(), PageCompanionKind::Stylesheet);
    assert_eq!(
        companions[2].source_sha256(),
        Some(Sha256Digest::of(b"retained"))
    );
    assert_eq!(companions[2].observed_bytes(), None);
    let json = serde_json::to_value(&companions[0]).unwrap();
    assert!(json.get("source_sha256").is_none());
    assert_eq!(json["observed_bytes"], 64_u64 << 30);
    assert!(matches!(
        resolved.assets_for(&key("site:one")),
        Err(authored::AssetError::Uncaptured(_))
    ));
    assert!(matches!(
        resolved.project_page(
            &key("content:guide"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak
        ),
        Err(authored::ProjectionError::Asset(
            authored::AssetError::Uncaptured(_)
        ))
    ));
    assert!(resolved.non_draft_selection().is_err());
    assert!(resolved.default_native_documentation_selection().is_err());
    let decisions = selection(&resolved, Disposition::Include);
    assert!(matches!(
        resolved.into_selected(&decisions),
        Err(SelectionError::Asset { .. })
    ));
}

#[test]
fn observations_refuse_captured_collisions_unbound_paths_syntax_and_limits() {
    let files = vec![(
        "authored/one/guide/index.md".into(),
        page("content:guide", "/guide/").into_bytes(),
    )];
    let corpus = parse(&files);
    let config = compilation();
    for name in [
        "authored/one/guide/index.md",
        "outside/image.png",
        "authored/one/guide/other.MD",
        "authored/one/guide/data.IPYNB",
        "authored/one/guide/index.LANDING.YAML",
    ] {
        assert!(
            matches!(
                authored::resolve_with_asset_metadata(
                    &config,
                    &corpus,
                    std::collections::BTreeMap::from([(path(name), 1)]),
                    Default::default()
                ),
                Err(authored::Error::AssetMetadata { .. })
            ),
            "{name}"
        );
    }
    for limits in [
        content::Limits {
            nodes: 0,
            ..Default::default()
        },
        content::Limits {
            text_bytes: 1,
            ..Default::default()
        },
    ] {
        assert!(matches!(
            authored::resolve_with_asset_metadata(
                &config,
                &corpus,
                std::collections::BTreeMap::from([(path("authored/one/guide/image.png"), 1)]),
                limits
            ),
            Err(authored::Error::AssetMetadata { .. })
        ));
    }
}
