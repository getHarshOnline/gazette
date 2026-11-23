//! Intended processing validates without reading providers or native source repositories.

use gazette_compiler::{contracts, toml::ParsedToml, workspace};
use serde_json::json;

fn configuration(selection: &str) -> String {
    format!(
        "[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n[[brands]]\nid='brand:one'\nname='One'\n[[brands.publications]]\nsite='site:one'\nrepository='https://github.com/example/site'\nprocessing={{{selection}}}\n[[sources]]\nid='source:missing'\nlocation={{kind='repository',root='/not-opened/processing-source',commit='1111111111111111111111111111111111111111',subdirectory='docs'}}\n"
    )
}

fn diagnostic(source: &str, code: &str) {
    let workspace::Error::Invalid(errors) =
        workspace::compile(source, Default::default()).expect_err("refused desired state")
    else {
        panic!("expected semantic diagnostic")
    };
    assert!(
        errors.iter().any(|error| error.code == code
            && error
                .key
                .as_ref()
                .is_some_and(|key| key.as_str() == "site:one")),
        "{errors:?}"
    );
}

fn native_discussion(mut source: String, enabled: bool, project: bool) -> String {
    let atom = "discussion={capability='discussion.render/v1',repository_id='private-repo-id',category_id='private-category-id'}\n";
    source = format!(
        "[composition.plugins]\n'discussion-plugin'={{state='enabled'}}\n[composition.capabilities]\n'discussion.render/v1'={}\n{source}",
        if enabled {
            "{state='enabled',provider={kind='plugin',package='discussion-plugin',provider='discussion'},requirement='required'}"
        } else {
            "{state='disabled'}"
        }
    );
    source.push_str("[[plugin_packages]]\npackage='discussion-plugin'\nsource='source:missing'\nmanifest='plugin.toml'\n");
    if project {
        source.push_str(&format!("[[projects]]\nid='project:one'\nlifecycle='active'\nvisibility='public'\navailability='available'\nrepository='https://github.com/example/project'\n{atom}[[placements]]\nproject='project:one'\nsite='site:one'\nrole='primary'\nrender=true\ndiscoverable=true\ndocuments={{source='source:missing',target='project'}}\n"));
    } else {
        source = source.replacen(
            "deployable=false\n",
            &format!("deployable=false\n{atom}"),
            1,
        );
    }
    source
}

fn native_browser(source: String, analytics: Option<bool>, webmcp: Option<bool>) -> String {
    let mut selected = String::new();
    for (capability, provider, enabled) in [
        (
            "publication.analytics/v1",
            "google-analytics-client",
            analytics,
        ),
        ("publication.webmcp/v1", "webmcp-client", webmcp),
    ] {
        if let Some(enabled) = enabled {
            let binding = if enabled {
                format!(
                    "{{state='enabled',provider={{kind='gazette',provider='{provider}'}},requirement='required'}}"
                )
            } else {
                "{state='disabled'}".to_owned()
            };
            selected.push_str(&format!("'{capability}'={binding}\n"));
        }
    }
    format!("[composition.capabilities]\n{selected}{source}")
}

fn selected_processing(compiled: &workspace::Compilation) -> &workspace::PublicationProcessing {
    compiled
        .workspace
        .publication_for(&"site:one".parse().unwrap())
        .unwrap()
        .processing
        .as_ref()
        .unwrap()
}

fn declaration(source: &str) -> serde_json::Value {
    ParsedToml::parse(source, Default::default())
        .expect("valid native TOML syntax")
        .deserialize()
        .expect("JSON projection of the same retained syntax")
}

