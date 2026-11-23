//! Shared resource inputs remain exact workspace bindings, not acquisition or public placement.

use gazette_compiler::{
    contracts, toml,
    workspace::{
        self, DeclarationKind, ResourceInputMount, ResourceInputName, ThemePackageBinding,
    },
};
use gazette_core::source::{EntryPath, Sha256Digest};
use serde_json::{Value, json};

const OWNERS: &str = "[[principals]]\nid='person:fixture'\nkind='person'\n\
[[sites]]\nid='site:fixture'\npublisher='person:fixture'\ndeployable=false\n";
const SOURCES: &str = "[[sources]]\nid='source:theme'\n\
location={kind='filesystem',root='/does-not-exist/resource-fixture/theme'}\n\
[[sources]]\nid='source:tokens'\n\
location={kind='filesystem',root='/does-not-exist/resource-fixture/tokens'}\n";
const PACKAGE: &str = "[[theme_packages]]\npackage='theme:fixture'\n\
source='source:theme'\nmanifest='theme.toml'\n\
resource_inputs={'shared/_layout.scss'={input='tokens',entry='dist/scss/_layout.scss'},\
'shared/world.css'={input='tokens',entry='dist/css/world.css'}}\n";

fn name(value: &str) -> ResourceInputName {
    value.parse().expect("explicit input name")
}

fn configuration() -> String {
    format!("[design_inputs]\ntokens='source:tokens'\n{OWNERS}{SOURCES}{PACKAGE}")
}

fn diagnostics(source: &str) -> Vec<workspace::Diagnostic> {
    match workspace::compile(source, Default::default()).expect_err("complete workspace refusal") {
        workspace::Error::Invalid(errors) => errors,
        error => panic!("expected semantic reference diagnostics, got {error}"),
    }
}

#[test]
fn named_inputs_and_exact_css_scss_mounts_resolve_without_native_reads_or_role_inference() {
    let source = configuration();
    let compiled = workspace::compile(&source, Default::default())
        .expect("pure compilation never opens nonexistent configured roots");
    let workspace = &compiled.workspace;
    assert_eq!(workspace.design_inputs().len(), 1);
    assert_eq!(
        workspace.design_inputs()[&name("tokens")].as_str(),
        "source:tokens"
    );
    let tokens = workspace
        .design_input(&name("tokens"))
        .expect("exact source binding");
    assert!(std::ptr::eq(
        tokens,
        workspace.source(&"source:tokens".parse().unwrap()).unwrap()
    ));
    assert!(workspace.design_input(&name("unknown")).is_none());
    let package = workspace
        .theme_package(&"theme:fixture".parse().unwrap())
        .unwrap();
    assert_eq!(package.resource_inputs.len(), 2);
    let mount = &package.resource_inputs[&"shared/_layout.scss".parse::<EntryPath>().unwrap()];
    assert_eq!(mount.input, name("tokens"));
    assert_eq!(mount.entry.as_str(), "dist/scss/_layout.scss");
    let model = serde_json::to_value(&compiled).unwrap();
    assert_eq!(
        model["workspace"]["design_inputs"],
        json!({"tokens":"source:tokens"})
    );
    assert_eq!(
        model["workspace"]["theme_packages"]["theme:fixture"]["resource_inputs"]["shared/world.css"],
        json!({"input":"tokens","entry":"dist/css/world.css"})
    );
    assert!(workspace.content_bindings().next().is_none());
    assert!(
        workspace
            .composition_for(&"site:fixture".parse().unwrap())
            .unwrap()
            .resources()
            .is_empty()
    );
    assert!(
        workspace
            .composition_for(&"site:fixture".parse().unwrap())
            .unwrap()
            .plugins()
            .is_empty()
    );
    assert!(
        workspace
            .public_base_url(&"site:fixture".parse().unwrap())
            .is_none()
    );
}

