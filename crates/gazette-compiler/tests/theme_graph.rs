//! Theme-package graph resolution is exact, deterministic, and cycle-safe.

#[path = "theme_graph/initial_color_mode.rs"]
mod initial_color_mode;
#[path = "theme_graph/social_card.rs"]
mod social_card;

use gazette_compiler::{
    theme_graph::{EffectiveTheme, Error, Graph},
    theme_package::{ParsedManifest, ThemePackageId},
    toml,
};

const FOUNDATION: &str =
    "package='theme:foundation'\nnamespace='foundation'\ntemplates='templates'\n";
const ARTICLE: &str = "package='theme:article'\nnamespace='article'\ntemplates='templates'\n[[dependencies]]\npackage='theme:foundation'\n";
const SEARCH: &str = "package='theme:search'\nnamespace='search'\ntemplates='templates'\n[[dependencies]]\npackage='theme:foundation'\n";
const ROOT: &str = "package='theme:root'\nnamespace='root'\ntemplates='templates'\n[[dependencies]]\npackage='theme:search'\n[[dependencies]]\npackage='theme:article'\n";

fn parse(source: &str) -> ParsedManifest<'_> {
    ParsedManifest::parse(source, toml::Limits::default()).expect("valid Theme package")
}

fn id(value: &str) -> ThemePackageId {
    value.parse().expect("valid package identity")
}

fn identities<'a>(graph: &Graph<'a>) -> Vec<&'a str> {
    graph
        .packages()
        .iter()
        .map(|manifest| manifest.package.as_str())
        .collect()
}

#[test]
fn resolves_one_exact_root_in_dependency_before_dependent_order() {
    let foundation = parse(FOUNDATION);
    let article = parse(ARTICLE);
    let search = parse(SEARCH);
    let root = parse(ROOT);

    let graph = Graph::resolve(
        &id("theme:root"),
        [
            search.manifest(),
            root.manifest(),
            article.manifest(),
            foundation.manifest(),
        ],
    )
    .expect("complete package graph");

    assert_eq!(graph.root().package.as_str(), "theme:root");
    assert_eq!(
        identities(&graph),
        [
            "theme:foundation",
            "theme:article",
            "theme:search",
            "theme:root"
        ]
    );
}

#[test]
fn manifest_input_order_does_not_change_the_resolved_graph() {
    let foundation = parse(FOUNDATION);
    let article = parse(ARTICLE);
    let search = parse(SEARCH);
    let root = parse(ROOT);
    let root_id = id("theme:root");

    let first = Graph::resolve(
        &root_id,
        [
            root.manifest(),
            article.manifest(),
            foundation.manifest(),
            search.manifest(),
        ],
    )
    .expect("first ordering");
    let second = Graph::resolve(
        &root_id,
        [
            search.manifest(),
            foundation.manifest(),
            root.manifest(),
            article.manifest(),
        ],
    )
    .expect("second ordering");

    assert_eq!(identities(&first), identities(&second));
}

#[test]
fn missing_root_and_dependency_are_distinct_exact_failures() {
    let foundation = parse(FOUNDATION);
    let root = parse(ROOT);

    let missing_root = Graph::resolve(&id("theme:missing"), [foundation.manifest()])
        .expect_err("selected root is absent");
    assert_eq!(missing_root.code(), "theme_package_missing");
    assert_eq!(
        missing_root,
        Error::MissingPackage {
            package: id("theme:missing"),
            required_by: None,
        }
    );

    let missing_dependency =
        Graph::resolve(&id("theme:root"), [root.manifest(), foundation.manifest()])
            .expect_err("selected closure is incomplete");
    assert_eq!(missing_dependency.code(), "theme_package_missing");
    assert_eq!(
        missing_dependency,
        Error::MissingPackage {
            package: id("theme:article"),
            required_by: Some(id("theme:root")),
        }
    );
}

#[test]
fn duplicate_package_identity_is_rejected_before_traversal() {
    let first = parse(FOUNDATION);
    let duplicate = parse(FOUNDATION);

    let error = Graph::resolve(
        &id("theme:foundation"),
        [first.manifest(), duplicate.manifest()],
    )
    .expect_err("duplicate identity");
    assert_eq!(error.code(), "theme_package_duplicate");
    assert_eq!(
        error,
        Error::DuplicatePackage {
            package: id("theme:foundation"),
        }
    );
}

