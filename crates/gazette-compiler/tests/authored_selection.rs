//! One retained candidate owner, not a publisher-policy or release attestation.

use gazette_compiler::{
    authored::{self, ResolvedAuthored, Selection, SelectionError},
    corpus::{File, Input, ParsedCorpus},
    workspace,
};
use gazette_core::{
    publication::{
        ContentLifecycle, Key,
        content::{self, Disposition},
    },
    source::EntryPath,
};

#[path = "authored_selection/mirrors.rs"]
mod mirrors;

fn key(value: &str) -> Key {
    value.parse().expect("exact content identity")
}

fn path(value: &str) -> EntryPath {
    value.parse().expect("literal corpus path")
}

fn compilation() -> workspace::Compilation {
    workspace::compile(
        "[[principals]]\nid='owner:one'\nkind='person'\n\
         [[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n\
         [[sites]]\nid='site:two'\npublisher='owner:one'\ndeployable=false\n\
         [[sources]]\nid='source:one'\nlocation={kind='repository',root='/private/retained',commit='1111111111111111111111111111111111111111',subdirectory='docs'}\n\
         [[brands]]\nid='brand:one'\nname='Example'\n\
         [[brands.publications]]\nsite='site:one'\npublic_base_url='https://example.test/mount/'\n\
         machine={markdown_alternates={enabled=true,directory_page='index.md',file_page='append-md',media_type='text/markdown',discovery_rel='alternate'},llms={enabled=true,description_from='content:keep',representation='markdown-alternate',include_human_link=true,sections=[{title='Selected reading',contents=['content:keep','collection:root']}]}}\n\
         authored=[{source='source:one',target='authored/one',static_assets={route_prefix='/media/'}}]\n\
         [[brands.publications]]\nsite='site:two'\npublic_base_url='https://other.test/'\n\
         authored=[{source='source:one',target='authored/two',static_assets={route_prefix='/media/'}}]\n\
         [[projects]]\nid='project:library'\nlifecycle='active'\nvisibility='private'\navailability='unavailable'\n\
         [[placements]]\nrole='primary'\nsite='site:one'\nproject='project:library'\nrender=true\ndiscoverable=false\n\
         documents={source='source:one',target='native/library',static_assets={route_prefix='/project-media/'}}\n\
         native_markdown={route_prefix='/docs/',lifecycle='published'}\n",
        Default::default(),
    ).expect("configured publications and native documentation source")
}

fn document(
    id: &str,
    state: &str,
    route: &str,
    edges: &str,
    structure: &str,
    body: &str,
) -> String {
    format!(
        "+++\n{edges}[content]\nid='{id}'\nsite='site:one'\ntitle='{id}'\ndescription='Declared selected reading.'\nlifecycle='{state}'\nroute={{path='{route}'}}\nstructure={{{structure}}}\n+++\n{body}"
    )
}

fn page(id: &str, state: &str, route: &str, body: &str) -> String {
    document(
        id,
        state,
        route,
        "",
        "kind='page',family={kind='article'}",
        body,
    )
}

fn parse(files: &[(String, String)]) -> ParsedCorpus<'_> {
    ParsedCorpus::parse(
        files.iter().map(|(name, source)| Input {
            path: path(name),
            bytes: source.as_bytes(),
        }),
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("retained document, notebook and opaque asset syntax")
}

fn all(resolved: &ResolvedAuthored<'_, '_>) -> Selection {
    Selection {
        content: resolved
            .content()
            .graph()
            .nodes()
            .map(|node| (node.id.clone(), Disposition::Include))
            .collect(),
        assets: resolved
            .corpus()
            .files()
            .iter()
            .filter(|(_, file)| matches!(file, File::Asset(_) | File::Notebook(_)))
            .map(|(path, _)| (path.clone(), Disposition::Include))
            .collect(),
    }
}

fn resolve<'a, 's>(
    compilation: &'a workspace::Compilation,
    corpus: &'a ParsedCorpus<'s>,
) -> ResolvedAuthored<'a, 's> {
    authored::resolve(compilation, corpus, content::Limits::default())
        .expect("whole private structural graph")
}

