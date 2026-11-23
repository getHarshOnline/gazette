//! Real TOML-to-topology behavior; no database seeding or semantic mock.

#[path = "workspace/social_card.rs"]
mod social_card;

use gazette_compiler::{
    toml,
    workspace::{
        self, Availability, Error, ProjectLifecycle, ProjectPlacementRole, SourceLocation,
        SourceRoot, Visibility,
    },
};
use gazette_core::source::EntryPath;

const PRINCIPAL: &str = "[[principals]]\nid='owner:publisher'\nkind='person'\n";
const SITE: &str = "[[sites]]\nid='site:one'\npublisher='owner:publisher'\ndeployable=false\n";
const PROJECT: &str =
    "[[projects]]\nid='project:one'\nlifecycle='done'\nvisibility='public'\navailability='gated'\n";
const PLACEMENT: &str = "[[placements]]\nrole='primary'\nsite='site:one'\nproject='project:one'\nrender=true\ndiscoverable=false\ndocuments={source='source:one',target='docs'}\n";
const SOURCE: &str = "[[sources]]\nid='source:one'\n[sources.location]\nkind='repository'\nroot='/example/repository'\ncommit='0000000000000000000000000000000000000000'\nsubdirectory='docs'\n";

fn compile(source: &str) -> Result<workspace::Compilation, Error> {
    workspace::compile(source, toml::Limits::default())
}

#[path = "workspace/publication_defaults.rs"]
mod publication_defaults;

#[path = "workspace/accounts.rs"]
mod accounts;

fn diagnostics(source: &str) -> Vec<workspace::Diagnostic> {
    match compile(source).expect_err("invalid whole workspace") {
        Error::Invalid(errors) => errors,
        error => panic!("expected semantic diagnostics: {error}"),
    }
}

#[test]
fn project_states_use_one_vocabulary_and_keep_availability_separate() {
    for state in ["planned", "active", "paused", "done", "closed"] {
        let project = PROJECT.replace("lifecycle='done'", &format!("lifecycle='{state}'"));
        let compiled = compile(&[PRINCIPAL, SITE, &project].concat()).expect("project state");
        let project = compiled.workspace.projects().next().unwrap();
        assert_eq!(serde_json::to_value(project.lifecycle).unwrap(), state);
        assert_eq!(project.availability, Availability::Gated);
    }
    for state in [
        "developing",
        "dormant",
        "historical",
        "retired",
        "draft",
        "published",
    ] {
        let project = PROJECT.replace("lifecycle='done'", &format!("lifecycle='{state}'"));
        assert!(
            compile(&[PRINCIPAL, SITE, &project].concat()).is_err(),
            "{state}"
        );
    }
}

#[test]
fn declarations_resolve_independently_of_order_without_collapsing_state_or_controls() {
    let first =
        compile(&[PRINCIPAL, SITE, PROJECT, PLACEMENT, SOURCE].concat()).expect("workspace");
    let reordered =
        compile(&[SOURCE, PLACEMENT, PROJECT, SITE, PRINCIPAL].concat()).expect("reordered");
    assert_eq!(first.workspace, reordered.workspace);
    assert_ne!(
        first.source_sha256, reordered.source_sha256,
        "source identity remains exact"
    );
    let project = first.workspace.projects().next().expect("project");
    assert_eq!(project.lifecycle, ProjectLifecycle::Done);
    assert_eq!(project.visibility, Visibility::Public);
    assert_eq!(project.availability, Availability::Gated);
    let placement = &first.workspace.placements()[0];
    assert_eq!(placement.role, ProjectPlacementRole::Primary);
    assert!(placement.render);
    assert!(!placement.discoverable);
    assert!(!first.workspace.sites().next().expect("site").deployable);
    assert_eq!(
        first
            .workspace
            .sites()
            .next()
            .expect("site")
            .publisher
            .as_str(),
        "owner:publisher"
    );
}

