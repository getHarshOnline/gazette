//! Test-only fresh journal and independent SDK release-history observations.

use gazette_sdk::store;
use serde_json::{Value, json};
use std::{io::Read, path::Path};

fn read() -> store::ReadOptions {
    store::ReadOptions {
        allow_coordination_files: true,
        ..Default::default()
    }
}

fn main() {
    let mut bytes = Vec::new();
    std::io::stdin()
        .lock()
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .expect("bounded fixture input");
    assert!(bytes.len() <= 1024 * 1024);
    let request: Value = serde_json::from_slice(&bytes).expect("fixture request");
    let result = match request["operation"].as_str().expect("fixture operation") {
        "initialize" => serde_json::to_value(
            store::initialize(
                Path::new(
                    request["journal"]
                        .as_str()
                        .expect("private fixture journal"),
                ),
                store::InitializeOptions {
                    allow_create: true,
                    session: store::SessionOptions {
                        allow_local_reservation: true,
                        read: read(),
                    },
                    ..Default::default()
                },
            )
            .expect("native initialization"),
        )
        .expect("native receipt"),
        "inspect" => {
            let database = Path::new(request["database"].as_str().expect("fixture database"));
            let record_id = request["record_id"].as_str().expect("exact record");
            let header: store::JournalHeader =
                serde_json::from_value(request["expected_header"].clone()).expect("exact header");
            json!({
                "authorization": store::inspect_release_authorization(database, record_id, Some(&header), read()).expect("native authorization history"),
                "leases": store::inspect_release_leases(database, record_id, Some(&header), read()).expect("native lease history"),
                "consumption": store::inspect_release_consumption(database, record_id, Some(&header), read()).expect("native consumption history"),
                "attempts": store::inspect_release_attempts(database, record_id, Some(&header), read()).expect("native attempt history"),
                "reconciliations": store::inspect_release_reconciliations(database, record_id, Some(&header), read()).expect("native reconciliation history")
            })
        }
        _ => panic!("unsupported fixture operation"),
    };
    serde_json::to_writer(std::io::stdout().lock(), &result).expect("complete native response");
}
