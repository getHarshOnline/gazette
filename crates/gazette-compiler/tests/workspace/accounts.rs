//! Shared profiles retain separate account ownership and publication selections.

use super::*;

const ACCOUNTS: &str = r#"
[[accounts]]
id='account:github'
platform='github'
handle='@Example'
owner='owner:publisher'
[[accounts]]
id='account:x'
platform='x'
handle='shared'
[[accounts]]
id='account:youtube'
platform='youtube'
handle='@Channel'
channel_id='UC-example'
[[accounts]]
id='account:linkedin'
platform='linkedin'
url='https://www.linkedin.com/in/example/'
owner='owner:other'
"#;

const OTHER: &str = "[[principals]]\nid='owner:other'\nkind='organization'\n";
const PUBLICATION: &str = "[[brands]]\nid='brand:one'\nname='Example'\n[[brands.publications]]\nsite='site:one'\naccounts=['account:x','account:youtube','account:linkedin']\n";

#[test]
fn empty_overrides_keep_site_and_publisher_origins_in_single_and_modular_workspaces() {
    let flat =
        "[[sites]]\nid='site:one'\nname='Example'\npublisher='owner:publisher'\ndeployable=false\n";
    for (override_source, expected_scope) in [
        (
            format!("{flat}accounts=[]\n"),
            workspace::PublicationScope::Site("site:one".parse().unwrap()),
        ),
        (
            format!("[[publishers]]\nid='owner:publisher'\npublication={{accounts=[]}}\n{flat}"),
            workspace::PublicationScope::Publisher("owner:publisher".parse().unwrap()),
        ),
    ] {
        let defaults = format!("[defaults.publication]\naccounts=['account:x']\n{ACCOUNTS}");
        let owner_source = [PRINCIPAL, OTHER, &override_source].concat();
        let source = format!("{defaults}{owner_source}");
        let query = workspace::PublicationOriginQuery {
            site: "site:one".parse().unwrap(),
            path: vec![toml::PathSegment::Key("accounts".into())],
        };
        let parsed = workspace::ParsedWorkspace::parse(&source, Default::default()).unwrap();
        let origin = parsed.publication_origin(&query).unwrap();
        assert_eq!(origin.scope, expected_scope);
        assert_eq!(origin.value, serde_json::json!([]));
        assert_eq!(&source[origin.span.start..origin.span.end], "[]");
        let root: EntryPath = "gazette.toml".parse().unwrap();
        let included: EntryPath = "defaults.toml".parse().unwrap();
        let root_source = format!("includes=['defaults.toml']\n{owner_source}");
        let modular = workspace::ParsedWorkspaceDocuments::parse(
            &root,
            [
                (&root, root_source.as_bytes()),
                (&included, defaults.as_bytes()),
            ],
            Default::default(),
        )
        .unwrap();
        let origin = modular.publication_origin(&query).unwrap();
        assert_eq!(origin.document.as_ref(), Some(&root));
        assert_eq!(origin.scope, expected_scope);
        assert_eq!(origin.value, serde_json::json!([]));
        assert_eq!(&root_source[origin.span.start..origin.span.end], "[]");
    }
}

#[test]
fn unused_or_overridden_defaults_reject_every_invalid_account_selection() {
    let root: EntryPath = "gazette.toml".parse().unwrap();
    let included: EntryPath = "defaults.toml".parse().unwrap();
    let site = "[[sites]]\nid='site:one'\nname='Example'\npublisher='owner:publisher'\ndeployable=false\naccounts=[]\n";
    let base = format!(
        "{PRINCIPAL}{OTHER}{ACCOUNTS}[[accounts]]\nid='account:other-x'\nplatform='x'\nhandle='different'\n{site}"
    );
    for invalid in [
        "['account:missing']",
        "['account:x','account:x']",
        "['account:x','account:other-x']",
    ] {
        for declared in [
            format!("[defaults.publication]\naccounts={invalid}\n"),
            format!("[[publishers]]\nid='owner:publisher'\npublication={{accounts={invalid}}}\n"),
            format!("[[publishers]]\nid='owner:other'\npublication={{accounts={invalid}}}\n"),
        ] {
            assert!(compile(&format!("{declared}{base}")).is_err(), "{declared}");
            let root_source = format!("includes=['defaults.toml']\n{base}");
            assert!(
                workspace::compile_documents(
                    &root,
                    [
                        (&root, root_source.as_bytes()),
                        (&included, declared.as_bytes())
                    ],
                    Default::default()
                )
                .is_err(),
                "{declared}"
            );
        }
    }
    let conflict = "[[publishers]]\nid='owner:other'\npublication={accounts=['account:x'],social={x={handle='inline'}}}\n";
    assert!(compile(&format!("{conflict}{base}")).is_err());
}

