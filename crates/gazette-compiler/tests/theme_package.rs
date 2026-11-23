//! Theme-package manifests are presentation contracts, never provider selections.

#[path = "theme_package/social_card.rs"]
mod social_card;

use gazette_compiler::{
    theme_package::{Namespace, ParsedManifest},
    toml,
};
use gazette_core::resource::ResourceId;

fn source() -> String {
    "package='theme:article'\n\
     namespace='article'\n\
     templates='templates'\n\
     render={page='page.html'}\n\
     palettes=[{id='palette:day',browser_theme_colors={light='#ffffff',dark='oklch(20% 0.02 250)'}}]\n\
     resource_requirements=['resource:tokens','resource:styles']\n\
     [components.callout]\n\
     template='components/callout.html'\n\
     [capabilities]\n\
     required=['publication.navigation/v1']\n\
     optional=['publication.search/v1']\n\
     [[dependencies]]\n\
     package='theme:foundation'\n\
     [[resources]]\n\
     id='resource:styles'\n\
     inputs=['styles/article.css']\n\
     build={kind='stylesheet',entry='styles/article.css'}\n\
     [[resources]]\n\
     id='resource:tokens'\n\
     inputs=['tokens/article.css']\n\
     build={kind='stylesheet',entry='tokens/article.css'}\n"
        .to_owned()
}

fn parse(source: &str) -> Result<ParsedManifest<'_>, gazette_compiler::theme_package::Error> {
    ParsedManifest::parse(source, toml::Limits::default())
}

#[test]
fn package_resources_and_capability_needs_are_typed_and_normalized() {
    let source = source();
    let parsed = parse(&source).expect("valid generic Theme package");
    let manifest = parsed.manifest();
    assert_eq!(manifest.package.as_str(), "theme:article");
    assert_eq!(manifest.namespace.as_str(), "article");
    assert_eq!(
        manifest.dependencies[0].package.as_str(),
        "theme:foundation"
    );
    assert_eq!(
        manifest
            .resources
            .iter()
            .map(|resource| resource.id.as_str())
            .collect::<Vec<_>>(),
        ["resource:styles", "resource:tokens"]
    );
    assert_eq!(
        manifest
            .resource_requirements
            .iter()
            .map(|resource| resource.as_str())
            .collect::<Vec<_>>(),
        ["resource:tokens", "resource:styles"]
    );
    assert_eq!(
        manifest.capabilities.required[0].as_str(),
        "publication.navigation/v1"
    );
    assert_eq!(
        manifest.capabilities.optional[0].as_str(),
        "publication.search/v1"
    );
    assert_eq!(manifest.render.as_ref().unwrap().page.as_str(), "page.html");
    assert_eq!(
        manifest.palettes[0].browser_theme_colors.light.as_str(),
        "#ffffff"
    );
    assert!(
        parsed
            .components()
            .definition(&"callout".parse().unwrap())
            .is_some()
    );
    assert_eq!(
        parsed.source_sha256().to_string(),
        gazette_core::source::Sha256Digest::of(source.as_bytes()).to_string()
    );
}

#[test]
fn namespace_grammar_is_small_portable_and_independent_of_design_vocabulary() {
    let long_valid = "a".repeat(63);
    for accepted in ["a", "base", "article-world", "theme2", long_valid.as_str()] {
        let namespace: Namespace = accepted.parse().expect(accepted);
        assert_eq!(namespace.as_str(), accepted);
    }
    let too_long = "a".repeat(64);
    for rejected in [
        "",
        "2base",
        "Base",
        "base_kit",
        "base/world",
        "base.",
        "base-",
        "é",
        too_long.as_str(),
    ] {
        assert!(rejected.parse::<Namespace>().is_err(), "{rejected:?}");
        let invalid = source().replace("namespace='article'", &format!("namespace='{rejected}'"));
        assert!(parse(&invalid).is_err(), "{rejected:?}");
    }
}

