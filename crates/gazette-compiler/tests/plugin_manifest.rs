//! Plugin packages keep capability, provider, resource and package identities separate.

use gazette_compiler::{plugin::ParsedManifest, toml};
use gazette_core::{plugin::PluginPackageId, publication::capability_runtime::ProviderId};
use gazette_core::{
    publication::{CapabilityId, CoreCapabilityCatalog},
    resource::ResourceId,
    source::Sha256Digest,
};
use serde_json::json;

const SOURCE: &str = "package='shopify-commerce'\n\
configuration_schema='schemas/configuration.schema.json'\n\
[resources.'module:commerce-selector']\n\
inputs=['browser/selector.ts','browser/selector-model.ts']\n\
build={kind='browser-module',entry='browser/selector.ts'}\n\
[resources.'stylesheet:commerce-interactions']\n\
inputs=['styles/interactions.scss','styles/tokens.scss']\n\
build={kind='stylesheet',entry='styles/interactions.scss'}\n\
[extensions.'shopify-commerce.catalog/v1']\n\
schema='schemas/catalog.schema.json'\n\
[components.product-card]\n\
arguments='schemas/product-card.arguments.schema.json'\n\
model='schemas/product-card.model.schema.json'\n\
module='module:commerce-selector'\n\
[providers.shopify-store]\n\
implements=['commerce.catalog/v1','shopify-commerce.catalog/v1']\n";

fn capability(value: &str) -> CapabilityId {
    value.parse().expect("fixture capability")
}

fn parse(source: &str) -> Result<ParsedManifest<'_>, gazette_compiler::plugin::Error> {
    ParsedManifest::parse(source, toml::Limits::default())
}

#[test]
fn core_capabilities_are_engine_owned_and_closed_to_fixture_authorization() {
    assert_eq!(
        CoreCapabilityCatalog::IDS,
        [
            "commerce.catalog/v1",
            "commerce.inventory/v1",
            "commerce.selector/v1",
            "publication.analytics/v1",
            "publication.search/v1",
            "publication.webmcp/v1",
        ]
    );
    for identity in CoreCapabilityCatalog::IDS {
        assert!(CoreCapabilityCatalog::contains(&capability(identity)));
    }
    assert!(!CoreCapabilityCatalog::contains(&capability(
        "publication.fixture/v1"
    )));
    assert!(
        parse(&SOURCE.replace("commerce.catalog/v1", "publication.fixture/v1")).is_err(),
        "a well-formed fixture identity is not a core contract"
    );
}

#[test]
fn complete_manifest_preserves_distinct_typed_exports_and_digest_identity() {
    let parsed = parse(SOURCE).expect("strict complete manifest");
    let manifest = parsed.manifest();
    assert_eq!(manifest.package.as_str(), "shopify-commerce");
    let provider: ProviderId = "shopify-store".parse().unwrap();
    let resource: ResourceId = "module:commerce-selector".parse().unwrap();
    assert!(
        manifest.providers[&provider]
            .implements
            .contains(&capability("commerce.catalog/v1"))
    );
    assert_eq!(
        manifest.resources[&resource].inputs[0].as_str(),
        "browser/selector-model.ts"
    );
    assert_eq!(
        manifest.resources[&resource].build.entry().as_str(),
        "browser/selector.ts"
    );
    assert_eq!(
        manifest.components[&"product-card".parse().unwrap()].module,
        Some(resource)
    );
    let source = Sha256Digest::of(b"selected source binding");
    let tree = Sha256Digest::of(b"captured package tree");
    let identity = parsed.identity(source, tree);
    assert_eq!(identity.package, "shopify-commerce".parse().unwrap());
    assert_eq!(identity.source, source);
    assert_eq!(identity.manifest, Sha256Digest::of(SOURCE.as_bytes()));
    assert_eq!(identity.tree, tree);
}

#[test]
fn identifiers_are_independent_contracts_with_non_interchangeable_grammars() {
    assert!("commerce.catalog/v1".parse::<CapabilityId>().is_ok());
    assert!("commerce.catalog/v1".parse::<ProviderId>().is_err());
    assert!("shopify-store".parse::<ProviderId>().is_ok());
    assert!("shopify-store".parse::<PluginPackageId>().is_ok());
    assert!("module:selector".parse::<ResourceId>().is_ok());
    assert!("module:selector".parse::<ProviderId>().is_err());
    for invalid in [
        "catalog/v1",
        "commerce.catalog",
        "commerce.catalog/latest",
        "Commerce.catalog/v1",
        "commerce..catalog/v1",
    ] {
        assert!(invalid.parse::<CapabilityId>().is_err(), "{invalid}");
    }
}

#[test]
fn identifier_json_schemas_enforce_the_same_grammars_as_native_parsing() {
    for (schema, accepted, rejected) in [
        (
            schemars::schema_for!(CapabilityId),
            json!("commerce.catalog/v1"),
            json!("catalog/v1"),
        ),
        (
            schemars::schema_for!(ProviderId),
            json!("shopify-store"),
            json!("provider:shopify"),
        ),
        (
            schemars::schema_for!(PluginPackageId),
            json!("shopify-commerce"),
            json!("Shopify"),
        ),
        (
            schemars::schema_for!(ResourceId),
            json!("module:selector"),
            json!("selector"),
        ),
    ] {
        let schema = serde_json::to_value(schema).expect("JSON Schema projection");
        let validator = jsonschema::validator_for(&schema).expect("generated identifier schema");
        assert!(validator.is_valid(&accepted));
        assert!(!validator.is_valid(&rejected));
    }
}

