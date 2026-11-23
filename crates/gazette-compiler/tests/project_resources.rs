//! Project navigation and reference-derived article views share native graph authority.
use gazette_compiler::{
    authored::{self, ListingError},
    corpus::{Input, ParsedCorpus},
    workspace,
};
use gazette_core::{markdown::ProjectResourceKind as Kind, publication::Key};
#[path = "project_resources/semantics.rs"]
mod semantics;

fn key(value: &str) -> Key {
    value.parse().expect("explicit key")
}
fn config(scope: &str, resources: &str) -> workspace::Compilation {
    workspace::compile(&configuration_text(scope, resources), Default::default())
        .expect("strict native workspace")
}
fn configuration_text(scope: &str, resources: &str) -> String {
    format!(
        r#"
[[principals]]
id='owner:one'
kind='person'
[[sites]]
id='site:one'
publisher='owner:one'
deployable=false
[[sites]]
id='site:two'
publisher='owner:one'
deployable=false
[[sources]]
id='source:content'
location={{kind='filesystem',root='/private/not-read'}}
[[brands]]
id='brand:one'
name='First fixture'
[[brands.publications]]
site='site:one'
public_base_url='https://one.test/'
authored=[{{source='source:content',target='one'}}]
listings=[{{content='page:blogs',select={{kind='project-articles',project='project:one',landing='page:product',scope='{scope}'}},order={{by='published',direction='descending',undated='last-title-then-id'}},alternate_order={{order={{by='published',direction='ascending',undated='last-title-then-id'}},path='oldest'}},pagination={{page_size=15,next_pages='page/{{number}}/'}}}}]
[[brands]]
id='brand:two'
name='Second fixture'
[[brands.publications]]
site='site:two'
public_base_url='https://two.test/'
authored=[{{source='source:content',target='two'}}]
[[projects]]
id='project:one'
name='The specimen'
lifecycle='planned'
visibility='public'
availability='unavailable'
{resources}
"#
    )
}
fn document(id: &str, site: &str, route: &str, family: &str, edges: &str, body: &str) -> String {
    format!(
        "+++\n{edges}\n[content]\nid='{id}'\nsite='{site}'\ntitle='{id}'\nlifecycle='published'\nlanguage='en'\nroute={{path='{route}'}}\nstructure={{kind='page',family={{kind='{family}'}}}}\n+++\n{body}\n"
    )
}
fn files() -> Vec<(String, String)> {
    vec![
        (
            "one/product.md".into(),
            document(
                "page:product",
                "site:one",
                "/product/",
                "project",
                "projects=[{project='project:one',kind='about'}]",
                "# Product",
            ),
        ),
        (
            "one/learn.md".into(),
            document(
                "page:learn",
                "site:one",
                "/product/docs/",
                "documentation",
                "",
                "# Learning",
            ),
        ),
        (
            "one/blogs.md".into(),
            document("page:blogs", "site:one", "/product/blog/", "index", "", ""),
        ),
    ]
}
fn parse(files: &[(String, String)]) -> ParsedCorpus<'_> {
    ParsedCorpus::parse(
        files.iter().map(|(path, text)| Input {
            path: path.parse().expect("path"),
            bytes: text.as_bytes(),
        }),
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("actual retained source parse")
}