#[test]
fn every_placed_project_has_one_explicit_primary_and_zero_or_more_mirrors() {
    let other_site = SITE.replace("site:one", "site:two");
    let primary = "[[placements]]\nrole='primary'\nsite='site:one'\nproject='project:one'\nrender=false\ndiscoverable=false\n";
    let mirror = "[[placements]]\nrole='mirror'\nsite='site:two'\nproject='project:one'\nrender=false\ndiscoverable=false\n";
    let compiled = compile(&[PRINCIPAL, SITE, &other_site, PROJECT, primary, mirror].concat())
        .expect("one canonical project placement");
    let roles = compiled
        .workspace
        .placements()
        .iter()
        .map(|placement| (placement.site.as_str(), placement.role))
        .collect::<Vec<_>>();
    assert_eq!(
        roles,
        [
            ("site:one", ProjectPlacementRole::Primary),
            ("site:two", ProjectPlacementRole::Mirror),
        ]
    );

    for placements in [
        mirror.to_owned(),
        [primary, &mirror.replace("role='mirror'", "role='primary'")].concat(),
    ] {
        let errors = diagnostics(&[PRINCIPAL, SITE, &other_site, PROJECT, &placements].concat());
        assert_eq!(
            errors
                .iter()
                .filter(|error| error.code == "workspace_placement_primary")
                .count(),
            1,
            "cardinality failure is one project-owned diagnostic: {errors:?}"
        );
        assert!(errors.iter().any(|error| {
            error.code == "workspace_placement_primary"
                && error.field == "placements.role"
                && error
                    .key
                    .as_ref()
                    .is_some_and(|key| key.as_str() == "project:one")
        }));
    }

    let missing_role = primary.replace("role='primary'\n", "");
    assert!(matches!(
        compile(&[PRINCIPAL, SITE, PROJECT, &missing_role].concat()),
        Err(Error::Toml(toml::Error::Invalid { span: Some(_), .. }))
    ));
    let invalid_role = primary.replace("role='primary'", "role='canonical'");
    assert!(matches!(
        compile(&[PRINCIPAL, SITE, PROJECT, &invalid_role].concat()),
        Err(Error::Toml(toml::Error::Invalid { span: Some(_), .. }))
    ));
}

#[test]
fn single_and_multiple_sites_share_one_complete_model_including_removals() {
    let one = compile(&[PRINCIPAL, SITE].concat()).expect("one site");
    assert_eq!(one.workspace.sites().len(), 1);
    assert_eq!(one.workspace.project_declarations().len(), 0);
    let other = SITE.replace("site:one", "site:two");
    let two = compile(&[PRINCIPAL, &other, SITE, PROJECT].concat()).expect("two sites");
    assert_eq!(
        two.workspace
            .sites()
            .map(|site| site.id.as_str())
            .collect::<Vec<_>>(),
        ["site:one", "site:two"]
    );
    assert_eq!(
        two.workspace.project_declarations().len(),
        1,
        "unplaced project is retained"
    );
    let removed = compile(&[PRINCIPAL, SITE].concat()).expect("complete replacement selection");
    assert_eq!(
        removed.workspace, one.workspace,
        "no prior registry or omitted source is retained"
    );
}

#[test]
fn all_independent_reference_failures_accumulate_without_fallback_or_partial_result() {
    let source = [
        "[[sites]]\nid='site:a'\npublisher='owner:missing'\ndeployable=false\n",
        "[[sites]]\nid='site:b'\npublisher='site:a'\ndeployable=false\n",
        "[[placements]]\nrole='primary'\nsite='site:missing'\nproject='project:missing'\nrender=false\ndiscoverable=true\n",
    ];
    let errors = diagnostics(&source.concat());
    assert_eq!(errors.len(), 4);
    assert!(
        errors
            .iter()
            .all(|error| error.code == "workspace_reference")
    );
    assert_eq!(
        errors,
        diagnostics(&[source[2], source[1], source[0]].concat())
    );
    assert!(
        errors.iter().any(|error| error
            .key
            .as_ref()
            .is_some_and(|key| key.as_str() == "site:a")),
        "site identity is not a publisher"
    );
}