#[test]
fn account_schema_and_editor_help_describe_the_same_closed_fields() {
    let schema = gazette_compiler::contracts::workspace();
    let validator = jsonschema::validator_for(schema.as_value()).unwrap();
    let valid = serde_json::json!({"accounts":[{"id":"account:one", "platform":"github", "handle":"example"}]});
    assert!(validator.is_valid(&valid));
    let mut unknown = valid.clone();
    unknown["accounts"][0]["administrator"] = "owner:one".into();
    assert!(!validator.is_valid(&unknown));
    let mut missing = valid;
    missing["accounts"][0]
        .as_object_mut()
        .unwrap()
        .remove("platform");
    assert!(!validator.is_valid(&missing));
    let source = "[[accounts]]\nid='account:one'\nplatform='github'\nown";
    let authoring = workspace::WorkspaceAuthoring::parse(source, Default::default()).unwrap();
    let help = authoring.help(source, source.len(), 1 << 18).unwrap();
    assert_eq!(
        help.candidates
            .iter()
            .map(|candidate| candidate.label.as_str())
            .collect::<Vec<_>>(),
        ["owner"]
    );
    assert!(
        help.candidates[0]
            .description
            .as_ref()
            .unwrap()
            .contains("ownership unresolved")
    );
}

#[test]
fn display_does_not_assign_ownership_and_unselected_owned_accounts_supply_identity() {
    let principal = PRINCIPAL.replace(
        "kind='person'",
        "kind='person'\nsame_as=['https://github.com/Example','https://identity.example/me']",
    );
    let compilation = compile(&[ACCOUNTS, &principal, OTHER, SITE, PUBLICATION].concat()).unwrap();
    let workspace = &compilation.workspace;
    assert_eq!(workspace.accounts().len(), 4);
    assert_eq!(
        workspace
            .principal(&"owner:publisher".parse().unwrap())
            .unwrap()
            .same_as,
        ["https://github.com/Example", "https://identity.example/me"]
    );
    assert_eq!(
        workspace
            .principal(&"owner:other".parse().unwrap())
            .unwrap()
            .same_as,
        ["https://www.linkedin.com/in/example/"]
    );
    let publication = workspace
        .publication_for(&"site:one".parse().unwrap())
        .unwrap();
    assert!(!publication.social.contains_key("github"));
    assert_eq!(publication.social["x"].url, "https://x.com/shared");
    assert_eq!(
        publication.social["youtube"].url,
        "https://www.youtube.com/@Channel"
    );
    assert_eq!(
        publication.social["youtube"].channel_id.as_deref(),
        Some("UC-example")
    );
    assert!(
        workspace
            .account(&"account:x".parse().unwrap())
            .unwrap()
            .owner
            .is_none()
    );
    let reordered = compile(&[OTHER, &principal, SITE, PUBLICATION, ACCOUNTS].concat()).unwrap();
    assert_eq!(workspace, &reordered.workspace);
}

#[test]
fn explicit_urls_win_and_custom_platforms_require_them() {
    let account = "[[accounts]]\nid='account:custom'\nplatform='custom'\nhandle='display'\nurl='https://profiles.example/person'\n";
    let github = "[[accounts]]\nid='account:github'\nplatform='github'\nhandle='example'\nurl='https://github.example/team'\n";
    let compilation = compile(&[PRINCIPAL, SITE, account, github].concat()).unwrap();
    assert_eq!(
        compilation
            .workspace
            .account(&"account:github".parse().unwrap())
            .unwrap()
            .profile
            .url,
        "https://github.example/team"
    );
    assert_eq!(
        compilation
            .workspace
            .account(&"account:custom".parse().unwrap())
            .unwrap()
            .profile
            .handle
            .as_deref(),
        Some("display")
    );
    for declaration in [
        account.replace("url='https://profiles.example/person'\n", ""),
        github.replace("url='https://github.example/team'", "url='javascript:bad'"),
        github.replace(
            "url='https://github.example/team'",
            "url='https://user:secret@example.test/'",
        ),
        github.replace("handle='example'", "handle='bad handle'"),
        github.replace("platform='github'", "platform='bad platform'"),
        github.replace("handle='example'", "channel_id='bad channel'"),
    ] {
        assert!(
            compile(&[PRINCIPAL, SITE, &declaration].concat()).is_err(),
            "{declaration}"
        );
    }
    for handle in ["", "@", "..", "a/b", "a?b", "a#b", "a%2fb", "@@example"] {
        let declaration =
            format!("[[accounts]]\nid='account:bad'\nplatform='github'\nhandle='{handle}'\n");
        assert!(
            compile(&[PRINCIPAL, SITE, &declaration].concat()).is_err(),
            "{handle}"
        );
    }
}

#[test]
fn rejects_missing_owners_accounts_duplicates_and_ambiguous_platforms() {
    let base = [PRINCIPAL, OTHER, SITE, ACCOUNTS].concat();
    for suffix in [
        PUBLICATION.replace("account:x", "account:missing"),
        PUBLICATION.replace("'account:x',", "'account:x','account:x',"),
        format!("{PUBLICATION}social={{x={{url='https://x.com/inline'}}}}\n"),
        "[[accounts]]\nid='account:github'\nplatform='github'\nhandle='other'\n".into(),
        "[[accounts]]\nid='account:duplicate'\nplatform='github'\nhandle='Example'\n".into(),
        "[[accounts]]\nid='account:bad-owner'\nplatform='x'\nhandle='other'\nowner='owner:missing'\n".into(),
        format!("{PUBLICATION}[[accounts]]\nid='account:second-x'\nplatform='x'\nhandle='second'\n").replace("'account:x',", "'account:x','account:second-x',"),
    ] {
        assert!(compile(&format!("{base}{suffix}")).is_err(), "{suffix}");
    }
}

