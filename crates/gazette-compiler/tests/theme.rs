//! Theme-owned component contracts use the Theme-package parser and retained TOML tree.

use gazette_compiler::{
    theme::{self, ComponentMount, Components},
    theme_package::{self, ParsedManifest},
    toml::{self, PathSegment},
};
use gazette_core::{
    markdown::{ComponentHost, ComponentSurface, Directive},
    publication::{Key, content::PageFamily},
    source::{Location, Precision, Span},
};

fn parse(source: &str) -> Result<ParsedManifest<'_>, theme_package::Error> {
    ParsedManifest::parse(source, toml::Limits::default())
}

#[test]
fn component_header_slots_share_exact_whitespace_and_partial_name_boundaries() {
    for (info, name, range, arguments) in [
        ("callout {tone='note'}  ", "callout", 0..7, "{tone='note'}"),
        ("  partial", "partial", 2..9, ""),
        ("   ", "", 3..3, ""),
        ("é-invalid {}", "é-invalid", 0..10, "{}"),
    ] {
        let directive = Directive {
            location: Location {
                span: Span {
                    start: 0,
                    end: info.len(),
                },
                precision: Precision::Construct,
            },
            header: Location {
                span: Span {
                    start: 0,
                    end: info.len(),
                },
                precision: Precision::Exact,
            },
            info: info.into(),
            emitted: true,
        };
        let actual = Components::header(&directive);
        assert_eq!(actual.name, name);
        assert_eq!(actual.name_range, range);
        assert_eq!(actual.arguments, arguments);
    }
}

#[test]
fn component_interaction_is_explicit_exact_and_retains_the_static_template() {
    let source = "package='theme:one'\nnamespace='one'\ntemplates='templates'\nrender={page='page.html'}\n[components.counter]\ntemplate='counter.html'\ninteraction={module='module:counter',export='mountCounter',mount='load'}\n";
    let manifest = parse(source).expect("explicit interaction Theme package");
    let definitions = manifest.components();
    let name = "counter".parse().expect("component name");
    let definition = definitions.definition(&name).expect("definition");
    assert_eq!(definition.template.as_str(), "counter.html");
    let interaction = definition.interaction.as_ref().expect("explicit intent");
    assert_eq!(interaction.module, "module:counter".parse().unwrap());
    assert_eq!(interaction.export.as_str(), "mountCounter");
    assert_eq!(interaction.mount, ComponentMount::Load);
    let span = manifest
        .syntax()
        .span_at(&[
            PathSegment::Key("components".into()),
            PathSegment::Key("counter".into()),
            PathSegment::Key("interaction".into()),
            PathSegment::Key("export".into()),
        ])
        .expect("original package declaration origin");
    assert_eq!(&source[span.start..span.end], "'mountCounter'");

    let static_source = source.replace(
        "interaction={module='module:counter',export='mountCounter',mount='load'}\n",
        "",
    );
    assert!(
        parse(&static_source)
            .expect("static default")
            .components()
            .definition(&name)
            .unwrap()
            .interaction
            .is_none()
    );
    assert!(parse(&source.replace("template='counter.html'\n", "")).is_err());
    for invalid in [
        "{module='module:counter',mount='load'}",
        "{module='module:counter',export='mountCounter'}",
        "{module='counter',export='mountCounter',mount='load'}",
        "{module='module:counter',export='mountCounter',mount='visible'}",
        "{module='module:counter',export='mount-counter',mount='load'}",
        "{module='module:counter',export='mountCounter',mount='load',framework='react'}",
    ] {
        assert!(
            parse(&source.replace(
                "{module='module:counter',export='mountCounter',mount='load'}",
                invalid
            ))
            .is_err(),
            "{invalid}"
        );
    }
}

