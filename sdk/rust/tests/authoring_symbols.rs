//! Search reaches real selected-source analysis and preserves exact current preimages.

use gazette_sdk::{
    authoring::{
        NavigationError, SymbolKind,
        session::{Session, Source},
    },
    source::Sha256Digest,
};

const WORKSPACE: &str = "[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n";

fn source(id: &str, path: &str, text: &str) -> Source {
    Source {
        id: id.into(),
        path: path.parse().expect("logical path"),
        text: text.into(),
    }
}

fn selected(reverse: bool) -> Session {
    let mut documents = vec![
        source("last-client-id", "a.md", "# Café 🦀\r\n"),
        source("first-client-id", "z.md", "# Café\n"),
    ];
    if reverse {
        documents.reverse();
    }
    Session::new(
        source(
            "file:///never-opened/workspace",
            "workspace.toml",
            WORKSPACE,
        ),
        documents,
        Default::default(),
    )
    .expect("selected baseline")
}

#[test]
fn complete_search_uses_native_names_logical_order_and_exact_source_coordinates() {
    let state = selected(false);
    let result = state
        .search_symbols("CAFÉ", 1 << 18)
        .expect("Unicode case-insensitive query");
    assert_eq!(result.generation, 1);
    assert_eq!(
        result.symbols.iter().map(|s| s.name).collect::<Vec<_>>(),
        ["Café 🦀", "Café"]
    );
    assert_eq!(result.symbols[0].document, "last-client-id");
    assert_eq!(result.symbols[0].source.path.as_str(), "a.md");
    for symbol in &result.symbols {
        let current = &state.files()[symbol.document];
        assert_eq!(symbol.kind, SymbolKind::Heading);
        assert_eq!(symbol.version, None);
        assert_eq!(
            symbol.source.source_sha256,
            Sha256Digest::of(current.text().as_bytes())
        );
        assert!(
            symbol
                .source
                .location
                .span
                .slice(current.text())
                .expect("native construct")
                .contains(symbol.name)
        );
    }
    assert_eq!(
        serde_json::to_value(&result).expect("result"),
        serde_json::to_value(
            selected(true)
                .search_symbols("café", 1 << 18)
                .expect("reordered input")
        )
        .expect("result")
    );
    let declarations = state
        .search_symbols("owner:", 1 << 18)
        .expect("declarations");
    assert_eq!(
        declarations.symbols.len(),
        1,
        "references are not duplicate declaration symbols"
    );
    assert_eq!(declarations.symbols[0].name, "owner:one");
    assert_eq!(declarations.symbols[0].kind, SymbolKind::Declaration);
    assert_eq!(
        state
            .search_symbols("", 1 << 18)
            .expect("complete set")
            .symbols
            .len(),
        4
    );
    assert!(
        state
            .search_symbols("not present", 1 << 18)
            .expect("empty result")
            .symbols
            .is_empty()
    );
}

#[test]
fn current_overlays_replace_matches_and_failed_analysis_cannot_return_a_partial_set() {
    let mut state = selected(false);
    let changed = "😀 prefix\r\n\r\n# Changed\r\n";
    state
        .open("last-client-id", 7, changed.into())
        .expect("overlay");
    let result = state
        .search_symbols("changed", 1 << 18)
        .expect("new source");
    assert_eq!(result.generation, 2);
    assert_eq!(result.symbols.len(), 1);
    assert_eq!(result.symbols[0].version, Some(7));
    assert_eq!(
        result.symbols[0].source.location.span.start,
        changed.find('#').expect("heading")
    );
    assert_eq!(
        state
            .search_symbols("café", 1 << 18)
            .expect("remaining source")
            .symbols
            .len(),
        1
    );
    state
        .change("last-client-id", 8, "+++\n[broken\n+++\n# Invalid\n".into())
        .expect("invalid current source");
    assert_eq!(
        state
            .search_symbols("café", 1 << 18)
            .expect_err("no partial current search"),
        NavigationError::Unavailable
    );
    state.close("last-client-id").expect("restore baseline");
    assert_eq!(
        state
            .search_symbols("café", 1 << 18)
            .expect("restored")
            .symbols
            .len(),
        2
    );
}

#[test]
fn search_bounds_refuse_complete_results_instead_of_truncating_matches() {
    let state = selected(false);
    assert_eq!(
        state.search_symbols("", 1).expect_err("bound"),
        NavigationError::Limit
    );
    assert_eq!(
        state
            .search_symbols("long query", 3)
            .expect_err("query bound"),
        NavigationError::Limit
    );
    assert_eq!(
        state
            .search_symbols("", 1 << 18)
            .expect("unchanged")
            .generation,
        1
    );
}
