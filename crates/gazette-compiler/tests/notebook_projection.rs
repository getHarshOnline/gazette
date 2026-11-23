//! A real bound article/notebook uses the selected Markdown transport, without execution.

use gazette_adapter_markdown::Comrak;
use gazette_compiler::{
    authored,
    corpus::{File, Input, LocatedMathResult, MathResult, ParsedCorpus},
    html::{self, TextKind},
    workspace,
};
use gazette_core::{
    markdown::MathBinding,
    publication::Key,
    raster,
    source::{EntryPath, Sha256Digest},
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

const MATHML: &str = r#"<math xmlns="http://www.w3.org/1998/Math/MathML"><semantics><mrow><mi>x</mi></mrow><annotation encoding="application/x-tex">x</annotation></semantics></math>"#;
const NOTEBOOK: &str = "authored/one/article/book.ipynb";
const CODE: &str = "first\n\n**literal code**, not emphasis\nprint('$not_math$')\n";
const STORED_CODE: &str = "alpha\n\n**literal stored**, not emphasis\n";
const STREAM: &str = "line one\n\nline two\n";

fn path(value: &str) -> EntryPath {
    value.parse().unwrap()
}
fn key(value: &str) -> Key {
    value.parse().unwrap()
}

fn compilation(enabled: bool) -> workspace::Compilation {
    workspace::compile(&format!(
        "[[principals]]\nid='owner:one'\nkind='person'\n\
         [[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n\
         [[sources]]\nid='source:one'\nlocation={{kind='repository',root='/private/notebook-source',commit='1111111111111111111111111111111111111111',subdirectory='content'}}\n\
         [[brands]]\nid='brand:one'\nname='Notebook fixture'\n\
         [[brands.publications]]\nsite='site:one'\npublic_base_url='https://example.test/mount/'\n\
         authored=[{{source='source:one',target='authored/one',markdown_profile='publication-math'}}]\n\
         machine={{markdown_alternates={{enabled={enabled},directory_page='index.md',file_page='append-md',media_type='text/markdown',discovery_rel='alternate'}}}}\n\
         [brands.publications.documentation_repository]\nname='notebook-docs'\npath='/private/notebook-docs'\npublic_base_url='https://docs.example.test/'\n"
    ), Default::default()).expect("explicit authored placement, math syntax and machine selection")
}

fn page(id: &str, route: &str, declarations: &str, body: &str) -> Vec<u8> {
    format!("+++\n{declarations}[content]\nid='{id}'\nsite='site:one'\ntitle='Notebook article'\nlifecycle='published'\nroute={{path='{route}'}}\nstructure={{kind='page',family={{kind='article'}}}}\n+++\n{body}").into_bytes()
}

fn stored_json() -> Value {
    json!({"answer":42,"blank":"first\n\nsecond"})
}

fn files() -> Vec<(EntryPath, Vec<u8>)> {
    let notebook = json!({"nbformat":4,"nbformat_minor":5,"metadata":{},"cells":[
        {"cell_type":"markdown","id":"intro","metadata":{},"source":"# Cell\n\nCell [[site:example.test/target/#destination|target]] and $x$.\n"},
        {"cell_type":"code","id":"code","metadata":{},"execution_count":3,"source":CODE,"outputs":[
            {"output_type":"stream","name":"stdout","text":STREAM},
            {"output_type":"display_data","metadata":{},"data":{"application/json":stored_json()}},
            {"output_type":"display_data","metadata":{},"data":{"text/markdown":format!("# Stored output\n\nStored [[site:example.test/target/#destination|target]] and $x$.\n\n```text\n{STORED_CODE}```\n")}}
        ]}
    ]});
    vec![
        (
            path("authored/one/article/index.md"),
            page(
                "content:article",
                "/article/",
                "notebooks=['article/book.ipynb']\n",
                "# Article\n\nOriginal contribution.\n",
            ),
        ),
        (path(NOTEBOOK), serde_json::to_vec(&notebook).unwrap()),
        (
            path("authored/one/target.md"),
            page(
                "content:target",
                "/target/",
                "",
                "# Destination\n\nRetained destination.\n",
            ),
        ),
    ]
}

fn parse<'a>(
    files: &'a [(EntryPath, Vec<u8>)],
    compilation: &workspace::Compilation,
) -> ParsedCorpus<'a> {
    let profiles = files
        .iter()
        .map(|(path, _)| (path.clone(), compilation.workspace.markdown_profile(path)))
        .collect();
    let corpus = ParsedCorpus::parse_with_profiles(
        files.iter().map(|(path, bytes)| Input {
            path: path.clone(),
            bytes,
        }),
        &profiles,
        Default::default(),
        &Comrak,
    )
    .unwrap();
    let results: BTreeMap<_, Vec<_>> = corpus
        .files()
        .iter()
        .filter_map(|(path, file)| {
            let results: Vec<_> = file
                .math_sources()
                .filter(|(_, _, facts)| facts.iter().any(|fact| fact.emitted))
                .map(|(location, identity, facts)| LocatedMathResult {
                    location,
                    result: MathResult {
                        identity,
                        bindings: facts
                            .iter()
                            .filter(|fact| fact.emitted)
                            .map(|fact| {
                                assert_eq!(fact.tex, "x");
                                MathBinding {
                                    occurrence: fact.clone(),
                                    mathml: MATHML.into(),
                                }
                            })
                            .collect(),
                    },
                })
                .collect();
            (!results.is_empty()).then(|| (path.clone(), results))
        })
        .collect();
    assert_eq!(
        results.len(),
        1,
        "only actual notebook formulas need retained results"
    );
    corpus.with_math(results).unwrap()
}