#[test]
fn duplicate_conflicting_and_empty_declarations_fail_closed() {
    for source in [
        [PRINCIPAL, PRINCIPAL, SITE].concat(),
        [PRINCIPAL, SITE, SITE].concat(),
        [PRINCIPAL, SITE, PROJECT, PROJECT].concat(),
        [PRINCIPAL, SITE, PROJECT, SOURCE, PLACEMENT, PLACEMENT].concat(),
    ] {
        assert!(
            diagnostics(&source)
                .iter()
                .any(|error| error.code == "workspace_duplicate")
        );
    }
    assert!(
        diagnostics("sites=[]")
            .iter()
            .any(|error| error.code == "workspace_empty")
    );
    let conflict = [
        PRINCIPAL,
        SITE,
        &PROJECT.replace("project:one", "owner:publisher"),
    ]
    .concat();
    assert!(
        diagnostics(&conflict)
            .iter()
            .any(|error| error.code == "workspace_identity_conflict")
    );
    let wrong = SITE.replace("owner:publisher", "owner:missing");
    assert_eq!(
        diagnostics(&[PRINCIPAL, SITE, &wrong].concat()),
        diagnostics(&[PRINCIPAL, &wrong, SITE].concat())
    );
}

#[test]
fn schema_rejects_unknown_fields_values_missing_roles_and_unscoped_keys() {
    for source in [
        ["future=true\n", PRINCIPAL, SITE].concat(),
        [PRINCIPAL, SITE, "controller='owner:publisher'\n"].concat(),
        [PRINCIPAL, "[[sites]]\nid='site:one'\n"].concat(),
        [PRINCIPAL, SITE, &PROJECT.replace("done", "published")].concat(),
        [
            PRINCIPAL,
            SITE,
            &PROJECT.replace("gated", "available_through"),
        ]
        .concat(),
        [
            PRINCIPAL,
            SITE,
            &PROJECT.replace("visibility='public'\n", ""),
        ]
        .concat(),
        [PRINCIPAL, &SITE.replace("deployable=false\n", "")].concat(),
        [PRINCIPAL, SITE, PROJECT, PLACEMENT, "deployable=true\n"].concat(),
        [PRINCIPAL, &SITE.replace("site:one", "example.org")].concat(),
        [PRINCIPAL, &SITE.replace("site:one", "site: ")].concat(),
    ] {
        match compile(&source).expect_err("unsupported or malformed schema") {
            Error::Toml(toml::Error::Invalid { span, .. }) => assert!(span.is_some()),
            error => panic!("expected source-local schema failure: {error}"),
        }
    }
}

#[test]
fn unpublished_contact_and_materialization_policy_fields_fail_closed() {
    let contact = PRINCIPAL.replace(
        "kind='person'\n",
        "kind='person'\ncontact_email='private@example.invalid'\n",
    );
    let obligation = PLACEMENT.replace(
        "documents={source='source:one',target='docs'}",
        "documents={source='source:one',target='docs',obligation='optional'}",
    );
    for source in [
        [contact.as_str(), SITE].concat(),
        [PRINCIPAL, SITE, PROJECT, obligation.as_str(), SOURCE].concat(),
    ] {
        assert!(
            matches!(
                compile(&source),
                Err(Error::Toml(toml::Error::Invalid { span: Some(_), .. }))
            ),
            "unpublished owner fields must not be silently accepted or flattened"
        );
    }
}

