//! Real configured-output CLI effects, native history and independent delivery evidence.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Output,
};

use gazette_sdk::{build, source, store, workspace::Key};
use serde_json::{Value, json};

use super::super::{fs, sites, tempdir};
use super::{Fixture, configuration, initialize, session};

const SELECTED: [&str; 2] = ["site:one", "site:other"];
const PERMISSIONS: [&str; 7] = [
    "--local",
    "--allow-read",
    "--allow-write-output",
    "--allow-record-history",
    "--advance-writer-generation",
    "--reserve-local-coordination",
    "--allow-coordination-files",
];

fn key(value: &str) -> Key {
    value.parse().expect("exact selected native identity")
}

fn ids(label: &str) -> BTreeMap<Key, source::Sha256Digest> {
    SELECTED
        .into_iter()
        .map(|site| {
            (
                key(site),
                source::Sha256Digest::of(format!("{label}:{site}").as_bytes()),
            )
        })
        .collect()
}

fn configured(fixture: &Fixture, parent: &Path) -> ([PathBuf; 2], String) {
    let roots = [parent.join("one"), parent.join("other")];
    for root in &roots {
        fs::create_dir(root).expect("existing native destination");
        fs::write(root.join("user.txt"), b"independent owner bytes").expect("user sentinel");
    }
    let _repository =
        git2::Repository::init(&roots[0]).expect("actual unrelated Git administration");
    fs::write(roots[1].join(".git"), b"gitdir: ../owner-admin\n").expect("unrelated Git pointer");
    let config = configuration(fixture, &roots[0]).replacen(
        "site='site:other'\npublic_base_url",
        &format!(
            "site='site:other'\noutput={{directory={:?}}}\npublic_base_url",
            roots[1].to_str().unwrap()
        ),
        1,
    );
    (roots, config)
}

fn arguments(
    root: &Path,
    header: store::JournalHeader,
    ids: &BTreeMap<Key, source::Sha256Digest>,
) -> Vec<String> {
    let mut args = vec![
        "materialize-sites".to_owned(),
        root.to_str().unwrap().to_owned(),
        "--expected-header".to_owned(),
        serde_json::to_string(&header).unwrap(),
    ];
    args.extend(PERMISSIONS.map(str::to_owned));
    for (site, id) in ids {
        args.extend(["--operation".to_owned(), format!("{site}={id}")]);
    }
    args
}

fn invoke(recipe: &Value, args: &[String]) -> Output {
    super::super::cli(
        &recipe.to_string(),
        &args.iter().map(String::as_str).collect::<Vec<_>>(),
    )
}

fn frames(output: &Output) -> Vec<Value> {
    std::str::from_utf8(&output.stdout)
        .expect("JSON-only stdout")
        .lines()
        .map(|line| serde_json::from_str(line).expect("whole JSONL response frame"))
        .collect()
}

fn snapshot(root: &Path) -> source::TreeIdentity {
    source::inspect(root, None, Default::default())
        .expect("whole native tree")
        .identity()
}

fn acquired(
    frames: &[Value],
    expected: store::JournalHeader,
    root: &Path,
    ids: &BTreeMap<Key, source::Sha256Digest>,
) -> store::JournalHeader {
    assert_eq!(frames.len(), 3, "reserved, acquired and terminal result");
    assert_eq!(frames[0]["event"], "reserved");
    assert_eq!(frames[0]["native_status"], "not_started");
    assert_eq!(frames[0]["close"]["status"], "not_acquired");
    assert_eq!(frames[0]["custody"]["acquired_header"], Value::Null);
    assert_eq!(frames[1]["event"], "acquired");
    assert_eq!(frames[1]["close"]["status"], "held");
    let header: store::JournalHeader =
        serde_json::from_value(frames[1]["custody"]["acquired_header"].clone())
            .expect("retained native acquisition coordinates");
    assert_eq!(
        header,
        store::JournalHeader {
            writer_fence: expected.writer_fence + 1,
            ..expected
        }
    );
    for frame in frames {
        assert_eq!(frame["custody"]["root"], json!(root));
        assert_eq!(frame["custody"]["expected_header"], json!(expected));
        assert_eq!(frame["custody"]["operations"], json!(ids));
        assert_eq!(
            frame["custody"]["request_sha256"],
            frames[0]["custody"]["request_sha256"]
        );
        frame["custody"]["request_sha256"]
            .as_str()
            .unwrap()
            .parse::<source::Sha256Digest>()
            .expect("native exact request digest");
    }
    assert_eq!(frames[2]["custody"]["acquired_header"], json!(header));
    header
}

