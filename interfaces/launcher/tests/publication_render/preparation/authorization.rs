//! Fixture-only signatures join two real local preparations; no publisher or release effects.

#[path = "authorization/consumption.rs"]
mod consumption;
#[path = "authorization/inspection.rs"]
mod inspection;
#[path = "authorization/readback.rs"]
mod readback;
#[path = "authorization/registration.rs"]
mod registration;
#[path = "authorization/selection.rs"]
mod selection;

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signer, SigningKey};
use gazette_sdk::{release, release_commit as worktree, source::Sha256Digest};
use serde_json::{json, value::RawValue};
use tempfile::TempDir;

use super::super::{cli, policy::configured, worktree::metadata};
use super::{
    Fixture, Value, build, check::disclosure_policy, enable_site, fs, initialize, options,
    private_directory, published_fixture, read, render, request, store,
};

struct Prepared {
    fixture: Fixture,
    _policy_fixture: Option<Fixture>,
    _journal_root: TempDir,
    _stage_parent: TempDir,
    database: PathBuf,
    header: store::JournalHeader,
    records: Vec<store::PreparationRecord>,
    selections: Vec<release::PreparationSelection>,
    protected: Vec<PathBuf>,
    checker: Option<(TempDir, PathBuf)>,
    coordinator: Option<(TempDir, store::coordinator::LocalCoordinatorSelection)>,
}

impl Prepared {
    fn new() -> Self {
        Self::with_candidate_qualification(true, None, None, None, true, true)
    }

    fn with_checker(enabled: bool) -> Self {
        Self::with_destinations(enabled, None)
    }

    fn with_destinations(enabled: bool, destinations: Option<[&str; 2]>) -> Self {
        Self::with_release_profile(enabled, destinations, None, None)
    }

    fn with_release_profile(
        enabled: bool,
        destinations: Option<[&str; 2]>,
        coordinator_id: Option<&str>,
        parents: Option<&[(worktree::ParentSource, gazette_sdk::source::CommitId); 2]>,
    ) -> Self {
        Self::with_qualification(enabled, destinations, coordinator_id, parents, false)
    }

    fn with_qualification(
        enabled: bool,
        destinations: Option<[&str; 2]>,
        coordinator_id: Option<&str>,
        parents: Option<&[(worktree::ParentSource, gazette_sdk::source::CommitId); 2]>,
        qualified: bool,
    ) -> Self {
        Self::with_candidate_qualification(
            enabled,
            destinations,
            coordinator_id,
            parents,
            qualified,
            false,
        )
    }

    fn with_candidate_qualification(
        enabled: bool,
        destinations: Option<[&str; 2]>,
        coordinator_id: Option<&str>,
        parents: Option<&[(worktree::ParentSource, gazette_sdk::source::CommitId); 2]>,
        qualified: bool,
        candidate: bool,
    ) -> Self {
        use super::direct::candidate::captured;
        use super::direct::output_policy;
        assert!(!candidate || qualified);
        let fixture = if qualified {
            let other =
                super::super::OTHER.replace("lifecycle='archived'", "lifecycle='published'");
            if candidate {
                captured::policy_fixture(&[("other/page.md", other.as_bytes())])
            } else {
                output_policy::policy_fixture(&[("other/page.md", other.as_bytes())])
            }
        } else {
            published_fixture(&[])
        };
        let configuration = fixture.config();
        let sites = vec!["site:one".to_owned(), "site:other".to_owned()];
        let destinations =
            destinations.map(|values| values.into_iter().map(str::to_owned).collect::<Vec<_>>());
        Self::with_fixture_profile(
            fixture,
            None,
            configuration,
            &sites,
            destinations.as_deref(),
            coordinator_id,
            parents.map(|values| values.as_slice()),
            enabled,
            qualified,
            candidate,
            true,
            true,
        )
    }