#[test]
fn retained_syntax_and_direct_compilation_share_one_resolution_path_and_limits() {
    let source = [PRINCIPAL, SITE].concat();
    let parsed = toml::ParsedToml::parse(&source, toml::Limits::default()).expect("syntax");
    assert_eq!(
        workspace::resolve(&parsed).expect("retained"),
        compile(&source).expect("direct")
    );
    for limits in [
        toml::Limits {
            source_bytes: source.len() - 1,
            ..Default::default()
        },
        toml::Limits {
            nodes: 2,
            ..Default::default()
        },
        toml::Limits {
            depth: 0,
            ..Default::default()
        },
    ] {
        assert!(matches!(
            workspace::compile(&source, limits),
            Err(Error::Toml(toml::Error::Limit(_)))
        ));
    }
}

#[test]
fn brand_publication_is_the_single_documentation_repository_binding() {
    let brand = "[[brands]]\nid='brand:one'\nname='Public brand'\n[[brands.publications]]\nsite='site:one'\ndocumentation_repository={name='custom-reference-repo',path='/configured/generated/reference',destination={kind='git',remote='https://example.invalid/reference.git',branch='published'}}\n";
    let compiled = compile(&[PRINCIPAL, SITE, brand].concat()).expect("brand settings");
    let reordered = compile(&[brand, SITE, PRINCIPAL].concat()).expect("reordered graph");
    assert_eq!(compiled.workspace, reordered.workspace);
    assert_eq!(
        compiled.workspace.brands().next().expect("brand").name,
        "Public brand"
    );
    let repository = compiled
        .workspace
        .documentation_repository(&"site:one".parse().expect("site"))
        .expect("configured target");
    assert_eq!(repository.name, "custom-reference-repo");
    assert_eq!(repository.path, "/configured/generated/reference");
    assert!(matches!(
        repository.destination.as_ref(),
        Some(gazette_core::release::PublicationDestination::Git { remote, branch })
            if remote == "https://example.invalid/reference.git" && branch == "published"
    ));
    assert!(
        compiled
            .workspace
            .documentation_repository(&"site:other".parse().expect("site"))
            .is_none()
    );
    let without = compile(&[PRINCIPAL, SITE].concat()).expect("unbranded generic site");
    assert!(
        without
            .workspace
            .documentation_repository(&"site:one".parse().expect("site"))
            .is_none(),
        "no domain_doc naming fallback"
    );
    assert_eq!(
        compiled
            .workspace
            .sites()
            .next()
            .expect("site")
            .publisher
            .as_str(),
        "owner:publisher",
        "brand is not publisher"
    );
    for (from, to, code) in [
        (
            "site='site:one'",
            "site='site:missing'",
            "workspace_reference",
        ),
        (
            "custom-reference-repo",
            "../repo",
            "workspace_repository_name",
        ),
        (
            "/configured/generated/reference",
            "../output",
            "workspace_repository_path",
        ),
        (
            "branch='published'",
            "branch='../published'",
            "workspace_destination",
        ),
        ("Public brand", "", "workspace_brand_name"),
        (
            "brand:one",
            "owner:publisher",
            "workspace_identity_conflict",
        ),
    ] {
        assert!(
            diagnostics(&[PRINCIPAL, SITE, &brand.replace(from, to)].concat())
                .iter()
                .any(|error| error.code == code),
            "{code}"
        );
    }
    let other = brand.replace("brand:one", "brand:two");
    let first = diagnostics(&[PRINCIPAL, SITE, brand, &other].concat());
    assert_eq!(
        first,
        diagnostics(&[PRINCIPAL, SITE, &other, brand].concat())
    );
    assert!(
        first
            .iter()
            .any(|error| error.code == "workspace_duplicate")
    );
}