fn history(
    database: &Path,
    root: &Path,
    header: store::JournalHeader,
) -> store::MaterializationInspection {
    store::inspect_materialization(
        database,
        &root.canonicalize().unwrap(),
        Some(&header),
        session().read,
    )
    .expect("independently reopened native durable history")
}

#[test]
fn cli_materializes_two_sites_and_repeats_owned_bytes_under_one_explicitly_closed_writer() {
    let fixture = Fixture::new(&[]);
    let outputs = tempdir().unwrap();
    let (roots, config) = configured(&fixture, outputs.path());
    let git_before = snapshot(&roots[0].join(".git"));
    let pointer_before = fs::read(roots[1].join(".git")).unwrap();
    let source_before = snapshot(fixture.directory.path());
    let recipe = sites::request(&config, &SELECTED);
    let inputs = sites::acquire_set(&config, &SELECTED);
    let page = "page.html".parse().unwrap();
    let alias = "alias.html".parse().unwrap();
    let model = "model.json".parse().unwrap();
    let expected = build::render_publications(
        &inputs,
        sites::options(&SELECTED, &page, &alias, &model),
        Default::default(),
    )
    .unwrap();
    assert!(expected.is_complete());
    let (journal, initialized) = initialize();
    let mut expected_header = initialized.header;
    let mut previous_ids = None;
    let mut retained_tree = None;
    for generation in ["first-cli", "second-cli-owned-noop"] {
        let operation_ids = ids(generation);
        let output = invoke(
            &recipe,
            &arguments(journal.path(), expected_header, &operation_ids),
        );
        assert!(output.status.success(), "{output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
        let reports = frames(&output);
        let header = acquired(&reports, expected_header, journal.path(), &operation_ids);
        let terminal = &reports[2];
        assert_eq!(terminal["event"], "result");
        assert_eq!(terminal["native_status"], "materialization_completed");
        assert_eq!(terminal["details_complete"], true);
        assert_eq!(terminal["close"]["status"], "closed");
        assert_eq!(terminal["detail"].as_object().unwrap().len(), 2);
        for (site, root) in SELECTED.into_iter().zip(&roots) {
            let result = &terminal["detail"][site];
            let generated = expected.sites()[&key(site)]
                .as_ref()
                .unwrap()
                .site()
                .output();
            assert_eq!(result["output"]["site"], site);
            assert_eq!(result["output"]["generated"], json!(generated.inventory()));
            assert_eq!(result["history"]["inserted"], true);
            assert_eq!(result["history"]["journal"], json!(header));
            let stored = history(&initialized.database, root, header);
            assert!(stored.frontier.active.is_none());
            let record = stored.frontier.head.expect("actual completed history");
            assert_eq!(json!(record), result["history"]["record"]);
            assert_eq!(record.intent.id, operation_ids[&key(site)]);
            assert_eq!(
                record.intent.previous,
                previous_ids
                    .as_ref()
                    .map(|prior: &BTreeMap<Key, source::Sha256Digest>| prior[&key(site)])
            );
            record
                .completion
                .as_ref()
                .unwrap()
                .validate(&record.intent)
                .expect("independent native completion linkage");
            assert_eq!(result["output"]["inventory"], json!(record.intent.expected));
            for entry in generated.inventory().entries() {
                if let Some(bytes) = generated.file(&entry.path) {
                    assert_eq!(fs::read(root.join(entry.path.as_str())).unwrap(), bytes);
                } else {
                    assert!(root.join(entry.path.as_str()).is_dir());
                }
            }
            assert_eq!(
                fs::read(root.join("user.txt")).unwrap(),
                b"independent owner bytes"
            );
            assert!(!root.read_dir().unwrap().any(|entry| {
                entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".gazette-write-")
            }));
        }
        let current_tree = snapshot(outputs.path());
        if let Some(previous) = retained_tree {
            assert_eq!(
                current_tree, previous,
                "a new owned no-op intent does not change native bytes"
            );
        }
        retained_tree = Some(current_tree);
        previous_ids = Some(operation_ids);
        expected_header = header;
        assert_eq!(
            store::inspect_journal(&initialized.database, Some(&header), session().read)
                .unwrap()
                .header,
            header
        );
    }
    assert_eq!(snapshot(fixture.directory.path()), source_before);
    assert_eq!(snapshot(&roots[0].join(".git")), git_before);
    assert_eq!(fs::read(roots[1].join(".git")).unwrap(), pointer_before);
    assert_eq!(
        journal.path().read_dir().unwrap().count(),
        1,
        "explicit close releases private coordination"
    );
}