#[test]
fn disabled_services_are_strict_inspectable_choices_without_native_provider_ids() {
    let source = configuration(
        "hosting={provider='github-pages'},analytics={enabled=false,provider='google-analytics'},subscription={enabled=false,provider='google-apps-script'},discussion={enabled=false,provider='giscus',repository='https://github.com/example/unobserved'},webmcp={enabled=false}",
    );
    let compiled = workspace::compile(&source, Default::default()).expect("pure configured intent");
    let processing = serde_json::to_value(
        compiled
            .workspace
            .publication_for(&"site:one".parse().unwrap())
            .unwrap()
            .processing
            .as_ref()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        processing["analytics"],
        json!({"enabled":false,"provider":"google-analytics"})
    );
    assert_eq!(
        processing["discussion"]["repository"],
        "https://github.com/example/unobserved"
    );
    for replacement in [
        "provider='unknown'",
        "provider='google-analytics',measurement_id='invented'",
    ] {
        let source = source.replace("provider='google-analytics'", replacement);
        assert!(matches!(
            workspace::compile(&source, Default::default()),
            Err(workspace::Error::Toml(_))
        ));
    }
}

#[test]
fn parked_subscription_refuses_before_any_source_acquisition() {
    diagnostic(
        &configuration("subscription={enabled=true,provider='google-apps-script'}"),
        "workspace_processing_unsupported",
    );
}

#[test]
fn enabled_browser_bindings_require_explicit_values_and_independent_native_capabilities() {
    for (selection, analytics, webmcp, code) in [
        (
            "analytics={enabled=true,provider='google-analytics'}",
            Some(true),
            None,
            "workspace_processing_analytics_binding",
        ),
        (
            "analytics={enabled=true,provider='google-analytics',measurement_id='G-EXACT123'}",
            None,
            None,
            "workspace_processing_analytics_binding",
        ),
        (
            "analytics={enabled=true,provider='google-analytics',measurement_id='G-EXACT123'}",
            Some(false),
            None,
            "workspace_processing_analytics_binding",
        ),
        (
            "webmcp={enabled=true}",
            None,
            Some(true),
            "workspace_processing_webmcp_binding",
        ),
        (
            "webmcp={enabled=true,tools=['current_page']}",
            None,
            None,
            "workspace_processing_webmcp_binding",
        ),
        (
            "webmcp={enabled=true,tools=['current_page']}",
            None,
            Some(false),
            "workspace_processing_webmcp_binding",
        ),
    ] {
        diagnostic(
            &native_browser(configuration(selection), analytics, webmcp),
            code,
        );
    }
}

#[test]
fn selected_browser_bindings_preserve_exact_public_values_and_generated_schema() {
    let source = native_browser(
        configuration(
            "analytics={enabled=true,provider='google-analytics',measurement_id='G-EXACT123'},webmcp={enabled=true,tools=['search','current_page']}",
        )
        .replace("[[sources]]", "machine={search='lexical'}\n[[sources]]"),
        Some(true),
        Some(true),
    );
    let compiled = workspace::compile(&source, Default::default())
        .expect("public bindings do not open the declared missing source");
    let processing = selected_processing(&compiled);
    assert_eq!(
        serde_json::to_value(processing).unwrap(),
        json!({
            "analytics":{"enabled":true,"provider":"google-analytics","measurement_id":"G-EXACT123"},
            "webmcp":{"enabled":true,"tools":["search","current_page"]}
        })
    );
    assert_eq!(
        processing
            .browser_bindings()
            .map(|value| serde_json::to_value(value).unwrap())
            .collect::<Vec<_>>(),
        vec![
            json!({"contract":"publication-analytics-v1","measurement_id":"G-EXACT123"}),
            json!({"contract":"publication-webmcp-v1","tools":["search","current_page"]}),
        ]
    );
    let schema = contracts::workspace();
    let validator = jsonschema::validator_for(schema.as_value()).unwrap();
    let input = declaration(&source);
    assert!(
        validator.is_valid(&input),
        "{:?}",
        validator.iter_errors(&input).collect::<Vec<_>>()
    );
}

