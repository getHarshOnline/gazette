//! Ordered menus use the exact shared route and destination presentation owners.

use super::*;
use std::collections::BTreeMap;

const CHROME: &str = include_str!("../fixtures/site_chrome.toml");

fn workspace(menu: &str) -> workspace::Compilation {
    configured_workspace(menu, CHROME)
}

fn configured_workspace(menu: &str, chrome: &str) -> workspace::Compilation {
    configured_publication_workspace(menu, chrome, "", "")
}

fn configured_publication_workspace(
    menu: &str,
    chrome: &str,
    first: &str,
    second: &str,
) -> workspace::Compilation {
    let source = format!(
        "[composition]\n{menu}\n{chrome}\n\
        [[principals]]\nid='owner:one'\nkind='person'\n\
        [[sites]]\nid='site:a'\npublisher='owner:one'\ndeployable=false\n\
        [[sites]]\nid='site:b'\npublisher='owner:one'\ndeployable=false\n\
        [sites.composition]\ntheme={{package='theme:remote',palette='palette:night'}}\n\
        [[sources]]\nid='source:remote-theme'\nlocation={{kind='filesystem',root='/not-opened/theme'}}\n\
        [[theme_packages]]\npackage='theme:remote'\nsource='source:remote-theme'\nmanifest='gazette-theme.toml'\n\
        [[brands]]\nid='brand:one'\nname='One'\n\
        [[brands.publications]]\nsite='site:a'\npublic_base_url='https://a.test/mount/'\n\
        [[brands.publications]]\nsite='site:b'\npublic_base_url='https://b.test/mount/'\n"
    );
    let source = source
        .replace(
            "site='site:a'\npublic_base_url",
            &format!("site='site:a'\n{first}\npublic_base_url"),
        )
        .replace(
            "site='site:b'\npublic_base_url",
            &format!("site='site:b'\n{second}\npublic_base_url"),
        );
    workspace::compile(&source, toml::Limits::default()).unwrap()
}

const SEARCH: &str = "[composition.chrome.shell.search]\ntrigger_label='Search'\ntitle='Search'\nfield_label='Query'\nloading_message='Loading'\nempty_message='No results'\nunavailable_message='Unavailable'\n";

#[test]
fn publication_search_pages_use_shared_copy_and_their_own_resolved_routes() {
    let chrome = [CHROME, SEARCH, "no_script_destination='content:remote'\n"].concat();
    let workspace = configured_publication_workspace(
        "",
        &chrome,
        "search_page='content:local-search'",
        "search_page='content:remote'",
    );
    let mut files = files(true);
    files.push(("search.md", "+++\n[content]\nid='content:local-search'\nsite='site:a'\ntitle='Search'\nlifecycle='published'\nroute={path='/search/'}\nstructure={kind='page',family={kind='index'}}\n+++\n".into()));
    let corpus = parse(&files, Default::default());
    let resolved = authored::resolve(&workspace, &corpus, Default::default()).unwrap();
    for (site, id, url) in [
        (
            "site:a",
            "content:local-search",
            "https://a.test/mount/search/",
        ),
        ("site:b", "content:remote", "https://b.test/mount/remote/"),
    ] {
        let chrome = resolved.site_chrome(&key(site)).unwrap().unwrap();
        let search = chrome.shell.search.unwrap();
        assert_eq!(search.trigger_label, "Search");
        let destination = search.page.unwrap();
        assert_eq!(destination.content.id, key(id));
        assert_eq!(destination.route.url, url);
    }
    let preview = resolved
        .with_preview_urls(BTreeMap::from([(
            key("site:a"),
            "http://127.0.0.1:4310/mount/".parse().unwrap(),
        )]))
        .unwrap();
    assert_eq!(
        preview
            .site_chrome(&key("site:a"))
            .unwrap()
            .unwrap()
            .shell
            .search
            .unwrap()
            .page
            .unwrap()
            .route
            .url,
        "http://127.0.0.1:4310/mount/search/"
    );
    let schema = serde_json::to_value(schemars::schema_for!(workspace::BrandPublication)).unwrap();
    assert!(schema["properties"].get("search_page").is_some());
}

