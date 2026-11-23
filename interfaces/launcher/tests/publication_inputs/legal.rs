//! Real native legal capture composes output identities without copying owner prose.

use super::*;

const TERMS: &str = "+++\n[content]\nid='content:owner-terms'\ntitle='Owner terms'\nlifecycle='draft'\nlanguage='en'\n+++\n# Exact owner terms\n\nRetained **single-source** prose.\n";

fn configuration(legal: &Fixture, theme: &Fixture, entry: &str) -> String {
    format!(
        "[composition]\ntheme={{package='theme:one'}}\n\
         [[principals]]\nid='owner:one'\nkind='person'\n\
         [[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n\
         [[sites]]\nid='site:other'\npublisher='owner:one'\ndeployable=false\n\
         [[brands]]\nid='brand:one'\nname='One'\n\
         [[brands.publications]]\nsite='site:one'\npublic_base_url='https://one.example/'\n\
         legal=[{{source='source:legal',entry='{entry}',source_content='content:owner-terms',id='content:one-terms',route='/terms/',family='utility'}}]\n\
         [[brands.publications]]\nsite='site:other'\npublic_base_url='https://other.example/'\n\
         legal=[{{source='source:legal',entry='{entry}',source_content='content:owner-terms',id='content:other-terms',route='/policy/',family='utility'}}]\n\
         {}{}\
         [[theme_packages]]\npackage='theme:one'\nsource='source:theme'\nmanifest='theme.toml'\n",
        legal.declaration("source:legal"),
        theme.declaration("source:theme"),
    )
}

fn theme() -> Fixture {
    Fixture::new(&[
        ("theme.toml", THEME_MANIFEST.as_bytes()),
        (
            "templates/page.html",
            b"{{ content.id }}:{{ body_html | safe }}",
        ),
    ])
}

#[test]
fn legal_only_native_publication_retains_one_selected_body_and_two_output_provenances() {
    let legal = Fixture::new(&[
        ("terms.md", TERMS.as_bytes()),
        (
            "unselected.md",
            b"+++\nnot valid publication metadata\n+++\n",
        ),
        ("private.bin", &[0xff, 0]),
    ]);
    let theme = theme();
    let config = configuration(&legal, &theme, "terms.md");
    let acquired = acquire(&config, acquisition_limits()).expect("legal-only native acquisition");
    assert_eq!(acquired.sources().len(), 2);
    fs::write(
        legal.directory.path().join("selected/terms.md"),
        "changed live source",
    )
    .expect("mutate disposable source after acquisition");
    let parsed = acquired
        .parse_publication(
            corpus::Limits {
                files: 1,
                ..Default::default()
            },
            &gazette_adapter_markdown::Comrak,
        )
        .expect("one retained legal document; unselected siblings are not parsed");
    let resolved = parsed
        .resolve(Default::default())
        .expect("two explicit legal outputs");
    let authored = resolved.authored();
    authored
        .validate_source_bindings()
        .expect("output-aware native associations");
    assert_eq!(authored.content().graph().nodes().count(), 2);
    let one = "content:one-terms".parse().expect("output identity");
    let other = "content:other-terms".parse().expect("output identity");
    assert_eq!(authored.source_path(&one), authored.source_path(&other));
    let first = authored.file(&one).expect("retained file");
    let second = authored.file(&other).expect("same retained file");
    assert!(
        std::ptr::eq(first, second),
        "not duplicate syntax/body storage"
    );
    for (id, site, url) in [
        (&one, "site:one", "https://one.example/terms/"),
        (&other, "site:other", "https://other.example/policy/"),
    ] {
        let binding = authored
            .source_binding(id)
            .expect("specific output binding");
        assert_eq!(binding.site.as_str(), site);
        assert_eq!(binding.source_key().as_str(), "source:legal");
        assert!(binding.tree_binding().is_none());
        assert_eq!(
            binding
                .legal_binding()
                .expect("explicit legal association")
                .id,
            *id
        );
        assert_eq!(
            binding.entry(authored.source_path(id).unwrap()),
            Some("terms.md")
        );
        assert_eq!(authored.content().route(id).unwrap().canonical_url, url);
        let page = authored
            .project_page(
                id,
                &gazette_sdk::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .expect("same retained legal body rendered by native Markdown");
        assert!(page.body_html.contains("<strong>single-source</strong>"));
        assert!(!page.body_html.contains("changed live source"));
    }
    let registry = serde_json::to_value(
        resolved
            .registry(Default::default())
            .expect("whole private registry"),
    )
    .expect("registry transport");
    assert_eq!(
        registry["documents"]["content:one-terms"],
        registry["documents"]["content:other-terms"]
    );
    assert_eq!(registry["documents"].as_object().unwrap().len(), 2);
    let model_output = "model.json".parse().expect("private model output");
    let render = build::render_resolved_publication(
        &resolved,
        build::SiteRenderOptions {
            render_year: None,
            model_output: Some(&model_output),
            graph: None,
            limits: Default::default(),
        },
        Default::default(),
    )
    .expect("real retained Tera publication from the legal-only graph");
    let html = std::str::from_utf8(
        render
            .site()
            .output()
            .file(&"terms/index.html".parse().unwrap())
            .unwrap(),
    )
    .expect("native HTML");
    assert!(html.contains("content:one-terms"));
    assert!(html.contains("<strong>single-source</strong>"));
    assert!(!html.contains("content:other-terms"));
    assert!(
        render
            .site()
            .output()
            .file(&"policy/index.html".parse().unwrap())
            .is_none(),
        "complete graph knowledge does not render another site's output"
    );
}

#[test]
fn absent_selected_legal_entry_refuses_instead_of_ingesting_a_sibling() {
    let legal = Fixture::new(&[("other.md", TERMS.as_bytes())]);
    let acquired = acquire(
        &configuration(&legal, &theme(), "missing.md"),
        acquisition_limits(),
    )
    .expect("native source capture exists");
    assert!(matches!(
        acquired.parse_publication(Default::default(), &gazette_adapter_markdown::Comrak),
        Err(build::PublicationInputError::Document(build::DocumentError::MissingPayload(path)))
            if path.as_str() == "missing.md"
    ));
}
