//! Independent menu atoms preserve order, clearing and native field origins.

use super::*;
use gazette_compiler::{
    toml::PathSegment,
    workspace::{CompositionOriginQuery, ParsedWorkspace, ParsedWorkspaceDocuments},
};
use std::collections::BTreeMap;

const CHROME: &str = include_str!("../fixtures/site_chrome.toml");
const MENUS: &str = "[composition]\n\
    navigation=[{label='Second',destination='content:second'},{label='First',destination='content:first'}]\n\
    footer=[{label='Legal',destination='content:legal'}]\n";

#[test]
fn menus_override_and_clear_independently_without_replacing_chrome_copy() {
    let source = [
        MENUS,
        CHROME,
        OWNER,
        ALPHA,
        "[sites.composition]\nnavigation=[]\n",
        BETA,
        "[sites.composition]\nfooter=[{label='Other',destination='content:other'}]\n",
    ]
    .concat();
    let parsed = ParsedWorkspace::parse(&source, toml::Limits::default()).unwrap();
    let alpha = parsed
        .compilation()
        .workspace
        .composition_for(&key("site:alpha"))
        .unwrap();
    let beta = parsed
        .compilation()
        .workspace
        .composition_for(&key("site:beta"))
        .unwrap();
    assert!(alpha.navigation().unwrap().value().is_empty());
    assert_eq!(
        alpha.navigation().unwrap().origin().scope,
        SelectionScope::Site(key("site:alpha"))
    );
    assert_eq!(alpha.footer().unwrap().value()[0].label, "Legal");
    assert_eq!(
        alpha.footer().unwrap().origin().scope,
        SelectionScope::Workspace
    );
    assert_eq!(
        beta.navigation()
            .unwrap()
            .value()
            .iter()
            .map(|entry| entry.label.as_str())
            .collect::<Vec<_>>(),
        ["Second", "First"]
    );
    assert_eq!(beta.footer().unwrap().value()[0].label, "Other");
    for site in [alpha, beta] {
        assert_eq!(
            site.chrome().unwrap().origin().unwrap().scope,
            SelectionScope::Workspace
        );
        assert_eq!(
            site.chrome().unwrap().value().navigation_label.as_deref(),
            Some("Primary navigation")
        );
        assert_eq!(
            site.chrome().unwrap().value().footer_label.as_deref(),
            Some("Site footer")
        );
    }
    for (site, field, path, expected) in [
        ("site:alpha", SelectionField::Navigation {}, vec![], "[]"),
        (
            "site:alpha",
            SelectionField::Footer {},
            vec![PathSegment::Index(0), PathSegment::Key("label".into())],
            "'Legal'",
        ),
        (
            "site:beta",
            SelectionField::Footer {},
            vec![
                PathSegment::Index(0),
                PathSegment::Key("destination".into()),
            ],
            "'content:other'",
        ),
    ] {
        let origin = parsed
            .composition_origin(&CompositionOriginQuery {
                site: key(site),
                field,
                path,
            })
            .unwrap();
        assert_eq!(&source[origin.span.start..origin.span.end], expected);
    }
}

#[test]
fn split_document_origins_bind_inherited_and_local_menu_atoms() {
    let root = "includes=['defaults.toml','sites.toml']\n";
    let defaults = [MENUS, CHROME, OWNER].concat();
    let sites = [ALPHA, "[sites.composition]\nfooter=[]\n", BETA].concat();
    let documents = BTreeMap::from([
        ("gazette.toml".parse().unwrap(), root),
        ("defaults.toml".parse().unwrap(), defaults.as_str()),
        ("sites.toml".parse().unwrap(), sites.as_str()),
    ]);
    let root = "gazette.toml".parse().unwrap();
    let parsed = ParsedWorkspaceDocuments::parse(
        &root,
        documents
            .iter()
            .map(|(path, source)| (path, source.as_bytes())),
        Default::default(),
    )
    .unwrap();
    for (field, path, document, expected) in [
        (
            SelectionField::Navigation {},
            vec![PathSegment::Index(0), PathSegment::Key("label".into())],
            "defaults.toml",
            "'Second'",
        ),
        (SelectionField::Footer {}, vec![], "sites.toml", "[]"),
    ] {
        let origin = parsed
            .composition_origin(&CompositionOriginQuery {
                site: key("site:alpha"),
                field,
                path,
            })
            .unwrap();
        assert_eq!(origin.document.as_str(), document);
        assert_eq!(
            &documents[&origin.document][origin.span.start..origin.span.end],
            expected
        );
    }
}

#[test]
fn menus_reject_invalid_copy_excess_entries_and_obsolete_chrome_ownership() {
    for field in ["navigation", "footer"] {
        let positional =
            format!("[composition]\n{field}=[['Label','content:first']]\n{OWNER}{ALPHA}");
        assert!(workspace::compile(&positional, toml::Limits::default()).is_err());
        let source = format!(
            "[composition]\n{field}=[{{label='',destination='content:first'}}]\n{OWNER}{ALPHA}"
        );
        assert!(
            invalid(&source)
                .iter()
                .any(|error| error.code == "workspace_navigation_copy"
                    && error.field == format!("composition.{field}.label"))
        );
        let entries = vec!["{label='Entry',destination='content:first'}"; 257].join(",");
        let source = format!("[composition]\n{field}=[{entries}]\n{OWNER}{ALPHA}");
        assert!(
            invalid(&source)
                .iter()
                .any(|error| error.code == "workspace_navigation_limit")
        );
    }
    let obsolete = CHROME.replace(
        "[composition.chrome]\n",
        "[composition.chrome]\nnavigation=[]\n",
    );
    assert!(
        workspace::compile(
            &[obsolete.as_str(), OWNER, ALPHA].concat(),
            toml::Limits::default()
        )
        .is_err()
    );
}

#[test]
fn semantic_menu_keys_are_optional_namespaced_and_unique_within_each_list() {
    let keyed = MENUS.replace("{label='Legal'", "{key='theme:credit',label='Legal'");
    let source = [keyed.as_str(), CHROME, OWNER, ALPHA, BETA].concat();
    let parsed = ParsedWorkspace::parse(&source, Default::default()).unwrap();
    let selected = parsed
        .compilation()
        .workspace
        .composition_for(&key("site:alpha"))
        .unwrap();
    assert_eq!(
        selected.footer().unwrap().value()[0].key,
        Some(key("theme:credit"))
    );
    assert!(selected.navigation().unwrap().value()[0].key.is_none());
    for footer in [
        "footer=[{key='theme:credit',label='One',destination='content:first'},{key='theme:credit',label='Two',destination='content:second'}]\n",
        "footer=[{key='unscoped',label='One',destination='content:first'}]\n",
    ] {
        let invalid = ["[composition]\n", footer, CHROME, OWNER, ALPHA, BETA].concat();
        assert!(ParsedWorkspace::parse(&invalid, Default::default()).is_err());
    }
}