#[test]
fn component_export_and_optional_interaction_schema_match_native_admission() {
    use serde_json::json;
    use theme::{Component, ComponentExport};

    let schema = serde_json::to_value(schemars::schema_for!(ComponentExport)).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    for (value, accepted) in [
        ("mount".to_owned(), true),
        ("$mount_2".to_owned(), true),
        ("default".to_owned(), true),
        ("a".repeat(128), true),
        ("a".repeat(129), false),
        (String::new(), false),
        ("2mount".to_owned(), false),
        ("mount-counter".to_owned(), false),
        ("mount\n".to_owned(), false),
        ("mount\r\n".to_owned(), false),
        ("mount\u{2028}".to_owned(), false),
        ("mounté".to_owned(), false),
        ("module:mount".to_owned(), false),
    ] {
        assert_eq!(
            value.parse::<ComponentExport>().is_ok(),
            accepted,
            "{value:?}"
        );
        assert_eq!(
            serde_json::from_value::<ComponentExport>(json!(value)).is_ok(),
            accepted,
            "serde {value:?}"
        );
        assert_eq!(
            validator.is_valid(&json!(value)),
            accepted,
            "schema {value:?}"
        );
    }

    let schema = serde_json::to_value(schemars::schema_for!(Component)).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    for (value, accepted) in [
        (json!({"template":"counter.html"}), true),
        (
            json!({"template":"counter.html","interaction":{"module":"module:counter","export":"mount","mount":"load"}}),
            true,
        ),
        (json!({"template":"counter.html","interaction":null}), false),
        (
            json!({"template":"counter.html","interaction":["module:counter","mount","load"]}),
            false,
        ),
        (
            json!({"template":"counter.html","interaction":{"module":"module:counter","mount":"load"}}),
            false,
        ),
        (
            json!({"template":"counter.html","interaction":{"module":"module:counter","export":"mount","mount":"idle"}}),
            false,
        ),
        (
            json!({"template":"counter.html","interaction":{"module":"module:counter","export":"mount","mount":"load","fallback":"default"}}),
            false,
        ),
        (
            json!({"interaction":{"module":"module:counter","export":"mount","mount":"load"}}),
            false,
        ),
    ] {
        assert_eq!(
            serde_json::from_value::<Component>(value.clone()).is_ok(),
            accepted,
            "native {value}"
        );
        assert_eq!(validator.is_valid(&value), accepted, "schema {value}");
    }
}