#[test]
fn unknown_fields_versions_and_execution_recipes_are_not_compatibility_escape_hatches() {
    for addition in [
        "version='1.0.0'\n",
        "compat='gazette/v1'\n",
        "executor='node'\n",
        "dynamic_library='plugin.so'\n",
        "unknown=true\n",
    ] {
        let source = SOURCE.replacen(
            "package='shopify-commerce'\n",
            &format!("package='shopify-commerce'\n{addition}"),
            1,
        );
        assert!(parse(&source).is_err(), "{addition}");
    }
    assert!(
        parse(&SOURCE.replace(
            "implements=['commerce.catalog/v1','shopify-commerce.catalog/v1']",
            "implements=['commerce.catalog/v1','shopify-commerce.catalog/v1']\nexecutor='node'",
        ))
        .is_err()
    );
    assert!(
        parse(&SOURCE.replace(
            "implements=['commerce.catalog/v1','shopify-commerce.catalog/v1']",
            "implements=['commerce.catalog/v1','shopify-commerce.catalog/v1']\nresources=['module:commerce-selector']",
        ))
        .is_err(),
        "providers cannot recapture independently selected static resources"
    );
}

#[test]
fn local_paths_are_required_for_resources_and_extension_schemas() {
    for (from, invalid) in [
        ("browser/selector.ts", "https://example.invalid/selector.js"),
        ("browser/selector.ts", "/absolute/selector.js"),
        ("browser/selector.ts", "../selector.js"),
        (
            "schemas/catalog.schema.json",
            "https://example.invalid/schema.json",
        ),
        ("schemas/catalog.schema.json", "/absolute/schema.json"),
        ("schemas/catalog.schema.json", "schemas/../schema.json"),
        (
            "schemas/configuration.schema.json",
            "/absolute/configuration.json",
        ),
        (
            "schemas/product-card.arguments.schema.json",
            "https://example.invalid/arguments.json",
        ),
        (
            "schemas/product-card.model.schema.json",
            "../model.schema.json",
        ),
    ] {
        assert!(
            parse(&SOURCE.replacen(from, invalid, 1)).is_err(),
            "{invalid}"
        );
    }
}

#[test]
fn every_provider_reference_closes_over_declared_capabilities() {
    assert!(
        parse(&SOURCE.replace(
            "'commerce.catalog/v1','shopify-commerce.catalog/v1'",
            "'commerce.unknown/v1','shopify-commerce.catalog/v1'",
        ))
        .is_err()
    );
    assert!(
        parse(&SOURCE.replace(
            "'commerce.catalog/v1','shopify-commerce.catalog/v1'",
            "'commerce.catalog/v1','commerce.catalog/v1'",
        ))
        .is_err()
    );
}

#[test]
fn extension_capabilities_are_package_namespaced_and_provider_backed() {
    assert!(
        parse(&SOURCE.replace("shopify-commerce.catalog/v1", "unrelated.catalog/v1",)).is_err()
    );
    let providerless = SOURCE.replace(
        "implements=['commerce.catalog/v1','shopify-commerce.catalog/v1']",
        "implements=['commerce.catalog/v1']",
    );
    assert!(parse(&providerless).is_err());
    assert!(
        parse("package='shopify-commerce'\n[providers.shopify-store]\nimplements=[]\n").is_err()
    );
}

#[test]
fn resource_recipes_reuse_bounded_inputs_and_component_modules_close_exactly() {
    assert!(
        parse(&SOURCE.replace(
            "inputs=['browser/selector.ts','browser/selector-model.ts']",
            "inputs=[]",
        ))
        .is_err()
    );
    assert!(
        parse(&SOURCE.replace(
            "inputs=['browser/selector.ts','browser/selector-model.ts']",
            "inputs=['browser/selector.ts','browser/selector.ts']",
        ))
        .is_err()
    );
    assert!(
        parse(&SOURCE.replace("entry='browser/selector.ts'", "entry='browser/missing.ts'",))
            .is_err()
    );
    assert!(
        parse(&SOURCE.replace(
            "module='module:commerce-selector'",
            "module='module:missing'",
        ))
        .is_err()
    );
    assert!(
        parse(&SOURCE.replace(
            "module='module:commerce-selector'",
            "module='stylesheet:commerce-interactions'",
        ))
        .is_err()
    );
    assert!(
        parse(&SOURCE.replace(
            "build={kind='browser-module',entry='browser/selector.ts'}",
            "build={kind='browser-module',entry='browser/selector.ts',imports={'../escape'='module:shared'}}",
        ))
        .is_err()
    );
}

#[test]
fn a_resource_only_package_is_valid_without_inventing_a_provider() {
    let source = "package='plain-assets'\n[resources.'stylesheet:base']\ninputs=['styles/base.css']\nbuild={kind='stylesheet',entry='styles/base.css'}\n";
    let parsed = parse(source).expect("resources need no synthetic capability provider");
    assert!(parsed.manifest().providers.is_empty());
    assert!(parsed.manifest().extensions.is_empty());
}

#[test]
fn an_empty_package_has_no_implicit_exports() {
    let error = parse("package='empty-package'\n").expect_err("empty package must fail");
    assert_eq!(error.code(), "plugin_manifest_invalid");
}
