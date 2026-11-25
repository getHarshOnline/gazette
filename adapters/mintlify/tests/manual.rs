//! Authored manuals and generated references share a repository, never a body model.

use std::collections::{BTreeMap, BTreeSet};

use gazette_adapter_mintlify::Mintlify;
use gazette_core::{
    documentation::{
        Error, Format, ManualAsset, ManualPage, ManualSection, ManualSource, ProjectItem,
        ProjectModule, Reference, Request, Selection, Target,
        projection::{AuthoringClosure, AuthoringManualPage, AuthoringReference},
    },
    source::{CapturedSource, Content, EntryPath, InventoryBuilder, Limits, Sha256Digest},
};
use gazette_ports::documentation::DocumentationTarget;
use serde_json::Value;

#[path = "manual/module_roots.rs"]
mod module_roots;

#[path = "manual/companions.rs"]
mod companions;

#[path = "manual/text_assets.rs"]
mod text_assets;

mod support;
use support::Input;

fn retained(files: &[(&str, &[u8])]) -> CapturedSource {
    let mut inventory = InventoryBuilder::new(Limits::default());
    let mut directories = BTreeSet::new();
    let mut contents = BTreeMap::new();
    for (name, bytes) in files {
        let path: EntryPath = name.parse().expect("fixture path");
        let parts: Vec<_> = name.split('/').collect();
        let mut parent = String::new();
        for part in &parts[..parts.len() - 1] {
            if !parent.is_empty() {
                parent.push('/');
            }
            parent.push_str(part);
            let directory: EntryPath = parent.parse().expect("fixture directory");
            if directories.insert(directory.clone()) {
                inventory
                    .insert(directory, Content::Directory)
                    .expect("directory");
            }
        }
        inventory
            .insert(
                path.clone(),
                Content::File {
                    bytes: bytes.len() as u64,
                    sha256: Sha256Digest::of(bytes),
                },
            )
            .expect("file");
        contents.insert(path, bytes.to_vec());
    }
    CapturedSource::from_parts(inventory.finish(), contents).expect("retained manual source")
}

fn selection(path: &str) -> Selection {
    Selection::File {
        path: path.parse().expect("selection path"),
    }
}

fn project<'a>(manual: &'a CapturedSource) -> ProjectModule<'a> {
    let start = Box::leak(Box::new(selection("docs/start.md")));
    let guide = Box::leak(Box::new(selection("docs/guide.md")));
    let diagram = Box::leak(Box::new(selection("docs/media/diagram.png")));
    let start_route = Box::leak(Box::new("gazette/start".parse().expect("route")));
    let guide_route = Box::leak(Box::new("gazette/guide".parse().expect("route")));
    let asset_path = Box::leak(Box::new("assets/diagram.png".parse().expect("asset")));
    ProjectModule {
        slug: "gazette",
        title: "Gazette",
        manual: Some(ManualSource {
            source: "source:manual",
            tree: manual.as_input(),
            assets: vec![ManualAsset {
                selection: diagram,
                path: asset_path,
            }],
        }),
        navigation: vec![
            ProjectItem::Manual(ManualSection {
                title: "Getting started",
                pages: vec![ManualPage {
                    title: "Start",
                    route: start_route,
                    selection: start,
                }],
            }),
            ProjectItem::Manual(ManualSection {
                title: "Guides",
                pages: vec![ManualPage {
                    title: "Guide",
                    route: guide_route,
                    selection: guide,
                }],
            }),
        ],
    }
}