#[test]
fn theme_copy_accepts_partial_labels_but_never_destinations_or_handoffs() {
    let header = "package='theme:copy'\nnamespace='copy'\ntemplates='templates'\n";
    let source = format!(
        "{header}[chrome]\nfooter_label='Footer'\n[chrome.shell.settings]\ntitle='Preferences'\n[chrome.regions]\n'base:empty'='No entries'\n"
    );
    let parsed = parse(&source).expect("a Theme can supply only some labels");
    let copy = parsed.manifest().chrome.as_ref().unwrap();
    assert_eq!(copy.footer_label.as_deref(), Some("Footer"));
    assert!(copy.navigation_label.is_none());
    for declaration in [
        "[chrome]\nfooter_label=''\n",
        "[chrome]\nfooter_label='   '\n",
        "[chrome.shell.search]\nenabled=false\n",
        "[chrome.shell.search]\nenabled=true\n",
        "[chrome.shell.search]\nno_script_destination='content:search'\n",
        "[chrome.reading_assistance]\nlabel='Ask'\nprompt_template='{selection} {article_url} {site_name} {site_url}'\nselection_limit=100\ncontext_limit=100\nproviders=[{id='assistant:one',label='One',endpoint='https://assistant.example/',query_parameter='prompt'}]\n",
    ] {
        assert!(
            parse(&format!("{header}{declaration}")).is_err(),
            "{declaration}"
        );
    }
}

#[test]
fn resources_use_the_shared_strict_logical_identity() {
    for accepted in ["module:article", "stylesheet:article-world"] {
        assert!(accepted.parse::<ResourceId>().is_ok(), "{accepted:?}");
    }
    for rejected in [
        "article",
        "Module:article",
        "module:Article",
        "module:article/world",
        "module:article:world",
    ] {
        assert!(rejected.parse::<ResourceId>().is_err(), "{rejected:?}");
        let invalid = source().replace("resource:styles", rejected);
        assert!(parse(&invalid).is_err(), "{rejected:?}");
    }
}

#[test]
fn duplicate_dependencies_resources_and_requirements_fail_closed() {
    let duplicate_dependency = source().replace(
        "[[resources]]",
        "[[dependencies]]\npackage='theme:foundation'\n[[resources]]",
    );
    assert_eq!(
        parse(&duplicate_dependency).unwrap_err().code(),
        "theme_package_declaration"
    );
    let duplicate_resource = source().replace("id='resource:tokens'", "id='resource:styles'");
    assert_eq!(
        parse(&duplicate_resource).unwrap_err().code(),
        "theme_package_declaration"
    );
    let duplicate_input = source().replace(
        "inputs=['tokens/article.css']",
        "inputs=['tokens/article.css','tokens/article.css']",
    );
    assert_eq!(
        parse(&duplicate_input).unwrap_err().code(),
        "theme_package_declaration"
    );
    let duplicate_requirement = source().replace(
        "resource_requirements=['resource:tokens','resource:styles']",
        "resource_requirements=['resource:tokens','resource:tokens']",
    );
    assert_eq!(
        parse(&duplicate_requirement).unwrap_err().code(),
        "theme_package_declaration"
    );
    let external_requirement = source().replace(
        "resource_requirements=['resource:tokens','resource:styles']",
        "resource_requirements=['resource:plugin-owned']",
    );
    assert_eq!(
        parse(&external_requirement)
            .expect("logical requirement need not be a Theme package export")
            .manifest()
            .resource_requirements[0]
            .as_str(),
        "resource:plugin-owned"
    );
    let legacy_load = source().replace("resource_requirements", "load");
    assert_eq!(parse(&legacy_load).unwrap_err().code(), "toml_invalid");
}

