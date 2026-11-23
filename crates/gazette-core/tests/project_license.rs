//! Project software licences stay exact, optional and distinct from content permissions.

use gazette_core::publication::{Project, ProjectLicense};
use serde_json::json;

#[test]
fn strict_spdx_expressions_round_trip_without_canonicalizing_owner_bytes() {
    for value in [
        "MIT",
        "AGPL-3.0-or-later",
        "MIT OR Apache-2.0",
        "Apache-2.0 WITH LLVM-exception",
        "(MIT OR Apache-2.0) AND BSD-3-Clause",
        "LicenseRef-Owner-Proprietary",
    ] {
        let parsed: ProjectLicense = value.parse().expect("strict SPDX expression");
        assert_eq!(parsed.as_str(), value);
        assert_eq!(serde_json::to_value(&parsed).expect("JSON"), value);
        assert_eq!(
            serde_json::from_value::<ProjectLicense>(json!(value)).expect("decode"),
            parsed
        );
    }
}

#[test]
fn malformed_lax_deprecated_and_ambiguous_values_fail_closed() {
    for value in [
        "",
        " MIT",
        "MIT ",
        "mit",
        "MIT/Apache-2.0",
        "GPL-2.0+",
        "Apache2",
        "MIT OR",
        "MIT Apache-2.0",
        "Unknown-License",
        "https://spdx.org/licenses/MIT.html",
    ] {
        assert!(
            value.parse::<ProjectLicense>().is_err(),
            "accepted {value:?}"
        );
        assert!(
            serde_json::from_value::<ProjectLicense>(json!(value)).is_err(),
            "decoded {value:?}"
        );
    }
}

#[test]
fn omission_stays_absent_and_never_uses_repository_or_content_fields() {
    let project: Project = serde_json::from_value(json!({
        "id":"project:one",
        "repository":"https://example.test/owner/repository",
        "lifecycle":"active",
        "visibility":"public",
        "availability":"available"
    }))
    .expect("project without licence");
    assert!(project.license.is_none());
    assert!(
        serde_json::to_value(project)
            .expect("project JSON")
            .get("license")
            .is_none()
    );
}