#[test]
fn dependency_cycle_reports_one_deterministic_closed_path() {
    let alpha = parse(
        "package='theme:alpha'\nnamespace='alpha'\ntemplates='templates'\n[[dependencies]]\npackage='theme:beta'\n",
    );
    let beta = parse(
        "package='theme:beta'\nnamespace='beta'\ntemplates='templates'\n[[dependencies]]\npackage='theme:gamma'\n",
    );
    let gamma = parse(
        "package='theme:gamma'\nnamespace='gamma'\ntemplates='templates'\n[[dependencies]]\npackage='theme:alpha'\n",
    );

    let error = Graph::resolve(
        &id("theme:alpha"),
        [gamma.manifest(), alpha.manifest(), beta.manifest()],
    )
    .expect_err("cycle is rejected");
    assert_eq!(error.code(), "theme_package_cycle");
    assert_eq!(
        error,
        Error::Cycle {
            packages: [
                id("theme:alpha"),
                id("theme:beta"),
                id("theme:gamma"),
                id("theme:alpha"),
            ]
            .into(),
        }
    );
}

#[test]
fn packages_outside_the_selected_closure_do_not_enter_the_graph() {
    let foundation = parse(FOUNDATION);
    let article = parse(ARTICLE);
    let search = parse(SEARCH);

    let graph = Graph::resolve(
        &id("theme:article"),
        [search.manifest(), article.manifest(), foundation.manifest()],
    )
    .expect("selected closure");

    assert_eq!(identities(&graph), ["theme:foundation", "theme:article"]);
}

#[test]
fn effective_theme_qualifies_templates_and_aggregates_capability_needs() {
    let foundation = parse(
        "package='theme:foundation'\nnamespace='foundation'\ntemplates='templates'\nresource_requirements=['stylesheet:tokens','module:shared']\n[components.callout]\ntemplate='components/callout.html'\n[capabilities]\noptional=['publication.search/v1']\n",
    );
    let root = parse(
        "package='theme:root'\nnamespace='publication'\ntemplates='templates'\nrender={page='page.html',alias='alias.html'}\nresource_requirements=['module:shared','stylesheet:article']\n[components.hero]\ntemplate='components/hero.html'\n[capabilities]\nrequired=['publication.search/v1']\noptional=['commerce.selector/v1']\n[[dependencies]]\npackage='theme:foundation'\n",
    );
    let graph =
        Graph::resolve(&id("theme:root"), [root.manifest(), foundation.manifest()]).unwrap();
    let effective = EffectiveTheme::resolve(&graph).expect("effective Theme");
    let reversed =
        Graph::resolve(&id("theme:root"), [foundation.manifest(), root.manifest()]).unwrap();
    let reversed = EffectiveTheme::resolve(&reversed).expect("reordered effective Theme");

    assert_eq!(effective.root().as_str(), "theme:root");
    assert_eq!(effective.render().page.as_str(), "publication/page.html");
    assert_eq!(
        effective.render().alias.as_ref().unwrap().as_str(),
        "publication/alias.html"
    );
    assert_eq!(
        effective
            .resource_requirements()
            .iter()
            .map(|resource| resource.as_str())
            .collect::<Vec<_>>(),
        ["stylesheet:tokens", "module:shared", "stylesheet:article"]
    );
    assert_eq!(
        effective.resource_requirements(),
        reversed.resource_requirements(),
        "manifest input order cannot change dependency-first logical requirements"
    );
    assert_eq!(
        effective
            .components()
            .definition(&"callout".parse().unwrap())
            .unwrap()
            .template
            .as_str(),
        "foundation/components/callout.html"
    );
    assert_eq!(
        effective
            .components()
            .definition(&"hero".parse().unwrap())
            .unwrap()
            .template
            .as_str(),
        "publication/components/hero.html"
    );
    assert_eq!(
        effective
            .required_capabilities()
            .iter()
            .map(|capability| capability.as_str())
            .collect::<Vec<_>>(),
        ["publication.search/v1"]
    );
    assert_eq!(
        effective
            .optional_capabilities()
            .iter()
            .map(|capability| capability.as_str())
            .collect::<Vec<_>>(),
        ["commerce.selector/v1"]
    );
}

#[test]
fn effective_theme_rejects_missing_root_render_and_component_collisions() {
    let root = parse(FOUNDATION);
    let graph = Graph::resolve(&id("theme:foundation"), [root.manifest()]).unwrap();
    assert_eq!(
        EffectiveTheme::resolve(&graph).unwrap_err().code(),
        "theme_package_render"
    );

    let dependency = parse(
        "package='theme:foundation'\nnamespace='foundation'\ntemplates='templates'\n[components.callout]\ntemplate='callout.html'\n",
    );
    let root = parse(
        "package='theme:root'\nnamespace='root'\ntemplates='templates'\nrender={page='page.html'}\n[components.callout]\ntemplate='callout.html'\n[[dependencies]]\npackage='theme:foundation'\n",
    );
    let graph =
        Graph::resolve(&id("theme:root"), [root.manifest(), dependency.manifest()]).unwrap();
    assert_eq!(
        EffectiveTheme::resolve(&graph).unwrap_err().code(),
        "theme_package_component_collision"
    );
}