#[test]
fn account_lists_replace_defaults_at_the_declared_publisher_boundary() {
    let defaults = "[defaults.publication]\naccounts=['account:github']\n[[publishers]]\nid='owner:publisher'\npublication={accounts=['account:x']}\n";
    let flat =
        "[[sites]]\nid='site:one'\nname='Example'\npublisher='owner:publisher'\ndeployable=false\n";
    for (selection, expected) in [
        ("", vec!["x"]),
        ("accounts=[]\n", vec![]),
        ("accounts=['account:youtube']\n", vec!["youtube"]),
    ] {
        let source = [defaults, PRINCIPAL, OTHER, ACCOUNTS, flat, selection].concat();
        let compiled = compile(&source).unwrap();
        let publication = compiled
            .workspace
            .publication_for(&"site:one".parse().unwrap())
            .unwrap();
        assert_eq!(
            publication
                .social
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            expected
        );
    }
    let other_site = flat.replace("owner:publisher", "owner:other");
    let compiled = compile(&[defaults, PRINCIPAL, OTHER, ACCOUNTS, &other_site].concat()).unwrap();
    assert!(
        compiled
            .workspace
            .publication_for(&"site:one".parse().unwrap())
            .unwrap()
            .social
            .contains_key("github")
    );
}

#[test]
fn modular_navigation_and_profile_origins_return_original_account_locations() {
    let root: EntryPath = "gazette.toml".parse().unwrap();
    let registry: EntryPath = "accounts.toml".parse().unwrap();
    let root_source = [
        "includes=['accounts.toml']\n",
        PRINCIPAL,
        OTHER,
        SITE,
        PUBLICATION,
    ]
    .concat();
    let parsed = workspace::ParsedWorkspaceDocuments::parse(
        &root,
        [
            (&root, root_source.as_bytes()),
            (&registry, ACCOUNTS.as_bytes()),
        ],
        Default::default(),
    )
    .unwrap();
    let offset = root_source.find("'account:x'").unwrap() + 1;
    let reference = parsed.navigation().at(&root, offset).unwrap();
    assert_eq!(reference.definition.document, &registry);
    assert_eq!(
        reference.symbol.symbol.declaration().unwrap().kind,
        workspace::DeclarationKind::Account
    );
    let offset = ACCOUNTS.find("owner='owner:publisher'").unwrap() + "owner='".len();
    assert_eq!(
        parsed
            .navigation()
            .at(&registry, offset)
            .unwrap()
            .definition
            .document,
        &root
    );
    for (platform, field, literal, value) in [
        ("x", "url", "'shared'", "https://x.com/shared"),
        ("youtube", "channel_id", "'UC-example'", "UC-example"),
        (
            "linkedin",
            "url",
            "'https://www.linkedin.com/in/example/'",
            "https://www.linkedin.com/in/example/",
        ),
    ] {
        let origin = parsed
            .publication_origin(&workspace::PublicationOriginQuery {
                site: "site:one".parse().unwrap(),
                path: ["social", platform, field]
                    .into_iter()
                    .map(|key| toml::PathSegment::Key(key.into()))
                    .collect(),
            })
            .unwrap();
        assert_eq!(origin.document.as_ref(), Some(&registry));
        assert_eq!(
            origin.scope,
            workspace::PublicationScope::Account(format!("account:{platform}").parse().unwrap())
        );
        assert_eq!(&ACCOUNTS[origin.span.start..origin.span.end], literal);
        assert_eq!(origin.value, value);
    }
}

#[test]
fn inherited_selections_keep_their_authored_reference_and_origin() {
    for selection in [
        "[defaults.publication]\naccounts=['account:x']\n",
        "[[publishers]]\nid='owner:publisher'\npublication={accounts=['account:x']}\n",
    ] {
        let flat = "[[sites]]\nid='site:one'\nname='Example'\npublisher='owner:publisher'\ndeployable=false\n";
        let source = [selection, PRINCIPAL, OTHER, ACCOUNTS, flat].concat();
        let parsed = workspace::ParsedWorkspace::parse(&source, Default::default()).unwrap();
        let offset = source.find("'account:x'").unwrap() + 1;
        assert_eq!(
            parsed.navigation().at(offset).unwrap().symbol.name(),
            "account:x"
        );
        let origin = parsed
            .publication_origin(&workspace::PublicationOriginQuery {
                site: "site:one".parse().unwrap(),
                path: vec![toml::PathSegment::Key("accounts".into())],
            })
            .unwrap();
        assert_eq!(&source[origin.span.start..origin.span.end], "['account:x']");
    }
}
