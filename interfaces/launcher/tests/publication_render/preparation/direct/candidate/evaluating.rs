//! Synthetic semantic evaluation of delivered output, never real publication approval.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use gazette_sdk::html;

use super::*;

const SELLER: &str = "urn:gazette-fixture:fictional-seller";
const EXPECTED_PRICE: u64 = 100;

fn offers(bytes: &[u8]) -> Vec<Value> {
    let mut offers = Vec::new();
    html::inspect(bytes, Default::default(), |kind, text| {
        if kind == html::TextKind::JsonScript {
            let value: Value = serde_json::from_str(text)?;
            if value["kind"] == "fictional-offer" {
                offers.push(value);
            }
        }
        Ok::<_, serde_json::Error>(())
    })
    .expect("actual HTML decoding and JSON grammar, not a substring policy");
    offers
}

#[test]
fn checker_child() {
    let Ok(path) = std::env::var(LOG) else {
        return;
    };
    let mut bytes = Vec::new();
    std::io::stdin()
        .read_to_end(&mut bytes)
        .expect("exact native checker input");
    let request: Value = serde_json::from_slice(&bytes).expect("native policy envelope");
    let mut log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .expect("disposable child observations");
    log.write_all(&bytes).expect("exact invocation bytes");
    log.write_all(b"\n").expect("invocation delimiter");
    log.sync_all().expect("retain observation before exit");

    let accepted = match request["format"].as_str() {
        Some("gazette-candidate-policy/1") => {
            let decisions = request["response"]["decisions"]
                .as_array()
                .expect("matched occurrence decisions");
            !decisions.is_empty()
                && decisions
                    .iter()
                    .all(|decision| decision["outcome"] == "allow")
        }
        Some("gazette-output-policy/1") => {
            assert_eq!(request["disclosure"]["findings"], json!([]));
            let mut observed = Vec::new();
            for (path, encoded) in request["files_base64"].as_object().expect("complete files") {
                if request["media_types"][path] == "html" {
                    observed.extend(offers(
                        &STANDARD
                            .decode(encoded.as_str().expect("base64 payload"))
                            .expect("decoded exact output bytes"),
                    ));
                }
            }
            // This one fixed fictional policy is identical for clean and rejected runs.
            // No environment-selected exit or candidate approval substitutes for evaluation.
            !observed.is_empty()
                && observed.iter().all(|offer| {
                    offer["seller"] == SELLER
                        && offer["currency"] == "USD"
                        && offer["price"].as_u64() == Some(EXPECTED_PRICE)
                })
        }
        _ => panic!("unexpected native policy envelope"),
    };
    std::process::exit(if accepted { 0 } else { 29 });
}

