//! Authored project-evidence projection through shared resolvers, not JSON-LD or admission.

use std::{collections::BTreeMap, error::Error as StdError};

use gazette_adapter_tera::TemplateSet;
use gazette_core::source::{CapturedSource, Content, EntryPath, InventoryBuilder, Sha256Digest};
use gazette_sdk::{
    authored, corpus,
    publication::{Key, content},
    toml::PathSegment,
    workspace,
};
use serde_json::{Value, json};

const WORKSPACE: &str = concat!(
    "[[principals]]\nid='owner:one'\nkind='person'\n",
    "[[sites]]\nid='site:home'\npublisher='owner:one'\ndeployable=false\n",
    "[[projects]]\nid='project:p'\nlifecycle='done'\nvisibility='private'\navailability='unavailable'\n",
    "[[projects]]\nid='project:q'\nlifecycle='planned'\nvisibility='public'\navailability='gated'\n"
);
const PAGE: &str = "+++\nprojects=[{project='project:q',kind='mentions'},{project='project:p',kind='about'}]\n[content]\nid='article:one'\nsite='site:home'\ntitle='Projects <&>'\nlifecycle='draft'\nstructure={kind='page',family={kind='article'}}\n+++\n# Body\n";
const ALPHA: &str = "+++\nprojects=[{project='project:p',kind='context'}]\n[content]\nid='collection:a'\nsite='site:home'\ntitle='Alpha'\nlifecycle='published'\nstructure={kind='collection',parent='collection:b'}\n+++\n# Alpha\n";
const BETA: &str = "+++\nprojects=[{project='project:q',kind='context'}]\n[content]\nid='collection:b'\nsite='site:home'\ntitle='Beta'\nlifecycle='archived'\nstructure={kind='collection'}\n+++\n# Beta\n";
const JOURNEY: &str = "+++\nprojects=[{project='project:p',kind='context'}]\n[content]\nid='journey:j'\nsite='site:home'\ntitle='Journey'\nlifecycle='published'\nstructure={kind='journey',collection='collection:a',members=[{content='article:one',primary=true}]}\n+++\n# Journey\n";
const EMPTY: &str = "+++\n[content]\nid='article:empty'\nsite='site:home'\ntitle='No projects'\nlifecycle='archived'\nstructure={kind='page',family={kind='documentation'}}\n+++\n# Retained\n";
const UNUSED_CONTEXT: &str = "+++\nprojects=[{project='project:p',kind='context'}]\n[content]\nid='collection:unused'\nsite='site:home'\ntitle='No descendants'\nlifecycle='archived'\nstructure={kind='collection'}\n+++\n# Empty collection\n";
const TEMPLATE: &str = concat!(
    "<main data-id=\"{{ content.id }}\" data-state=\"{{ content.lifecycle }}\"><h1>{{ content.title }}</h1><ul>",
    "{% for link in projects %}<li data-project=\"{{ link.project.id }}\" data-relation=\"{{ link.relation }}\">",
    "{{ link.project.lifecycle }}/{{ link.project.visibility }}/{{ link.project.availability }}",
    "{% for evidence in link.evidence %}<span>{{ evidence.source }}:{{ evidence.kind }}</span>{% endfor %}",
    "</li>{% endfor %}</ul></main>"
);

fn key(value: &str) -> Key {
    value.parse().expect("fixture identity")
}

fn sources() -> [(&'static str, &'static [u8]); 5] {
    [
        ("page.md", PAGE.as_bytes()),
        ("beta.md", BETA.as_bytes()),
        ("empty.md", EMPTY.as_bytes()),
        ("journey.md", JOURNEY.as_bytes()),
        ("alpha.md", ALPHA.as_bytes()),
    ]
}

fn parse<'s>(sources: &[(&str, &'s [u8])]) -> corpus::ParsedCorpus<'s> {
    corpus::ParsedCorpus::parse(
        sources.iter().map(|(path, bytes)| corpus::Input {
            path: path.parse().expect("logical path"),
            bytes,
        }),
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("retained authored syntax")
}

fn templates(template: &str) -> Result<TemplateSet, gazette_adapter_tera::Error> {
    let path: EntryPath = "projects.html".parse().expect("entry template");
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
    let source = CapturedSource::from_parts(
        inventory.finish(),
        BTreeMap::from([(path, template.as_bytes().to_vec())]),
    )
    .expect("captured template");
    TemplateSet::compile(&source, Default::default())
}

// Fixture-only composition: project relationships come solely from the resolved graph.
fn project(
    corpus: &corpus::ParsedCorpus<'_>,
    template: &str,
) -> Result<(Vec<u8>, Vec<u8>), Box<dyn StdError>> {
    let compilation = workspace::compile(WORKSPACE, Default::default())?;
    let resolved = authored::resolve(&compilation, corpus, content::Limits::default())?;
    let graph = resolved.content().graph();
    let page = key("article:one");
    let projects = graph
        .project_relations(&page)
        .expect("known page")
        .iter()
        .map(|(id, link)| {
            let project = compilation
                .workspace
                .project(id)
                .expect("exact compiler-validated project");
            json!({"project":project,"relation":link.relation(),"evidence":link.evidence()})
        })
        .collect::<Vec<_>>();
    let context = json!({"content":graph.node(&page).expect("page"),"projects":projects});
    let machine = serde_json::to_vec(&context)?;
    let html = templates(template)?.render(&"projects.html".parse()?, &context)?;
    Ok((html, machine))
}

