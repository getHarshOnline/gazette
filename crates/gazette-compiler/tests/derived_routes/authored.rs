use super::key;
use gazette_compiler::{
    authored,
    corpus::{Input, ParsedCorpus},
    workspace,
};

fn config() -> String {
    super::WORKSPACE.replace("home='content:home'\n", "").replace("[[brands]]", "[[sources]]\nid='source:publication'\nlocation={kind='filesystem',root='/never-opened/one'}\n[[sources]]\nid='source:product'\nlocation={kind='filesystem',root='/never-opened/two'}\n[[brands]]")
        + "authored=[{source='source:publication',target='site-folder'},{source='source:product',target='product-folder'}]\n"
}

#[test]
fn cross_source_membership_moves_urls_without_moving_bundles_or_identity_links() {
    let config = workspace::compile(&config(), Default::default()).unwrap();
    let child = "+++\n[content]\nid='content:opaque'\ntitle='A retained title'\nlifecycle='draft'\nslug='first-step'\nstructure={kind='page',family={kind='article'}}\n+++\n# A retained title\n\n## Detail {#detail}\n";
    let journey = "+++\n[content]\nid='journey:product'\ntitle='Product journey'\nlifecycle='draft'\nslug='product-journey'\nstructure={kind='journey',collection='collection:root',members=[{content='content:opaque',primary=true}]}\n+++\n# Product journey\n";
    let link = "+++\n[content]\nid='content:incoming'\ntitle='Incoming'\nlifecycle='draft'\nslug='incoming'\nstructure={kind='page',family={kind='article'}}\n+++\n[[id:content:opaque#detail|Retained label]]\n";
    for slug in ["systems", "systems-moved"] {
        let root = format!(
            "+++\n[content]\nid='collection:root'\ntitle='Root'\nlifecycle='draft'\nslug='{slug}'\nstructure={{kind='collection'}}\n+++\n# Root\n"
        );
        let corpus = ParsedCorpus::parse(
            [
                ("site-folder/root.md", root.as_str()),
                ("product-folder/journey/index.md", journey),
                ("product-folder/physical-bundle/index.md", child),
                ("site-folder/incoming.md", link),
            ]
            .into_iter()
            .map(|(path, text)| Input {
                path: path.parse().unwrap(),
                bytes: text.as_bytes(),
            }),
            Default::default(),
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
        let resolved = authored::resolve(&config, &corpus, Default::default()).unwrap();
        resolved.validate_source_bindings().unwrap();
        let page = resolved
            .project_page(
                &key("content:incoming"),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .unwrap();
        assert_eq!(
            page.data.links[0].destination,
            format!("https://one.test/mount/collections/{slug}/product-journey/first-step/#detail")
        );
        assert_eq!(page.data.links[0].content, Some(key("content:opaque")));
        let refs = resolved.publication_references(100, 1 << 20).unwrap();
        assert_eq!(
            refs[0].target.as_str(),
            "product-folder/physical-bundle/index.md"
        );
        assert!(
            resolved
                .origin(
                    &key("content:opaque"),
                    &[
                        gazette_compiler::toml::PathSegment::Key("content".into()),
                        gazette_compiler::toml::PathSegment::Key("route".into())
                    ]
                )
                .is_none()
        );
    }
}

#[test]
fn project_facts_join_the_derived_documentation_root_route() {
    let config = config() + "[[projects]]\nid='project:gadget'\nlanding='collection:docs'\n";
    let config = workspace::compile(&config, Default::default()).unwrap();
    let source = "+++\nprojects=[{project='project:gadget',kind='about'}]\n[project]\nid='project:gadget'\nname='Gadget'\nlifecycle='active'\nvisibility='public'\navailability='available'\n[content]\nid='collection:docs'\ntitle='Introduction'\nlifecycle='draft'\nslug='gadget'\nstructure={kind='collection',purpose='documentation'}\n+++\n# Introduction\n";
    let corpus = ParsedCorpus::parse(
        [Input {
            path: "product-folder/index.md".parse().unwrap(),
            bytes: source.as_bytes(),
        }],
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .unwrap();
    let resolved = authored::resolve(&config, &corpus, Default::default()).unwrap();
    let landing = resolved
        .content()
        .project(&key("project:gadget"))
        .unwrap()
        .resources
        .as_ref()
        .unwrap()
        .landing
        .as_ref()
        .unwrap();
    assert_eq!(landing, &key("collection:docs"));
    assert_eq!(
        resolved.content().route(landing).unwrap().url,
        "https://one.test/mount/gadget/"
    );
}