#[test]
fn machine_entry_inherits_from_explicit_dependency_and_root_can_override() {
    let base = parse(
        "package='theme:base'\nnamespace='base'\ntemplates='templates'\nrender={page='page.html',llms='llms.txt'}\n",
    );
    let middle = parse(
        "package='theme:middle'\nnamespace='middle'\ntemplates='templates'\n[[dependencies]]\npackage='theme:base'\n",
    );
    for own in [false, true] {
        let root = parse(if own {
            "package='theme:root'\nnamespace='root'\ntemplates='templates'\nrender={page='page.html',llms='machine.txt'}\n[[dependencies]]\npackage='theme:middle'\n"
        } else {
            "package='theme:root'\nnamespace='root'\ntemplates='templates'\nrender={page='page.html'}\n[[dependencies]]\npackage='theme:middle'\n"
        });
        let graph = Graph::resolve(
            &id("theme:root"),
            [root.manifest(), middle.manifest(), base.manifest()],
        )
        .unwrap();
        let effective = EffectiveTheme::resolve(&graph).unwrap();
        assert_eq!(
            effective.render().llms.as_ref().unwrap().as_str(),
            if own {
                "root/machine.txt"
            } else {
                "base/llms.txt"
            }
        );
        assert!(
            effective
                .render()
                .entries()
                .any(|entry| entry == effective.render().llms.as_ref().unwrap())
        );
    }
}

#[test]
fn competing_inherited_machine_entries_fail_instead_of_choosing_traversal_order() {
    let first = parse(
        "package='theme:first'\nnamespace='first'\ntemplates='templates'\nrender={page='page.html',llms='llms.txt'}\n",
    );
    let second = parse(
        "package='theme:second'\nnamespace='second'\ntemplates='templates'\nrender={page='page.html',llms='llms.txt'}\n",
    );
    let root = parse(
        "package='theme:root'\nnamespace='root'\ntemplates='templates'\nrender={page='page.html'}\n[[dependencies]]\npackage='theme:first'\n[[dependencies]]\npackage='theme:second'\n",
    );
    let graph = Graph::resolve(
        &id("theme:root"),
        [root.manifest(), second.manifest(), first.manifest()],
    )
    .unwrap();
    assert_eq!(
        EffectiveTheme::resolve(&graph).unwrap_err().code(),
        "theme_package_render_collision"
    );
}

#[test]
fn page_kind_and_mode_entries_inherit_with_explicit_overrides_and_conflict_checks() {
    use gazette_compiler::theme::PageKind;
    use gazette_core::publication::content::{ContentKind, PageFamily};
    let base = parse(
        "package='theme:base'\nnamespace='base'\ntemplates='templates'\nrender={page='page.html',home='home.html',collection_directory='directory.html',kinds={article='reading.html',collection='index.html'},modes={'mode:home'='home.html'}}\n",
    );
    let other = parse(
        "package='theme:other'\nnamespace='other'\ntemplates='templates'\nrender={page='page.html',kinds={article='article.html'}}\n",
    );
    for override_article in [false, true] {
        let source = format!(
            "package='theme:root'\nnamespace='root'\ntemplates='templates'\nrender={{page='page.html'{}}}\n[[dependencies]]\npackage='theme:base'\n[[dependencies]]\npackage='theme:other'\n",
            if override_article {
                ",kinds={article='own.html'}"
            } else {
                ""
            }
        );
        let root = parse(&source);
        let graph = Graph::resolve(
            &id("theme:root"),
            [base.manifest(), other.manifest(), root.manifest()],
        )
        .unwrap();
        if !override_article {
            assert_eq!(
                EffectiveTheme::resolve(&graph).unwrap_err().code(),
                "theme_package_render_collision"
            );
            continue;
        }
        let effective = EffectiveTheme::resolve(&graph).unwrap();
        let recipes = effective.render();
        assert_eq!(recipes.home.as_ref().unwrap().as_str(), "base/home.html");
        assert_eq!(
            recipes.collection_directory.as_ref().unwrap().as_str(),
            "base/directory.html"
        );
        assert_eq!(
            recipes.kinds[&PageKind::Collection].as_str(),
            "base/index.html"
        );
        assert_eq!(
            recipes
                .page_for(&ContentKind::Page {
                    family: PageFamily::Article
                })
                .as_str(),
            "root/own.html"
        );
        assert_eq!(
            recipes
                .page_for(&ContentKind::Page {
                    family: PageFamily::Utility
                })
                .as_str(),
            "root/page.html"
        );
        let structure: ContentKind = serde_json::from_value(serde_json::json!({"kind":"page","family":{"kind":"extension","schema":"schema:one","data":{},"presentation":{"mode":"mode:home","amplitude":0}}})).unwrap();
        assert_eq!(recipes.page_for(&structure).as_str(), "base/home.html");
        assert!(
            recipes
                .entries()
                .any(|entry| entry.as_str() == "base/home.html")
        );
    }
    assert!(ParsedManifest::parse("package='theme:bad'\nnamespace='bad'\ntemplates='templates'\nrender={page='page.html',kinds={made_up='other.html'}}\n", Default::default()).is_err());
}