#[test]
fn omission_remains_empty_and_multiple_explicit_names_can_reference_one_source() {
    let source = format!(
        "{OWNERS}{SOURCES}[[theme_packages]]\n\
        package='theme:fixture'\nsource='source:theme'\nmanifest='theme.toml'\n"
    );
    let compiled = workspace::compile(&source, Default::default()).unwrap();
    assert!(compiled.workspace.design_inputs().is_empty());
    let package = compiled.workspace.theme_packages().next().unwrap();
    assert!(package.resource_inputs.is_empty());
    assert!(
        serde_json::to_value(package)
            .unwrap()
            .get("resource_inputs")
            .is_none()
    );
    assert!(
        serde_json::to_value(&compiled.workspace)
            .unwrap()
            .get("design_inputs")
            .is_none()
    );
    let source =
        format!("[design_inputs]\ntokens='source:tokens'\ncolors='source:tokens'\n{source}");
    let compiled = workspace::compile(&source, Default::default()).unwrap();
    assert_eq!(compiled.workspace.design_inputs().len(), 2);
    assert!(std::ptr::eq(
        compiled.workspace.design_input(&name("tokens")).unwrap(),
        compiled.workspace.design_input(&name("colors")).unwrap()
    ));
}

#[test]
fn resource_input_names_have_identical_native_and_generated_schema_admission() {
    let schema = serde_json::to_value(schemars::schema_for!(ResourceInputName)).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let workspace_schema = contracts::workspace();
    let workspace_validator = jsonschema::validator_for(workspace_schema.as_value()).unwrap();
    let longest = format!("a{}", "9".repeat(63));
    let too_long = format!("a{}", "9".repeat(64));
    let cases = [
        ("a", true),
        ("tokens", true),
        ("palette_2-dark", true),
        (&longest, true),
        ("", false),
        ("Tokens", false),
        ("2tokens", false),
        ("_tokens", false),
        ("-tokens", false),
        ("token.name", false),
        ("token/name", false),
        ("source:tokens", false),
        ("tökens", false),
        ("tokens ", false),
        ("tokens\n", false),
        ("tokens\r", false),
        ("tokens\u{2028}", false),
        (&too_long, false),
    ];
    for (value, accepted) in cases {
        assert_eq!(
            value.parse::<ResourceInputName>().is_ok(),
            accepted,
            "{value:?}"
        );
        assert_eq!(
            serde_json::from_value::<ResourceInputName>(json!(value)).is_ok(),
            accepted,
            "{value:?}"
        );
        assert_eq!(validator.is_valid(&json!(value)), accepted, "{value:?}");
        let declaration = json!({"design_inputs": {(value): "source:tokens"}});
        assert_eq!(
            workspace_validator.is_valid(&declaration),
            accepted,
            "map key {value:?}"
        );
        let literal = serde_json::to_string(value).unwrap();
        let source = format!("[design_inputs]\n{literal}='source:tokens'\n{OWNERS}{SOURCES}");
        assert_eq!(
            workspace::compile(&source, Default::default()).is_ok(),
            accepted,
            "TOML key {value:?}"
        );
    }
    assert_eq!(name(&longest).as_str(), longest);
    assert_eq!(name("palette_2-dark").to_string(), "palette_2-dark");
    assert_eq!(
        serde_json::to_value(name("tokens")).unwrap(),
        json!("tokens")
    );
}

#[test]
fn absent_source_and_mount_alias_are_independent_exact_reference_failures() {
    let source = configuration();
    let invalid = source.replace("tokens='source:tokens'", "tokens='source:absent'");
    let errors = diagnostics(&invalid);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "workspace_reference");
    assert_eq!(errors[0].field, "design_inputs[\"tokens\"]");
    assert_eq!(errors[0].key.as_ref().unwrap().as_str(), "source:absent");
    let invalid = source.replace("input='tokens'", "input='absent'");
    let errors = diagnostics(&invalid);
    assert_eq!(errors.len(), 2);
    for (error, target) in errors
        .iter()
        .zip(["shared/_layout.scss", "shared/world.css"])
    {
        assert_eq!(error.code, "workspace_resource_input_reference");
        assert_eq!(
            error.field,
            format!("theme_packages[\"theme:fixture\"].resource_inputs[{target:?}].input")
        );
        assert_eq!(error.key.as_ref().unwrap().as_str(), "theme:fixture");
    }
    let both = invalid.replace("tokens='source:tokens'", "tokens='source:absent'");
    assert_eq!(
        diagnostics(&both).len(),
        3,
        "neither failure is masked or granted a fallback"
    );
}