#[test]
fn active_browser_capabilities_refuse_omitted_or_disabled_processing() {
    for (analytics, webmcp, selection, code) in [
        (
            Some(true),
            None,
            "",
            "workspace_processing_analytics_conflict",
        ),
        (
            Some(true),
            None,
            "analytics={enabled=false,provider='google-analytics'}",
            "workspace_processing_analytics_conflict",
        ),
        (
            Some(true),
            None,
            "analytics={enabled=false,provider='google-analytics',measurement_id='G-DORMANT'}",
            "workspace_processing_analytics_conflict",
        ),
        (None, Some(true), "", "workspace_processing_webmcp_conflict"),
        (
            None,
            Some(true),
            "webmcp={enabled=false}",
            "workspace_processing_webmcp_conflict",
        ),
        (
            None,
            Some(true),
            "webmcp={enabled=false,tools=['search','current_page']}",
            "workspace_processing_webmcp_conflict",
        ),
    ] {
        let source = native_browser(configuration(selection), analytics, webmcp);
        diagnostic(&source, code);
        if selection.is_empty() {
            diagnostic(&source.replace("processing={}\n", ""), code);
        }
    }
}

#[test]
fn dormant_browser_values_do_not_activate_capabilities_search_or_providers() {
    for (analytics, webmcp) in [(None, None), (Some(false), Some(false))] {
        let source = native_browser(
            configuration(
                "analytics={enabled=false,provider='google-analytics',measurement_id='G-DORMANT'},webmcp={enabled=false,tools=['search','current_page']}",
            ),
            analytics,
            webmcp,
        );
        let compiled = workspace::compile(&source, Default::default()).unwrap();
        let processing = selected_processing(&compiled);
        assert_eq!(processing.browser_bindings().count(), 0);
        assert_eq!(
            serde_json::to_value(processing).unwrap(),
            json!({
                "analytics":{"enabled":false,"provider":"google-analytics","measurement_id":"G-DORMANT"},
                "webmcp":{"enabled":false,"tools":["search","current_page"]}
            })
        );
        assert!(
            compiled
                .workspace
                .publication_for(&"site:one".parse().unwrap())
                .unwrap()
                .machine
                .search
                .is_none()
        );
    }
}

#[test]
fn webmcp_search_requires_selected_machine_search_while_current_page_needs_no_search() {
    let source = native_browser(
        configuration("webmcp={enabled=true,tools=['search','current_page']}"),
        None,
        Some(true),
    );
    diagnostic(&source, "workspace_processing_webmcp_search");
    workspace::compile(
        &source.replace("[[sources]]", "machine={search='lexical'}\n[[sources]]"),
        Default::default(),
    )
    .expect("explicit native search selection supports the selected read-only search tool");
    let current = workspace::compile(
        &source.replace("['search','current_page']", "['current_page']"),
        Default::default(),
    )
    .expect("current-page projection does not infer a search output");
    assert_eq!(
        selected_processing(&current)
            .browser_bindings()
            .map(|value| serde_json::to_value(value).unwrap())
            .collect::<Vec<_>>(),
        vec![json!({"contract":"publication-webmcp-v1","tools":["current_page"]})]
    );
}

#[test]
fn site_disabled_browser_capability_does_not_fall_back_to_enabled_workspace_default() {
    let source = native_browser(
        configuration("analytics={enabled=true,provider='google-analytics',measurement_id='G-EXACT123'}"),
        Some(true),
        None,
    )
    .replace(
        "deployable=false\n",
        "deployable=false\n[sites.composition.capabilities]\n'publication.analytics/v1'={state='disabled'}\n",
    );
    diagnostic(&source, "workspace_processing_analytics_binding");
    let disabled = workspace::compile(
        &source.replace("analytics={enabled=true", "analytics={enabled=false"),
        Default::default(),
    )
    .expect("explicit site disable wins without activating its dormant ID");
    assert_eq!(selected_processing(&disabled).browser_bindings().count(), 0);
    let capability: gazette_core::publication::CapabilityId =
        "publication.analytics/v1".parse().unwrap();
    let selected = disabled
        .workspace
        .composition_for(&"site:one".parse().unwrap())
        .unwrap()
        .capabilities()
        .get(&capability)
        .unwrap();
    assert!(matches!(
        selected.value(),
        workspace::CapabilitySelection::Disabled {}
    ));
    assert_eq!(
        selected.origin().scope,
        workspace::SelectionScope::Site("site:one".parse().unwrap())
    );
}