#[test]
fn all_editorial_ancestry_evidence_and_project_states_reach_both_projections() {
    let files = sources();
    let corpus = parse(&files);
    let compilation = workspace::compile(WORKSPACE, Default::default()).expect("workspace");
    let resolved = authored::resolve(&compilation, &corpus, content::Limits::default())
        .expect("whole authored graph");
    let graph = resolved.content().graph();
    assert!(
        graph.memberships().is_empty(),
        "journey members have no second collection residence"
    );
    assert_eq!(
        graph
            .editorial_ancestry(&key("article:one"))
            .expect("known article")
            .map(|node| node.id.as_str())
            .collect::<Vec<_>>(),
        ["article:one", "journey:j", "collection:a", "collection:b"]
    );
    let links = graph
        .project_relations(&key("article:one"))
        .expect("page links");
    assert_eq!(links.len(), 2);
    assert_eq!(
        links[&key("project:p")].relation(),
        content::ProjectRelation::About
    );
    assert_eq!(links[&key("project:p")].evidence().len(), 3);
    assert_eq!(
        links[&key("project:q")].relation(),
        content::ProjectRelation::Mentions
    );
    assert_eq!(links[&key("project:q")].evidence().len(), 2);
    assert!(
        graph
            .project_relations(&key("article:empty"))
            .expect("known empty page")
            .is_empty()
    );
    assert!(graph.project_relations(&key("collection:a")).is_none());
    assert!(graph.project_relations(&key("journey:j")).is_none());
    assert!(graph.project_relations(&key("article:absent")).is_none());
    let origin = resolved
        .origin(
            &key("article:one"),
            &[
                PathSegment::Key("projects".into()),
                PathSegment::Index(0),
                PathSegment::Key("project".into()),
            ],
        )
        .expect("original unsorted declaration");
    let span = origin.span.expect("project value range");
    assert_eq!(&PAGE[span.start..span.end], "'project:q'");
    assert_eq!(origin.path.as_str(), "page.md");
    assert_eq!(origin.source_sha256, Sha256Digest::of(PAGE.as_bytes()));

    let (html, machine) = project(&corpus, TEMPLATE).expect("shared project projection");
    assert_eq!(
        String::from_utf8(html.clone()).expect("HTML"),
        concat!(
            "<main data-id=\"article:one\" data-state=\"draft\"><h1>Projects &lt;&amp;&gt;</h1><ul>",
            "<li data-project=\"project:p\" data-relation=\"about\">done/private/unavailable",
            "<span>article:one:about</span><span>collection:a:context</span><span>journey:j:context</span></li>",
            "<li data-project=\"project:q\" data-relation=\"mentions\">planned/public/gated",
            "<span>article:one:mentions</span><span>collection:b:context</span></li></ul></main>"
        )
    );
    let decoded: Value = serde_json::from_slice(&machine).expect("JSON");
    assert_eq!(decoded["content"]["id"], "article:one");
    assert_eq!(decoded["content"]["title"], "Projects <&>");
    assert_eq!(decoded["content"]["lifecycle"], "draft");
    assert_eq!(
        decoded["projects"],
        json!([
            {"project":{"id":"project:p","lifecycle":"done","visibility":"private","availability":"unavailable"},"relation":"about","evidence":[
                {"source":"article:one","kind":"about"},
                {"source":"collection:a","kind":"context"},
                {"source":"journey:j","kind":"context"}
            ]},
            {"project":{"id":"project:q","lifecycle":"planned","visibility":"public","availability":"gated"},"relation":"mentions","evidence":[
                {"source":"article:one","kind":"mentions"},
                {"source":"collection:b","kind":"context"}
            ]}
        ])
    );
    let reordered_page = PAGE.replace(
        "[{project='project:q',kind='mentions'},{project='project:p',kind='about'}]",
        "[{project='project:p',kind='about'},{project='project:q',kind='mentions'}]",
    );
    let mut reordered = files;
    reordered[0].1 = reordered_page.as_bytes();
    reordered.reverse();
    assert_eq!(
        (html, machine),
        project(&parse(&reordered), TEMPLATE).expect("order independent union")
    );
}