#[test]
fn authored_and_generated_repositories_require_distinct_git_destinations() {
    let brand = "[[brands]]\nid='brand:one'\nname='Public brand'\n[[brands.publications]]\nsite='site:one'\ndestination={kind='git',remote='https://example.invalid/shared.git',branch='published'}\ndocumentation_repository={name='reference',path='/generated/reference',destination={kind='git',remote='https://example.invalid/shared.git',branch='published'}}\n";
    let errors = diagnostics(&[PRINCIPAL, SITE, brand].concat());
    let duplicate: Vec<_> = errors
        .iter()
        .filter(|error| error.code == "workspace_destination_duplicate")
        .collect();
    assert_eq!(
        duplicate.len(),
        2,
        "both conflicting owners are identified: {errors:?}"
    );
    assert!(
        duplicate
            .iter()
            .any(|error| error.field == "brands.publications.destination")
    );
    assert!(duplicate.iter().any(|error| {
        error.field == "brands.publications.documentation_repository.destination"
    }));

    let independent = brand.replace(
        "https://example.invalid/shared.git',branch='published'}}",
        "https://example.invalid/reference.git',branch='published'}}",
    );
    compile(&[PRINCIPAL, SITE, &independent].concat())
        .expect("authored and generated repositories have distinct selected targets");
}

#[test]
fn one_brand_can_bind_multiple_sites_without_sharing_their_generated_repositories() {
    let other = SITE.replace("site:one", "site:two");
    let brand = "[[brands]]\nid='brand:one'\nname='Brand'\n";
    let one = "[[brands.publications]]\nsite='site:one'\ndocumentation_repository={name='first',path='/generated/one'}\n";
    let two = "[[brands.publications]]\nsite='site:two'\ndocumentation_repository={name='second',path='/generated/two'}\n";
    let compiled =
        compile(&[PRINCIPAL, SITE, &other, brand, two, one].concat()).expect("two publications");
    let ordered = compile(&[PRINCIPAL, SITE, &other, brand, one, two].concat()).expect("ordered");
    assert_eq!(compiled.workspace, ordered.workspace);
    assert_eq!(
        compiled
            .workspace
            .documentation_repository(&"site:two".parse().expect("site"))
            .expect("target")
            .name,
        "second"
    );
    let duplicate_path = two.replace("/generated/two", "/generated/one");
    assert!(
        diagnostics(&[PRINCIPAL, SITE, &other, brand, one, &duplicate_path].concat())
            .iter()
            .any(|error| error.code == "workspace_duplicate")
    );
}

#[test]
fn source_bindings_require_exact_coordinates_and_cannot_be_implied_by_rendering() {
    let valid = [PRINCIPAL, SITE, PROJECT, PLACEMENT, SOURCE].concat();
    let resolved = compile(&valid).expect("explicit source");
    assert_eq!(resolved.workspace.sources().len(), 1);
    assert_eq!(
        resolved.workspace.placements()[0]
            .documents
            .as_ref()
            .expect("binding")
            .target
            .as_str(),
        "docs"
    );
    for (from, to) in [
        ("0000000000000000000000000000000000000000", "main"),
        ("subdirectory='docs'", "subdirectory='../docs'"),
        ("target='docs'", "target='docs/%2e%2e'"),
        ("kind='repository'", "kind='mirror'"),
    ] {
        assert!(matches!(
            compile(&valid.replace(from, to)),
            Err(Error::Toml(_))
        ));
    }
    for root in ["../repo", "/repo/../other", "/repo//child", "/"] {
        assert!(matches!(
            compile(&valid.replace("/example/repository", root)),
            Err(Error::Toml(_))
        ));
    }
    let no_binding = valid.replace("documents={source='source:one',target='docs'}\n", "");
    assert!(
        diagnostics(&no_binding)
            .iter()
            .any(|error| error.code == "workspace_source_required")
    );
    assert!(compile(&no_binding.replace("render=true", "render=false")).is_ok());
    let missing = valid.replace("source='source:one'", "source='source:missing'");
    assert!(
        diagnostics(&missing)
            .iter()
            .any(|error| error.code == "workspace_reference")
    );
    let portable = compile(&valid.replace("/example/repository", "relative/repo"))
        .expect("workspace-relative source root");
    assert!(matches!(
        &portable.workspace.sources().next().expect("source").location,
        SourceLocation::Repository { root: SourceRoot::WorkspaceRelative(path), .. }
            if path.to_string() == "relative/repo"
    ));
    let workspace_root =
        compile(&valid.replace("/example/repository", ".")).expect("workspace root source");
    assert!(matches!(
        &workspace_root
            .workspace
            .sources()
            .next()
            .expect("source")
            .location,
        SourceLocation::Repository {
            root: SourceRoot::Workspace,
            ..
        }
    ));
    let bad = SOURCE.replace("/example/repository", "../repo");
    assert!(matches!(
        compile(&[PRINCIPAL, SITE, SOURCE, &bad].concat()),
        Err(Error::Toml(_))
    ));
    assert!(matches!(
        compile(&[PRINCIPAL, SITE, &bad, SOURCE].concat()),
        Err(Error::Toml(_))
    ));
}