#[test]
fn owner_native_documentation_default_is_closed_and_accepts_exact_blocks() {
    let files = vec![
        (
            "native/library/plain.md".into(),
            "# Plain\n\n![image](image.png)\n".into(),
        ),
        (
            "native/library/archived.md".into(),
            document(
                "content:archived-doc",
                "archived",
                "/archived-doc/",
                "notebooks=['notes.ipynb']\n[[resources]]\ninputs=['main.ts']\nbuild={kind='browser-module',entry='main.ts'}\noutput='/archived.js'\n",
                "kind='page',family={kind='documentation'}",
                "Archived documentation\n",
            ),
        ),
        (
            "native/library/draft.md".into(),
            document(
                "content:draft-doc",
                "draft",
                "/draft-doc/",
                "",
                "kind='page',family={kind='documentation'}",
                "Draft documentation\n",
            ),
        ),
        (
            "authored/one/article.md".into(),
            page(
                "content:article",
                "published",
                "/article/",
                "Publication article\n",
            ),
        ),
        ("native/library/image.png".into(), "retained image".into()),
        (
            "native/library/notes.ipynb".into(),
            r#"{"nbformat":4,"nbformat_minor":5,"metadata":{},"cells":[]}"#.into(),
        ),
        (
            "native/library/main.ts".into(),
            "export const value = 1;".into(),
        ),
        ("native/library/unused.bin".into(), "unrelated".into()),
    ];
    let corpus = parse(&files);
    let compilation = compilation();
    let private = resolve(&compilation, &corpus);
    let mut selection = private
        .default_native_documentation_selection()
        .expect("complete owner-native documentation default");
    let repeated = private
        .default_native_documentation_selection()
        .expect("same retained inputs produce the same complete default");
    assert_eq!(
        serde_json::to_vec(&selection).expect("selection JSON"),
        serde_json::to_vec(&repeated).expect("repeated selection JSON")
    );
    let plain = private
        .content()
        .graph()
        .nodes()
        .find(|node| node.title == "Plain")
        .expect("lowered native documentation")
        .id
        .clone();

    assert_eq!(selection.content[&plain], Disposition::Include);
    assert_eq!(
        selection.content[&key("content:archived-doc")],
        Disposition::Include
    );
    assert_eq!(
        selection.content[&key("content:draft-doc")],
        Disposition::Exclude
    );
    assert_eq!(
        selection.content[&key("content:article")],
        Disposition::Exclude
    );
    for input in ["image.png", "notes.ipynb", "main.ts"] {
        assert_eq!(
            selection.assets[&path(&format!("native/library/{input}"))],
            Disposition::Include
        );
    }
    assert_eq!(
        selection.assets[&path("native/library/unused.bin")],
        Disposition::Exclude
    );

    let mut blocked_asset = selection.clone();
    blocked_asset
        .assets
        .insert(path("native/library/image.png"), Disposition::Exclude);
    let error = resolve(&compilation, &corpus)
        .into_selected(&blocked_asset)
        .expect_err("a required static reference cannot be blocked silently");
    assert!(matches!(error, SelectionError::Asset {
        path: actual,
        reason: "selected body requires an included asset",
    } if actual == path("native/library/image.png")));

    selection
        .content
        .insert(key("content:archived-doc"), Disposition::Exclude);
    let selected = private
        .into_selected(&selection)
        .expect("exact documentation block preserves the remaining closed candidate");
    let retained: Vec<_> = selected
        .content()
        .graph()
        .nodes()
        .map(|node| node.id.clone())
        .collect();
    assert_eq!(retained, [plain]);
}