#[test]
fn cli_independent_permissions_refuse_before_generation_sources_history_and_output() {
    let fixture = Fixture::new(&[]);
    let outputs = tempdir().unwrap();
    let (roots, config) = configured(&fixture, outputs.path());
    let recipe = sites::request(&config, &SELECTED);
    let before = snapshot(outputs.path());
    let (journal, initialized) = initialize();
    let database_before = fs::read(&initialized.database).unwrap();
    // The selected source was genuinely retained when this recipe was authored,
    // but is now unavailable. The promised permission preflight must win first.
    drop(fixture);
    for (missing, code) in [
        ("--local", "--local"),
        ("--allow-read", "materialization_permission"),
        ("--allow-write-output", "materialization_permission"),
        ("--allow-record-history", "materialization_permission"),
        ("--advance-writer-generation", "store_writer_permission"),
        (
            "--reserve-local-coordination",
            "store_reservation_permission",
        ),
        (
            "--allow-coordination-files",
            "store_coordination_permission",
        ),
    ] {
        let mut args = arguments(journal.path(), initialized.header, &ids("denied"));
        args.retain(|arg| arg != missing);
        let output = invoke(&recipe, &args);
        assert!(!output.status.success(), "{missing}: {output:?}");
        assert!(
            output.stdout.is_empty(),
            "no pre-effect custody/native frame: {output:?}"
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(code),
            "{missing}: {output:?}"
        );
        assert_eq!(fs::read(&initialized.database).unwrap(), database_before);
        assert_eq!(snapshot(outputs.path()), before);
        for root in &roots {
            let inspected = history(&initialized.database, root, initialized.header);
            assert!(inspected.frontier.head.is_none() && inspected.frontier.active.is_none());
        }
        assert_eq!(journal.path().read_dir().unwrap().count(), 1);
    }
    let wrong = store::JournalHeader {
        writer_fence: initialized.header.writer_fence + 1,
        ..initialized.header
    };
    let output = invoke(
        &recipe,
        &arguments(journal.path(), wrong, &ids("wrong-header")),
    );
    assert!(!output.status.success(), "{output:?}");
    assert!(
        output.stderr.is_empty(),
        "native acquisition refusal is structured: {output:?}"
    );
    let reports = frames(&output);
    assert_eq!(
        reports.len(),
        2,
        "reservation and failed acquisition, never a ready writer"
    );
    assert_eq!(reports[0]["event"], "reserved");
    let result = &reports[1];
    assert_eq!(result["native_status"], "acquisition_failed");
    assert_eq!(result["details_complete"], true);
    assert_eq!(result["custody"]["expected_header"], json!(wrong));
    assert_eq!(result["custody"]["acquired_header"], Value::Null);
    assert_eq!(result["detail"]["phase"], "acquisition");
    assert_eq!(result["detail"]["code"], "store_journal_mismatch");
    assert_eq!(result["detail"]["generation"], "NotAttempted");
    assert_eq!(result["close"]["status"], "not_acquired");
    assert_eq!(fs::read(&initialized.database).unwrap(), database_before);
    assert_eq!(snapshot(outputs.path()), before);
    assert_eq!(journal.path().read_dir().unwrap().count(), 1);
}