#[test]
fn filesystem_sources_select_one_confined_tree_without_inventing_revision_lineage() {
    let whole = SOURCE
        .replace("kind='repository'", "kind='filesystem'")
        .replace("commit='0000000000000000000000000000000000000000'\n", "")
        .replace("subdirectory='docs'\n", "");
    let compiled =
        compile(&[PRINCIPAL, SITE, PROJECT, PLACEMENT, &whole].concat()).expect("whole local tree");
    assert!(matches!(
        &compiled.workspace.sources().next().expect("source").location,
        SourceLocation::Filesystem { root, subdirectory: None }
            if root.external() == Some("/example/repository")
    ));

    let selected = format!("{whole}subdirectory='authored'\n");
    let compiled = compile(&[PRINCIPAL, SITE, PROJECT, PLACEMENT, &selected].concat())
        .expect("confined local subtree");
    assert!(matches!(
        &compiled.workspace.sources().next().expect("source").location,
        SourceLocation::Filesystem { root, subdirectory: Some(path) }
            if root.external() == Some("/example/repository") && path.to_string() == "authored"
    ));

    for invalid in [
        format!("{whole}subdirectory='../escape'\n"),
        format!("{whole}commit='0000000000000000000000000000000000000000'\n"),
    ] {
        assert!(matches!(
            compile(&[PRINCIPAL, SITE, PROJECT, PLACEMENT, &invalid].concat()),
            Err(Error::Toml(_))
        ));
    }
}

#[test]
fn explicit_workspace_documents_resolve_once_and_retain_document_origins() {
    let root: EntryPath = "gazette.toml".parse().expect("root path");
    let site: EntryPath = "sites/one.toml".parse().expect("site path");
    let root_source = format!("includes=['{site}']\n{PRINCIPAL}");
    let site_source = SITE.as_bytes();
    let parsed = workspace::ParsedWorkspaceDocuments::parse(
        &root,
        [(&root, root_source.as_bytes()), (&site, site_source)],
        toml::SetLimits::default(),
    )
    .expect("one strict modular workspace");
    assert_eq!(parsed.compilation().workspace.sites().count(), 1);
    assert_eq!(parsed.documents().len(), 2);
    assert_eq!(parsed.documents()[0].path, root);
    assert_eq!(parsed.documents()[1].path, site);
    let origin = parsed
        .origin(&workspace::FieldQuery {
            kind: workspace::DeclarationKind::Site,
            identity: "site:one".parse().expect("site key"),
            field: vec![gazette_compiler::toml::PathSegment::Key("publisher".into())],
        })
        .expect("site-local origin");
    assert_eq!(origin.document, site);
    let publisher = site_source
        .windows(b"owner:publisher".len())
        .position(|bytes| bytes == b"owner:publisher")
        .expect("publisher reference");
    let association = parsed
        .navigation()
        .at(&site, publisher)
        .expect("cross-document publisher association");
    assert_eq!(association.symbol.symbol.name(), "owner:publisher");
    assert_eq!(association.occurrence.document, &site);
    assert_eq!(association.definition.document, &root);
    assert_eq!(association.references.len(), 1);
    let reference = parsed
        .navigation()
        .reference_at(&site, publisher)
        .expect("typed cross-document reference");
    assert_eq!(reference.symbol.expect("resolved symbol").document, root);
    assert_eq!(
        reference
            .candidates()
            .next()
            .expect("principal candidate")
            .replacement(),
        "'owner:publisher'"
    );
    assert_eq!(parsed.navigation().symbols(&root).count(), 1);
    assert_eq!(parsed.navigation().symbols(&site).count(), 1);
}

