//! Shared session retains current schema help independently from strict workspace validity.

use gazette_sdk::{
    authoring::{
        self, AssistanceScope,
        session::{Session, Source},
    },
    workspace::{AuthoringFieldPresence, CompletionKind},
};

const VALID: &str = "[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n";

fn source(text: &str) -> Source {
    Source {
        id: "workspace".into(),
        path: "workspace.toml".parse().expect("path"),
        text: text.into(),
    }
}

#[test]
fn authored_principal_repair_tracks_native_workspace_overlays_and_exact_source_preimages() {
    let page = "+++\r\n# é🦀\r\n[content]\r\nid='content:one'\r\nsite='site:one'\r\ntitle='One'\r\nlifecycle='draft'\r\nstructure={kind='page',family={kind='article'}}\r\nattributions=[{principal='owner:typo',role='author'}]\r\n+++\r\n# Body\r\n";
    let mut session = Session::new(
        source(VALID),
        [Source {
            id: "article".into(),
            path: "content/one.md".parse().unwrap(),
            text: page.into(),
        }],
        Default::default(),
    )
    .unwrap();
    let article = session.files()["article"].path().clone();
    let offset = page.find("'owner:typo'").unwrap();
    let query = |state: &Session| {
        serde_json::to_value(
            state
                .analysis()
                .as_ref()
                .unwrap()
                .complete_authored_reference(&article, page, offset, 1 << 18)
                .unwrap()
                .unwrap(),
        )
        .unwrap()
    };
    let baseline = query(&session);
    assert_eq!(baseline["field"], "attribution_principal");
    assert_eq!(baseline["current"], serde_json::Value::Null);
    assert_eq!(baseline["candidates"][0]["identity"], "owner:one");
    assert_eq!(baseline["preimage"], "'owner:typo'");
    assert_eq!(
        baseline["source_sha256"],
        serde_json::json!(gazette_sdk::source::Sha256Digest::of(page.as_bytes()))
    );
    let overlay = VALID
        .replace("id='owner:one'", "id='owner:two'")
        .replace("publisher='owner:one'", "publisher='owner:two'");
    assert!(session.open("workspace", 7, overlay.clone()).unwrap());
    let changed = query(&session);
    assert_eq!(changed["candidates"][0]["identity"], "owner:two");
    assert_eq!(
        changed["candidates"][0]["source_sha256"],
        serde_json::json!(gazette_sdk::source::Sha256Digest::of(overlay.as_bytes()))
    );
    assert!(
        !session.change("workspace", 7, VALID.into()).unwrap(),
        "nonincreasing overlay never replaces the current declaration set"
    );
    assert_eq!(query(&session), changed);
    assert!(matches!(
            session
                .analysis()
                .as_ref()
                .unwrap()
                .complete_authored_reference(
                    &article,
                    &(page.to_owned() + "changed"),
                    offset,
                    1 << 18,
                ),
            Err(authoring::NavigationError::InvalidSource)
        ));
    assert!(session.close("workspace").unwrap());
    assert_eq!(
        query(&session),
        baseline,
        "close restores only captured current native workspace bytes"
    );
}

#[test]
fn reduced_legal_hints_retain_selected_binding_through_invalid_unsaved_frontmatter_and_close() {
    let workspace = "principals=[{id='owner:one',kind='person'}]\nsites=[{id='site:one',publisher='owner:one',deployable=false}]\nsources=[{id='source:legal',location={kind='filesystem',root='.'}}]\n[[brands]]\nid='brand:one'\nname='One'\n[[brands.publications]]\nsite='site:one'\npublic_base_url='https://one.test/'\nlegal=[{source='source:legal',entry='terms.md',source_content='content:terms',id='content:site-terms',route='/terms/',family='utility'}]\n";
    let legal =
        "+++\n[content]\nid='content:terms'\ntitle='Terms'\nlifecycle='draft'\n+++\n# Terms\n";
    let mut session = Session::new(
        source(workspace),
        [Source {
            id: "legal".into(),
            path: "__legal/source:legal/terms.md".parse().unwrap(),
            text: legal.into(),
        }],
        Default::default(),
    )
    .unwrap();
    assert!(session.analysis().as_ref().unwrap().structurally_valid());
    let incomplete = "+++\r\n# é🦀\r\n[content]\r\nlifecycle = ";
    assert!(session.open("legal", 11, incomplete.into()).unwrap());
    let document = &session.files()["legal"];
    let analysis = session.analysis().as_ref().unwrap();
    assert!(!analysis.structurally_valid());
    let help = analysis
        .assistance(
            document.path(),
            document.text(),
            document.text().len(),
            1 << 18,
        )
        .unwrap()
        .unwrap();
    assert_eq!(help.scope, AssistanceScope::LegalSourceFrontmatter);
    assert_eq!(
        help.help
            .candidates
            .iter()
            .map(|candidate| candidate.label.as_str())
            .collect::<Vec<_>>(),
        ["archived", "draft", "published"]
    );
    assert_eq!(document.version(), Some(11));
    assert!(
        analysis
            .assistance(document.path(), legal, document.text().len(), 1 << 18)
            .is_err(),
        "captured baseline bytes cannot masquerade as the current overlay"
    );
    assert!(session.close("legal").unwrap());
    assert_eq!(session.files()["legal"].text(), legal);
    assert!(session.analysis().as_ref().unwrap().structurally_valid());
}

