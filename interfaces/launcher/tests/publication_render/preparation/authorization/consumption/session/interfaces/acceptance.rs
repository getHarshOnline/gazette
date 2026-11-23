//! One fresh-journal executable path joins existing public operations without a second workflow.

use super::super::super::super::sign;
use super::super::super::commit;
use super::*;
use crate::preparation::direct::candidate::captured;
use gazette_sdk::{build, release_commit, source};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

fn json_command(input: &Value, arguments: &[&str]) -> Value {
    let output = crate::cli(&input.to_string(), arguments);
    assert!(
        output.status.success(),
        "{}: stdout={} stderr={}",
        arguments.join(" "),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty(), "{output:?}");
    serde_json::from_slice(&output.stdout).expect("complete public CLI JSON")
}

fn initialize(root: &Path) -> (PathBuf, store::JournalHeader) {
    let root = root.to_str().expect("UTF-8 journal root");
    let receipt = json_command(
        &Value::Null,
        &[
            "initialize-store",
            root,
            "--create-store",
            "--reserve-local-coordination",
            "--allow-coordination-files",
        ],
    );
    (
        serde_json::from_value(receipt["database"].clone()).expect("database locator"),
        serde_json::from_value(receipt["header"].clone()).expect("initial header"),
    )
}

fn reconcile_registry(
    configuration: &str,
    root: &Path,
    expected: store::JournalHeader,
) -> store::JournalHeader {
    let root = root.to_str().expect("UTF-8 journal root");
    let expected = serde_json::to_string(&expected).expect("expected header");
    let arguments = [
        "reconcile-registry",
        root,
        "--site",
        "site:one",
        "--expected-header",
        &expected,
        "--replace-registry",
        "--advance-writer-generation",
        "--reserve-local-coordination",
        "--allow-coordination-files",
    ];
    let output = crate::cli(configuration, &arguments);
    assert!(
        output.status.success(),
        "{}: {}",
        arguments.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty(), "{output:?}");
    let receipt: Value =
        serde_json::from_slice(&output.stdout).expect("complete registry CLI JSON");
    assert!(
        receipt["registry"]["rows"]
            .as_u64()
            .is_some_and(|rows| rows > 0)
    );
    serde_json::from_value(receipt["journal"].clone()).expect("registry header")
}

fn enroll(
    root: &Path,
    anchor: &Path,
    protected_roots: &[PathBuf],
) -> store::coordinator::LocalCoordinatorSelection {
    let input = json!({
        "anchor_root": anchor,
        "journal_root": root,
        "coordinator_id": "coordinator:cli-acceptance",
        "principal_scope": source::Sha256Digest::of(b"generic CLI acceptance principal"),
        "protected_roots": protected_roots,
    });
    let seal: store::coordinator::LocalCoordinatorSeal = serde_json::from_value(json_command(
        &input,
        &[
            "enroll-local-coordinator",
            "--write-coordinator-anchor",
            "--reserve-local-coordination",
            "--allow-coordination-files",
        ],
    ))
    .expect("CLI coordinator seal");
    assert_eq!(
        seal.database_sha256,
        source::Sha256Digest::of(
            &fs::read(root.join("store.sqlite")).expect("fresh release journal")
        )
    );
    store::coordinator::LocalCoordinatorSelection {
        anchor_root: anchor.to_owned(),
        binding: seal.binding,
    }
}

fn configured_fixture(
    fixture: &crate::Fixture,
    log: &Path,
    destinations: &[PathBuf],
) -> (String, build::CandidatePolicyRequirement) {
    let (mut configuration, requirement) =
        captured::source_configuration_with_base(fixture, &fixture.config(), log, "none");
    for (site, destination) in ["one", "other"].into_iter().zip(destinations) {
        let anchor = format!("site='site:{site}'\npublic_base_url=");
        assert_eq!(configuration.matches(&anchor).count(), 1);
        configuration = configuration.replacen(
            &anchor,
            &format!(
                "site='site:{site}'\ndestination={{kind='git',remote={:?},branch='publication'}}\npublic_base_url=",
                destination.to_str().expect("UTF-8 local destination")
            ),
            1,
        );
        configuration = crate::preparation::enable_site(configuration, site);
    }
    (
        configuration,
        serde_json::from_value(requirement).expect("candidate requirement"),
    )
}