#[test]
fn modular_workspace_refuses_self_nested_or_incomplete_include_sets() {
    let root: EntryPath = "gazette.toml".parse().expect("root path");
    let site: EntryPath = "sites/one.toml".parse().expect("site path");
    let root_only = |source: &str| {
        workspace::compile_documents(
            &root,
            [(&root, source.as_bytes())],
            toml::SetLimits::default(),
        )
        .expect_err("invalid module set")
        .code()
    };
    assert_eq!(
        root_only("includes=['gazette.toml']\n"),
        "workspace_invalid"
    );
    assert_eq!(
        root_only("includes=['sites/one.toml']\n"),
        "workspace_invalid"
    );

    let root_source = "includes=['sites/one.toml']\n";
    let nested = "includes=['sites/two.toml']\n";
    let error = workspace::compile_documents(
        &root,
        [(&root, root_source.as_bytes()), (&site, nested.as_bytes())],
        toml::SetLimits::default(),
    )
    .expect_err("included files cannot recursively include more files");
    assert_eq!(error.code(), "workspace_invalid");
}

#[test]
fn machine_roots_keep_confined_logical_paths_and_root_only_binding_selection() {
    for spelling in [
        "machine:documents",
        "machine:repositories/project/docs",
        "machine:builds/site",
    ] {
        let root: SourceRoot = spelling.parse().unwrap();
        assert_eq!(root.to_string(), spelling);
        assert!(root.external().is_none());
        assert!(root.workspace_relative().is_none());
    }
    for spelling in [
        "machine:",
        "machine:/docs",
        "machine:../docs",
        "machine:1root/docs",
        "machine:documents/",
        "machine:documents/../outside",
        "machine:documents/./page",
        "machine:documents/a//b",
        "machine:documents/a\\b",
        "machine:documents/%2e%2e/page",
    ] {
        assert!(spelling.parse::<SourceRoot>().is_err(), "{spelling}");
    }
    let root: EntryPath = "gazette.toml".parse().unwrap();
    let module: EntryPath = "sites/one.toml".parse().unwrap();
    for (root_source, module_source, expected) in [
        (
            "includes=['sites/one.toml']\nmachine_binding='gazette.toml'\n",
            SITE.to_owned(),
            "workspace_machine_binding_document",
        ),
        (
            "includes=['sites/one.toml']\nmachine_binding='sites/one.toml'\n",
            SITE.to_owned(),
            "workspace_machine_binding_document",
        ),
        (
            "includes=['sites/one.toml']\n",
            format!("machine_binding='studio.local.toml'\n{SITE}"),
            "workspace_machine_binding_nested",
        ),
    ] {
        let root_source = format!("{root_source}{PRINCIPAL}");
        let error = workspace::compile_documents(
            &root,
            [
                (&root, root_source.as_bytes()),
                (&module, module_source.as_bytes()),
            ],
            Default::default(),
        )
        .unwrap_err();
        let Error::Invalid(diagnostics) = error else {
            panic!("semantic binding selection failure");
        };
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == expected),
            "{diagnostics:?}"
        );
    }
    let source = format!("machine_binding='tools/studio.local.toml'\n{PRINCIPAL}{SITE}");
    let compiled = compile(&source).unwrap();
    assert_eq!(
        compiled.workspace.machine_binding().unwrap().as_str(),
        "tools/studio.local.toml"
    );
}
