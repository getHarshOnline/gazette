use gazette_sdk::configuration::{
    ListedEntry, ListingLimits, discover_authoring_sources, discover_sources, load_workspace,
};

#[test]
fn workspace_roles_select_sources_without_opening_unrelated_roots() {
    let root = tempfile::tempdir().unwrap();
    for directory in ["pages", "manual", "packages"] {
        std::fs::create_dir(root.path().join(directory)).unwrap();
    }
    std::fs::write(root.path().join("pages/index.md"), "# Page").unwrap();
    std::fs::write(root.path().join("manual/README.md"), "# Manual").unwrap();
    std::fs::write(root.path().join("packages/theme.toml"), "unparsed manifest").unwrap();
    std::fs::write(
        root.path().join("packages/plugin.toml"),
        "unparsed manifest",
    )
    .unwrap();
    let config = r#"
[[principals]]
id='owner:one'
kind='person'
[[sites]]
id='site:one'
publisher='owner:one'
deployable=false
[[projects]]
id='project:one'
lifecycle='active'
visibility='public'
availability='available'
[[sources]]
id='source:pages'
location={kind='filesystem',root='pages'}
[[sources]]
id='source:manual'
location={kind='filesystem',root='manual'}
[[sources]]
id='source:packages'
location={kind='filesystem',root='packages'}
[[sources]]
id='source:unused'
location={kind='filesystem',root='does-not-exist'}
[[theme_packages]]
package='theme:one'
source='source:packages'
manifest='theme.toml'
[[plugin_packages]]
package='example-plugin'
source='source:packages'
manifest='plugin.toml'
[[brands]]
id='brand:one'
name='Example'
[[brands.publications]]
site='site:one'
authored=[{source='source:pages',target='content'}]
[brands.publications.documentation_repository]
name='manual'
path='output'
[brands.publications.documentation_repository.build]
target={kind='mintlify',theme='mint',primary_color='#123abc'}
[[brands.publications.documentation_repository.build.modules]]
project='project:one'
slug='example'
title='Example'
manual={source='source:manual'}
navigation=[{kind='manual',title='Start',pages=[{title='Overview',route='example/overview',input={kind='file',path='README.md'}}]}]
"#;
    std::fs::write(root.path().join("gazette.toml"), config).unwrap();
    let loaded = load_workspace(
        root.path(),
        &"gazette.toml".parse().unwrap(),
        Default::default(),
    )
    .unwrap();
    let sources = discover_authoring_sources(&loaded, ListingLimits::default()).unwrap();
    assert_eq!(
        sources
            .iter()
            .map(|source| source.source.as_str())
            .collect::<Vec<_>>(),
        ["source:manual", "source:packages", "source:pages"]
    );
    std::fs::remove_file(root.path().join("manual/README.md")).unwrap();
    std::fs::remove_dir(root.path().join("manual")).unwrap();
    assert!(
        discover_authoring_sources(&loaded, ListingLimits::default()).is_err(),
        "a missing configured manual fails discovery instead of silently vanishing"
    );
}

#[test]
fn configured_source_discovery_lists_assets_and_preserves_capture_relative_paths() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("pages")).unwrap();
    std::fs::create_dir(root.path().join("manuals")).unwrap();
    std::fs::write(root.path().join("pages/guide.md"), "# Guide").unwrap();
    std::fs::File::create(root.path().join("pages/film.mp4"))
        .unwrap()
        .set_len(64_u64 << 30)
        .unwrap();
    std::fs::write(root.path().join("manuals/README.md"), "# Manual").unwrap();
    std::fs::write(
        root.path().join("gazette.toml"),
        "[[principals]]\nid='owner:one'\nkind='person'\n\
         [[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n\
         [[sources]]\nid='source:pages'\nlocation={kind='included-filesystem',root='.',include=['pages']}\n\
         [[sources]]\nid='source:manuals'\nlocation={kind='filesystem',root='.',subdirectory='manuals'}\n",
    )
    .unwrap();
    let loaded = load_workspace(
        root.path(),
        &"gazette.toml".parse().unwrap(),
        Default::default(),
    )
    .unwrap();
    let pages = "source:pages".parse().unwrap();
    let manuals = "source:manuals".parse().unwrap();
    let sources = discover_sources(
        &loaded,
        [&pages, &manuals, &pages],
        ListingLimits::default(),
    )
    .unwrap();
    assert_eq!(
        sources.len(),
        2,
        "repeated bindings list their shared owner once"
    );
    let pages = sources.iter().find(|s| s.source == pages).unwrap();
    assert!(pages.subdirectory.is_none());
    assert!(pages.listing.entries().contains_key("pages/film.mp4"));
    assert_eq!(
        pages.listing.entries().get("pages/film.mp4"),
        Some(&ListedEntry::File {
            observed_bytes: 64_u64 << 30
        })
    );
    assert!(pages.listing.entries().contains_key("pages/guide.md"));
    let manual = sources.iter().find(|s| s.source == manuals).unwrap();
    assert_eq!(manual.subdirectory.as_ref().unwrap().to_string(), "manuals");
    assert!(manual.listing.entries().contains_key("README.md"));
    assert_eq!(manual.root, root.path());
    assert!(
        discover_sources(
            &loaded,
            [&"source:pages".parse().unwrap(), &manuals],
            ListingLimits {
                entries: 3,
                ..ListingLimits::default()
            }
        )
        .is_err(),
        "metadata limits apply across sources"
    );
    assert!(
        discover_sources(
            &loaded,
            [&"source:absent".parse().unwrap()],
            ListingLimits::default()
        )
        .is_err()
    );
}
