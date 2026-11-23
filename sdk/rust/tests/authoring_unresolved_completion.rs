//! Current unresolved-reference repair must not promote invalid workspace semantics.

use gazette_sdk::{
    authoring::{
        self, NavigationError,
        session::{Session, Source},
    },
    source::{Precision, Sha256Digest},
    workspace::{DeclarationKind, ParsedWorkspace},
};

const TEXT: &str = "# é🦀\r\nprincipals=[{id=\"owner:\\u006fne\",kind='person'},{id='owner:two',kind='organization'}]\r\nprojects=[{id='owner:decoy',lifecycle='active',visibility='public',availability='available'}]\r\nsites=[{id='site:one',publisher='owner:one',deployable=false}]\r\n";
const MAXIMUM: usize = 1 << 18;

fn session(text: &str) -> Session {
    Session::new(
        Source {
            id: "workspace".into(),
            path: "workspace.toml".parse().expect("selected path"),
            text: text.into(),
        },
        [],
        Default::default(),
    )
    .expect("session retains current semantic failures")
}

fn typo() -> String {
    TEXT.replace("publisher='owner:one'", "publisher='owner:typo'")
}

#[test]
fn unresolved_publisher_offers_current_principals_without_erasing_diagnostics() {
    let text = typo();
    let mut state = session(&text);
    assert!(state.open("workspace", 7, text.clone()).unwrap());
    let generation = state.generation();
    let document = &state.files()["workspace"];
    let analysis = state.analysis().as_ref().expect("current analysis");
    assert!(!analysis.structurally_valid());
    assert!(ParsedWorkspace::parse(&text, Default::default()).is_err());
    let before = authoring::diagnostics(analysis, document.path(), &text, MAXIMUM)
        .expect("current diagnostics");
    assert!(before.iter().any(|row| {
        row.code == "workspace_reference"
            && row.field == Some("sites[\"site:one\"].publisher")
            && row.key.is_some_and(|key| key.as_str() == "owner:typo")
    }));
    let before = serde_json::to_value(before).unwrap();
    let completion = analysis
        .complete_reference(
            document.path(),
            &text,
            text.rfind("owner:typo").unwrap(),
            MAXIMUM,
        )
        .expect("a semantic typo does not erase its typed reference slot")
        .expect("publisher reference replacement");
    assert_eq!(completion.preimage, "'owner:typo'");
    assert!(completion.current.is_none());
    assert_eq!(
        completion.kind,
        gazette_sdk::workspace::WorkspaceSymbolKind::Declaration(DeclarationKind::Principal)
    );
    assert_eq!(completion.source_sha256, Sha256Digest::of(text.as_bytes()));
    assert_eq!(completion.location.precision, Precision::Exact);
    assert_eq!(
        completion.location.span.slice(&text),
        Some(completion.preimage)
    );
    assert_eq!(
        completion
            .candidates
            .iter()
            .map(|candidate| candidate.replacement.as_ref())
            .collect::<Vec<_>>(),
        ["\"owner:\\u006fne\"", "'owner:two'"]
    );
    assert!(completion.candidates.iter().all(|candidate| {
        candidate
            .symbol
            .declaration()
            .expect("Key declaration")
            .kind
            == DeclarationKind::Principal
            && candidate.symbol.name() != "owner:decoy"
    }));
    let candidate = completion
        .candidates
        .iter()
        .find(|candidate| candidate.symbol.name() == "owner:one")
        .expect("declared principal, never a prefix-matched project");
    let mut repaired = text.clone();
    repaired.replace_range(
        completion.location.span.start..completion.location.span.end,
        candidate.replacement.as_ref(),
    );
    assert_eq!(
        serde_json::to_value(
            authoring::diagnostics(analysis, document.path(), &text, MAXIMUM).unwrap()
        )
        .unwrap(),
        before,
        "querying suggestions does not accept or mutate the workspace"
    );
    assert_eq!(state.generation(), generation);
    assert!(state.change("workspace", 8, repaired.clone()).unwrap());
    assert_eq!(state.generation(), generation + 1);
    let document = &state.files()["workspace"];
    let analysis = state.analysis().as_ref().unwrap();
    assert!(analysis.structurally_valid());
    assert!(
        authoring::diagnostics(analysis, document.path(), &repaired, MAXIMUM)
            .unwrap()
            .is_empty()
    );
    let parsed = ParsedWorkspace::parse(&repaired, Default::default()).unwrap();
    assert_eq!(
        parsed
            .compilation()
            .workspace
            .sites()
            .next()
            .unwrap()
            .publisher
            .as_str(),
        "owner:one"
    );
}

#[test]
fn ambiguous_declarations_and_malformed_syntax_never_offer_a_repair_subset() {
    let duplicate = typo().replace(
        "kind='person'}",
        "kind='person'},{id='owner:one',kind='person'}",
    );
    for text in [duplicate, "[broken\n".into()] {
        let state = session(&text);
        let document = &state.files()["workspace"];
        let analysis = state.analysis().as_ref().unwrap();
        assert!(!analysis.structurally_valid());
        assert!(
            !authoring::diagnostics(analysis, document.path(), &text, MAXIMUM)
                .unwrap()
                .is_empty()
        );
        assert!(matches!(
            analysis.complete_reference(
                document.path(),
                &text,
                text.rfind("owner:typo").unwrap_or(0),
                MAXIMUM
            ),
            Err(NavigationError::Unavailable | NavigationError::Ambiguous)
        ));
    }
}

#[test]
fn stale_versions_and_source_bytes_cannot_replace_current_typo_evidence() {
    let mut state = session(TEXT);
    assert!(state.open("workspace", 7, TEXT.into()).unwrap());
    let previous_generation = state.generation();
    let text = typo();
    assert!(state.change("workspace", 8, text.clone()).unwrap());
    let current_generation = state.generation();
    assert_eq!(current_generation, previous_generation + 1);
    for version in [7, 8] {
        assert!(!state.change("workspace", version, TEXT.into()).unwrap());
        assert_eq!(state.generation(), current_generation);
        assert_eq!(state.files()["workspace"].text(), text);
        assert_eq!(state.files()["workspace"].version(), Some(8));
    }
    let document = &state.files()["workspace"];
    let analysis = state.analysis().as_ref().unwrap();
    assert!(!analysis.structurally_valid());
    assert!(matches!(
        analysis.complete_reference(
            document.path(),
            TEXT,
            TEXT.rfind("owner:one").unwrap(),
            MAXIMUM
        ),
        Err(NavigationError::InvalidSource)
    ));
    let current = analysis
        .complete_reference(
            document.path(),
            &text,
            text.rfind("owner:typo").unwrap(),
            MAXIMUM,
        )
        .expect("current typed slot survives stale update attempts")
        .expect("current publisher slot");
    assert_eq!(current.source_sha256, Sha256Digest::of(text.as_bytes()));
    assert_eq!(current.preimage, "'owner:typo'");
    assert_eq!(state.generation(), current_generation);
}
