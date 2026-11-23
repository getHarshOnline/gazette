//! Plural authored bindings share native resolution, source coordinates and schemas.

use gazette_compiler::{contracts, workspace};

const DECLARATIONS: &str = "\
[[principals]]\nid='person:author'\nkind='person'\n\
[[sites]]\nid='site:one'\npublisher='person:author'\ndeployable=false\n\
[[sources]]\nid='source:root'\nlocation={kind='filesystem',root='content'}\n\
[[sources]]\nid='source:descendant'\nlocation={kind='filesystem',root='other-content'}\n\
[[brands]]\nid='brand:one'\nname='One'\n\
[[brands.publications]]\nsite='site:one'\n";

#[test]
fn plural_bindings_preserve_order_and_exact_native_reference_locations() {
    let source = format!(
        "{DECLARATIONS}authored=[\n\
        {{source='source:descendant',target='captured/descendant'}},\n\
        {{source='source:root',target='captured/root'}}]\n"
    );
    let parsed = workspace::ParsedWorkspace::parse(&source, Default::default())
        .expect("complete native workspace");
    let bindings: Vec<_> = parsed.compilation().workspace.content_bindings().collect();
    assert_eq!(bindings.len(), 2);
    assert_eq!(bindings[0].source_key().as_str(), "source:descendant");
    assert_eq!(bindings[1].source_key().as_str(), "source:root");
    for binding in bindings {
        assert_eq!(binding.site.as_str(), "site:one");
        assert!(
            binding.project.is_none(),
            "source placement invents no project"
        );
        let needle = format!("source='{}'", binding.source_key());
        let offset = source.rfind(&needle).expect("selected reference") + "source=".len();
        let reference = parsed
            .navigation()
            .reference_at(offset)
            .expect("array reference");
        assert_eq!(
            reference.symbol.expect("exact declaration").name(),
            binding.source_key().as_str()
        );
    }
}

#[test]
fn authored_binding_array_tables_and_empty_selection_use_the_same_contract() {
    let source = format!(
        "{DECLARATIONS}\
        [[brands.publications.authored]]\nsource='source:root'\ntarget='root'\n\
        [[brands.publications.authored]]\nsource='source:descendant'\ntarget='descendant'\n"
    );
    let compiled = workspace::compile(&source, Default::default()).expect("array tables");
    assert_eq!(compiled.workspace.content_bindings().count(), 2);
    for suffix in ["", "authored=[]\n"] {
        let empty = workspace::compile(&format!("{DECLARATIONS}{suffix}"), Default::default())
            .expect("configuration can select no authored inputs");
        assert_eq!(empty.workspace.content_bindings().count(), 0);
    }
}

#[test]
fn every_binding_is_validated_before_any_topology_is_returned() {
    for second in [
        "source='source:missing',target='other'",
        "source='source:descendant',target='root'",
        "source='source:descendant',target='root/nested'",
        "source='source:descendant',target='other',static_assets={route_prefix='/not-a-directory'}",
    ] {
        let source = format!(
            "{DECLARATIONS}authored=[{{source='source:root',target='root'}},{{{second}}}]\n"
        );
        assert!(
            workspace::compile(&source, Default::default()).is_err(),
            "{second}"
        );
    }
}

#[test]
fn singular_and_positional_bindings_are_refused_without_compatibility_fallback() {
    for authored in [
        "{source='source:root',target='root'}",
        "[['source:root','root']]",
    ] {
        let source = format!("{DECLARATIONS}authored={authored}\n");
        assert!(workspace::compile(&source, Default::default()).is_err());
    }
}

#[test]
fn generated_workspace_schema_exposes_authored_as_an_array() {
    let schema = serde_json::to_value(contracts::workspace()).expect("native schema");
    let authored = &schema["$defs"]["BrandPublication"]["properties"]["authored"];
    assert_eq!(authored["type"], "array");
    assert!(authored["items"].is_object());
}
