//! Shared selected-source state exercises the real compiler, not an editor-state mock.

use gazette_sdk::{
    authoring::{
        self,
        session::{Error, Limits, Session, Source},
    },
    source::Sha256Digest,
};

const WORKSPACE: &str = "[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n";
const COLLECTION: &str = "+++\n[content]\nid='collection:guide'\nsite='site:one'\ntitle='Guide'\nlifecycle='draft'\nstructure={kind='collection'}\n+++\n# Collection\n";
const PAGE: &str = "+++\ncollections=['collection:guide']\n[content]\nid='content:one'\nsite='site:one'\ntitle='One'\nlifecycle='draft'\nstructure={kind='page',family={kind='article'}}\n+++\n# One\n";

fn source(id: &str, path: &str, text: &str) -> Source {
    Source {
        id: id.into(),
        path: path.parse().expect("selected logical source path"),
        text: text.into(),
    }
}

fn session(limits: Limits) -> Session {
    Session::new(
        source("workspace", "workspace.toml", WORKSPACE),
        [
            source("collection", "collection.md", COLLECTION),
            source("page", "page.md", PAGE),
        ],
        limits,
    )
    .expect("complete real selected baseline")
}

fn assert_current_native_parity(state: &Session) {
    fn input(document: &gazette_sdk::authoring::session::Document) -> authoring::Input<'_> {
        authoring::Input {
            path: document.path().clone(),
            bytes: document.text().as_bytes(),
        }
    }
    let native = authoring::analyze(
        input(&state.files()["workspace"]),
        [
            input(&state.files()["collection"]),
            input(&state.files()["page"]),
        ],
        Default::default(),
    )
    .expect("same exact current compiler inputs");
    let current = state
        .analysis()
        .as_ref()
        .expect("current native phase result");
    assert_eq!(current.inputs(), native.inputs());
    assert_eq!(current.structurally_valid(), native.structurally_valid());
    for document in state.files().values() {
        let current = authoring::diagnostics(current, document.path(), document.text(), 1 << 20)
            .expect("current exact diagnostics");
        let native = authoring::diagnostics(&native, document.path(), document.text(), 1 << 20)
            .expect("direct exact diagnostics");
        assert_eq!(
            serde_json::to_value(current).expect("current projection"),
            serde_json::to_value(native).expect("direct projection")
        );
    }
}

#[test]
fn versions_and_close_share_current_cross_document_semantics_without_stale_success() {
    let mut state = session(Default::default());
    assert_eq!(state.generation(), 1);
    assert!(
        state
            .analysis()
            .as_ref()
            .expect("baseline analysis")
            .structurally_valid()
    );
    assert_current_native_parity(&state);
    assert!(
        !state
            .change("collection", 1, "unopened".into())
            .expect("unopened no-op")
    );
    assert!(
        !state
            .open("unselected", 1, "not admitted".into())
            .expect("unselected no-op")
    );
    assert_eq!(state.generation(), 1);
    assert!(
        state
            .open("collection", 7, COLLECTION.into())
            .expect("selected overlay")
    );
    assert!(
        !state
            .open("collection", 8, "already open".into())
            .expect("already-open no-op")
    );
    let changed = COLLECTION.replace("collection:guide", "collection:changed");
    assert!(
        state
            .change("collection", 8, changed.clone())
            .expect("new source version")
    );
    assert_eq!(state.generation(), 3);
    assert_eq!(state.files()["collection"].version(), Some(8));
    assert_eq!(state.files()["collection"].text(), changed);
    let current = state
        .analysis()
        .as_ref()
        .expect("native cross-document failure record");
    assert!(!current.structurally_valid());
    assert!(
        current
            .failures()
            .iter()
            .any(|failure| matches!(failure, authoring::Failure::Authored(_)))
    );
    assert_eq!(
        current.inputs()["collection.md"],
        Sha256Digest::of(changed.as_bytes())
    );
    assert_current_native_parity(&state);
    assert!(
        !state
            .change("collection", 8, COLLECTION.into())
            .expect("same version no-op")
    );
    assert!(
        !state
            .change("collection", 7, COLLECTION.into())
            .expect("older version no-op")
    );
    assert_eq!(state.generation(), 3);
    assert_eq!(state.files()["collection"].text(), changed);
    assert!(state.close("collection").expect("restore exact baseline"));
    assert_eq!(state.generation(), 4);
    assert_eq!(state.files()["collection"].version(), None);
    assert_eq!(state.files()["collection"].text(), COLLECTION);
    assert!(
        state
            .analysis()
            .as_ref()
            .expect("restored analysis")
            .structurally_valid()
    );
    assert_current_native_parity(&state);
    assert!(!state.close("collection").expect("already closed no-op"));
    assert_eq!(state.generation(), 4);
}

