//! Plain resource aliases use the same selected-source navigation and completion owner.

use gazette_sdk::{
    authoring::{
        self, NavigationError,
        session::{Session, Source},
    },
    source::{EntryPath, Precision, Sha256Digest},
    workspace::{WorkspaceSymbol, WorkspaceSymbolKind},
};

const ROOT: &str = "includes=['inputs/names.toml','inputs/mounts.toml']\n\
[[principals]]\nid='person:one'\nkind='person'\n\
[[sites]]\nid='site:one'\npublisher='person:one'\ndeployable=false\n\
[[sources]]\nid='source:theme'\nlocation={kind='filesystem',root='/not-opened/theme'}\n\
[[sources]]\nid='source:tokens'\nlocation={kind='filesystem',root='/not-opened/tokens'}\n";
const NAMES: &str = "# 🦀 tokens is prose\r\n[design_inputs]\r\n\"tok\\u0065ns\"='source:tokens'\r\ncolors='source:tokens'\r\n";
const MOUNTS: &str = "[[theme_packages]]\r\npackage='theme:one'\r\nsource='source:theme'\r\nmanifest='theme.toml'\r\nresource_inputs={'shared/_layout.scss'={input='tokens',entry='dist/scss/_layout.scss'}}\r\n";

fn path(value: &str) -> EntryPath {
    value.parse().unwrap()
}
fn source(id: &str, value: &str, text: &str) -> Source {
    Source {
        id: id.into(),
        path: path(value),
        text: text.into(),
    }
}
fn session() -> Session {
    Session::new_with_workspace_documents(
        source("root", "gazette.toml", ROOT),
        [
            source("names", "inputs/names.toml", NAMES),
            source("mounts", "inputs/mounts.toml", MOUNTS),
        ],
        [],
        [],
        Default::default(),
    )
    .unwrap()
}

#[test]
fn modular_alias_navigation_symbols_and_quoted_completion_share_exact_native_baselines() {
    let state = session();
    let analysis = state.analysis().as_ref().unwrap();
    let offset = MOUNTS.find("input='tokens'").unwrap() + "input=".len();
    let navigation = analysis
        .source_navigation(&path("inputs/mounts.toml"), MOUNTS, offset)
        .unwrap()
        .unwrap();
    assert_eq!(navigation.definition.path, &path("inputs/names.toml"));
    assert_eq!(
        navigation.definition.source_sha256,
        Sha256Digest::of(NAMES.as_bytes())
    );
    assert_eq!(
        navigation.definition.location.span.slice(NAMES),
        Some("\"tok\\u0065ns\"")
    );
    assert_eq!(navigation.occurrence.span.slice(MOUNTS), Some("'tokens'"));
    assert_eq!(
        navigation.source_sha256,
        Sha256Digest::of(MOUNTS.as_bytes())
    );
    assert_eq!(navigation.references.len(), 1);
    let identity = analysis
        .workspace_identity(&path("inputs/mounts.toml"), MOUNTS, offset)
        .unwrap()
        .unwrap();
    assert!(
        matches!(identity.symbol, WorkspaceSymbol::ResourceInput { name, .. } if name.as_str() == "tokens")
    );
    assert!(
        identity.composition.is_none(),
        "alias is no publication or authority role"
    );
    let names = analysis
        .workspace_symbols(&path("inputs/names.toml"), NAMES)
        .unwrap()
        .unwrap();
    assert_eq!(
        names.iter().map(|symbol| symbol.name()).collect::<Vec<_>>(),
        ["tokens", "colors"]
    );
    assert!(
        std::ptr::eq(identity.symbol, names[0]),
        "sole retained symbol owner"
    );
    let symbols = state.search_symbols("tokens", 1 << 18).unwrap();
    assert!(symbols.symbols.iter().any(|symbol| symbol.name == "tokens"
        && symbol.source.path == &path("inputs/names.toml")
        && symbol.source.location == navigation.definition.location));
    let completion = analysis
        .complete_reference(&path("inputs/mounts.toml"), MOUNTS, offset, 1 << 18)
        .unwrap()
        .unwrap();
    assert_eq!(completion.kind, WorkspaceSymbolKind::ResourceInput);
    assert_eq!(completion.location.precision, Precision::Exact);
    assert_eq!(completion.preimage, "'tokens'");
    assert_eq!(completion.candidates[0].definition, navigation.definition);
    for candidate in &completion.candidates {
        assert_eq!(candidate.definition.path, &path("inputs/names.toml"));
        assert_eq!(
            candidate.definition.source_sha256,
            Sha256Digest::of(NAMES.as_bytes())
        );
        assert_eq!(candidate.definition.location, candidate.symbol.location());
        assert!(candidate.definition.location.span.slice(NAMES).is_some());
    }
    assert_eq!(
        completion
            .candidates
            .iter()
            .map(|candidate| candidate.replacement.as_ref())
            .collect::<Vec<_>>(),
        ["\"tokens\"", "\"colors\""]
    );
    let bytes = serde_json::to_vec(&completion).unwrap().len();
    assert!(
        analysis
            .complete_reference(&path("inputs/mounts.toml"), MOUNTS, offset, bytes)
            .is_ok()
    );
    assert!(matches!(
        analysis.complete_reference(&path("inputs/mounts.toml"), MOUNTS, offset, bytes - 1),
        Err(NavigationError::Limit)
    ));
    assert!(matches!(
        analysis.complete_reference(
            &path("inputs/mounts.toml"),
            &(MOUNTS.to_owned() + "\n"),
            offset,
            bytes
        ),
        Err(NavigationError::InvalidSource)
    ));
    for candidate in &completion.candidates {
        let mut changed = MOUNTS.to_owned();
        changed.replace_range(
            completion.location.span.start..completion.location.span.end,
            candidate.replacement.as_ref(),
        );
        let fresh = Session::new_with_workspace_documents(
            source("root", "gazette.toml", ROOT),
            [
                source("names", "inputs/names.toml", NAMES),
                source("mounts", "inputs/mounts.toml", &changed),
            ],
            [],
            [],
            Default::default(),
        )
        .unwrap();
        let resolved = fresh
            .analysis()
            .as_ref()
            .unwrap()
            .workspace_identity(&path("inputs/mounts.toml"), &changed, offset)
            .unwrap()
            .unwrap();
        assert_eq!(
            resolved.symbol.name(),
            candidate.symbol.name(),
            "actual edited workspace resolves"
        );
    }
}