/// Every notebook fragment the page's Notes carry, in cell order.
fn notebook_html(page: &authored::PageProjection<'_>) -> String {
    use gazette_compiler::notebook::NotebookCell;
    let notes = page.data.notes.as_ref().expect("notebooks join the Notes");
    let mut html = String::new();
    for notebook in &notes.notebooks {
        for cell in &notebook.cells {
            match cell {
                NotebookCell::Markdown { html: fragment, .. } => html.push_str(fragment),
                NotebookCell::Code {
                    source_html,
                    outputs_html,
                    ..
                } => {
                    html.push_str(source_html);
                    html.push_str(outputs_html);
                }
            }
        }
    }
    html
}

fn decoded_runs(html: &str) -> Vec<String> {
    let mut text = Vec::new();
    html::inspect(html.as_bytes(), Default::default(), |kind, value| {
        if kind == TextKind::Text {
            text.push(value.to_owned());
        }
        Ok::<_, &'static str>(())
    })
    .expect("maintained native entity decoding of the actual emitted fragment");
    text
}

#[test]
fn notebook_inputs_use_declared_language_and_preserve_plain_stored_outputs() {
    let code = "# literal input\nprint('<script> & café')\n\nvalue = 42";
    for (metadata, language, highlighted) in [
        (
            json!({"language_info":{"name":"python"}}),
            Some("python"),
            true,
        ),
        (
            json!({"kernelspec":{"name":"python3","display_name":"Python","language":"python"}}),
            Some("python"),
            true,
        ),
        (
            json!({"language_info":{"name":"unknown-language"}}),
            Some("unknown-language"),
            false,
        ),
        (json!({}), None, false),
    ] {
        let mut files = files();
        let mut notebook: Value = serde_json::from_slice(&files[1].1).unwrap();
        notebook["metadata"] = metadata;
        notebook["cells"][1]["source"] = code.into();
        files[1].1 = serde_json::to_vec(&notebook).unwrap();
        let compilation = compilation(true);
        let corpus = parse(&files, &compilation);
        let resolved = authored::resolve(&compilation, &corpus, Default::default()).unwrap();
        let page = resolved
            .project_page(&key("content:article"), &raster::unavailable, &Comrak)
            .unwrap();
        let fragments = notebook_html(&page);
        assert!(!page.body_html.contains("notebook"));
        let input = fragments
            .split_once("<pre class=\"notebook-source\">")
            .unwrap()
            .1
            .split_once("</pre>")
            .unwrap()
            .0;
        assert_eq!(input.contains("<span class=\"syntax-"), highlighted);
        if let Some(language) = language {
            assert!(input.contains(&format!("class=\"language-{language}\"")));
        }
        assert_eq!(decoded_runs(input).concat(), code);
        assert!(!input.contains("<script>"));
        let stored = fragments
            .split_once("<pre class=\"notebook-output\">")
            .unwrap()
            .1
            .split_once("</pre>")
            .unwrap()
            .0;
        assert!(!stored.contains("syntax-"));
        assert_eq!(decoded_runs(stored).concat(), STREAM);
        assert_eq!(
            page.notebooks[0].source_sha256,
            Sha256Digest::of(&files[1].1)
        );
        if language == Some("python") {
            assert!(page.body_markdown.as_ref().unwrap().contains("```python\n"));
        }
    }
}

