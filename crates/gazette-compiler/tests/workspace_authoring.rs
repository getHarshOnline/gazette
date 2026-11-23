//! Tolerant help derives from the strict workspace Rust schema without admitting source.

use gazette_compiler::{toml, workspace};

#[test]
fn incomplete_array_table_offers_only_missing_schema_fields() {
    let source = "[[sites]]\nid='site:one'\npublisher='owner:one'\ndep";
    let authoring = workspace::WorkspaceAuthoring::parse(source, toml::Limits::default())
        .expect("bounded tolerant facts");
    let help = authoring
        .help(source, source.len(), 1 << 18)
        .expect("current incomplete source help");
    assert_eq!(help.completion_kind, Some(workspace::CompletionKind::Field));
    assert_eq!(help.completion_preimage.as_deref(), Some("dep"));
    assert_eq!(
        help.candidates
            .iter()
            .map(|candidate| candidate.label.as_str())
            .collect::<Vec<_>>(),
        ["deployable"]
    );
    assert_eq!(
        help.candidates[0].presence,
        Some(workspace::AuthoringFieldPresence::Required)
    );
    assert!(
        help.candidates[0]
            .description
            .as_deref()
            .is_some_and(|value| value.contains("release eligibility"))
    );
}

#[test]
fn value_slots_use_exact_boolean_and_serde_enum_literals() {
    for (source, expected) in [
        ("[[sites]]\ndeployable = ", vec!["false", "true"]),
        (
            "[[projects]]\nlifecycle = ",
            vec!["active", "closed", "done", "paused", "planned"],
        ),
    ] {
        let authoring = workspace::WorkspaceAuthoring::parse(source, toml::Limits::default())
            .expect("bounded tolerant facts");
        let help = authoring
            .help(source, source.len(), 1 << 18)
            .expect("literal help");
        assert_eq!(
            help.completion_kind,
            Some(workspace::CompletionKind::Literal)
        );
        assert_eq!(
            help.candidates
                .iter()
                .map(|candidate| candidate.label.as_str())
                .collect::<Vec<_>>(),
            expected
        );
    }
}

#[test]
fn key_hover_is_generated_from_the_same_rust_field() {
    let source = "[[sites]]\ndeployable=false\n";
    let authoring = workspace::WorkspaceAuthoring::parse(source, toml::Limits::default())
        .expect("bounded tolerant facts");
    let offset = source.find("deployable").expect("key");
    let hover = authoring
        .help(source, offset, 1 << 18)
        .expect("schema hover")
        .hover
        .expect("key facts");
    let field = hover.field.expect("field facts");
    assert_eq!(field.name, "deployable");
    assert_eq!(field.presence, workspace::AuthoringFieldPresence::Required);
    assert_eq!(field.value_kinds, [workspace::AuthoringValueKind::Boolean]);
    assert!(
        field
            .description
            .as_deref()
            .is_some_and(|value| value.contains("release eligibility"))
    );
}

#[test]
fn resource_axis_help_comes_from_the_strict_schema_without_legacy_modules() {
    let source = "[composition]\nres";
    let authoring = workspace::WorkspaceAuthoring::parse(source, toml::Limits::default())
        .expect("bounded composition facts");
    let help = authoring
        .help(source, source.len(), 1 << 18)
        .expect("resource field help");
    assert_eq!(
        help.candidates
            .iter()
            .map(|candidate| candidate.label.as_str())
            .collect::<Vec<_>>(),
        ["resources"]
    );
    assert!(
        !help
            .candidates
            .iter()
            .any(|candidate| candidate.label == "modules")
    );

    let selection = serde_json::to_string(&schemars::schema_for!(workspace::ResourceSelection))
        .expect("resource selection schema");
    for value in ["use", "dependency", "site"] {
        assert!(
            selection.contains(value),
            "schema must expose {value}: {selection}"
        );
    }
}

#[test]
fn authoring_bounds_fail_closed_without_a_partial_candidate_prefix() {
    let source = "[[sites]]\ndeployable = ";
    let authoring = workspace::WorkspaceAuthoring::parse(source, toml::Limits::default())
        .expect("bounded tolerant facts");
    assert!(matches!(
        authoring.help(source, source.len(), 32),
        Err(workspace::AuthoringError::Limit("result bytes"))
    ));
    assert!(matches!(
        workspace::WorkspaceAuthoring::parse(
            source,
            toml::Limits {
                nodes: 1,
                ..Default::default()
            }
        ),
        Err(workspace::AuthoringError::Limit("structural facts"))
    ));
}

