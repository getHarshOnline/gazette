//! Disposable native checkers receive candidate review, never publisher authority.

#[path = "policy/output.rs"]
mod output;

use std::{collections::BTreeMap, io::Read, path::Path};

use gazette_sdk::{authored, process, publication::content::Disposition};

use super::{Fixture, PAGE, acquire, build, fs, source, tempdir};

const RESULT: &str = "GAZETTE_POLICY_FIXTURE_RESULT";
const EXIT: &str = "GAZETTE_POLICY_FIXTURE_EXIT";

#[test]
fn checker_child() {
    let Ok(result) = std::env::var(RESULT) else {
        return;
    };
    let exit = std::env::var(EXIT)
        .expect("explicit child mode")
        .parse::<i32>()
        .expect("native fixture exit code");
    let mut input = Vec::new();
    std::io::stdin()
        .read_to_end(&mut input)
        .expect("complete checker input through EOF");
    fs::write(result, input).expect("explicit disposable checker observation");
    std::process::exit(exit);
}

fn fixture() -> Fixture {
    let published = PAGE.replace("lifecycle='draft'", "lifecycle='published'");
    Fixture::new(&[("one/page.md", published.as_bytes())])
}

pub(super) fn configured(
    fixture: &Fixture,
    result: &Path,
    exit: i32,
) -> (String, process::Command) {
    let command = process::Command {
        executable: None,
        program: std::env::current_exe()
            .expect("real native test executable")
            .to_str()
            .expect("fixture executable path")
            .to_owned(),
        arguments: vec!["--exact".into(), "policy::checker_child".into()],
        directory: result
            .parent()
            .expect("disposable result directory")
            .to_str()
            .expect("fixture directory")
            .to_owned(),
        environment: BTreeMap::from([
            (
                RESULT.into(),
                result.to_str().expect("fixture result path").into(),
            ),
            (EXIT.into(), exit.to_string()),
        ]),
    };
    // The selected command contains no expected request digest: configuration itself is bound.
    let configuration = format!(
        "policy_check={{program={:?},arguments=['--exact','policy::checker_child'],directory={:?},environment={{{RESULT}={:?},{EXIT}={:?}}}}}\n{}",
        command.program,
        command.directory,
        command.environment[RESULT],
        command.environment[EXIT],
        fixture.config()
    );
    (configuration, command)
}

fn selection(publication: &build::ResolvedPublication<'_, '_>) -> authored::Selection {
    authored::Selection {
        content: publication
            .authored()
            .content()
            .graph()
            .nodes()
            .map(|node| (node.id.clone(), Disposition::Include))
            .collect(),
        assets: BTreeMap::new(),
    }
}

fn permitted() -> process::Options {
    process::Options {
        allow_execute: true,
        ..Default::default()
    }
}

