//! Framed origin operation retains native bytes and adds checked UTF-16 coordinates only.
use super::{footnotes::response, *};
use gazette_sdk::{
    authoring::session::{Session, Source},
    source::Sha256Digest,
};

const BASE: &str = "principals=[{id='owner:one',kind='person',name='é🦀'}]\r\nsites=[{id='site:one',publisher='owner:one',deployable=false}]\r\n[composition.capabilities]\r\n'publication.search/v1'={state='disabled'}\r\n[composition.resources]\r\n'module:search'={state='disabled'}\r\n";

fn query() -> Value {
    json!({"kind":"composition","query":{"site":"site:one","field":{"kind":"capability","capability":"publication.search/v1"},"path":["state"]}})
}
fn resource_query() -> Value {
    json!({"kind":"composition","query":{"site":"site:one","field":{"kind":"resource","resource":"module:search"},"path":["state"]}})
}
fn origin(id: i32, generation: u64, query: Value) -> Value {
    request(
        id,
        "gazette/origin",
        json!({"generation":generation,"query":query}),
    )
}
fn point(text: &str, offset: usize) -> Value {
    let before = &text[..offset];
    json!({"line":before.matches('\n').count(),"character":before.rsplit('\n').next().expect("line").encode_utf16().count()})
}

#[test]
fn origins_are_discoverable_current_source_bound_and_restore_after_invalid_overlays() {
    let mut init = selected(&[("untitled:bad", "bad.md", "+++\n[broken\n+++\n")]);
    init["params"]["initializationOptions"]["workspace"]["text"] = json!(BASE);
    let changed = BASE.replace(
        "deployable=false}",
        "deployable=false,composition={capabilities={'publication.search/v1'={state=\"disabled\"}}}}",
    );
    let field = json!({"kind":"field","query":{"kind":"principal","identity":"owner:one","field":["name"]}});
    let (result, messages) = selected_session(
        init,
        [
            origin(2, 1, query()),
            origin(9, 1, resource_query()),
            origin(3, 1, field.clone()),
            open(URI, 5, &changed),
            origin(4, 1, query()),
            origin(5, 2, query()),
            origin(10, 2, resource_query()),
            origin(
                11,
                2,
                json!({"kind":"composition","query":{"site":"site:one","field":{"kind":"module","module":"module:search"},"path":["state"]}}),
            ),
            change(URI, 6, "[broken\n"),
            origin(6, 3, query()),
            close(URI),
            origin(7, 4, query()),
            request(8, "gazette/origin", json!({"query":query()})),
        ],
        Limits::default(),
    );
    result.expect("real framed protocol");
    assert_eq!(
        response(&messages, 1)["result"]["capabilities"]["experimental"]["gazette"]["origin"]["method"],
        "gazette/origin"
    );
    for (id, text, generation, q) in [
        (2, BASE, 1, query()),
        (9, BASE, 1, resource_query()),
        (3, BASE, 1, field),
        (5, changed.as_str(), 2, query()),
        (10, changed.as_str(), 2, resource_query()),
        (7, BASE, 4, query()),
    ] {
        let state = Session::new(
            Source {
                id: URI.into(),
                path: "workspace.toml".parse().expect("path"),
                text: text.into(),
            },
            [],
            Default::default(),
        )
        .expect("native selected bytes");
        let q = serde_json::from_value(q).expect("native query");
        let native = state.origin(1, &q).expect("direct native result");
        let result = &response(&messages, id)["result"];
        assert_eq!(result["native"]["value"], json!(native.value));
        assert_eq!(result["native"]["generation"], generation);
        assert_eq!(
            result["native"]["value"]["origin"]["source_sha256"],
            json!(Sha256Digest::of(text.as_bytes()))
        );
        let span = native.value.span();
        assert_eq!(
            result["range"],
            json!({"start":point(text,span.start),"end":point(text,span.end)})
        );
    }
    assert_eq!(
        response(&messages, 4)["error"]["message"],
        "authoring_stale_generation"
    );
    assert_eq!(
        response(&messages, 6)["error"]["message"],
        "authoring_origin_workspace"
    );
    assert_eq!(response(&messages, 8)["error"]["code"], -32602);
    assert_eq!(response(&messages, 11)["error"]["code"], -32602);
    assert_eq!(
        response(&messages, 2)["result"]["native"]["value"]["origin"]["selection"]["field"]["kind"],
        "capability"
    );
    assert_eq!(
        response(&messages, 9)["result"]["native"]["value"]["origin"]["selection"]["field"]["kind"],
        "resource"
    );
}
