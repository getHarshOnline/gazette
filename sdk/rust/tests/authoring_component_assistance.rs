//! Selected Theme component facts remain source-bound across SDK session overlays.

use gazette_sdk::{
    authoring::{
        self, ComponentCompletion, Failure, PackageBaselineKey,
        session::{PackageSource, Session, Source},
    },
    source::Sha256Digest,
};

const WORKSPACE: &str = "[composition]\ntheme={package='theme:one'}\n\
[[principals]]\nid='owner:one'\nkind='person'\n\
[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n\
[[sources]]\nid='source:theme'\nlocation={kind='repository',root='/not-opened/theme',commit='0000000000000000000000000000000000000000',subdirectory='theme'}\n\
[[sources]]\nid='source:content'\nlocation={kind='repository',root='/not-opened/content',commit='0000000000000000000000000000000000000000',subdirectory='content'}\n\
[[theme_packages]]\npackage='theme:one'\nsource='source:theme'\nmanifest='theme.toml'\n\
[[brands]]\nid='brand:one'\nname='One'\n[[brands.publications]]\nsite='site:one'\nauthored=[{source='source:content',target='authored'}]\n";

const THEME: &str = "package='theme:one'\nnamespace='main'\ntemplates='templates'\nrender={page='page.html'}\n\
[components.callout]\ntemplate='callout.html'\n\
arguments={type='object',required=['tone'],properties={tone={'$ref'='#/$defs/tone'}},'$defs'={tone={type='string',enum=['note','warning']}},additionalProperties=false}\n\
[components.figure]\ntemplate='figure.html'\n";

fn page(component: &str) -> String {
    format!(
        "+++\n[content]\nid='article:one'\nsite='site:one'\ntitle='One'\nlifecycle='draft'\n\
[content.structure]\nkind='page'\nfamily={{kind='article'}}\n+++\n😀\n\n:::{component} {{tone='note'}}\nBody\n:::\n"
    )
}

fn source(id: &str, path: &str, text: impl Into<String>) -> Source {
    Source {
        id: id.into(),
        path: path.parse().expect("fixture path"),
        text: text.into(),
    }
}

fn package(id: &str, text: impl Into<String>) -> PackageSource {
    PackageSource {
        package: PackageBaselineKey::Theme("theme:one".parse().expect("package identity")),
        input: source(id, "packages/theme.toml", text),
    }
}

fn session(page: String) -> Session {
    Session::new_with_package_baselines(
        source("workspace", "workspace.toml", WORKSPACE),
        [source("page", "authored/page.md", page)],
        [package("theme", THEME)],
        Default::default(),
    )
    .expect("explicit selected session")
}

#[test]
fn exact_component_name_completion_and_hover_use_the_selected_theme_catalog() {
    let state = session(page("call"));
    let document = state.files().get("page").expect("selected page");
    let offset = document.text().find(":::call").unwrap() + 5;
    let help = state
        .analysis()
        .as_ref()
        .expect("current analysis")
        .component_assistance(document.path(), document.text(), offset, 1 << 20)
        .expect("source-bound query")
        .expect("component slot");
    let ComponentCompletion::Name {
        location,
        preimage,
        candidates,
    } = help.completion.expect("name completion")
    else {
        panic!("name slot returned argument completion")
    };
    assert_eq!(location.span.slice(document.text()), Some("call"));
    assert_eq!(preimage, "call");
    assert_eq!(
        candidates
            .iter()
            .map(|candidate| candidate.replacement.as_ref())
            .collect::<Vec<_>>(),
        ["callout", "figure"]
    );
    assert!(
        help.hover.is_none(),
        "partial names never borrow a definition"
    );
    assert_eq!(
        help.source_sha256,
        Sha256Digest::of(document.text().as_bytes())
    );

    let exact = session(page("callout"));
    let document = exact.files().get("page").unwrap();
    let offset = document.text().find("callout").unwrap() + 2;
    let hover = exact
        .analysis()
        .as_ref()
        .unwrap()
        .component_assistance(document.path(), document.text(), offset, 1 << 20)
        .unwrap()
        .unwrap()
        .hover
        .expect("exact selected definition");
    assert_eq!(hover.name.as_str(), "callout");
    assert_eq!(hover.location.span.slice(document.text()), Some("callout"));
    assert_eq!(hover.arguments["$defs"]["tone"]["enum"][1], "warning");
}

