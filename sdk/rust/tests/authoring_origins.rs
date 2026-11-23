//! Exact origin queries over retained overlays use the native workspace resolver.

use gazette_sdk::{
    authoring::session::{Limits, OriginError, OriginQuery, Session, Source},
    source::Sha256Digest,
    workspace,
};
use serde_json::{Value, json};

const BASE: &str = "principals=[{id='owner:one',kind='person',name='é🦀'}]\r\nsites=[{id='site:one',publisher='owner:one',deployable=false}]\r\n[composition.capabilities]\r\n'publication.search/v1'={state='disabled'}\r\n";

fn source(id: &str, path: &str, text: &str) -> Source {
    Source {
        id: id.into(),
        path: path.parse().expect("logical path"),
        text: text.into(),
    }
}
fn query() -> OriginQuery {
    serde_json::from_value(json!({"kind":"composition","query":{"site":"site:one","field":{"kind":"capability","capability":"publication.search/v1"},"path":["state"]}})).expect("native query")
}
fn field(identity: &str, key: &str) -> OriginQuery {
    serde_json::from_value(
        json!({"kind":"field","query":{"kind":"principal","identity":identity,"field":[key]}}),
    )
    .expect("native field query")
}
fn expected(text: &str, query: &OriginQuery) -> Value {
    let parsed = workspace::ParsedWorkspace::parse(text, Default::default()).expect("native parse");
    match query {
        OriginQuery::Field { query } => json!(parsed.origin(query).expect("native field")),
        OriginQuery::Composition { query } => json!(
            parsed
                .composition_origin(query)
                .expect("native composition")
        ),
    }
}

#[test]
fn origin_uses_current_inherited_overridden_and_restored_bytes_despite_unrelated_corpus_failure() {
    let mut session = Session::new(
        source("workspace", "workspace.toml", BASE),
        [source("broken", "broken.md", "+++\n[broken\n+++\n")],
        Limits::default(),
    )
    .expect("retained selection");
    assert!(
        session
            .analysis()
            .as_ref()
            .is_ok_and(|analysis| !analysis.structurally_valid()),
        "malformed selected Markdown must produce a current phase failure"
    );
    let query = query();
    let first = session
        .origin(1, &query)
        .expect("independent workspace origin");
    assert_eq!(json!(first.value)["origin"], expected(BASE, &query));
    assert_eq!(
        json!(first.value)["origin"]["source_sha256"],
        json!(Sha256Digest::of(BASE.as_bytes()))
    );
    assert_eq!(first.value.span().slice(BASE), Some("'disabled'"));
    assert_eq!(
        json!(first.value)["origin"]["selection"]["scope"],
        "workspace"
    );
    let direct = field("owner:one", "name");
    assert_eq!(
        json!(session.origin(1, &direct).expect("field").value)["origin"],
        expected(BASE, &direct)
    );
    let changed = BASE.replace(
        "deployable=false}",
        "deployable=false,composition={capabilities={'publication.search/v1'={state=\"disabled\"}}}}",
    );
    assert!(
        session
            .open("workspace", 5, changed.clone())
            .expect("overlay")
    );
    assert_eq!(
        session.origin(1, &query).expect_err("stale generation"),
        OriginError::Generation
    );
    let current = session.origin(2, &query).expect("current site override");
    assert_eq!(current.version, Some(5));
    assert_eq!(json!(current.value)["origin"], expected(&changed, &query));
    assert_eq!(current.value.span().slice(&changed), Some("\"disabled\""));
    assert!(
        !session
            .change("workspace", 5, "broken".into())
            .expect("stale version is no-op")
    );
    assert_eq!(session.generation(), 2);
    assert!(
        session
            .change("workspace", 6, "[broken\n".into())
            .expect("invalid current overlay")
    );
    assert_eq!(
        session
            .origin(3, &query)
            .expect_err("no stale native parse"),
        OriginError::Workspace
    );
    assert!(
        session
            .close("workspace")
            .expect("restore retained baseline")
    );
    let restored = session.origin(4, &query).expect("baseline origin");
    assert_eq!(restored.version, None);
    assert_eq!(json!(restored.value)["origin"], expected(BASE, &query));
    assert_eq!(
        session
            .origin(4, &field("owner:missing", "name"))
            .expect_err("absent owner"),
        OriginError::UnknownIdentity
    );
    assert_eq!(
        session
            .origin(4, &field("owner:one", "missing"))
            .expect_err("no fabricated field"),
        OriginError::NoExplicitField
    );
}

#[test]
fn origin_query_and_complete_result_obey_session_bounds() {
    let mut limits = Limits::default();
    limits.analysis.navigation_bytes = 1;
    let session = Session::new(source("workspace", "workspace.toml", BASE), [], limits)
        .expect("selection remains inspectable");
    assert_eq!(
        session
            .origin(1, &query())
            .expect_err("complete query bound"),
        OriginError::Limit
    );
    let query = query();
    let query_bytes = serde_json::to_vec(&query)
        .expect("complete typed query")
        .len();
    let unrestricted = Session::new(
        source("workspace", "workspace.toml", BASE),
        [],
        Limits::default(),
    )
    .expect("ordinary session");
    let result = unrestricted
        .origin(1, &query)
        .expect("complete native result");
    assert!(
        serde_json::to_vec(&result)
            .expect("native result bytes")
            .len()
            > query_bytes
    );
    let mut limits = Limits::default();
    limits.analysis.navigation_bytes = query_bytes;
    let bounded = Session::new(source("workspace", "workspace.toml", BASE), [], limits)
        .expect("bounded session");
    assert_eq!(
        bounded
            .origin(1, &query)
            .expect_err("query fits but complete origin does not"),
        OriginError::Limit
    );
}

#[test]
fn modular_origin_returns_the_exact_owning_document_and_overlay_version() {
    let root = "includes=['sites/one.toml']\n[[principals]]\nid='owner:one'\nkind='person'\n";
    let site = "[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n";
    let query: OriginQuery = serde_json::from_value(json!({
        "kind":"field",
        "query":{"kind":"site","identity":"site:one","field":["deployable"]}
    }))
    .expect("site field query");
    let mut session = Session::new_with_workspace_documents(
        source("root", "gazette.toml", root),
        [source("site", "sites/one.toml", site)],
        [],
        [],
        Limits::default(),
    )
    .expect("modular selection");
    let baseline = session.origin(1, &query).expect("document origin");
    assert_eq!(baseline.document, "site");
    assert_eq!(baseline.path.as_str(), "sites/one.toml");
    assert_eq!(baseline.version, None);
    assert_eq!(baseline.value.span().slice(site), Some("false"));

    let changed = site.replace("false", "true");
    assert!(
        session
            .open("site", 9, changed.clone())
            .expect("site overlay")
    );
    let current = session.origin(2, &query).expect("current document origin");
    assert_eq!(current.document, "site");
    assert_eq!(current.version, Some(9));
    assert_eq!(current.value.span().slice(&changed), Some("true"));
    assert_eq!(
        session
            .origin(1, &query)
            .expect_err("stale result rejected"),
        OriginError::Generation
    );
}