#[test]
fn complete_analysis_failure_replaces_previous_result_and_next_version_recovers() {
    let limits = Limits {
        analysis: authoring::Limits {
            headings: 2,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut state = session(limits);
    assert!(
        state
            .analysis()
            .as_ref()
            .expect("two initial headings")
            .structurally_valid()
    );
    let excessive = format!("{PAGE}\n# Extra\n");
    assert!(
        state
            .open("page", 1, excessive.clone())
            .expect("bounded state admits failed analysis")
    );
    assert!(matches!(
        state.analysis(),
        Err(authoring::Error::Limit("headings"))
    ));
    assert_eq!(state.generation(), 2);
    assert_eq!(state.files()["page"].text(), excessive);
    assert_eq!(state.files()["page"].version(), Some(1));
    assert!(
        state
            .change("page", 2, PAGE.into())
            .expect("new exact text recovers")
    );
    assert_eq!(state.generation(), 3);
    assert!(
        state
            .analysis()
            .as_ref()
            .expect("current result restored")
            .structurally_valid()
    );
    assert_eq!(
        state
            .analysis()
            .as_ref()
            .expect("fresh input hash")
            .inputs()["page.md"],
        Sha256Digest::of(PAGE.as_bytes())
    );
}

#[test]
fn retained_baseline_and_overlay_budget_is_exact_and_failed_update_is_atomic() {
    let baseline = WORKSPACE.len() + COLLECTION.len() + PAGE.len();
    let exact = Limits {
        documents: 3,
        identifier_bytes: "workspacecollectionpage".len(),
        source_bytes: baseline + PAGE.len(),
        ..Default::default()
    };
    let mut state = session(exact);
    assert!(
        state
            .open("page", 1, PAGE.into())
            .expect("exact baseline-plus-overlay ceiling")
    );
    let before = state
        .analysis()
        .as_ref()
        .expect("before rejected update")
        .inputs()
        .clone();
    assert_eq!(
        state.change("page", 2, format!("{PAGE}x")),
        Err(Error::Limit("retained source bytes"))
    );
    assert_eq!(state.generation(), 2);
    assert_eq!(state.files()["page"].version(), Some(1));
    assert_eq!(state.files()["page"].text(), PAGE);
    assert_eq!(
        state
            .analysis()
            .as_ref()
            .expect("unchanged complete result")
            .inputs(),
        &before
    );
    assert!(state.close("page").expect("release only overlay retention"));
    assert!(
        state
            .open("page", -1, PAGE.into())
            .expect("reopen uses restored capacity and a new version stream")
    );
    assert_eq!(state.files()["page"].version(), Some(-1));
    for limits in [
        Limits {
            documents: 2,
            ..exact
        },
        Limits {
            identifier_bytes: exact.identifier_bytes - 1,
            ..exact
        },
        Limits {
            source_bytes: baseline - 1,
            ..exact
        },
    ] {
        assert!(matches!(
            Session::new(
                source("workspace", "workspace.toml", WORKSPACE),
                [
                    source("collection", "collection.md", COLLECTION),
                    source("page", "page.md", PAGE)
                ],
                limits
            ),
            Err(Error::Limit(_))
        ));
    }
    for duplicate in [
        source("workspace", "other.md", PAGE),
        source("different", "workspace.toml", PAGE),
    ] {
        assert!(matches!(
            Session::new(
                source("workspace", "workspace.toml", WORKSPACE),
                [duplicate],
                Default::default()
            ),
            Err(Error::Selection(_))
        ));
    }
}

#[test]
fn path_and_identifier_bounds_reject_before_retaining_a_partial_selection() {
    let path_bytes = "workspace.tomlcollection.mdpage.md".len();
    for allowed in ["workspace.toml".len() - 1, path_bytes - 1] {
        let mut bounded = Limits::default();
        bounded.analysis.corpus.path_bytes = allowed;
        assert!(
            matches!(
                Session::new(
                    source("workspace", "workspace.toml", WORKSPACE),
                    [
                        source("collection", "collection.md", COLLECTION),
                        source("page", "page.md", PAGE)
                    ],
                    bounded,
                ),
                Err(Error::Limit("selected path bytes"))
            ),
            "no partial session escapes a rejected whole-set path budget"
        );
    }
    let mut exact = Limits::default();
    exact.analysis.corpus.path_bytes = path_bytes;
    assert!(
        session(exact)
            .analysis()
            .as_ref()
            .expect("exact shared path bound")
            .structurally_valid()
    );
    assert!(matches!(
        Session::new(
            source("workspace", "workspace.toml", WORKSPACE),
            [],
            Limits {
                identifier_bytes: "workspace".len() - 1,
                ..Default::default()
            }
        ),
        Err(Error::Limit("selected identifier bytes"))
    ));
}

#[test]
fn client_identifiers_never_read_native_files_and_close_uses_selected_baseline() {
    let directory = tempfile::tempdir().expect("disposable unrelated native file");
    let native = directory.path().join("workspace.toml");
    std::fs::write(&native, "PRIVATE invalid native bytes")
        .expect("native file deliberately differs");
    let identifier = format!("file://{}", native.display());
    let mut state = Session::new(
        source(&identifier, "workspace.toml", WORKSPACE),
        [],
        Default::default(),
    )
    .expect("supplied baseline only");
    assert!(
        state
            .analysis()
            .as_ref()
            .expect("no native parsing")
            .structurally_valid()
    );
    assert!(
        state
            .open(&identifier, 1, "[[".into())
            .expect("supplied invalid overlay")
    );
    assert!(
        !state
            .analysis()
            .as_ref()
            .expect("current syntax failure")
            .structurally_valid()
    );
    assert!(
        state
            .close(&identifier)
            .expect("selected baseline restored")
    );
    assert_eq!(state.files()[&identifier].text(), WORKSPACE);
    assert!(
        state
            .analysis()
            .as_ref()
            .expect("baseline, not disk")
            .structurally_valid()
    );
    assert_eq!(
        std::fs::read(&native).expect("no source mutation"),
        b"PRIVATE invalid native bytes"
    );
}

#[test]
fn saving_the_unchanged_malformed_baseline_discards_completion_history() {
    let broken = WORKSPACE.replace("publisher='owner:one'", "publisher='owner:o");
    let offset = broken.find("publisher=").unwrap() + "publisher='owner:o".len();
    let mut state = Session::new(
        source("workspace", "workspace.toml", &broken),
        [],
        Default::default(),
    )
    .unwrap();
    state.open("workspace", 1, broken.clone()).unwrap();
    state.change("workspace", 2, WORKSPACE.into()).unwrap();
    state.change("workspace", 3, broken.clone()).unwrap();
    assert!(
        state
            .complete_previous_reference("workspace", offset, 1 << 20)
            .unwrap()
            .is_some()
    );
    let generation = state.generation();
    assert!(!state.save("workspace", &broken).unwrap());
    assert_eq!(state.generation(), generation);
    assert_eq!(state.files()["workspace"].version(), Some(3));
    assert_eq!(state.files()["workspace"].text(), broken);
    assert!(
        state
            .complete_previous_reference("workspace", offset, 1 << 20)
            .unwrap()
            .is_none()
    );
}
