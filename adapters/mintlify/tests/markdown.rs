//! Real Markdown-port and target composition, not language-generator or hosted qualification.

#[path = "markdown/site_links.rs"]
mod site_links;
mod support;

use gazette_adapter_markdown::Comrak;
use gazette_adapter_mintlify::Mintlify;
use gazette_core::{
    documentation::{
        Error, Format, ProjectItem, ProjectModule, Reference, Request, Selection, Target,
    },
    source::{CapturedSource, Content, EntryPath, InventoryBuilder, Limits, Sha256Digest},
};
use gazette_ports::documentation::DocumentationTarget;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use support::Input;

fn directory(files: &[(&str, &[u8])], root: Option<&str>) -> Input {
    let mut inventory = InventoryBuilder::new(Limits::default());
    let mut directories = BTreeSet::new();
    let mut retained = BTreeMap::new();
    for (name, bytes) in files {
        let path: EntryPath = name.parse().expect("page path");
        let mut parent = String::new();
        let components: Vec<_> = name.split('/').collect();
        for part in &components[..components.len() - 1] {
            if !parent.is_empty() {
                parent.push('/');
            }
            parent.push_str(part);
            let directory = parent.parse::<EntryPath>().expect("directory path");
            if directories.insert(directory.clone()) {
                inventory
                    .insert(directory, Content::Directory)
                    .expect("directory inventory");
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
            .expect("file inventory");
        retained.insert(path, bytes.to_vec());
    }
    Input {
        tree: CapturedSource::from_parts(inventory.finish(), retained).expect("retained directory"),
        selection: Selection::Directory {
            path: root.map(|path| path.parse().expect("selection path")),
        },
    }
}

const PAGE: &[u8] = b"# Library reference\n\nVersion 1.0.0; [source](https://example.test/source/commit).\n\n<a name=\"Client\"></a>\n## Client\n\n```go\ntype Client[T any] struct { Value T }\n```\n\nSee [Client](#Client).\n\n| Member | Type |\n| --- | --- |\n| Value | T |\n";

#[test]
fn generated_reference_return_links_follow_mode_without_changing_code() {
    let input = Input::new(b"# Reference\n\n[Return](https://publication.example.test/guide/?q=one#step)\n\n`https://publication.example.test/guide/`\n");
    let modules = [ProjectModule {
        slug: "first",
        title: "First",
        manual: None,
        navigation: vec![ProjectItem::Reference(Reference {
            slug: "library",
            title: "Library",
            pages: None,
            artifact: input.artifact(Format::Markdown),
        })],
    }];
    let bases = gazette_core::publication::routing::PreviewLinkBases::try_from(BTreeMap::from([(
        "https://publication.example.test/".into(),
        "http://127.0.0.1:4501/".parse().unwrap(),
    )]))
    .unwrap();
    let target = Target::Mintlify {
        theme: "mint".into(),
        primary_color: "#123abc".into(),
        light_color: None,
        dark_color: None,
        telemetry: None,
    };
    let local = Mintlify::new(&Comrak)
        .with_preview_link_bases(Some(&bases))
        .assemble(
            Request {
                name: "Reference",
                public_base_url: None,
                target: &target,
                projects: &modules,
                usage: None,
                metadata: Default::default(),
            },
            Limits::default(),
        )
        .unwrap();
    let path = "first/library.md".parse().unwrap();
    let body = std::str::from_utf8(local.file(&path).unwrap()).unwrap();
    assert!(body.contains("[Return](http://127.0.0.1:4501/guide/?q=one#step)"));
    assert!(body.contains("`https://publication.example.test/guide/`"));
    let public = assemble(&[("first", &input)], Limits::default()).unwrap();
    let body = std::str::from_utf8(public.file(&path).unwrap()).unwrap();
    assert!(body.contains("[Return](https://publication.example.test/guide/?q=one#step)"));
    assert!(!body.contains("127.0.0.1"));
    let bad = Input::new(b"# Reference\n\n[Return](http://localhost:4501/guide/)\n");
    assert!(assemble(&[("first", &bad)], Limits::default()).is_err());
}

#[test]
fn selected_markdown_layouts_get_the_same_bounded_heading_navigation_support() {
    let source = b"# Library\n\n[Details](#details)\n\n:::accordion \"Details\"\n\n## Details\n\nKeep this explanation.\n\n:::\n";
    let input = Input::new(source);
    let output = assemble(&[("first", &input)], Limits::default()).unwrap();
    let page =
        std::str::from_utf8(output.file(&"first/library.md".parse().unwrap()).unwrap()).unwrap();
    assert!(page.contains("## Details {#details}"), "{page}");
    assert!(page.contains("[Details](#details)"), "{page}");
    assert!(
        page.contains("className=\"gazette-layout-disclosure\""),
        "{page}"
    );
    assert_eq!(
        output
            .file(&"gazette-layout-navigation.js".parse().unwrap())
            .unwrap(),
        include_bytes!("../src/layout-navigation.js"),
    );
    assert_eq!(
        input.tree.file(&"reference.json".parse().unwrap()),
        Some(source.as_slice())
    );
}

#[test]
fn selected_markdown_titles_and_aliases_get_clearance_without_layout_directives() {
    let source =
        b"# Library\n\n<a name=\"earlier-library\"></a>\n\n[Earlier name](#earlier-library).\n";
    let input = Input::new(source);
    let output = assemble(&[("first", &input)], Limits::default()).unwrap();
    let page =
        std::str::from_utf8(output.file(&"first/library.md".parse().unwrap()).unwrap()).unwrap();
    assert!(page.contains("title: \"Library\""), "{page}");
    assert!(
        page.contains("<a name=\"earlier&#45;library\"></a>"),
        "{page}"
    );
    assert!(page.contains("[Earlier name](#earlier-library)"), "{page}");
    assert!(
        output
            .file(&"gazette-layout-navigation.js".parse().unwrap())
            .is_some()
    );
    assert!(
        output
            .file(&"gazette-fragment-navigation.css".parse().unwrap())
            .is_some()
    );
    assert_eq!(
        input.tree.file(&"reference.json".parse().unwrap()),
        Some(source.as_slice())
    );
}

#[test]
fn generated_title_is_not_repeated_when_a_member_has_a_level_one_heading() {
    let source = b"# Class: AuthoringCorrection\n\nCopied compiler-proved edit.\n\n## Accessors\n\n### path\n\n#### Get Signature\n\n```ts\nget path(): string;\n```\n\n# Errors\n\nAn invalid member returns an error.\n\n##### Returns\n\n`string`\n\n[Errors](#errors)\n";
    let input = Input::new(source);
    let output = assemble(&[("first", &input)], Limits::default()).unwrap();
    let page = output.file(&"first/library.md".parse().unwrap()).unwrap();
    let (title, body) = target_parts(page);
    assert_eq!(title.as_deref(), Some("Class: AuthoringCorrection"));
    assert!(body.starts_with("<a id=\"class&#45;authoringcorrection\"></a>"));
    assert!(!body.contains("# Class:"));
    assert!(body.contains("# Errors {#errors}"));
    assert!(body.contains("get path(): string;"));
    assert!(body.contains("An invalid member returns an error\\."));
    assert!(body.contains("<Heading level={5} id=\"returns\""));
    assert!(body.contains("[Errors](#errors)"));
    assert_eq!(
        input.tree.file(&"reference.json".parse().unwrap()),
        Some(source.as_slice())
    );
}

fn navigation_page(path: &str) -> gazette_core::documentation::ReferencePage {
    gazette_core::documentation::ReferencePage::Page {
        path: path.parse().unwrap(),
    }
}

fn navigation_directory(path: &str) -> gazette_core::documentation::ReferencePage {
    gazette_core::documentation::ReferencePage::Directory {
        path: path.parse().unwrap(),
    }
}

fn target_parts(page: &[u8]) -> (Option<String>, &str) {
    let text = std::str::from_utf8(page).expect("UTF-8 target");
    match text.strip_prefix("---\ntitle: ") {
        Some(rest) => {
            let (title, body) = rest.split_once("\n---\n\n").expect("native title envelope");
            (
                Some(serde_json::from_str(title).expect("quoted title scalar")),
                body,
            )
        }
        None => (None, text),
    }
}

fn with_navigation(
    input: &Input,
    pages: &[gazette_core::documentation::ReferencePage],
) -> Result<CapturedSource, Error> {
    let project = ProjectModule {
        slug: "first",
        title: "first",
        manual: None,
        navigation: vec![ProjectItem::Reference(Reference {
            slug: "library",
            title: "Library API",
            pages: Some(pages),
            artifact: input.artifact(Format::Markdown),
        })],
    };
    Mintlify::new(&Comrak).assemble(
        Request {
            name: "Reference",
            public_base_url: None,
            target: &Target::Mintlify {
                theme: "mint".into(),
                primary_color: "#123abc".into(),
                light_color: None,
                dark_color: None,
                telemetry: None,
            },
            projects: &[project],
            usage: None,
            metadata: Default::default(),
        },
        Limits::default(),
    )
}

#[test]
fn explicit_nested_navigation_changes_only_configuration_not_complete_page_bytes() {
    use gazette_core::documentation::ReferencePage;
    let input = directory(
        &[
            ("ref/index.md", b"# Package\n\n[Value](types/Value.md)\n"),
            ("ref/types/Value.md", b"# Value\n\n[Package](../index.md)\n"),
            ("ref/classes/Client.md", b"# Client\n"),
        ],
        Some("ref"),
    );
    let legacy = assemble(&[("first", &input)], Limits::default()).unwrap();
    let pages = [
        navigation_page("index"),
        ReferencePage::Group {
            title: "Types".into(),
            pages: vec![
                navigation_page("types/Value"),
                ReferencePage::Group {
                    title: "Classes".into(),
                    pages: vec![navigation_page("classes/Client")],
                },
            ],
        },
    ];
    let output = with_navigation(&input, &pages).unwrap();
    let config: Value =
        serde_json::from_slice(output.file(&"docs.json".parse().unwrap()).unwrap()).unwrap();
    assert_eq!(
        config["navigation"]["groups"][0]["pages"][0]["pages"],
        json!(["first/library/gazette-page-index",{"group":"Types","pages":["first/library/types/Value",{"group":"Classes","pages":["first/library/classes/Client"]}]}])
    );
    let expected: Vec<_> = legacy
        .inventory()
        .entries()
        .iter()
        .map(|e| &e.path)
        .collect();
    let actual: Vec<_> = output
        .inventory()
        .entries()
        .iter()
        .map(|e| &e.path)
        .collect();
    assert_eq!(actual, expected);
    for path in expected {
        if path.as_str() != "docs.json" {
            assert_eq!(output.file(path), legacy.file(path), "changed {path}");
        }
    }
}

#[test]
fn explicit_navigation_cannot_hide_add_or_repeat_a_selected_markdown_page() {
    let input = directory(
        &[("index.md", b"# Index\n"), ("Value.md", b"# Value\n")],
        None,
    );
    for pages in [
        vec![navigation_page("index")],
        vec![
            navigation_page("index"),
            navigation_page("Value"),
            navigation_page("extra"),
        ],
        vec![
            navigation_page("index"),
            navigation_page("Value"),
            navigation_page("index"),
        ],
    ] {
        assert!(with_navigation(&input, &pages).is_err());
    }
    assert!(
        with_navigation(&Input::new(PAGE), &[navigation_page("index")]).is_err(),
        "single-file override refused before conversion"
    );
}

#[test]
fn directory_navigation_derives_nested_groups_and_preserves_every_page_byte() {
    use gazette_core::documentation::ReferencePage;
    let input = directory(
        &[
            ("ref/index.md", b"# Overview\n"),
            ("ref/js/Apple.md", b"# Apple\n"),
            ("ref/js/index.md", b"# JavaScript\n"),
            ("ref/js/types/Value.md", b"# Value\n"),
            ("ref/js/types/nested/index.md", b"# Nested\n"),
            ("ref/wasm/index.md", b"# WebAssembly\n"),
        ],
        Some("ref"),
    );
    let pages = [
        navigation_page("index"),
        ReferencePage::Group {
            title: "JavaScript".into(),
            pages: vec![navigation_directory("js")],
        },
        ReferencePage::Group {
            title: "WebAssembly".into(),
            pages: vec![navigation_directory("wasm")],
        },
    ];
    let output = with_navigation(&input, &pages).unwrap();
    let legacy = assemble(&[("first", &input)], Limits::default()).unwrap();
    let config: Value =
        serde_json::from_slice(output.file(&"docs.json".parse().unwrap()).unwrap()).unwrap();
    assert_eq!(
        config["navigation"]["groups"][0]["pages"][0]["pages"],
        json!([
            "first/library/gazette-page-index",
            {"group":"JavaScript","pages":["first/library/js/gazette-page-index","first/library/js/Apple",{
                "group":"types","pages":["first/library/js/types/Value",{
                    "group":"nested","pages":["first/library/js/types/nested/gazette-page-index"]
                }]
            }]},
            {"group":"WebAssembly","pages":["first/library/wasm/gazette-page-index"]}
        ])
    );
    assert_eq!(
        output.inventory().entries().len(),
        legacy.inventory().entries().len()
    );
    for entry in legacy.inventory().entries() {
        if entry.path.as_str() != "docs.json" {
            assert_eq!(output.file(&entry.path), legacy.file(&entry.path));
        }
    }
    assert_eq!(
        output.inventory(),
        with_navigation(&input, &pages).unwrap().inventory()
    );
}

#[test]
fn directory_navigation_refuses_empty_overlap_and_incomplete_prefix_selections() {
    let input = directory(
        &[
            ("js/index.md", b"# JS\n"),
            ("js/types/Value.md", b"# Value\n"),
            ("js-other/index.md", b"# Other\n"),
        ],
        None,
    );
    for pages in [
        vec![navigation_directory("absent")],
        vec![navigation_directory("js/types/Value")],
        vec![navigation_directory("js")],
        vec![
            navigation_directory("js"),
            navigation_directory("js-other"),
            navigation_page("js/index"),
        ],
        vec![
            navigation_directory("js"),
            navigation_directory("js-other"),
            navigation_directory("js/types"),
        ],
        vec![
            navigation_directory("js/types"),
            navigation_directory("js"),
            navigation_directory("js-other"),
        ],
    ] {
        assert!(
            with_navigation(&input, &pages).is_err(),
            "accepted {pages:?}"
        );
    }
}

#[test]
fn derived_and_authored_groups_share_the_navigation_depth_limit() {
    use gazette_core::documentation::ReferencePage;
    let input = directory(&[("js/types/Value.md", b"# Value\n")], None);
    let mut node = navigation_directory("js");
    for _ in 0..63 {
        node = ReferencePage::Group {
            title: "Nested".into(),
            pages: vec![node],
        };
    }
    with_navigation(&input, std::slice::from_ref(&node)).unwrap();
    let excessive = ReferencePage::Group {
        title: "Too deep".into(),
        pages: vec![node],
    };
    assert!(with_navigation(&input, &[excessive]).is_err());
}

fn assert_markdown_projection(page: &[u8], source: &[u8], ids: &[&str]) {
    use gazette_core::markdown::{Limits as MarkdownLimits, Profile};
    use gazette_ports::markdown::Markdown;

    let parse = |bytes| {
        Comrak
            .parse(
                std::str::from_utf8(bytes).expect("UTF-8 Markdown"),
                Profile::Publication,
                MarkdownLimits::default(),
            )
            .expect("native Markdown facts")
    };
    let original = parse(source);
    let (title, body) = target_parts(page);
    let body = if title.is_some() {
        let (anchor, body) = body.split_once('\n').expect("native title anchor");
        let anchors =
            gazette_core::documentation::projection::manual_html_anchors(anchor, "target").unwrap();
        assert!(
            anchors.contains(ids[0]),
            "original title anchor must remain inert"
        );
        body
    } else {
        body
    };
    let emitted = parse(body.as_bytes());
    let removed = title.as_ref().map(|title| {
        let heading = &original.syntax().headings[0];
        assert_eq!(title, &heading.text);
        heading.location.span
    });
    let text = |document: &dyn gazette_ports::markdown::Document,
                removed: Option<gazette_core::source::Span>| {
        document
            .syntax()
            .text
            .iter()
            .filter(|part| {
                !removed.is_some_and(|span| {
                    part.location.span.start >= span.start && part.location.span.end <= span.end
                })
            })
            .map(|part| part.value.as_str())
            .collect::<String>()
    };
    assert_eq!(
        text(emitted.as_ref(), None),
        text(original.as_ref(), removed)
    );
    assert_eq!(
        emitted
            .syntax()
            .headings
            .iter()
            .map(|heading| (heading.level, heading.text.as_str()))
            .collect::<Vec<_>>(),
        original
            .syntax()
            .headings
            .iter()
            .skip(usize::from(title.is_some()))
            .map(|heading| (heading.level, heading.text.as_str()))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        emitted
            .syntax()
            .headings
            .iter()
            .map(|heading| heading.id.as_deref().expect("explicit target anchor"))
            .collect::<Vec<_>>(),
        &ids[usize::from(title.is_some())..]
    );
    assert_eq!(
        emitted
            .syntax()
            .links
            .iter()
            .map(|link| (link.destination.as_str(), link.title.as_str()))
            .collect::<Vec<_>>(),
        original
            .syntax()
            .links
            .iter()
            .map(|link| (link.destination.as_str(), link.title.as_str()))
            .collect::<Vec<_>>()
    );
}

fn assemble(inputs: &[(&str, &Input)], limits: Limits) -> Result<CapturedSource, Error> {
    let projects: Vec<_> = inputs
        .iter()
        .map(|(slug, input)| ProjectModule {
            slug,
            title: slug,
            manual: None,
            navigation: vec![ProjectItem::Reference(Reference {
                pages: None,
                slug: "library",
                title: "Library API",
                artifact: input.artifact(Format::Markdown),
            })],
        })
        .collect();
    Mintlify::new(&Comrak).assemble(
        Request {
            name: "Reference",
            public_base_url: None,
            target: &Target::Mintlify {
                theme: "mint".into(),
                primary_color: "#123abc".into(),
                light_color: None,
                dark_color: None,
                telemetry: None,
            },
            projects: &projects,
            usage: None,
            metadata: Default::default(),
        },
        limits,
    )
}

#[test]
fn a_sole_plain_leading_h1_becomes_target_metadata_with_its_original_anchor() {
    let source = b"# Interface: SearchIndex\\<R extends SearchRecord = SearchRecord\\> {#search-index}\n\n[Self](#search-index) and [Record](Record.md#record).\n\n```typescript\nreadonly records?: ReadonlyArray<R>;\n```\n";
    let input = directory(&[("Index.md", source), ("Record.md", b"# Record\n")], None);
    let output = assemble(&[("first", &input)], Limits::default()).unwrap();
    let page = output
        .file(&"first/library/Index.md".parse().unwrap())
        .unwrap();
    let (title, body) = target_parts(page);
    assert_eq!(
        title.as_deref(),
        Some("Interface: SearchIndex<R extends SearchRecord = SearchRecord>")
    );
    assert!(body.contains("readonly records?: ReadonlyArray<R>;"));
    assert!(body.contains("[Record](/first/library/Record#record)"));
    assert!(body.contains("[Self](#search-index)"));
    assert!(!body.contains("# Interface:"));
    let anchors = gazette_core::documentation::projection::manual_html_anchors(
        body.lines().next().unwrap(),
        "Index",
    )
    .unwrap();
    assert!(anchors.contains("search-index"));
    assert_eq!(
        input.tree.file(&"Index.md".parse().unwrap()),
        Some(source.as_slice()),
        "the captured producer body is immutable"
    );
}

#[test]
fn target_titles_quote_native_text_without_turning_it_into_yaml_fields() {
    for (source, expected) in [
        (
            "# API: \"quoted\" \\\\ path \\<T\\>\n",
            "API: \"quoted\" \\ path <T>",
        ),
        (
            "A plain setext title\n====================\n",
            "A plain setext title",
        ),
    ] {
        let input = Input::new(source.as_bytes());
        let output = assemble(&[("first", &input)], Limits::default()).unwrap();
        let (title, body) =
            target_parts(output.file(&"first/library.md".parse().unwrap()).unwrap());
        assert_eq!(title.as_deref(), Some(expected));
        assert!(body.starts_with("<a id=\""));
        assert!(!body.contains("# API:"));
    }
}

#[test]
fn noneligible_generated_titles_keep_their_existing_heading_and_inline_structure() {
    use gazette_core::markdown::{Limits as MarkdownLimits, Profile};
    use gazette_ports::markdown::Markdown;
    for source in [
        "A retained preamble.\n\n# Reference\n",
        "<!-- retained source comment -->\n\n# Reference\n",
        "## No top title\n",
        "#\n",
        "# Rich *emphasis* and `code<T>`\n",
        "# Interface: Query\\<R *extends* [Record](https://example.test/record) = [Record](https://example.test/record)\\>\n",
        "# API <span>safe</span>\n",
    ] {
        let input = Input::new(source.as_bytes());
        let output = assemble(&[("first", &input)], Limits::default()).unwrap();
        let (title, body) =
            target_parts(output.file(&"first/library.md".parse().unwrap()).unwrap());
        assert_eq!(title, None, "{source}");
        let original = Comrak
            .parse(source, Profile::Publication, MarkdownLimits::default())
            .unwrap();
        let projected = Comrak
            .parse(body, Profile::Publication, MarkdownLimits::default())
            .unwrap();
        assert_eq!(
            projected
                .syntax()
                .headings
                .iter()
                .map(|h| (&h.text, h.level))
                .collect::<Vec<_>>(),
            original
                .syntax()
                .headings
                .iter()
                .map(|h| (&h.text, h.level))
                .collect::<Vec<_>>(),
            "{source}"
        );
        assert_eq!(
            projected
                .syntax()
                .links
                .iter()
                .map(|l| &l.destination)
                .collect::<Vec<_>>(),
            original
                .syntax()
                .links
                .iter()
                .map(|l| &l.destination)
                .collect::<Vec<_>>(),
            "{source}"
        );
        if source.contains("*emphasis*") {
            assert!(body.contains("*emphasis*"));
            assert!(body.contains("`code<T>`"));
        }
        if source.contains("<span>") {
            assert!(body.contains("<span>safe</span>"));
        }
    }
}

#[test]
fn derived_title_bytes_still_obey_the_complete_target_file_ceiling() {
    let source = format!("# {}\n\nBody.\n", "Title".repeat(1024));
    let input = Input::new(source.as_bytes());
    let output = assemble(&[("first", &input)], Limits::default()).unwrap();
    let bytes = output
        .file(&"first/library.md".parse().unwrap())
        .unwrap()
        .len();
    assert!(bytes > output.file(&"docs.json".parse().unwrap()).unwrap().len());
    assert!(
        assemble(
            &[("first", &input)],
            Limits {
                file_bytes: (bytes - 1) as u64,
                ..Limits::default()
            }
        )
        .is_err()
    );
}

#[test]
fn the_private_index_uses_projected_titles_after_the_body_h1_becomes_an_anchor() {
    use gazette_ports::markdown::Markdown;
    for (source, expected) in [
        (
            "# Interface: Result\\<R extends SearchRecord = SearchRecord\\>\n",
            "Interface: Result<R extends SearchRecord = SearchRecord>",
        ),
        ("# Type Alias: Captured\\<T\\>\n", "Type Alias: Captured<T>"),
    ] {
        let input = Input::new(source.as_bytes());
        let projects = [ProjectModule {
            slug: "first",
            title: "First",
            manual: None,
            navigation: vec![ProjectItem::Reference(Reference {
                slug: "library",
                title: "Library",
                pages: None,
                artifact: input.artifact(Format::Markdown),
            })],
        }];
        let bases =
            gazette_core::publication::routing::PreviewLinkBases::try_from(BTreeMap::from([(
                "https://example.test/".into(),
                "http://127.0.0.1:4501/".parse().unwrap(),
            )]))
            .unwrap();
        let output = Mintlify::new(&Comrak)
            .with_preview_link_bases(Some(&bases))
            .assemble(
                Request {
                    name: "Reference",
                    public_base_url: None,
                    target: &Target::Mintlify {
                        theme: "mint".into(),
                        primary_color: "#123abc".into(),
                        light_color: None,
                        dark_color: None,
                        telemetry: None,
                    },
                    projects: &projects,
                    usage: None,
                    metadata: Default::default(),
                },
                Limits::default(),
            )
            .unwrap();
        let index =
            std::str::from_utf8(output.file(&"llms.txt".parse().unwrap()).unwrap()).unwrap();
        let parsed = Comrak
            .parse(
                index,
                gazette_core::markdown::Profile::Publication,
                gazette_core::markdown::Limits::default(),
            )
            .unwrap();
        assert!(
            parsed.syntax().html.is_empty(),
            "title text must not become HTML: {index}"
        );
        let link = &parsed.syntax().links[0];
        assert_eq!(link.destination, "/first/library");
        let label = parsed
            .syntax()
            .text
            .iter()
            .filter(|text| {
                text.location.span.start >= link.location.span.start
                    && text.location.span.end <= link.location.span.end
                    && text.value != "\n"
            })
            .map(|text| text.value.as_str())
            .collect::<String>();
        assert_eq!(label, expected, "{index}");
    }
}

#[test]
fn target_anchor_projection_preserves_source_catalogs_forward_links_and_raw_anchor_identity() {
    let index = b"# Page title\n\n[Self](#page-title) [Forward](Target.md#content) [Encoded](Target.md#%63ontent) [Raw](Target.md#gazette-anchor-content) [Name](Target.md#page-title) [Cross](/second/library.md#content) [External](https://example.test/Target#content).\n";
    let target = include_bytes!("fixtures/deep-headings.md");
    let first = directory(&[("Index.md", index), ("Target.md", target)], None);
    let second = Input::new(b"# Other\n\n## content\n");
    let output = assemble(&[("first", &first), ("second", &second)], Limits::default()).unwrap();
    let text = std::str::from_utf8(
        output
            .file(&"first/library/Index.md".parse().unwrap())
            .unwrap(),
    )
    .unwrap();
    for expected in [
        "(#gazette-anchor-page-title)",
        "(/first/library/Target#gazette-anchor-content)",
        "(/first/library/Target#gazette-anchor-%63ontent)",
        "(/first/library/Target#gazette-anchor-gazette-anchor-content)",
        "(/first/library/Target#gazette-anchor-page-title)",
        "(/second/library#gazette-anchor-content)",
        "(https://example.test/Target#content)",
    ] {
        assert!(text.contains(expected), "missing {expected}: {text}");
    }
    assert!(
        text.contains("<a id=\"gazette&#45;anchor&#45;page&#45;title\"></a>"),
        "the projected title anchor is encoded exactly once"
    );
    let body = std::str::from_utf8(
        output
            .file(&"first/library/Target.md".parse().unwrap())
            .unwrap(),
    )
    .unwrap();
    assert!(
        body.contains("## content {#gazette-anchor-content}"),
        "{body}"
    );
    assert!(
        body.contains("<Heading level={5} id=\"workspace\" className=\"scroll-mt-(--scroll-mt)\">workspace **strong** *emphasis* [link](https://example.test/) and `code`</Heading>"),
        "{body}"
    );
    assert!(
        body.contains("<Heading level={6} id=\"workspace&#45;1\" className=\"scroll-mt-(--scroll-mt)\">workspace</Heading>"),
        "{body}"
    );
    assert!(
        body.contains("id=\"gazette&#45;anchor&#45;gazette&#45;anchor&#45;content\""),
        "{body}"
    );
    assert!(
        body.contains("name=\"gazette&#45;anchor&#45;page&#45;title\""),
        "{body}"
    );
    assert_eq!(
        first.tree.file(&"Index.md".parse().unwrap()),
        Some(index.as_slice())
    );
    assert_eq!(
        first.tree.file(&"Target.md".parse().unwrap()),
        Some(target.as_slice())
    );

    let invalid = directory(
        &[
            (
                "Index.md",
                b"# Index\n\n[Invented target alias](Target.md#gazette-anchor-content)\n",
            ),
            ("Target.md", b"# Target\n\n## content\n"),
        ],
        None,
    );
    assert!(
        assemble(&[("first", &invalid)], Limits::default()).is_err(),
        "a target alias is not admitted as a source fragment"
    );
    for source in [
        b"# Unsafe\n\n<h5>Lost</h5>\n".as_slice(),
        b"# Unsafe\n\n<Heading level={5}>Authored</Heading>\n",
        b"# Unsafe\n\n<script>active()</script>\n",
    ] {
        assert!(assemble(&[("first", &Input::new(source))], Limits::default()).is_err());
    }
}

#[test]
fn selected_markdown_meaning_and_module_navigation_are_preserved_deterministically() {
    let input = Input::new(PAGE);
    let inputs = [("first", &input), ("second", &input)];
    let result = assemble(&inputs, Limits::default()).expect("complete Markdown target");
    let again = assemble(&inputs, Limits::default()).expect("same retained inputs");
    assert_eq!(result.inventory(), again.inventory());
    for slug in ["first", "second"] {
        assert_markdown_projection(
            result
                .file(&format!("{slug}/library.md").parse().expect("page path"))
                .expect("emitted page"),
            PAGE,
            &["library-reference", "client"],
        );
    }
    assert!(
        result
            .file(&"reference.json".parse().expect("input path"))
            .is_none()
    );
    let config: Value = serde_json::from_slice(
        result
            .file(&"docs.json".parse().expect("config path"))
            .expect("config bytes"),
    )
    .expect("native configuration");
    assert_eq!(
        config["navigation"],
        json!({"dropdowns":[
            {"dropdown":"first","pages":[{"group":"Library API","pages":["first/library"]}]},
            {"dropdown":"second","pages":[{"group":"Library API","pages":["second/library"]}]}
        ]})
    );
    assert_eq!(
        input
            .tree
            .file(&"reference.json".parse().expect("retained path")),
        Some(PAGE)
    );
}

#[test]
fn directory_selection_and_unsupported_relative_inputs_fail_without_successful_subsets() {
    let good = Input::new(PAGE);
    let mut directory = Input::new(PAGE);
    directory.selection = Selection::Directory { path: None };
    assert!(
        assemble(
            &[("first", &good), ("second", &directory)],
            Limits::default()
        )
        .is_err()
    );
    for bytes in [
        b"".as_slice(),
        b"\xff",
        b"text\0",
        b"---\ntitle: Custom\n---\n# Page",
        "\u{feff}---\ntitle: Custom\n---\n# Page".as_bytes(),
        b"--- \t\r\ntitle: Custom\r\n---\r\n# Page",
        b"  ---\ntitle: Custom\n---\n# Page",
        "\u{feff}+++ \t\r\ntitle = 'Custom'\r\n+++\r\n# Page".as_bytes(),
        b"+++\ntitle = 'Custom'\n+++\n# Page",
        b"[sibling](other.md)",
        b"![asset](image.png)",
        b"[escape](../private.md)",
        b"[active](javascript:run())",
        b"[root](/other)",
        b"[empty]()",
        b"<script>alert(1)</script>",
        b"<span onclick=\"alert(1)\">active</span>",
        b"<img src=\"https://example.test/image.png\">",
        b"<a href=\"https://example.test/\">raw bypass</a>",
    ] {
        let invalid = Input::new(bytes);
        assert!(
            assemble(&[("first", &good), ("second", &invalid)], Limits::default()).is_err(),
            "unsupported input {bytes:?}"
        );
    }
}

#[test]
fn target_and_markdown_analysis_limits_are_both_enforced() {
    let input = Input::new(PAGE);
    assert!(
        assemble(
            &[("first", &input)],
            Limits {
                file_bytes: 1,
                ..Limits::default()
            }
        )
        .is_err()
    );
    let large = Input::new(&vec![b'x'; 1024 * 1024 + 1]);
    assert!(matches!(
        assemble(&[("first", &large)], Limits::default()),
        Err(Error::Markdown(_))
    ));
}

#[test]
fn selected_directory_preserves_nested_pages_links_casing_and_deterministic_navigation() {
    let overview = b"# Overview\n[Client](./types/Client#custom-anchor)\n[Self](#overview)\n";
    let client = b"# Client {#custom-anchor}\n[Overview](../README)\n[Self](./Client)\n";
    let input = directory(
        &[
            ("generated/types/Client.md", client),
            ("ignored.txt", b"not selected"),
            ("generated/README.md", overview),
        ],
        Some("generated"),
    );
    let result = assemble(&[("first", &input)], Limits::default()).expect("complete pages");
    let again = assemble(&[("first", &input)], Limits::default()).expect("deterministic pages");
    assert_eq!(result.inventory(), again.inventory());
    for (path, bytes, id) in [
        ("first/library/README.md", b"# Overview\n[Client](/first/library/types/Client#custom-anchor)\n[Self](#overview)\n".as_slice(), "overview"),
        (
            "first/library/types/Client.md",
            b"# Client {#custom-anchor}\n[Overview](/first/library/README)\n[Self](/first/library/types/Client)\n".as_slice(),
            "custom-anchor",
        ),
    ] {
        assert_markdown_projection(
            result
                .file(&path.parse().expect("output path"))
                .expect("emitted page"),
            bytes,
            &[id],
        );
    }
    let config: Value = serde_json::from_slice(
        result
            .file(&"docs.json".parse().expect("config path"))
            .expect("configuration"),
    )
    .expect("JSON");
    assert_eq!(
        config["navigation"]["groups"][0]["pages"][0]["pages"],
        json!(["first/library/README", "first/library/types/Client"])
    );
    assert!(
        result
            .file(&"ignored.txt".parse().expect("unselected path"))
            .is_none()
    );
    assert_eq!(
        input
            .tree
            .file(&"generated/README.md".parse().expect("input page")),
        Some(overview.as_slice())
    );
}

#[test]
fn capture_root_directory_uses_the_same_packaging_and_markdown_parser() {
    let input = directory(
        &[
            ("Start.md", b"[Reference][api]\n\n[api]: ./Reference\n"),
            ("Reference.md", PAGE),
        ],
        None,
    );
    let result = assemble(&[("first", &input)], Limits::default())
        .expect("root pages with reference-style link");
    assert_markdown_projection(
        result
            .file(&"first/library/Reference.md".parse().expect("page path"))
            .expect("emitted reference"),
        PAGE,
        &["library-reference", "client"],
    );
}

#[test]
fn selected_pages_never_silently_drop_companions_or_unresolved_links() {
    let good = Input::new(PAGE);
    for destination in [
        "./missing",
        "../escape",
        "./Missing.md",
        "../escape.md",
        "./target.md",
        "./Target.md?view=all",
        "./Target?view=all",
        "./Target#",
        "/Target",
        "./target",
        "%54arget",
        "./nested/../../escape",
    ] {
        let page = format!("[Target]({destination})\n");
        let input = directory(&[("Start.md", page.as_bytes()), ("Target.md", PAGE)], None);
        assert!(
            assemble(&[("first", &good), ("second", &input)], Limits::default()).is_err(),
            "accepted {destination}"
        );
    }
    for name in [
        "asset.png",
        "other.mdx",
        "other.MD",
        "space name.md",
        "dot.name.md",
    ] {
        let input = directory(&[("Start.md", PAGE), (name, PAGE)], None);
        assert!(
            assemble(&[("first", &good), ("second", &input)], Limits::default()).is_err(),
            "dropped {name}"
        );
    }
    let image = directory(
        &[
            ("Start.md", b"![not an image](./Target)"),
            ("Target.md", PAGE),
        ],
        None,
    );
    assert!(assemble(&[("first", &image)], Limits::default()).is_err());
    let empty = directory(&[], None);
    assert!(assemble(&[("first", &empty)], Limits::default()).is_err());
}

#[test]
fn directory_pages_share_text_and_output_limit_validation() {
    for bytes in [
        b"".as_slice(),
        b"\xff",
        b"---\ntitle: Override\n---\n# Page",
        b"text\0",
    ] {
        let input = directory(&[("Start.md", PAGE), ("Bad.md", bytes)], None);
        assert!(assemble(&[("first", &input)], Limits::default()).is_err());
    }
    let input = directory(&[("Start.md", PAGE), ("End.md", PAGE)], None);
    assert!(
        assemble(
            &[("first", &input)],
            Limits {
                file_bytes: 1,
                ..Limits::default()
            }
        )
        .is_err()
    );
}

#[test]
fn source_links_convert_through_markdown_structure_not_text_replacement() {
    use gazette_core::markdown::{Limits as MarkdownLimits, Profile};
    use gazette_ports::markdown::Markdown;

    let start = b"# Start\n\n[Client][api] and [`Client`](./types/Client.md#method \"call\").\n\n[api]: ./types/Client.md#method\n\n```rust\n// [Client](./types/Client.md#method)\npub fn call<T>(arg: &T) {}\n```\n\n<a id=\"start\"></a>\n";
    let client = b"# Client\n\n## Method\n\n[Back](../Start.md#start)\n";
    let input = directory(&[("Start.md", start), ("types/Client.md", client)], None);
    let output = assemble(&[("first", &input)], Limits::default()).expect("converted target");
    let again = assemble(&[("first", &input)], Limits::default()).expect("same retained inputs");
    assert_eq!(output.inventory(), again.inventory());
    let page = output
        .file(&"first/library/Start.md".parse().expect("page"))
        .expect("converted bytes");
    let text = std::str::from_utf8(page).expect("UTF-8");
    let document = Comrak
        .parse(text, Profile::Publication, MarkdownLimits::default())
        .expect("native syntax");
    assert_eq!(
        document
            .syntax()
            .links
            .iter()
            .map(|link| link.destination.as_str())
            .collect::<Vec<_>>(),
        [
            "/first/library/types/Client#method",
            "/first/library/types/Client#method"
        ]
    );
    assert!(text.contains("// [Client](./types/Client.md#method)\npub fn call<T>(arg: &T) {}"));
    assert!(text.contains("<a id=\"start\"></a>"));
    assert_eq!(
        input.tree.file(&"Start.md".parse().expect("input")),
        Some(start.as_slice())
    );
    assert_ne!(Sha256Digest::of(start), Sha256Digest::of(page));
    let back = output
        .file(
            &"first/library/types/Client.md"
                .parse()
                .expect("nested page"),
        )
        .expect("nested bytes");
    assert!(
        std::str::from_utf8(back)
            .expect("UTF-8")
            .contains("(/first/library/Start#start)")
    );
}

#[test]
fn conversion_preserves_qualified_ids_and_refuses_unqualified_heading_classes() {
    let input = directory(
        &[
            ("Start.md", b"# Start {#custom}\n\n[Target](Target.md)\n"),
            ("Target.md", PAGE),
        ],
        None,
    );
    let converted = assemble(&[("first", &input)], Limits::default())
        .expect("explicit heading ID has qualified target syntax");
    assert_eq!(
        converted.file(&"first/library/Start.md".parse().expect("page")),
        Some(
            b"---\ntitle: \"Start\"\n---\n\n<a id=\"custom\"></a>\n\n[Target](/first/library/Target)\n".as_slice()
        )
    );
    let classes = directory(
        &[
            (
                "Start.md",
                b"# Start {#custom .panel}\n\n[Target](Target.md)\n",
            ),
            ("Target.md", PAGE),
        ],
        None,
    );
    assert!(matches!(
        assemble(&[("first", &classes)], Limits::default()),
        Err(Error::Markdown(
            gazette_core::markdown::Error::Unsupported { .. }
        ))
    ));
    let unchanged = directory(
        &[
            ("Start.md", b"# Start {#custom}\n\n[Target](Target)\n"),
            ("Target.md", PAGE),
        ],
        None,
    );
    let output = assemble(&[("first", &unchanged)], Limits::default())
        .expect("plain title projection keeps unchanged link destinations");
    assert_eq!(
        output.file(&"first/library/Start.md".parse().expect("page")),
        Some(
            b"---\ntitle: \"Start\"\n---\n\n<a id=\"custom\"></a>\n\n[Target](/first/library/Target)\n".as_slice()
        )
    );
}

#[test]
fn markdown_adapter_navigation_accepts_qualified_roots_without_guessing_symbols() {
    let api =
        Input::new(br#"{"openapi":"3.0.3","info":{"title":"HTTP API","version":"1"},"paths":{}}"#);
    let exact_root = Input::new(b"[API](/api-project/http)\n");
    let projects = [
        ProjectModule {
            slug: "first",
            title: "First",
            manual: None,
            navigation: vec![ProjectItem::Reference(Reference {
                pages: None,
                slug: "library",
                title: "Library",
                artifact: exact_root.artifact(Format::Markdown),
            })],
        },
        ProjectModule {
            slug: "api-project",
            title: "API",
            manual: None,
            navigation: vec![ProjectItem::Reference(Reference {
                pages: None,
                slug: "http",
                title: "HTTP API",
                artifact: api.artifact(Format::OpenApiJson),
            })],
        },
    ];
    Mintlify::new(&Comrak)
        .assemble(
            Request {
                name: "Reference",
                public_base_url: None,
                target: &Target::Mintlify {
                    theme: "mint".into(),
                    primary_color: "#123abc".into(),
                    light_color: None,
                    dark_color: None,
                    telemetry: None,
                },
                projects: &projects,
                usage: None,
                metadata: Default::default(),
            },
            Limits::default(),
        )
        .expect("the exact qualified provider root is a valid page target");

    for destination in [
        "/api-project/http#guessed-symbol",
        "/api-project/http/guessed-symbol",
        "/artifacts/api-project/http.json",
    ] {
        let source = format!("[API]({destination})\n");
        let input = Input::new(source.as_bytes());
        let projects = [
            ProjectModule {
                slug: "first",
                title: "First",
                manual: None,
                navigation: vec![ProjectItem::Reference(Reference {
                    pages: None,
                    slug: "library",
                    title: "Library",
                    artifact: input.artifact(Format::Markdown),
                })],
            },
            ProjectModule {
                slug: "api-project",
                title: "API",
                manual: None,
                navigation: vec![ProjectItem::Reference(Reference {
                    pages: None,
                    slug: "http",
                    title: "HTTP API",
                    artifact: api.artifact(Format::OpenApiJson),
                })],
            },
        ];
        Mintlify::new(&Comrak)
            .assemble(
                Request {
                    name: "Reference",
                    public_base_url: None,
                    target: &Target::Mintlify {
                        theme: "mint".into(),
                        primary_color: "#123abc".into(),
                        light_color: None,
                        dark_color: None,
                        telemetry: None,
                    },
                    projects: &projects[1..],
                    usage: None,
                    metadata: Default::default(),
                },
                Limits::default(),
            )
            .expect("adapter input is valid independently of the unresolved page link");
        assert!(
            Mintlify::new(&Comrak)
                .assemble(
                    Request {
                        name: "Reference",
                        public_base_url: None,
                        target: &Target::Mintlify {
                            theme: "mint".into(),
                            primary_color: "#123abc".into(),
                            light_color: None,
                            dark_color: None,
                            telemetry: None,
                        },
                        projects: &projects,
                        usage: None,
                        metadata: Default::default(),
                    },
                    Limits::default(),
                )
                .is_err(),
            "guessed dynamic adapter page or symbol {destination}"
        );
    }
}

#[test]
fn reserved_index_pages_keep_distinct_navigation_links_and_source_evidence() {
    let package =
        b"# Package\n\n[Module](index/index.md#module)\n[Escaped](gazette-page-index.md)\n";
    let module = b"# Module\n\n[Package](../index.md)\n[Self](#module)\n";
    let escaped = b"# Original escaped name\n\n[Package](/first/library/index#package)\n";
    let input = directory(
        &[
            ("index.md", package),
            ("index/index.md", module),
            ("gazette-page-index.md", escaped),
        ],
        None,
    );
    let output = with_navigation(
        &input,
        &[
            navigation_page("index"),
            navigation_page("index/index"),
            navigation_page("gazette-page-index"),
        ],
    )
    .expect("distinct target routes");
    let config: Value =
        serde_json::from_slice(output.file(&"docs.json".parse().unwrap()).unwrap()).unwrap();
    assert_eq!(
        config["navigation"]["groups"][0]["pages"][0]["pages"],
        json!([
            "first/library/gazette-page-index",
            "first/library/index/gazette-page-index",
            "first/library/gazette-page-gazette-page-index"
        ])
    );
    for (source, target, original, links) in [
        (
            "index.md",
            "first/library/gazette-page-index.md",
            package.as_slice(),
            vec![
                "/first/library/index/gazette-page-index#module",
                "/first/library/gazette-page-gazette-page-index",
            ],
        ),
        (
            "index/index.md",
            "first/library/index/gazette-page-index.md",
            module.as_slice(),
            vec!["/first/library/gazette-page-index", "#module"],
        ),
        (
            "gazette-page-index.md",
            "first/library/gazette-page-gazette-page-index.md",
            escaped.as_slice(),
            vec!["/first/library/gazette-page-index#package"],
        ),
    ] {
        let bytes = output
            .file(&target.parse().unwrap())
            .expect("physical target");
        let (_, body) = target_parts(bytes);
        use gazette_ports::markdown::Markdown;
        let document = Comrak
            .parse(
                body,
                gazette_core::markdown::Profile::Publication,
                gazette_core::markdown::Limits::default(),
            )
            .unwrap();
        assert_eq!(
            document
                .syntax()
                .links
                .iter()
                .map(|link| link.destination.as_str())
                .collect::<Vec<_>>(),
            links
        );
        assert_eq!(
            input.tree.file(&source.parse().unwrap()),
            Some(original),
            "source evidence stays exact"
        );
    }
    assert!(
        output
            .file(&"first/library/index.md".parse().unwrap())
            .is_none()
    );
    let broken = directory(
        &[
            (
                "index.md",
                b"# Package\n\n[Module](index/index.md#missing)\n",
            ),
            ("index/index.md", module),
        ],
        None,
    );
    assert!(
        assemble(&[("first", &broken)], Limits::default()).is_err(),
        "route projection must not bypass source anchor validation"
    );
}