fn assemble(projects: &[ProjectModule<'_>]) -> Result<CapturedSource, Error> {
    Mintlify::new(&gazette_adapter_markdown::Comrak).assemble(
        Request {
            name: "Product documentation",
            public_base_url: None,
            target: &Target::Mintlify {
                theme: "mint".into(),
                primary_color: "#123abc".into(),
                light_color: None,
                dark_color: None,
                telemetry: None,
            },
            projects,
            usage: None,
            metadata: Default::default(),
        },
        Limits::default(),
    )
}

fn manual_source() -> CapturedSource {
    retained(&[
        (
            "docs/start.md",
            b"# Start\n\n[Guide](guide.md#guide)\n\n[Self](#start)\n\n![Diagram](media/diagram.png)\n",
        ),
        ("docs/guide.md", b"# Guide\n\n[Back](start.md#start)\n"),
        ("docs/media/diagram.png", b"retained-image"),
    ])
}

fn entry(path: &str) -> EntryPath {
    path.parse().expect("fixture path")
}

#[test]
fn manual_math_uses_the_shared_formula_profile() {
    let tree = retained(&[
        ("docs/start.md", b"# Start\n\nUse $D(\\cdot \\| \\cdot)$ with $B=(M_s,M_b)$.\n\n```math\nU(r)=\\frac{u(r)}{C(r)}\n```\n\n[Guide](guide.md#guide)\n"),
        ("docs/guide.md", b"# Guide\n\n- One\n\n1. Two\n"),
        ("docs/media/diagram.png", b"retained-image"),
    ]);
    let output = assemble(&[project(&tree)]).unwrap();
    let start = std::str::from_utf8(output.file(&entry("gazette/start.md")).unwrap()).unwrap();
    assert!(start.contains("$D(\\cdot \\| \\cdot)$"), "{start}");
    assert!(start.contains("$$\nU(r)=\\frac{u(r)}{C(r)}\n$$"), "{start}");
    let guide = std::str::from_utf8(output.file(&entry("gazette/guide.md")).unwrap()).unwrap();
    assert!(guide.contains("{/* list boundary */}"), "{guide}");
    assert!(!guide.contains("<!-- end list -->"), "{guide}");
}

#[test]
fn native_formula_presentation_is_reserved_and_charged_to_the_target() {
    let tree = manual_source();
    let output = assemble(&[project(&tree)]).unwrap();
    assert_eq!(
        output.file(&entry("gazette-provider-math.css")).unwrap(),
        include_bytes!("../src/provider-math.css")
    );
    let mut collision = project(&tree);
    collision.manual.as_mut().unwrap().assets[0].path =
        Box::leak(Box::new(entry("gazette-provider-math.css")));
    assert!(
        assemble(&[collision]).is_err(),
        "authored assets cannot replace provider presentation"
    );
}

#[test]
fn authored_layouts_keep_nested_manual_routes_and_formulas_in_one_target() {
    let source = b"# Start\n\n:::::tabs\n::::tab \"Rust\"\n\nRead [the guide](guide.md#guide), with $x^2$.\n\n:::code-group\n```rust Client.rs\nlet count = 2;\n```\n```javascript Client.js\nconst count = 2;\n```\n:::\n\n::::\n::::tab \"Reasoning\"\n\n:::accordion \"Why retain the input?\"\n\nThe [same captured source](guide.md#guide) supplies the build.\n\n:::\n\n::::\n:::::\n";
    let tree = retained(&[
        ("docs/start.md", source),
        ("docs/guide.md", b"# Guide\n\n[Return](start.md#start)\n"),
        ("docs/media/diagram.png", b"retained-image"),
    ]);
    let output = assemble(&[project(&tree)]).unwrap();
    let page = std::str::from_utf8(output.file(&entry("gazette/start.md")).unwrap()).unwrap();
    assert!(page.contains("<Tabs>"), "{page}");
    assert!(page.contains("<CodeGroup>"), "{page}");
    assert!(
        page.contains(
            r#"<Accordion title="Why retain the input&#92;&#63;" id="gazette-layout-2" className="gazette-layout-disclosure" _disabled>"#
        ),
        "{page}"
    );
    assert_eq!(page.matches("](/gazette/guide#guide)").count(), 2, "{page}");
    assert!(page.contains("$x^2$"), "{page}");
    assert_eq!(tree.file(&entry("docs/start.md")), Some(source.as_slice()));
}

#[test]
fn layout_bodies_do_not_bypass_manual_html_or_link_checks() {
    for body in [
        "<script>run()</script>",
        "<span onclick=\"run()\">Run</span>",
        "<Tab title={value}>Authored JSX</Tab>",
        "<Tab onClick={run()}>Authored JSX</Tab>",
        "[Missing](missing.md)",
        "[Active](javascript:run())",
    ] {
        let source = format!("# Start\n\n:::accordion \"Details\"\n\n{body}\n\n:::\n");
        let tree = retained(&[
            ("docs/start.md", source.as_bytes()),
            ("docs/guide.md", b"# Guide\n"),
            ("docs/media/diagram.png", b"retained-image"),
        ]);
        assert!(assemble(&[project(&tree)]).is_err(), "{body}");
        assert_eq!(tree.file(&entry("docs/start.md")), Some(source.as_bytes()));
    }
}

#[test]
fn layout_bodies_preserve_supported_html_without_evaluating_attribute_values() {
    let source = b"# Start\n\n:::accordion \"Details\"\n\n<span title={value}>Literal value</span>\n\n[Guide](guide.md#guide)\n\n:::\n";
    let tree = retained(&[
        ("docs/start.md", source),
        ("docs/guide.md", b"# Guide\n"),
        ("docs/media/diagram.png", b"retained-image"),
    ]);
    let output = assemble(&[project(&tree)]).unwrap();
    let page = std::str::from_utf8(output.file(&entry("gazette/start.md")).unwrap()).unwrap();
    assert!(
        page.contains("<span title=\"&#123;value&#125;\">Literal value</span>"),
        "{page}"
    );
    assert!(page.contains("[Guide](/gazette/guide#guide)"), "{page}");
    assert!(!page.contains("title={value}"), "{page}");
    assert_eq!(tree.file(&entry("docs/start.md")), Some(source.as_slice()));
}

#[test]
fn fragment_navigation_is_target_owned_conditional_and_charged_to_tree_limits() {
    let plain = retained(&[
        ("docs/start.md", b"Start here.\n"),
        ("docs/guide.md", b"Guide text.\n"),
        ("docs/media/diagram.png", b"retained-image"),
    ]);
    assert!(
        assemble(&[project(&plain)])
            .unwrap()
            .file(&entry("gazette-layout-navigation.js"))
            .is_none()
    );

    let source = b"# Start\n\n::::accordion \"Outer\"\n\n:::accordion \"Inner\"\n\n## Detail\n\n[This detail](#detail) and [Guide](guide.md#guide).\n\n:::\n::::\n";
    let tree = retained(&[
        ("docs/start.md", source),
        ("docs/guide.md", b"# Guide\n"),
        ("docs/media/diagram.png", b"retained-image"),
    ]);
    let output = assemble(&[project(&tree)]).unwrap();
    let script = output.file(&entry("gazette-layout-navigation.js")).unwrap();
    assert_eq!(script, include_bytes!("../src/layout-navigation.js"));
    assert_eq!(
        output
            .file(&entry("gazette-fragment-navigation.css"))
            .unwrap(),
        include_bytes!("../src/fragment-navigation.css")
    );
    let page = std::str::from_utf8(output.file(&entry("gazette/start.md")).unwrap()).unwrap();
    assert_eq!(
        page.matches("className=\"gazette-layout-disclosure\"")
            .count(),
        2
    );
    assert!(page.contains("## Detail {#detail}"), "{page}");
    assert!(page.contains("[This detail](#detail)"), "{page}");
    assert!(!page.contains("defaultOpen"));
    assert_eq!(tree.file(&entry("docs/start.md")), Some(source.as_slice()));

    let error = Mintlify::new(&gazette_adapter_markdown::Comrak)
        .assemble(
            Request {
                name: "Developer guide",
                public_base_url: None,
                target: &Target::Mintlify {
                    theme: "mint".into(),
                    primary_color: "#123abc".into(),
                    light_color: None,
                    dark_color: None,
                    telemetry: None,
                },
                projects: &[project(&tree)],
                usage: None,
                metadata: Default::default(),
            },
            Limits {
                file_bytes: script.len() as u64 - 1,
                ..Limits::default()
            },
        )
        .unwrap_err();
    assert!(
        error.to_string().contains("gazette-layout-navigation.js"),
        "{error}"
    );

    let mut collision = project(&tree);
    collision.manual.as_mut().unwrap().assets[0].path =
        Box::leak(Box::new(entry("gazette-layout-navigation.js")));
    assert!(
        assemble(&[collision]).is_err(),
        "authored assets cannot replace trusted support"
    );
}

#[test]
fn title_and_authored_aliases_without_layouts_retain_native_fragments_and_clearance_support() {
    let source = b"# Start\n\n<a id=\"old-start\"></a>\n\n[Earlier name](#old-start).\n\n## Continue\n\n[Guide](guide.md#guide).\n";
    let tree = retained(&[
        ("docs/start.md", source),
        ("docs/guide.md", b"# Guide\n"),
        ("docs/media/diagram.png", b"retained-image"),
    ]);
    let output = assemble(&[project(&tree)]).unwrap();
    let page = std::str::from_utf8(output.file(&entry("gazette/start.md")).unwrap()).unwrap();
    assert!(page.contains("<a id=\"start\"></a>"), "{page}");
    assert!(page.contains("<a id=\"old&#45;start\"></a>"), "{page}");
    assert!(page.contains("[Earlier name](#old-start)"), "{page}");
    assert!(page.contains("## Continue {#continue}"), "{page}");
    for path in [
        "gazette-layout-navigation.js",
        "gazette-fragment-navigation.css",
    ] {
        assert!(output.file(&entry(path)).is_some(), "{path}");
    }
    assert_eq!(tree.file(&entry("docs/start.md")), Some(source.as_slice()));

    let mut collision = project(&tree);
    collision.manual.as_mut().unwrap().assets[0].path =
        Box::leak(Box::new(entry("gazette-fragment-navigation.css")));
    assert!(
        assemble(&[collision]).is_err(),
        "authored assets cannot replace target CSS"
    );
}

#[test]
fn authoring_closure_accepts_the_same_nested_layout_and_heading_profile_as_assembly() {
    let input = entry("docs/start.md");
    let route = entry("manual/start");
    let body = b"# Start\n\n[Detail](#content)\n\n::::accordion \"Outer\"\n\n:::accordion \"Inner\"\n\n## content\n\nRetain this explanation.\n\n:::\n::::\n";
    assert_eq!(
        Mintlify::new(&gazette_adapter_markdown::Comrak)
            .authoring_closure(
                &[AuthoringManualPage {
                    source: "source:manual",
                    input: &input,
                    route: &route,
                    bytes: body
                }],
                &[],
                Limits::default(),
            )
            .unwrap(),
        AuthoringClosure::Complete,
    );
}

#[test]
fn native_callouts_project_in_authored_and_generated_markdown_without_changing_sources() {
    let start =
        b"# Start\n\n> [!IMPORTANT]\n> Read [Guide](guide.md#guide), keep {value} and `code`.\n";
    let guide = b"# Guide\n\n> [!CAUTION]\n> Keep this warning with the task.\n";
    let tree = retained(&[
        ("docs/start.md", start),
        ("docs/guide.md", guide),
        ("docs/media/diagram.png", b"retained-image"),
    ]);
    let generated_source = b"# Library\n\n> [!TIP]\n> Return to [Start](/gazette/start#start).\n";
    let generated = Input::new(generated_source);
    let mut module = project(&tree);
    module.navigation.push(ProjectItem::Reference(Reference {
        pages: None,
        slug: "library",
        title: "Library",
        artifact: generated.artifact(Format::Markdown),
    }));
    let first = assemble(std::slice::from_ref(&module)).unwrap();
    let second = assemble(&[module]).unwrap();
    assert_eq!(first.inventory(), second.inventory());
    for item in first.inventory().entries() {
        if matches!(item.content, Content::File { .. }) {
            assert_eq!(first.file(&item.path), second.file(&item.path));
        }
    }
    let start_output =
        std::str::from_utf8(first.file(&entry("gazette/start.md")).unwrap()).unwrap();
    assert!(start_output.contains("<Info>"), "{start_output}");
    assert!(start_output.contains("</Info>"), "{start_output}");
    assert!(
        start_output.contains("/gazette/guide#guide"),
        "{start_output}"
    );
    assert!(start_output.contains("\\{value\\}"), "{start_output}");
    assert!(start_output.contains("`code`"), "{start_output}");
    let guide_output =
        std::str::from_utf8(first.file(&entry("gazette/guide.md")).unwrap()).unwrap();
    assert!(guide_output.contains("<Danger>"), "{guide_output}");
    let guide_document = gazette_ports::markdown::Markdown::parse(
        &gazette_adapter_markdown::Comrak,
        guide_output,
        gazette_core::markdown::Profile::Publication,
        gazette_core::markdown::Limits::default(),
    )
    .unwrap();
    let guide_text: String = guide_document
        .syntax()
        .text
        .iter()
        .map(|part| part.value.as_str())
        .collect();
    assert!(guide_text.contains("Keep this warning with the task."));
    let library = std::str::from_utf8(first.file(&entry("gazette/library.md")).unwrap()).unwrap();
    assert!(library.contains("<Tip>"), "{library}");
    assert!(library.contains("/gazette/start#start"), "{library}");
    assert_eq!(tree.file(&entry("docs/start.md")), Some(start.as_slice()));
    assert_eq!(tree.file(&entry("docs/guide.md")), Some(guide.as_slice()));
    assert_eq!(
        generated.tree.file(&entry("reference.json")),
        Some(generated_source.as_slice())
    );
}

#[test]
fn native_callout_children_cannot_bypass_selected_manual_link_closure() {
    let tree = retained(&[
        (
            "docs/start.md",
            b"# Start\n\n> [!IMPORTANT]\n> [Missing](missing.md).\n",
        ),
        ("docs/guide.md", b"# Guide\n"),
        ("docs/media/diagram.png", b"retained-image"),
    ]);
    assert!(assemble(&[project(&tree)]).is_err());
}

#[test]
fn manual_modules_share_only_the_exact_configured_capture() {
    let tree = retained(&[
        ("first/docs/start.md", b"# First\n\n[Second](../../second/docs/start.md#details)\n\n[Source](../../second/source.rs)\n"),
        ("second/docs/start.md", b"# Second\n\n## Details\n\n[First](../../first/docs/start.md)\n"),
        ("second/source.rs", b"// exact retained source\n"),
    ]);
    let a = selection("first/docs/start.md");
    let b = selection("second/docs/start.md");
    let asset = selection("second/source.rs");
    let ar = entry("first/start");
    let br = entry("second/start");
    let target = entry("second/source.rs");
    let module = |source, slug, input, route, title| ProjectModule {
        slug,
        title,
        manual: Some(ManualSource {
            source,
            tree: tree.as_input(),
            assets: Vec::new(),
        }),
        navigation: vec![ProjectItem::Manual(ManualSection {
            title: "Manual",
            pages: vec![ManualPage {
                title,
                route,
                selection: input,
            }],
        })],
    };
    let first = module("source:shared", "first", &a, &ar, "First");
    let mut second = module("source:shared", "second", &b, &br, "Second");
    second.manual.as_mut().unwrap().assets.push(ManualAsset {
        selection: &asset,
        path: &target,
    });
    let output = assemble(&[first, second]).expect("cross-module capture-relative links close");
    let body = std::str::from_utf8(output.file(&entry("first/start.md")).unwrap()).unwrap();
    assert!(body.contains("/second/start#details"));
    assert!(body.contains("/second/source.rs"));
    assert_eq!(
        output.file(&target),
        Some(b"// exact retained source\n".as_slice())
    );

    let first = module("source:first", "first", &a, &ar, "First");
    let second = module("source:second", "second", &b, &br, "Second");
    assert!(
        assemble(&[first, second]).is_err(),
        "equal tree bytes do not merge source owners"
    );

    let first = module("source:shared", "first", &a, &ar, "First");
    let second = module("source:shared", "second", &a, &br, "First");
    assert!(
        assemble(&[first, second]).is_err(),
        "duplicate input routes are ambiguous"
    );

    let different = retained(&[("second/docs/start.md", b"# Second\n\nDifferent capture.\n")]);
    let first = module("source:shared", "first", &a, &ar, "First");
    let mut second = module("source:shared", "second", &b, &br, "Second");
    second.manual.as_mut().unwrap().tree = different.as_input();
    assert!(
        assemble(&[first, second]).is_err(),
        "one source key cannot stand for conflicting captures"
    );
}

#[test]
fn manual_target_fragments_use_source_validation_and_the_same_reserved_anchor_projection() {
    let source = retained(&[
        ("docs/start.md", b"# Start\n\n[Forward](guide.md#content) [Root](/gazette/guide#page-title)\n"),
        ("docs/guide.md", b"# Guide\n\n## content\n\n[Self](#content)\n\n<a name=\"page-title\"></a>\n\n##### options\n"),
        ("docs/media/diagram.png", b"retained-image"),
    ]);
    let output = assemble(&[project(&source)]).unwrap();
    let start = std::str::from_utf8(output.file(&entry("gazette/start.md")).unwrap()).unwrap();
    assert!(
        start.contains("(/gazette/guide#gazette-anchor-content)"),
        "{start}"
    );
    assert!(
        start.contains("(/gazette/guide#gazette-anchor-page-title)"),
        "{start}"
    );
    let guide = std::str::from_utf8(output.file(&entry("gazette/guide.md")).unwrap()).unwrap();
    assert!(guide.contains("(#gazette-anchor-content)"), "{guide}");
    assert!(
        guide.contains("<Heading level={5} id=\"options\" className=\"scroll-mt-(--scroll-mt)\">options</Heading>"),
        "{guide}"
    );
}

#[test]
fn authoring_closure_scopes_equal_capture_paths_by_configured_source() {
    let first_input = entry("docs/start.md");
    let second_input = entry("docs/start.md");
    let first_route = entry("first/start");
    let second_route = entry("second/start");
    let pages = [
        AuthoringManualPage {
            source: "source:first",
            input: &first_input,
            route: &first_route,
            bytes: b"# First\n\n[Second](/second/start#second)\n",
        },
        AuthoringManualPage {
            source: "source:second",
            input: &second_input,
            route: &second_route,
            bytes: b"# Second\n\n[First](/first/start#first)\n",
        },
    ];
    assert_eq!(
        Mintlify::new(&gazette_adapter_markdown::Comrak)
            .authoring_closure(&pages, &[], Limits::default())
            .expect("source-qualified manual closure"),
        AuthoringClosure::Complete
    );
}

#[test]
fn authoring_closure_reports_missing_manual_pages_and_anchors() {
    let input = entry("docs/start.md");
    let route = entry("manual/start");
    for (body, reason) in [
        (
            "# Start\n\n[Missing](/manual/missing)\n",
            "qualified provider page",
        ),
        ("# Start\n\n[Missing](#missing)\n", "page anchor"),
    ] {
        let error = Mintlify::new(&gazette_adapter_markdown::Comrak)
            .authoring_closure(
                &[AuthoringManualPage {
                    source: "source:manual",
                    input: &input,
                    route: &route,
                    bytes: body.as_bytes(),
                }],
                &[],
                Limits::default(),
            )
            .expect_err("manual closure defect");
        assert!(error.to_string().contains(reason), "{error}");
    }
}

#[test]
fn authoring_closure_keeps_absent_generated_references_explicitly_incomplete() {
    let input = entry("docs/start.md");
    let route = entry("manual/start");
    let reference = entry("python/library");
    let result = Mintlify::new(&gazette_adapter_markdown::Comrak)
        .authoring_closure(
            &[AuthoringManualPage {
                source: "source:manual",
                input: &input,
                route: &route,
                bytes: b"# Start\n\n[Python](/python/library)\n\n[Generated symbol](/python/library/model#record)\n",
            }],
            &[AuthoringReference { route: &reference }],
            Limits::default(),
        )
        .expect("generated reference absence remains an explicit state");
    assert_eq!(result, AuthoringClosure::GeneratedReferencesIncomplete);
}

#[test]
fn manual_only_project_uses_explicit_navigation_routes_assets_and_links() {
    let manual = manual_source();
    let first = assemble(&[project(&manual)]).expect("manual-only target");
    let second = assemble(&[project(&manual)]).expect("deterministic repeat");
    assert_eq!(first.inventory(), second.inventory());
    let start = std::str::from_utf8(
        first
            .file(&"gazette/start.md".parse().expect("route"))
            .expect("page"),
    )
    .expect("UTF-8");
    assert!(start.starts_with("---\ntitle: \"Start\"\n---\n"));
    assert!(start.contains("[Guide](/gazette/guide#guide)"));
    assert!(start.contains("[Self](#start)"));
    assert!(start.contains("![Diagram](/assets/diagram.png)"));
    assert_eq!(
        first.file(&"assets/diagram.png".parse().expect("asset")),
        Some(b"retained-image".as_slice())
    );
    let configuration: Value = serde_json::from_slice(
        first
            .file(&"docs.json".parse().expect("configuration"))
            .expect("configuration bytes"),
    )
    .expect("JSON");
    assert_eq!(
        configuration["navigation"]["groups"][0]["pages"],
        serde_json::json!([
            {"group":"Getting started","pages":["gazette/start"]},
            {"group":"Guides","pages":["gazette/guide"]}
        ])
    );
}

#[test]
fn local_manual_return_links_use_explicit_bases_without_rewriting_examples() {
    let manual = retained(&[
        ("docs/start.md", b"# Start\n\n[Return](https://publication.example.test/tool/start/?q=one#edit)\n\n[Other](https://other.example.test/)\n\n`https://publication.example.test/tool/start/`\n"),
        ("docs/guide.md", b"# Guide\n\n[Start](start.md#start)\n"),
        ("docs/media/diagram.png", b"retained-image"),
    ]);
    let bases = gazette_core::publication::routing::PreviewLinkBases::try_from(BTreeMap::from([(
        "https://publication.example.test/".into(),
        "http://127.0.0.1:4501/".parse().unwrap(),
    )]))
    .unwrap();
    let target =
        Mintlify::new(&gazette_adapter_markdown::Comrak).with_preview_link_bases(Some(&bases));
    let projected = target
        .assemble(
            Request {
                name: "Product documentation",
                public_base_url: None,
                target: &Target::Mintlify {
                    theme: "mint".into(),
                    primary_color: "#123abc".into(),
                    light_color: None,
                    dark_color: None,
                    telemetry: None,
                },
                projects: &[project(&manual)],
                usage: None,
                metadata: Default::default(),
            },
            Limits::default(),
        )
        .unwrap();
    let body = std::str::from_utf8(projected.file(&entry("gazette/start.md")).unwrap()).unwrap();
    assert!(body.contains("[Return](http://127.0.0.1:4501/tool/start/?q=one#edit)"));
    assert!(body.contains("[Other](https://other.example.test/)"));
    assert!(body.contains("`https://publication.example.test/tool/start/`"));
    assert_eq!(target.preview_link_bases(), Some(&bases));
    let index = std::str::from_utf8(projected.file(&entry("llms.txt")).unwrap()).unwrap();
    assert!(index.starts_with("# Product documentation\n"));
    assert!(index.contains("[Start](/gazette/start)"));
    assert!(index.contains("(/gazette/guide)"));
    assert!(!index.contains(".md)"));
    let public = assemble(&[project(&manual)]).unwrap();
    assert!(public.file(&entry("llms.txt")).is_none());
    let public_body =
        std::str::from_utf8(public.file(&entry("gazette/start.md")).unwrap()).unwrap();
    assert!(
        public_body.contains("[Return](https://publication.example.test/tool/start/?q=one#edit)")
    );
}

#[test]
fn public_manuals_refuse_loopback_return_links() {
    for destination in [
        "http://localhost:4501/",
        "http://127.0.0.1:4501/",
        "http://[::1]:4501/",
    ] {
        let source = format!("# Start\n\n[Return]({destination})\n");
        let manual = retained(&[
            ("docs/start.md", source.as_bytes()),
            ("docs/guide.md", b"# Guide\n"),
            ("docs/media/diagram.png", b"retained-image"),
        ]);
        assert!(assemble(&[project(&manual)]).is_err(), "{destination}");
    }
}

#[test]
fn manual_links_require_exact_emitted_heading_anchors() {
    let broken = retained(&[
        ("docs/start.md", b"# Start\n\n[Guide](guide.md#missing)\n"),
        ("docs/guide.md", b"# Guide\n"),
        ("docs/media/diagram.png", b"retained-image"),
    ]);
    assert!(
        assemble(&[project(&broken)])
            .expect_err("missing manual heading anchor")
            .to_string()
            .contains("page anchor")
    );
}

#[test]
fn combined_project_keeps_authored_manual_and_generated_markdown_distinct_and_ordered() {
    let manual = manual_source();
    let generated = Input::new(b"# Generated library\n");
    let mut project = project(&manual);
    project.navigation.insert(
        1,
        ProjectItem::Reference(Reference {
            pages: None,
            slug: "library",
            title: "Generated reference",
            artifact: generated.artifact(Format::Markdown),
        }),
    );
    let output = assemble(&[project]).expect("combined manual and reference target");
    assert!(
        output
            .file(&"gazette/start.md".parse().expect("manual"))
            .is_some()
    );
    assert_eq!(
        output.file(&"gazette/library.md".parse().expect("reference")),
        Some(
            b"---\ntitle: \"Generated library\"\n---\n\n<a id=\"generated&#45;library\"></a>\n"
                .as_slice()
        ),
        "the generated title and exact original anchor remain independent of the manual title"
    );
    let configuration: Value = serde_json::from_slice(
        output
            .file(&"docs.json".parse().expect("configuration"))
            .expect("configuration bytes"),
    )
    .expect("JSON");
    let pages = configuration["navigation"]["groups"][0]["pages"]
        .as_array()
        .expect("project navigation");
    assert_eq!(pages[0]["group"], "Getting started");
    assert_eq!(pages[1]["group"], "Generated reference");
    assert_eq!(pages[2]["group"], "Guides");
}

#[test]
fn mismatched_or_nonleading_manual_h1_is_refused_instead_of_discarded() {
    for source in [
        "# A different title\n\nImportant context.\n",
        "Important context.\n\n# Start\n",
        "# Start\n\n# A substantive section\n",
    ] {
        let manual = retained(&[
            ("docs/start.md", source.as_bytes()),
            ("docs/guide.md", b"# Guide\n"),
            ("docs/media/diagram.png", b"retained-image"),
        ]);
        let error = assemble(&[project(&manual)]).expect_err("a title decision is required");
        assert!(error.to_string().contains("H1"), "{error}");
        assert_eq!(
            manual.file(&entry("docs/start.md")),
            Some(source.as_bytes())
        );
    }
}

#[test]
fn manual_reference_and_asset_namespaces_cannot_overlap() {
    let manual = manual_source();
    let generated = Input::new(b"# Generated library\n");
    let mut route_collision = project(&manual);
    let ProjectItem::Manual(section) = &mut route_collision.navigation[0] else {
        panic!("manual section")
    };
    section.pages[0].route = Box::leak(Box::new("gazette/library".parse().expect("route")));
    route_collision
        .navigation
        .push(ProjectItem::Reference(Reference {
            pages: None,
            slug: "library",
            title: "Reference",
            artifact: generated.artifact(Format::Markdown),
        }));
    assert!(
        assemble(&[route_collision])
            .expect_err("cross-kind route collision")
            .to_string()
            .contains("overlaps")
    );

    let mut asset_collision = project(&manual);
    asset_collision.manual.as_mut().expect("manual").assets[0].path =
        Box::leak(Box::new("gazette/library".parse().expect("asset root")));
    asset_collision
        .navigation
        .push(ProjectItem::Reference(Reference {
            pages: None,
            slug: "library",
            title: "Reference",
            artifact: generated.artifact(Format::Markdown),
        }));
    assert!(
        assemble(&[asset_collision])
            .expect_err("cross-kind asset collision")
            .to_string()
            .contains("overlaps")
    );
}
