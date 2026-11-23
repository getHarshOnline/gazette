//! Documentation selection resolves exact manual/reference identities without native effects.

use gazette_compiler::workspace::{self, DocumentationInput, DocumentationNavigation};
use gazette_core::{
    documentation::{Format, Selection, Target},
    publication::Key,
};
use serde_json::json;

fn markdown_reference(pages: &str) -> String {
    format!(
        "{{kind='reference',slug='reference',title='API',pages={pages},input={{kind='artifact',source='source:api',selection={{kind='directory',path='reference'}},format='markdown'}}}}"
    )
}

#[test]
fn explicit_reference_navigation_round_trips_without_changing_legacy_projection() {
    let pages = "[{kind='page',path='index'},{kind='group',title='Members',pages=[{kind='directory',path='types'}]}]";
    let selected = module(
        "project:a",
        "library",
        "Library",
        &markdown_reference(pages),
    );
    let result = compile(&configuration(&selected));
    let build = result
        .workspace
        .documentation_repository(&key("site:other"))
        .unwrap()
        .build
        .as_ref()
        .unwrap();
    let node = &build.modules[0].navigation[0];
    let value = serde_json::to_value(node).unwrap();
    assert_eq!(value["pages"][0]["path"], "index");
    assert_eq!(
        value["pages"][1]["pages"][0],
        json!({"kind":"directory","path":"types"})
    );
    let schema = serde_json::to_value(schemars::schema_for!(DocumentationNavigation)).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    assert!(validator.is_valid(&value));
    let mut invalid = value.clone();
    invalid["pages"][1]["pages"][0]["title"] = json!("Not a directory field");
    assert!(!validator.is_valid(&invalid));
    assert_eq!(
        serde_json::from_value::<DocumentationNavigation>(value).unwrap(),
        *node
    );

    let legacy = compile(&configuration(&modules()));
    let legacy_build = legacy
        .workspace
        .documentation_repository(&key("site:other"))
        .unwrap()
        .build
        .as_ref()
        .unwrap();
    assert!(
        serde_json::to_value(&legacy_build.modules[0].navigation[0])
            .unwrap()
            .get("pages")
            .is_none()
    );
}

#[test]
fn named_toml_and_direct_json_refuse_malformed_recursive_reference_pages() {
    for pages in [
        "[['page','index']]",
        "[1]",
        "[{kind='group',title='Bad',pages=[['page','index']]}]",
        "[{kind='page',path='index',title='Wrong variant'}]",
    ] {
        let selected = module(
            "project:a",
            "library",
            "Library",
            &markdown_reference(pages),
        );
        assert!(
            workspace::compile(&configuration(&selected), Default::default()).is_err(),
            "accepted {pages}"
        );
    }
    let reference = json!({"kind":"reference","slug":"reference","title":"API","input":{"kind":"artifact","source":"source:api","selection":{"kind":"directory"},"format":"markdown"}});
    for pages in [
        json!(null),
        json!(false),
        json!([["page", "index"]]),
        json!([{"kind":"group","title":"Group","pages":[{"kind":"page","path":"index"},["page","other"]]}]),
    ] {
        let mut value = reference.clone();
        value["pages"] = pages;
        assert!(serde_json::from_value::<DocumentationNavigation>(value).is_err());
    }
}