#[test]
fn invalid_current_frontmatter_retains_only_its_current_schema_facts() {
    let text = "+++\r\n[content]\r\nlifecycle = ";
    let state = Session::new(
        source(VALID),
        [Source {
            id: "article".into(),
            path: "articles/example.md".parse().expect("path"),
            text: text.into(),
        }],
        Default::default(),
    )
    .expect("session");
    let document = &state.files()["article"];
    let assistance = state
        .analysis()
        .as_ref()
        .expect("phase failures retain current analysis")
        .assistance(document.path(), document.text(), text.len(), 1 << 18)
        .expect("current source help")
        .expect("frontmatter help");
    assert_eq!(assistance.scope, AssistanceScope::AuthoredFrontmatter);
    assert_eq!(assistance.path, document.path());
    assert_eq!(
        assistance
            .help
            .candidates
            .iter()
            .map(|candidate| candidate.label.as_str())
            .collect::<Vec<_>>(),
        ["archived", "draft", "published"]
    );
    assert!(
        state
            .analysis()
            .as_ref()
            .expect("analysis")
            .assistance(
                document.path(),
                &(document.text().to_owned() + "# stale"),
                text.len(),
                1 << 18,
            )
            .is_err()
    );
}

fn help(state: &Session, offset: usize) -> authoring::Assistance<'_> {
    let document = &state.files()["workspace"];
    state
        .analysis()
        .as_ref()
        .expect("phase failures retain current analysis")
        .assistance(document.path(), document.text(), offset, 1 << 18)
        .expect("current source-bound help")
        .expect("workspace help")
}

#[test]
fn invalid_current_overlay_replaces_prior_success_without_losing_schema_help() {
    let mut state = Session::new(source(VALID), [], Default::default()).expect("session");
    let incomplete = "[[sites]]\nid='site:one'\npublisher='owner:one'\ndep";
    assert!(
        state
            .open("workspace", 1, incomplete.into())
            .expect("open current overlay")
    );
    let analysis = state.analysis().as_ref().expect("current phase evidence");
    assert!(!analysis.structurally_valid());
    let assistance = help(&state, incomplete.len());
    assert_eq!(state.generation(), 2);
    assert_eq!(assistance.help.completion_kind, Some(CompletionKind::Field));
    assert_eq!(assistance.help.completion_preimage.as_deref(), Some("dep"));
    assert_eq!(assistance.help.candidates.len(), 1);
    assert_eq!(assistance.help.candidates[0].label, "deployable");
    assert_eq!(
        assistance.help.candidates[0].presence,
        Some(AuthoringFieldPresence::Required)
    );

    let value = "[[sites]]\ndeployable = ";
    assert!(
        state
            .change("workspace", 2, value.into())
            .expect("replace exact overlay")
    );
    let assistance = help(&state, value.len());
    assert_eq!(state.generation(), 3);
    assert_eq!(
        assistance.help.completion_kind,
        Some(CompletionKind::Literal)
    );
    assert_eq!(
        assistance
            .help
            .candidates
            .iter()
            .map(|candidate| candidate.label.as_str())
            .collect::<Vec<_>>(),
        ["false", "true"]
    );

    assert!(state.close("workspace").expect("restore selected baseline"));
    assert!(
        state
            .analysis()
            .as_ref()
            .expect("restored strict analysis")
            .structurally_valid()
    );
}

#[test]
fn exact_current_source_and_complete_result_bounds_are_enforced() {
    let state = Session::new(source(VALID), [], Default::default()).expect("session");
    let document = &state.files()["workspace"];
    let analysis = state.analysis().as_ref().expect("analysis");
    assert!(
        analysis
            .assistance(
                document.path(),
                &(VALID.to_owned() + "# changed"),
                VALID.find("deployable").expect("key"),
                1 << 18,
            )
            .is_err()
    );
    assert!(
        analysis
            .assistance(
                document.path(),
                document.text(),
                VALID.find("deployable").expect("key"),
                32,
            )
            .is_err()
    );
}