#[test]
fn analytics_field_completion_uses_the_current_native_binding_schema() {
    let schema = serde_json::to_value(schemars::schema_for!(workspace::AnalyticsSelection))
        .expect("native analytics schema");
    let description = schema["properties"]["measurement_id"]["description"]
        .as_str()
        .expect("native field documentation");
    let source = "[[brands]]\n[[brands.publications]]\n[brands.publications.processing.analytics]\nenabled=false\nprovider='google-analytics'\nmeas";
    let authoring = workspace::WorkspaceAuthoring::parse(source, toml::Limits::default())
        .expect("current tolerant publication facts");
    let help = authoring
        .help(source, source.len(), 1 << 18)
        .expect("native binding field help");
    assert_eq!(help.completion_kind, Some(workspace::CompletionKind::Field));
    assert_eq!(help.completion_preimage.as_deref(), Some("meas"));
    assert_eq!(
        help.completion_location
            .expect("exact replacement")
            .span
            .slice(source),
        Some("meas")
    );
    assert_eq!(help.candidates.len(), 1);
    let candidate = &help.candidates[0];
    assert_eq!(candidate.label, "measurement_id");
    assert_eq!(candidate.description.as_deref(), Some(description));
    assert_eq!(
        candidate.presence,
        Some(workspace::AuthoringFieldPresence::Optional)
    );
    assert_eq!(
        candidate.value_kinds,
        [workspace::AuthoringValueKind::String]
    );
}

#[test]
fn analytics_key_hover_uses_the_same_schema_in_table_and_inline_forms() {
    let schema = serde_json::to_value(schemars::schema_for!(workspace::AnalyticsSelection))
        .expect("native analytics schema");
    let description = schema["properties"]["measurement_id"]["description"]
        .as_str()
        .expect("native field documentation");
    for source in [
        "[[brands]]\n[[brands.publications]]\n[brands.publications.processing.analytics]\nenabled=false\nprovider='google-analytics'\nmeasurement_id='G-EXACT123'\n",
        "[[brands]]\n[[brands.publications]]\nprocessing={analytics={enabled=false,provider='google-analytics',measurement_id='G-EXACT123'}}\n",
    ] {
        let authoring = workspace::WorkspaceAuthoring::parse(source, toml::Limits::default())
            .expect("current tolerant publication facts");
        let hover = authoring
            .help(
                source,
                source.find("measurement_id").expect("binding key"),
                1 << 18,
            )
            .expect("native binding key hover")
            .hover
            .expect("key facts");
        assert_eq!(hover.location.span.slice(source), Some("measurement_id"));
        assert_eq!(
            hover.path.last(),
            Some(&workspace::AuthoringPathSegment::Field(
                "measurement_id".into()
            ))
        );
        let field = hover.field.expect("native field facts");
        assert_eq!(field.name, "measurement_id");
        assert_eq!(field.description.as_deref(), Some(description));
        assert_eq!(field.presence, workspace::AuthoringFieldPresence::Optional);
        assert_eq!(field.value_kinds, [workspace::AuthoringValueKind::String]);
        assert!(
            field.allowed_values.is_empty(),
            "help must not invent tag identities"
        );
    }
}

#[test]
fn webmcp_tool_literals_come_from_typed_array_items_in_table_and_inline_forms() {
    use gazette_core::publication::browser::WebMcpTool;

    let expected = [WebMcpTool::CurrentPage, WebMcpTool::Search]
        .map(|tool| serde_json::to_value(tool).expect("native tool literal"));
    for (source, preimage) in [
        (
            "[[brands]]\n[[brands.publications]]\n[brands.publications.processing.webmcp]\nenabled=false\ntools=['current_page']\n",
            "'current_page'",
        ),
        (
            "[[brands]]\n[[brands.publications]]\nprocessing={webmcp={enabled=false,tools=['search']}}\n",
            "'search'",
        ),
    ] {
        let authoring = workspace::WorkspaceAuthoring::parse(source, toml::Limits::default())
            .expect("current tolerant publication facts");
        let help = authoring
            .help(
                source,
                source.find(preimage).expect("tool scalar") + 1,
                1 << 18,
            )
            .expect("typed array item help");
        assert_eq!(
            help.completion_kind,
            Some(workspace::CompletionKind::Literal)
        );
        assert_eq!(help.completion_preimage.as_deref(), Some(preimage));
        assert_eq!(
            help.completion_location
                .expect("exact scalar replacement")
                .span
                .slice(source),
            Some(preimage)
        );
        assert_eq!(
            help.candidates
                .iter()
                .map(|candidate| candidate.label.as_str())
                .collect::<Vec<_>>(),
            ["current_page", "search"]
        );
        assert_eq!(
            help.candidates
                .iter()
                .map(
                    |candidate| serde_json::from_str::<serde_json::Value>(&candidate.replacement)
                        .expect("quoted native literal")
                )
                .collect::<Vec<_>>(),
            expected
        );
        let hover = help.hover.expect("array item schema facts");
        assert_eq!(hover.location.span.slice(source), Some(preimage));
        assert_eq!(
            hover.path.last(),
            Some(&workspace::AuthoringPathSegment::ArrayItem)
        );
        assert_eq!(
            hover.node.value_kinds,
            [workspace::AuthoringValueKind::String]
        );
        assert_eq!(hover.node.allowed_values.len(), expected.len());
        assert!(matches!(
            authoring.help(source, source.find(preimage).expect("tool scalar") + 1, 32),
            Err(workspace::AuthoringError::Limit("result bytes"))
        ));
    }
}