#[test]
fn configured_checker_receives_exact_frozen_candidate_and_cannot_promote_public_output() {
    let fixture = fixture();
    let observation = tempdir().expect("disposable checker result directory");
    let result = observation.path().join("received.json");
    let (configuration, command) = configured(&fixture, &result, 0);
    let inputs = acquire(&configuration);
    assert_eq!(
        inputs
            .configuration()
            .policy_command()
            .expect("bound checker"),
        Some(&command)
    );
    let parsed = inputs
        .parse_publication(Default::default(), &gazette_adapter_markdown::Comrak)
        .expect("captured publication syntax");
    let resolved = parsed
        .resolve(Default::default())
        .expect("native selected graph");
    let decisions = selection(&resolved);
    let selection_digest =
        source::Sha256Digest::of(&serde_json::to_vec(&decisions).expect("full selection"));
    let selected = resolved
        .into_selected(decisions)
        .expect("complete candidate");
    let review =
        build::review_selected(&selected, Default::default()).expect("same review projector");
    let expected = serde_json::to_vec(&serde_json::json!({
        "format": "gazette-candidate-policy/1",
        "review": review,
        "response": null,
    }))
    .expect("candidate envelope");
    let receipt = build::check_policy(&selected, permitted(), Default::default())
        .expect("real checker exited zero after complete input");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(
            &fs::read(&result).expect("native child captured stdin")
        )
        .expect("received envelope"),
        serde_json::from_slice::<serde_json::Value>(&expected).expect("expected envelope")
    );
    assert_eq!(
        receipt.review().request().selection_sha256(),
        Some(selection_digest)
    );
    assert_eq!(receipt.review().sha256(), review.sha256());
    assert_eq!(
        receipt.invocation().request_sha256,
        source::Sha256Digest::of(&fs::read(&result).expect("actual exact request bytes"))
    );
    assert_eq!(
        receipt.invocation().command_sha256,
        source::Sha256Digest::of(&serde_json::to_vec(&command).expect("exact command identity"))
    );
    let json = serde_json::to_value(&receipt).expect("private zero-exit evidence");
    assert_eq!(json["status"], "policy-check-passed-unverified");
    assert_eq!(json["review"], serde_json::json!(review));
    assert!(json["response"].is_null());

    fs::write(
        fixture.directory.path().join("one/page.md"),
        "mutated native input after capture",
    )
    .expect("disposable native edit");
    fs::write(&result, "observation must be replaced by the next child")
        .expect("invalidate prior disposable observation");
    let repeated = build::check_policy(&selected, permitted(), Default::default())
        .expect("checker receives retained input, not changed filesystem bytes");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(
            &fs::read(&result).expect("repeated native stdin capture")
        )
        .expect("repeated envelope"),
        serde_json::from_slice::<serde_json::Value>(&expected).expect("expected envelope")
    );
    assert_eq!(repeated.invocation(), receipt.invocation());

    let rendered = build::render_selected_publication(
        &selected,
        build::SiteRenderOptions {
            render_year: None,
            model_output: Some(&"model.json".parse().expect("machine output")),
            graph: None,
            limits: Default::default(),
        },
        Default::default(),
    )
    .expect("same private candidate rendering remains separate from policy invocation");
    let output = rendered.site().output();
    assert!(
        output
            .file(&"guide/index.html".parse().expect("HTML output"))
            .is_some()
    );
    assert!(
        output
            .file(&"model.json".parse().expect("machine output"))
            .is_some()
    );
    for entry in output.inventory().entries() {
        let Some(bytes) = output.file(&entry.path) else {
            continue;
        };
        let text = String::from_utf8_lossy(bytes);
        for private in [
            command.program.as_str(),
            command.directory.as_str(),
            RESULT,
            "policy::checker_child",
        ] {
            assert!(
                !text.contains(private),
                "{} leaked checker configuration",
                entry.path
            );
        }
    }
    assert_eq!(
        json,
        serde_json::json!({
            "status": "policy-check-passed-unverified",
            "invocation": receipt.invocation(),
            "review": review,
            "response": null,
        }),
        "receipt retains complete private evidence, not an admission/release token"
    );
}

#[test]
fn rejected_checker_preserves_exact_native_invocation_and_nonzero_exit() {
    let fixture = fixture();
    let observation = tempdir().expect("disposable checker result directory");
    let result = observation.path().join("rejected.json");
    let (configuration, command) = configured(&fixture, &result, 7);
    let inputs = acquire(&configuration);
    let parsed = inputs
        .parse_publication(Default::default(), &gazette_adapter_markdown::Comrak)
        .expect("captured syntax");
    let resolved = parsed.resolve(Default::default()).expect("native graph");
    let decisions = selection(&resolved);
    let selected = resolved
        .into_selected(decisions)
        .expect("complete candidate");
    let review = build::review_selected(&selected, Default::default()).expect("exact review");
    let error = build::check_policy(&selected, permitted(), Default::default())
        .expect_err("nonzero native exit is never policy acceptance");
    assert_eq!(error.code(), "policy_rejected");
    let build::PolicyCheckError::Rejected { invocation, exit } = error else {
        panic!("real completed checker must report its native nonzero exit");
    };
    assert_eq!(exit.code, Some(7));
    let received = fs::read(result).expect("rejected checker still received complete input");
    let envelope: serde_json::Value =
        serde_json::from_slice(&received).expect("candidate envelope");
    assert_eq!(envelope["format"], "gazette-candidate-policy/1");
    assert_eq!(envelope["review"], serde_json::json!(review));
    assert!(envelope["response"].is_null());
    assert_eq!(
        invocation.request_sha256,
        source::Sha256Digest::of(&received)
    );
    assert_eq!(
        invocation.command_sha256,
        source::Sha256Digest::of(&serde_json::to_vec(&command).expect("selected command"))
    );
}