#[test]
fn native_component_projection_carries_definition_intent_without_changing_authored_text() {
    use gazette_compiler::markdown::{ComponentPresentation, Limits, ParsedMarkdown};
    use gazette_core::markdown::{ComponentBinding, Error, Fragment};
    use gazette_ports::markdown::ComponentRenderer;

    struct StaticBody;
    impl ComponentRenderer for StaticBody {
        fn render(
            &self,
            binding: &ComponentBinding,
            body: &Fragment,
            _: bool,
        ) -> Result<String, Error> {
            let address = &binding.context.as_ref().expect("hosted context").address;
            Ok(format!(
                "<section data-address=\"{address}\">{}</section>",
                body.html
            ))
        }
    }

    let source = "package='theme:one'\nnamespace='one'\ntemplates='templates'\nrender={page='page.html'}\n[components.counter]\ntemplate='counter.html'\ninteraction={module='module:counter',export='mountCounter',mount='load'}\n[components.note]\ntemplate='note.html'\n";
    let manifest = parse(source).expect("selected catalog");
    let body = ParsedMarkdown::parse(
        ":::counter\nMeaning **stays**.\n:::\n\n:::note\nStatic text.\n:::\n",
        Limits::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("real native directives");
    let host = ComponentHost {
        content: "content:component-host".parse::<Key>().unwrap(),
        site: "site:one".parse::<Key>().unwrap(),
        project_resources: None,
        surface: ComponentSurface::Page {
            family: PageFamily::Article,
        },
    };
    let render = || {
        body.render_with_hosted_components(
            Some(ComponentPresentation {
                definitions: manifest.components(),
                renderer: &StaticBody,
            }),
            &host,
            |_, _, _| Ok(None),
        )
        .expect("shared projection")
    };
    let output = render();
    assert_eq!(output, render());
    assert_eq!(output.components.len(), 2);
    let interactive = &output.components[0];
    assert_eq!(interactive.name.as_str(), "counter");
    assert!(interactive.arguments.is_empty());
    let context = interactive
        .context
        .as_ref()
        .expect("compiler-owned context");
    assert_eq!(context.address, "content:component-host#component-1");
    assert_eq!(context.occurrence, 1);
    assert_eq!(context.host, host);
    assert_eq!(
        output.components[1].context.as_ref().unwrap().address,
        "content:component-host#component-2"
    );
    assert_eq!(
        interactive.interaction.as_ref().unwrap().export.as_str(),
        "mountCounter"
    );
    assert!(output.components[1].interaction.is_none());
    assert!(output.body_html.contains("Meaning <strong>stays</strong>."));
    assert!(
        output
            .body_html
            .contains("data-address=\"content:component-host#component-1\"")
    );
    assert!(output.body_text.contains("Meaning stays."));
    assert!(output.body_text.contains("Static text."));
    assert!(!output.body_text.contains("mountCounter"));
    let projected = serde_json::to_value(&output.components).unwrap();
    assert_eq!(projected[0]["context"]["host"]["surface"]["kind"], "page");
    assert_eq!(
        projected[0]["context"]["host"]["surface"]["family"]["kind"],
        "article"
    );
    assert!(!projected.to_string().contains("/private/"));
    assert!(!projected.to_string().contains("span"));
    assert_eq!(projected[0]["interaction"]["module"], "module:counter");
    assert!(projected[1].get("interaction").is_none());
}

#[test]
fn component_names_are_theme_local_and_native_schema_and_package_admission_agree() {
    use gazette_core::markdown::ComponentName;

    let schema = serde_json::to_value(schemars::schema_for!(ComponentName)).expect("shared schema");
    let validator = jsonschema::validator_for(&schema).expect("native JSON Schema validator");
    for (name, accepted) in [
        ("callout", true),
        ("Callout-2", true),
        ("code_example", true),
        ("", false),
        ("2callout", false),
        ("component:callout", false),
        ("callout\n", false),
        ("callout\r\n", false),
        ("callout\u{2028}", false),
        ("call out", false),
        ("café", false),
        ("../callout", false),
    ] {
        assert_eq!(
            name.parse::<ComponentName>().is_ok(),
            accepted,
            "native {name:?}"
        );
        assert_eq!(
            validator.is_valid(&serde_json::json!(name)),
            accepted,
            "schema {name:?}"
        );
        let source = format!(
            "package='theme:one'\nnamespace='one'\ntemplates='templates'\nrender={{page='page.html'}}\n[components.{}]\ntemplate='callout.html'\n",
            serde_json::to_string(name).expect("quoted name")
        );
        assert_eq!(parse(&source).is_ok(), accepted, "package {name:?}");
    }
}

#[test]
fn presentation_only_is_a_checked_definition_choice_and_keeps_resource_resolution() {
    use gazette_compiler::markdown::{ComponentPresentation, Limits, ParsedMarkdown};
    use gazette_core::markdown::{ComponentBinding, Error, Fragment};
    use gazette_ports::markdown::ComponentRenderer;

    struct StaticBody;
    impl ComponentRenderer for StaticBody {
        fn render(&self, _: &ComponentBinding, body: &Fragment, _: bool) -> Result<String, Error> {
            Ok(format!("<section>{}</section>", body.html))
        }
    }
    let source = "package='theme:one'\nnamespace='one'\ntemplates='templates'\nrender={page='page.html'}\n[components.payload]\ntemplate='payload.html'\npresentation_only=true\n[components.caption]\ntemplate='caption.html'\n";
    let manifest = parse(source).unwrap();
    assert!(
        manifest
            .components()
            .definition(&"payload".parse().unwrap())
            .unwrap()
            .presentation_only
    );
    assert!(
        !manifest
            .components()
            .definition(&"caption".parse().unwrap())
            .unwrap()
            .presentation_only
    );
    assert!(parse(&source.replace("presentation_only=true", "presentation_only='true'")).is_err());
    let body = ParsedMarkdown::parse(
        ":::payload\n[Arbitrary technical label](frame.webp)\n:::\n\n:::caption\nMeaningful film description.\n:::\n",
        Limits::default(), &gazette_adapter_markdown::Comrak,
    ).unwrap();
    let mut resources = Vec::new();
    let rendered = body
        .render_with_components(
            Some(ComponentPresentation {
                definitions: manifest.components(),
                renderer: &StaticBody,
            }),
            |_, value, _| {
                resources.push(value.to_string());
                Ok(Some("/assets/frame.webp".into()))
            },
        )
        .unwrap();
    assert_eq!(resources, ["frame.webp"]);
    assert!(rendered.body_html.contains("Arbitrary technical label"));
    assert!(rendered.body_html.contains("/assets/frame.webp"));
    assert!(!rendered.body_text.contains("Arbitrary technical label"));
    assert_eq!(
        rendered
            .body_text
            .matches("Meaningful film description.")
            .count(),
        1
    );
}

#[test]
fn landing_presentation_payload_is_absent_from_search_and_markdown_alternates() {
    use gazette_compiler::{
        authored,
        corpus::{Input, ParsedCorpus},
        markdown::ComponentPresentation,
        workspace,
    };
    use gazette_core::markdown::{ComponentBinding, Error, Fragment};
    use gazette_ports::markdown::ComponentRenderer;

    struct StaticBody;
    impl ComponentRenderer for StaticBody {
        fn render(&self, _: &ComponentBinding, body: &Fragment, _: bool) -> Result<String, Error> {
            Ok(format!("<section>{}</section>", body.html))
        }
    }
    let config = "[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n[[sources]]\nid='source:one'\nlocation={kind='filesystem',root='/private/input'}\n[[brands]]\nid='brand:one'\nname='Film'\n[[brands.publications]]\nsite='site:one'\npublic_base_url='https://example.test/'\nauthored=[{source='source:one',target='one'}]\nmachine={search='lexical',markdown_alternates={enabled=true,directory_page='index.md',file_page='append-md',media_type='text/markdown',discovery_rel='alternate'}}\n";
    let compilation = workspace::compile(config, Default::default()).unwrap();
    let source = "format: gazette-landing/v1\ncontent:\n  id: content:home\n  site: site:one\n  title: Film\n  lifecycle: draft\n  route: {path: /}\n  structure: {kind: page, family: {kind: index}}\nsections:\n  - component: film\n    body: A meaningful description.\n    sections:\n      - component: payload\n        body: '![Frame 1](https://example.test/frame.webp)'\n      - body: A meaningful caption.\n";
    let corpus = ParsedCorpus::parse(
        [Input {
            path: "one/index.landing.yaml".parse().unwrap(),
            bytes: source.as_bytes(),
        }],
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .unwrap();
    let resolved = authored::resolve(&compilation, &corpus, Default::default()).unwrap();
    let theme = "package='theme:one'\nnamespace='one'\ntemplates='templates'\nrender={page='page.html'}\n[components.film]\ntemplate='film.html'\n[components.payload]\ntemplate='payload.html'\npresentation_only=true\n";
    let manifest = parse(theme).unwrap();
    let page = resolved
        .project_page_with(
            &"content:home".parse().unwrap(),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
            None,
            Some(ComponentPresentation {
                definitions: manifest.components(),
                renderer: &StaticBody,
            }),
        )
        .unwrap();
    assert_eq!(
        page.data.images.len(),
        1,
        "presentation inventory retains the frame"
    );
    let metadata = serde_json::to_value(page.json_ld()).unwrap();
    assert!(
        metadata.get("image").is_none(),
        "decorative frames are not page images"
    );
    assert!(metadata.get("gazette:assets").is_none());
    let search = serde_json::to_value(page.search()).unwrap();
    let body = search["body"].as_str().expect("search body");
    let markdown = page.body_markdown.as_deref().expect("selected alternate");
    assert!(page.body_html.contains("Frame 1"));
    for reading in [body, markdown] {
        assert!(!reading.contains("Frame 1"));
        assert_eq!(reading.matches("A meaningful description.").count(), 1);
        assert_eq!(reading.matches("A meaningful caption.").count(), 1);
    }
}

#[path = "theme/faq.rs"]
mod faq;
#[path = "theme/figures.rs"]
mod figures;