#[test]
fn notebook_hyperlinks_follow_the_same_preview_and_public_rules_as_pages() {
    use gazette_core::publication::routing::PreviewLinkBases;
    let compilation = compilation(true);
    for (local, mapped, loopback) in [
        (false, false, false),
        (true, true, false),
        (true, false, false),
        (false, false, true),
    ] {
        let mut files = files();
        let mut notebook: Value = serde_json::from_slice(&files[1].1).unwrap();
        notebook["cells"][0]["source"] = if loopback {
            "# Cell\n\n[Local](http://localhost:4500/) and $x$.\n".into()
        } else {
            "# Cell\n\n[Page](https://example.test/mount/target/?view=one#destination) [Docs](https://docs.example.test/guide#step) [External](https://unrelated.test/path) and $x$.\n\n`https://docs.example.test/guide#step`\n".into()
        };
        files[1].1 = serde_json::to_vec(&notebook).unwrap();
        let corpus = parse(&files, &compilation);
        let mut resolved = authored::resolve(&compilation, &corpus, Default::default()).unwrap();
        if local {
            resolved = resolved
                .with_preview_urls(BTreeMap::from([(
                    key("site:one"),
                    "http://127.0.0.1:4500/mount/".parse().unwrap(),
                )]))
                .unwrap();
        }
        if mapped {
            resolved = resolved.with_preview_link_bases(
                PreviewLinkBases::try_from(BTreeMap::from([(
                    "https://docs.example.test/".into(),
                    "http://127.0.0.1:4501/".parse().unwrap(),
                )]))
                .unwrap(),
            );
        }
        let result = resolved.project_page(&key("content:article"), &raster::unavailable, &Comrak);
        if loopback || (local && !mapped) {
            assert!(
                result.is_err(),
                "notebook links must enforce the shared mode boundary"
            );
            continue;
        }
        let page = result.unwrap();
        let expected = if local {
            [
                "http://127.0.0.1:4500/mount/target/?view=one#destination",
                "http://127.0.0.1:4501/guide#step",
            ]
        } else {
            [
                "https://example.test/mount/target/?view=one#destination",
                "https://docs.example.test/guide#step",
            ]
        };
        let fragments = notebook_html(&page);
        for destination in expected.into_iter().chain(["https://unrelated.test/path"]) {
            assert!(
                fragments.contains(&format!("href=\"{destination}\"")),
                "{destination}"
            );
        }
        assert!(fragments.contains("<code>https://docs.example.test/guide#step</code>"));
        assert_eq!(
            page.notebooks[0].source_sha256,
            Sha256Digest::of(&files[1].1)
        );
    }
}