#[test]
fn cli_duplicate_coordinates_and_undersized_input_or_response_refuse_before_writer_acquisition() {
    let fixture = Fixture::new(&[]);
    let outputs = tempdir().unwrap();
    let (roots, config) = configured(&fixture, outputs.path());
    let recipe = sites::request(&config, &SELECTED);
    let source_before = snapshot(fixture.directory.path());
    let before = snapshot(outputs.path());
    let (journal, initialized) = initialize();
    let database_before = fs::read(&initialized.database).unwrap();
    let operation_ids = ids("preflight");
    let base = arguments(journal.path(), initialized.header, &operation_ids);
    let mut duplicate_site = base.clone();
    duplicate_site.extend([
        "--operation".to_owned(),
        format!(
            "site:one={}",
            source::Sha256Digest::of(b"different operation")
        ),
    ]);
    let repeated = source::Sha256Digest::of(b"same operation two sites");
    let duplicate_ids = arguments(
        journal.path(),
        initialized.header,
        &SELECTED
            .into_iter()
            .map(|site| (key(site), repeated))
            .collect(),
    );
    let mut uppercase = base.clone();
    uppercase.extend([
        "--operation".to_owned(),
        format!("site:third={}", repeated.to_string().to_uppercase()),
    ]);
    let mut input_limit = base.clone();
    input_limit.extend(["--max-source-bytes".to_owned(), "1".to_owned()]);
    let mut response_limit = base.clone();
    response_limit.extend(["--max-response-bytes".to_owned(), "1".to_owned()]);
    for (args, code) in [
        (duplicate_site, "materialization_operation_duplicate"),
        (duplicate_ids, "materialization_operation_duplicate"),
        (uppercase, "lowercase SHA-256"),
        (input_limit, "publication_set_input_limit"),
        (response_limit, "materialization_response_reservation"),
    ] {
        let output = invoke(&recipe, &args);
        assert!(!output.status.success(), "{output:?}");
        assert!(output.stdout.is_empty(), "{output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(code),
            "{output:?}"
        );
        assert_eq!(fs::read(&initialized.database).unwrap(), database_before);
        assert_eq!(snapshot(outputs.path()), before);
        assert_eq!(snapshot(fixture.directory.path()), source_before);
        for root in &roots {
            let inspected = history(&initialized.database, root, initialized.header);
            assert!(inspected.frontier.head.is_none() && inspected.frontier.active.is_none());
        }
    }
}