#[test]
fn mount_objects_paths_and_names_are_strict_in_native_serde_and_public_schema() {
    let schema = serde_json::to_value(schemars::schema_for!(ThemePackageBinding)).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let valid = json!({
        "package":"theme:fixture", "source":"source:theme", "manifest":"theme.toml",
        "resource_inputs":{"shared/_layout.scss":{"input":"tokens","entry":"dist/scss/_layout.scss"}}
    });
    assert!(validator.is_valid(&valid));
    let binding: ThemePackageBinding = serde_json::from_value(valid.clone()).unwrap();
    assert_eq!(serde_json::to_value(binding).unwrap(), valid);
    for mount in [
        json!({"input":"tokens"}),
        json!({"entry":"dist/scss/_layout.scss"}),
        json!({"input":"Tokens","entry":"dist/scss/_layout.scss"}),
        json!({"input":"tokens","entry":"dist/scss/_layout.scss","output":"/tokens.css"}),
        json!({"input":"tokens","entry":"dist/scss/_layout.scss","provider":"tokens"}),
    ] {
        assert!(serde_json::from_value::<ResourceInputMount>(mount.clone()).is_err());
        let mut declaration = valid.clone();
        declaration["resource_inputs"]["shared/_layout.scss"] = mount;
        assert!(!validator.is_valid(&declaration));
    }
    for path in [
        "/absolute.scss",
        "../escape.scss",
        "shared/../escape.scss",
        "shared\\escape.scss",
        "shared//bad.scss",
    ] {
        for entry in [true, false] {
            let mut declaration = valid.clone();
            if entry {
                declaration["resource_inputs"]["shared/_layout.scss"]["entry"] = json!(path);
            } else {
                declaration["resource_inputs"] =
                    json!({(path):{"input":"tokens","entry":"dist/scss/_layout.scss"}});
            }
            assert!(
                serde_json::from_value::<ThemePackageBinding>(declaration.clone()).is_err(),
                "{path:?}"
            );
            assert!(!validator.is_valid(&declaration), "schema {path:?}");
        }
    }
    for changed in [
        configuration().replace("input='tokens'", "input='Tokens'"),
        configuration().replace("entry='dist/scss/_layout.scss'", "entry='../escape.scss'"),
        configuration().replace("'shared/_layout.scss'", "'../escape.scss'"),
        configuration().replace("input='tokens',entry=", "provider='tokens',entry="),
        configuration().replace(
            "entry='dist/scss/_layout.scss'",
            "entry='dist/scss/_layout.scss',unknown=true",
        ),
    ] {
        assert!(matches!(
            workspace::compile(&changed, Default::default()),
            Err(workspace::Error::Toml(_))
        ));
    }
}

#[test]
fn included_documents_merge_disjoint_names_and_refuse_even_equal_duplicate_aliases() {
    let root: EntryPath = "gazette.toml".parse().unwrap();
    let first: EntryPath = "inputs/first.toml".parse().unwrap();
    let second: EntryPath = "inputs/second.toml".parse().unwrap();
    let root_source =
        format!("includes=['inputs/second.toml','inputs/first.toml']\n{OWNERS}{SOURCES}");
    let one = format!("[design_inputs]\ntokens='source:tokens'\n{PACKAGE}");
    let two = "[design_inputs]\ncolors='source:tokens'\n";
    let compile = |second_source: &str| {
        workspace::compile_documents(
            &root,
            [
                (&root, root_source.as_bytes()),
                (&first, one.as_bytes()),
                (&second, second_source.as_bytes()),
            ],
            Default::default(),
        )
    };
    let complete = compile(two).expect("disjoint names share one strict workspace");
    assert_eq!(complete.compilation.workspace.design_inputs().len(), 2);
    assert_eq!(complete.documents.len(), 3);
    for duplicate in ["source:tokens", "source:theme"] {
        let source = format!("[design_inputs]\ntokens='{duplicate}'\n");
        let error =
            compile(&source).expect_err("equal aliases never receive first/last-wins semantics");
        let workspace::Error::Invalid(errors) = error else {
            panic!("duplicate is semantic refusal");
        };
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "workspace_duplicate");
        assert_eq!(errors[0].field, "design_inputs[\"tokens\"]");
    }
    let root_duplicate = format!(
        "includes=['inputs/first.toml']\n[design_inputs]\ntokens='source:tokens'\n{OWNERS}{SOURCES}"
    );
    assert!(
        workspace::compile_documents(
            &root,
            [(&root, root_duplicate.as_bytes()), (&first, one.as_bytes())],
            Default::default()
        )
        .is_err(),
        "root versus include aliases are equally exclusive"
    );
    let reversed = root_source.replace(
        "'inputs/second.toml','inputs/first.toml'",
        "'inputs/first.toml','inputs/second.toml'",
    );
    let reordered = workspace::compile_documents(
        &root,
        [
            (&second, two.as_bytes()),
            (&root, reversed.as_bytes()),
            (&first, one.as_bytes()),
        ],
        Default::default(),
    )
    .unwrap();
    assert_eq!(
        complete.compilation.workspace,
        reordered.compilation.workspace
    );
    assert_ne!(
        complete.compilation.source_sha256,
        reordered.compilation.source_sha256
    );
}