#[test]
fn selected_notebook_alternate_preserves_bound_cells_json_math_links_and_private_identity() {
    let files = files();
    for enabled in [true, false] {
        let compilation = compilation(enabled);
        let corpus = parse(&files, &compilation);
        let resolved = authored::resolve(&compilation, &corpus, Default::default()).unwrap();
        resolved.validate_source_bindings().unwrap();
        assert_eq!(
            resolved.content().graph().nodes().len(),
            2,
            "the attachment is not another content object"
        );
        let id = key("content:article");
        let bound = resolved.notebooks(&id).unwrap();
        assert_eq!(bound.len(), 1);
        assert_eq!(bound[0].path, path(NOTEBOOK));
        assert_eq!(bound[0].notebook.source(), files[1].1.as_slice());
        assert!(matches!(corpus.files()[&path(NOTEBOOK)], File::Notebook(_)));
        let projected = resolved
            .project_page(&id, &raster::unavailable, &Comrak)
            .unwrap();
        assert_eq!(projected.notebooks.len(), 1);
        assert_eq!(
            projected.notebooks[0].source_sha256,
            Sha256Digest::of(&files[1].1)
        );
        assert_eq!(projected.notebooks[0].path, path(NOTEBOOK));
        assert!(!projected.body_html.contains("gazette-notebook"));
        let notes = projected.data.notes.as_ref().expect("notebook-only Notes");
        assert_eq!(notes.notebooks.len(), 1);
        assert!(notes.body_html.is_empty() && notes.source().is_none());
        let fragments = notebook_html(&projected);
        assert_eq!(fragments.matches("<math ").count(), 2);
        assert!(projected.data.links.is_empty());
        assert_eq!(notes.links.len(), 2);
        assert_ne!(notes.links[0].address, notes.links[1].address);
        for link in &notes.links {
            assert_eq!(link.content.as_ref(), Some(&key("content:target")));
            assert_eq!(link.site.as_ref(), Some(&key("site:one")));
            assert_eq!(
                link.kind,
                gazette_core::markdown::LinkDestinationKind::InternalNative
            );
            assert_eq!(
                link.destination,
                "https://example.test/mount/target/#destination"
            );
        }
        let expected_json = serde_json::to_string(&stored_json()).unwrap();
        let native_text = decoded_runs(&fragments);
        for exact in [CODE, STREAM, STORED_CODE, expected_json.as_str()] {
            assert!(
                native_text.iter().any(|value| value == exact),
                "exact native cell run: {exact:?}: {native_text:?}"
            );
        }
        if enabled {
            let alternate = projected
                .body_markdown
                .as_deref()
                .expect("explicitly selected alternate");
            use gazette_ports::markdown::Markdown;
            let reading = Comrak
                .parse(
                    alternate,
                    gazette_core::markdown::Profile::PublicationMath,
                    Default::default(),
                )
                .unwrap();
            assert!(!alternate.contains("<section class=\"gazette-notebook\""));
            assert_eq!(
                reading
                    .syntax()
                    .links
                    .iter()
                    .map(|link| link.destination.as_str())
                    .collect::<Vec<_>>(),
                vec!["https://example.test/mount/target/#destination"; 2]
            );
            assert_eq!(
                reading
                    .syntax()
                    .math
                    .iter()
                    .map(|math| math.tex.as_str())
                    .collect::<Vec<_>>(),
                vec!["x"; 2]
            );
            for exact in [CODE, STREAM, STORED_CODE] {
                assert!(
                    reading.syntax().text.iter().any(|part| part.value == exact),
                    "exact reading code/output: {exact:?}"
                );
            }
            assert!(
                reading
                    .syntax()
                    .text
                    .iter()
                    .any(|part| part.value == format!("{expected_json}\n"))
            );
            assert!(alternate.contains("Execution count: 3"));
            assert!(!alternate.contains("[target](../target.md"));
            assert!(!alternate.contains("[[site:example.test/target/"));
        } else {
            assert!(projected.body_markdown.is_none());
        }
        let model = serde_json::to_value(&projected).unwrap();
        assert_eq!(model["content"]["id"], "content:article");
        assert!(model.get("body_markdown").is_none());
        assert!(model.get("notebooks").is_none());
        let serialized = model.to_string();
        for private in [
            NOTEBOOK,
            "article/book.ipynb",
            "/private/notebook-source",
            "source:one",
        ] {
            assert!(
                !serialized.contains(private),
                "private capture field {private}"
            );
        }
    }
}

