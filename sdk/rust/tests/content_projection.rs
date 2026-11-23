//! Structural graph-to-HTML/JSON parity, not publication admission or a site build.

use std::{collections::BTreeMap, error::Error as StdError};

use gazette_adapter_tera::TemplateSet;
use gazette_core::source::{CapturedSource, Content, EntryPath, InventoryBuilder, Sha256Digest};
use gazette_sdk::publication::{
    ContentLifecycle, Key,
    content::{
        ContentKind, ContentNode, Declaration, EditorialRelation, Error, JourneyMember, PageFamily,
    },
};
use gazette_sdk::workspace;
use serde_json::{Value, json};

const WORKSPACE: &str = concat!(
    "[[principals]]\nid='owner:publisher'\nkind='person'\n",
    "[[sites]]\nid='site:home'\npublisher='owner:publisher'\ndeployable=false\n",
    "[[sites]]\nid='site:other'\npublisher='owner:publisher'\ndeployable=true\n"
);

const TEMPLATE: &str = concat!(
    "<section data-id=\"{{ journey.id }}\" data-site=\"{{ journey.site }}\" ",
    "data-state=\"{{ journey.lifecycle }}\"><h1>{{ journey.title }}</h1><ol>",
    "{% for step in steps %}<li data-id=\"{{ step.id }}\" ",
    "data-site=\"{{ step.site }}\" data-state=\"{{ step.lifecycle }}\">",
    "{{ step.title }}</li>{% endfor %}</ol></section>"
);

fn key(value: &str) -> Key {
    value.parse().expect("fixture key")
}

fn declaration() -> Declaration {
    let page = |id: &str, site: &str, title: &str, lifecycle| ContentNode {
        id: key(id),
        site: key(site),
        title: title.into(),
        description: None,
        keywords: vec![],
        schema_type: None,
        subject: None,
        license: None,
        citation: None,
        published: None,
        modified: None,
        lifecycle,
        attributions: vec![],
        language: None,
        taxonomies: vec![],
        slug: None,
        route: None,
        structure: ContentKind::Page {
            family: PageFamily::Article,
        },
    };
    Declaration {
        browsing: vec![],
        nodes: vec![
            page(
                "content:alpha",
                "site:home",
                "Shared <title>",
                ContentLifecycle::Draft,
            ),
            ContentNode {
                id: key("journey:main"),
                site: key("site:home"),
                title: "A < B & Ω".into(),
                description: None,
                keywords: vec![],
                schema_type: None,
                subject: None,
                license: None,
                citation: None,
                published: None,
                modified: None,
                lifecycle: ContentLifecycle::Published,
                attributions: vec![],
                language: None,
                taxonomies: vec![],
                slug: None,
                route: None,
                structure: ContentKind::Journey {
                    collection: Some(key("collection:main")),
                    members: ["content:mu", "content:Zeta", "content:alpha"]
                        .map(|content| JourneyMember {
                            content: key(content),
                            primary: true,
                        })
                        .into(),
                },
            },
            page(
                "content:mu",
                "site:home",
                "古い & kept",
                ContentLifecycle::Archived,
            ),
            page(
                "content:Zeta",
                "site:home",
                "Shared <title>",
                ContentLifecycle::Published,
            ),
            {
                let mut collection = page(
                    "collection:main",
                    "site:home",
                    "Containing inquiry",
                    ContentLifecycle::Archived,
                );
                collection.structure = ContentKind::Collection {
                    parent: None,
                    purpose: None,
                    navigation: None,
                    member_order: vec![],
                };
                collection
            },
            page(
                "content:foreign",
                "site:other",
                "Related foreign work",
                ContentLifecycle::Draft,
            ),
        ],
        memberships: vec![],
        related: vec![EditorialRelation {
            source: key("content:alpha"),
            target: key("content:foreign"),
        }],
        projects: vec![],
    }
}

fn captured(template: &str) -> CapturedSource {
    let path: EntryPath = "journey.html".parse().expect("template name");
    let mut inventory = InventoryBuilder::new(Default::default());
    inventory
        .insert(
            path.clone(),
            Content::File {
                bytes: template.len() as u64,
                sha256: Sha256Digest::of(template.as_bytes()),
            },
        )
        .expect("template inventory");
    CapturedSource::from_parts(
        inventory.finish(),
        BTreeMap::from([(path, template.as_bytes().to_vec())]),
    )
    .expect("captured template")
}