    fn seven_site_candidate_qualified(
        fixture: Fixture,
        configuration: String,
        sites: &[&str],
        destinations: &[PathBuf],
        parents: &[(worktree::ParentSource, gazette_sdk::source::CommitId)],
    ) -> Self {
        let policy_fixture = super::direct::candidate::captured::policy_fixture(&[]);
        let sites = sites
            .iter()
            .map(|site| (*site).to_owned())
            .collect::<Vec<_>>();
        let destinations = destinations
            .iter()
            .map(|path| path.to_str().expect("fixture destination UTF-8").to_owned())
            .collect::<Vec<_>>();
        Self::with_fixture_profile(
            fixture,
            Some(policy_fixture),
            configuration,
            &sites,
            Some(&destinations),
            Some("coordinator:fixture"),
            Some(parents),
            true,
            true,
            true,
            false,
            false,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn with_fixture_profile(
        fixture: Fixture,
        policy_fixture: Option<Fixture>,
        base_configuration: String,
        sites: &[String],
        destinations: Option<&[String]>,
        coordinator_id: Option<&str>,
        parents: Option<&[(worktree::ParentSource, gazette_sdk::source::CommitId)]>,
        enabled: bool,
        qualified: bool,
        candidate: bool,
        include_home: bool,
        include_graph: bool,
    ) -> Self {
        use super::direct::candidate::captured;
        use super::direct::output_policy;
        assert!(!candidate || qualified);
        assert!(destinations.is_none_or(|values| values.len() == sites.len()));
        assert!(parents.is_none_or(|values| values.len() == sites.len()));
        let checker = enabled.then(|| {
            let directory = private_directory();
            let path = directory.path().join("actual-input.json");
            (directory, path)
        });
        let mut candidate_requirement = None;
        let (mut configuration, required) = if candidate {
            let (_, path) = checker.as_ref().expect("candidate fixture checker");
            let policy_fixture = policy_fixture.as_ref().unwrap_or(&fixture);
            let (config, selected) = captured::source_configuration_with_base(
                policy_fixture,
                &base_configuration,
                path,
                "none",
            );
            let selected: build::CandidatePolicyRequirement =
                serde_json::from_value(selected).unwrap();
            let output = build::OutputPolicyRequirement {
                policy: selected.policy.clone(),
                command_sha256: selected.command_sha256,
                response_bytes: 256 << 10,
            };
            candidate_requirement = Some(selected);
            (config, Some(output))
        } else if qualified {
            let (_, path) = checker
                .as_ref()
                .expect("qualified fixture selects its checker");
            let (config, required) = output_policy::source_configuration(&fixture, path, "allow");
            (config, Some(required))
        } else {
            (
                checker.as_ref().map_or_else(
                    || base_configuration.clone(),
                    |(_, path)| configured(&fixture, path, 0).0,
                ),
                None,
            )
        };
        for (ordinal, site) in sites.iter().enumerate() {
            let slug = site
                .strip_prefix("site:")
                .expect("fixture site has the canonical prefix");
            let anchor = format!("site='{site}'\npublic_base_url=");
            assert!(configuration.contains(&anchor));
            let remote = destinations.map_or_else(
                || format!("'https://example.invalid/{slug}.git'"),
                |paths| format!("{:?}", paths[ordinal]),
            );
            let channel = format!("content:{slug}");
            let machine = candidate.then(|| format!(
                "machine={{sitemap=true,{},search='lexical',json_ld=true,feeds={{identity_base='https://identity.example/{slug}/',channels=[{{id='feed:json',channel='{channel}',path='/feed.json',format='json',scope={{kind='site'}}}}]}}}}\n",
                crate::curated_llms(&channel, &[("Start here", &[channel.as_str()])])
            ));
            configuration = configuration.replace(&anchor, &format!(
                "site='{site}'\ndestination={{kind='git',remote={remote},branch='publication'}}\n{}public_base_url=",
                machine.as_deref().unwrap_or_default()
            ));
            configuration = enable_site(configuration, slug);
        }
        let journal_root = private_directory();
        let journal = initialize(journal_root.path());
        let stage_parent = private_directory();
        let mut coordinator_protected = vec![
            fixture.directory.path().to_owned(),
            stage_parent.path().to_owned(),
        ];
        if let Some(policy_fixture) = &policy_fixture {
            coordinator_protected.push(policy_fixture.directory.path().to_owned());
        }
        let coordinator = coordinator_id.map(|coordinator_id| {
            let anchor = private_directory();
            let seal = store::coordinator::enroll(
                store::coordinator::EnrollmentRequest {
                    anchor_root: anchor.path(),
                    journal_root: journal_root.path(),
                    coordinator_id,
                    principal_scope: Sha256Digest::of(b"disposable publication test principal"),
                    protected_roots: &coordinator_protected,
                },
                options().writer.session,
                true,
            )
            .expect("enroll independent anchor before any preparation writes");
            let selected = store::coordinator::LocalCoordinatorSelection {
                anchor_root: anchor.path().to_owned(),
                binding: seal.binding,
            };
            (anchor, selected)
        });
        let mut header = journal.header;
        let mut records = Vec::new();
        let mut selections = Vec::new();
        let selected_sites = sites.iter().map(String::as_str).collect::<Vec<_>>();
        let captured_inputs =
            candidate.then(|| crate::sites::acquire_set(&configuration, &selected_sites));
        let checked = captured_inputs.as_ref().map(|inputs| {
            let mut content = sites
                .iter()
                .map(|site| {
                    (
                        format!(
                            "content:{}",
                            site.strip_prefix("site:").expect("fixture site prefix")
                        ),
                        "include",
                    )
                })
                .collect::<BTreeMap<_, _>>();
            if include_home {
                content.insert("collection:home".to_owned(), "include");
            }
            let selection = serde_json::from_value(json!({"content":content,"assets":{}}))
                .expect("complete fixture candidate selection");
            captured::render_selected_profile(
                inputs,
                candidate_requirement.as_ref().unwrap(),
                selection,
                include_graph,
            )
        });
        for (ordinal, selected_site) in sites.iter().enumerate() {
            let site = selected_site
                .strip_prefix("site:")
                .expect("fixture site prefix");
            let key = selected_site.parse().expect("explicit configured site");
            let inputs = (!candidate).then(|| {
                build::acquire_publication(
                    &configuration,
                    &key,
                    Default::default(),
                    gazette_sdk::source::PreparationLimits {
                        sources: 8,
                        entries: 128,
                        bytes: 1 << 20,
                        per_source: Default::default(),
                    },
                )
                .expect("real exact-pin selected site acquisition")
            });
            let ordinary_render = inputs
                .as_ref()
                .map(|inputs| render(inputs).expect("actual human and machine output"));
            let rendered = ordinary_render.as_ref().unwrap_or_else(|| {
                checked
                    .as_ref()
                    .unwrap()
                    .sites()
                    .get(&key)
                    .unwrap()
                    .as_ref()
                    .expect("whole checked candidate render")
            });
            let qualification = required.as_ref().map(|required| {
                let mut selected = output_policy::qualification::expected(rendered, required);
                selected.candidate_policy = candidate_requirement.clone();
                selected
            });
            let selected_journal = coordinator
                .as_ref()
                .map_or(journal_root.path(), |(_, selected)| {
                    selected.binding.journal_root.as_path()
                });
            let mut stage = request(selected_journal, header, stage_parent.path(), site);
            stage.id = Sha256Digest::of(site.as_bytes());
            let parent = parents.map(|parents| &parents[ordinal]);
            let mut recipe = metadata();
            recipe.parent = parent.map(|(_, commit)| commit.clone());
            let completed = build::prepare_local(
                rendered,
                build::LocalPreparationRequest {
                    output_policy: None,
                    qualification: qualification.as_ref(),
                    coordinator: coordinator.as_ref().map(|(_, selected)| selected),
                    stage,
                    metadata: &recipe,
                    parent_source: parent.map(|(source, _)| source),
                    disclosure: &disclosure_policy(),
                },
                build::LocalPreparationOptions {
                    stage: options(),
                    allow_anchor_write: coordinator.is_some(),
                    policy: gazette_sdk::process::Options {
                        allow_execute: enabled,
                        ..Default::default()
                    },
                    worktree: worktree::WorktreeOptions {
                        allow_initialize: true,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .expect("real stage, checks, SQLite and Git without contacting configured remote");
            assert_eq!(
                completed.record.intent.output,
                rendered.site().output().inventory().identity()
            );
            let commit = completed
                .record
                .worktree
                .as_ref()
                .expect("durable recipe")
                .completion
                .as_ref()
                .expect("native completion")
                .report
                .commit
                .clone();
            selections.push(release::PreparationSelection {
                output_policy: None,
                qualification: qualification.map(release::PreparationAdmission::authored),
                preparation_id: completed.record.intent.id,
                site: key,
                domain: format!("{site}.example"),
                commit,
                expected_remote_oid_or_unborn: parent
                    .map_or(release::RemoteState::Unborn, |(_, oid)| {
                        release::RemoteState::Commit { oid: oid.clone() }
                    }),
            });
            header = completed.journal;
            records.push(completed.record);
        }
        let protected = vec![fixture.directory.path().to_owned()];
        let mut protected = protected;
        if let Some(policy_fixture) = &policy_fixture {
            protected.push(policy_fixture.directory.path().to_owned());
        }
        Self {
            fixture,
            _policy_fixture: policy_fixture,
            _journal_root: journal_root,
            _stage_parent: stage_parent,
            database: journal.database,
            header,
            records,
            selections,
            protected,
            checker,
            coordinator,
        }
    }

    fn request<'a>(
        &'a self,
        selections: &'a [release::PreparationSelection],
    ) -> release::PreparationBatchRequest<'a> {
        release::PreparationBatchRequest {
            database: &self.database,
            expected: self.header,
            selections,
            protected_roots: &self.protected,
        }
    }

    fn inspect(&self) -> release::PreparationBatch {
        release::inspect_preparations(self.request(&self.selections), batch_options())
            .expect("complete native ordered batch")
    }

    fn envelope(&self) -> Value {
        json!({"database":self.database, "expected_header":self.header,
            "selections":self.selections, "protected_roots":self.protected})
    }

    fn snapshot(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        let mut result = BTreeMap::new();
        result.insert(
            self.database.clone(),
            fs::read(&self.database).expect("database bytes"),
        );
        for record in &self.records {
            files(&record.intent.address.path(), &mut result);
        }
        files(self.fixture.directory.path(), &mut result);
        if let Some((_, path)) = &self.checker {
            result.insert(
                path.clone(),
                fs::read(path).expect("actual checker observation"),
            );
        }
        result
    }

    fn unchanged(&self, snapshot: &BTreeMap<PathBuf, Vec<u8>>) {
        assert_eq!(
            &self.snapshot(),
            snapshot,
            "no database, Git, output or source-byte mutation"
        );
        for record in &self.records {
            let inspected = store::inspect_preparation(
                &self.database,
                record.intent.id,
                Some(&self.header),
                read(),
            )
            .expect("same independently expected writer coordinates");
            assert_eq!(inspected.journal.header, self.header);
            assert_eq!(inspected.record.as_ref(), Some(record));
        }
    }
}

fn files(root: &Path, result: &mut BTreeMap<PathBuf, Vec<u8>>) {
    for entry in fs::read_dir(root).expect("disposable native tree") {
        let entry = entry.expect("native entry");
        let kind = entry
            .file_type()
            .expect("native kind without following symlinks");
        if kind.is_dir() {
            files(&entry.path(), result);
        } else {
            assert!(
                kind.is_file(),
                "fixture must not silently follow special files"
            );
            result.insert(
                entry.path(),
                fs::read(entry.path()).expect("exact native bytes"),
            );
        }
    }
}

fn batch_options() -> release::PreparationBatchOptions {
    release::PreparationBatchOptions {
        inspection: worktree::JournalWorktreeInspectionOptions {
            read: read(),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn verify(
    prepared: &Prepared,
    selections: &[release::PreparationSelection],
    authorization: &[u8],
    binding: &release::Binding,
) -> Result<release::PreparedAuthorizationReport, release::PreparedAuthorizationError> {
    release::verify_prepared_authorization(
        release::PreparedAuthorizationRequest {
            authorization,
            binding,
            preparations: prepared.request(selections),
        },
        release::PreparedAuthorizationOptions {
            preparations: batch_options(),
            ..Default::default()
        },
    )
}

// Existing canonical fixtures supply only the stable authorization/key/time shape.
// Every target, journal coordinate and local evidence digest below comes from real
// native preparation. This local test issuer has no production private-key role.
fn declaration(batch: &release::PreparationBatch) -> (release::Authorization, release::Binding) {
    let mut body: release::Authorization = serde_json::from_str(include_str!(
        "../../../../../crates/gazette-compiler/tests/release_authorization/authorization.json"
    ))
    .expect("existing native authorization fixture");
    let mut binding: release::Binding = serde_json::from_str(include_str!(
        "../../../../../crates/gazette-compiler/tests/release_authorization/binding.json"
    ))
    .expect("independent fixture authority declarations");
    let header = batch.journal();
    body.journal_id = header.journal_id;
    body.journal_epoch = header.journal_epoch;
    body.writer_fence = header.writer_fence;
    body.ordered_targets = batch.targets().to_vec();
    binding.journal = header;
    binding.ordered_targets = batch.targets().to_vec();
    let evidence = batch.evidence();
    body.validation_evidence_digest = evidence.validation_evidence_digest;
    body.disclosure_evidence_digest = evidence.disclosure_evidence_digest;
    body.policy_evidence_digest = evidence.policy_evidence_digest;
    body.scanner_revision = evidence.scanner_revision.clone();
    body.generic_rule_set_digest = evidence.generic_rule_set_digest;
    body.website_policy_projection_digest = evidence.website_policy_projection_digest;
    body.publisher_authorizations.truncate(1);
    let ordinals = (0..u32::try_from(batch.targets().len()).expect("fixture target count"))
        .collect::<Vec<_>>();
    body.publisher_authorizations[0].authorized_target_ordinals = ordinals.clone();
    binding.publishers.truncate(1);
    binding.publishers[0].authorized_target_ordinals = ordinals;
    (body, binding)
}

fn sign(
    mut body: release::Authorization,
    mut binding: release::Binding,
) -> (Vec<u8>, release::Binding) {
    let mut canonical = serde_json::to_value(&body).expect("fixture body projection");
    canonical
        .as_object_mut()
        .expect("body object")
        .remove("canonical_body_sha256");
    for publisher in canonical["publisher_authorizations"]
        .as_array_mut()
        .expect("publishers")
    {
        publisher
            .as_object_mut()
            .expect("publisher object")
            .remove("signature_envelope");
    }
    let digest = Sha256Digest::of(
        &serde_jcs::to_vec(&canonical).expect("native pinned JCS fixture tooling"),
    );
    body.canonical_body_sha256 = digest;
    binding.canonical_body_sha256 = digest;
    let key = SigningKey::from_bytes(&[3; 32]);
    assert_eq!(
        binding.publishers[0].public_key,
        STANDARD.encode(key.verifying_key().to_bytes())
    );
    let mut message = b"WEBSITE_RELEASE_BATCH_AUTHORIZATION_V1\0".to_vec();
    message.extend_from_slice(digest.as_bytes());
    body.publisher_authorizations[0]
        .signature_envelope
        .signature = STANDARD.encode(key.sign(&message).to_bytes());
    let transport = serde_json::to_vec(&body).expect("signed fixture transport");
    release::verify(&transport, &binding, Default::default())
        .expect("positive pure signature control before native join");
    (transport, binding)
}

/// Reconstruct the documented literal row/envelope encoding independently from
/// actual immutable records, without invoking the application's digest helpers.
fn assert_evidence(prepared: &Prepared, batch: &release::PreparationBatch) {
    let (mut validation, mut disclosure, mut policy, mut website) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (ordinal, record) in prepared.records.iter().enumerate() {
        let reference = Sha256Digest::of(
            format!(
                "{{\"format\":\"gazette.prepared-local/1\",\"preparation\":{}}}",
                serde_json::to_string(record)
                    .expect("immutable original record, not receipt/current header")
            )
            .as_bytes(),
        );
        assert_eq!(
            batch.targets()[ordinal].prepared_release_receipt_ref,
            reference
        );
        validation.push(format!(
            "{{\"ordinal\":{ordinal},\"preparation_ref\":\"{reference}\",\"output\":{},\"preparation_evidence_sha256\":\"{}\"}}",
            serde_json::to_string(&record.intent.output).expect("native tree wire identity"), record.intent.evidence_sha256));
        let check = record.check.as_ref().expect("actual retained check");
        let raw: BTreeMap<String, &RawValue> = serde_json::from_str(&check.evidence)
            .expect("borrow exact original nested slices without canonicalizing");
        for (name, rows) in [("disclosure", &mut disclosure), ("policy", &mut policy)] {
            let digest = Sha256Digest::of(raw[name].get().as_bytes());
            rows.push(format!("{{\"ordinal\":{ordinal},\"sha256\":\"{digest}\"}}"));
        }
        let decoded: Value =
            serde_json::from_str(raw["disclosure"].get()).expect("retained disclosure fields");
        assert_eq!(decoded["scanner"], batch.evidence().scanner_revision);
        assert_eq!(
            decoded["generic_rules_sha256"],
            batch.evidence().generic_rule_set_digest.to_string()
        );
        let qualification = prepared.selections[ordinal].qualification.as_ref();
        let candidate = qualification
            .and_then(release::PreparationAdmission::as_authored)
            .and_then(|required| required.candidate_policy.as_ref())
            .map(|requirement| &requirement.policy);
        let output = qualification
            .and_then(release::PreparationAdmission::as_authored)
            .map(|required| &required.output_policy.policy);
        website.push(format!(
            "{{\"ordinal\":{ordinal},\"candidate\":{},\"output\":{}}}",
            serde_json::to_string(&candidate).expect("candidate policy identity"),
            serde_json::to_string(&output).expect("output policy identity")
        ));
    }
    for (name, rows, actual) in [
        (
            "validation",
            validation,
            batch.evidence().validation_evidence_digest,
        ),
        (
            "disclosure",
            disclosure,
            batch.evidence().disclosure_evidence_digest,
        ),
        ("policy", policy, batch.evidence().policy_evidence_digest),
        (
            "website-policy",
            website,
            batch.evidence().website_policy_projection_digest,
        ),
    ] {
        let encoded = format!(
            "{{\"format\":\"gazette.prepared-{name}/1\",\"rows\":[{}]}}",
            rows.join(",")
        );
        assert_eq!(
            actual,
            Sha256Digest::of(encoded.as_bytes()),
            "{name} exact ordered native projection"
        );
    }
}

#[test]
fn two_native_preparations_join_sdk_cli_and_fixture_signatures_without_consumption_or_reopen() {
    let prepared = Prepared::new();
    fs::write(
        prepared.fixture.directory.path().join("one/home.md"),
        b"not reopened",
    )
    .expect("source changed after all retained preparations");
    let snapshot = prepared.snapshot();
    let batch = prepared.inspect();
    assert_evidence(&prepared, &batch);
    assert_eq!(batch.targets().len(), 2);
    for (index, target) in batch.targets().iter().enumerate() {
        assert_eq!(target.ordinal as usize, index);
        assert_eq!(target.local_commit_oid, prepared.selections[index].commit);
        assert_eq!(
            target.output_digest,
            prepared.records[index].intent.output.sha256()
        );
        assert_eq!(
            target.refspec,
            format!("{}:refs/heads/publication", target.local_commit_oid)
        );
    }
    let expected = serde_json::to_value(&batch).expect("native batch evidence");
    assert_eq!(
        expected["status"],
        "supported-profile-technical-preparations"
    );
    assert_eq!(
        serde_json::to_value(prepared.inspect()).expect("repeat read"),
        expected
    );
    let output = cli(
        &prepared.envelope().to_string(),
        &["inspect-release-preparations", "--allow-coordination-files"],
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).expect("actual CLI report"),
        expected
    );
    let (body, binding) = declaration(&batch);
    let (authorization, binding) = sign(body, binding);
    let verified = verify(&prepared, &prepared.selections, &authorization, &binding)
        .expect("exact native join");
    assert_eq!(verified.verification.target_count, 2);
    assert_eq!(
        verified.preparations_sha256,
        Sha256Digest::of(&serde_json::to_vec(&batch).expect("same native complete batch bytes"))
    );
    let expected = serde_json::to_value(verified).expect("joined report");
    assert_eq!(
        expected["status"],
        "signature-and-local-preparations-verified-not-certified"
    );
    // Preserve the received transport bytes, not merely its decoded JSON meaning.
    let mut envelope: BTreeMap<String, Box<RawValue>> =
        serde_json::from_value(prepared.envelope()).expect("private request fields");
    envelope.insert(
        "authorization".into(),
        RawValue::from_string(String::from_utf8(authorization.clone()).expect("JSON transport"))
            .expect("original signed transport"),
    );
    envelope.insert(
        "binding".into(),
        RawValue::from_string(
            serde_json::to_string(&binding).expect("independent fixture binding"),
        )
        .expect("binding JSON"),
    );
    let output = cli(
        &serde_json::to_string(&envelope).expect("raw authorization retained in CLI envelope"),
        &[
            "verify-prepared-authorization",
            "--allow-coordination-files",
        ],
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).expect("actual joined CLI report"),
        expected
    );
    assert_eq!(
        serde_json::to_value(
            verify(&prepared, &prepared.selections, &authorization, &binding)
                .expect("read-only replay is not one-use consumption")
        )
        .expect("repeat report"),
        expected
    );
    prepared.unchanged(&snapshot);
}

#[test]
fn configured_policy_and_exact_nested_whitespace_remain_distinct_retained_evidence() {
    let mut prepared = Prepared::with_checker(true);
    let batch = prepared.inspect();
    assert_evidence(&prepared, &batch);
    let (_, result_path) = prepared
        .checker
        .as_ref()
        .expect("real configured subprocess fixture");
    let delivered = fs::read(result_path).expect("actual final checker stdin");
    for record in &prepared.records {
        let evidence: Value = serde_json::from_str(&record.check.as_ref().expect("check").evidence)
            .expect("stored evidence");
        assert_eq!(evidence["policy"]["status"], "checked");
    }
    let final_check: Value = serde_json::from_str(
        &prepared.records[1]
            .check
            .as_ref()
            .expect("final check")
            .evidence,
    )
    .expect("last checker evidence");
    assert_eq!(
        final_check["policy"]["evidence"]["invocation"]["request_sha256"],
        Sha256Digest::of(&delivered).to_string()
    );
    fs::write(result_path, b"checker must not run during inspection")
        .expect("native observation sentinel");
    let snapshot = prepared.snapshot();
    let (body, binding) = declaration(&batch);
    let (authorization, binding) = sign(body, binding);
    assert!(matches!(
        verify(&prepared, &prepared.selections, &authorization, &binding),
        Err(release::PreparedAuthorizationError::Preparation(
            release::PreparationBatchError::Admission
        ))
    ));
    prepared.unchanged(&snapshot);

    let original = &prepared.records[0].check.as_ref().expect("check").evidence;
    // Policy-envelope whitespace is outer transport. The nested disclosure bytes,
    // unlike that envelope, are bound to the actual checker and cannot be rewritten.
    let changed = original.replacen("\"policy\":{", "\"policy\":{ ", 1);
    assert_ne!(changed, *original);
    assert_eq!(
        serde_json::from_str::<Value>(&changed).expect("whitespace transport"),
        serde_json::from_str::<Value>(original).expect("same JSON meaning")
    );
    replace_check(&prepared, &changed);
    prepared.records[0] = store::inspect_preparation(
        &prepared.database,
        prepared.records[0].intent.id,
        Some(&prepared.header),
        read(),
    )
    .expect("real replaced fixture history")
    .record
    .expect("stored record");
    let snapshot = prepared.snapshot();
    let changed_batch = prepared.inspect();
    assert_evidence(&prepared, &changed_batch);
    assert_eq!(
        changed_batch.targets()[0].output_digest,
        batch.targets()[0].output_digest
    );
    assert_ne!(
        changed_batch.targets()[0].prepared_release_receipt_ref,
        batch.targets()[0].prepared_release_receipt_ref
    );
    assert_ne!(
        changed_batch.evidence().validation_evidence_digest,
        batch.evidence().validation_evidence_digest
    );
    assert_eq!(
        changed_batch.evidence().disclosure_evidence_digest,
        batch.evidence().disclosure_evidence_digest
    );
    assert_ne!(
        changed_batch.evidence().policy_evidence_digest,
        batch.evidence().policy_evidence_digest
    );
    assert_eq!(
        changed_batch.evidence().website_policy_projection_digest,
        batch.evidence().website_policy_projection_digest
    );
    assert!(matches!(
        verify(&prepared, &prepared.selections, &authorization, &binding),
        Err(release::PreparedAuthorizationError::Preparation(
            release::PreparationBatchError::Admission
        ))
    ));
    prepared.unchanged(&snapshot);
}

#[test]
fn complete_selection_identity_order_and_bounds_are_not_silently_repaired() {
    let prepared = Prepared::new();
    let snapshot = prepared.snapshot();
    let (body, binding) = declaration(&prepared.inspect());
    let (authorization, binding) = sign(body, binding);
    assert!(matches!(
        release::inspect_preparations(prepared.request(&[]), batch_options()),
        Err(release::PreparationBatchError::Selection)
    ));
    let duplicate = vec![
        prepared.selections[0].clone(),
        prepared.selections[0].clone(),
    ];
    assert!(matches!(
        release::inspect_preparations(prepared.request(&duplicate), batch_options()),
        Err(release::PreparationBatchError::Selection)
    ));
    assert!(matches!(
        verify(
            &prepared,
            &prepared.selections[..1],
            &authorization,
            &binding
        ),
        Err(release::PreparedAuthorizationError::Mismatch)
    ));
    let mut reordered = prepared.selections.clone();
    reordered.reverse();
    assert!(matches!(
        verify(&prepared, &reordered, &authorization, &binding),
        Err(release::PreparedAuthorizationError::Mismatch)
    ));
    let mut wrong = prepared.selections.clone();
    wrong[0].site = "site:absent".parse().expect("different site");
    assert!(matches!(
        release::inspect_preparations(prepared.request(&wrong), batch_options()),
        Err(release::PreparationBatchError::Mismatch)
    ));
    wrong = prepared.selections.clone();
    wrong[0].domain = "other-association.example".into();
    assert!(matches!(
        verify(&prepared, &wrong, &authorization, &binding),
        Err(release::PreparedAuthorizationError::Mismatch)
    ));
    wrong = prepared.selections.clone();
    wrong[0].preparation_id = Sha256Digest::of(b"not a stored preparation");
    assert!(matches!(
        release::inspect_preparations(prepared.request(&wrong), batch_options()),
        Err(release::PreparationBatchError::Inspection(_))
    ));
    for selected_options in [
        release::PreparationBatchOptions {
            targets: 1,
            ..batch_options()
        },
        release::PreparationBatchOptions {
            evidence_bytes: 0,
            ..batch_options()
        },
    ] {
        assert!(matches!(
            release::inspect_preparations(prepared.request(&prepared.selections), selected_options),
            Err(release::PreparationBatchError::Selection)
        ));
    }
    assert!(matches!(
        release::inspect_preparations(
            prepared.request(&prepared.selections),
            release::PreparationBatchOptions {
                evidence_bytes: 100,
                ..batch_options()
            }
        ),
        Err(release::PreparationBatchError::Evidence)
    ));
    let denied =
        release::inspect_preparations(prepared.request(&prepared.selections), Default::default());
    assert!(matches!(
        denied,
        Err(release::PreparationBatchError::Inspection(_))
    ));
    prepared.unchanged(&snapshot);
}

#[test]
fn valid_signatures_over_wrong_targets_or_evidence_do_not_replace_native_preparation() {
    let prepared = Prepared::new();
    let snapshot = prepared.snapshot();
    let (original, independent) = declaration(&prepared.inspect());
    for field in [
        "domain",
        "remote",
        "branch",
        "receipt",
        "output",
        "commit",
        "remote-state",
        "validation",
        "disclosure",
        "policy",
        "scanner",
        "generic-rules",
        "website-policy",
    ] {
        let mut body = original.clone();
        let mut binding = independent.clone();
        let digest = Sha256Digest::of(b"different signed coordinate");
        match field {
            "domain" => body.ordered_targets[0].domain = "wrong.example".into(),
            "remote" => {
                body.ordered_targets[0].remote = "https://wrong.example.invalid/repo.git".into()
            }
            "branch" => {
                body.ordered_targets[0].branch = "different".into();
                body.ordered_targets[0].refspec = format!(
                    "{}:refs/heads/different",
                    body.ordered_targets[0].local_commit_oid
                );
            }
            "receipt" => body.ordered_targets[0].prepared_release_receipt_ref = digest,
            "output" => body.ordered_targets[0].output_digest = digest,
            "commit" => {
                body.ordered_targets[0].local_commit_oid =
                    "a".repeat(40).parse().expect("full different OID");
                body.ordered_targets[0].refspec = format!(
                    "{}:refs/heads/publication",
                    body.ordered_targets[0].local_commit_oid
                );
            }
            "remote-state" => {
                body.ordered_targets[0].expected_remote_oid_or_unborn =
                    release::RemoteState::Commit {
                        oid: "b".repeat(40).parse().expect("different pre-state"),
                    }
            }
            "validation" => body.validation_evidence_digest = digest,
            "disclosure" => body.disclosure_evidence_digest = digest,
            "policy" => body.policy_evidence_digest = digest,
            "scanner" => body.scanner_revision = "different-scanner".into(),
            "generic-rules" => body.generic_rule_set_digest = digest,
            "website-policy" => body.website_policy_projection_digest = digest,
            _ => unreachable!("closed attack cases"),
        }
        // Even matching caller assertions and a fresh valid fixture signature may
        // not substitute these coordinates for the retained native preparation.
        binding.ordered_targets = body.ordered_targets.clone();
        let (authorization, binding) = sign(body, binding);
        assert!(
            matches!(
                verify(&prepared, &prepared.selections, &authorization, &binding),
                Err(release::PreparedAuthorizationError::Mismatch)
            ),
            "{field}"
        );
    }
    prepared.unchanged(&snapshot);
}

#[test]
fn changed_retained_output_refuses_native_join_without_fixing_git_or_advancing_journal() {
    let prepared = Prepared::new();
    let (body, binding) = declaration(&prepared.inspect());
    let (authorization, binding) = sign(body, binding);
    let root = prepared.records[1].intent.address.path();
    fs::write(root.join("model.json"), b"changed native bytes").expect("controlled output drift");
    let snapshot = prepared.snapshot();
    assert!(matches!(
        verify(&prepared, &prepared.selections, &authorization, &binding),
        Err(release::PreparedAuthorizationError::Preparation(
            release::PreparationBatchError::Inspection(_)
        ))
    ));
    let output = cli(
        &prepared.envelope().to_string(),
        &["inspect-release-preparations", "--allow-coordination-files"],
    );
    assert!(!output.status.success());
    assert!(output.stdout.is_empty(), "no successful partial batch");
    prepared.unchanged(&snapshot);
}

#[test]
fn caller_remote_prestate_must_match_the_original_local_parent_recipe() {
    let prepared = Prepared::new();
    let snapshot = prepared.snapshot();
    let (mut body, mut binding) = declaration(&prepared.inspect());
    let state = release::RemoteState::Commit {
        oid: "b"
            .repeat(40)
            .parse()
            .expect("well-formed alternate remote pre-state"),
    };
    let mut selections = prepared.selections.clone();
    selections[0].expected_remote_oid_or_unborn = state.clone();
    body.ordered_targets[0].expected_remote_oid_or_unborn = state;
    binding.ordered_targets = body.ordered_targets.clone();
    let (authorization, binding) = sign(body, binding);
    assert!(matches!(
        verify(&prepared, &selections, &authorization, &binding),
        Err(release::PreparedAuthorizationError::Preparation(
            release::PreparationBatchError::Mismatch
        ))
    ));
    prepared.unchanged(&snapshot);
}

// Controlled corruption of this disposable database only. Recompute both hashes
// so the application must check actual evidence rather than just its byte digest.
// The operation under test receives no SQL-writing capability.
fn replace_check(prepared: &Prepared, evidence: &str) {
    let connection =
        rusqlite::Connection::open(&prepared.database).expect("exact disposable journal");
    connection
        .execute_batch("PRAGMA foreign_keys=ON; BEGIN; PRAGMA defer_foreign_keys=ON;")
        .expect("atomic fixture-only evidence replacement");
    let digest = Sha256Digest::of(evidence.as_bytes());
    let id = prepared.records[0].intent.id;
    assert_eq!(connection.execute(
        "UPDATE gazette_preparation_checks SET evidence=?1,evidence_sha256=?2 WHERE intent_id=?3",
        rusqlite::params![evidence, digest.as_bytes(), id.as_bytes()],
    ).expect("selected fixture check"), 1);
    assert_eq!(
        connection
            .execute(
                "UPDATE gazette_preparation_worktrees SET check_sha256=?1 WHERE intent_id=?2",
                rusqlite::params![digest.as_bytes(), id.as_bytes()],
            )
            .expect("same exact worktree linkage"),
        1
    );
    connection
        .execute_batch("COMMIT;")
        .expect("consistent linked corruption setup");
    connection
        .close()
        .expect("close fixture writer before verifier");
}

// Rebind fixture-only original evidence and the existing check/worktree links. The
// generic fixture has no checker invocation whose raw disclosure digest could
// otherwise be changed by re-encoding unrelated JSON during corruption setup.
fn replace_original_evidence(prepared: &Prepared, evidence: &str) {
    let record = &prepared.records[0];
    let digest = Sha256Digest::of(evidence.as_bytes());
    let mut check: Value = serde_json::from_str(
        &record
            .check
            .as_ref()
            .expect("existing native checks")
            .evidence,
    )
    .expect("original check envelope");
    assert_eq!(check["policy"]["status"], "not-configured");
    check["preparation_evidence_sha256"] = json!(digest);
    let connection =
        rusqlite::Connection::open(&prepared.database).expect("exact disposable journal");
    assert_eq!(
        connection
            .execute(
                "UPDATE gazette_preparation_intents SET evidence=?1,evidence_sha256=?2 WHERE id=?3",
                rusqlite::params![evidence, digest.as_bytes(), record.intent.id.as_bytes()],
            )
            .expect("selected fixture original evidence"),
        1
    );
    connection
        .close()
        .expect("close original-evidence setup writer");
    replace_check(
        prepared,
        &serde_json::to_string(&check).expect("linked check evidence"),
    );
}

#[test]
fn malformed_repository_source_provenance_refuses_native_join_after_hashes_are_rebound() {
    let prepared = Prepared::with_checker(false);
    let original: Value = serde_json::from_str(&prepared.records[0].intent.evidence)
        .expect("actual original producer evidence");
    let repository_source = original["repository_source_provenance"].clone();
    assert_eq!(
        repository_source["profile"],
        "gazette-repository-source-provenance/1"
    );
    prepared.inspect();
    // A complete valid re-encoding is accepted before testing malformed fields,
    // proving that refusal below is not incidental JSON ordering or stale linkage.
    replace_original_evidence(
        &prepared,
        &serde_json::to_string(&original).expect("valid evidence"),
    );
    prepared.inspect();
    for field in [
        "missing", "null", "tuple", "profile", "digest", "unknown", "files", "bytes",
    ] {
        let mut changed = original.clone();
        match field {
            "missing" => {
                changed
                    .as_object_mut()
                    .expect("original object")
                    .remove("repository_source_provenance");
            }
            "null" => changed["repository_source_provenance"] = Value::Null,
            "tuple" => {
                changed["repository_source_provenance"] = json!([
                    repository_source["profile"],
                    repository_source["sha256"],
                    repository_source["files"],
                    repository_source["bytes"]
                ])
            }
            "profile" => {
                changed["repository_source_provenance"]["profile"] =
                    json!("unverified-arbitrary-source")
            }
            "digest" => changed["repository_source_provenance"]["sha256"] = json!("not-a-digest"),
            "unknown" => changed["repository_source_provenance"]["approved"] = json!(true),
            "files" => changed["repository_source_provenance"]["files"] = json!(0),
            "bytes" => {
                changed["repository_source_provenance"]["bytes"] = json!((256_u64 << 20) + 1)
            }
            _ => unreachable!("closed malformed provenance cases"),
        }
        replace_original_evidence(
            &prepared,
            &serde_json::to_string(&changed).expect("malformed fixture evidence"),
        );
        let historical = store::inspect_preparation(
            &prepared.database,
            prepared.records[0].intent.id,
            Some(&prepared.header),
            read(),
        )
        .expect("store-level hashes and linked history remain valid");
        let snapshot = prepared.snapshot();
        assert!(
            matches!(
                release::inspect_preparations(
                    prepared.request(&prepared.selections),
                    batch_options()
                ),
                Err(release::PreparationBatchError::Evidence)
            ),
            "{field} must fail the repository-source provenance contract"
        );
        assert_eq!(
            prepared.snapshot(),
            snapshot,
            "read refusal must not repair {field}"
        );
        let after = store::inspect_preparation(
            &prepared.database,
            prepared.records[0].intent.id,
            Some(&prepared.header),
            read(),
        )
        .expect("unchanged native history after refusal");
        assert_eq!(after.record, historical.record);
        assert_eq!(after.journal.header, historical.journal.header);
    }
    replace_original_evidence(
        &prepared,
        &serde_json::to_string(&original).expect("restored fixture evidence"),
    );
    prepared.inspect();
}

#[test]
fn checker_disclosure_binding_survives_only_exact_report_bytes() {
    let prepared = Prepared::with_checker(true);
    prepared.inspect();
    let original = &prepared.records[0]
        .check
        .as_ref()
        .expect("real check")
        .evidence;
    let raw: BTreeMap<String, &RawValue> = serde_json::from_str(original).expect("raw report");
    let hash = Sha256Digest::of(raw["disclosure"].get().as_bytes());
    let binding = format!("\"disclosure_sha256\":\"{hash}\"");
    assert!(original.contains(&binding));
    for changed in [
        original.replacen("\"disclosure\":{", "\"disclosure\":{ ", 1),
        original.replacen(
            &binding,
            &format!(
                "\"disclosure_sha256\":\"{}\"",
                Sha256Digest::of(b"another scan")
            ),
            1,
        ),
        original.replacen(&format!(",{binding}"), "", 1),
    ] {
        assert_ne!(changed, *original);
        replace_check(&prepared, &changed);
        // All native envelope hashes and Git linkage are valid; only the internal
        // checker/report join is wrong. Read-back must refuse without repairing it.
        let historical = store::inspect_preparation(
            &prepared.database,
            prepared.records[0].intent.id,
            Some(&prepared.header),
            read(),
        )
        .expect("valid store-level linked history");
        let snapshot = prepared.snapshot();
        assert!(matches!(
            release::inspect_preparations(prepared.request(&prepared.selections), batch_options()),
            Err(release::PreparationBatchError::Evidence)
        ));
        assert_eq!(
            prepared.snapshot(),
            snapshot,
            "reader must not repair altered evidence"
        );
        let after = store::inspect_preparation(
            &prepared.database,
            prepared.records[0].intent.id,
            Some(&prepared.header),
            read(),
        )
        .expect("same writer coordinates after refusal");
        assert_eq!(after.record, historical.record);
        assert_eq!(after.journal.header, historical.journal.header);
    }
    replace_check(&prepared, original);
    prepared.inspect();
}

#[test]
fn corrupted_stored_check_envelopes_and_coverage_fail_after_hashes_and_linkage_are_rebound() {
    let prepared = Prepared::with_checker(false);
    let original = &prepared.records[0]
        .check
        .as_ref()
        .expect("real producer check")
        .evidence;
    for field in [
        "duplicate",
        "unknown",
        "tuple-output",
        "scanner-profile",
        "encoded-profile",
        "filename-profile",
        "generic-rules-profile",
        "decoder-profile",
        "decoder-goal",
        "findings",
        "dropped-file",
        "duplicate-file",
        "embedded-path",
    ] {
        let mut value: Value = serde_json::from_str(original).expect("actual produced evidence");
        match field {
            "duplicate" => {}
            "unknown" => value["unexpected"] = json!(true),
            "tuple-output" => {
                let output = value["disclosure"]["output_identity"].clone();
                value["disclosure"]["output_identity"] =
                    json!([output["format"], output["sha256"]]);
            }
            "scanner-profile" => {
                value["disclosure"]["scanner"] = json!("other-scanner/1");
            }
            "encoded-profile" => {
                value["disclosure"]["encoded_profile"] = json!("other-encoded/1");
            }
            "filename-profile" => {
                value["disclosure"]["filename_profile"] = json!("other-filenames/1");
            }
            "generic-rules-profile" => {
                value["disclosure"]["generic_rules_sha256"] =
                    json!(Sha256Digest::of(b"other generic rules"));
            }
            "decoder-profile" => {
                let files = value["disclosure"]["files"]
                    .as_array_mut()
                    .expect("coverage");
                let decoded = files
                    .iter_mut()
                    .find_map(|file| file["decoding"].as_object_mut())
                    .expect("structured decoder coverage");
                let (_, profile) = decoded.iter_mut().next().expect("one decoder");
                let profile = profile.as_object_mut().expect("decoder profile");
                let key = profile
                    .keys()
                    .find(|key| key.ends_with("profile"))
                    .cloned()
                    .expect("profile coordinate");
                profile[&key] = json!("other-decoder/1");
            }
            "decoder-goal" => {
                value["disclosure"]["files"][0]["decoding"] = json!({
                    "javascript-syntax": {
                        "profile": gazette_sdk::javascript::PROFILE,
                        "goal": "worker"
                    }
                });
            }
            "findings" => {
                value["disclosure"]["findings"] = json!([{
                    "path":"model.json", "rule":"rule:secret", "class":"secret", "representation":"text"
                }]);
                value["disclosure"]["findings_sha256"] = json!(Sha256Digest::of(
                    &serde_json::to_vec(&value["disclosure"]["findings"])
                        .expect("complete changed findings")
                ));
            }
            "dropped-file" => {
                value["disclosure"]["files"]
                    .as_array_mut()
                    .expect("coverage")
                    .remove(0);
            }
            "duplicate-file" => {
                let file = value["disclosure"]["files"][0].clone();
                value["disclosure"]["files"]
                    .as_array_mut()
                    .expect("coverage")
                    .push(file);
            }
            "embedded-path" => {
                value["disclosure"]["embedded"] = json!([{
                    "path":"never-emitted.txt", "depth":1, "representation":"percent-decoded",
                    "sha256":Sha256Digest::of(b"text"), "bytes":4, "media_type":null, "decoding":"utf8-text"
                }])
            }
            _ => unreachable!("closed corruption cases"),
        }
        let evidence = if field == "duplicate" {
            // Duplicate is nested: a parser that silently keeps the last value
            // would retain a plausible clean report with unchanged semantics.
            original.replacen("\"findings\":[]", "\"findings\":[],\"findings\":[]", 1)
        } else {
            serde_json::to_string(&value).expect("changed fixture envelope")
        };
        assert_ne!(
            evidence, *original,
            "corruption must actually alter the retained record"
        );
        replace_check(&prepared, &evidence);
        let snapshot = prepared.snapshot();
        let error =
            release::inspect_preparations(prepared.request(&prepared.selections), batch_options())
                .expect_err("forged digest equality is not complete native evidence");
        match field {
            "duplicate" => assert!(matches!(
                error,
                release::PreparationBatchError::Inspection(_)
            )),
            "dropped-file" | "embedded-path" => {
                assert!(matches!(error, release::PreparationBatchError::Mismatch))
            }
            _ => assert!(
                matches!(error, release::PreparationBatchError::Evidence),
                "{field}: {error}"
            ),
        }
        assert_eq!(
            prepared.snapshot(),
            snapshot,
            "verifier must not repair {field}"
        );
        assert_eq!(
            store::inspect_journal(&prepared.database, Some(&prepared.header), read())
                .expect("no writer generation effect")
                .header,
            prepared.header
        );
    }
}

#[test]
fn rebound_zero_exclusion_certificate_values_refuse_historical_admission() {
    let prepared = Prepared::with_qualification(true, None, None, None, true);
    let original = &prepared.records[0]
        .check
        .as_ref()
        .expect("qualified producer check")
        .evidence;
    for field in ["profile", "removed"] {
        let mut value: Value = serde_json::from_str(original).expect("actual certificate evidence");
        let requirements = &mut value["technical_certificate"]["requirements"];
        match field {
            "profile" => requirements["exclusion_profile"] = json!("other-exclusion-profile/1"),
            "removed" => requirements["removed"] = json!(["guide/index.html"]),
            _ => unreachable!("closed zero-exclusion corruptions"),
        }
        let evidence = serde_json::to_string(&value).expect("rebound certificate fixture");
        replace_check(&prepared, &evidence);
        let snapshot = prepared.snapshot();
        assert!(matches!(
            release::inspect_preparations(prepared.request(&prepared.selections), batch_options()),
            Err(release::PreparationBatchError::Evidence)
        ));
        assert_eq!(
            prepared.snapshot(),
            snapshot,
            "historical refusal must not repair {field}"
        );
    }
}

#[test]
fn missing_completion_stays_incomplete_even_when_native_git_commit_still_exists() {
    let prepared = Prepared::new();
    prepared.inspect();
    let connection =
        rusqlite::Connection::open(&prepared.database).expect("disposable history fault setup");
    assert_eq!(
        connection
            .execute(
                "DELETE FROM gazette_preparation_commits WHERE intent_id=?1",
                [prepared.records[0].intent.id.as_bytes()]
            )
            .expect("remove only selected fixture completion"),
        1
    );
    connection.close().expect("close test-only writer");
    let snapshot = prepared.snapshot();
    assert!(matches!(
        release::inspect_preparations(prepared.request(&prepared.selections), batch_options()),
        Err(release::PreparationBatchError::Incomplete)
    ));
    let inspection = store::inspect_preparation(
        &prepared.database,
        prepared.records[0].intent.id,
        Some(&prepared.header),
        read(),
    )
    .expect("historical pending worktree remains readable");
    assert!(
        inspection
            .record
            .expect("original preparation")
            .worktree
            .expect("original recipe")
            .completion
            .is_none()
    );
    assert_eq!(
        prepared.snapshot(),
        snapshot,
        "no adoption or synthesized completion"
    );
}