#[test]
fn resource_defaults_follow_diamonds_and_require_an_explicit_sibling_override() {
    let resource = "[[resources]]\nid='style:shared'\ninputs=['style.css']\nbuild={kind='stylesheet',entry='style.css'}\noutput='/shared.css'\n";
    let foundation_text = format!("{FOUNDATION}{resource}");
    let foundation = parse(&foundation_text);
    let article = parse(ARTICLE);
    let search = parse(SEARCH);
    let root_text = format!("render={{page='page.html'}}\n{ROOT}");
    let root = parse(&root_text);
    let resolve = |article: &ParsedManifest<'_>, root: &ParsedManifest<'_>| {
        EffectiveTheme::resolve(
            &Graph::resolve(
                &id("theme:root"),
                [
                    root.manifest(),
                    article.manifest(),
                    search.manifest(),
                    foundation.manifest(),
                ],
            )
            .unwrap(),
        )
    };
    let effective = resolve(&article, &root).expect("diamond shares one declaration");
    let shared = &effective.resources()[&"style:shared".parse().unwrap()];
    assert_eq!(shared.package, id("theme:foundation"));
    assert_eq!(shared.output.as_str(), "/shared.css");
    assert_eq!(shared.use_, gazette_compiler::workspace::ResourceUse::Site);

    let article_text = format!(
        "{ARTICLE}{}",
        resource.replace("/shared.css", "/article.css")
    );
    let article = parse(&article_text);
    assert!(
        matches!(
            resolve(&article, &root),
            Err(Error::ResourceCollision { .. })
        ),
        "one branch override must not silently replace the other branch's declaration"
    );
    let root_text = format!(
        "{root_text}{}",
        resource.replace("/shared.css", "/root.css")
    );
    let root = parse(&root_text);
    let effective = resolve(&article, &root).expect("dependent makes an explicit choice");
    assert_eq!(
        effective.resources()[&"style:shared".parse().unwrap()].package,
        id("theme:root")
    );
}

#[test]
fn copy_defaults_deduplicate_diamonds_and_require_an_explicit_branch_choice() {
    let foundation_text = format!(
        "{FOUNDATION}[chrome]\nnavigation_label='Foundation'\n[chrome.regions]\n'base:empty'='Nothing here'\n"
    );
    let foundation = parse(&foundation_text);
    let article = parse(ARTICLE);
    let search = parse(SEARCH);
    let root_text = format!("render={{page='page.html'}}\n{ROOT}");
    let root = parse(&root_text);
    let resolve = |article: &ParsedManifest<'_>, root: &ParsedManifest<'_>| {
        EffectiveTheme::resolve(
            &Graph::resolve(
                &id("theme:root"),
                [
                    root.manifest(),
                    article.manifest(),
                    search.manifest(),
                    foundation.manifest(),
                ],
            )
            .unwrap(),
        )
    };
    let effective = resolve(&article, &root).expect("one declaration through two paths");
    assert_eq!(
        effective.labels()["navigation_label"].package,
        id("theme:foundation")
    );
    assert_eq!(
        effective.labels()["regions.base:empty"].value,
        "Nothing here"
    );
    let article_text = format!("{ARTICLE}[chrome]\nnavigation_label='Article'\n");
    let article = parse(&article_text);
    assert!(
        matches!(resolve(&article, &root), Err(Error::CopyCollision { field, .. }) if field == "navigation_label")
    );
    let root_text = format!("{root_text}[chrome]\nnavigation_label='Chosen by root'\n");
    let root = parse(&root_text);
    let effective = resolve(&article, &root).expect("dependent chooses the conflicting label");
    assert_eq!(
        effective.labels()["navigation_label"].package,
        id("theme:root")
    );
    assert_eq!(
        effective.labels()["navigation_label"].value,
        "Chosen by root"
    );
    assert_eq!(
        effective.labels()["regions.base:empty"].package,
        id("theme:foundation")
    );
}