#[test]
fn authored_notebook_local_markdown_reference_retains_exact_refusal_and_cell_coordinates() {
    let mut files = files();
    let source = "# Cell\n\nCell [target](../target.md#destination) and $x$.\n";
    let mut notebook: Value = serde_json::from_slice(&files[1].1).unwrap();
    notebook["cells"][0]["source"] = source.into();
    files[1].1 = serde_json::to_vec(&notebook).unwrap();
    let compilation = compilation(true);
    let corpus = parse(&files, &compilation);
    let resolved = authored::resolve(&compilation, &corpus, Default::default()).unwrap();
    let error = resolved
        .project_page(&key("content:article"), &raster::unavailable, &Comrak)
        .unwrap_err();
    assert_eq!(error.code(), "markdown_link_resolution");
    let authored::ProjectionError::Notebook { path: input, error } = error else {
        panic!("exact notebook failure");
    };
    assert_eq!(input, path(NOTEBOOK));
    let gazette_compiler::notebook::RenderError::Markdown {
        cell: Some(0),
        error,
    } = error
    else {
        panic!("cell-local retained Markdown failure");
    };
    let gazette_compiler::markdown::Error::LinkResolution { span, reason } = error else {
        panic!("original reference refusal");
    };
    assert_eq!(
        reason,
        "local notebook references require an explicit retained asset mapping"
    );
    assert_eq!(
        &source[span.start..span.end],
        "[target](../target.md#destination)"
    );
    assert_eq!(
        resolved.notebooks(&key("content:article")).unwrap()[0]
            .notebook
            .source(),
        files[1].1.as_slice()
    );
}

fn with_declaration(declaration: &str) -> Vec<(EntryPath, Vec<u8>)> {
    let mut files = files();
    files[0].1 = page(
        "content:article",
        "/article/",
        declaration,
        "# Article\n\nOriginal contribution.\n\n## Essay section\n\nMore essay.\n",
    );
    files
}

#[test]
fn notebooks_join_notes_as_cells_and_leave_the_essay_body_outline_and_text_alone() {
    let files = with_declaration("notebooks=['article/book.ipynb']\n");
    let compilation = compilation(true);
    let corpus = parse(&files, &compilation);
    let resolved = authored::resolve(&compilation, &corpus, Default::default()).unwrap();
    let projected = resolved
        .project_page(&key("content:article"), &raster::unavailable, &Comrak)
        .unwrap();
    assert!(!projected.body_html.contains("notebook"));
    assert!(!projected.body_text.contains("literal code"));
    assert_eq!(
        projected
            .data
            .headings
            .iter()
            .map(|heading| heading.text.as_str())
            .collect::<Vec<_>>(),
        ["Article", "Essay section"]
    );
    let notes = projected
        .data
        .notes
        .as_ref()
        .expect("Notes from notebooks alone");
    assert!(notes.body_text.contains("literal code"));
    assert!(notes.body_text.contains("line one"));
    assert!(notes.headings.is_empty());

    let model = serde_json::to_value(&projected).unwrap();
    let namespace = format!("gazette-notebook-{}", Sha256Digest::of(NOTEBOOK.as_bytes()));
    let notebook = &model["notes"]["notebooks"][0];
    assert_eq!(notebook["id"], namespace.as_str());
    assert_eq!(notebook["download"], Value::Null);
    let cells = notebook["cells"].as_array().unwrap();
    assert_eq!(cells.len(), 2);
    assert_eq!(cells[0]["kind"], "markdown");
    assert_eq!(cells[0]["cell_id"], "intro");
    let markdown = cells[0]["html"].as_str().unwrap();
    // The notebook's H1 sits one level down, under the Notes heading; its ID is unchanged.
    assert!(markdown.starts_with(&format!("<h2 id=\"{namespace}--cell-0-")));
    assert!(!markdown.contains("<h1"));
    assert_eq!(cells[1]["kind"], "code");
    assert_eq!(cells[1]["cell_id"], "code");
    assert_eq!(cells[1]["execution_count"], 3);
    assert_eq!(cells[1]["language"], Value::Null);
    let source = cells[1]["source_html"].as_str().unwrap();
    assert!(source.starts_with("<pre class=\"notebook-source\"><code>"));
    assert!(source.ends_with("</code></pre>"));
    assert_eq!(decoded_runs(source).concat(), CODE);
    let outputs = cells[1]["outputs_html"].as_str().unwrap();
    assert!(outputs.starts_with("<div class=\"notebook-stream\">"));
    assert!(!outputs.contains("notebook-source"));
    assert!(
        model["body_html"]
            .as_str()
            .unwrap()
            .find("notebook")
            .is_none()
    );
}

