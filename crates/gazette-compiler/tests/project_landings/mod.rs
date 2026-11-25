use gazette_compiler::{
    authored,
    corpus::{Input, ParsedCorpus},
    workspace,
};
use gazette_core::publication::{Key, ProjectLifecycle};
mod semantics;

fn key(value: &str) -> Key {
    value.parse().unwrap()
}

fn configuration() -> String {
    r#"
[[principals]]
id='person:publisher'
kind='person'
[[sites]]
id='site:one'
publisher='person:publisher'
deployable=false
[[sources]]
id='source:writing'
location={kind='filesystem',root='/never-read'}
[[brands]]
id='brand:one'
name='One'
[[brands.publications]]
site='site:one'
public_base_url='https://one.test/'
authored=[{source='source:writing',target='writing'}]
[[projects]]
id='project:one'
landing='content:one'
"#
    .into()
}

fn landing() -> String {
    r#"+++
projects=[{project='project:one',kind='about'}]
[project]
id='project:one'
name='A project whose address can change'
lifecycle='paused'
visibility='public'
availability='gated'
repository='https://code.test/one'
license='MIT'
[content]
id='content:one'
site='site:one'
title='The project'
lifecycle='draft'
route={path='/one/'}
structure={kind='page',family={kind='project'}}
+++
# The project
Its published facts have one owner.
"#
    .into()
}

fn parse(text: &str) -> ParsedCorpus<'_> {
    ParsedCorpus::parse(
        [Input {
            path: "writing/one.md".parse().unwrap(),
            bytes: text.as_bytes(),
        }],
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .unwrap()
}

#[test]
fn project_resources_take_licence_and_notices_from_the_source_record() {
    use gazette_core::markdown::ProjectResourceKind::{License, Notice};

    let config = workspace::compile(&configuration(), Default::default()).unwrap();
    let text = landing().replace(
        "[content]",
        "[project.source]\nlicense_url='https://code.test/one/LICENSE'\nnotice_url='https://code.test/one/NOTICE'\ncitation_url='https://code.test/one/CITATION.cff'\nauthor='person:publisher'\nrights_holder='person:publisher'\n[content]",
    );
    let corpus = parse(&text);
    let resolved = authored::resolve(&config, &corpus, Default::default()).unwrap();
    let actions = resolved
        .project_resources_for(&key("content:one"))
        .unwrap()
        .unwrap()
        .actions;
    assert_eq!(
        actions
            .iter()
            .filter(|action| matches!(action.kind, License | Notice))
            .map(|action| (action.kind, action.url.as_str()))
            .collect::<Vec<_>>(),
        [
            (License, "https://code.test/one/LICENSE"),
            (Notice, "https://code.test/one/NOTICE"),
        ]
    );
}