#[test]
fn processing_literals_and_generated_schema_agree_without_provider_observation() {
    let schema = contracts::workspace();
    let validator = jsonschema::validator_for(schema.as_value()).unwrap();
    for selection in [
        "analytics={enabled=false,provider='google-analytics',measurement_id='G-A'}".to_owned(),
        format!(
            "analytics={{enabled=false,provider='google-analytics',measurement_id='G-{}'}}",
            "A".repeat(62)
        ),
        "webmcp={enabled=false,tools=['current_page']}".to_owned(),
        "webmcp={enabled=false,tools=['search','current_page']}".to_owned(),
    ] {
        let source = configuration(&selection);
        let input = declaration(&source);
        assert!(validator.is_valid(&input), "{selection}");
        workspace::compile(&source, Default::default()).expect("valid dormant literal");
    }
    for selection in [
        "analytics={enabled=false,provider='unknown'}".to_owned(),
        "analytics={enabled=false,provider='google-analytics',measurement_id='UA-1'}".to_owned(),
        "analytics={enabled=false,provider='google-analytics',measurement_id='G-'}".to_owned(),
        "analytics={enabled=false,provider='google-analytics',measurement_id='G-lower'}".to_owned(),
        "analytics={enabled=false,provider='google-analytics',measurement_id='G-Å'}".to_owned(),
        "analytics={enabled=false,provider='google-analytics',measurement_id=123}".to_owned(),
        "analytics={enabled=false,provider='google-analytics',measurement_id=\"G-A\\n\"}"
            .to_owned(),
        format!(
            "analytics={{enabled=false,provider='google-analytics',measurement_id='G-{}'}}",
            "A".repeat(63)
        ),
        "analytics={enabled=false,provider='google-analytics',consent=true}".to_owned(),
        "webmcp={enabled=false,tools=[]}".to_owned(),
        "webmcp={enabled=false,tools=['current_page','current_page']}".to_owned(),
        "webmcp={enabled=false,tools=['search','current_page','search']}".to_owned(),
        "webmcp={enabled=false,tools=['write_output']}".to_owned(),
        "webmcp={enabled=false,tools='current_page'}".to_owned(),
        "webmcp={enabled=false,provider='automatic'}".to_owned(),
        "invented={enabled=false}".to_owned(),
    ] {
        let source = configuration(&selection);
        let input = declaration(&source);
        assert!(!validator.is_valid(&input), "schema accepted {selection}");
        assert!(
            matches!(
                workspace::compile(&source, Default::default()),
                Err(workspace::Error::Toml(_))
            ),
            "native accepted {selection}"
        );
    }
}

#[test]
fn disabled_discussion_refuses_active_site_and_rendered_project_capabilities() {
    for project in [false, true] {
        let source = configuration("discussion={enabled=false,provider='giscus'}");
        diagnostic(
            &native_discussion(source.clone(), true, project),
            "workspace_processing_discussion_conflict",
        );
        workspace::compile(
            &native_discussion(source, false, project),
            Default::default(),
        )
        .expect("disabled native atom never activates its provider");
    }
}

#[test]
fn enabled_discussion_requires_existing_native_bridge_and_exact_repository() {
    let selection =
        "discussion={enabled=true,provider='giscus',repository='https://github.com/example/site'}";
    diagnostic(
        &configuration(selection),
        "workspace_processing_discussion_binding",
    );
    let source = native_discussion(configuration(selection), true, false);
    workspace::compile(&source, Default::default())
        .expect("existing supported native discussion stays selected");
    diagnostic(
        &source.replace(
            "repository='https://github.com/example/site'}",
            "repository='https://github.com/example/namesake'}",
        ),
        "workspace_processing_discussion_binding",
    );
    diagnostic(
        &configuration(
            "discussion={enabled=false,provider='giscus',repository='file:///private/service'}",
        ),
        "workspace_processing_repository",
    );
}