#[test]
fn notebooks_follow_notes_md_in_the_same_notes() {
    let mut files = with_declaration("notebooks=['article/book.ipynb']\n");
    files.push((
        path("authored/one/article/notes.md"),
        b"## Commentary\n\nAuthored commentary.\n".to_vec(),
    ));
    let compilation = compilation(true);
    let corpus = parse(&files, &compilation);
    let resolved = authored::resolve(&compilation, &corpus, Default::default()).unwrap();
    let projected = resolved
        .project_page(&key("content:article"), &raster::unavailable, &Comrak)
        .unwrap();
    let notes = projected.data.notes.as_ref().expect("article Notes");
    assert!(notes.source().is_some());
    assert!(notes.body_html.contains("Authored commentary."));
    assert!(!notes.body_html.contains("notebook"));
    assert_eq!(notes.notebooks.len(), 1);
    let commentary = notes.body_text.find("Authored commentary.").unwrap();
    assert!(commentary < notes.body_text.find("literal code").unwrap());
    let alternate = projected.body_markdown.as_deref().unwrap();
    let heading = alternate.find("## Article notes").unwrap();
    assert!(heading < alternate.find("Authored commentary.").unwrap());
    assert!(
        alternate.find("Authored commentary.").unwrap()
            < alternate.find("Execution count: 3").unwrap()
    );
}

#[test]
fn a_selected_download_emits_exact_bytes_beside_its_page() {
    let files = with_declaration("notebooks=[{path='article/book.ipynb',download=true}]\n");
    let compilation = compilation(true);
    let corpus = parse(&files, &compilation);
    let resolved = authored::resolve(&compilation, &corpus, Default::default()).unwrap();
    let plan = resolved.assets_for(&key("site:one")).unwrap();
    let asset = plan.get(&path(NOTEBOOK)).expect("one site asset record");
    assert_eq!(asset.content(), files[1].1.as_slice());
    assert_eq!(asset.url, "https://example.test/mount/article/book.ipynb");
    assert_eq!(asset.output.as_str(), "article/book.ipynb");
    assert!(!plan.uncompiled().contains(&path(NOTEBOOK)));
    let projected = resolved
        .project_page(&key("content:article"), &raster::unavailable, &Comrak)
        .unwrap();
    let model = serde_json::to_value(&projected).unwrap();
    assert_eq!(
        model["notes"]["notebooks"][0]["download"],
        json!({
            "url": "https://example.test/mount/article/book.ipynb",
            "bytes": files[1].1.len(),
            "media_type": "application/x-ipynb+json"
        })
    );
    assert!(
        projected
            .data
            .assets
            .iter()
            .any(|asset| asset.url.ends_with("/article/book.ipynb"))
    );
    assert!(projected.notebooks[0].download_refusal.is_none());
}