#[test]
fn checker_configuration_permissions_and_complete_input_bounds_refuse_before_spawning() {
    let fixture = fixture();
    let observation = tempdir().expect("disposable checker result directory");
    let result = observation.path().join("must-not-exist.json");
    let (configuration, _) = configured(&fixture, &result, 0);
    let inputs = acquire(&configuration);
    let parsed = inputs
        .parse_publication(Default::default(), &gazette_adapter_markdown::Comrak)
        .expect("captured syntax");
    let resolved = parsed.resolve(Default::default()).expect("native graph");
    let decisions = selection(&resolved);
    let selected = resolved
        .into_selected(decisions)
        .expect("complete candidate");
    let error = build::check_policy(&selected, Default::default(), Default::default())
        .expect_err("configured execution still needs explicit permission");
    assert!(matches!(
        error,
        build::PolicyCheckError::Refused(process::Error {
            failure: process::Failure::Permission,
            cleanup: process::Cleanup::NotStarted,
            ..
        })
    ));
    assert!(!result.exists());
    let review = build::review_selected(&selected, Default::default()).expect("complete request");
    let exact_bytes = serde_json::to_vec(&serde_json::json!({
        "format": "gazette-candidate-policy/1",
        "review": review,
        "response": null,
    }))
    .expect("complete candidate envelope bytes")
    .len();
    let error = build::check_policy(
        &selected,
        process::Options {
            max_input_bytes: exact_bytes - 1,
            ..permitted()
        },
        Default::default(),
    )
    .expect_err("no prefix delivery to checker");
    assert!(matches!(
        error,
        build::PolicyCheckError::Encoding(gazette_sdk::render::Error::Limit(
            "candidate policy stdin bytes"
        ))
    ));
    assert!(!result.exists());
    let error = build::check_policy(
        &selected,
        permitted(),
        build::ReviewLimits {
            bytes: 0,
            ..Default::default()
        },
    )
    .expect_err("failed review cannot spawn a checker");
    assert!(matches!(error, build::PolicyCheckError::Review(_)));
    assert!(!result.exists());

    let plain_inputs = acquire(&fixture.config());
    let plain_parsed = plain_inputs
        .parse_publication(Default::default(), &gazette_adapter_markdown::Comrak)
        .expect("unconfigured captured syntax");
    let plain = plain_parsed
        .resolve(Default::default())
        .expect("unconfigured graph");
    let decisions = selection(&plain);
    let plain = plain
        .into_selected(decisions)
        .expect("unconfigured candidate");
    assert!(matches!(
        build::check_policy(&plain, permitted(), Default::default())
            .expect_err("missing checker is explicit, not synthetic success"),
        build::PolicyCheckError::Unconfigured
    ));
    assert!(!result.exists());

    let receipt = build::check_policy(
        &selected,
        process::Options {
            max_input_bytes: exact_bytes,
            ..permitted()
        },
        Default::default(),
    )
    .expect("exact complete-envelope budget permits one native invocation");
    let received = fs::read(&result).expect("complete request at the exact boundary");
    assert_eq!(received.len(), exact_bytes);
    assert_eq!(
        receipt.invocation().request_sha256,
        source::Sha256Digest::of(&received)
    );
}