#[test]
fn named_input_source_references_keep_exact_native_cross_document_coordinates() {
    let root: EntryPath = "gazette.toml".parse().unwrap();
    let include: EntryPath = "inputs/theme.toml".parse().unwrap();
    let root_source = format!("includes=['inputs/theme.toml']\n{OWNERS}{SOURCES}");
    let declaration = format!("[design_inputs]\ntokens='source:tokens'\n{PACKAGE}");
    let parsed = workspace::ParsedWorkspaceDocuments::parse(
        &root,
        [
            (&root, root_source.as_bytes()),
            (&include, declaration.as_bytes()),
        ],
        Default::default(),
    )
    .unwrap();
    let offset = declaration.find("source:tokens").unwrap();
    let association = parsed
        .navigation()
        .at(&include, offset)
        .expect("typed source reference");
    assert_eq!(
        association
            .symbol
            .symbol
            .declaration()
            .expect("source declaration")
            .kind,
        DeclarationKind::Source
    );
    assert_eq!(association.symbol.symbol.name(), "source:tokens");
    assert_eq!(association.definition.document, &root);
    assert_eq!(association.occurrence.document, &include);
    assert_eq!(
        association.occurrence.source_sha256,
        Sha256Digest::of(declaration.as_bytes())
    );
    assert_eq!(
        association.occurrence.location.span.slice(&declaration),
        Some("'source:tokens'")
    );
    let reference = parsed.navigation().reference_at(&include, offset).unwrap();
    assert_eq!(
        reference.kind,
        workspace::WorkspaceSymbolKind::Declaration(DeclarationKind::Source)
    );
    assert_eq!(reference.candidates().count(), 2);
    assert!(reference.unique_identity);
    let single = configuration();
    let parsed = workspace::ParsedWorkspace::parse(&single, Default::default()).unwrap();
    let reference = parsed
        .navigation()
        .reference_at(single.find("source:tokens").unwrap())
        .unwrap();
    assert_eq!(reference.symbol.unwrap().name(), "source:tokens");
    assert_eq!(
        reference.location.span.slice(&single),
        Some("'source:tokens'")
    );
}