#[test]
fn retained_landing_owns_facts_and_an_unsaved_replacement_changes_the_same_catalog() {
    let config = workspace::compile(&configuration(), Default::default()).unwrap();
    assert!(config.workspace.project(&key("project:one")).is_none());
    assert!(
        config
            .workspace
            .project_declaration(&key("project:one"))
            .is_some()
    );
    let text = landing();
    let corpus = parse(&text);
    let resolved = authored::resolve(&config, &corpus, Default::default()).unwrap();
    let facts = resolved.content().project(&key("project:one")).unwrap();
    assert_eq!(facts.lifecycle, ProjectLifecycle::Paused);
    assert_eq!(
        facts.resources.as_ref().unwrap().landing.as_ref(),
        Some(&key("content:one"))
    );
    let page = resolved
        .project_page(
            &key("content:one"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    let json = serde_json::to_value(page).unwrap();
    assert!(
        json.to_string()
            .contains("A project whose address can change")
    );
    let replacement = text.replace("lifecycle='paused'", "lifecycle='active'");
    let edited = parse(&replacement);
    let edited = authored::resolve(&config, &edited, Default::default()).unwrap();
    assert_eq!(
        edited
            .content()
            .project(&key("project:one"))
            .unwrap()
            .lifecycle,
        ProjectLifecycle::Active
    );
    assert_eq!(
        facts.lifecycle,
        ProjectLifecycle::Paused,
        "the old capture remains immutable"
    );
}

#[test]
fn mixed_configuration_and_unselected_landing_facts_are_refused() {
    let mixed =
        configuration() + "lifecycle='active'\nvisibility='public'\navailability='available'\n";
    assert!(workspace::compile(&mixed, Default::default()).is_err());
    let inline = configuration().replace(
        "landing='content:one'",
        "lifecycle='active'\nvisibility='public'\navailability='available'",
    );
    let config = workspace::compile(&inline, Default::default()).unwrap();
    let text = landing();
    let corpus = parse(&text);
    let error = authored::resolve(&config, &corpus, Default::default()).unwrap_err();
    assert!(error.to_string().contains("project_facts_unselected"));
}

#[test]
fn absent_wrong_identity_wrong_family_and_missing_about_never_produce_a_partial_catalog() {
    let config = workspace::compile(&configuration(), Default::default()).unwrap();
    let empty =
        ParsedCorpus::parse([], Default::default(), &gazette_adapter_markdown::Comrak).unwrap();
    assert!(
        authored::resolve(&config, &empty, Default::default())
            .unwrap_err()
            .to_string()
            .contains("project_landing_missing")
    );
    for text in [
        landing().replacen(
            "[project]\nid='project:one'",
            "[project]\nid='project:other'",
            1,
        ),
        landing().replace("family={kind='project'}", "family={kind='article'}"),
        landing().replace("kind='about'", "kind='mentions'"),
    ] {
        let corpus = parse(&text);
        assert!(
            authored::resolve(&config, &corpus, Default::default())
                .unwrap_err()
                .to_string()
                .contains("project_landing_owner")
        );
    }
}

#[test]
fn documentation_uses_the_selected_module_and_the_targets_actual_index_spelling() {
    let declaration = configuration().replace("[[projects]]", r#"
[brands.publications.documentation_repository]
name='manuals'
path='/not-read/manuals'
public_base_url='https://docs.test/prefix/'
[brands.publications.documentation_repository.build]
target={kind='mintlify',theme='mint',primary_color='#112233'}
[[brands.publications.documentation_repository.build.modules]]
project='project:one'
slug='manual'
title='Manual'
manual={source='source:writing'}
navigation=[{kind='manual',title='Read',pages=[{title='Start',route='manual/index',input={kind='file',path='docs/index.md'}}]}]
[[projects]]"#);
    let config = workspace::compile(&declaration, Default::default()).unwrap();
    let text = landing();
    let corpus = parse(&text);
    let resolved = authored::resolve(&config, &corpus, Default::default()).unwrap();
    let resources = resolved
        .project_resources_for(&key("content:one"))
        .unwrap()
        .unwrap();
    assert_eq!(resources.actions.len(), 2);
    assert_eq!(
        resources.actions[0].url,
        "https://docs.test/prefix/manual/gazette-page-index"
    );
    let no_base = declaration.replace("public_base_url='https://docs.test/prefix/'", "");
    let local = workspace::compile(&no_base, Default::default()).unwrap();
    let local = authored::resolve(&local, &corpus, Default::default()).unwrap();
    assert!(
        local
            .project_resources_for(&key("content:one"))
            .unwrap()
            .unwrap()
            .actions
            .iter()
            .all(|action| action.kind
                != gazette_core::markdown::ProjectResourceKind::CodeDocumentation)
    );
    let repeated_url = text.replace("[content]", "resources={code_documentation='https://docs.test/prefix/manual/gazette-page-index'}\n[content]");
    let corpus = parse(&repeated_url);
    assert!(
        authored::resolve(&config, &corpus, Default::default())
            .unwrap_err()
            .to_string()
            .contains("project_documentation_owner")
    );
}

#[test]
fn gated_availability_resolves_a_cross_publication_relationship_without_changing_state() {
    let config = configuration()
        + r#"
[[sites]]
id='site:service'
publisher='person:publisher'
deployable=false
[[brands]]
id='brand:service'
name='Service'
[[brands.publications]]
site='site:service'
public_base_url='https://service.test/'
authored=[{source='source:writing',target='service'}]
"#;
    let config = workspace::compile(&config, Default::default()).unwrap();
    let text = landing().replace(
        "[content]",
        "available_through={site='site:service',content='content:enquiry'}\n[content]",
    );
    let service = r#"+++
[content]
id='content:enquiry'
site='site:service'
title='Ask about the project'
lifecycle='draft'
route={path='/enquiry/'}
structure={kind='page',family={kind='article'}}
+++
# Ask about the project
"#;
    let corpus = ParsedCorpus::parse(
        [
            Input {
                path: "writing/one.md".parse().unwrap(),
                bytes: text.as_bytes(),
            },
            Input {
                path: "service/enquiry.md".parse().unwrap(),
                bytes: service.as_bytes(),
            },
        ],
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .unwrap();
    let resolved = authored::resolve(&config, &corpus, Default::default()).unwrap();
    let page = resolved
        .project_page(
            &key("content:one"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    let action = page
        .data
        .project_resources
        .as_ref()
        .unwrap()
        .actions
        .iter()
        .find(|action| action.kind == gazette_core::markdown::ProjectResourceKind::AvailableThrough)
        .unwrap();
    assert_eq!(action.url, "https://service.test/enquiry/");
    assert_eq!(action.destination.site, Some(key("site:service")));
    assert_eq!(
        resolved
            .content()
            .project(&key("project:one"))
            .unwrap()
            .lifecycle,
        ProjectLifecycle::Paused
    );
    let json_ld = serde_json::to_value(page.json_ld()).unwrap().to_string();
    assert!(json_ld.contains("gazette:availableThrough"));
    assert!(json_ld.contains("content:enquiry"));
    assert!(
        serde_json::to_value(page.search())
            .unwrap()
            .to_string()
            .contains("available_through")
    );
}

#[test]
fn yaml_and_toml_landings_share_the_same_fact_contract() {
    let config = workspace::compile(&configuration(), Default::default()).unwrap();
    let text = r#"format: gazette-landing/v1
projects: [{project: 'project:one', kind: about}]
project:
  id: 'project:one'
  name: A project whose address can change
  lifecycle: paused
  visibility: public
  availability: gated
  repository: https://code.test/one
  license: MIT
content:
  id: 'content:one'
  site: 'site:one'
  title: The project
  lifecycle: draft
  route: {path: /one/}
  structure: {kind: page, family: {kind: project}}
sections:
  - heading: {level: 1, text: The project}
    body: Its facts have one owner.
"#;
    let yaml = ParsedCorpus::parse(
        [Input {
            path: "writing/one.landing.yaml".parse().unwrap(),
            bytes: text.as_bytes(),
        }],
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .unwrap();
    let yaml = authored::resolve(&config, &yaml, Default::default()).unwrap();
    let text = landing();
    let toml = parse(&text);
    let toml = authored::resolve(&config, &toml, Default::default()).unwrap();
    assert_eq!(
        yaml.content().project(&key("project:one")),
        toml.content().project(&key("project:one"))
    );
}

#[test]
fn missing_facts_unrouted_duplicate_and_wrong_source_landings_are_refused() {
    let config = workspace::compile(&configuration(), Default::default()).unwrap();
    let text = landing();
    let missing = text[..text.find("[project]").unwrap()].to_owned()
        + &text[text.find("[content]").unwrap()..];
    for (text, expected) in [
        (missing, "project_landing_facts_missing"),
        (
            text.replace("route={path='/one/'}", ""),
            "project_landing_owner",
        ),
        (
            text.replace(
                "[content]",
                "resources={landing='content:other'}\n[content]",
            ),
            "project_landing_conflict",
        ),
    ] {
        let corpus = parse(&text);
        let error = authored::resolve(&config, &corpus, Default::default()).unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
    }
    let duplicate = ParsedCorpus::parse(
        [
            Input {
                path: "writing/one.md".parse().unwrap(),
                bytes: text.as_bytes(),
            },
            Input {
                path: "writing/copy.md".parse().unwrap(),
                bytes: text.as_bytes(),
            },
        ],
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .unwrap();
    assert!(authored::resolve(&config, &duplicate, Default::default()).is_err());
    let other_owner = configuration().replace("target='writing'", "target='elsewhere'");
    let other_owner = workspace::compile(&other_owner, Default::default()).unwrap();
    let corpus = parse(&text);
    assert!(
        authored::resolve(&other_owner, &corpus, Default::default())
            .unwrap_err()
            .to_string()
            .contains("project_landing_source_owner")
    );
}

#[test]
fn primary_placement_and_captured_fact_home_must_agree() {
    let configuration = configuration()
        + r#"
[[sites]]
id='site:two'
publisher='person:publisher'
deployable=false
[[placements]]
role='primary'
site='site:two'
project='project:one'
render=false
discoverable=false
"#;
    let config = workspace::compile(&configuration, Default::default()).unwrap();
    let text = landing();
    let corpus = parse(&text);
    assert!(
        authored::resolve(&config, &corpus, Default::default())
            .unwrap_err()
            .to_string()
            .contains("project_landing_owner")
    );
}

#[test]
fn availability_relationship_requires_gated_state_and_an_exact_routed_site_target() {
    let config = workspace::compile(&configuration(), Default::default()).unwrap();
    for (state, site, content) in [
        ("available", "site:one", "content:one"),
        ("unavailable", "site:one", "content:one"),
        ("gated", "site:missing", "content:one"),
        ("gated", "site:one", "content:missing"),
    ] {
        let text = landing()
            .replace("availability='gated'", &format!("availability='{state}'"))
            .replace(
                "[content]",
                &format!("available_through={{site='{site}',content='{content}'}}\n[content]"),
            );
        let corpus = parse(&text);
        assert!(
            authored::resolve(&config, &corpus, Default::default())
                .unwrap_err()
                .to_string()
                .contains("project_availability_destination")
        );
    }
}

#[test]
fn learn_can_select_an_article_in_a_documentation_collection() {
    let config = workspace::compile(&configuration(), Default::default()).unwrap();
    let text = landing().replace("[content]", "resources={learn='content:guide'}\n[content]");
    let help = "+++\n[content]\nid='content:help'\nsite='site:one'\ntitle='Help'\nlifecycle='draft'\nroute={path='/help/'}\nstructure={kind='collection',purpose='documentation'}\n+++\n# Help\n";
    let guide = "+++\ncollections=['content:help']\n[content]\nid='content:guide'\nsite='site:one'\ntitle='Guide'\nlifecycle='draft'\nroute={path='/help/guide/'}\nstructure={kind='page',family={kind='article'}}\n+++\n# Guide\n";
    let corpus = ParsedCorpus::parse(
        [
            Input {
                path: "writing/one.md".parse().unwrap(),
                bytes: text.as_bytes(),
            },
            Input {
                path: "writing/help.md".parse().unwrap(),
                bytes: help.as_bytes(),
            },
            Input {
                path: "writing/guide.md".parse().unwrap(),
                bytes: guide.as_bytes(),
            },
        ],
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .unwrap();
    let resolved = authored::resolve(&config, &corpus, Default::default()).unwrap();
    let actions = resolved
        .project_resources_for(&key("content:one"))
        .unwrap()
        .unwrap()
        .actions;
    assert_eq!(
        actions[0].kind,
        gazette_core::markdown::ProjectResourceKind::Learn
    );
    assert_eq!(actions[0].url, "https://one.test/help/guide/");
}

#[test]
fn a_programme_can_own_its_facts_on_the_exact_configured_publication_home() {
    let source =
        configuration().replace("public_base_url=", "home='content:one'\npublic_base_url=");
    let config = workspace::compile(&source, Default::default()).unwrap();
    let text = landing()
        .replace("family={kind='project'}", "family={kind='index'}")
        .replace("path='/one/'", "path='/'");
    let corpus = parse(&text);
    let resolved = authored::resolve(&config, &corpus, Default::default()).unwrap();
    let page = resolved
        .project_page(
            &key("content:one"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    assert!(matches!(
        page.data.content.structure,
        gazette_core::publication::content::ContentKind::Page {
            family: gazette_core::publication::content::PageFamily::Index
        }
    ));
    assert_eq!(
        page.data.projects[0].landing.as_ref().unwrap().url,
        "https://one.test/"
    );
    assert_eq!(
        page.data.project_resources.as_ref().unwrap().project,
        key("project:one")
    );
    let ordinary_index = workspace::compile(&configuration(), Default::default()).unwrap();
    assert!(
        authored::resolve(&ordinary_index, &corpus, Default::default())
            .unwrap_err()
            .to_string()
            .contains("project_landing_owner"),
        "a root address and About do not select a publication Home"
    );
}