#[test]
fn malformed_or_ambiguous_included_overlay_cannot_reuse_alias_associations_and_close_restores_them()
{
    let mut state = session();
    let offset = MOUNTS.find("input='tokens'").unwrap() + "input=".len();
    let duplicate = format!("{NAMES}tokens='source:tokens'\r\n");
    for invalid in [duplicate, "[broken\n".into()] {
        assert!(state.open("names", 1, invalid).unwrap());
        let analysis = state.analysis().as_ref().unwrap();
        assert!(matches!(
            analysis.source_navigation(&path("inputs/mounts.toml"), MOUNTS, offset),
            Err(NavigationError::Unavailable)
        ));
        assert!(matches!(
            analysis.complete_reference(&path("inputs/mounts.toml"), MOUNTS, offset, 1 << 18),
            Err(NavigationError::Unavailable)
        ));
        assert!(state.close("names").unwrap());
        assert!(
            state
                .analysis()
                .as_ref()
                .unwrap()
                .source_navigation(&path("inputs/mounts.toml"), MOUNTS, offset)
                .unwrap()
                .is_some()
        );
    }
}

#[test]
fn missing_plain_alias_has_current_typed_candidates_without_admission_or_cross_namespace_proposals()
{
    let names = NAMES.replace('\r', "");
    let text = format!(
        "{names}{}{}",
        ROOT.replace("includes=['inputs/names.toml','inputs/mounts.toml']\n", ""),
        MOUNTS.replace("input='tokens'", "input='unknown'")
    );
    let state = Session::new(
        source("root", "workspace.toml", &text),
        [],
        Default::default(),
    )
    .unwrap();
    let analysis = state.analysis().as_ref().unwrap();
    assert!(!analysis.structurally_valid());
    let offset = text.find("input='unknown'").unwrap() + "input=".len();
    let result = analysis
        .complete_reference(&path("workspace.toml"), &text, offset, 1 << 18)
        .unwrap()
        .unwrap();
    assert!(result.current.is_none());
    assert_eq!(result.preimage, "'unknown'");
    assert_eq!(result.kind, WorkspaceSymbolKind::ResourceInput);
    for candidate in &result.candidates {
        assert_eq!(candidate.definition.path, &path("workspace.toml"));
        assert_eq!(
            candidate.definition.source_sha256,
            Sha256Digest::of(text.as_bytes())
        );
        assert_eq!(candidate.definition.location, candidate.symbol.location());
    }
    assert!(
        result
            .candidates
            .iter()
            .all(|candidate| matches!(candidate.symbol, WorkspaceSymbol::ResourceInput { .. }))
    );
    assert!(
        !result.matches_unlocated_diagnostic(&"source:tokens".parse().unwrap()),
        "never binds alias repair to a Key diagnostic"
    );
    assert!(matches!(
        analysis.source_navigation(&path("workspace.toml"), &text, offset),
        Err(NavigationError::Unavailable)
    ));
    let symbols = analysis
        .workspace_symbols(&path("workspace.toml"), &text)
        .unwrap()
        .unwrap();
    assert!(symbols.iter().any(|symbol| symbol.name() == "tokens"));
    let (_, navigation) =
        gazette_sdk::workspace::WorkspaceInspection::parse(&text, Default::default())
            .unwrap()
            .into_parts();
    let navigation = navigation.unwrap();
    assert_eq!(
        navigation.key_bytes(),
        navigation
            .symbols()
            .iter()
            .map(|symbol| symbol.name().len())
            .sum::<usize>()
    );
    assert_eq!(
        navigation
            .symbols()
            .iter()
            .filter(|symbol| symbol.kind() == WorkspaceSymbolKind::ResourceInput)
            .count(),
        2
    );
    for limit in [
        authoring::Limits {
            navigation_occurrences: navigation.occurrence_count() - 1,
            ..Default::default()
        },
        authoring::Limits {
            navigation_bytes: navigation.key_bytes() - 1,
            ..Default::default()
        },
    ] {
        assert!(
            authoring::analyze(
                authoring::Input {
                    path: path("workspace.toml"),
                    bytes: text.as_bytes()
                },
                [],
                limit
            )
            .is_err(),
            "aliases are charged to the existing complete native ceilings"
        );
    }
}