#[test]
fn reference_pages_schema_is_optional_but_requires_an_array_when_present() {
    let schema = serde_json::to_value(schemars::schema_for!(DocumentationNavigation)).unwrap();
    let variants = schema
        .get("oneOf")
        .or_else(|| schema.get("anyOf"))
        .unwrap()
        .as_array()
        .unwrap();
    let reference = variants
        .iter()
        .find(|variant| variant["properties"].get("input").is_some())
        .unwrap();
    assert_eq!(reference["properties"]["pages"]["type"], "array");
    assert!(
        !reference["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|field| field == "pages")
    );
}

const BASE: &str = "[[principals]]\nid='owner:publisher'\nkind='person'\n\
[[sites]]\nid='site:one'\npublisher='owner:publisher'\ndeployable=false\n\
[[sites]]\nid='site:other'\npublisher='owner:publisher'\ndeployable=false\n\
[[projects]]\nid='project:a'\nlifecycle='planned'\nvisibility='private'\navailability='unavailable'\n\
[[projects]]\nid='project:z'\nlifecycle='done'\nvisibility='unlisted'\navailability='gated'\n\
[[sources]]\nid='source:api'\nlocation={kind='repository',root='/not-opened/native-project',commit='0000000000000000000000000000000000000000',subdirectory='generated'}\n\
[[brands]]\nid='brand:publication'\nname='Brand display & name'\n\
[[brands.publications]]\nsite='site:one'\n\
[[brands.publications]]\nsite='site:other'\n";
const TARGET: &str = "{kind='mintlify',theme='mint',primary_color='#123456'}";
const REFERENCE_Z: &str = "{kind='reference',slug='z-reference',title='Reference shown first',input={kind='artifact',source='source:api',selection={kind='file',path='z/api.json'},format='typedoc-json'}}";
const REFERENCE_A: &str = "{kind='reference',slug='a-reference',title='Reference shown second',input={kind='artifact',source='source:api',selection={kind='file',path='a/api.json'},format='typedoc-json'}}";

fn key(value: &str) -> Key {
    value.parse().expect("exact namespaced key")
}

fn module(project: &str, slug: &str, title: &str, references: &str) -> String {
    format!("{{project='{project}',slug='{slug}',title='{title}',navigation=[{references}]}}")
}

fn references(
    module: &workspace::DocumentationModule,
) -> impl Iterator<Item = (&str, &str, &DocumentationInput)> {
    module
        .navigation
        .iter()
        .filter_map(DocumentationNavigation::reference)
}

fn modules() -> String {
    format!(
        "{},{}",
        module(
            "project:z",
            "z-module",
            "First shown, not project identity",
            &format!("{REFERENCE_Z},{REFERENCE_A}")
        ),
        module("project:a", "a-module", "Second shown", REFERENCE_Z)
    )
}

fn manual_module(navigation: &str, manual: Option<&str>) -> String {
    let manual = manual.map_or_else(String::new, |manual| format!(",manual={manual}"));
    format!(
        "{{project='project:z',slug='z-module',title='Gazette'{manual},navigation=[{navigation}]}}"
    )
}

const MANUAL: &str = "{source='source:api',assets=[{input={kind='file',path='docs/logo.svg'},path='assets/logo.svg'}]}";
const MANUAL_SECTION: &str = "{kind='manual',title='Guide',pages=[{title='Start',route='guide/start',input={kind='file',path='docs/start.md'}}]}";

fn configuration(modules: &str) -> String {
    format!(
        "{BASE}documentation_repository={{name='configured_reference_repo',path='/not-opened/local-output/different-folder',build={{target={TARGET},modules=[{modules}]}}}}\n"
    )
}

fn compile(source: &str) -> workspace::Compilation {
    workspace::compile(source, Default::default()).expect("real TOML resolution, no native I/O")
}

#[test]
fn generated_markdown_format_uses_existing_workspace_source_and_reference_selection() {
    let source = configuration(&modules()).replace("format='typedoc-json'", "format='markdown'");
    let result = compile(&source);
    let build = result
        .workspace
        .documentation_repository(&key("site:other"))
        .expect("configured publication")
        .build
        .as_ref()
        .expect("target selection");
    assert!(
        build
            .modules
            .iter()
            .flat_map(references)
            .all(|(_, _, input)| matches!(
                input,
                DocumentationInput::Artifact {
                    format: Format::Markdown,
                    ..
                }
            ))
    );
}

fn invalid(source: &str) -> Vec<workspace::Diagnostic> {
    match workspace::compile(source, Default::default()).expect_err("no partial workspace") {
        workspace::Error::Invalid(errors) => errors,
        other => panic!("expected semantic reference diagnostic, got {other}"),
    }
}

#[test]
fn authored_manual_and_generated_references_remain_distinct_and_ordered() {
    let selected = manual_module(
        &format!("{MANUAL_SECTION},{REFERENCE_Z},{MANUAL_SECTION}"),
        Some(MANUAL),
    );
    let errors = invalid(&configuration(&selected));
    assert!(
        errors
            .iter()
            .any(|error| error.code == "workspace_documentation_manual_page"),
        "a duplicate manual route cannot silently win: {errors:?}"
    );

    let second = MANUAL_SECTION
        .replace("title='Guide'", "title='Advanced'")
        .replace("title='Start'", "title='Next'")
        .replace("route='guide/start'", "route='guide/next'")
        .replace("path='docs/start.md'", "path='docs/next.md'");
    let source = configuration(&manual_module(
        &format!("{MANUAL_SECTION},{REFERENCE_Z},{second}"),
        Some(MANUAL),
    ));
    let build = compile(&source)
        .workspace
        .documentation_repository(&key("site:other"))
        .expect("repository")
        .build
        .clone()
        .expect("build");
    assert!(matches!(
        build.modules[0].navigation[0],
        DocumentationNavigation::Manual { .. }
    ));
    assert!(matches!(
        build.modules[0].navigation[1],
        DocumentationNavigation::Reference { .. }
    ));
    assert!(matches!(
        build.modules[0].navigation[2],
        DocumentationNavigation::Manual { .. }
    ));
    let manual = build.modules[0].manual.as_ref().expect("manual binding");
    assert_eq!(manual.source, key("source:api"));
    assert_eq!(manual.assets[0].path.as_str(), "assets/logo.svg");
}

#[test]
fn manual_binding_and_nonempty_overall_selection_are_enforced() {
    for selected in [
        manual_module(MANUAL_SECTION, None),
        manual_module(REFERENCE_Z, Some(MANUAL)),
    ] {
        assert!(
            invalid(&configuration(&selected))
                .iter()
                .any(|error| error.code == "workspace_documentation_manual_binding")
        );
    }

    let manual_only = configuration(&manual_module(MANUAL_SECTION, Some(MANUAL)));
    let build = compile(&manual_only)
        .workspace
        .documentation_repository(&key("site:other"))
        .expect("repository")
        .build
        .clone()
        .expect("manual-only build");
    assert_eq!(build.modules.len(), 1);
    assert!(references(&build.modules[0]).next().is_none());

    let reference_only = configuration(&manual_module(REFERENCE_Z, None));
    let build = compile(&reference_only)
        .workspace
        .documentation_repository(&key("site:other"))
        .expect("repository")
        .build
        .clone()
        .expect("reference-only build");
    assert_eq!(references(&build.modules[0]).count(), 1);
}

#[test]
fn ordered_modules_and_references_borrow_the_sole_brand_repository_configuration() {
    let source = configuration(&modules());
    let compilation = compile(&source);
    let workspace = &compilation.workspace;
    assert!(
        workspace
            .documentation_repository(&key("site:one"))
            .is_none()
    );
    assert!(
        workspace
            .documentation_repository(&key("site:missing"))
            .is_none()
    );
    let repository = workspace
        .documentation_repository(&key("site:other"))
        .expect("configured site target");
    assert!(std::ptr::eq(
        repository,
        workspace
            .publication_for(&key("site:other"))
            .expect("publication")
            .documentation_repository
            .as_ref()
            .expect("same repository owner")
    ));
    assert_eq!(repository.name, "configured_reference_repo");
    assert_eq!(repository.path, "/not-opened/local-output/different-folder");
    assert_eq!(
        workspace.brands().next().expect("brand").name,
        "Brand display & name"
    );
    let build = repository.build.as_ref().expect("explicit build selection");
    assert_eq!(
        build.target,
        Target::Mintlify {
            theme: "mint".into(),
            primary_color: "#123456".into(),
            light_color: None,
            dark_color: None,
            telemetry: None,
        }
    );
    assert_eq!(
        build
            .modules
            .iter()
            .map(|module| module.project.as_str())
            .collect::<Vec<_>>(),
        ["project:z", "project:a"]
    );
    assert_eq!(
        build
            .modules
            .iter()
            .map(|module| module.slug.as_str())
            .collect::<Vec<_>>(),
        ["z-module", "a-module"]
    );
    assert_eq!(build.modules[0].title, "First shown, not project identity");
    let selected_references = references(&build.modules[0]).collect::<Vec<_>>();
    assert_eq!(
        selected_references
            .iter()
            .map(|(slug, _, _)| *slug)
            .collect::<Vec<_>>(),
        ["z-reference", "a-reference"]
    );
    assert_eq!(
        selected_references
            .iter()
            .map(|(_, title, _)| *title)
            .collect::<Vec<_>>(),
        ["Reference shown first", "Reference shown second"]
    );
    for ((_, _, input), expected) in selected_references.iter().zip(["z/api.json", "a/api.json"]) {
        let DocumentationInput::Artifact {
            source,
            selection,
            format,
        } = input
        else {
            panic!("imported artifact")
        };
        assert_eq!(source, &key("source:api"));
        let Selection::File { path } = selection else {
            panic!("explicit selected file")
        };
        assert_eq!(path.as_str(), expected);
        assert_eq!(*format, Format::TypeDocJson);
        assert!(workspace.source(source).is_some());
    }
    assert_eq!(
        references(&build.modules[1]).next().expect("reference").0,
        "z-reference",
        "reference slugs are scoped to their module"
    );
    let encoded = serde_json::to_value(repository).expect("resolved configuration");
    assert_eq!(
        encoded["build"]["modules"][0]["navigation"][0]["input"]["format"],
        "typedoc-json"
    );
    assert!(encoded["build"].get("name").is_none());
    assert!(encoded["build"].get("path").is_none());
    assert!(encoded["build"]["target"].get("name").is_none());
    let project = serde_json::to_value(
        workspace
            .project(&key("project:z"))
            .expect("native project"),
    )
    .expect("project projection");
    assert!(
        project.get("name").is_none(),
        "navigation labels do not invent project fields"
    );
    assert_eq!(project["lifecycle"], "done");
    assert_eq!(project["availability"], "gated");
    assert_eq!(
        compile(&source).workspace,
        *workspace,
        "repeat resolution is deterministic"
    );
}

#[test]
fn a_repository_destination_does_not_implicitly_select_an_assembly_target() {
    let compilation = compile(&format!(
        "{BASE}documentation_repository={{name='explicit_repo',path='/not-opened/explicit-folder'}}\n"
    ));
    let repository = compilation
        .workspace
        .documentation_repository(&key("site:other"))
        .expect("destination only");
    assert!(repository.build.is_none());
    assert_eq!(
        serde_json::to_value(repository).expect("destination"),
        json!({"name":"explicit_repo","path":"/not-opened/explicit-folder"})
    );
    assert!(
        compile(BASE)
            .workspace
            .documentation_repository(&key("site:other"))
            .is_none()
    );
}

#[test]
fn missing_exact_project_or_source_fails_even_for_a_non_deployable_other_site() {
    let configured = configuration(&modules());
    for (source, missing, suffix) in [
        (
            configured.replace("project='project:z'", "project='project:Z'"),
            "project:Z",
            ".modules[0].project",
        ),
        (
            configured.replace("project='project:a'", "project='source:api'"),
            "source:api",
            ".modules[1].project",
        ),
        (
            configured.replace("source='source:api'", "source='source:API'"),
            "source:API",
            ".modules[0].navigation[0].input.source",
        ),
        (
            configured.replace("source='source:api'", "source='project:z'"),
            "project:z",
            ".modules[0].navigation[0].input.source",
        ),
    ] {
        let errors = invalid(&source);
        assert!(errors.iter().any(|error| error.code == "workspace_reference" && error.key.as_ref() == Some(&key(missing)) && error.field.starts_with("brands[\"brand:publication\"].publications[\"site:other\"].documentation_repository.build") && error.field.ends_with(suffix)), "exact missing field: {errors:?}");
    }
}

#[test]
fn duplicate_modules_or_reference_slugs_never_choose_a_silent_winner() {
    let first = module("project:z", "z-module", "First", REFERENCE_Z);
    for selected in [
        format!(
            "{first},{}",
            module("project:z", "other-slug", "Second", REFERENCE_A)
        ),
        format!(
            "{first},{}",
            module("project:a", "z-module", "Second", REFERENCE_A)
        ),
        module(
            "project:z",
            "z-module",
            "First",
            &format!("{REFERENCE_Z},{REFERENCE_Z}"),
        ),
    ] {
        let errors = invalid(&configuration(&selected));
        assert!(
            errors
                .iter()
                .any(|error| error.code == "workspace_duplicate"
                    && error
                        .field
                        .contains("documentation_repository.build.modules")),
            "{errors:?}"
        );
    }
    let valid = configuration(&format!(
        "{first},{}",
        module(
            "project:a",
            "other-slug",
            "Same or different title",
            REFERENCE_Z
        )
    ));
    assert_eq!(
        compile(&valid)
            .workspace
            .documentation_repository(&key("site:other"))
            .expect("repository")
            .build
            .as_ref()
            .expect("build")
            .modules
            .len(),
        2
    );
}

#[test]
fn empty_selections_and_invalid_navigation_labels_fail_explicitly() {
    for selected in [String::new(), module("project:z", "z-module", "Module", "")] {
        assert!(
            invalid(&configuration(&selected))
                .iter()
                .any(|error| error.code == "workspace_documentation_empty")
        );
    }
    for selected in [
        module("project:z", "", "Module", REFERENCE_Z),
        module("project:z", "z-module", "  ", REFERENCE_Z),
        module(
            "project:z",
            "z-module",
            "Module",
            &REFERENCE_Z.replace("slug='z-reference'", "slug=''"),
        ),
        module(
            "project:z",
            "z-module",
            "Module",
            &REFERENCE_Z.replace("title='Reference shown first'", "title='  '"),
        ),
        module(
            "project:z",
            "z-module",
            "Module",
            &REFERENCE_Z.replace("title='Reference shown first'", "title=\"line\\nbreak\""),
        ),
    ] {
        assert!(
            invalid(&configuration(&selected))
                .iter()
                .any(|error| error.code == "workspace_documentation_label")
        );
    }
}

#[test]
fn native_reference_formats_resolve_without_claiming_target_compatibility() {
    for (name, expected) in [
        ("rustdoc-html", Format::RustdocHtml),
        ("rustdoc-json", Format::RustdocJson),
        ("openapi-json", Format::OpenApiJson),
        ("openapi-yaml", Format::OpenApiYaml),
    ] {
        let compilation = compile(
            &configuration(&modules())
                .replace("format='typedoc-json'", &format!("format='{name}'")),
        );
        let repository = compilation
            .workspace
            .documentation_repository(&key("site:other"))
            .expect("configured target");
        let build = repository.build.as_ref().expect("explicit build");
        for module in &build.modules {
            for (_, _, input) in references(module) {
                let DocumentationInput::Artifact { format, .. } = input else {
                    panic!("import remains an import")
                };
                assert_eq!(*format, expected);
            }
        }
    }
}

#[test]
fn unsupported_modes_formats_fields_and_nonliteral_entries_fail_at_the_real_parser() {
    let valid = configuration(&modules());
    for source in [
        valid.replace("kind='mintlify'", "kind='unknown-target'"),
        valid.replace("kind='artifact'", "kind='generator'"),
        valid.replace("format='typedoc-json'", "format='type-doc-json'"),
        valid.replace("format='typedoc-json'", "format='unknown-reference'"),
        valid.replace("theme='mint'", "theme='mint',name='second brand owner'"),
        valid.replace("build={target=", "build={name='duplicated brand',target="),
        valid.replace(
            "project='project:z'",
            "project='project:z',name='invented project name'",
        ),
        valid.replace("slug='z-reference'", "slug='z-reference',unknown=true"),
        valid.replace("kind='artifact'", "kind='artifact',unknown=true"),
        valid.replace("path='z/api.json'", "path='../z/api.json'"),
        valid.replace("path='z/api.json'", "path='/absolute/api.json'"),
    ] {
        assert!(
            matches!(
                workspace::compile(&source, Default::default()),
                Err(workspace::Error::Toml(_))
            ),
            "unsupported schema must not resolve: {source}"
        );
    }
    let literal = compile(&valid.replace("path='z/api.json'", "path='z/%2e%2e/api.json'"));
    let repository = literal
        .workspace
        .documentation_repository(&key("site:other"))
        .expect("repository");
    let DocumentationInput::Artifact { selection, .. } =
        references(&repository.build.as_ref().expect("build").modules[0])
            .next()
            .expect("reference")
            .2
    else {
        panic!("imported artifact")
    };
    assert_eq!(
        selection.path().expect("explicit file path").as_str(),
        "z/%2e%2e/api.json",
        "capture coordinates are literal names, never URL-decoded traversal"
    );
}

#[test]
fn explicit_directory_selection_resolves_nested_or_capture_root_without_inference() {
    let valid = configuration(&modules());
    for (selector, expected) in [
        (
            "{kind='directory',path='native/python'}",
            Some("native/python"),
        ),
        ("{kind='directory'}", None),
    ] {
        let source = valid.replace("{kind='file',path='z/api.json'}", selector);
        let compilation = compile(&source);
        let repository = compilation
            .workspace
            .documentation_repository(&key("site:other"))
            .expect("configured repository");
        let DocumentationInput::Artifact { selection, .. } =
            references(&repository.build.as_ref().expect("build").modules[0])
                .next()
                .expect("reference")
                .2
        else {
            panic!("retained artifact input")
        };
        let Selection::Directory { path } = selection else {
            panic!("explicit directory")
        };
        assert_eq!(path.as_ref().map(|path| path.as_str()), expected);
        assert_eq!(selection.path().map(|path| path.as_str()), expected);
    }
}

#[test]
fn selectors_reject_legacy_ambiguous_malformed_and_escaping_coordinates() {
    let valid = configuration(&modules());
    for selector in [
        "'z/api.json'",
        "[]",
        "['file','z/api.json']",
        "{}",
        "{path='z/api.json'}",
        "{kind='file'}",
        "{kind='unknown',path='z/api.json'}",
        "{kind='file',path=4}",
        "{kind='file',path='z/api.json',unknown=true}",
        "{kind='directory',path='native',file='z/api.json'}",
        "{kind='file',path='z/api.json',directory='native'}",
        "{kind='directory',path=''}",
        "{kind='directory',path='.'}",
        "{kind='directory',path='../native'}",
        "{kind='directory',path='/native'}",
        "{kind='directory',path='native/../private'}",
    ] {
        let source = valid.replacen("{kind='file',path='z/api.json'}", selector, 1);
        gazette_compiler::toml::ParsedToml::parse(&source, Default::default())
            .expect("valid TOML syntax isolates selector schema rejection");
        assert!(
            matches!(
                workspace::compile(&source, Default::default()),
                Err(workspace::Error::Toml(_))
            ),
            "selector must be rejected by the native parser: {selector}"
        );
    }
    for replacement in [
        "entry='z/api.json'",
        "entry='z/api.json',selection={kind='file',path='z/api.json'}",
    ] {
        let source = valid.replacen("selection={kind='file',path='z/api.json'}", replacement, 1);
        assert!(
            matches!(
                workspace::compile(&source, Default::default()),
                Err(workspace::Error::Toml(_))
            ),
            "obsolete or competing coordinates must not resolve: {replacement}"
        );
    }
}

#[test]
fn selection_path_origin_retains_exact_original_scalar_span() {
    use gazette_compiler::workspace::{DeclarationKind, FieldQuery, ParsedWorkspace};

    let source = format!(
        "# 🦀 path='z/api.json' is not the selected scalar\n{}",
        configuration(&modules())
    );
    let parsed =
        ParsedWorkspace::parse(&source, Default::default()).expect("native syntax and resolution");
    let field = serde_json::from_value(json!([
        "publications",
        1,
        "documentation_repository",
        "build",
        "modules",
        0,
        "navigation",
        0,
        "input",
        "selection",
        "path"
    ]))
    .expect("typed source coordinates");
    let origin = parsed
        .origin(&FieldQuery {
            kind: DeclarationKind::Brand,
            identity: key("brand:publication"),
            field,
        })
        .expect("explicit selection path origin");
    assert_eq!(
        origin.span.slice(&source).expect("native scalar span"),
        "'z/api.json'"
    );
    let selector_start = source
        .find("selection={kind='file',path=")
        .expect("exact selector");
    assert_eq!(
        origin.span.start,
        selector_start + "selection={kind='file',path=".len()
    );
}

#[test]
fn every_named_documentation_boundary_rejects_positional_array_transport() {
    let modules = modules();
    let valid = configuration(&modules);
    let build = format!("{{target={TARGET},modules=[{modules}]}}");
    let repository = format!(
        "{{name='configured_reference_repo',path='/not-opened/local-output/different-folder',build={build}}}"
    );
    let first_module = module(
        "project:z",
        "z-module",
        "First shown, not project identity",
        &format!("{REFERENCE_Z},{REFERENCE_A}"),
    );
    let input = "{kind='artifact',source='source:api',selection={kind='file',path='z/api.json'},format='typedoc-json'}";
    for (boundary, original, positional) in [
        (
            "repository",
            repository,
            format!(
                "['configured_reference_repo','/not-opened/local-output/different-folder',{build}]"
            ),
        ),
        ("build", build, format!("[{TARGET},[{modules}]]")),
        (
            "target",
            TARGET.into(),
            "['mintlify','mint','#123456']".into(),
        ),
        (
            "module",
            first_module,
            format!(
                "['project:z','z-module','First shown, not project identity',[{REFERENCE_Z},{REFERENCE_A}]]"
            ),
        ),
        (
            "reference",
            REFERENCE_Z.into(),
            format!("['z-reference','Reference shown first',{input}]"),
        ),
        (
            "input",
            input.into(),
            "['artifact','source:api',{kind='file',path='z/api.json'},'typedoc-json']".into(),
        ),
        (
            "selection",
            "{kind='file',path='z/api.json'}".into(),
            "['file','z/api.json']".into(),
        ),
    ] {
        assert!(
            valid.contains(&original),
            "fixture anchors the exact {boundary} boundary"
        );
        let source = valid.replacen(&original, &positional, 1);
        gazette_compiler::toml::ParsedToml::parse(&source, Default::default())
            .expect("valid TOML syntax: rejection must be the named-table contract");
        assert!(
            matches!(
                workspace::compile(&source, Default::default()),
                Err(workspace::Error::Toml(_))
            ),
            "positional {boundary} must not resolve"
        );
    }
}

#[test]
fn generator_coordinates_validate_without_binding_or_expanding_literal_environment() {
    let selected = |program: &str, environment: &str| {
        configuration(&module(
            "project:a",
            "library",
            "Library",
            &format!(
                "{{kind='reference',slug='api',title='API',input={{kind='generated',source='source:api',recipe={{program={program:?},executable={{bytes=1,sha256='{}'}},arguments=[{{kind='input-root'}},{{kind='output-root'}}],environment={environment},version_arguments=['--version'],expected_version_sha256='{}',output={{kind='directory'}},format='sphinx-json'}}}}}}",
                "0".repeat(64),
                "1".repeat(64)
            ),
        ))
    };
    for program in ["machine:tools/python", "tools/python", "/selected/python"] {
        let source = selected(
            program,
            "{PYTHONPATH={path='machine:packages'},LITERAL='machine:missing/not-expanded'}",
        );
        let compiled = compile(&source);
        let build = compiled
            .workspace
            .documentation_repository(&key("site:other"))
            .unwrap()
            .build
            .as_ref()
            .unwrap();
        let value = serde_json::to_value(&build.modules[0].navigation[0]).unwrap();
        assert_eq!(value["input"]["recipe"]["program"], program);
        assert_eq!(
            value["input"]["recipe"]["environment"]["LITERAL"],
            "machine:missing/not-expanded"
        );
    }
    for (program, environment) in [
        ("machine:tools/../python", "{}"),
        ("../python", "{}"),
        (
            "/python",
            "{PYTHONPATH={path='machine:packages/../escape'}}",
        ),
        ("/python", "{'INVALID-NAME'='value'}"),
    ] {
        assert!(
            invalid(&selected(program, environment))
                .iter()
                .any(|error| error.code == "workspace_documentation_generator")
        );
    }
}

#[test]
fn generator_path_arguments_and_version_values_use_explicit_coordinates() {
    let selected = |argument: &str, version: &str, validation: &str| {
        configuration(&module(
            "project:a",
            "library",
            "Library",
            &format!(
                "{{kind='reference',slug='api',title='API',input={{kind='generated',source='source:api',recipe={{program='machine:tools/node',executable={{bytes=1,sha256='{}'}},arguments=[{{kind='input-root'}},{{kind='output-root'}},{argument}],version_arguments=[{version}],validation_arguments=[{{kind='output'}},{validation}],expected_version_sha256='{}',output={{kind='directory'}},format='markdown'}}}}}}",
                "0".repeat(64),
                "1".repeat(64)
            ),
        ))
    };
    let good = selected(
        "{kind='path',path='machine:tools/plugin.mjs'}",
        "{path='machine:tools/cli.mjs'},'--version','machine:missing/literal'",
        "{kind='path',path='checks/check.mjs'}",
    );
    let compiled = compile(&good);
    let build = compiled
        .workspace
        .documentation_repository(&key("site:other"))
        .unwrap()
        .build
        .as_ref()
        .unwrap();
    let value = serde_json::to_value(&build.modules[0].navigation[0]).unwrap();
    assert_eq!(
        value["input"]["recipe"]["version_arguments"][0]["path"],
        "machine:tools/cli.mjs"
    );
    assert_eq!(
        value["input"]["recipe"]["version_arguments"][2],
        "machine:missing/literal"
    );
    for (argument, version, validation) in [
        (
            "{kind='path',path='machine:tools/../escape'}",
            "'--version'",
            "{kind='literal',value='ok'}",
        ),
        (
            "{kind='literal',value='ok'}",
            "{path='../escape'}",
            "{kind='literal',value='ok'}",
        ),
        (
            "{kind='literal',value='ok'}",
            "'--version'",
            "{kind='path',path='machine:tools/../escape'}",
        ),
    ] {
        assert!(
            invalid(&selected(argument, version, validation))
                .iter()
                .any(|error| error.code == "workspace_documentation_generator")
        );
    }
}