#[test]
fn exact_unknown_project_keys_fail_before_rendering_with_original_source_identity() {
    assert!(
        project(&parse(&sources()), "{% if %}")
            .expect_err("Tera control")
            .downcast_ref::<gazette_adapter_tera::Error>()
            .is_some()
    );
    for wrong in ["project:missing", "project:P", "owner:one"] {
        let changed = ALPHA.replace("project:p", wrong);
        let mut files = sources();
        files[4].1 = changed.as_bytes();
        let corpus = parse(&files);
        let error = project(&corpus, "{% if %}").expect_err("no output for unresolved project");
        let Some(authored::Error::Structure {
            error: content::Error::Invalid(diagnostics),
            declarations,
        }) = error.downcast_ref::<authored::Error>()
        else {
            panic!("workspace project failure must precede Tera: {error}");
        };
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "content_project_reference");
        assert_eq!(diagnostics[0].source, key("collection:a"));
        assert_eq!(diagnostics[0].target, Some(key(wrong)));
        let origins = &declarations[&key("collection:a")];
        assert_eq!(origins.len(), 1);
        assert_eq!(origins[0].path.as_str(), "alpha.md");
        assert_eq!(
            origins[0].source_sha256,
            Sha256Digest::of(changed.as_bytes())
        );
        let span = origins[0].span.expect("declaring metadata span");
        assert!(changed[span.start..span.end].contains(wrong));
    }
}

#[test]
fn unused_context_still_validates_project_and_site_and_aggregates_exact_origins() {
    let mut valid = sources().to_vec();
    valid.push(("unused.md", UNUSED_CONTEXT.as_bytes()));
    let valid_corpus = parse(&valid);
    let compilation = workspace::compile(WORKSPACE, Default::default()).expect("workspace");
    let resolved = authored::resolve(&compilation, &valid_corpus, content::Limits::default())
        .expect("valid unused context");
    assert!(
        resolved
            .content()
            .graph()
            .declared_projects()
            .iter()
            .any(|declaration| {
                declaration.content == key("collection:unused")
                    && declaration.project == key("project:p")
                    && declaration.kind == content::ProjectDeclarationKind::Context
            })
    );
    assert_eq!(
        project(&valid_corpus, TEMPLATE).expect("unused context"),
        project(&parse(&sources()), TEMPLATE).expect("same rendered page")
    );

    for missing_site in [false, true] {
        let mut changed = UNUSED_CONTEXT.replace("project:p", "project:missing");
        if missing_site {
            changed = changed.replace("site:home", "site:missing");
        }
        let mut files = sources().to_vec();
        files.push(("unused.md", changed.as_bytes()));
        let mut expected = vec![content::Diagnostic {
            code: "content_project_reference",
            source: key("collection:unused"),
            target: Some(key("project:missing")),
        }];
        if missing_site {
            expected.push(content::Diagnostic {
                code: "content_site_reference",
                source: key("collection:unused"),
                target: Some(key("site:missing")),
            });
        }
        for reverse in [false, true] {
            if reverse {
                files.reverse();
            }
            let error = project(&parse(&files), "{% if %}")
                .expect_err("unused declaration cannot escape validation");
            let Some(authored::Error::Structure {
                error: content::Error::Invalid(diagnostics),
                declarations,
            }) = error.downcast_ref::<authored::Error>()
            else {
                panic!("declaration validation must precede Tera: {error}");
            };
            assert_eq!(diagnostics, &expected, "stable complete diagnostic order");
            let origins = &declarations[&key("collection:unused")];
            assert_eq!(origins.len(), 1);
            assert_eq!(origins[0].path.as_str(), "unused.md");
            assert_eq!(
                origins[0].source_sha256,
                Sha256Digest::of(changed.as_bytes())
            );
            let span = origins[0].span.expect("original metadata range");
            assert_eq!(
                &changed[span.start..span.end],
                changed
                    .strip_prefix("+++\n")
                    .expect("opening fence")
                    .split_once("+++\n")
                    .expect("closing fence")
                    .0
            );
        }
    }
}

#[test]
fn malformed_project_entries_fail_the_strict_profile_with_exact_source_identity() {
    let original = "projects=[{project='project:p',kind='context'}]";
    for malformed in [
        "projects=[{kind='context'}]",
        "projects=[{project='project:p',kind='invented'}]",
        "projects=[{project='project:p',kind='context',unexpected=true}]",
        "projects=[{project='project:p',kind=42}]",
    ] {
        let changed = UNUSED_CONTEXT.replace(original, malformed);
        let mut files = sources().to_vec();
        files.push(("unused.md", changed.as_bytes()));
        let error = project(&parse(&files), "{% if %}")
            .expect_err("malformed project entry cannot become partial success");
        let Some(authored::Error::Files(failures)) = error.downcast_ref::<authored::Error>() else {
            panic!("strict schema must reject before graph/render: {error}");
        };
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].origin.path.as_str(), "unused.md");
        assert_eq!(
            failures[0].origin.source_sha256,
            Sha256Digest::of(changed.as_bytes())
        );
        assert!(matches!(
            failures[0].error,
            authored::FileError::Metadata(_)
        ));
        if let Some(span) = failures[0].origin.span {
            assert!(
                changed.get(span.start..span.end).is_some(),
                "original byte range"
            );
        }
    }
}