fn preparation_request(
    qualification: &Value,
    requirement: &build::CandidatePolicyRequirement,
    expected: store::JournalHeader,
    coordinator: &store::coordinator::LocalCoordinatorSelection,
    stage_parent: &Path,
    parents: &[(release_commit::ParentSource, source::CommitId)],
) -> Value {
    let sites = ["one", "other"]
        .into_iter()
        .enumerate()
        .map(|(ordinal, slug)| {
            let mut metadata = super::super::super::super::metadata();
            metadata.parent = Some(parents[ordinal].1.clone());
            json!({
                "render": qualification["sites"][ordinal]["render"],
                "stage_parent": stage_parent,
                "stage_name": slug,
                "preparation_id": source::Sha256Digest::of(slug.as_bytes()),
                "metadata": metadata,
                "parent_source": parents[ordinal].0,
                "qualification": qualification["sites"][ordinal]["qualification"],
            })
        })
        .collect::<Vec<_>>();
    json!({
        "workspace": qualification["workspace"],
        "journal": coordinator.binding.journal_root,
        "expected_header": expected,
        "coordinator": coordinator,
        "selection": qualification["selection"],
        "disclosure": qualification["disclosure"],
        "candidate_check": build::ReviewLimits::default(),
        "candidate_policies": {
            "site:one": requirement,
            "site:other": requirement,
        },
        "sites": sites,
    })
}

fn declaration(batch: &Value) -> (release::Authorization, release::Binding) {
    let mut body: Value = serde_json::from_str(include_str!(
        "../../../../../../../../../crates/gazette-compiler/tests/release_authorization/authorization.json"
    ))
    .expect("authorization fixture shape");
    let mut binding: Value = serde_json::from_str(include_str!(
        "../../../../../../../../../crates/gazette-compiler/tests/release_authorization/binding.json"
    ))
    .expect("binding fixture shape");
    for field in ["journal_id", "journal_epoch", "writer_fence"] {
        body[field] = batch["journal"][field].clone();
    }
    body["ordered_targets"] = batch["ordered_targets"].clone();
    binding["journal"] = batch["journal"].clone();
    binding["ordered_targets"] = batch["ordered_targets"].clone();
    for field in [
        "validation_evidence_digest",
        "disclosure_evidence_digest",
        "policy_evidence_digest",
        "scanner_revision",
        "generic_rule_set_digest",
        "website_policy_projection_digest",
    ] {
        body[field] = batch["evidence"][field].clone();
    }
    let ordinals = json!([0, 1]);
    body["publisher_authorizations"]
        .as_array_mut()
        .expect("publisher authorizations")
        .truncate(1);
    body["publisher_authorizations"][0]["authorized_target_ordinals"] = ordinals.clone();
    binding["publishers"]
        .as_array_mut()
        .expect("publisher bindings")
        .truncate(1);
    binding["publishers"][0]["authorized_target_ordinals"] = ordinals;
    body["coordinator_id"] = json!("coordinator:cli-acceptance");
    binding["coordinator_id"] = json!("coordinator:cli-acceptance");
    (
        serde_json::from_value(body).expect("typed authorization"),
        serde_json::from_value(binding).expect("typed binding"),
    )
}

fn release_open(
    coordinator: store::coordinator::LocalCoordinatorSelection,
    expected: store::JournalHeader,
    protected_roots: Vec<PathBuf>,
    control: &Path,
    current: &Path,
) -> session::OpenRequest {
    let authority = control.join("public-authority");
    let calls = control.join("authority-calls");
    fs::write(
        &authority,
        format!(
            "#!/bin/sh\nprintf x >> '{}'\nexec /bin/cat '{}'\n",
            calls.display(),
            current.display()
        ),
    )
    .expect("fixture authority provider");
    fs::set_permissions(&authority, fs::Permissions::from_mode(0o700))
        .expect("private authority provider");
    let isolation = control.join("git-isolation");
    fs::create_dir(&isolation).expect("Git isolation root");
    fs::set_permissions(&isolation, fs::Permissions::from_mode(0o700))
        .expect("private Git isolation root");
    session::OpenRequest {
        coordinator,
        expected,
        protected_roots,
        writer: crate::preparation::options().writer,
        allow_anchor_write: true,
        git: Some(session::GitSelection {
            program: PathBuf::from(crate::native::git()),
            isolation,
            credentials: None,
        }),
        reviewed: None,
        authority: Some(session::AuthorityCommand {
            command: gazette_sdk::process::Command {
                program: authority
                    .to_str()
                    .expect("UTF-8 authority provider")
                    .to_owned(),
                executable: None,
                arguments: vec![],
                directory: control.to_str().expect("UTF-8 control root").to_owned(),
                environment: BTreeMap::new(),
            },
            input: String::new(),
            allow_execute: true,
            max_input_bytes: 0,
            max_output_bytes: 1 << 20,
            timeout_ms: 10_000,
            limits: Default::default(),
        }),
    }
}

