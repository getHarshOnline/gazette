//! Production worker admission cannot create a pending computation queue.

use super::*;

#[test]
fn exact_worker_ceiling_holds_until_permit_is_released() {
    let server = Server::new(Limits {
        workers: 2,
        ..Limits::default()
    });
    let one = server.reserve().expect("first worker");
    let two = server.reserve().expect("second worker");
    assert!(
        server.reserve().is_err(),
        "overload rejects without enqueueing"
    );
    drop(one);
    let replacement = server.reserve().expect("released slot reused");
    assert!(server.reserve().is_err());
    drop((two, replacement));
    assert_eq!(server.permits.available_permits(), 2);
}

#[test]
fn dispatch_classification_preserves_existing_error_data_and_defaults_safe() {
    let refused = not_dispatched(ErrorData::invalid_params(
        "refused",
        Some(serde_json::json!({"code":"existing","retryable":true})),
    ));
    assert_eq!(refused.data.as_ref().expect("data")["code"], "existing");
    assert_eq!(
        refused.data.as_ref().expect("data")[DISPATCH_DISPOSITION],
        "not-dispatched"
    );
    let uncertain = outcome_unknown(ErrorData::internal_error("uncertain", None));
    assert_eq!(
        uncertain.data.as_ref().expect("data")[DISPATCH_DISPOSITION],
        "outcome-unknown"
    );
}
