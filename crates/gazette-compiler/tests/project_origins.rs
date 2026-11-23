//! Explicit project lineage through the native workspace parser and editor coordinates.

use gazette_compiler::{toml::PathSegment, workspace};

const BASE: &str = "[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n";

fn project(id: &str, origin: &str) -> String {
    format!(
        "[[projects]]\nid='project:{id}'\nlifecycle='active'\nvisibility='public'\navailability='unavailable'\n{origin}\n"
    )
}

#[test]
fn declared_origins_preserve_distinct_meaning_and_exact_upstream_navigation() {
    let input = format!(
        "{BASE}{}{}{}{}",
        project("original", "origin={kind='original'}"),
        project(
            "fork",
            "origin={kind='successor-fork',upstream='project:original'}"
        ),
        project(
            "mirror",
            "origin={kind='convenience-mirror',upstream='project:fork'}"
        ),
        project(
            "unspecified",
            "repository='https://code.example/owner/fork'"
        )
    );
    let parsed = workspace::ParsedWorkspace::parse(&input, Default::default())
        .expect("explicit lineage is a native workspace declaration");
    for (id, expected) in [
        ("original", serde_json::json!({"kind":"original"})),
        (
            "fork",
            serde_json::json!({"kind":"successor-fork","upstream":"project:original"}),
        ),
        (
            "mirror",
            serde_json::json!({"kind":"convenience-mirror","upstream":"project:fork"}),
        ),
    ] {
        let key = format!("project:{id}").parse().expect("key");
        let value = serde_json::to_value(parsed.compilation().workspace.project(&key)).unwrap();
        assert_eq!(value["origin"], expected);
        assert_eq!(value["lifecycle"], "active");
        assert_eq!(value["availability"], "unavailable");
    }
    let key = "project:unspecified".parse().unwrap();
    let value = serde_json::to_value(parsed.compilation().workspace.project(&key)).unwrap();
    assert!(
        value.get("origin").is_none(),
        "a fork-looking URL proves nothing"
    );

    let key = "project:fork".parse().unwrap();
    let origin = parsed
        .origin(&workspace::FieldQuery {
            kind: workspace::DeclarationKind::Project,
            identity: key,
            field: vec![
                PathSegment::Key("origin".into()),
                PathSegment::Key("upstream".into()),
            ],
        })
        .expect("source-addressable upstream field");
    assert_eq!(
        &input[origin.span.start..origin.span.end],
        "'project:original'"
    );
    let slot = parsed
        .navigation()
        .reference_at(origin.span.start)
        .expect("typed editor reference");
    let target = slot.symbol.expect("resolved upstream");
    assert_eq!(target.name(), "project:original");
    assert_eq!(
        slot.kind,
        workspace::WorkspaceSymbolKind::Declaration(workspace::DeclarationKind::Project)
    );
    assert_eq!(slot.candidates().count(), 4);
    let association = parsed
        .navigation()
        .at(origin.span.start)
        .expect("definition and references");
    assert_eq!(association.references.len(), 1);
    assert_eq!(association.definition, target.location());

    let changed = input.replace(
        "https://code.example/owner/fork",
        "https://elsewhere.example/mirror",
    );
    assert_eq!(
        serde_json::to_value(
            workspace::compile(&changed, Default::default())
                .unwrap()
                .workspace
                .project(&"project:fork".parse().unwrap())
        )
        .unwrap(),
        serde_json::to_value(
            parsed
                .compilation()
                .workspace
                .project(&"project:fork".parse().unwrap())
        )
        .unwrap(),
        "remote changes cannot reclassify another project or its lineage"
    );
}

#[test]
fn origin_schema_rejects_ambiguous_or_invented_shapes() {
    for origin in [
        "origin={kind='original',upstream='project:root'}",
        "origin={kind='successor-fork'}",
        "origin={kind='convenience-mirror'}",
        "origin={kind='client'}",
        "origin={kind='original',owner='owner:one'}",
        "origin={kind='successor-fork',upstream=['project:root']}",
        "origin={kind='successor-fork',upstream='project:root',upstream='project:root'}",
    ] {
        let input = format!("{BASE}{}{}", project("root", ""), project("one", origin));
        assert!(
            matches!(
                workspace::compile(&input, Default::default()),
                Err(workspace::Error::Toml(_))
            ),
            "must reject {origin}"
        );
    }
}

#[test]
fn lineage_reference_and_cycles_fail_before_any_resolved_workspace() {
    for (origin, code) in [
        (
            "origin={kind='successor-fork',upstream='project:missing'}",
            "workspace_reference",
        ),
        (
            "origin={kind='convenience-mirror',upstream='owner:one'}",
            "workspace_reference",
        ),
        (
            "origin={kind='successor-fork',upstream='project:one'}",
            "workspace_project_lineage_cycle",
        ),
    ] {
        let input = format!("{BASE}{}", project("one", origin));
        let workspace::Error::Invalid(errors) =
            workspace::compile(&input, Default::default()).expect_err("invalid lineage")
        else {
            panic!("semantic refusal")
        };
        assert!(
            errors
                .iter()
                .any(|error| error.code == code && error.field.contains("origin.upstream")),
            "{errors:?}"
        );
    }
    let a = project("a", "origin={kind='successor-fork',upstream='project:b'}");
    let b = project(
        "b",
        "origin={kind='convenience-mirror',upstream='project:a'}",
    );
    let mut outcomes = Vec::new();
    for input in [format!("{BASE}{a}{b}"), format!("{BASE}{b}{a}")] {
        let workspace::Error::Invalid(errors) =
            workspace::compile(&input, Default::default()).expect_err("cycle")
        else {
            panic!("semantic refusal")
        };
        assert_eq!(
            errors
                .iter()
                .filter(|error| error.code == "workspace_project_lineage_cycle")
                .count(),
            2
        );
        outcomes.push(errors);
    }
    assert_eq!(
        outcomes[0], outcomes[1],
        "declaration order cannot choose a cycle winner"
    );
}

#[test]
fn shared_upstream_and_long_chains_are_not_duplicate_or_recursive_hierarchies() {
    let mut input = format!("{BASE}{}", project("root", "origin={kind='original'}"));
    for index in 0..192 {
        let upstream = if index == 0 {
            "root".to_owned()
        } else {
            (index - 1).to_string()
        };
        input.push_str(&project(
            &index.to_string(),
            &format!("origin={{kind='successor-fork',upstream='project:{upstream}'}}"),
        ));
    }
    input.push_str(&project(
        "mirror",
        "origin={kind='convenience-mirror',upstream='project:root'}",
    ));
    let compiled = workspace::compile(&input, Default::default())
        .expect("bounded iterative lineage resolution");
    assert_eq!(compiled.workspace.projects().count(), 194);
    assert_eq!(
        compiled.workspace.placements().len(),
        0,
        "lineage never creates site placement"
    );
}