#[test]
fn fresh_cli_processes_join_registry_qualification_preparation_and_terminal_release() {
    let fixture = crate::preparation::direct::candidate::captured::policy_fixture(&[]);
    let remotes = crate::preparation::private_directory();
    let destinations = ["one", "other"]
        .map(|site| remotes.path().join(format!("{site}.git")))
        .to_vec();
    for destination in &destinations {
        git2::Repository::init_bare(destination).expect("local release destination");
    }
    let parents = destinations
        .iter()
        .enumerate()
        .map(|(ordinal, destination)| {
            (
                release_commit::ParentSource::Bare {
                    path: destination.clone(),
                },
                commit(destination, &format!("reviewed CLI parent {ordinal}")),
            )
        })
        .collect::<Vec<_>>();
    let checker = crate::preparation::private_directory();
    let checker_log = checker.path().join("cli-acceptance.jsonl");
    let (configuration, requirement) = configured_fixture(&fixture, &checker_log, &destinations);

    // Registry projection and coordinated release deliberately use distinct stores: local
    // coordinator enrollment accepts only a never-written release journal.
    let registry_root = crate::preparation::private_directory();
    let (_, registry_initial) = initialize(registry_root.path());
    let registry_header =
        reconcile_registry(&configuration, registry_root.path(), registry_initial);
    assert_eq!(
        registry_header.writer_fence,
        registry_initial.writer_fence + 1
    );
    let journal_root = crate::preparation::private_directory();
    let (database, initial) = initialize(journal_root.path());
    let stage_parent = crate::preparation::private_directory();
    let anchor = crate::preparation::private_directory();
    let coordinator = enroll(
        journal_root.path(),
        anchor.path(),
        &[
            fixture.directory.path().to_owned(),
            stage_parent.path().to_owned(),
        ],
    );
    let enrolled_database = fs::read(&database).expect("enrolled journal bytes");

    let qualification_request =
        crate::preparation::direct::candidate::captured::qualification::request(
            &configuration,
            &requirement,
        );
    let qualified = json_command(
        &qualification_request,
        &["qualify-sites", "--allow-read", "--allow-policy-execution"],
    );
    assert_eq!(qualified["requested"], json!(["site:one", "site:other"]));
    assert!(
        qualified["sites"]
            .as_array()
            .expect("qualified sites")
            .iter()
            .all(|site| site["status"] == "supported-profile-technically-qualified")
    );
    assert_eq!(
        fs::read(&database).expect("qualification-independent journal"),
        enrolled_database
    );

    let request = preparation_request(
        &qualification_request,
        &requirement,
        initial,
        &coordinator,
        stage_parent.path(),
        &parents,
    );
    let prepared = json_command(
        &request,
        &[
            "prepare-sites",
            "--local",
            "--allow-read",
            "--create-stage",
            "--record-preparation",
            "--advance-writer-generation",
            "--reserve-local-coordination",
            "--allow-coordination-files",
            "--write-coordinator-anchor",
            "--initialize-worktree",
            "--allow-policy-execution",
        ],
    );
    assert_eq!(
        prepared["status"],
        "local-preparations-complete-not-certified"
    );
    let receipts = prepared["sites"]
        .as_array()
        .expect("two complete preparations");
    assert_eq!(receipts.len(), 2);
    let header: store::JournalHeader = serde_json::from_value(
        receipts.last().expect("preparation tail")["preparation"]["journal"].clone(),
    )
    .expect("post-preparation header");
    for (ordinal, receipt) in receipts.iter().enumerate() {
        let check: Value = serde_json::from_str(
            receipt["preparation"]["record"]["check"]["evidence"]
                .as_str()
                .expect("qualified output check"),
        )
        .expect("check evidence");
        assert_eq!(
            check["technical_certificate"], qualified["sites"][ordinal]["certificate"],
            "standalone qualification and retained preparation share the same certificate"
        );
    }
    let selections = receipts
        .iter()
        .enumerate()
        .map(|(ordinal, receipt)| release::PreparationSelection {
            output_policy: None,
            qualification: Some(release::PreparationAdmission::authored(
                serde_json::from_value(
                    qualification_request["sites"][ordinal]["qualification"].clone(),
                )
                .expect("authored qualification"),
            )),
            preparation_id: source::Sha256Digest::of(["one", "other"][ordinal].as_bytes()),
            site: format!("site:{}", ["one", "other"][ordinal])
                .parse()
                .expect("site key"),
            domain: format!("{}.example", ["one", "other"][ordinal]),
            commit: receipt["preparation"]["record"]["worktree"]["completion"]["report"]["commit"]
                .as_str()
                .expect("prepared commit")
                .parse()
                .expect("Git commit identity"),
            expected_remote_oid_or_unborn: release::RemoteState::Commit {
                oid: parents[ordinal].1.clone(),
            },
        })
        .collect::<Vec<_>>();
    let protected = vec![fixture.directory.path().to_owned()];
    let envelope = |expected| {
        json!({
            "database": database,
            "expected_header": expected,
            "selections": selections,
            "protected_roots": protected,
        })
    };
    let control = crate::preparation::private_directory();
    let current = control.path().join("public-binding.json");
    let open = release_open(
        coordinator,
        header,
        protected.clone(),
        control.path(),
        &current,
    );
    let mut carrier = Carrier::new(false, control.path());
    let ready = carrier.open(&open);

    let batch = json_command(
        &envelope(ready),
        &["inspect-release-preparations", "--allow-coordination-files"],
    );
    assert_eq!(batch["status"], "supported-profile-technical-preparations");
    assert_eq!(
        batch["ordered_targets"]
            .as_array()
            .expect("release set")
            .len(),
        2
    );
    let (body, binding) = declaration(&batch);
    let (bytes, binding) = sign(body, binding);
    fs::write(
        &current,
        serde_json::to_vec(&binding).expect("public fixture binding"),
    )
    .expect("publish fixture authority after readiness");
    let input = session::PreparedInput {
        authorization: format!(" \n{}\t", std::str::from_utf8(&bytes).expect("signed JSON")),
        selections: selections.clone(),
    };

    let registered = carrier.operate(Operation::Register {
        expected: ready,
        input: input.clone(),
        options: release::RegisterAuthorizationOptions {
            verification: verification(),
            record: store::ReleaseAuthorizationWriteOptions { allow_record: true },
        },
    });
    assert_eq!(registered["result"], "registered");
    let consumed = carrier.operate(Operation::Consume {
        expected: ready,
        input: input.clone(),
        options: consume_options(),
    });
    assert_eq!(consumed["result"], "consumed");
    let authorization_id = consumed["receipt"]["record"]["authorization_id"]
        .as_str()
        .expect("consumed authorization")
        .to_owned();
    assert_eq!(
        consumed["receipt"]["record"]["targets"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    for ordinal in 0..2 {
        let dispatched = carrier.operate(Operation::Dispatch {
            expected: ready,
            input: input.clone(),
            ordinal,
            number: 1,
            options: release::DispatchAttemptOptions {
                verification: verification(),
                read: release::RemoteReadOptions {
                    allow_read: true,
                    ..Default::default()
                },
                record: store::ReleaseAttemptOptions { allow_record: true },
                dispatch: release::DispatchOptions {
                    allow_dispatch: true,
                    timeout: Duration::from_secs(10),
                    ..Default::default()
                },
            },
        });
        assert_eq!(dispatched["result"], "dispatched");
        assert_eq!(
            dispatched["attempt"]["result"]["readback"]["comparison"],
            "intended"
        );
    }
    let settled = carrier.operate(Operation::Reconcile {
        expected: ready,
        authorization_id: authorization_id.clone(),
        number: 1,
        options: release::ReconcileReleaseOptions {
            read: release::RemoteReadOptions {
                allow_read: true,
                ..Default::default()
            },
            record: store::ReleaseReconciliationOptions {
                allow_record: true,
                ..Default::default()
            },
            ..Default::default()
        },
    });
    assert_eq!(
        settled["receipt"]["record"]["settlement"]["state"],
        "succeeded"
    );
    assert_eq!(
        carrier.operate(Operation::Close),
        json!({"result":"closed"})
    );
    carrier.finish();

    let history =
        store::inspect_release_reconciliations(&database, &authorization_id, Some(&ready), read())
            .expect("terminal native history after executable exit");
    assert_eq!(history.history.attempts.len(), 2);
    assert_eq!(history.records.len(), 1);
    assert_eq!(
        history.records[0].settlement.state,
        store::ReleaseSettlementState::Succeeded
    );
    drop((
        fixture,
        checker,
        registry_root,
        journal_root,
        stage_parent,
        anchor,
        remotes,
    ));
}