// Exercise real SDK workspace/content joins, serialization and adapter entrypoints in order.
// This test composition is not an application admission or production build API.
fn project(input: Declaration, template: &str) -> Result<(Vec<u8>, Vec<u8>), Box<dyn StdError>> {
    let compiled = workspace::compile(WORKSPACE, Default::default())?;
    let joined = compiled.resolve_content(input, Default::default())?;
    assert_eq!(
        joined
            .graph()
            .editorial_parent(&key("journey:main"))
            .map(|node| &node.id),
        Some(&key("collection:main"))
    );
    assert_eq!(
        joined
            .graph()
            .primary_journey(&key("content:alpha"))
            .map(|node| &node.id),
        Some(&key("journey:main"))
    );
    let related = joined
        .graph()
        .related()
        .first()
        .expect("cross-site editorial reference");
    assert_ne!(
        joined.graph().node(&related.source).expect("source").site,
        joined.graph().node(&related.target).expect("target").site
    );
    let view = joined
        .graph()
        .journey_view(&key("journey:main"))
        .expect("fixture journey");
    let context = serde_json::to_value(&view)?;
    let machine = serde_json::to_vec(&view)?;
    let templates = TemplateSet::compile(&captured(template), Default::default())?;
    let html = templates.render(&"journey.html".parse()?, &context)?;
    Ok((html, machine))
}

#[test]
fn unknown_home_site_rejects_the_join_before_template_compilation() {
    let mut input = declaration();
    input
        .nodes
        .iter_mut()
        .find(|node| node.id == key("content:foreign"))
        .expect("standalone related target")
        .site = key("site:absent");
    let error = project(input, "{% if %}").expect_err("no partial join or rendered output");
    let Some(Error::Invalid(diagnostics)) = error.downcast_ref::<Error>() else {
        panic!("workspace/content failure must precede Tera: {error}");
    };
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code, "content_site_reference");
    assert_eq!(diagnostics[0].source, key("content:foreign"));
    assert_eq!(diagnostics[0].target, Some(key("site:absent")));
}

#[test]
fn journey_view_preserves_exact_content_in_html_and_json_without_state_filtering() {
    let (html, machine) = project(declaration(), TEMPLATE).expect("complete structural projection");
    assert_eq!(
        String::from_utf8(html.clone()).expect("UTF-8 HTML"),
        concat!(
            "<section data-id=\"journey:main\" data-site=\"site:home\" data-state=\"published\">",
            "<h1>A &lt; B &amp; Ω</h1><ol>",
            "<li data-id=\"content:mu\" data-site=\"site:home\" data-state=\"archived\">古い &amp; kept</li>",
            "<li data-id=\"content:Zeta\" data-site=\"site:home\" data-state=\"published\">Shared &lt;title&gt;</li>",
            "<li data-id=\"content:alpha\" data-site=\"site:home\" data-state=\"draft\">Shared &lt;title&gt;</li>",
            "</ol></section>"
        )
    );
    let decoded: Value = serde_json::from_slice(&machine).expect("complete JSON");
    assert_eq!(decoded["journey"]["id"], "journey:main");
    assert_eq!(decoded["journey"]["title"], "A < B & Ω");
    assert_eq!(decoded["journey"]["site"], "site:home");
    assert_eq!(decoded["journey"]["lifecycle"], "published");
    let rows: Vec<_> = decoded["steps"]
        .as_array()
        .expect("ordered steps")
        .iter()
        .map(|step| json!([step["id"], step["site"], step["title"], step["lifecycle"]]))
        .collect();
    assert_eq!(
        rows,
        vec![
            json!(["content:mu", "site:home", "古い & kept", "archived"]),
            json!(["content:Zeta", "site:home", "Shared <title>", "published"]),
            json!(["content:alpha", "site:home", "Shared <title>", "draft"]),
        ]
    );
    let declared_members = decoded["journey"]["structure"]["members"]
        .as_array()
        .expect("declared sequence")
        .iter()
        .map(|member| &member["content"])
        .collect::<Vec<_>>();
    let resolved_members = decoded["steps"]
        .as_array()
        .expect("resolved sequence")
        .iter()
        .map(|step| &step["id"])
        .collect::<Vec<_>>();
    assert_eq!(declared_members, resolved_members);

    let mut reordered = declaration();
    reordered.nodes.reverse();
    assert_eq!(
        (html, machine),
        project(reordered, TEMPLATE).expect("declaration order independence")
    );
}

#[test]
fn invalid_references_reject_the_whole_graph_before_template_compilation() {
    let invalid_template = "{% if %}";
    // Control: rendering input really is invalid, so a graph success reaches and fails Tera.
    let renderer_error = project(declaration(), invalid_template).expect_err("invalid template");
    assert!(
        renderer_error
            .downcast_ref::<gazette_adapter_tera::Error>()
            .is_some()
    );

    for (target, expected_code) in [
        ("content:absent", "content_reference"),
        ("collection:main", "content_reference_kind"),
    ] {
        let mut input = declaration();
        let journey = input
            .nodes
            .iter_mut()
            .find(|node| node.id == key("journey:main"))
            .expect("journey declaration");
        let ContentKind::Journey { members, .. } = &mut journey.structure else {
            panic!("fixture must be a journey");
        };
        members[1].content = key(target);
        let error = project(input, invalid_template).expect_err("no partial graph/output");
        let Some(Error::Invalid(diagnostics)) = error.downcast_ref::<Error>() else {
            panic!("graph failure must precede the known template failure: {error}");
        };
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, expected_code);
        assert_eq!(diagnostics[0].source, key("journey:main"));
        assert_eq!(diagnostics[0].target, Some(key(target)));
    }
}