#[test]
fn publication_search_page_refuses_missing_foreign_unrouted_and_nonpage_bindings() {
    let chrome = [CHROME, SEARCH].concat();
    for (target, route, node_change) in [
        ("content:missing", true, ""),
        ("content:remote", true, ""),
        ("content:local-search", false, ""),
        ("content:local-search", true, "collection"),
        ("content:local-search", true, "copy"),
    ] {
        let workspace =
            configured_publication_workspace("", &chrome, &format!("search_page='{target}'"), "");
        let mut files = files(true);
        let route = if route {
            "route={path='/search/'}\n"
        } else {
            ""
        };
        let mut local = format!(
            "+++\n[content]\nid='content:local-search'\nsite='site:a'\ntitle='Search'\nlifecycle='published'\n{route}structure={{kind='page',family={{kind='index'}}}}\n+++\n"
        );
        if node_change == "collection" {
            local = local.replace("kind='page',family={kind='index'}", "kind='collection'");
        }
        if node_change == "copy" {
            local = local.replace(
                "path='/search/'",
                "path='/search/',canonical='content:original'",
            );
            files.push(("original.md","+++\n[content]\nid='content:original'\nsite='site:a'\ntitle='Original'\nlifecycle='published'\nroute={path='/original/'}\nstructure={kind='page',family={kind='index'}}\n+++\n".into()));
        }
        files.push(("search.md", local));
        let corpus = parse(&files, Default::default());
        assert!(
            matches!(
                authored::resolve(&workspace, &corpus, Default::default()),
                Err(authored::Error::PublicationMetadata(_))
            ),
            "{target} {node_change}"
        );
    }
    let files = files(true);
    let corpus = parse(&files, Default::default());
    let legacy = [CHROME, SEARCH, "no_script_destination='content:remote'\n"].concat();
    let compatible =
        configured_publication_workspace("", &legacy, "", "search_page='content:remote'");
    assert!(authored::resolve(&compatible, &corpus, Default::default()).is_ok());
}

fn files(route: bool) -> Vec<(&'static str, String)> {
    let route = if route {
        "route={path='/remote/'}\n"
    } else {
        ""
    };
    vec![(
        "remote.md",
        format!(
            "+++\n[content]\nid='content:remote'\nsite='site:b'\ntitle='Remote'\nlifecycle='published'\n{route}structure={{kind='page',family={{kind='documentation'}}}}\n+++\nRemote\n"
        ),
    )]
}

#[test]
fn menus_share_public_and_preview_crossings_with_destination_presentation() {
    let workspace = workspace(
        "navigation=[{label='First',destination='content:remote'},{label='Again',destination='content:remote'}]\nfooter=[{key='theme:credit',label='Read remotely',destination='content:remote'}]",
    );
    let files = files(true);
    let corpus = parse(&files, corpus::Limits::default());
    let resolved = authored::resolve(&workspace, &corpus, content::Limits::default()).unwrap();
    let chrome = resolved.site_chrome(&key("site:a")).unwrap().unwrap();
    assert_eq!(
        chrome
            .navigation
            .iter()
            .map(|entry| entry.label)
            .collect::<Vec<_>>(),
        ["First", "Again"]
    );
    assert_eq!(chrome.footer[0].label, "Read remotely");
    assert_eq!(chrome.footer[0].key, Some(&key("theme:credit")));
    assert!(chrome.navigation[0].key.is_none());
    assert_eq!(
        chrome.footer[0].destination.route.url,
        "https://b.test/mount/remote/"
    );
    let presentation = chrome.footer[0]
        .destination
        .content
        .presentation
        .as_ref()
        .unwrap();
    assert_eq!(presentation.theme.as_str(), "theme:remote");
    assert_eq!(
        presentation.palette.as_ref().unwrap(),
        &key("palette:night")
    );
    let json = serde_json::to_value(&chrome).unwrap();
    assert!(json.get("reading_assistance").is_none());
    assert_eq!(
        json["footer"][0]["destination"]["content"]["id"],
        "content:remote"
    );
    assert_eq!(chrome.skip_to_content_label, "Skip to content");
    let local = resolved
        .with_preview_urls(BTreeMap::from([(
            key("site:b"),
            "http://127.0.0.1:4311/mount/".parse().unwrap(),
        )]))
        .unwrap();
    let chrome = local.site_chrome(&key("site:a")).unwrap().unwrap();
    assert_eq!(
        chrome.navigation[0].destination.route.url,
        "http://127.0.0.1:4311/mount/remote/"
    );
    assert_eq!(
        chrome.footer[0].destination.route.url,
        chrome.navigation[0].destination.route.url
    );
    assert_eq!(
        chrome.footer[0]
            .destination
            .content
            .presentation
            .as_ref()
            .unwrap()
            .theme
            .as_str(),
        "theme:remote"
    );
}