#[test]
fn cli_reconstructed_same_id_refuses_self_predecessor_before_effect_and_closes_healthy_writer() {
    let fixture = Fixture::new(&[]);
    let outputs = tempdir().unwrap();
    let (roots, config) = configured(&fixture, outputs.path());
    let recipe = sites::request(&config, &SELECTED);
    let (journal, initialized) = initialize();
    let operation_ids = ids("original-cli");
    let first = invoke(
        &recipe,
        &arguments(journal.path(), initialized.header, &operation_ids),
    );
    assert!(first.status.success(), "{first:?}");
    let first_frames = frames(&first);
    let header = acquired(
        &first_frames,
        initialized.header,
        journal.path(),
        &operation_ids,
    );
    let prior = roots
        .iter()
        .map(|root| json!(history(&initialized.database, root, header).frontier))
        .collect::<Vec<_>>();
    let before = snapshot(outputs.path());
    // CLI owns no preserved original plan after explicit close. The new plan
    // observes the completed head and current bytes, making the reused ID its
    // own predecessor. Native payload validation refuses that cycle before SQL
    // effects or writer poisoning; the independent explicit close stays healthy.
    let repeated = invoke(&recipe, &arguments(journal.path(), header, &operation_ids));
    assert!(!repeated.status.success(), "{repeated:?}");
    assert!(
        repeated.stderr.is_empty(),
        "structured native failure is retained: {repeated:?}"
    );
    let reports = frames(&repeated);
    let next = acquired(&reports, header, journal.path(), &operation_ids);
    let result = &reports[2];
    assert_eq!(result["event"], "result");
    assert_eq!(result["details_complete"], true);
    assert_eq!(result["native_status"], "materialization_failed");
    assert_eq!(result["detail"]["requested"], json!(SELECTED));
    assert_eq!(result["detail"]["completed"], json!({}));
    assert_eq!(result["detail"]["failed"], "site:one");
    assert_eq!(result["detail"]["cause"]["phase"], "history");
    assert_eq!(
        result["detail"]["cause"]["id"],
        json!(operation_ids[&key("site:one")])
    );
    assert_eq!(
        result["detail"]["cause"]["cause"]["code"],
        "store_materialization_verification"
    );
    assert_eq!(result["detail"]["cause"]["effect"], "NotAttempted");
    assert_eq!(result["close"], json!({"status":"closed"}));
    assert_eq!(snapshot(outputs.path()), before);
    for (root, prior) in roots.iter().zip(prior) {
        assert_eq!(
            json!(history(&initialized.database, root, next).frontier),
            prior
        );
    }
    assert_eq!(
        journal.path().read_dir().unwrap().count(),
        1,
        "healthy explicit close releases custody after the pre-effect refusal"
    );
}

#[test]
fn cli_result_overflow_preserves_custody_closed_history_and_unavailable_details() {
    let fixture = Fixture::new(&[]);
    let outputs = tempdir().unwrap();
    let (roots, config) = configured(&fixture, outputs.path());
    let recipe = sites::request(&config, &SELECTED);
    let (journal, initialized) = initialize();
    let first_ids = ids("full-result");
    let first = invoke(
        &recipe,
        &arguments(journal.path(), initialized.header, &first_ids),
    );
    assert!(first.status.success(), "{first:?}");
    let first_frames = frames(&first);
    let header = acquired(
        &first_frames,
        initialized.header,
        journal.path(),
        &first_ids,
    );
    let lines = first
        .stdout
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    let maximum = (lines[0].len().max(lines[1].len()) + 1) * 2;
    assert!(
        lines[2].len() > maximum,
        "the actual native result exceeds compact custody, not a guessed DTO"
    );
    let operation_ids = ids("bounded-result");
    let mut args = arguments(journal.path(), header, &operation_ids);
    args.extend(["--max-response-bytes".to_owned(), maximum.to_string()]);
    let before = snapshot(outputs.path());
    let output = invoke(&recipe, &args);
    assert!(
        !output.status.success(),
        "delivery overflow is not process success: {output:?}"
    );
    assert!(
        output.stderr.is_empty(),
        "complete compact JSONL fallback: {output:?}"
    );
    let reports = frames(&output);
    let next = acquired(&reports, header, journal.path(), &operation_ids);
    let result = &reports[2];
    assert_eq!(result["event"], "delivery_incomplete");
    assert_eq!(result["native_status"], "materialization_completed");
    assert_eq!(result["details_complete"], false);
    assert_eq!(result["detail"], Value::Null);
    assert_eq!(result["close"]["status"], "closed");
    for line in output
        .stdout
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        assert!(
            line.len() < maximum,
            "each whole frame includes the framing byte"
        );
    }
    for (site, root) in SELECTED.into_iter().zip(&roots) {
        let stored = history(&initialized.database, root, next);
        assert!(stored.frontier.active.is_none());
        let head = stored.frontier.head.unwrap();
        assert_eq!(head.intent.id, operation_ids[&key(site)]);
        assert_eq!(head.intent.previous, Some(first_ids[&key(site)]));
        head.completion
            .as_ref()
            .unwrap()
            .validate(&head.intent)
            .unwrap();
    }
    assert_eq!(
        snapshot(outputs.path()),
        before,
        "repeat preserved native bytes despite unavailable delivery details"
    );
}
