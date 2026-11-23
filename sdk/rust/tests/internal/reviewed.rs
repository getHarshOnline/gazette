//! Credential routing uses independent workspace ownership, never an approval's chosen helper.

use super::*;
use std::{fs, os::unix::fs::PermissionsExt};

fn selection() -> ReviewedSelection {
    ReviewedSelection {
        workspace: WorkspaceInput::Supplied { source: "\
[[principals]]\nid='owner:one'\nkind='person'\n\
[[principals]]\nid='owner:two'\nkind='person'\n\
[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=true\n\
[[sites]]\nid='site:two'\npublisher='owner:two'\ndeployable=true\n\
[[brands]]\nid='brand:fixture'\nname='Fixture'\n\
[[brands.publications]]\nsite='site:one'\npublic_base_url='https://one.example/'\nrepository='https://example.invalid/one/site.git'\n\
[[brands.publications]]\nsite='site:two'\npublic_base_url='https://two.example/'\nrepository='https://example.invalid/two/site.git'\n".into() },
        sites: Some(vec!["site:one".parse().unwrap(), "site:two".parse().unwrap()]),
        credentials: ["one", "two"].map(|name| ReviewedCredential {
            publisher: format!("owner:{name}").parse().unwrap(), owner: name.into(),
            credential: GitCredentialSelection { helper: format!("/selected/{name}").into(), allow_credentials: true },
        }).to_vec(), allow_read: false, limits: Default::default(),
    }
}

#[test]
fn reviewed_scope_requires_exact_distinct_publishers_and_selected_eligible_sites() {
    let selected = selection();
    let scope = selected.resolve().unwrap();
    assert_eq!(scope.publishers.len(), 2);
    assert_eq!(scope.publishers[0].authorized_target_ordinals, [0]);
    assert_eq!(scope.publishers[1].authorized_target_ordinals, [1]);
    for case in 0..6 {
        let mut changed = selected.clone();
        match case {
            0 => {
                changed.credentials.pop();
            }
            1 => changed.credentials.push(changed.credentials[0].clone()),
            2 => changed.credentials[0].owner = " ".into(),
            3 => changed.credentials[0].credential.helper = "/helper --ambient".into(),
            4 => changed.sites = Some(vec!["site:one".parse().unwrap()]),
            _ => changed
                .sites
                .as_mut()
                .unwrap()
                .push("site:one".parse().unwrap()),
        }
        assert!(changed.resolve().is_err(), "case {case}");
    }
}

#[test]
fn reviewed_transport_routes_helpers_per_publisher_without_ambient_or_cross_target_fallback() {
    let scope = selection().resolve().unwrap();
    let control = tempfile::tempdir().unwrap();
    let isolation = tempfile::tempdir().unwrap();
    fs::set_permissions(isolation.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let calls = control.path().join("arguments");
    let program = control.path().join("selected-git");
    let quoted_calls = calls.to_str().unwrap().replace('\'', "'\"'\"'");
    fs::write(&program, format!("#!/bin/sh\nprintf '%s\\n' \"$@\" >> '{quoted_calls}'\nprintf '{}\\trefs/heads/main\\n'\n", "a1".repeat(20))).unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o700)).unwrap();
    let git = GitSelection {
        program,
        isolation: isolation.path().into(),
        credentials: None,
    };
    let options = RemoteReadOptions {
        allow_read: true,
        allow_network: true,
        ..Default::default()
    };
    for name in ["one", "two"] {
        scope
            .transport(&git)
            .observe_remote(
                &format!("https://example.invalid/{name}/site.git"),
                "main",
                options,
            )
            .unwrap();
        let arguments = fs::read_to_string(&calls).unwrap();
        assert!(arguments.contains(&format!("credential.helper=/selected/{name}\n")));
        let other = if name == "one" { "two" } else { "one" };
        assert!(!arguments.contains(&format!("credential.helper=/selected/{other}\n")));
        fs::write(&calls, b"").unwrap();
    }
    assert!(
        scope
            .transport(&git)
            .observe_remote("https://example.invalid/unknown/site.git", "main", options)
            .is_err()
    );
    assert!(fs::read(&calls).unwrap().is_empty());
    let ambiguous = GitSelection {
        credentials: Some(GitCredentialSelection {
            helper: "/ambient".into(),
            allow_credentials: true,
        }),
        ..git
    };
    assert!(
        scope
            .transport(&ambiguous)
            .observe_remote("https://example.invalid/one/site.git", "main", options)
            .is_err()
    );
    assert!(fs::read(&calls).unwrap().is_empty());
}
