//! Effect-free documentation checking projects the application-owned resolved plan.

#![cfg(feature = "native-local")]

use gazette_sdk::{build, toml, workspace::Key};

const CONFIGURATION: &str = "[[principals]]\nid='owner:one'\nkind='person'\n\
[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n\
[[projects]]\nid='project:gazette'\nlifecycle='active'\nvisibility='public'\navailability='available'\n\
[[sources]]\nid='source:gazette'\nlocation={kind='repository',root='/not-opened/gazette',commit='1111111111111111111111111111111111111111',subdirectory='project'}\n\
[[brands]]\nid='brand:gazette'\nname='Gazette'\n\
[[brands.publications]]\nsite='site:one'\n\
[brands.publications.documentation_repository]\nname='gazette-domain-doc'\npath='/not-opened/gazette-domain-doc'\n\
[brands.publications.documentation_repository.build]\ntarget={kind='mintlify',theme='mint',primary_color='#123abc'}\n\
[[brands.publications.documentation_repository.build.modules]]\nproject='project:gazette'\nslug='gazette'\ntitle='Gazette manual'\nmanual={source='source:gazette'}\n\
navigation=[{kind='manual',title='Start',pages=[{title='Overview',route='gazette/overview',input={kind='file',path='docs/overview.md'}}]}]\n";

#[test]
fn sdk_check_retains_brand_destination_and_distinct_manual_navigation() {
    let report = build::check_documentation(
        CONFIGURATION,
        &"site:one".parse::<Key>().expect("site key"),
        toml::Limits::default(),
    )
    .expect("effect-free target preflight");
    assert_eq!(report.repository_name, "gazette-domain-doc");
    assert_eq!(report.repository_path, "/not-opened/gazette-domain-doc");
    assert!(matches!(
        report.build.modules[0].navigation[0],
        gazette_sdk::workspace::DocumentationNavigation::Manual { .. }
    ));
}

#[test]
fn sdk_check_rejects_target_incompatibility_without_opening_declared_paths() {
    let source = CONFIGURATION.replace("theme='mint'", "theme='unknown'");
    let error = build::check_documentation(
        &source,
        &"site:one".parse::<Key>().expect("site key"),
        toml::Limits::default(),
    )
    .expect_err("unsupported target");
    assert_eq!(error.code(), "documentation_target_input");
}
