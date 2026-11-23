//! Retained production browser SDK bytes shared by native qualification fixtures.

/// Production search entry point and its complete local module closure.
pub(crate) const SEARCH: &[(&str, &[u8])] = &[
    (
        "search/index.mjs",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../sdk/js/search/index.mjs"
        )),
    ),
    (
        "search/browse.mjs",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../sdk/js/search/browse.mjs"
        )),
    ),
    (
        "search/corpus.mjs",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../sdk/js/search/corpus.mjs"
        )),
    ),
    (
        "search/load.mjs",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../sdk/js/search/load.mjs"
        )),
    ),
    (
        "search/vendor/elasticlunr.mjs",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../sdk/js/search/vendor/elasticlunr.mjs"
        )),
    ),
];

/// Search plus browser-tool adapters, using the same retained search closure.
pub(crate) fn webmcp() -> Vec<(&'static str, &'static [u8])> {
    let mut files = SEARCH.to_vec();
    files.extend([
        (
            "webmcp/index.mjs",
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../sdk/js/webmcp/index.mjs"
            ))
            .as_slice(),
        ),
        (
            "webmcp/data.mjs",
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../sdk/js/webmcp/data.mjs"
            ))
            .as_slice(),
        ),
    ]);
    files
}

/// One resource input list derived from the exact files installed by the fixture.
pub(crate) fn inputs(files: &[(&str, &[u8])]) -> String {
    serde_json::to_string(
        &std::iter::once("controls.mjs")
            .chain(files.iter().map(|(path, _)| *path))
            .collect::<Vec<_>>(),
    )
    .unwrap()
}
