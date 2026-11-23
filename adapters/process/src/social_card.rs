//! Fixed retained helper sources for explicit social-card browser acquisition.

/// Exact helper entrypoint and module bytes, staged with the selected `payload/` assets.
///
/// Invoke `social-card.mjs` through the retained process port with an explicitly pinned Node
/// executable. The JSON request independently pins Chrome; this data selects neither program.
#[must_use]
pub fn resources() -> &'static [(&'static str, &'static [u8])] {
    &[
        ("social-card.mjs", include_bytes!("../social-card.mjs")),
        (
            "social_card/contract.mjs",
            include_bytes!("../social_card/contract.mjs"),
        ),
        (
            "social_card/assets.mjs",
            include_bytes!("../social_card/assets.mjs"),
        ),
        (
            "social_card/cdp.mjs",
            include_bytes!("../social_card/cdp.mjs"),
        ),
        (
            "social_card/document.mjs",
            include_bytes!("../social_card/document.mjs"),
        ),
        (
            "social_card/launch.mjs",
            include_bytes!("../social_card/launch.mjs"),
        ),
    ]
}
