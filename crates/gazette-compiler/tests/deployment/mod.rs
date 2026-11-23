//! Publisher defaults and site overrides own crawler policy, without crawler taxonomy.

const CONFIG: &str = r#"
[[principals]]
id='owner:one'
kind='person'
[[sites]]
id='site:one'
publisher='owner:one'
deployable=false
[[brands]]
id='brand:one'
name='One'
[[brands.publications]]
site='site:one'
public_base_url='https://one.example/'
machine={robots={enabled=false},web_manifest={short_name='One'}}
[defaults.publication]
machine={robots={enabled=true,rules=[{user_agent='*',allow=['/']},{user_agent='TrainingBot',disallow=['/']}]},web_manifest={enabled=true}}
"#;

#[test]
fn crawler_policy_and_manifest_inherit_without_overwriting_site_choices() {
    let compilation = gazette_compiler::workspace::compile(CONFIG, Default::default()).unwrap();
    let publication = compilation
        .workspace
        .publication_for(&"site:one".parse().unwrap())
        .unwrap();
    let robots = publication.machine.robots.as_ref().unwrap();
    assert!(!robots.enabled);
    assert_eq!(robots.rules.len(), 2);
    assert_eq!(robots.rules[1].disallow, ["/"]);
    let manifest = publication.machine.web_manifest.as_ref().unwrap();
    assert!(manifest.enabled);
    assert_eq!(manifest.short_name.as_deref(), Some("One"));
    let replacement = CONFIG.replace(
        "robots={enabled=false}",
        "robots={enabled=true,rules=[{user_agent='SpecificBot',disallow=['/private/']}]}",
    );
    let compilation =
        gazette_compiler::workspace::compile(&replacement, Default::default()).unwrap();
    let publication = compilation
        .workspace
        .publication_for(&"site:one".parse().unwrap())
        .unwrap();
    assert_eq!(publication.machine.robots.as_ref().unwrap().rules.len(), 1);
}

#[test]
fn malformed_crawler_groups_are_rejected_before_output() {
    for rules in [
        "[]",
        "[{user_agent='*'},{user_agent='*'}]",
        "[{user_agent='bad token'}]",
        "[{user_agent='Bot',disallow=['relative']}]",
        "[{user_agent='Bot',allow=['/#comment']}]",
    ] {
        let source = CONFIG.replace(
            "robots={enabled=false}",
            &format!("robots={{enabled=true,rules={rules}}}"),
        );
        assert!(
            gazette_compiler::workspace::compile(&source, Default::default()).is_err(),
            "{rules}"
        );
    }
}

#[test]
fn missing_page_recipe_inherits_and_conflicting_parents_fail() {
    use gazette_compiler::{
        theme_graph::{EffectiveTheme, Graph},
        theme_package::ParsedManifest,
    };
    let base = ParsedManifest::parse("package='theme:base'\nnamespace='base'\ntemplates='templates'\nrender={page='page.html',not_found='missing.html'}\n", Default::default()).unwrap();
    let other = ParsedManifest::parse("package='theme:other'\nnamespace='other'\ntemplates='templates'\nrender={page='page.html',not_found='missing.html'}\n", Default::default()).unwrap();
    let id = "theme:root".parse().unwrap();
    for own in [false, true] {
        let source = format!(
            "package='theme:root'\nnamespace='root'\ntemplates='templates'\nrender={{page='page.html'{}}}\n[[dependencies]]\npackage='theme:base'\n",
            if own { ",not_found='own.html'" } else { "" }
        );
        let root = ParsedManifest::parse(&source, Default::default()).unwrap();
        let graph = Graph::resolve(&id, [root.manifest(), base.manifest()]).unwrap();
        let theme = EffectiveTheme::resolve(&graph).unwrap();
        assert_eq!(
            theme.render().not_found.as_ref().unwrap().as_str(),
            if own {
                "root/own.html"
            } else {
                "base/missing.html"
            }
        );
        assert!(
            theme
                .render()
                .entries()
                .any(|entry| Some(entry) == theme.render().not_found.as_ref())
        );
    }
    let root = ParsedManifest::parse("package='theme:root'\nnamespace='root'\ntemplates='templates'\nrender={page='page.html'}\n[[dependencies]]\npackage='theme:base'\n[[dependencies]]\npackage='theme:other'\n", Default::default()).unwrap();
    let graph = Graph::resolve(&id, [root.manifest(), base.manifest(), other.manifest()]).unwrap();
    assert_eq!(
        EffectiveTheme::resolve(&graph).unwrap_err().code(),
        "theme_package_render_collision"
    );
}

#[test]
fn terms_inherit_and_resolve_beneath_the_production_base() {
    let source = CONFIG.replace(
        "web_manifest={enabled=true}}",
        "web_manifest={enabled=true},terms={path='/terms/',tdm_reservation=true}}",
    );
    let compilation = gazette_compiler::workspace::compile(&source, Default::default()).unwrap();
    let publication = compilation
        .workspace
        .publication_for(&"site:one".parse().unwrap())
        .unwrap();
    let terms = publication.machine.terms.as_ref().unwrap();
    assert!(terms.tdm_reservation);
    assert_eq!(
        publication.terms_url().as_deref(),
        Some("https://one.example/terms/")
    );
    // A site keeps the inherited path while overriding only the reservation.
    let source = source.replace(
        "web_manifest={short_name='One'}}",
        "web_manifest={short_name='One'},terms={tdm_reservation=false}}",
    );
    let compilation = gazette_compiler::workspace::compile(&source, Default::default()).unwrap();
    let publication = compilation
        .workspace
        .publication_for(&"site:one".parse().unwrap())
        .unwrap();
    let terms = publication.machine.terms.as_ref().unwrap();
    assert!(!terms.tdm_reservation);
    assert_eq!(terms.path, "/terms/");
    let omitted = gazette_compiler::workspace::compile(CONFIG, Default::default()).unwrap();
    let publication = omitted
        .workspace
        .publication_for(&"site:one".parse().unwrap())
        .unwrap();
    assert!(publication.machine.terms.is_none());
    assert!(publication.terms_url().is_none());
}

#[test]
fn malformed_terms_paths_are_rejected_before_output() {
    let long = format!("/{}", "a".repeat(1024));
    for path in [
        "terms/",
        "",
        "/terms//",
        "/terms page/",
        "/terms/#top",
        "/terms/?edition=1",
        "/terms/\\u0007",
        "/../terms/",
        "/terms/%zz",
        long.as_str(),
    ] {
        let source = CONFIG.replace(
            "web_manifest={enabled=true}}",
            &format!("web_manifest={{enabled=true}},terms={{path=\"{path}\"}}}}"),
        );
        let error =
            gazette_compiler::workspace::compile(&source, Default::default()).expect_err(path);
        assert!(
            format!("{error:?}").contains("brands.publications.machine.terms.path"),
            "{path}: {error:?}"
        );
    }
    let source = CONFIG.replace(
        "web_manifest={enabled=true}}",
        "web_manifest={enabled=true},terms={path='/terms/',unknown=true}}",
    );
    assert!(gazette_compiler::workspace::compile(&source, Default::default()).is_err());
}
