//! Actual workspace declarations retain exact delivery choices, never inferred authority.

use gazette_compiler::{toml, workspace};
use gazette_core::release::PublicationDestination;

const TOPOLOGY: &str = r#"
[[principals]]
id = 'owner:one'
kind = 'person'
[[sites]]
id = 'site:one'
publisher = 'owner:one'
deployable = false
[[sites]]
id = 'site:two'
publisher = 'owner:one'
deployable = false
"#;

fn brand(site: &str, destination: &str) -> String {
    format!(
        "[[brands]]\nid='brand:{site}'\nname='Display {site}'\n[[brands.publications]]\nsite='site:{site}'\npublic_base_url='https://{site}.example.invalid/'\n{destination}\n"
    )
}

fn compile(publications: &str) -> Result<workspace::Compilation, workspace::Error> {
    workspace::compile(
        &format!("{TOPOLOGY}\n{publications}"),
        toml::Limits::default(),
    )
}

#[test]
fn absence_remains_local_only_and_selected_destination_is_the_same_brand_value() {
    let plain = compile(&brand("one", "")).expect("local-only declaration");
    let one = "site:one".parse().expect("site key");
    let two = "site:two".parse().expect("other site key");
    assert!(plain.workspace.destination(&one).is_none());
    assert!(plain.workspace.destination(&two).is_none());
    let selected = compile(&brand("one", "[brands.publications.destination]\nkind='git'\nremote='git@EXAMPLE.invalid:Exact Repository.git'\nbranch='release/é'"))
        .expect("native compile does not resolve remote or branch");
    let destination = selected
        .workspace
        .destination(&one)
        .expect("explicit delivery");
    assert_eq!(
        destination,
        &PublicationDestination::Git {
            remote: "git@EXAMPLE.invalid:Exact Repository.git".into(),
            branch: "release/é".into()
        }
    );
    assert!(std::ptr::eq(
        destination,
        selected
            .workspace
            .publication_for(&one)
            .expect("brand publication")
            .destination
            .as_ref()
            .expect("sole declaration")
    ));
    assert!(
        selected.workspace.destination(&two).is_none(),
        "other site does not inherit destination"
    );
    assert_eq!(
        plain.workspace.sites().collect::<Vec<_>>(),
        selected.workspace.sites().collect::<Vec<_>>()
    );
    assert_eq!(
        plain.workspace.principals().collect::<Vec<_>>(),
        selected.workspace.principals().collect::<Vec<_>>()
    );
    assert_eq!(
        plain.workspace.public_base_url(&one),
        selected.workspace.public_base_url(&one)
    );
}

#[test]
fn destination_shape_is_a_strict_optional_named_table() {
    for declaration in [
        "destination=['git','origin','main']",
        "destination='origin'",
        "destination={kind='github',remote='origin',branch='main'}",
        "destination={kind='git',remote='origin'}",
        "destination={kind='git',remote='origin',branch='main',force=false}",
        "destination={kind='git',remote=2,branch='main'}",
        "destination={kind='git',remote='origin',branch=['main']}",
        "destination={kind='git',remote='origin',remote='another',branch='main'}",
    ] {
        assert!(
            compile(&brand("one", declaration)).is_err(),
            "{declaration}"
        );
    }
}

#[test]
fn destination_branch_origin_uses_the_retained_workspace_syntax_and_digest() {
    let source = format!(
        "# 🦀 branch='decoy'\r\n{TOPOLOGY}\n{}",
        brand(
            "one",
            "[brands.publications.destination]\nkind='git'\nremote='origin'\nbranch='release/é'"
        )
    );
    let parsed = workspace::ParsedWorkspace::parse(&source, toml::Limits::default())
        .expect("shared parsed workspace");
    let query: workspace::FieldQuery = serde_json::from_value(serde_json::json!({
        "kind":"brand", "identity":"brand:one",
        "field":["publications",0,"destination","branch"]
    }))
    .expect("existing editor-independent query");
    let origin = parsed.origin(&query).expect("explicit source field");
    assert_eq!(&source[origin.span.start..origin.span.end], "'release/é'");
    assert_eq!(origin.source_sha256, parsed.compilation().source_sha256);
    assert_eq!(origin.identity, query.identity);
    assert_eq!(parsed.source(), source);
}