#[test]
fn theme_only_semantic_offer_cannot_bypass_output_policy_with_allow_and_clean_scan() {
    let pin = executable_pin();
    for price in [EXPECTED_PRICE, EXPECTED_PRICE + 1] {
        let template = format!(
            "<h1>{{{{ content.title }}}}</h1>{{{{ body_html | safe }}}}<script type=\"application/json\">{{\"kind\":\"fictional-offer\",\"seller\":\"{SELLER}\",\"currency\":\"USD\",\"price\":{price}}}</script>"
        );
        let fixture = published_fixture(&[("theme/templates/page.html", template.as_bytes())]);
        let repository =
            git2::Repository::open(fixture.directory.path()).expect("original sources");
        let source_status = || {
            repository
                .statuses(None)
                .expect("source frontier")
                .iter()
                .map(|entry| (entry.path_bytes().to_vec(), entry.status().bits()))
                .collect::<Vec<_>>()
        };
        let original_status = source_status();
        let root = private_directory();
        let journal = initialize(root.path());
        let parent = private_directory();
        let log = parent.path().join("evaluated.jsonl");
        let config = pinned_configuration(
            &configuration(&fixture, &log, "none").replace(
                "preparation::direct::candidate::checker_child",
                "preparation::direct::candidate::evaluating::checker_child",
            ),
            pin,
        );
        let request = reviewed_request(&config, root.path(), journal.header, parent.path());
        let mut arguments = ARGUMENTS.to_vec();
        arguments.push("--allow-policy-execution");
        let (output, report) = result(&request, &arguments);
        let accepted = price == EXPECTED_PRICE;
        assert_eq!(output.status.success(), accepted, "{output:?}; {report}");
        let calls = events(&log);
        assert_eq!(calls.len(), if accepted { 4 } else { 3 }, "{report}");
        for (call, site) in calls[..2].iter().zip(["site:one", "site:other"]) {
            assert_eq!(call["format"], "gazette-candidate-policy/1");
            assert_eq!(call["review"]["request"]["site"], site);
            let surfaces = call["review"]["request"]["surfaces"]
                .as_array()
                .expect("actual reviewed surfaces");
            assert!(!surfaces.is_empty());
            assert!(
                !serde_json::to_string(surfaces)
                    .expect("reviewed facts")
                    .contains(SELLER),
                "the fictional offer exists only in Theme output, not reviewed page facts"
            );
            let decisions = call["response"]["decisions"].as_array().expect("all uses");
            assert!(!decisions.is_empty());
            assert!(
                decisions
                    .iter()
                    .all(|decision| decision["outcome"] == "allow")
            );
            assert_eq!(
                call["response"]["response_json"],
                request["review_responses"][site]["response_json"]
            );
        }
        for call in &calls[2..] {
            assert_eq!(call["format"], "gazette-output-policy/1");
            assert_eq!(call["disclosure"]["findings"], json!([]));
            let site = call["candidate_check"]["review"]["request"]["site"]
                .as_str()
                .expect("output site's bound candidate");
            let short = site.strip_prefix("site:").expect("fixture key");
            let record = store::inspect_preparation(&journal.database, id(short), None, read())
                .expect("independent actual journal inspection")
                .record
                .expect("retained intent");
            assert!(record.retention.is_some());
            let evidence: Value =
                serde_json::from_str(&record.intent.evidence).expect("private intent");
            assert_eq!(evidence["candidate_check"], call["candidate_check"]);
            let mut offer_count = 0;
            for (path, encoded) in call["files_base64"].as_object().expect("complete output") {
                let delivered = STANDARD
                    .decode(encoded.as_str().expect("payload"))
                    .expect("base64");
                let retained = fs::read(record.intent.address.path().join(path))
                    .expect("actual retained bytes");
                assert_eq!(
                    retained, delivered,
                    "checker and stage must receive identical {path}"
                );
                if call["media_types"][path] == "html" {
                    for offer in offers(&retained) {
                        assert_eq!(offer["price"], price);
                        offer_count += 1;
                    }
                }
            }
            assert!(offer_count > 0, "cannot pass by observing no offer");
            if accepted {
                let check: Value =
                    serde_json::from_str(&record.check.as_ref().expect("actual check").evidence)
                        .expect("check evidence");
                assert_eq!(
                    check["policy"]["evidence"]["status"],
                    "output-policy-check-passed-unverified"
                );
                assert!(
                    record
                        .worktree
                        .as_ref()
                        .expect("Git preparation")
                        .completion
                        .is_some()
                );
            } else {
                assert!(
                    record.check.is_none(),
                    "rejection is not successful check history"
                );
                assert!(
                    record.worktree.is_none(),
                    "no subsequent Git intent or completion"
                );
                assert!(!record.intent.address.path().join(".git").exists());
                let captured = source::capture_pinned(
                    &record.intent.address.path(),
                    record.intent.output,
                    Default::default(),
                )
                .expect("rejected complete stage remains byte-exact, not scrubbed");
                assert_eq!(captured.inventory().identity(), record.intent.output);
                assert_eq!(json!(record), report["failure"]["prior_receipt"]["record"]);
            }
        }
        if accepted {
            assert_eq!(
                report["status"],
                "local-preparations-complete-not-certified"
            );
            assert_eq!(report["sites"].as_array().expect("complete set").len(), 2);
        } else {
            assert_eq!(report["status"], "incomplete");
            assert_eq!(report["completed"], json!([]));
            assert_eq!(report["failure"]["site"], "site:other");
            assert_eq!(report["failure"]["phase"]["kind"], "policy");
            assert_eq!(
                report["failure"]["phase"]["evidence"]["code"],
                "policy_rejected"
            );
            assert_eq!(report["failure"]["phase"]["evidence"]["exit"]["code"], 29);
            let raw = fs::read_to_string(&log).expect("exact delivered requests");
            assert_eq!(
                report["failure"]["phase"]["evidence"]["invocation"]["request_sha256"],
                json!(source::Sha256Digest::of(
                    raw.lines()
                        .nth(2)
                        .expect("actual rejected request")
                        .as_bytes()
                ))
            );
            assert_eq!(
                report["candidate_checks"]
                    .as_object()
                    .expect("successful candidate calls")
                    .len(),
                2
            );
            assert!(
                store::inspect_preparation(&journal.database, id("one"), None, read())
                    .expect("unstarted second site")
                    .record
                    .is_none()
            );
            assert!(!parent.path().join("one").exists());
        }
        assert_eq!(
            fs::read(fixture.directory.path().join("theme/templates/page.html"))
                .expect("original Theme bytes"),
            template.as_bytes()
        );
        assert_eq!(
            repository.head().expect("source HEAD").target(),
            Some(fixture.commit)
        );
        assert_eq!(source_status(), original_status);
    }
}
