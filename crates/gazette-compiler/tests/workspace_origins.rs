//! Origin queries traverse retained syntax after the real complete-topology resolver.

use gazette_compiler::{
    toml::Limits,
    workspace::{self, FieldQuery, OriginError, ParsedWorkspace},
};

const BASE: &str = "[[principals]]\nid='owner:one'\nkind='organization'\n[[sites]]\nid='site:z'\npublisher='owner:one'\ndeployable=false\n[[sites]]\nid='site:a'\npublisher='owner:one'\ndeployable=true\n";

fn query(kind: &str, identity: &str, field: serde_json::Value) -> FieldQuery {
    serde_json::from_value(serde_json::json!({"kind":kind, "identity":identity, "field":field}))
        .expect("shared query contract")
}

#[test]
fn semantic_sorting_never_changes_the_original_field_coordinate() {
    let source = format!(
        "# 🦀 name='decoy'\r\n{BASE}[[brands]]\nid='brand:one'\nname='Rêve'\n[[brands.publications]]\nsite='site:z'\ndocumentation_repository={{name='chosen',path='/generated/z',destination={{kind='git',remote='https://example.invalid/z.git',branch='docs'}}}}\n[[brands.publications]]\nsite='site:a'\ndocumentation_repository={{name='other',path='/generated/a'}}\n"
    );
    let parsed = ParsedWorkspace::parse(&source, Limits::default()).expect("same compiler");
    assert_eq!(
        parsed.compilation(),
        &workspace::compile(&source, Limits::default()).expect("ordinary compilation")
    );
    assert_eq!(
        parsed
            .compilation()
            .workspace
            .sites()
            .next()
            .expect("sorted")
            .id
            .as_str(),
        "site:a"
    );
    for (query, expected) in [
        (
            query("brand", "brand:one", serde_json::json!(["name"])),
            "'Rêve'",
        ),
        (
            query("site", "site:z", serde_json::json!(["deployable"])),
            "false",
        ),
        (
            query(
                "brand",
                "brand:one",
                serde_json::json!(["publications", 0, "documentation_repository", "path"]),
            ),
            "'/generated/z'",
        ),
        (
            query(
                "brand",
                "brand:one",
                serde_json::json!(["publications", 1, "documentation_repository", "name"]),
            ),
            "'other'",
        ),
        (
            query(
                "brand",
                "brand:one",
                serde_json::json!([
                    "publications",
                    0,
                    "documentation_repository",
                    "destination",
                    "remote"
                ]),
            ),
            "'https://example.invalid/z.git'",
        ),
    ] {
        let origin = parsed.origin(&query).expect("explicit origin");
        assert_eq!(&source[origin.span.start..origin.span.end], expected);
        assert_eq!(origin.source_sha256, parsed.compilation().source_sha256);
        assert_eq!(origin.identity, query.identity);
    }
    assert_eq!(parsed.source(), source);
}

#[test]
fn source_and_principal_namespaces_cannot_choose_each_others_coordinates() {
    let source = format!(
        "{BASE}[[sources]]\nid='owner:one'\nlocation={{kind='repository',root='/explicit/source',commit='{}',subdirectory='docs'}}\n",
        "0".repeat(40)
    );
    let parsed = ParsedWorkspace::parse(&source, Limits::default())
        .expect("source binding is a distinct namespace");
    let principal = parsed
        .origin(&query(
            "principal",
            "owner:one",
            serde_json::json!(["kind"]),
        ))
        .expect("principal");
    assert_eq!(
        &source[principal.span.start..principal.span.end],
        "'organization'"
    );
    let binding = parsed
        .origin(&query(
            "source",
            "owner:one",
            serde_json::json!(["location", "kind"]),
        ))
        .expect("source");
    assert_eq!(
        &source[binding.span.start..binding.span.end],
        "'repository'"
    );
    assert_eq!(
        parsed.origin(&query("brand", "owner:one", serde_json::json!(["kind"]))),
        Err(OriginError::UnknownIdentity)
    );
}

#[test]
fn missing_fields_defaults_and_invalid_graphs_do_not_acquire_fabricated_origins() {
    let source = format!("{BASE}[[brands]]\nid='brand:one'\nname='Brand'\n");
    let parsed =
        ParsedWorkspace::parse(&source, Limits::default()).expect("brand without publications");
    assert!(
        parsed
            .compilation()
            .workspace
            .brands()
            .next()
            .expect("brand")
            .publications
            .is_empty()
    );
    assert_eq!(
        parsed.origin(&query(
            "brand",
            "brand:one",
            serde_json::json!(["publications"])
        )),
        Err(OriginError::NoExplicitField)
    );
    assert_eq!(
        parsed.origin(&query("site", "site:absent", serde_json::json!(["id"]))),
        Err(OriginError::UnknownIdentity)
    );
    for invalid in [
        source.replace("publisher='owner:one'", "publisher='owner:missing'"),
        format!("{source}[[brands]]\nid='brand:one'\nname='duplicate'"),
        format!("{source}unknown='not ignored'"),
    ] {
        assert!(ParsedWorkspace::parse(&invalid, Limits::default()).is_err());
    }
}