#[test]
fn one_candidate_removes_excluded_associations_from_every_shared_projection() {
    let files = vec![
        (
            "authored/one/root.md".into(),
            document(
                "collection:root",
                "published",
                "/root/",
                "",
                "kind='collection'",
                "# Root\n",
            ),
        ),
        (
            "authored/one/keep.md".into(),
            document(
                "content:keep",
                "archived",
                "/keep/",
                "collections=['collection:root']\n",
                "kind='page',family={kind='article'}",
                "# Kept\n\n**Original body**.\n",
            ),
        ),
        (
            "authored/one/drop.md".into(),
            document(
                "content:drop",
                "draft",
                "/drop/",
                "collections=['collection:root']\nrelated=['content:keep']\n",
                "kind='page',family={kind='article'}",
                "PRIVATE DRAFT\n",
            ),
        ),
        (
            "authored/two/foreign.md".into(),
            page("content:foreign", "published", "/foreign/", "Foreign\n")
                .replace("site='site:one'", "site='site:two'"),
        ),
    ];
    let corpus = parse(&files);
    let compilation = compilation();
    let private = resolve(&compilation, &corpus);
    assert!(private.document(&key("content:drop")).is_some());
    assert_eq!(
        private
            .project_site_pages(&key("site:one"))
            .expect("private discovery")
            .pages()
            .len(),
        3
    );
    let original = private
        .project_page(
            &key("content:keep"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("private page");
    let original_html = original.body_html.clone();
    let original_text = original.body_text.clone();
    let mut selection = all(&private);
    selection
        .content
        .insert(key("content:drop"), Disposition::Exclude);
    selection
        .content
        .insert(key("content:foreign"), Disposition::Exclude);
    let selected = private
        .into_selected(&selection)
        .expect("closed non-draft candidate");
    assert!(
        std::ptr::eq(selected.corpus(), &corpus),
        "syntax is retained, not reparsed or erased"
    );
    for id in [key("content:drop"), key("content:foreign")] {
        assert!(selected.content().graph().node(&id).is_none());
        assert!(selected.content().route(&id).is_none());
        assert!(selected.source_path(&id).is_none());
        assert!(selected.file(&id).is_none());
        assert!(selected.document(&id).is_none());
        assert!(selected.origin(&id, &[]).is_none());
        assert!(selected.derivation(&id).is_none());
        assert!(selected.resources(&id).is_empty());
        assert!(
            selected
                .project_page(
                    &id,
                    &gazette_core::raster::unavailable,
                    &gazette_adapter_markdown::Comrak
                )
                .is_err()
        );
    }
    let root = selected
        .project_page(
            &key("collection:root"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("selected container");
    assert_eq!(
        root.data
            .children
            .iter()
            .map(|view| view.node.id.clone())
            .collect::<Vec<_>>(),
        [key("content:keep")]
    );
    let page = selected
        .project_page(
            &key("content:keep"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("same selected owner");
    assert_eq!(page.data.content.lifecycle, ContentLifecycle::Archived);
    assert_eq!(page.body_html, original_html);
    assert_eq!(page.body_text, original_text);
    assert!(std::ptr::eq(
        page.data.content.node,
        selected
            .content()
            .graph()
            .node(&key("content:keep"))
            .expect("selected node")
    ));
    let model = serde_json::to_value(&page).expect("page model");
    let search = serde_json::to_value(page.search()).expect("shared search model");
    assert_eq!(search["content"], model["content"]);
    assert_eq!(search["collections"], model["collections"]);
    assert_eq!(search["body"], page.body_text);
    let graph = selected
        .project_site_graph(&key("site:one"), Default::default())
        .expect("selected graph projection");
    assert_eq!(graph.nodes.len(), 2);
    assert_eq!(graph.memberships.len(), 1);
    assert!(graph.references.is_empty());
    assert!(graph.related.is_empty());
    let pages = selected
        .project_site_pages(&key("site:one"))
        .expect("selected discovery owner");
    assert_eq!(
        pages
            .pages()
            .map(|page| page.node.id.clone())
            .collect::<Vec<_>>(),
        [key("content:keep"), key("collection:root")]
    );
    for output in [
        pages.sitemap(65_536).expect("sitemap"),
        pages
            .llms(&selected.discovery().unwrap(), 65_536)
            .expect("llms text"),
        serde_json::to_vec(&graph).expect("graph JSON"),
        serde_json::to_vec(&page.json_ld()).expect("shared JSON-LD"),
    ] {
        let output = String::from_utf8(output).expect("text projection");
        for absent in ["content:drop", "/drop/", "content:foreign", "PRIVATE DRAFT"] {
            assert!(
                !output.contains(absent),
                "excluded value {absent}: {output}"
            );
        }
    }
}

#[test]
fn cached_assets_and_uncompiled_notebooks_do_not_survive_explicit_exclusion() {
    let files = vec![
        (
            "authored/one/page.md".into(),
            page("content:keep", "published", "/keep/", "![kept](keep.png)\n"),
        ),
        (
            "authored/one/keep.png".into(),
            "retained image bytes".into(),
        ),
        (
            "authored/one/drop.pdf".into(),
            "private download bytes".into(),
        ),
        (
            "authored/one/notes.ipynb".into(),
            r#"{"nbformat":4,"nbformat_minor":5,"metadata":{},"cells":[]}"#.into(),
        ),
    ];
    let corpus = parse(&files);
    let compilation = compilation();
    let private = resolve(&compilation, &corpus);
    let plan = private
        .assets_for(&key("site:one"))
        .expect("populate private cache");
    assert_eq!(plan.entries().len(), 2);
    assert_eq!(plan.uncompiled(), [path("authored/one/notes.ipynb")]);
    let mut selection = all(&private);
    for excluded in ["authored/one/drop.pdf", "authored/one/notes.ipynb"] {
        selection
            .assets
            .insert(path(excluded), Disposition::Exclude);
    }
    let selected = private
        .into_selected(&selection)
        .expect("independent asset decisions");
    let plan = selected
        .assets_for(&key("site:one"))
        .expect("recomputed candidate cache");
    assert_eq!(plan.entries().len(), 1);
    assert!(plan.uncompiled().is_empty());
    assert!(plan.get(&path("authored/one/drop.pdf")).is_none());
    assert_eq!(
        plan.get(&path("authored/one/keep.png"))
            .expect("retained transport")
            .content(),
        files[1].1.as_bytes()
    );
    let page = selected
        .project_page(
            &key("content:keep"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("selected static reference");
    assert_eq!(page.data.assets.len(), 1);
    assert!(
        page.body_html
            .contains("https://example.test/mount/media/keep.png")
    );
    assert_eq!(
        selected.corpus().files().len(),
        4,
        "private retained evidence is not deleted"
    );
}

#[test]
fn excluding_a_referenced_asset_fails_even_when_every_content_item_is_included() {
    let compilation = compilation();
    for (filename, body, source_relative) in [
        ("image.png", "![retained image](image.png)\n", true),
        ("download.pdf", "[retained download](download.pdf)\n", true),
        (
            "image.png",
            "![retained image](/mount/media/image.png?size=1#preview)\n",
            false,
        ),
        (
            "download.pdf",
            "[retained download](https://example.test/mount/media/download.pdf?version=1#page=2)\n",
            false,
        ),
    ] {
        let asset_path = format!("authored/one/{filename}");
        let files = vec![
            (
                "authored/one/page.md".into(),
                page("content:keep", "published", "/keep/", body),
            ),
            (asset_path.clone(), "retained opaque bytes".into()),
        ];
        let corpus = parse(&files);
        let private = resolve(&compilation, &corpus);
        assert_eq!(
            private
                .project_page(
                    &key("content:keep"),
                    &gazette_core::raster::unavailable,
                    &gazette_adapter_markdown::Comrak
                )
                .expect("valid private asset reference")
                .data
                .assets
                .len(),
            usize::from(source_relative),
            "only source-relative syntax attaches an asset to the private page model",
        );
        assert!(
            private
                .assets_for(&key("site:one"))
                .expect("known native transport plan")
                .get(&path(&asset_path))
                .is_some()
        );
        let mut selection = all(&private);
        assert!(
            selection
                .content
                .values()
                .all(|value| *value == Disposition::Include)
        );
        selection
            .assets
            .insert(path(&asset_path), Disposition::Exclude);
        let error = private
            .into_selected(&selection)
            .expect_err("asset closure applies without any excluded content URL");
        assert_eq!(error.code(), "selection_asset");
        assert!(matches!(error, SelectionError::Asset {
            path: actual,
            reason: "selected body requires an included asset",
        } if actual == path(&asset_path)));
    }
}

#[test]
fn excluded_cross_site_assets_are_checked_without_compiling_unreferenced_sites() {
    let compilation = compilation();
    for destination in [
        "https://other.test/media/secret.png?size=2#preview",
        "//other.test/media/secret.png",
    ] {
        let files = vec![
            (
                "authored/one/keep.md".into(),
                page(
                    "content:keep",
                    "published",
                    "/keep/",
                    &format!("![foreign]({destination})\n"),
                ),
            ),
            ("authored/two/secret.png".into(), "private bytes".into()),
        ];
        let corpus = parse(&files);
        let private = resolve(&compilation, &corpus);
        let included = all(&private);
        assert!(
            resolve(&compilation, &corpus)
                .into_selected(&included)
                .is_ok()
        );
        let mut selection = included;
        selection
            .assets
            .insert(path("authored/two/secret.png"), Disposition::Exclude);
        assert!(
            matches!(private.into_selected(&selection), Err(SelectionError::Asset {
            path: actual, reason: "selected body requires an included asset",
        }) if actual == path("authored/two/secret.png"))
        );
    }

    let files = vec![
        (
            "authored/one/keep.md".into(),
            page(
                "content:keep",
                "published",
                "/keep/",
                "[external](https://unrelated.test/)\n",
            ),
        ),
        ("authored/two/invalid%.png".into(), "private bytes".into()),
    ];
    let corpus = parse(&files);
    let private = resolve(&compilation, &corpus);
    assert!(
        private.assets_for(&key("site:two")).is_err(),
        "invalid foreign route"
    );
    let mut selection = all(&private);
    selection
        .assets
        .insert(path("authored/two/invalid%.png"), Disposition::Exclude);
    let selected = private
        .into_selected(&selection)
        .expect("unreferenced foreign failure stays isolated");
    assert!(
        selected
            .project_page(
                &key("content:keep"),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak
            )
            .is_ok()
    );
}

#[test]
fn asset_decisions_are_complete_exact_and_independent_of_content_dispositions() {
    let files = vec![
        (
            "authored/one/page.md".into(),
            page("content:keep", "published", "/keep/", "Body\n"),
        ),
        ("authored/one/keep.png".into(), "bytes".into()),
        (
            "authored/one/notes.ipynb".into(),
            r#"{"nbformat":4,"nbformat_minor":5,"metadata":{},"cells":[]}"#.into(),
        ),
    ];
    let corpus = parse(&files);
    let compilation = compilation();
    for missing in ["authored/one/keep.png", "authored/one/notes.ipynb"] {
        let private = resolve(&compilation, &corpus);
        let mut selection = all(&private);
        selection.assets.remove(&path(missing));
        let error = private
            .into_selected(&selection)
            .expect_err("all retained asset kinds require a decision");
        assert!(
            matches!(error, SelectionError::Asset {path: actual, reason: "missing explicit disposition"} if actual == path(missing))
        );
    }
    for unknown in ["authored/one/unknown.png", "authored/one/page.md"] {
        let private = resolve(&compilation, &corpus);
        let mut selection = all(&private);
        selection.assets.insert(path(unknown), Disposition::Exclude);
        let error = private
            .into_selected(&selection)
            .expect_err("unknown and content paths are not asset decisions");
        assert!(
            matches!(error, SelectionError::Asset {path: actual, reason: "unknown asset or non-asset input"} if actual == path(unknown))
        );
    }
    let private = resolve(&compilation, &corpus);
    let mut selection = all(&private);
    selection.content.clear();
    assert!(matches!(
        private.into_selected(&selection),
        Err(SelectionError::Content(content::Error::Invalid(_)))
    ));
}

#[test]
fn drafts_require_explicit_exclusion_and_selected_programs_require_every_declared_input() {
    let program = "[[resources]]\ninputs=['main.ts','unused.ts']\nbuild={kind='browser-module',entry='main.ts'}\noutput='/page.js'\n";
    let files = vec![
        (
            "authored/one/page.md".into(),
            document(
                "content:keep",
                "published",
                "/keep/",
                program,
                "kind='page',family={kind='article'}",
                "Body\n",
            ),
        ),
        (
            "authored/one/draft.md".into(),
            page("content:draft", "draft", "/draft/", "Private\n"),
        ),
        (
            "authored/one/main.ts".into(),
            "export const value = 1;".into(),
        ),
        (
            "authored/one/unused.ts".into(),
            "export const extra = 2;".into(),
        ),
    ];
    let corpus = parse(&files);
    let compilation = compilation();
    let private = resolve(&compilation, &corpus);
    let selection = all(&private);
    assert!(
        matches!(private.into_selected(&selection), Err(SelectionError::Draft(id)) if id == key("content:draft"))
    );
    for input in ["main.ts", "unused.ts"] {
        let private = resolve(&compilation, &corpus);
        let mut selection = all(&private);
        selection
            .content
            .insert(key("content:draft"), Disposition::Exclude);
        let excluded = path(&format!("authored/one/{input}"));
        selection
            .assets
            .insert(excluded.clone(), Disposition::Exclude);
        let error = private
            .into_selected(&selection)
            .expect_err("declared closure, not merely entry or consumed imports");
        assert!(
            matches!(error, SelectionError::Asset {path, reason: "selected program requires an included input"} if path == excluded)
        );
    }
    let private = resolve(&compilation, &corpus);
    let mut selection = all(&private);
    selection
        .content
        .insert(key("content:draft"), Disposition::Exclude);
    let selected = private
        .into_selected(&selection)
        .expect("exact declared inputs retained without executing them");
    assert_eq!(selected.resources(&key("content:keep")).len(), 1);
    assert!(selected.resources(&key("content:draft")).is_empty());
}

#[test]
fn known_canonical_alias_and_relative_public_links_cannot_target_excluded_content() {
    let compilation = compilation();
    for destination in [
        "https://example.test/mount/drop/",
        "https://example.test/mount/old/?query=1#heading",
        "/mount/drop/",
        "../drop/",
    ] {
        let files = vec![
            (
                "authored/one/keep.md".into(),
                page(
                    "content:keep",
                    "published",
                    "/keep/",
                    &format!("[target]({destination})\n"),
                ),
            ),
            (
                "authored/one/drop.md".into(),
                page("content:drop", "published", "/drop/", "# Heading\n")
                    .replace("path='/drop/'", "path='/drop/',aliases=['/old/']"),
            ),
        ];
        let corpus = parse(&files);
        let private = resolve(&compilation, &corpus);
        let mut selection = all(&private);
        selection
            .content
            .insert(key("content:drop"), Disposition::Exclude);
        let error = private
            .into_selected(&selection)
            .expect_err("known outgoing target must not become a dangling URL");
        assert_eq!(
            error.code(),
            "selection_excluded_link",
            "{destination}: {error}"
        );
        let SelectionError::Link {
            content,
            target,
            span,
        } = error
        else {
            panic!("exact retained link diagnostic")
        };
        assert_eq!(content, key("content:keep"));
        assert_eq!(target, key("content:drop"));
        assert!(span.start < span.end);
    }
}

#[test]
fn native_markdown_links_and_derived_lookup_associations_use_the_same_selected_owner() {
    let compilation = compilation();
    let files = vec![
        (
            "native/library/README.md".into(),
            "# Native root\n\n[target](other.md#target)\n".into(),
        ),
        ("native/library/other.md".into(), "# Target\n".into()),
    ];
    let corpus = parse(&files);
    let private = resolve(&compilation, &corpus);
    let id_for = |name: &str| {
        private
            .content()
            .graph()
            .nodes()
            .find(|node| private.source_path(&node.id) == Some(&path(name)))
            .expect("derived native identity")
            .id
            .clone()
    };
    let root = id_for("native/library/README.md");
    let target = id_for("native/library/other.md");
    assert!(private.derivation(&target).is_some());
    let mut selection = all(&private);
    selection
        .content
        .insert(target.clone(), Disposition::Exclude);
    let error = private
        .into_selected(&selection)
        .expect_err("native source-coordinate links also need closure");
    assert!(
        matches!(error, SelectionError::Link {content, target: actual, ..} if content == root && actual == target)
    );
    let private = resolve(&compilation, &corpus);
    let mut selection = all(&private);
    selection.content.insert(root.clone(), Disposition::Exclude);
    let selected = private
        .into_selected(&selection)
        .expect("excluding link owner removes its outgoing obligations");
    assert!(selected.derivation(&root).is_none());
    assert!(selected.file(&root).is_none());
    assert!(selected.source_path(&root).is_none());
    assert!(selected.derivation(&target).is_some());
    assert!(
        selected
            .project_page(
                &target,
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak
            )
            .expect("selected native document")
            .data
            .derivation
            .is_some()
    );
}