#[test]
fn product_help_collection_keeps_the_landing_identity_and_resolves_its_children() {
    let config = config(
        "site",
        "resources={landing='page:product',learn='page:product'}",
    );
    let mut files = files();
    files[0].1 = files[0].1.replace(
        "structure={kind='page',family={kind='project'}}",
        "structure={kind='collection'}",
    );
    files[1].1 = files[1]
        .1
        .replacen("+++\n", "+++\ncollections=['page:product']\n", 1);
    let corpus = parse(&files);
    let resolved = authored::resolve(&config, &corpus, Default::default()).unwrap();
    let page = resolved
        .project_page(
            &key("page:product"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("the existing landing is also the help collection");
    let model = serde_json::to_value(page).unwrap();
    assert_eq!(
        resolved
            .content()
            .graph()
            .project_relations(&key("page:product"))
            .unwrap()[&key("project:one")]
            .relation(),
        gazette_core::publication::content::ProjectRelation::About
    );
    assert!(
        !resolved
            .content()
            .graph()
            .project_relations(&key("page:learn"))
            .unwrap()
            .contains_key(&key("project:one")),
        "a collection subject does not silently become inherited subject matter"
    );
    assert_eq!(model["content"]["id"], "page:product");
    assert_eq!(model["content"]["structure"]["kind"], "collection");
    assert_eq!(
        model["project_resources"]["actions"],
        serde_json::json!([]),
        "a help landing omits its self-pointing Learn action"
    );
    assert_eq!(model["children"][0]["id"], "page:learn");
    assert_eq!(
        model["children"][0]["resolved_route"]["url"],
        "https://one.test/product/docs/"
    );
    assert!(
        resolved
            .project_listing_pages(&key("site:one"), &resolved.discovery().unwrap())
            .is_ok()
    );
}

#[test]
fn an_unselected_editorial_collection_does_not_acquire_product_resources() {
    let config = config(
        "site",
        "resources={landing='page:product',learn='page:learn'}",
    );
    let mut files = files();
    files.push((
        "one/editorial.md".into(),
        document(
            "collection:editorial",
            "site:one",
            "/inquiry/",
            "project",
            "projects=[{project='project:one',kind='about'}]",
            "# An inquiry",
        )
        .replace(
            "structure={kind='page',family={kind='project'}}",
            "structure={kind='collection'}",
        ),
    ));
    let corpus = parse(&files);
    let resolved = authored::resolve(&config, &corpus, Default::default()).unwrap();
    assert!(
        resolved
            .project_resources_for(&key("collection:editorial"))
            .unwrap()
            .is_none()
    );
}

#[test]
fn preview_hyperlinks_and_docs_action_share_the_exact_explicit_base() {
    let config = config(
        "site",
        "resources={landing='page:product',learn='page:learn',code_documentation='https://docs.example.test/tool/overview'}",
    );
    let mut files = files();
    files[0].1.push_str("\n[Build](https://docs.example.test/tool/install?q=exact#build)\n\n[Other](https://other.example.test/guide)\n\n`https://docs.example.test/tool/install`\n");
    let corpus = parse(&files);
    let resolved = authored::resolve(&config, &corpus, Default::default())
        .unwrap()
        .with_preview_link_bases(
            gazette_core::publication::routing::PreviewLinkBases::try_from(
                std::collections::BTreeMap::from([(
                    "https://docs.example.test/".into(),
                    "http://127.0.0.1:4500/".parse().unwrap(),
                )]),
            )
            .unwrap(),
        );
    let page = resolved
        .project_page(
            &key("page:product"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    assert!(
        page.body_html
            .contains("href=\"http://127.0.0.1:4500/tool/install?q=exact#build\"")
    );
    assert!(
        page.body_html
            .contains("href=\"https://other.example.test/guide\"")
    );
    assert!(
        page.body_html
            .contains("<code>https://docs.example.test/tool/install</code>")
    );
    let docs = &page.data.project_resources.as_ref().unwrap().actions[0];
    assert_eq!(docs.url, "http://127.0.0.1:4500/tool/overview");
    assert_eq!(
        docs.destination.kind,
        gazette_core::markdown::LinkDestinationKind::External
    );
    assert_eq!(
        resolved
            .content()
            .route(&key("page:product"))
            .unwrap()
            .canonical_url,
        "https://one.test/product/"
    );
}

#[test]
fn owned_links_follow_local_mode_and_missing_documentation_provider_is_explicit() {
    let source = configuration_text("site", "").replacen(
        "\n[[brands]]\nid='brand:two'",
        "\n[brands.publications.documentation_repository]\nname='Docs'\npath='/private/not-read-docs'\npublic_base_url='https://docs.example.test/'\n\n[[brands]]\nid='brand:two'",
        1,
    );
    let compiled = workspace::compile(&source, Default::default()).unwrap();
    let mut files = files();
    files[0].1.push_str("\n[Article](https://two.test/entry/?q=one#answer)\n\n[Manual](https://docs.example.test/tool/kernel#first-call)\n");
    let corpus = parse(&files);
    let local = || {
        authored::resolve(&compiled, &corpus, Default::default())
            .unwrap()
            .with_preview_urls(std::collections::BTreeMap::from([
                (key("site:one"), "http://127.0.0.1:4500/".parse().unwrap()),
                (key("site:two"), "http://127.0.0.1:4501/".parse().unwrap()),
            ]))
            .unwrap()
    };
    assert!(
        local()
            .project_page(
                &key("page:product"),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak
            )
            .is_err()
    );
    let resolved = local().with_preview_link_bases(
        gazette_core::publication::routing::PreviewLinkBases::try_from(
            std::collections::BTreeMap::from([(
                "https://docs.example.test/".into(),
                "http://127.0.0.1:4502/".parse().unwrap(),
            )]),
        )
        .unwrap(),
    );
    let page = resolved
        .project_page(
            &key("page:product"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    assert!(
        page.body_html
            .contains("href=\"http://127.0.0.1:4501/entry/?q=one#answer\"")
    );
    assert!(
        page.body_html
            .contains("href=\"http://127.0.0.1:4502/tool/kernel#first-call\"")
    );
    let public = authored::resolve(&compiled, &corpus, Default::default()).unwrap();
    let page = public
        .project_page(
            &key("page:product"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    assert!(
        page.body_html
            .contains("href=\"https://two.test/entry/?q=one#answer\"")
    );
    assert!(
        page.body_html
            .contains("href=\"https://docs.example.test/tool/kernel#first-call\"")
    );
    assert!(!page.body_html.contains("127.0.0.1"));
}

#[test]
fn public_pages_refuse_loopback_hyperlinks_but_keep_code_examples() {
    let compiled = config("site", "");
    for target in [
        "http://localhost:4500/",
        "http://127.0.0.1:4500/",
        "http://[::1]:4500/",
        "//localhost:4500/",
    ] {
        let mut files = files();
        files[0].1.push_str(&format!("\n[Local]({target})\n"));
        let corpus = parse(&files);
        let resolved = authored::resolve(&compiled, &corpus, Default::default()).unwrap();
        assert!(
            resolved
                .project_page(
                    &key("page:product"),
                    &gazette_core::raster::unavailable,
                    &gazette_adapter_markdown::Comrak
                )
                .is_err(),
            "{target}"
        );
    }
    let mut files = files();
    files[0].1.push_str("\n`http://localhost:4500/`\n");
    let corpus = parse(&files);
    let resolved = authored::resolve(&compiled, &corpus, Default::default()).unwrap();
    assert!(
        resolved
            .project_page(
                &key("page:product"),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak
            )
            .unwrap()
            .body_html
            .contains("<code>http://localhost:4500/</code>")
    );
}

#[test]
fn editorial_root_selection_uses_collection_purpose_not_project_registration() {
    let mut files = files();
    files[0].1 = files[0].1.replace(
        "structure={kind='page',family={kind='project'}}",
        "structure={kind='collection'}",
    );
    files.push((
        "one/inquiry.md".into(),
        document(
            "collection:inquiry",
            "site:one",
            "/inquiry/",
            "project",
            "projects=[{project='project:one',kind='about'}]",
            "# Inquiry",
        )
        .replace(
            "structure={kind='page',family={kind='project'}}",
            "structure={kind='collection'}",
        ),
    ));
    for documentation in [false, true] {
        let mut selected_files = files.clone();
        if documentation {
            selected_files[0].1 = selected_files[0].1.replace(
                "kind='collection'",
                "kind='collection',purpose='documentation'",
            );
        }
        let mut text = configuration_text(
            "site",
            "resources={landing='page:product',learn='page:product'}",
        );
        let start = text.find("select={kind='project-articles'").unwrap();
        let end = text[start..].find("},order=").unwrap() + start + 1;
        text.replace_range(start..end, "select={kind='root-collections'}");
        let config = workspace::compile(&text, Default::default()).unwrap();
        let corpus = parse(&selected_files);
        let resolved = authored::resolve(&config, &corpus, Default::default()).unwrap();
        let pages = resolved
            .project_listing_pages(&key("site:one"), &resolved.discovery().unwrap())
            .unwrap();
        let items = &pages[0].items;
        assert!(
            items
                .iter()
                .any(|item| item.id == key("collection:inquiry"))
        );
        assert_eq!(
            items.iter().any(|item| item.id == key("page:product")),
            !documentation
        );
    }
}

#[test]
fn documentation_examples_keep_their_leaf_family_but_not_editorial_mentions_membership() {
    let config = config(
        "site",
        "resources={landing='page:product',learn='page:product'}",
    );
    let mut files = files();
    files[0].1 = files[0].1.replace(
        "structure={kind='page',family={kind='project'}}",
        "structure={kind='collection',purpose='documentation'}",
    );
    files[1].1 = files[1]
        .1
        .replace("family={kind='documentation'}", "family={kind='article'}")
        .replacen(
            "+++\n",
            "+++\ncollections=['page:product']\nprojects=[{project='project:one',kind='about'}]\n",
            1,
        );
    files.push((
        "one/essay.md".into(),
        document(
            "page:essay",
            "site:one",
            "/ordinary-writing/",
            "article",
            "projects=[{project='project:one',kind='about'}]",
            "# Essay",
        ),
    ));
    let corpus = parse(&files);
    let resolved = authored::resolve(&config, &corpus, Default::default()).unwrap();
    let discovery = resolved.discovery().unwrap();
    let listings = resolved
        .project_listing_pages(&key("site:one"), &discovery)
        .unwrap();
    for listing in listings {
        assert_eq!(listing.items.len(), 1);
        assert_eq!(listing.items[0].id, key("page:essay"));
    }
    let page = resolved
        .project_page(
            &key("page:learn"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    let searchable =
        serde_json::to_value(page.discovery_view(&discovery).unwrap().search().unwrap()).unwrap();
    assert_eq!(searchable["content"]["documentation"], true);
    assert_eq!(
        searchable["content"]["structure"]["family"]["kind"],
        "article"
    );
    assert_eq!(
        searchable["collections"][0]["structure"]["purpose"],
        "documentation"
    );
}

#[test]
fn project_landing_is_explicit_and_preserves_subject_relations_and_cross_site_facts() {
    for relation in ["about", "mentions"] {
        let config = config(
            "ecosystem",
            "resources={landing='page:product',learn='page:learn'}",
        );
        let mut files = files();
        files.push((
            "two/article.md".into(),
            document(
                "page:article",
                "site:two",
                "/article/",
                "article",
                &format!("projects=[{{project='project:one',kind='{relation}'}}]"),
                "# Reading",
            ),
        ));
        let corpus = parse(&files);
        let resolved = authored::resolve(&config, &corpus, Default::default()).unwrap();
        let page = resolved
            .project_page(
                &key("page:article"),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .unwrap();
        let model = serde_json::to_value(&page).unwrap();
        let member = &model["projects"][0];
        assert_eq!(member["relationship"]["relation"], relation);
        assert_eq!(member["landing"]["id"], "page:product");
        assert_eq!(member["landing"]["url"], "https://one.test/product/");
        assert_eq!(member["landing"]["destination"]["kind"], "cross-site");
        assert_eq!(member["resources"]["actions"][0]["kind"], "learn");
        let landing = page.data.projects[0].landing.as_ref().unwrap();
        assert_eq!(
            landing.title.as_ptr(),
            resolved
                .content()
                .graph()
                .node(&key("page:product"))
                .unwrap()
                .title
                .as_ptr()
        );
        let discovery = resolved.discovery().unwrap();
        let search =
            serde_json::to_value(page.discovery_view(&discovery).unwrap().search().unwrap())
                .unwrap();
        assert_eq!(search["projects"][0]["landing"], member["landing"]);
        let ld = serde_json::to_value(page.json_ld()).unwrap();
        assert!(ld.get(relation).is_some());
        assert!(
            ld.get(if relation == "about" {
                "mentions"
            } else {
                "about"
            })
            .is_none()
        );
        let preview = resolved
            .with_preview_urls(std::collections::BTreeMap::from([(
                key("site:one"),
                "http://127.0.0.1:4991/".parse().unwrap(),
            )]))
            .unwrap();
        let page = preview
            .project_page(
                &key("page:article"),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .unwrap();
        assert_eq!(
            page.data.projects[0].landing.as_ref().unwrap().url,
            "http://127.0.0.1:4991/product/"
        );
        assert_eq!(
            page.data.projects[0]
                .landing
                .as_ref()
                .unwrap()
                .destination
                .kind,
            gazette_core::markdown::LinkDestinationKind::CrossSite
        );
        let local = preview
            .project_page(
                &key("page:product"),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .unwrap();
        assert_eq!(
            local.data.projects[0]
                .landing
                .as_ref()
                .unwrap()
                .destination
                .kind,
            gazette_core::markdown::LinkDestinationKind::InternalNative
        );
    }
    let config = config(
        "ecosystem",
        "repository='https://code.example/repo'\nresources={learn='page:learn',mentions='page:blogs'}",
    );
    let files = files();
    let corpus = parse(&files);
    let resolved = authored::resolve(&config, &corpus, Default::default()).unwrap();
    let page = resolved
        .project_page(
            &key("page:product"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    assert!(
        page.data.projects[0].landing.is_none(),
        "manuals, mentions and repositories are never a landing fallback"
    );
}

#[test]
fn central_project_landing_replaces_only_the_matching_legacy_listing_selection() {
    let files = files();
    let corpus = parse(&files);
    let old = config("ecosystem", "");
    let central = configuration_text("ecosystem", "resources={landing='page:product'}")
        .replace(",landing='page:product'", "");
    let central = workspace::compile(&central, Default::default()).unwrap();
    let legacy = authored::resolve(&old, &corpus, Default::default()).unwrap();
    let resolved = authored::resolve(&central, &corpus, Default::default()).unwrap();
    let pages = resolved
        .project_listing_pages(&key("site:one"), &resolved.discovery().unwrap())
        .unwrap();
    let old_pages = legacy
        .project_listing_pages(&key("site:one"), &legacy.discovery().unwrap())
        .unwrap();
    assert_eq!(
        serde_json::to_value(&pages).unwrap(),
        serde_json::to_value(&old_pages).unwrap()
    );
    assert!(
        resolved
            .project_site_pages(&key("site:one"))
            .unwrap()
            .with_listing_pages(&pages)
            .is_ok()
    );
    let absent = configuration_text("ecosystem", "").replace(",landing='page:product'", "");
    let absent = workspace::compile(&absent, Default::default()).unwrap();
    let absent = authored::resolve(&absent, &corpus, Default::default()).unwrap();
    assert!(
        absent
            .project_listing_pages(&key("site:one"), &absent.discovery().unwrap())
            .is_err()
    );
    let conflict = config("ecosystem", "resources={landing='page:learn'}");
    let conflict = authored::resolve(&conflict, &corpus, Default::default()).unwrap();
    assert!(
        conflict
            .project_listing_pages(&key("site:one"), &conflict.discovery().unwrap())
            .is_err(),
        "conflicting declarations never pick a winner"
    );
}

#[test]
fn invalid_project_landings_fail_without_fallback_to_another_project_page() {
    for case in [
        "missing",
        "documentation",
        "mentions",
        "other-project",
        "unrouted",
        "copy",
    ] {
        let target = match case {
            "missing" => "page:missing",
            "documentation" => "page:learn",
            _ => "page:product",
        };
        let config_text =
            configuration_text("ecosystem", &format!("resources={{landing='{target}'}}"));
        let config_text = if case == "other-project" {
            format!(
                "{config_text}\n[[projects]]\nid='project:other'\nlifecycle='planned'\nvisibility='public'\navailability='unavailable'\n"
            )
        } else {
            config_text
        };
        let config = workspace::compile(&config_text, Default::default()).unwrap();
        let mut files = files();
        match case {
            "mentions" => files[0].1 = files[0].1.replace("kind='about'", "kind='mentions'"),
            "other-project" => {
                files[0].1 = files[0]
                    .1
                    .replace("project='project:one'", "project='project:other'")
            }
            "unrouted" => files[0].1 = files[0].1.replace("route={path='/product/'}\n", ""),
            "copy" => {
                files[0].1 = files[0].1.replace(
                    "path='/product/'",
                    "path='/product/',canonical='page:another'",
                );
                files.push((
                    "one/another.md".into(),
                    document(
                        "page:another",
                        "site:one",
                        "/another/",
                        "project",
                        "projects=[{project='project:one',kind='about'}]",
                        "",
                    ),
                ));
            }
            _ => {}
        }
        files.push((
            "two/article.md".into(),
            document(
                "page:article",
                "site:two",
                "/article/",
                "article",
                "projects=[{project='project:one',kind='mentions'}]",
                "# Reading",
            ),
        ));
        let corpus = parse(&files);
        let resolved = authored::resolve(&config, &corpus, Default::default()).unwrap();
        assert!(
            resolved
                .project_page(
                    &key("page:article"),
                    &gazette_core::raster::unavailable,
                    &gazette_adapter_markdown::Comrak
                )
                .is_err(),
            "{case}"
        );
    }
}
#[test]
fn independently_optional_resources_keep_order_and_exact_destination_facts() {
    for (declaration, expected) in [
        ("", vec![]),
        (
            "repository='https://code.example.test/specimen'",
            vec![Kind::Github],
        ),
        (
            "resources={github='https://code.example.test/team'}",
            vec![Kind::Github],
        ),
        ("resources={learn='page:learn'}", vec![Kind::Learn]),
        (
            "resources={blogs='page:blogs',github='https://code.example.test/team'}",
            vec![Kind::Github, Kind::Blogs],
        ),
        (
            "resources={thoughts='page:blogs',github='https://code.example.test/team'}",
            vec![Kind::Github, Kind::Thoughts],
        ),
        (
            "resources={mentions='page:blogs',github='https://code.example.test/team'}",
            vec![Kind::Github, Kind::Mentions],
        ),
        (
            "repository='https://code.example.test/specimen'\nresources={learn='page:learn',mentions='page:blogs',code_documentation='https://docs.example.test/specimen/'}",
            vec![
                Kind::CodeDocumentation,
                Kind::Learn,
                Kind::Github,
                Kind::Mentions,
            ],
        ),
        (
            "repository='https://code.example.test/specimen'\nresources={learn='page:learn',blogs='page:blogs',code_documentation='https://docs.example.test/specimen/'}",
            vec![
                Kind::CodeDocumentation,
                Kind::Learn,
                Kind::Github,
                Kind::Blogs,
            ],
        ),
    ] {
        let config = config("ecosystem", declaration);
        let files = files();
        let corpus = parse(&files);
        let resolved =
            authored::resolve(&config, &corpus, Default::default()).expect("native graph");
        let view = resolved
            .project_resources_for(&key("page:product"))
            .expect("resolved resources")
            .expect("one direct About project");
        assert_eq!(
            view.actions
                .iter()
                .map(|action| action.kind)
                .collect::<Vec<_>>(),
            expected
        );
        for action in view.actions {
            match action.kind {
                Kind::Learn => {
                    assert_eq!(action.url, "https://one.test/product/docs/");
                    assert_eq!(action.destination.content, Some(key("page:learn")));
                }
                Kind::Mentions | Kind::Blogs | Kind::Thoughts => {
                    assert_eq!(action.url, "https://one.test/product/blog/");
                    assert_eq!(action.destination.content, Some(key("page:blogs")));
                }
                _ => assert_eq!(
                    action.destination.kind,
                    gazette_core::markdown::LinkDestinationKind::External
                ),
            }
        }
        assert!(
            resolved
                .project_resources_for(&key("page:learn"))
                .expect("documentation projection")
                .is_none(),
            "documentation does not become a product-family host"
        );
    }
}
#[test]
fn bad_resource_destinations_and_conflicting_github_owners_fail() {
    assert!(workspace::compile("[[projects]]\nid='project:one'\nlifecycle='planned'\nvisibility='public'\navailability='unavailable'\nrepository='https://code.example.test/repo'\nresources={github='https://code.example.test/team'}",Default::default()).is_err());
    for url in [
        "http://docs.example.test/",
        "https://user:secret@docs.example.test/",
        "file:///private/manual",
    ] {
        let text = format!(
            "[[projects]]\nid='project:one'\nlifecycle='planned'\nvisibility='public'\navailability='unavailable'\nresources={{code_documentation={url:?}}}"
        );
        assert!(workspace::compile(&text, Default::default()).is_err());
    }
    for declaration in [
        "resources={blogs='page:learn'}",
        "resources={learn='page:missing'}",
    ] {
        let config = config("site", declaration);
        let files = files();
        let corpus = parse(&files);
        let resolved = authored::resolve(&config, &corpus, Default::default()).expect("graph");
        assert!(
            resolved
                .project_resources_for(&key("page:product"))
                .is_err(),
            "resource kind and native identity are validated"
        );
    }
    let config = config("site", "resources={learn='page:product'}");
    let files = files();
    let corpus = parse(&files);
    let resolved = authored::resolve(&config, &corpus, Default::default()).expect("graph");
    assert!(
        resolved
            .project_resources_for(&key("page:product"))
            .expect("self Learn is omitted")
            .unwrap()
            .actions
            .is_empty()
    );
}
#[test]
fn project_articles_include_direct_assertions_emitted_main_notes_and_foreign_links_only() {
    for (scope, foreign_expected) in [("site", false), ("ecosystem", true)] {
        let config = config(scope, "");
        let mut files = files();
        for (slug, edges, body) in [
            (
                "about",
                "projects=[{project='project:one',kind='about'}]",
                "# Direct",
            ),
            (
                "mentions",
                "projects=[{project='project:one',kind='mentions'}]",
                "# Mention",
            ),
            (
                "linked",
                "",
                "# Linked\n[product](https://one.test/product/)",
            ),
            (
                "unused",
                "",
                "# Not linked\nAn unrelated statement.\n\n[unused]: https://one.test/product/",
            ),
            ("unrelated", "", "# Unrelated"),
        ] {
            files.push((
                format!("one/{slug}.md"),
                document(
                    &format!("article:{slug}"),
                    "site:one",
                    &format!("/{slug}/"),
                    "article",
                    edges,
                    body,
                ),
            ));
        }
        files.push((
            "two/foreign.md".into(),
            document(
                "article:foreign",
                "site:two",
                "/foreign/",
                "article",
                "",
                "# Across\n[Product](https://one.test/product/)",
            ),
        ));
        files.push((
            "one/note/index.md".into(),
            document(
                "article:notes",
                "site:one",
                "/notes/",
                "article",
                "",
                "# Notes-owned reference",
            ),
        ));
        files.push((
            "one/note/notes.md".into(),
            "# A note\n\n[Product](https://one.test/product/)\n".into(),
        ));
        let corpus = parse(&files);
        let resolved =
            authored::resolve(&config, &corpus, Default::default()).expect("complete native graph");
        let pages = resolved
            .project_listing_pages(
                &key("site:one"),
                &resolved.discovery().expect("selected discovery"),
            )
            .expect("reference-based listing");
        assert_eq!(pages.len(), 2, "same members in two chronology views");
        for page in pages {
            let mut actual = page
                .items
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>();
            actual.sort_unstable();
            let mut expected = vec![
                "article:about",
                "article:mentions",
                "article:linked",
                "article:notes",
            ];
            if foreign_expected {
                expected.push("article:foreign");
            }
            expected.sort_unstable();
            assert_eq!(actual, expected);
            assert_eq!(
                page.subject.as_ref().expect("scope").landing.url,
                "https://one.test/product/"
            );
            if let Some(foreign) = page
                .items
                .iter()
                .find(|item| item.id == key("article:foreign"))
            {
                assert_eq!(foreign.site, key("site:two"));
                assert_eq!(
                    foreign.destination.kind,
                    gazette_core::markdown::LinkDestinationKind::CrossSite
                );
                assert_eq!(foreign.destination.site, Some(key("site:two")));
            }
        }
    }
}
#[test]
fn invalid_landing_subject_fails_without_partial_views() {
    let config = config("ecosystem", "");
    let mut files = files();
    files[0].1 = files[0].1.replace("kind='about'", "kind='mentions'");
    let corpus = parse(&files);
    let resolved = authored::resolve(&config, &corpus, Default::default()).expect("native graph");
    assert!(matches!(
        resolved.project_listing_pages(&key("site:one"), &resolved.discovery().expect("selection")),
        Err(ListingError::Binding { .. })
    ));
}

#[test]
fn inherited_subject_is_not_an_article_assertion_and_reference_work_is_bounded() {
    let config = config("ecosystem", "");
    let mut files = files();
    files.push(("one/collection.md".into(),"+++\nprojects=[{project='project:one',kind='context'}]\n[content]\nid='collection:inquiry'\nsite='site:one'\ntitle='Inquiry'\nlifecycle='published'\nroute={path='/inquiry/'}\nstructure={kind='collection'}\n+++\nA premise.\n".into()));
    files.push((
        "one/child.md".into(),
        document(
            "article:child",
            "site:one",
            "/child/",
            "article",
            "collections=['collection:inquiry']",
            "# Child\nNo direct subject or product reference.",
        ),
    ));
    let corpus = parse(&files);
    let resolved =
        authored::resolve(&config, &corpus, Default::default()).expect("inherited graph context");
    let pages = resolved
        .project_listing_pages(
            &key("site:one"),
            &resolved.discovery().expect("selected pages"),
        )
        .expect("empty subject view");
    assert!(
        pages
            .iter()
            .all(|page| page.total_items == 0 && page.items.is_empty()),
        "a container's context never manufactures this direct article selection"
    );
    let links = (0..100)
        .map(|number| format!("[Reference{number}](https://elsewhere.test/{number})\n"))
        .collect::<String>();
    files.push((
        "one/many.md".into(),
        document("article:many", "site:one", "/many/", "article", "", &links),
    ));
    let corpus = parse(&files);
    let limits = gazette_core::publication::content::Limits {
        edges: 24,
        ..Default::default()
    };
    let resolved =
        authored::resolve(&config, &corpus, limits).expect("small structural graph remains valid");
    assert!(
        matches!(
            resolved
                .project_listing_pages(&key("site:one"), &resolved.discovery().expect("selection")),
            Err(ListingError::Limit(_))
        ),
        "exhausted reference budget returns no partial pages"
    );
}

#[test]
fn complete_project_set_is_sorted_then_sliced_in_both_orders_without_browsing_ancestry() {
    let config = config("ecosystem", "");
    let mut files = files();
    for number in 1..=17 {
        let mut text = document(
            &format!("article:dated-{number:02}"),
            "site:one",
            &format!("/dated-{number}/"),
            "article",
            "projects=[{project='project:one',kind='mentions'}]",
            "# Dated article",
        );
        text = text.replace(
            "language='en'",
            &format!("language='en'\npublished='2026-01-{number:02}'"),
        );
        if number % 2 == 0 {
            text = text.replace("site='site:one'", "site='site:two'");
            files.push((format!("two/dated-{number}.md"), text));
        } else {
            files.push((format!("one/dated-{number}.md"), text));
        }
    }
    let corpus = parse(&files);
    let resolved = authored::resolve(&config, &corpus, Default::default()).expect("graph");
    let discovery = resolved.discovery().expect("selected corpus");
    let pages = resolved
        .project_listing_pages(&key("site:one"), &discovery)
        .expect("complete chronological pages");
    assert_eq!(pages.len(), 4);
    for page in &pages {
        let json = page.json_ld();
        assert_eq!(json["about"]["identifier"], "project:one");
        assert_eq!(json["relatedLink"], "https://one.test/product/");
    }

    for variant in [
        workspace::ListingVariant::Primary,
        workspace::ListingVariant::Alternate,
    ] {
        let slices = pages
            .iter()
            .filter(|page| page.variant == variant)
            .collect::<Vec<_>>();
        assert_eq!(
            slices
                .iter()
                .map(|page| page.items.len())
                .collect::<Vec<_>>(),
            [15, 2]
        );
        let numbers = slices
            .iter()
            .flat_map(|page| page.items.iter())
            .map(|item| {
                item.id
                    .as_str()
                    .strip_prefix("article:dated-")
                    .expect("known id")
                    .parse::<usize>()
                    .expect("ordinal")
            })
            .collect::<Vec<_>>();
        let expected = if variant == workspace::ListingVariant::Primary {
            (1..=17).rev().collect::<Vec<_>>()
        } else {
            (1..=17).collect()
        };
        assert_eq!(numbers, expected);
        assert_eq!(slices[0].next.as_deref(), Some(slices[1].url.as_str()));
        assert_eq!(slices[1].previous.as_deref(), Some(slices[0].url.as_str()));
        assert!(
            slices
                .iter()
                .all(|page| page.subject.as_ref().expect("subject").project == key("project:one"))
        );
    }
    assert!(
        pages
            .iter()
            .all(|page| page.location_for(&key("article:dated-01")).is_none()),
        "subject views do not become global article ancestry"
    );
}

#[test]
fn resource_json_ld_keeps_code_repository_separate_from_navigation_and_subject_pages() {
    for (declaration, repository) in [
        (
            "resources={learn='page:learn',blogs='page:blogs',github='https://code.example.test/team',code_documentation='https://docs.example.test/specimen/'}",
            false,
        ),
        (
            "repository='https://code.example.test/specimen'\nresources={learn='page:learn',blogs='page:blogs',code_documentation='https://docs.example.test/specimen/'}",
            true,
        ),
    ] {
        let config = config("ecosystem", declaration);
        let files = files();
        let corpus = parse(&files);
        let resolved = authored::resolve(&config, &corpus, Default::default()).expect("graph");
        let page = resolved
            .project_page(
                &key("page:product"),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .expect("page projection");
        let encoded = serde_json::to_string(&page.json_ld()).expect("same native metadata");
        assert_eq!(encoded.contains("codeRepository"), repository);
        assert!(
            encoded.contains("subjectOf")
                && encoded.contains("https://one.test/product/docs/")
                && encoded.contains("https://docs.example.test/specimen/")
                && encoded.contains("https://one.test/product/blog/")
        );
        if !repository {
            assert!(
                !encoded.contains("https://code.example.test/team"),
                "navigation is not repository or identity metadata"
            );
        }
    }
}

#[test]
fn selected_candidate_cannot_retain_a_resource_to_excluded_content() {
    use gazette_core::publication::content::Disposition;
    let config = config(
        "ecosystem",
        "resources={learn='page:learn',blogs='page:blogs'}",
    );
    let files = files();
    let corpus = parse(&files);
    let resolved = authored::resolve(&config, &corpus, Default::default()).expect("full graph");
    let selection = authored::Selection {
        content: [
            (key("page:product"), Disposition::Include),
            (key("page:blogs"), Disposition::Include),
            (key("page:learn"), Disposition::Exclude),
        ]
        .into_iter()
        .collect(),
        assets: Default::default(),
    };
    let selected = resolved
        .into_selected(&selection)
        .expect("exact selected graph without an invented body link");
    assert!(
        selected
            .project_resources_for(&key("page:product"))
            .is_err(),
        "an excluded target cannot survive in configured resource output"
    );
}

fn segmented_text() -> String {
    let source = configuration_text(
        "ecosystem",
        "repository='https://code.example.test/specimen'\nresources={learn='page:learn',blogs='page:blogs',thoughts='page:thoughts',code_documentation='https://docs.example.test/specimen/'}",
    );
    let source = source.replace(
        "scope='ecosystem'",
        "scope='ecosystem',residence='standalone',explore='page:all-blogs'",
    );
    let additional = ",{content='page:thoughts',select={kind='project-articles',project='project:one',landing='page:product',scope='ecosystem',residence='collected',explore='page:all-thoughts'},order={by='published',direction='descending',undated='last-title-then-id'},alternate_order={order={by='published',direction='ascending',undated='last-title-then-id'},path='oldest'},pagination={page_size=15,next_pages='page/{number}/'}},{content='page:all-blogs',select={kind='standalone-articles'},order={first=[],remaining='title-then-id'},pagination={page_size=15,next_pages='page/{number}/'}},{content='page:all-thoughts',select={kind='root-collections'},order={first=[],remaining='title-then-id'},pagination={page_size=15,next_pages='page/{number}/'}}";
    source.replace(
        "next_pages='page/{number}/'}}]",
        &format!("next_pages='page/{{number}}/'}}}}{additional}]"),
    )
}
fn segmented_files() -> Vec<(String, String)> {
    let mut files = files();
    for (id, path) in [
        ("page:thoughts", "/product/thoughts/"),
        ("page:all-blogs", "/blog/"),
        ("page:all-thoughts", "/thoughts/"),
    ] {
        files.push((
            format!("one/{}.md", id.split(':').next_back().expect("slug")),
            document(id, "site:one", path, "index", "", ""),
        ));
    }
    files.push(("one/inquiry.md".into(),"+++\n[content]\nid='collection:inquiry'\nsite='site:one'\ntitle='Inquiry'\nlifecycle='published'\nroute={path='/inquiry/'}\nstructure={kind='collection'}\n+++\nA collection.\n".into()));
    files
}
#[test]
fn standalone_and_collected_views_partition_the_editorial_graph_and_retain_native_exploration() {
    let config = workspace::compile(&segmented_text(), Default::default())
        .expect("separate residence and native exploration declarations");
    let mut files = segmented_files();
    for number in 1..=17 {
        for collected in [false, true] {
            let slug = if collected { "collected" } else { "standalone" };
            let memberships = if collected {
                "collections=['collection:inquiry']\n"
            } else {
                ""
            };
            let site = if number % 2 == 0 {
                "site:two"
            } else {
                "site:one"
            };
            let prefix = if number % 2 == 0 { "two" } else { "one" };
            // Collection membership, not these misleading paths, supplies residence.
            let logical = if collected {
                "standalone-path"
            } else {
                "collected-path"
            };
            files.push((
                format!("{prefix}/{logical}-{number}.md"),
                document(
                    &format!("article:{slug}-{number:02}"),
                    site,
                    &format!("/{slug}-{number}/"),
                    "article",
                    &format!("{memberships}projects=[{{project='project:one',kind='mentions'}}]"),
                    "# An article",
                ),
            ));
        }
    }
    files.push(("one/journey.md".into(),"+++\n[content]\nid='journey:one'\nsite='site:one'\ntitle='Journey'\nlifecycle='published'\nroute={path='/journey/'}\nstructure={kind='journey',collection='collection:inquiry',members=[{content='article:journey',primary=true}]}\n+++\nA reading sequence.\n".into()));
    files.push((
        "one/journey-article.md".into(),
        document(
            "article:journey",
            "site:one",
            "/journey-article/",
            "article",
            "projects=[{project='project:one',kind='about'}]",
            "# Journey article",
        ),
    ));
    // Foreign articles need a collection in their own publication.
    files.push(("two/inquiry.md".into(),"+++\n[content]\nid='collection:foreign'\nsite='site:two'\ntitle='Foreign inquiry'\nlifecycle='published'\nroute={path='/foreign-inquiry/'}\nstructure={kind='collection'}\n+++\nA foreign collection.\n".into()));
    for (path, text) in &mut files {
        if path.starts_with("two/") {
            *text = text.replace(
                "collections=['collection:inquiry']",
                "collections=['collection:foreign']",
            );
        }
    }
    let corpus = parse(&files);
    let resolved =
        authored::resolve(&config, &corpus, Default::default()).expect("native editorial graph");
    let discovery = resolved.discovery().expect("eligible set");
    let pages = resolved
        .project_listing_pages(&key("site:one"), &discovery)
        .expect("complete partitions");
    let resources = resolved
        .project_resources_for(&key("page:product"))
        .expect("resource destinations")
        .expect("one product");
    assert_eq!(
        resources
            .actions
            .iter()
            .map(|action| action.kind)
            .collect::<Vec<_>>(),
        [
            Kind::CodeDocumentation,
            Kind::Learn,
            Kind::Github,
            Kind::Blogs,
            Kind::Thoughts
        ]
    );
    for (id, count, residence, explore) in [
        (
            "page:blogs",
            17,
            workspace::ListingProjectResidence::Standalone,
            "page:all-blogs",
        ),
        (
            "page:thoughts",
            18,
            workspace::ListingProjectResidence::Collected,
            "page:all-thoughts",
        ),
    ] {
        for variant in [
            workspace::ListingVariant::Primary,
            workspace::ListingVariant::Alternate,
        ] {
            let views = pages
                .iter()
                .filter(|page| page.content == key(id) && page.variant == variant)
                .collect::<Vec<_>>();
            assert_eq!(views.len(), 2);
            assert_eq!(views[0].items.len(), 15);
            assert_eq!(views[1].items.len(), count - 15);
            let subject = views[1].subject.as_ref().expect("continuation subject");
            assert_eq!(subject.residence, residence);
            let target = subject.explore.as_ref().expect("native exploration");
            assert_eq!(target.id, key(explore));
            assert_eq!(
                target.destination.kind,
                gazette_core::markdown::LinkDestinationKind::InternalNative
            );
            assert_eq!(views[0].total_items, count);
            for item in views.iter().flat_map(|page| page.items.iter()) {
                assert_eq!(
                    resolved
                        .content()
                        .graph()
                        .editorial_parent(&item.id)
                        .is_some(),
                    residence == workspace::ListingProjectResidence::Collected
                );
            }
            assert_eq!(views[1].json_ld()["relatedLink"][1], target.url);
        }
    }
}
#[test]
fn empty_subject_keeps_an_explicit_fallback_and_invalid_exploration_refuses() {
    let source = segmented_text();
    let files = segmented_files();
    let corpus = parse(&files);
    let config = workspace::compile(&source, Default::default()).expect("valid fallback");
    let resolved = authored::resolve(&config, &corpus, Default::default()).expect("graph");
    let pages = resolved
        .project_listing_pages(&key("site:one"), &resolved.discovery().expect("selection"))
        .expect("empty subject views");
    assert!(
        pages
            .iter()
            .filter(|page| page.subject.is_some())
            .all(|page| page.total_items == 0
                && page.subject.as_ref().expect("subject").explore.is_some()),
        "empty selections retain the authored exploration destination"
    );
    for bad in ["page:missing", "page:product", "page:blogs"] {
        let altered = source.replace("explore='page:all-blogs'", &format!("explore='{bad}'"));
        let config = workspace::compile(&altered, Default::default())
            .expect("keys are resolved against the corpus");
        let resolved = authored::resolve(&config, &corpus, Default::default()).expect("graph");
        assert!(
            resolved
                .project_listing_pages(&key("site:one"), &resolved.discovery().expect("selection"))
                .is_err(),
            "missing, non-index or already filtered fallback refused"
        );
    }
}

#[test]
fn combined_mentions_merge_all_editorial_residences_without_duplicate_members_or_resource_buttons()
{
    let source=configuration_text("ecosystem","repository='https://code.example.test/specimen'\nresources={code_documentation='https://docs.example.test/specimen/',learn='page:learn',mentions='page:blogs'}").replace("scope='ecosystem'","scope='ecosystem',residence='all'");
    let config =
        workspace::compile(&source, Default::default()).expect("one combined subject view");
    let mut files = files();
    files.push(("one/inquiry.md".into(),"+++\n[content]\nid='collection:inquiry'\nsite='site:one'\ntitle='Inquiry'\nlifecycle='published'\nroute={path='/inquiry/'}\nstructure={kind='collection'}\n+++\nA collection.\n".into()));
    files.push(("two/inquiry.md".into(),"+++\n[content]\nid='collection:foreign'\nsite='site:two'\ntitle='Foreign inquiry'\nlifecycle='published'\nroute={path='/inquiry/'}\nstructure={kind='collection'}\n+++\nAnother collection.\n".into()));
    for number in 1..=17 {
        for collected in [false, true] {
            let (site, prefix, parent) = if number % 2 == 0 {
                ("site:two", "two", "collection:foreign")
            } else {
                ("site:one", "one", "collection:inquiry")
            };
            let slug = if collected { "collected" } else { "standalone" };
            let mut edges = "projects=[{project='project:one',kind='mentions'}]\n".to_owned();
            if collected {
                edges += &format!("collections=['{parent}']\n");
            }
            // Both explicit mentions and a body reference must still yield one item.
            files.push((
                format!("{prefix}/{slug}-{number}.md"),
                document(
                    &format!("article:{slug}-{number:02}"),
                    site,
                    &format!("/{slug}-{number}/"),
                    "article",
                    &edges,
                    "# Writing\n[Product](https://one.test/product/)",
                ),
            ));
        }
    }
    let corpus = parse(&files);
    let resolved = authored::resolve(&config, &corpus, Default::default()).expect("complete graph");
    let pages = resolved
        .project_listing_pages(
            &key("site:one"),
            &resolved.discovery().expect("eligible set"),
        )
        .expect("combined native pagination");
    for variant in [
        workspace::ListingVariant::Primary,
        workspace::ListingVariant::Alternate,
    ] {
        let views = pages
            .iter()
            .filter(|page| page.variant == variant)
            .collect::<Vec<_>>();
        assert_eq!(
            views
                .iter()
                .map(|page| page.items.len())
                .collect::<Vec<_>>(),
            [15, 15, 4]
        );
        let ids = views
            .iter()
            .flat_map(|page| page.items.iter().map(|item| &item.id))
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(ids.len(), 34);
        assert!(views.iter().all(|page| page.total_items == 34
            && page.subject.as_ref().expect("subject").residence
                == workspace::ListingProjectResidence::All));
    }
    let resources = resolved
        .project_resources_for(&key("page:product"))
        .expect("resource projection")
        .expect("one product");
    assert_eq!(
        resources
            .actions
            .iter()
            .map(|action| action.kind)
            .collect::<Vec<_>>(),
        [
            Kind::CodeDocumentation,
            Kind::Learn,
            Kind::Github,
            Kind::Mentions
        ]
    );
    assert_eq!(
        resources.actions[3].destination.content,
        Some(key("page:blogs")),
        "configured identity, not a route name, supplies Mentions"
    );
}
