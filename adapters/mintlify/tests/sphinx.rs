//! Structural packaging proof with representative native contexts, not a Sphinx run.

use gazette_adapter_mintlify::Mintlify;
use gazette_core::{
    documentation::{
        Artifact, Error, Format, ManualPage, ManualSection, ManualSource, ProjectItem,
        ProjectModule, Reference, Request, Selection, Target,
    },
    source::{CapturedSource, Content, EntryPath, InventoryBuilder, Limits, Sha256Digest},
};
use gazette_ports::documentation::DocumentationTarget;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

fn fixture() -> BTreeMap<String, Vec<u8>> {
    BTreeMap::from([
        ("globalcontext.json".into(), serde_json::to_vec(&json!({"root_doc":"overview", "master_doc":"overview", "version":"1.0", "release":"1.0.1"})).expect("valid test fixture")),
        ("searchindex.json".into(), br#"{"docnames":["overview","api/client"],"terms":{}}"#.to_vec()),
        ("overview.fjson".into(), serde_json::to_vec(&json!({"current_page_name":"overview","title":"Library","body":"<h1 id=\"library\">Library</h1><a href=\"api/client.html#client.send\">Client</a><img src=\"_images/logo.png\">","toc":"<a href=\"#library\">Library</a>","source_url":"https://example.test/source"})).expect("valid test fixture")),
        ("api/client.fjson".into(), serde_json::to_vec(&json!({"current_page_name":"api/client","title":"Client","body":"<dl><dt id=\"client.send\">send(value: str) -&gt; bool</dt><dd>Example: send('hello') <a href=\"../overview.html#library\">Library</a></dd></dl>","toc":""})).expect("valid test fixture")),
        ("search.fjson".into(), br#"{"current_page_name":"search"}"#.to_vec()),
        ("genindex.fjson".into(), br#"{"current_page_name":"genindex","genindexentries":[],"genindexcounts":[],"split_index":false}"#.to_vec()),
        ("genindex-all.fjson".into(), br#"{"current_page_name":"genindex-all","genindexentries":[],"genindexcounts":[],"split_index":true}"#.to_vec()),
        ("genindex-C.fjson".into(), br#"{"current_page_name":"genindex-C","key":"C","entries":[],"count":0,"genindexentries":[]}"#.to_vec()),
        ("py-modindex.fjson".into(), br#"{"current_page_name":"py-modindex","indextitle":"Python Module Index","content":[],"collapse_index":false}"#.to_vec()),
        ("_static/theme.css".into(), b"body { color: black; }".to_vec()),
        ("_images/logo.png".into(), b"retained image".to_vec()),
        ("_sources/overview.rst.txt".into(), b"Library\n=======\n".to_vec()),
        ("_downloads/environment.pickle".into(), b"a deliberately named download, not a runtime cache".to_vec()),
        ("objects.inv".into(), b"retained opaque inventory".to_vec()),
        ("environment.pickle".into(), b"NEVER UNPICKLE".to_vec()),
        ("last_build".into(), Vec::new()),
        (".doctrees/overview.doctree".into(), b"private runtime".to_vec()),
    ])
}

fn capture(files: &BTreeMap<String, Vec<u8>>) -> CapturedSource {
    let mut inventory = InventoryBuilder::new(Limits::default());
    let mut directories = BTreeSet::new();
    let mut contents = BTreeMap::new();
    for (name, bytes) in files {
        let mut parent = String::new();
        let pieces: Vec<_> = name.split('/').collect();
        for piece in &pieces[..pieces.len() - 1] {
            if !parent.is_empty() {
                parent.push('/');
            }
            parent.push_str(piece);
            if directories.insert(parent.clone()) {
                inventory
                    .insert(
                        parent.parse().expect("valid test fixture"),
                        Content::Directory,
                    )
                    .expect("valid test fixture");
            }
        }
        let path: EntryPath = name.parse().expect("valid test fixture");
        inventory
            .insert(
                path.clone(),
                Content::File {
                    bytes: bytes.len() as u64,
                    sha256: Sha256Digest::of(bytes),
                },
            )
            .expect("valid test fixture");
        contents.insert(path, bytes.clone());
    }
    CapturedSource::from_parts(inventory.finish(), contents).expect("valid test fixture")
}

fn assemble_catalog(
    tree: &CapturedSource,
    selection: &Selection,
    limits: Limits,
) -> Result<gazette_core::documentation::TargetAssembly, Error> {
    Mintlify::new(&gazette_adapter_markdown::Comrak).assemble_with_catalog(
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
            projects: &[ProjectModule {
                slug: "client",
                title: "Client",
                manual: None,
                navigation: vec![ProjectItem::Reference(Reference {
                    pages: None,
                    slug: "python",
                    title: "Python API",
                    artifact: Artifact {
                        format: Format::SphinxJson,
                        tree: tree.as_input(),
                        selection,
                    },
                })],
            }],
            usage: None,
            metadata: Default::default(),
        },
        limits,
        gazette_core::documentation::CatalogLimits::default(),
    )
}

fn assemble(
    tree: &CapturedSource,
    selection: &Selection,
    limits: Limits,
) -> Result<CapturedSource, Error> {
    assemble_catalog(tree, selection, limits).map(|assembled| {
        let catalog = assembled
            .catalog
            .as_ref()
            .expect("validated Sphinx catalog");
        for entry in catalog.entries() {
            assert_eq!(entry.module, "client");
            assert!(entry.anchors.is_some());
            let gazette_core::documentation::CatalogEvidence::QualifiedVirtual { artifacts } =
                &entry.evidence
            else {
                panic!("Sphinx context route")
            };
            assert!(
                artifacts
                    .iter()
                    .all(|path| assembled.output.file(path).is_some())
            );
            assert!(
                artifacts
                    .iter()
                    .any(|path| path.as_str().ends_with(".fjson"))
            );
        }
        assembled.output
    })
}

#[test]
fn discovery_excludes_unserved_sphinx_contexts_but_retains_their_original_artifacts() {
    let mut files = fixture();
    for document in [
        "genindex",
        "search",
        "py-modindex",
        "_modules/client",
        "api/_hidden",
    ] {
        files.insert(
            format!("{document}.fjson"),
            serde_json::to_vec(&json!({
                "current_page_name": document,
                "title": "Retained context",
                "body": "<p>Nonempty content does not make an excluded route available.</p>"
            }))
            .unwrap(),
        );
    }
    files.insert(
        "empty.fjson".into(),
        br#"{"current_page_name":"empty","title":"Empty","body":" \n "}"#.to_vec(),
    );
    let input = capture(&files);
    let assembled = assemble_catalog(
        &input,
        &Selection::Directory { path: None },
        Limits::default(),
    )
    .unwrap();
    let routes: Vec<_> = assembled
        .catalog
        .as_ref()
        .unwrap()
        .entries()
        .iter()
        .map(|entry| entry.route.as_str())
        .collect();
    assert_eq!(routes, ["client/python", "client/python/api/client"]);
    for document in [
        "genindex",
        "search",
        "py-modindex",
        "_modules/client",
        "api/_hidden",
        "empty",
    ] {
        let source = format!("{document}.fjson");
        let output = format!("artifacts/client/python/{source}").parse().unwrap();
        assert_eq!(
            assembled.output.file(&output),
            Some(files[&source].as_slice())
        );
    }
}

#[test]
fn configured_sphinx_root_must_be_a_page_the_provider_serves() {
    for (root, body) in [
        ("overview", " \n "),
        ("search", "<p>Utility</p>"),
        ("_hidden", "<p>Hidden</p>"),
    ] {
        let mut files = fixture();
        files.insert(
            "globalcontext.json".into(),
            serde_json::to_vec(&json!({"root_doc":root})).unwrap(),
        );
        files.insert(
            format!("{root}.fjson"),
            serde_json::to_vec(&json!({"current_page_name":root,"title":"Root","body":body}))
                .unwrap(),
        );
        assert!(
            assemble(
                &capture(&files),
                &Selection::Directory { path: None },
                Limits::default()
            )
            .is_err()
        );
    }
}

#[test]
fn native_directory_preserves_contexts_and_assets_without_publishing_runtime() {
    let files = fixture();
    let input = capture(&files);
    let selection = Selection::Directory { path: None };
    let output = assemble(&input, &selection, Limits::default()).expect("valid test fixture");
    let repeat = assemble(&input, &selection, Limits::default()).expect("valid test fixture");
    assert_eq!(output.inventory(), repeat.inventory());
    for (name, bytes) in &files {
        let actual = output.file(
            &format!("artifacts/client/python/{name}")
                .parse()
                .expect("valid test fixture"),
        );
        if name == "environment.pickle" || name == "last_build" || name.starts_with(".doctrees/") {
            assert!(actual.is_none(), "runtime cache {name}");
        } else {
            assert_eq!(actual, Some(bytes.as_slice()), "preserve {name}");
        }
        assert_eq!(
            input.file(&name.parse().expect("valid test fixture")),
            Some(bytes.as_slice()),
            "private capture unchanged"
        );
    }
    let docs: Value = serde_json::from_slice(
        output
            .file(&"docs.json".parse().expect("valid test fixture"))
            .expect("valid test fixture"),
    )
    .expect("valid test fixture");
    assert_eq!(
        docs["navigation"]["groups"][0]["pages"][0]["sdk"],
        json!({"format":"sphinx","source":"artifacts/client/python","directory":"client/python"})
    );
}

#[test]
fn authored_manual_links_close_against_public_sphinx_route_not_private_artifact_storage() {
    let files = fixture();
    let input = capture(&files);
    let manual = capture(&BTreeMap::from([(
        "start.md".into(),
        b"# Start\n\n[Python reference](/client/python)\n".to_vec(),
    )]));
    let manual_selection = Selection::File {
        path: "start.md".parse().expect("manual selection"),
    };
    let manual_route = "manual/start".parse().expect("manual route");
    let sphinx_selection = Selection::Directory { path: None };
    let output = Mintlify::new(&gazette_adapter_markdown::Comrak)
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
                projects: &[
                    ProjectModule {
                        slug: "manual",
                        title: "Manual",
                        manual: Some(ManualSource {
                            source: "source:manual",
                            tree: manual.as_input(),
                            assets: Vec::new(),
                        }),
                        navigation: vec![ProjectItem::Manual(ManualSection {
                            title: "Start",
                            pages: vec![ManualPage {
                                title: "Start",
                                route: &manual_route,
                                selection: &manual_selection,
                            }],
                        })],
                    },
                    ProjectModule {
                        slug: "client",
                        title: "Client",
                        manual: None,
                        navigation: vec![ProjectItem::Reference(Reference {
                            pages: None,
                            slug: "python",
                            title: "Python API",
                            artifact: Artifact {
                                format: Format::SphinxJson,
                                tree: input.as_input(),
                                selection: &sphinx_selection,
                            },
                        })],
                    },
                ],
                usage: None,
                metadata: Default::default(),
            },
            Limits::default(),
        )
        .expect("public Sphinx route closes the authored link");
    let manual = std::str::from_utf8(
        output
            .file(&"manual/start.md".parse().expect("manual output"))
            .expect("manual output bytes"),
    )
    .expect("UTF-8 manual");
    assert!(manual.contains("[Python reference](/client/python)"));
    assert!(
        output
            .file(
                &"artifacts/client/python/globalcontext.json"
                    .parse()
                    .expect("private artifact")
            )
            .is_some()
    );
}

#[test]
fn nested_selection_is_component_confined_and_matches_root_packaging() {
    let files = fixture();
    let mut nested: BTreeMap<_, _> = files
        .iter()
        .map(|(name, bytes)| (format!("docs/{name}"), bytes.clone()))
        .collect();
    nested.insert("docs-private/secret.txt".into(), b"do not include".to_vec());
    nested.insert("globalcontext.json".into(), b"do not use parent".to_vec());
    let root = assemble(
        &capture(&files),
        &Selection::Directory { path: None },
        Limits::default(),
    )
    .expect("valid test fixture");
    let selected = assemble(
        &capture(&nested),
        &Selection::Directory {
            path: Some("docs".parse().expect("valid test fixture")),
        },
        Limits::default(),
    )
    .expect("valid test fixture");
    assert_eq!(root.inventory(), selected.inventory());
}

#[test]
fn malformed_missing_and_unrecognized_contexts_fail_the_target() {
    for (name, bytes) in [
        (
            "globalcontext.json",
            br#"{"root_doc":"overview","master_doc":"different"}"#.as_slice(),
        ),
        (
            "overview.fjson",
            br#"{"current_page_name":"wrong","body":"","title":""}"#,
        ),
        (
            "overview.fjson",
            br#"{"current_page_name":"overview","body":[],"title":""}"#,
        ),
        (
            "overview.fjson",
            br#"{"current_page_name":"overview","body":"","body":"duplicate","title":""}"#,
        ),
        ("extension.fjson", br#"{"current_page_name":"extension"}"#),
        ("secret.txt", b"unclassified"),
        ("searchindex.json", b"[]"),
    ] {
        let mut files = fixture();
        files.insert(name.into(), bytes.to_vec());
        assert!(
            assemble(
                &capture(&files),
                &Selection::Directory { path: None },
                Limits::default()
            )
            .is_err(),
            "reject {name}"
        );
    }
    for missing in ["globalcontext.json", "searchindex.json", "overview.fjson"] {
        let mut files = fixture();
        files.remove(missing);
        assert!(
            assemble(
                &capture(&files),
                &Selection::Directory { path: None },
                Limits::default()
            )
            .is_err(),
            "missing {missing}"
        );
    }
}

#[test]
fn local_page_asset_and_fragment_failures_refuse_the_complete_target() {
    for body in [
        "<a href=\"missing.html\">missing</a>",
        "<img src=\"_images/missing.png\">",
        "<a href=\"api/client.html#missing\">missing fragment</a>",
        "<a href=\"../../escape.html\">escape</a>",
        "<a href=\"API/client.html\">case mismatch</a>",
        "<a href=\"api%2Fclient.html\">encoded separator</a>",
        "<a href=\"javascript:alert(1)\">active</a>",
        "<a href=\"api/client.html#\">empty target fragment</a>",
        "<script>alert(1)</script>",
        "<p onclick=\"alert(1)\">active attribute</p>",
        "<iframe src=\"https://example.test/\"></iframe>",
        "<p style=\"background:url(https://example.test/)\">styled</p>",
    ] {
        let mut files = fixture();
        files.insert(
            "overview.fjson".into(),
            serde_json::to_vec(&json!({
                "current_page_name":"overview",
                "title":"Library",
                "body":format!("<h1 id=\"library\">Library</h1>{body}"),
                "toc":""
            }))
            .expect("valid JSON fixture"),
        );
        assert!(
            assemble(
                &capture(&files),
                &Selection::Directory { path: None },
                Limits::default()
            )
            .is_err(),
            "broken local reference accepted: {body}"
        );
    }
}

#[test]
fn selected_file_and_missing_directory_are_not_reinterpreted() {
    let input = capture(&fixture());
    for selection in [
        Selection::File {
            path: "overview.fjson".parse().expect("valid test fixture"),
        },
        Selection::Directory {
            path: Some("missing".parse().expect("valid test fixture")),
        },
        Selection::Directory {
            path: Some("overview.fjson".parse().expect("valid test fixture")),
        },
    ] {
        assert!(assemble(&input, &selection, Limits::default()).is_err());
    }
}

#[test]
fn underscore_document_is_validated_not_mistaken_for_an_asset() {
    let mut files = fixture();
    files.insert(
        "_private.fjson".into(),
        br#"{"current_page_name":"different","body":"","title":""}"#.to_vec(),
    );
    assert!(
        assemble(
            &capture(&files),
            &Selection::Directory { path: None },
            Limits::default()
        )
        .is_err()
    );
}

#[test]
fn root_document_is_validated_even_inside_an_asset_namespace() {
    for bytes in [
        b"not JSON".as_slice(),
        br#"{"current_page_name":"wrong","body":"","title":""}"#,
        br#"{"current_page_name":"_static/root"}"#,
    ] {
        let mut files = fixture();
        files.insert(
            "globalcontext.json".into(),
            br#"{"root_doc":"_static/root","master_doc":"_static/root"}"#.to_vec(),
        );
        files.insert("_static/root.fjson".into(), bytes.to_vec());
        assert!(
            assemble(
                &capture(&files),
                &Selection::Directory { path: None },
                Limits::default()
            )
            .is_err()
        );
    }
}

#[test]
fn all_companions_share_the_target_byte_budget() {
    let input = capture(&fixture());
    let baseline = assemble(
        &input,
        &Selection::Directory { path: None },
        Limits::default(),
    )
    .expect("valid test fixture");
    let bytes: u64 = baseline
        .inventory()
        .entries()
        .iter()
        .map(|entry| match entry.content {
            Content::File { bytes, .. } => bytes,
            Content::Directory => 0,
        })
        .sum();
    let limits = Limits {
        total_bytes: bytes - 1,
        ..Limits::default()
    };
    assert!(assemble(&input, &Selection::Directory { path: None }, limits).is_err());
}

fn with_body(body: &str) -> BTreeMap<String, Vec<u8>> {
    let mut files = fixture();
    let mut page: Value = serde_json::from_slice(&files["api/client.fjson"]).expect("page");
    page["body"] = json!(format!("<h2 id=\"client.send\">Client</h2>{body}"));
    page["extra"] = json!({"version":"1.0", "opaque":[true, null, 42]});
    files.insert(
        "api/client.fjson".into(),
        serde_json::to_vec(&page).expect("JSON"),
    );
    files
}

fn provider_page(output: &CapturedSource) -> Value {
    serde_json::from_slice(
        output
            .file(
                &"artifacts/client/python/api/client.fjson"
                    .parse()
                    .expect("path"),
            )
            .expect("page"),
    )
    .expect("JSON")
}

#[test]
fn adjacent_type_emphasis_preserves_links_text_examples_and_original_capture() {
    let examples = "<pre>  first\n\t&lt;value&gt; &amp; λ\n<em>a</em><em>b</em></pre><code><em>x</em><em>y</em></code>";
    let body = format!(
        "<p><strong>policy</strong> (<a class=\"reference internal\" href=\"../overview.html#library\" title=\"Library\"><em>Library</em></a><em> | </em><em>None</em>)</p><p><em>Callable</em><em>[[</em><em>list</em><em>[</em><a href=\"#client.send\"><em>Client</em></a><em>]], </em><em>None</em><em>]</em></p><p><em>café &amp; λ</em><em>  |\n\tNone</em></p>{examples}"
    );
    let files = with_body(&body);
    let input = capture(&files);
    let output = assemble(
        &input,
        &Selection::Directory { path: None },
        Limits::default(),
    )
    .expect("normalized");
    let page = provider_page(&output);
    let actual = page["body"].as_str().expect("body");
    assert!(actual.contains("<em><a class=\"reference internal\" href=\"../overview.html#library\" title=\"Library\">Library</a> | None</em>"));
    assert!(
        actual.contains("<em>Callable[[list[<a href=\"#client.send\">Client</a>]], None]</em>")
    );
    assert!(actual.contains("<em>café &amp; λ  |\n\tNone</em>"));
    assert!(
        actual.contains(examples),
        "code examples keep their contents"
    );
    assert!(actual.contains("id=\"client.send\""));
    let mut original: Value = serde_json::from_slice(&files["api/client.fjson"]).expect("JSON");
    original["body"] = page["body"].clone();
    assert_eq!(page, original, "other JSON values are unchanged");
    for (name, bytes) in &files {
        assert_eq!(
            input.file(&name.parse().expect("path")),
            Some(bytes.as_slice()),
            "native input {name}"
        );
        if name != "api/client.fjson"
            && name != "environment.pickle"
            && name != "last_build"
            && !name.starts_with(".doctrees/")
        {
            assert_eq!(
                output.file(
                    &format!("artifacts/client/python/{name}")
                        .parse()
                        .expect("path")
                ),
                Some(bytes.as_slice()),
                "unchanged companion {name}"
            );
        }
    }
    let emitted: BTreeMap<_, _> = files
        .iter()
        .map(|(name, bytes)| {
            (
                name.clone(),
                output
                    .file(
                        &format!("artifacts/client/python/{name}")
                            .parse()
                            .expect("path"),
                    )
                    .unwrap_or(bytes)
                    .to_vec(),
            )
        })
        .collect();
    let repeated = assemble(
        &capture(&emitted),
        &Selection::Directory { path: None },
        Limits::default(),
    )
    .expect("repeat");
    assert_eq!(
        output.inventory(),
        repeated.inventory(),
        "normalization is idempotent"
    );
}

#[test]
fn emphasis_with_attributes_separators_or_unsupported_structure_stays_unchanged() {
    for body in [
        "<p><em>alone</em></p>",
        "<p><em>one</em> <em>two</em></p>",
        "<p><em>one</em><!--keep--><em>two</em></p>",
        "<p><em class=\"type\">one</em><em>two</em></p>",
        "<p><em id=\"label\">one</em><em>two</em></p>",
        "<pre><em>one</em><em>two</em></pre><code><em>one</em><em>two</em></code>",
        "<p><em>one</em><em>two</em>",
    ] {
        let files = with_body(body);
        let output = assemble(
            &capture(&files),
            &Selection::Directory { path: None },
            Limits::default(),
        )
        .expect("native pass-through");
        assert_eq!(
            output.file(
                &"artifacts/client/python/api/client.fjson"
                    .parse()
                    .expect("path")
            ),
            Some(files["api/client.fjson"].as_slice()),
            "preserve {body}"
        );
    }
}

#[test]
fn emphasis_in_titles_and_contents_keeps_named_links() {
    let mut files = with_body("<p>Text</p>");
    let mut page: Value = serde_json::from_slice(&files["api/client.fjson"]).expect("page");
    page["title"] = json!("<em>Client</em><em> API</em>");
    page["toc"] = json!("<a href=\"#client.send\"><em>Client</em></a><em> API</em>");
    files.insert(
        "api/client.fjson".into(),
        serde_json::to_vec(&page).expect("JSON"),
    );
    let output = assemble(
        &capture(&files),
        &Selection::Directory { path: None },
        Limits::default(),
    )
    .expect("normalized");
    let page = provider_page(&output);
    assert_eq!(page["title"], "<em>Client API</em>");
    assert_eq!(
        page["toc"],
        "<em><a href=\"#client.send\">Client</a> API</em>"
    );
}

#[test]
fn emphasis_conversion_keeps_link_and_active_content_checks() {
    for body in [
        "<a href=\"missing.html\"><em>Missing</em></a><em> | None</em>",
        "<a href=\"#missing\"><em>Missing</em></a><em> | None</em>",
        "<em onclick=\"run()\">Active</em><em> | None</em>",
        "<script>run()</script><em>A</em><em>B</em>",
    ] {
        let error = assemble(
            &capture(&with_body(body)),
            &Selection::Directory { path: None },
            Limits::default(),
        )
        .expect_err("invalid context");
        assert!(
            !error.to_string().contains("emphasis conversion"),
            "validate original links and content first"
        );
    }
}

#[test]
fn emphasis_conversion_is_bounded_by_depth_tokens_and_emitted_bytes() {
    for (body, reason) in [
        (
            format!(
                "{}<em>A</em><em>B</em>{}",
                "<span>".repeat(129),
                "</span>".repeat(129)
            ),
            "nesting limit",
        ),
        (
            format!("<em>A</em><em>B</em>{}", "<span>x</span>".repeat(34_000)),
            "token limit",
        ),
    ] {
        let error = assemble(
            &capture(&with_body(&body)),
            &Selection::Directory { path: None },
            Limits::default(),
        )
        .expect_err("bounded conversion");
        assert!(error.to_string().contains(reason), "{error}");
    }
    let mut files = with_body("<em>A</em><em>B</em>");
    let mut page: Value = serde_json::from_slice(&files["api/client.fjson"]).expect("page");
    page["extra"] = json!(vec![0; 1000]);
    files.insert(
        "api/client.fjson".into(),
        serde_json::to_vec(&page).expect("JSON"),
    );
    let input = capture(&files);
    let output = assemble(
        &input,
        &Selection::Directory { path: None },
        Limits::default(),
    )
    .expect("default bounds");
    let largest_input = files.values().map(Vec::len).max().expect("files") as u64;
    let error = assemble(
        &input,
        &Selection::Directory { path: None },
        Limits {
            file_bytes: largest_input,
            ..Limits::default()
        },
    )
    .expect_err("expanded JSON exceeds file budget");
    assert!(error.to_string().contains("bytes"), "{error}");
    let total = output
        .inventory()
        .entries()
        .iter()
        .map(|entry| match entry.content {
            Content::File { bytes, .. } => bytes,
            Content::Directory => 0,
        })
        .sum::<u64>();
    assert!(
        assemble(
            &input,
            &Selection::Directory { path: None },
            Limits {
                total_bytes: total - 1,
                ..Limits::default()
            }
        )
        .is_err()
    );
}

#[test]
fn raw_text_elements_preserve_escaped_markup_when_other_emphasis_is_adjacent() {
    for tag in [
        "xmp",
        "noembed",
        "noframes",
        "plaintext",
        "noscript",
        "title",
    ] {
        for text in [
            "&lt;A&gt; &amp; B".to_owned(),
            format!("&lt;/{tag}&gt;&lt;script&gt;bad()&lt;/script&gt;"),
        ] {
            let body = format!(
                "<em>Before</em><em> pair</em><{tag}>{text}</{tag}><em>After</em><em> pair</em>"
            );
            let files = with_body(&body);
            let output = assemble(
                &capture(&files),
                &Selection::Directory { path: None },
                Limits::default(),
            )
            .expect("existing checked fragment passes through");
            assert_eq!(
                output.file(
                    &"artifacts/client/python/api/client.fjson"
                        .parse()
                        .expect("path")
                ),
                Some(files["api/client.fjson"].as_slice()),
                "preserve escaped text inside {tag}"
            );
        }
    }
}

#[test]
fn obsolete_void_elements_cannot_drop_text_during_emphasis_conversion() {
    for tag in ["basefont", "bgsound", "keygen"] {
        let files = with_body(&format!(
            "<{tag}>keep this text</{tag}><em>A</em><em>B</em>"
        ));
        let output = assemble(
            &capture(&files),
            &Selection::Directory { path: None },
            Limits::default(),
        )
        .expect("unchanged native context");
        assert_eq!(
            output.file(
                &"artifacts/client/python/api/client.fjson"
                    .parse()
                    .expect("path")
            ),
            Some(files["api/client.fjson"].as_slice()),
            "preserve content around {tag}"
        );
    }
}

#[test]
fn mixed_http_discovery_admits_only_selected_specification_and_keeps_sdk_artifacts_ignored() {
    let api = br#"{"openapi":"3.0.3","info":{"title":"HTTP API","version":"1"},"paths":{"/items":{"get":{"operationId":"listItems","responses":{"200":{"description":"Items"}}}}}}"#;
    let mut files = fixture();
    files.insert("_static/unselected-openapi.json".into(), api.to_vec());
    let sdk = capture(&files);
    let http = capture(&BTreeMap::from([
        ("selected.json".into(), api.to_vec()),
        ("unselected.json".into(), api.to_vec()),
    ]));
    let sdk_selection = Selection::Directory { path: None };
    let http_selection = Selection::File {
        path: "selected.json".parse().expect("selection"),
    };
    let output = Mintlify::new(&gazette_adapter_markdown::Comrak)
        .assemble(
            Request {
                name: "Mixed reference",
                public_base_url: None,
                target: &Target::Mintlify {
                    theme: "mint".into(),
                    primary_color: "#123abc".into(),
                    light_color: None,
                    dark_color: None,
                    telemetry: None,
                },
                projects: &[ProjectModule {
                    slug: "client",
                    title: "Client",
                    manual: None,
                    navigation: vec![
                        ProjectItem::Reference(Reference {
                            pages: None,
                            slug: "python",
                            title: "Python API",
                            artifact: Artifact {
                                format: Format::SphinxJson,
                                tree: sdk.as_input(),
                                selection: &sdk_selection,
                            },
                        }),
                        ProjectItem::Reference(Reference {
                            pages: None,
                            slug: "http",
                            title: "HTTP API",
                            artifact: Artifact {
                                format: Format::OpenApiJson,
                                tree: http.as_input(),
                                selection: &http_selection,
                            },
                        }),
                    ],
                }],
                usage: None,
                metadata: Default::default(),
            },
            Limits::default(),
        )
        .expect("mixed retained target");
    let file = |path: &str| {
        output
            .file(&path.parse().expect("output path"))
            .expect("retained output")
    };
    assert_eq!(
        file(".mintignore"),
        b"artifacts/**\n!artifacts/client/\n!artifacts/client/http.json\n"
    );
    assert_eq!(file("artifacts/client/http.json"), api);
    assert_eq!(
        file("artifacts/client/python/overview.fjson"),
        files["overview.fjson"]
    );
    assert_eq!(
        file("artifacts/client/python/_static/unselected-openapi.json"),
        api
    );
    assert!(
        output
            .file(
                &"artifacts/client/unselected.json"
                    .parse()
                    .expect("unselected path")
            )
            .is_none()
    );
    let docs: Value = serde_json::from_slice(file("docs.json")).expect("configuration");
    let pages = &docs["navigation"]["groups"][0]["pages"];
    assert_eq!(pages[0]["sdk"]["source"], "artifacts/client/python");
    assert_eq!(pages[1]["openapi"]["source"], "artifacts/client/http.json");
    assert_eq!(
        http.file(&"selected.json".parse().expect("source path")),
        Some(api.as_slice())
    );
}