#[test]
fn optional_reading_assistance_retains_exact_contract_without_preview_endpoint_rewrites() {
    let chrome = [CHROME, include_str!("../fixtures/reading_assistance.toml")].concat();
    let workspace = configured_workspace("", &chrome);
    let files = files(true);
    let corpus = parse(&files, corpus::Limits::default());
    let resolved = authored::resolve(&workspace, &corpus, content::Limits::default()).unwrap();
    let selected = resolved.site_chrome(&key("site:b")).unwrap().unwrap();
    let assistance = serde_json::to_value(selected.reading_assistance.unwrap()).unwrap();
    assert_eq!(assistance["providers"][0]["id"], "assistant:chosen");
    assert_eq!(
        assistance["providers"][0]["endpoint"],
        "https://assistant.example/new?mode=reading"
    );
    let preview = resolved
        .with_preview_urls(BTreeMap::from([(
            key("site:b"),
            "http://127.0.0.1:4311/mount/".parse().unwrap(),
        )]))
        .unwrap();
    let selected = preview.site_chrome(&key("site:b")).unwrap().unwrap();
    assert_eq!(
        serde_json::to_value(selected.reading_assistance.unwrap()).unwrap(),
        assistance
    );
    let page = preview
        .project_page(
            &key("content:remote"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    let linked = serde_json::to_value(page.json_ld()).unwrap();
    assert!(!linked.to_string().contains("assistant.example"));
}

#[test]
fn missing_or_unrouted_menu_destinations_refuse_without_partial_chrome() {
    for field in ["navigation", "footer"] {
        for (destination, has_route) in [("content:missing", true), ("content:remote", false)] {
            let workspace = workspace(&format!(
                "{field}=[{{label='Selected',destination='{destination}'}}]"
            ));
            let files = files(has_route);
            let corpus = parse(&files, corpus::Limits::default());
            let resolved =
                authored::resolve(&workspace, &corpus, content::Limits::default()).unwrap();
            assert!(matches!(
                resolved.site_chrome(&key("site:a")),
                Err(ProjectionError::Binding { .. })
            ));
        }
    }
}

#[test]
fn selected_menu_without_chrome_copy_is_not_silently_ignored() {
    let files = files(true);
    let corpus = parse(&files, corpus::Limits::default());
    let workspace = configured_workspace("footer=[]", "");
    let resolved = authored::resolve(&workspace, &corpus, content::Limits::default()).unwrap();
    assert!(matches!(
        resolved.site_chrome(&key("site:a")),
        Err(ProjectionError::Binding { .. })
    ));
}

#[test]
fn invitations_select_articles_and_exact_pages_without_footer_or_template_inference() {
    let source = "[[principals]]\nid='owner:one'\nkind='person'\n\
        [[sites]]\nid='site:a'\npublisher='owner:one'\ndeployable=false\n\
        [sites.composition.invitations.'theme:services']\nstate='enabled'\ndestination='content:remote'\narticles=true\n\
        pages={'content:about'=true,'content:project'=true,'content:excluded'=false}\n\
        [[sites]]\nid='site:b'\npublisher='owner:one'\ndeployable=false\n\
        [[brands]]\nid='brand:one'\nname='One'\n\
        [[brands.publications]]\nsite='site:a'\npublic_base_url='https://a.test/'\n\
        [[brands.publications]]\nsite='site:b'\npublic_base_url='https://b.test/'\n";
    let workspace = workspace::compile(source, toml::Limits::default()).unwrap();
    let mut files = files(true);
    for (id, family) in [
        ("article", "article"),
        ("excluded", "article"),
        ("about", "utility"),
        ("home", "index"),
    ] {
        files.push((match id { "article" => "article.md", "excluded" => "excluded.md", "about" => "about.md", _ => "home.md" }, format!("+++\n[content]\nid='content:{id}'\nsite='site:a'\ntitle='{id}'\nlifecycle='draft'\nroute={{path='/{id}/'}}\nstructure={{kind='page',family={{kind='{family}'}}}}\n+++\nBody\n")));
    }
    files.push(("project.md", "+++\n[content]\nid='content:project'\nsite='site:a'\ntitle='Project'\nlifecycle='draft'\nroute={path='/project/'}\nstructure={kind='collection',purpose='documentation'}\n+++\nProject landing\n".into()));
    let corpus = parse(&files, Default::default());
    let resolved = authored::resolve(&workspace, &corpus, Default::default()).unwrap();
    let preview = resolved
        .with_preview_urls(BTreeMap::from([(
            key("site:b"),
            "http://127.0.0.1:4311/".parse().unwrap(),
        )]))
        .unwrap();
    for (id, selected) in [
        ("article", true),
        ("excluded", false),
        ("about", true),
        ("project", true),
        ("home", false),
        ("remote", false),
    ] {
        let page = preview
            .project_page(
                &key(&format!("content:{id}")),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .unwrap();
        let json = serde_json::to_value(&page).unwrap();
        assert_eq!(json.get("invitations").is_some(), selected, "{id}");
        if selected {
            assert_eq!(
                json["invitations"]["theme:services"]["destination"]["route"]["url"],
                "http://127.0.0.1:4311/remote/"
            );
        }
    }
    for (old, new) in [
        ("content:about", "content:missing"),
        ("content:about", "content:remote"),
        (
            "destination='content:remote'",
            "destination='content:missing'",
        ),
    ] {
        let invalid =
            workspace::compile(&source.replace(old, new), toml::Limits::default()).unwrap();
        let resolved = authored::resolve(&invalid, &corpus, Default::default()).unwrap();
        assert!(resolved.site_chrome(&key("site:a")).is_err(), "{new}");
    }
}

#[test]
fn invitation_destination_context_does_not_require_a_footer_or_visible_self_invitation() {
    let workspace = workspace(
        "footer=[]\ninvitations.'theme:services'={state='enabled',destination='content:remote',articles=true}",
    );
    let files = files(true);
    let corpus = parse(&files, Default::default());
    let resolved = authored::resolve(&workspace, &corpus, Default::default()).unwrap();
    let chrome = resolved.site_chrome(&key("site:b")).unwrap().unwrap();
    assert!(chrome.footer.is_empty());
    assert_eq!(
        chrome.invitation_destinations[&key("theme:services")]
            .content
            .id,
        key("content:remote")
    );
    assert_eq!(
        chrome.invitation_destinations[&key("theme:services")]
            .route
            .url,
        "https://b.test/mount/remote/"
    );
    let page = resolved
        .project_page(
            &key("content:remote"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    assert!(page.data.invitations.is_empty());
}