#[test]
fn generated_workspace_help_exposes_named_input_and_mount_fields_with_native_budgets() {
    for (source, expected) in [
        ("des", "design_inputs"),
        ("[[theme_packages]]\nres", "resource_inputs"),
        (
            "[[theme_packages]]\n[theme_packages.resource_inputs.'shared/_layout.scss']\nin",
            "input",
        ),
    ] {
        let authoring = workspace::WorkspaceAuthoring::parse(source, Default::default()).unwrap();
        let help = authoring.help(source, source.len(), 1 << 18).unwrap();
        assert_eq!(
            help.candidates
                .iter()
                .map(|candidate| candidate.label.as_str())
                .collect::<Vec<_>>(),
            [expected]
        );
        assert_eq!(
            help.candidates[0].presence,
            Some(if expected == "input" {
                workspace::AuthoringFieldPresence::Required
            } else {
                workspace::AuthoringFieldPresence::Optional
            })
        );
    }
    let source = configuration();
    for limits in [
        toml::Limits {
            source_bytes: source.len() - 1,
            ..Default::default()
        },
        toml::Limits {
            nodes: 1,
            ..Default::default()
        },
        toml::Limits {
            depth: 1,
            ..Default::default()
        },
    ] {
        assert!(workspace::compile(&source, limits).is_err());
    }
    let schema: Value = serde_json::to_value(schemars::schema_for!(ResourceInputMount)).unwrap();
    assert_eq!(schema["additionalProperties"], false);
    let mut required = schema["required"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect::<Vec<_>>();
    required.sort_unstable();
    assert_eq!(required, ["entry", "input"]);
}

#[test]
fn alias_keys_and_source_values_have_distinct_native_symbols_and_mount_reference_spans() {
    let input = format!("# 🦀 tokens is just prose\n[design_inputs]\n\"tok\\u0065ns\"='source:tokens'\ncolors='source:tokens'\n{OWNERS}{SOURCES}{PACKAGE}").replace('\n', "\r\n");
    let parsed = workspace::ParsedWorkspace::parse(&input, Default::default()).unwrap();
    let nav = parsed.navigation();
    assert_eq!(
        nav.key_bytes(),
        nav.symbols()
            .iter()
            .map(|symbol| symbol.name().len())
            .sum::<usize>(),
        "decoded alias bytes are included, not their escaped key spelling"
    );
    assert_eq!(
        nav.symbols()
            .iter()
            .filter(|symbol| symbol.kind() == workspace::WorkspaceSymbolKind::ResourceInput)
            .count(),
        2
    );
    let offset = input.find("tok\\u0065ns").unwrap();
    let alias = nav.at(offset).expect("native alias key");
    assert!(
        matches!(alias.symbol, workspace::WorkspaceSymbol::ResourceInput { name, .. } if name.as_str() == "tokens")
    );
    assert_eq!(
        alias.definition.span.slice(&input),
        Some("\"tok\\u0065ns\"")
    );
    assert_eq!(
        alias.definition.precision,
        gazette_core::source::Precision::Construct
    );
    assert_eq!(alias.references.len(), 2);
    assert!(
        nav.reference_at(offset).is_none(),
        "alias declaration is not an edit slot"
    );
    let source = nav.at(input.find("='source:tokens'").unwrap() + 2).unwrap();
    assert_eq!(
        source.symbol.kind(),
        workspace::WorkspaceSymbolKind::Declaration(DeclarationKind::Source)
    );
    assert_ne!(
        source.definition, alias.definition,
        "value is the independently declared source"
    );
    for occurrence in alias.references {
        assert_eq!(occurrence.span.slice(&input), Some("'tokens'"));
        let reference = nav.reference_at(occurrence.span.start).unwrap();
        assert_eq!(
            reference.kind,
            workspace::WorkspaceSymbolKind::ResourceInput
        );
        assert_eq!(
            reference.location.precision,
            gazette_core::source::Precision::Exact
        );
        assert_eq!(reference.symbol, Some(alias.symbol));
        let candidates = reference.candidates().collect::<Vec<_>>();
        assert_eq!(
            candidates
                .iter()
                .map(|symbol| symbol.name())
                .collect::<Vec<_>>(),
            ["tokens", "colors"]
        );
        for symbol in candidates {
            let scalar = symbol.replacement(&input).unwrap();
            let value = scalar
                .parse::<toml_edit::Value>()
                .expect("native TOML string, not key spelling");
            assert_eq!(value.as_str(), Some(symbol.name()));
        }
    }
    assert!(
        nav.at(input.find("tokens is just prose").unwrap())
            .is_none()
    );
    assert!(
        nav.at(input.find("dist/scss/_layout.scss").unwrap())
            .is_none()
    );

    let inline = format!("design_inputs={{tokens='source:tokens'}}\n{OWNERS}{SOURCES}{PACKAGE}");
    let parsed = workspace::ParsedWorkspace::parse(&inline, Default::default()).unwrap();
    let alias = parsed
        .navigation()
        .at(inline.find("tokens=").unwrap())
        .unwrap();
    assert_eq!(alias.definition.span.slice(&inline), Some("tokens"));
    assert_eq!(alias.symbol.replacement(&inline).unwrap(), "\"tokens\"");

    let source = parsed
        .navigation()
        .at(inline.find("tokens='source:tokens'").unwrap() + "tokens=".len())
        .unwrap();
    assert_eq!(alias.symbol.name(), "tokens");
    assert_eq!(source.symbol.name(), "source:tokens");
    assert_eq!(
        alias.symbol.kind(),
        workspace::WorkspaceSymbolKind::ResourceInput
    );
    assert_eq!(
        source.symbol.kind(),
        workspace::WorkspaceSymbolKind::Declaration(DeclarationKind::Source)
    );
    assert_ne!(
        alias.definition, source.definition,
        "matching suffixes never merge the two native namespaces"
    );
    let invalid_plain_key = inline.replace("source:tokens", "tokens");
    assert!(
        workspace::ParsedWorkspace::parse(&invalid_plain_key, Default::default()).is_err(),
        "a resource-input alias cannot replace a namespaced source Key"
    );
}

#[test]
fn included_alias_definition_and_mount_reference_keep_their_own_document_and_baseline() {
    let root: EntryPath = "gazette.toml".parse().unwrap();
    let names: EntryPath = "inputs/names.toml".parse().unwrap();
    let mounts: EntryPath = "inputs/mounts.toml".parse().unwrap();
    let source = format!("includes=['inputs/mounts.toml','inputs/names.toml']\n{OWNERS}{SOURCES}");
    let definitions = "# é\r\n[design_inputs]\r\n 'tokens' = 'source:tokens'\r\n";
    let parsed = workspace::ParsedWorkspaceDocuments::parse(
        &root,
        [
            (&mounts, PACKAGE.as_bytes()),
            (&names, definitions.as_bytes()),
            (&root, source.as_bytes()),
        ],
        Default::default(),
    )
    .unwrap();
    let offset = PACKAGE.find("input='tokens'").unwrap() + "input=".len();
    let association = parsed.navigation().at(&mounts, offset).unwrap();
    assert_eq!(association.definition.document, &names);
    assert_eq!(
        association.definition.source_sha256,
        Sha256Digest::of(definitions.as_bytes())
    );
    assert_eq!(
        association.definition.location.span.slice(definitions),
        Some("'tokens'")
    );
    assert_eq!(association.occurrence.document, &mounts);
    assert_eq!(
        association.occurrence.source_sha256,
        Sha256Digest::of(PACKAGE.as_bytes())
    );
    assert_eq!(
        association.occurrence.location.span.slice(PACKAGE),
        Some("'tokens'")
    );
    assert_eq!(association.references.len(), 2);
    let slot = parsed.navigation().reference_at(&mounts, offset).unwrap();
    assert_eq!(
        slot.candidates().next().unwrap().replacement(),
        "\"tokens\""
    );
    let definition = parsed
        .navigation()
        .at(&names, definitions.find("'tokens'").unwrap())
        .unwrap();
    assert_eq!(definition.references, association.references);
    let source_ref = parsed
        .navigation()
        .at(&names, definitions.find("source:tokens").unwrap())
        .unwrap();
    assert_eq!(source_ref.definition.document, &root);
    assert_eq!(
        source_ref.symbol.symbol.kind(),
        workspace::WorkspaceSymbolKind::Declaration(DeclarationKind::Source)
    );
}

#[test]
fn missing_alias_keeps_typed_repair_candidates_but_ambiguous_includes_never_choose_a_target() {
    let input = format!(
        "[design_inputs]\ntokens='source:tokens'\n{OWNERS}{SOURCES}{}",
        PACKAGE.replace("input='tokens'", "input='unknown'")
    );
    let (compilation, navigation) =
        workspace::WorkspaceInspection::parse(&input, Default::default())
            .unwrap()
            .into_parts();
    assert!(
        compilation.is_err(),
        "coordinates never admit a missing alias"
    );
    let navigation = navigation.expect("unambiguous typed source facts");
    let slot = navigation
        .reference_at(input.find("input='unknown'").unwrap() + "input=".len())
        .unwrap();
    assert!(slot.symbol.is_none());
    assert_eq!(slot.kind, workspace::WorkspaceSymbolKind::ResourceInput);
    assert_eq!(
        slot.candidates()
            .map(|symbol| symbol.name())
            .collect::<Vec<_>>(),
        ["tokens"]
    );
    assert!(
        !slot.unique_identity,
        "two unknown references cannot bind one unlocated diagnostic"
    );
    let root: EntryPath = "gazette.toml".parse().unwrap();
    let included: EntryPath = "names.toml".parse().unwrap();
    let first = format!(
        "includes=['names.toml']\n[design_inputs]\ntokens='source:tokens'\n{OWNERS}{SOURCES}{PACKAGE}"
    );
    let second = "[design_inputs]\ntokens='source:tokens'\n";
    assert!(
        workspace::ParsedWorkspaceDocuments::parse(
            &root,
            [(&root, first.as_bytes()), (&included, second.as_bytes())],
            Default::default()
        )
        .is_err(),
        "duplicate equal aliases refuse the complete set, never a first/last-wins catalogue"
    );
}
