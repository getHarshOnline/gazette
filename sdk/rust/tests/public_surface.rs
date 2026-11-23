//! Compile-time consumer proof for Gazette-owned native SDK contracts.

#![cfg(feature = "native-local")]

use gazette_sdk::{delivery, github, preview, source};

#[test]
fn native_handles_and_http_selection_are_gazette_owned_and_composable() {
    fn require_reservation<T: preview::PreviewReservation>() {}
    fn require_session<T: preview::PreviewSession>() {}
    fn same_http_selection(selection: delivery::Curl) -> github::Curl {
        selection
    }

    require_reservation::<preview::Reservation>();
    require_session::<preview::Session<'static>>();

    let _: fn(&source::StagedTree) -> &std::path::Path = source::StagedTree::path;
    let _: fn(&source::StagedTree) -> &source::Inventory = source::StagedTree::inventory;
    let _: fn(source::StagedTree) -> Result<source::RetainedStage, source::StageError> =
        source::StagedTree::retain;
    let _: fn(source::StagedTree) -> Result<(), source::StageError> = source::StagedTree::close;
    let _: fn(delivery::Curl) -> github::Curl = same_http_selection;
}
