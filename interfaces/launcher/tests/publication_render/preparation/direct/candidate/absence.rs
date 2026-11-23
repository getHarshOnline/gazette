//! Unavailable native acquisition reaches the candidate/output/preparation path.

use base64::{Engine as _, engine::general_purpose::STANDARD};

use super::*;

#[test]
fn unavailable_native_acquisition_is_bound_through_policy_and_complete_preparation() {
    let manifest = "package='theme:one'\nnamespace='fixture'\ntemplates='templates'\nbrowser_theme_colors={light='#ffffff',dark='#000000'}\nrender={page='page.html',alias='alias.html'}\n";
    let plugin =
        "package='fixture-plugin'\n[providers.search]\nimplements=['publication.search/v1']\n";
    let template = "<h1>{{ content.title }}</h1>{{ body_html | safe }}{% for name, capability in presentation.capabilities %}<p>{{ capability.state }}:{{ capability.reason }}</p>{% endfor %}";
    let fixture = published_fixture(&[
        ("theme/theme.toml", manifest.as_bytes()),
        ("theme/plugin.toml", plugin.as_bytes()),
        ("theme/templates/page.html", template.as_bytes()),
        (
            "theme/main.ts",
            b"throw new Error('not compiled without recipe');",
        ),
        (
            "theme/assets/search.js",
            b"stale unselected output must never be copied",
        ),
    ]);
    let root = private_directory();
    let journal = initialize(root.path());
    let parent = private_directory();
    let log = parent.path().join("absence-checks.jsonl");
    let config = pinned_configuration(&configuration(&fixture, &log, "none"), executable_pin())
        .replacen("[[principals]]", "[composition.plugins]\n'fixture-plugin'={state='enabled'}\n[composition.capabilities]\n'publication.search/v1'={state='enabled',provider={kind='plugin',package='fixture-plugin',provider='search'},requirement='optional',allow_absence='unsupported'}\n[[principals]]", 1)
        + "[[plugin_packages]]\npackage='fixture-plugin'\nsource='source:theme'\nmanifest='plugin.toml'\n";
    let request = reviewed_request(&config, root.path(), journal.header, parent.path());
    let mut arguments = ARGUMENTS.to_vec();
    arguments.push("--allow-policy-execution");
    let (output, report) = result(&request, &arguments);
    assert!(output.status.success(), "{output:?}; {report}");
    assert_eq!(
        report["status"],
        "local-preparations-complete-not-certified"
    );
    let calls = events(&log);
    assert_eq!(
        calls.len(),
        4,
        "two candidate checks precede two output checks"
    );
    let unavailable = json!({"state":"unavailable","reason":"unsupported"});
    for (candidate, site) in calls[..2].iter().zip(["site:one", "site:other"]) {
        assert_eq!(candidate["format"], "gazette-candidate-policy/1");
        assert_eq!(candidate["review"]["request"]["site"], site);
        assert_eq!(
            candidate["response"]["response_json"],
            request["review_responses"][site]["response_json"]
        );
        assert!(
            serde_json::to_string(&candidate["review"]["request"]["surfaces"])
                .expect("retained selected composition")
                .contains("unsupported"),
            "candidate review must contain the actual unavailability permission"
        );
    }
    for site in report["sites"]
        .as_array()
        .expect("complete preparation set")
    {
        let site_id = site["site"].as_str().expect("exact site");
        let short = site_id.strip_prefix("site:").expect("fixture key");
        let record = store::inspect_preparation(&journal.database, id(short), None, read())
            .expect("independent durable inspection")
            .record
            .expect("complete original record");
        assert_eq!(json!(record), site["preparation"]["record"]);
        let evidence: Value = serde_json::from_str(&record.intent.evidence)
            .expect("original private render evidence");
        let capability = &evidence["rendering"]["capabilities"]["publication.search/v1"];
        assert_eq!(capability["state"], "unavailable");
        assert_eq!(capability["reason"], "unsupported");
        assert_eq!(capability["runtime"]["kind"], "plugin");
        assert_eq!(capability["runtime"]["provider"], "search");
        assert_eq!(
            capability["runtime"]["package"]["package"],
            "fixture-plugin"
        );
        for digest in ["source", "manifest", "tree"] {
            assert_eq!(
                capability["runtime"]["package"][digest]
                    .as_str()
                    .expect("exact plugin package digest")
                    .len(),
                64
            );
        }
        let capability = capability.as_object().expect("typed capability evidence");
        for resource_field in ["output", "url", "resource"] {
            assert!(
                !capability.contains_key(resource_field),
                "semantic capability evidence must not carry resource field {resource_field}"
            );
        }
        let call = calls[2..]
            .iter()
            .find(|call| call["candidate_check"]["review"]["request"]["site"] == site_id)
            .expect("own immutable output check");
        assert_eq!(call["format"], "gazette-output-policy/1");
        assert_eq!(call["disclosure"]["findings"], json!([]));
        assert_eq!(call["candidate_check"], evidence["candidate_check"]);
        assert!(call["files_base64"].get("assets/search.js").is_none());
        for (path, encoded) in call["files_base64"]
            .as_object()
            .expect("exact delivered files")
        {
            let bytes = STANDARD
                .decode(encoded.as_str().expect("encoded bytes"))
                .expect("actual delivered output");
            assert_eq!(
                fs::read(record.intent.address.path().join(path)).expect("exact retained bytes"),
                bytes
            );
        }
        let model_bytes = fs::read(record.intent.address.path().join("model.json"))
            .expect("retained machine output");
        let model: Value = serde_json::from_slice(&model_bytes).expect("shared model");
        assert_eq!(model["capabilities"]["publication.search/v1"], unavailable);
        let pages = model["pages"].as_object().expect("complete local pages");
        assert!(!pages.is_empty());
        for page in pages.values() {
            assert_eq!(
                page["presentation"]["capabilities"]["publication.search/v1"],
                unavailable
            );
        }
        let path = if short == "other" {
            "elsewhere/index.html"
        } else {
            "guide/index.html"
        };
        assert!(
            String::from_utf8(
                fs::read(record.intent.address.path().join(path)).expect("real rendered page")
            )
            .expect("HTML")
            .contains("unavailable:unsupported")
        );
        assert!(
            !record
                .intent
                .address
                .path()
                .join("assets/search.js")
                .exists()
        );
        let completion = record
            .worktree
            .as_ref()
            .expect("native Git preparation")
            .completion
            .as_ref()
            .expect("verified completion");
        release_commit::inspect_journal_worktree(
            release_commit::JournalWorktreeInspectionRequest {
                database: &journal.database,
                preparation_id: id(short),
                expected_header: None,
                commit: &completion.report.commit,
                protected_roots: &[fixture.directory.path().to_owned()],
            },
            release_commit::JournalWorktreeInspectionOptions { read: read(), ..Default::default() },
        ).expect("independent journal/Git/complete-output readback, not a plain source capture of Git metadata");
        let checked: Value = serde_json::from_str(
            &record
                .check
                .as_ref()
                .expect("successful output check")
                .evidence,
        )
        .expect("durable check evidence");
        assert_eq!(
            checked["preparation_evidence_sha256"],
            json!(record.intent.evidence_sha256)
        );
        assert_eq!(
            checked["policy"]["evidence"]["status"],
            "output-policy-check-passed-unverified"
        );
        assert!(
            record
                .worktree
                .as_ref()
                .expect("native Git preparation")
                .completion
                .is_some()
        );
    }
    assert_eq!(
        fs::read(fixture.directory.path().join("theme/assets/search.js"))
            .expect("unselected native bytes preserved"),
        b"stale unselected output must never be copied"
    );
}