#[test]
fn the_string_form_and_an_unselected_table_emit_no_download() {
    for declaration in [
        "notebooks=['article/book.ipynb']\n",
        "notebooks=[{path='article/book.ipynb'}]\n",
        "notebooks=[{path='article/book.ipynb',download=false}]\n",
    ] {
        let files = with_declaration(declaration);
        let compilation = compilation(false);
        let corpus = parse(&files, &compilation);
        let resolved = authored::resolve(&compilation, &corpus, Default::default()).unwrap();
        let plan = resolved.assets_for(&key("site:one")).unwrap();
        assert!(plan.get(&path(NOTEBOOK)).is_none(), "{declaration}");
        assert!(plan.uncompiled().contains(&path(NOTEBOOK)));
        let projected = resolved
            .project_page(&key("content:article"), &raster::unavailable, &Comrak)
            .unwrap();
        let notes = projected.data.notes.as_ref().unwrap();
        assert!(notes.notebooks[0].download.is_none(), "{declaration}");
    }
}

#[test]
fn notebook_declarations_stay_strict() {
    for declaration in [
        "notebooks=[{path='article/book.ipynb',download=true,public=true}]\n",
        "notebooks=[{download=true}]\n",
        "notebooks=[{path='article/book.ipynb',download='yes'}]\n",
        "notebooks=[3]\n",
    ] {
        let files = with_declaration(declaration);
        let compilation = compilation(false);
        let profiles = files
            .iter()
            .map(|(path, _)| (path.clone(), compilation.workspace.markdown_profile(path)))
            .collect();
        let rejected = match ParsedCorpus::parse_with_profiles(
            files.iter().map(|(path, bytes)| Input {
                path: path.clone(),
                bytes,
            }),
            &profiles,
            Default::default(),
            &Comrak,
        ) {
            Err(_) => true,
            Ok(corpus) => authored::resolve(&compilation, &corpus, Default::default()).is_err(),
        };
        assert!(rejected, "{declaration}");
    }
}

#[test]
fn a_failing_scan_withholds_only_the_download() {
    let mut files = with_declaration("notebooks=[{path='article/book.ipynb',download=true}]\n");
    let mut notebook: Value = serde_json::from_slice(&files[1].1).unwrap();
    notebook["cells"][1]["outputs"][0]["text"] =
        "Traceback in /home/privateperson/analysis.py\n".into();
    files[1].1 = serde_json::to_vec(&notebook).unwrap();
    let compilation = compilation(false);
    let corpus = parse(&files, &compilation);
    let resolved = authored::resolve(&compilation, &corpus, Default::default()).unwrap();
    let plan = resolved.assets_for(&key("site:one")).unwrap();
    assert!(plan.get(&path(NOTEBOOK)).is_none());
    let projected = resolved
        .project_page(&key("content:article"), &raster::unavailable, &Comrak)
        .unwrap();
    let notes = projected.data.notes.as_ref().unwrap();
    assert!(notes.notebooks[0].download.is_none());
    assert!(
        notes.body_text.contains("privateperson"),
        "the section still renders"
    );
    let refusal = projected.notebooks[0]
        .download_refusal
        .as_ref()
        .expect("evidence names the refusal");
    assert_eq!(
        refusal.class,
        gazette_compiler::notebook::ScanClass::LocalPath
    );
    assert_eq!(refusal.cell, Some(1));
    assert_eq!(refusal.field, "cells[1].outputs[0].text");
    let evidence = serde_json::to_string(&projected.notebooks).unwrap();
    assert!(evidence.contains("notebook_download") || evidence.contains("local_path"));
    assert!(!evidence.contains("privateperson"));
}