#[test]
fn invalid_coordinates_refuse_complete_compilation_without_exposing_remote() {
    for (remote, branch) in [
        ("PRIVATE remote", "../main"),
        (" PRIVATE remote", "main"),
        ("PRIVATE\nremote", "main"),
        ("PRIVATE remote", "release.lock"),
        ("PRIVATE remote", ""),
    ] {
        let destination = format!(
            "destination={{kind='git',remote={},branch={}}}",
            serde_json::to_string(remote).expect("TOML-compatible quoted string"),
            serde_json::to_string(branch).expect("quoted branch")
        );
        let workspace::Error::Invalid(errors) = compile(&brand("one", &destination))
            .expect_err("invalid destination rejects complete topology")
        else {
            panic!("expected native semantic diagnostics");
        };
        assert!(errors.iter().any(|error| {
            error.code == "workspace_destination"
                && error
                    .key
                    .as_ref()
                    .is_some_and(|key| key.as_str() == "site:one")
        }));
        assert!(
            !serde_json::to_string(&errors)
                .expect("diagnostic projection")
                .contains("PRIVATE")
        );
    }
}

#[test]
fn exact_remote_branch_duplicates_report_every_site_independent_of_declaration_order() {
    let destination = "destination={kind='git',remote='PRIVATE remote',branch='main'}";
    let one = brand("one", destination);
    let two = brand("two", destination);
    let errors = |source: String| {
        let workspace::Error::Invalid(errors) =
            compile(&source).expect_err("duplicate exact delivery target")
        else {
            panic!("expected semantic duplicate");
        };
        errors
    };
    let forward = errors(format!("{one}\n{two}"));
    assert_eq!(forward, errors(format!("{two}\n{one}")));
    assert_eq!(forward.len(), 2);
    assert_eq!(
        forward
            .iter()
            .map(|error| error.key.as_ref().expect("affected site").as_str())
            .collect::<Vec<_>>(),
        ["site:one", "site:two"]
    );
    assert!(
        forward
            .iter()
            .all(|error| error.code == "workspace_destination_duplicate"
                && error.field == "brands.publications.destination")
    );
    assert!(
        !serde_json::to_string(&forward)
            .expect("private diagnostics")
            .contains("PRIVATE")
    );
    for alternative in [
        "destination={kind='git',remote='PRIVATE remote',branch='other'}",
        "destination={kind='git',remote='private remote',branch='main'}",
        "destination={kind='git',remote='PRIVATE remote/',branch='main'}",
        "",
    ] {
        compile(&format!("{one}\n{}", brand("two", alternative)))
            .expect("different exact pair or local-only is not a duplicate");
    }
}

#[test]
fn publication_repository_supplies_main_without_changing_deployability() {
    let key = "site:one".parse().unwrap();
    let selected = compile(&brand(
        "one",
        "repository='https://example.invalid/ExactCase.git'",
    ))
    .expect("one repository declaration");
    assert_eq!(
        selected.workspace.destination(&key),
        Some(&PublicationDestination::Git {
            remote: "https://example.invalid/ExactCase.git".into(),
            branch: "main".into(),
        })
    );
    assert_eq!(
        selected.workspace.sites().collect::<Vec<_>>(),
        compile(&brand("one", ""))
            .unwrap()
            .workspace
            .sites()
            .collect::<Vec<_>>()
    );
    assert!(
        selected
            .workspace
            .destination(&"site:two".parse().unwrap())
            .is_none()
    );
    let overridden = compile(&brand("one", "repository='https://example.invalid/ExactCase.git'\ndestination={kind='git',remote='https://example.invalid/private.git',branch='published'}"))
        .expect("explicit delivery remains supported");
    assert_eq!(
        overridden.workspace.destination(&key),
        Some(&PublicationDestination::Git {
            remote: "https://example.invalid/private.git".into(),
            branch: "published".into(),
        })
    );
}

#[test]
fn repository_defaults_share_exact_destination_collision_checks() {
    let repository = "repository='https://example.invalid/shared.git'";
    let error = compile(&format!(
        "{}\n{}",
        brand("one", repository),
        brand("two", repository)
    ))
    .expect_err("two publications cannot share a delivery branch");
    let workspace::Error::Invalid(errors) = error else {
        panic!("semantic collision");
    };
    assert_eq!(
        errors
            .iter()
            .filter(|error| error.code == "workspace_destination_duplicate")
            .count(),
        2
    );
}