#[test]
fn required_and_optional_capability_needs_are_disjoint_sets() {
    let duplicate_required = source().replace(
        "required=['publication.navigation/v1']",
        "required=['publication.navigation/v1','publication.navigation/v1']",
    );
    assert_eq!(
        parse(&duplicate_required).unwrap_err().code(),
        "theme_package_declaration"
    );
    let overlap = source().replace(
        "optional=['publication.search/v1']",
        "optional=['publication.navigation/v1']",
    );
    assert_eq!(
        parse(&overlap).unwrap_err().code(),
        "theme_package_declaration"
    );
}

#[test]
fn render_components_palettes_and_resources_reuse_existing_contracts() {
    let duplicate_palette = source().replace(
        "palettes=[{id='palette:day',browser_theme_colors={light='#ffffff',dark='oklch(20% 0.02 250)'}}]",
        "palettes=[{id='palette:day',browser_theme_colors={light='#ffffff',dark='#000000'}},{id='palette:day',browser_theme_colors={light='white',dark='black'}}]",
    );
    assert_eq!(
        parse(&duplicate_palette).unwrap_err().code(),
        "theme_package_declaration"
    );
    let missing_entry = source().replace(
        "inputs=['styles/article.css']",
        "inputs=['styles/other.css']",
    );
    assert_eq!(
        parse(&missing_entry).unwrap_err().code(),
        "theme_package_declaration"
    );
    let invalid_component = source().replace(
        "template='components/callout.html'",
        "template='components/callout.html'\narguments={type='not-a-json-schema-type'}",
    );
    assert_eq!(
        parse(&invalid_component).unwrap_err().code(),
        "theme_package_declaration"
    );
}

#[test]
fn browser_theme_colors_are_complete_native_css_values() {
    for color in [
        "#0f172a",
        "rgb(10 20 30 / 80%)",
        "oklch(62% 0.2 250)",
        "rebeccapurple",
    ] {
        let candidate = source().replace("#ffffff", color);
        assert!(parse(&candidate).is_ok(), "{color}");
    }
    for color in [
        "",
        " currentcolor",
        "currentcolor",
        "var(--theme)",
        "red; color: blue",
    ] {
        let candidate = source().replace("#ffffff", color);
        assert!(parse(&candidate).is_err(), "{color:?}");
    }
}

#[test]
fn provider_compatibility_and_package_authored_source_fields_are_rejected() {
    for injected in [
        "provider='provider:shopify'\n",
        "providers=['provider:search']\n",
        "schema=1\n",
        "version='1.2.3'\n",
        "compatibility='legacy'\n",
    ] {
        let invalid = format!("{injected}{}", source());
        assert_eq!(parse(&invalid).unwrap_err().code(), "toml_invalid");
    }
    for field in [
        "provider='provider:theme'\n",
        "source='source:foundation'\n",
        "tree_sha256='0000000000000000000000000000000000000000000000000000000000000000'\n",
        "namespace='dependency-alias'\n",
    ] {
        let invalid = source().replace(
            "package='theme:foundation'",
            &format!("package='theme:foundation'\n{field}"),
        );
        assert_eq!(parse(&invalid).unwrap_err().code(), "toml_invalid");
    }
}

#[test]
fn self_dependency_is_rejected() {
    assert_eq!(
        parse(&source().replace("theme:foundation", "theme:article"))
            .unwrap_err()
            .code(),
        "theme_package_declaration"
    );
}

#[test]
fn resource_output_defaults_reject_incomplete_or_unusable_declarations() {
    for suffix in [
        "output='/assets/'",
        "use='dependency'",
        "output='/asset.css'\nuse='dependency'",
    ] {
        let text = format!("{}\n{suffix}\n", source());
        assert!(parse(&text).is_err(), "{suffix}");
    }
    let text = format!("{}\noutput='/tokens.css'\n", source());
    let parsed = parse(&text).expect("site output may omit use");
    assert_eq!(
        parsed.manifest().resources[1]
            .output
            .as_ref()
            .unwrap()
            .as_str(),
        "/tokens.css"
    );
}
