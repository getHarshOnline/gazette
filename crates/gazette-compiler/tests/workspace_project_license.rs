//! Workspace parsing retains exact project SPDX expressions and rejects lax substitutes.

use gazette_compiler::{toml, workspace};

fn workspace(license: Option<&str>) -> String {
    format!(
        "[[principals]]\nid='owner:publisher'\nkind='person'\n\
         [[sites]]\nid='site:one'\npublisher='owner:publisher'\ndeployable=false\n\
         [[projects]]\nid='project:one'\n{}lifecycle='active'\nvisibility='public'\navailability='available'\n",
        license.map_or_else(String::new, |value| format!("license={value:?}\n"))
    )
}

#[test]
fn exact_expression_and_absence_share_the_normal_workspace_path() {
    for expected in [None, Some("MIT OR Apache-2.0")] {
        let compiled = workspace::compile(&workspace(expected), toml::Limits::default())
            .expect("complete workspace");
        assert_eq!(
            compiled
                .workspace
                .projects()
                .next()
                .expect("project")
                .license
                .as_ref()
                .map(|value| value.as_str()),
            expected
        );
    }
}

#[test]
fn lax_unknown_and_non_expression_values_fail_at_the_toml_owner_boundary() {
    for value in [
        "mit",
        "MIT/Apache-2.0",
        "Unknown-License",
        "https://spdx.org/licenses/MIT.html",
    ] {
        assert!(
            matches!(
                workspace::compile(&workspace(Some(value)), toml::Limits::default()),
                Err(workspace::Error::Toml(toml::Error::Invalid {
                    span: Some(_),
                    ..
                }))
            ),
            "accepted non-strict project licence {value:?}"
        );
    }
}
