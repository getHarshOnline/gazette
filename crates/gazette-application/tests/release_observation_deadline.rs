//! Aggregate release-observation deadline behavior over a complete target set.

use std::{sync::Mutex, thread, time::Duration};

use gazette_application::release;
use gazette_compiler::release::{self as release_contract, Limits};
use gazette_core::release::{
    RefAdvertisement, RemoteReadError, RemoteReadOptions, Target, TargetReadback,
};
use gazette_ports::release::RemoteReader;
use serde_json::Value;

struct SlowFirstReader {
    calls: Mutex<Vec<Duration>>,
}

impl RemoteReader for SlowFirstReader {
    fn observe_remote(
        &self,
        _remote: &str,
        _branch: &str,
        options: RemoteReadOptions,
    ) -> Result<RefAdvertisement, RemoteReadError> {
        self.calls.lock().expect("call log").push(options.timeout);
        thread::sleep(Duration::from_millis(20));
        Ok(RefAdvertisement::NotAdvertised)
    }
}

fn targets() -> Vec<Target> {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../gazette-compiler/tests/release_authorization/authorization.json"
    ))
    .expect("authorization fixture");
    let template: Target =
        serde_json::from_value(fixture["ordered_targets"][0].clone()).expect("target fixture");
    (0..3)
        .map(|ordinal| {
            let mut target = template.clone();
            target.ordinal = ordinal;
            target.domain = format!("{ordinal}.deadline.example");
            target.remote = format!("https://example.invalid/deadline-{ordinal}.git");
            target
        })
        .collect()
}

#[test]
fn one_deadline_bounds_the_complete_sequential_target_set() {
    let targets = release_contract::check_targets(&targets(), Limits::default())
        .expect("complete bounded target set");
    let reader = SlowFirstReader {
        calls: Mutex::new(Vec::new()),
    };
    let report = release::observe_targets(
        &reader,
        &targets,
        RemoteReadOptions {
            allow_read: true,
            timeout: Duration::from_millis(1),
            ..RemoteReadOptions::default()
        },
    )
    .expect("complete report retains deadline failures");

    assert_eq!(reader.calls.lock().expect("call log").len(), 1);
    assert!(matches!(
        report.targets[0].readback,
        TargetReadback::Observed { .. }
    ));
    for row in &report.targets[1..] {
        assert!(matches!(
            row.readback,
            TargetReadback::Unavailable { error }
                if error.failure == gazette_core::release::RemoteReadFailure::Timeout
        ));
    }
}
