//! Current native reference edits, not fuzzy completion over earlier valid buffers.

use gazette_sdk::{
    authoring::{
        self, NavigationError,
        session::{Session, Source},
    },
    source::{Precision, Sha256Digest},
    workspace::ParsedWorkspace,
};

const TEXT: &str = "# é🦀 owner:one\r\nprincipals=[{id=\"owner:\\u006fne\",kind='person'},{id='owner:two',kind='organization'}]\r\nsites=[{id='site:one',publisher='owner:one',deployable=false}]\r\n";

fn state() -> Session {
    Session::new(
        Source {
            id: "workspace".into(),
            path: "workspace.toml".parse().expect("path"),
            text: TEXT.into(),
        },
        [],
        Default::default(),
    )
    .expect("native session")
}

#[test]
fn exact_replacements_compile_and_bind_complete_current_source_without_a_catalog() {
    let state = state();
    let document = &state.files()["workspace"];
    let analysis = state.analysis().as_ref().expect("analysis");
    let offset = TEXT.rfind("owner:one").expect("reference");
    let result = analysis
        .complete_reference(document.path(), TEXT, offset, 1 << 18)
        .expect("source")
        .expect("typed slot");
    assert_eq!(result.preimage, "'owner:one'");
    assert_eq!(result.source_sha256, Sha256Digest::of(TEXT.as_bytes()));
    assert_eq!(result.location.precision, Precision::Exact);
    assert_eq!(
        result
            .candidates
            .iter()
            .map(|v| v.replacement.as_ref())
            .collect::<Vec<_>>(),
        ["\"owner:\\u006fne\"", "'owner:two'"]
    );
    for candidate in &result.candidates {
        let mut changed = TEXT.to_owned();
        changed.replace_range(
            result.location.span.start..result.location.span.end,
            candidate.replacement.as_ref(),
        );
        let parsed = ParsedWorkspace::parse(&changed, Default::default())
            .expect("actual substituted TOML compiles");
        assert_eq!(
            &parsed
                .compilation()
                .workspace
                .sites()
                .next()
                .expect("site")
                .publisher,
            &candidate
                .symbol
                .declaration()
                .expect("Key declaration")
                .identity
        );
    }
    let bytes = serde_json::to_vec(&result)
        .expect("shared projection")
        .len();
    assert!(
        analysis
            .complete_reference(document.path(), TEXT, offset, bytes)
            .is_ok()
    );
    assert!(matches!(
        analysis.complete_reference(document.path(), TEXT, offset, bytes - 1),
        Err(NavigationError::Limit)
    ));
    assert!(matches!(
        analysis.complete_reference(document.path(), &(TEXT.to_owned() + "\n"), offset, bytes),
        Err(NavigationError::InvalidSource)
    ));
    for offset in [
        0,
        TEXT.find("owner:\\u006fne").expect("declaration"),
        TEXT.len(),
    ] {
        assert!(
            analysis
                .complete_reference(document.path(), TEXT, offset, 1 << 18)
                .expect("current nonreference")
                .is_none()
        );
    }
}

#[test]
fn unresolved_current_reference_never_reuses_a_prior_selected_model() {
    let path: gazette_sdk::source::EntryPath = "workspace.toml".parse().expect("path");
    for source in [
        TEXT.replace("publisher='owner:one'", "publisher='owner:missing'"),
        "[broken\n".into(),
    ] {
        let analysis = authoring::analyze(
            authoring::Input {
                path: path.clone(),
                bytes: source.as_bytes(),
            },
            [],
            Default::default(),
        )
        .expect("native failed analysis retained");
        if source == "[broken\n" {
            assert!(matches!(
                analysis.complete_reference(&path, &source, 0, 1 << 18),
                Err(NavigationError::Unavailable)
            ));
        } else {
            assert!(
                analysis
                    .complete_reference(&path, &source, 0, 1 << 18)
                    .expect("current typed facts, not an older model")
                    .is_none(),
                "a comment is not a reference slot"
            );
        }
    }
}