#[test]
fn component_argument_names_and_closed_values_reuse_the_selected_schema_and_exact_bytes() {
    let property_page = page("callout").replace("tone='note'", "to='note'");
    let state = session(property_page);
    let document = state.files().get("page").unwrap();
    assert!(
        state
            .analysis()
            .as_ref()
            .unwrap()
            .failures()
            .iter()
            .any(|failure| matches!(failure, Failure::Components { .. })),
        "the same Theme contract must diagnose the incomplete property"
    );
    let offset = document.text().find("to='note'").unwrap() + 2;
    let assistance = state
        .analysis()
        .as_ref()
        .unwrap()
        .component_assistance(document.path(), document.text(), offset, 1 << 20)
        .unwrap()
        .unwrap();
    let ComponentCompletion::Argument {
        location,
        preimage,
        completion_kind,
        candidates,
    } = assistance.completion.expect("property completion")
    else {
        panic!("argument property returned name completion")
    };
    assert_eq!(
        completion_kind,
        gazette_sdk::workspace::CompletionKind::Field
    );
    assert_eq!(location.span.slice(document.text()), Some("to"));
    assert_eq!(preimage, "to");
    assert_eq!(
        candidates
            .iter()
            .map(|candidate| candidate.replacement.as_str())
            .collect::<Vec<_>>(),
        ["tone"]
    );

    let value_page = page("callout").replace("tone='note'", "tone='n'");
    let state = session(value_page);
    let document = state.files().get("page").unwrap();
    let offset = document.text().find("'n'").unwrap() + 2;
    let assistance = state
        .analysis()
        .as_ref()
        .unwrap()
        .component_assistance(document.path(), document.text(), offset, 1 << 20)
        .unwrap()
        .unwrap();
    let ComponentCompletion::Argument {
        location,
        preimage,
        completion_kind,
        candidates,
    } = assistance.completion.expect("closed value completion")
    else {
        panic!("argument value returned name completion")
    };
    assert_eq!(
        completion_kind,
        gazette_sdk::workspace::CompletionKind::Literal
    );
    assert_eq!(location.span.slice(document.text()), Some("'n'"));
    assert_eq!(preimage, "'n'");
    assert_eq!(
        candidates
            .iter()
            .map(|candidate| candidate.replacement.as_str())
            .collect::<Vec<_>>(),
        ["\"note\"", "\"warning\""]
    );
}

#[test]
fn package_overlay_replaces_current_component_semantics_and_close_restores_baseline() {
    let mut state = session(page("missing"));
    assert!(
        state
            .analysis()
            .as_ref()
            .unwrap()
            .failures()
            .iter()
            .any(|failure| matches!(failure, Failure::Components { .. }))
    );
    let changed = THEME.replace("components.callout", "components.missing");
    assert!(state.open("theme", 1, changed).expect("open overlay"));
    assert!(
        !state
            .analysis()
            .as_ref()
            .unwrap()
            .failures()
            .iter()
            .any(|failure| matches!(failure, Failure::Components { .. }))
    );
    assert!(state.close("theme").expect("restore baseline"));
    assert!(
        state
            .analysis()
            .as_ref()
            .unwrap()
            .failures()
            .iter()
            .any(|failure| matches!(failure, Failure::Components { .. }))
    );
}

#[test]
fn duplicate_package_identity_and_manifest_identity_mismatch_fail_closed() {
    assert!(
        Session::new_with_package_baselines(
            source("workspace", "workspace.toml", WORKSPACE),
            [],
            [
                package("theme", THEME),
                PackageSource {
                    package: PackageBaselineKey::Theme("theme:one".parse().unwrap()),
                    input: source("other", "packages/other.toml", THEME),
                },
            ],
            Default::default(),
        )
        .is_err()
    );

    let state = Session::new_with_package_baselines(
        source("workspace", "workspace.toml", WORKSPACE),
        [source("page", "authored/page.md", page("callout"))],
        [package(
            "theme",
            THEME.replace("package='theme:one'", "package='theme:other'"),
        )],
        Default::default(),
    )
    .expect("selection shape is unambiguous");
    assert!(
        state
            .analysis()
            .as_ref()
            .unwrap()
            .failures()
            .iter()
            .any(|failure| matches!(failure, Failure::Package { .. }))
    );
    let package = state
        .package_baseline("theme")
        .expect("selected package role remains typed");
    assert!(state.package_baseline("workspace").is_none());
    let document = state.files().get("theme").unwrap();
    let rows = authoring::package_diagnostics(
        state.analysis().as_ref().unwrap(),
        package,
        document.path(),
        document.text(),
        1 << 20,
    )
    .expect("failure projects only against its exact package baseline");
    let row = rows
        .iter()
        .find(|row| row.code == "package_binding")
        .expect("typed package binding diagnostic");
    assert_eq!(row.path, document.path());
    assert!(row.selection_wide);
    assert!(row.location.is_none());
    assert_eq!(row.source.unwrap().as_str(), "source:theme");
    assert!(matches!(
        authoring::package_diagnostics(
            state.analysis().as_ref().unwrap(),
            package,
            document.path(),
            "changed",
            1 << 20,
        ),
        Err(authoring::DiagnosticError::InvalidSource)
    ));
    let workspace = state.files().get("workspace").unwrap();
    assert!(
        authoring::diagnostics(
            state.analysis().as_ref().unwrap(),
            workspace.path(),
            workspace.text(),
            1 << 20,
        )
        .expect("workspace projection remains source-bound")
        .iter()
        .all(|row| row.code != "package_binding")
    );
}
