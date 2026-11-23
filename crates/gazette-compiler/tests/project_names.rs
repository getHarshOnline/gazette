//! Project presentation facts are explicit values, never identity joins or remote defaults.

use gazette_compiler::{toml, workspace};

const CONFIG: &str = "[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n[[projects]]\nid='project:one'\nlifecycle='active'\nvisibility='public'\navailability='gated'\n";

#[test]
fn explicit_project_name_is_exact_optional_and_source_addressable() {
    let key = "project:one".parse().expect("key");
    let baseline = workspace::compile(CONFIG, Default::default()).expect("unnamed project");
    assert!(
        baseline
            .workspace
            .project(&key)
            .expect("project")
            .name
            .is_none()
    );
    let name = "  Shared Ω <&> label  ";
    let input = format!("{CONFIG}name={name:?}\n");
    let parsed =
        workspace::ParsedWorkspace::parse(&input, Default::default()).expect("named project");
    let project = parsed
        .compilation()
        .workspace
        .project(&key)
        .expect("same key");
    assert_eq!(project.name.as_deref(), Some(name));
    assert_eq!(project.id, key);
    let query = workspace::FieldQuery {
        kind: workspace::DeclarationKind::Project,
        identity: key,
        field: vec![toml::PathSegment::Key("name".into())],
    };
    let origin = parsed.origin(&query).expect("native field origin");
    assert_eq!(
        &input[origin.span.start..origin.span.end],
        format!("{name:?}")
    );
}

#[test]
fn names_do_not_merge_projects_and_invalid_names_fail_without_trimming() {
    let config = format!(
        "{CONFIG}name='Shared'\n[[projects]]\nid='project:two'\nname='Shared'\nlifecycle='done'\nvisibility='private'\navailability='unavailable'\n"
    );
    let compiled = workspace::compile(&config, Default::default()).expect("distinct keys");
    assert_eq!(compiled.workspace.projects().count(), 2);
    for name in ["", " ", "\t", "line\nfeed", "bad\u{7f}"] {
        let input = format!(
            "{CONFIG}name={}\n",
            serde_json::to_string(name)
                .expect("TOML string")
                .replace('\u{7f}', "\\u007F")
        );
        let workspace::Error::Invalid(errors) =
            workspace::compile(&input, Default::default()).expect_err("invalid name")
        else {
            panic!("semantic refusal")
        };
        assert!(
            errors
                .iter()
                .any(|error| error.code == "workspace_project_name")
        );
    }
}

#[test]
fn repository_is_exact_optional_and_bound_to_its_own_configuration_origin() {
    let key = "project:one".parse().expect("key");
    let baseline = workspace::compile(CONFIG, Default::default()).expect("no repository");
    let project = baseline.workspace.project(&key).expect("project");
    assert!(project.repository.is_none());
    assert!(
        serde_json::to_value(project)
            .expect("catalog")
            .get("repository")
            .is_none()
    );

    for repository in [
        "https://code.example/Team/Repo.git",
        "HTTPS://Code.example/Team/Repo?view=src&lang=rust#API",
        "http://code.example:8080/团队/Repo",
    ] {
        let input = format!("{CONFIG}repository={repository:?}\n");
        let parsed = workspace::ParsedWorkspace::parse(&input, Default::default())
            .expect("explicit repository");
        let project = parsed
            .compilation()
            .workspace
            .project(&key)
            .expect("same project");
        assert_eq!(project.repository.as_deref(), Some(repository));
        assert_eq!(project.id, key);
        assert!(
            project.name.is_none(),
            "repository never supplies the display name"
        );
        let origin = parsed
            .origin(&workspace::FieldQuery {
                kind: workspace::DeclarationKind::Project,
                identity: key.clone(),
                field: vec![toml::PathSegment::Key("repository".into())],
            })
            .expect("repository source origin");
        assert_eq!(
            &input[origin.span.start..origin.span.end],
            format!("{repository:?}")
        );
    }
}

#[test]
fn repository_refuses_credentials_local_paths_and_parser_repairs_without_network_access() {
    for repository in [
        "",
        "/local/repository",
        "../repo",
        "code.example/team/repo",
        "git@code.example:team/repo",
        "ssh://code.example/team/repo",
        "file:///local/repo",
        "javascript:alert(1)",
        "https:code.example/repo",
        "https:///code.example/repo",
        "https://",
        "https://@code.example/repo",
        "https://user@code.example/repo",
        "https://user:secret@code.example/repo",
        "https://:secret@code.example/repo",
        " https://code.example/repo",
        "https://code.example/repo ",
        "https://code.example/repo\n",
        "https://code.example/repo\u{a0}",
        "https://code.example\\repo",
        "https://code.example/repo\u{0}",
    ] {
        let input = format!(
            "{CONFIG}repository={}\n",
            serde_json::to_string(repository).expect("TOML string")
        );
        let workspace::Error::Invalid(errors) = workspace::compile(&input, Default::default())
            .expect_err("invalid publication repository")
        else {
            panic!("semantic repository refusal for {repository:?}");
        };
        assert!(
            errors
                .iter()
                .any(|error| error.code == "workspace_project_repository"
                    && error.field == "projects.repository"
                    && error
                        .key
                        .as_ref()
                        .is_some_and(|key| key.as_str() == "project:one")),
            "exact repository diagnostic for {repository:?}"
        );
    }
}
