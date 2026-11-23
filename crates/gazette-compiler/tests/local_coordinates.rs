//! Configured local addresses are pure projections, not listeners or public identity changes.

use gazette_compiler::workspace;

fn key(value: &str) -> workspace::Key {
    value.parse().unwrap()
}

fn source(port: &str, output: &str) -> String {
    format!(
        "[[principals]]\nid='person:owner'\nkind='person'\n\
        [[sites]]\nid='site:one'\npublisher='person:owner'\ndeployable=false\n\
        [[sites]]\nid='site:two'\npublisher='person:owner'\ndeployable=false\n\
        [[brands]]\nid='brand:one'\nname='Example'\n\
        [[brands.publications]]\nsite='site:one'\npublic_base_url='https://one.test/pr%C3%A9fix/'\n\
        local_port={port}\noutput={{directory='{output}'}}\n\
        [[brands.publications]]\nsite='site:two'\npublic_base_url='https://two.test/'\nlocal_port=4312\n"
    )
}

#[test]
fn configured_local_addresses_retain_public_prefixes_and_do_not_mutate_public_configuration() {
    let compiled =
        workspace::compile(&source("4311", "generated/one"), Default::default()).unwrap();
    let original = serde_json::to_vec(&compiled).unwrap();
    let urls = compiled
        .workspace
        .local_urls(&[key("site:two"), key("site:one")])
        .unwrap();
    assert_eq!(
        urls[&key("site:one")].as_str(),
        "http://127.0.0.1:4311/pr%C3%A9fix/"
    );
    assert_eq!(urls[&key("site:two")].as_str(), "http://127.0.0.1:4312/");
    assert_eq!(serde_json::to_vec(&compiled).unwrap(), original);
    let output = &compiled
        .workspace
        .publication_for(&key("site:one"))
        .unwrap()
        .output
        .as_ref()
        .unwrap()
        .directory;
    assert_eq!(output.to_string(), "generated/one");
    assert!(output.workspace_relative().is_some());
}

#[test]
fn local_projection_refuses_incomplete_and_conflicting_sets_without_fallback() {
    let compiled =
        workspace::compile(&source("4312", "generated/one"), Default::default()).unwrap();
    assert!(compiled.workspace.local_urls(&[]).is_err());
    assert!(
        compiled
            .workspace
            .local_urls(&[key("site:one"), key("site:one")])
            .is_err()
    );
    assert!(
        compiled
            .workspace
            .local_urls(&[key("site:one"), key("site:two")])
            .is_err()
    );
    assert!(
        compiled
            .workspace
            .local_urls(&[key("site:missing")])
            .is_err()
    );
    assert!(compiled.workspace.local_urls(&[key("site:one")]).is_ok());
    for missing in ["local_port=4312\n", "public_base_url='https://two.test/'\n"] {
        let source = source("4311", "generated/one").replace(missing, "");
        let compiled = workspace::compile(&source, Default::default()).unwrap();
        assert!(
            compiled
                .workspace
                .local_urls(&[key("site:one"), key("site:two")])
                .is_err()
        );
    }
}

#[test]
fn native_schema_rejects_zero_ports_and_unsafe_or_unknown_output_coordinates() {
    for port in ["0", "65536", "-1", "'4311'"] {
        assert!(workspace::compile(&source(port, "generated/one"), Default::default()).is_err());
    }
    for output in ["../escape", "generated/../escape", "generated//one"] {
        assert!(workspace::compile(&source("4311", output), Default::default()).is_err());
    }
    let unknown = source("4311", "generated/one").replace(
        "output={directory='generated/one'}",
        "output={directory='generated/one',replace=true}",
    );
    assert!(workspace::compile(&unknown, Default::default()).is_err());
    let schema = schemars::schema_for!(workspace::BrandPublication);
    let text = serde_json::to_string(&schema).unwrap();
    assert!(text.contains("local_port"));
    assert!(text.contains("PublicationOutput"));
    assert!(text.contains("minimum"));
}

#[test]
fn output_coordinates_retain_machine_root_names_without_native_paths() {
    let compiled =
        workspace::compile(&source("4311", "machine:builds/one"), Default::default()).unwrap();
    assert_eq!(
        compiled
            .workspace
            .publication_for(&key("site:one"))
            .unwrap()
            .output
            .as_ref()
            .unwrap()
            .directory
            .to_string(),
        "machine:builds/one"
    );
}
