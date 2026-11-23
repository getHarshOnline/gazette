//! Pure preparation prerequisites precede native source acquisition in CLI and SDK.

use super::*;

fn denied(flag: &str) -> build::LocalPreparationRunOptions {
    let mut options = permitted();
    match flag {
        "--create-stage" => options.preparation.stage.allow_stage = false,
        "--record-preparation" => options.preparation.stage.record.allow_record = false,
        "--advance-writer-generation" => {
            options.preparation.stage.writer.allow_generation_advance = false;
        }
        "--reserve-local-coordination" => {
            options
                .preparation
                .stage
                .writer
                .session
                .allow_local_reservation = false;
        }
        "--allow-coordination-files" => {
            options
                .preparation
                .stage
                .writer
                .session
                .read
                .allow_coordination_files = false;
        }
        "--initialize-worktree" => options.preparation.worktree.allow_initialize = false,
        _ => panic!("unknown fixture permission"),
    }
    options
}

fn report(
    request: &Value,
    options: build::LocalPreparationRunOptions,
    expected_code: &str,
) -> Value {
    let outcome = recipe(request).execute(options);
    let error = outcome.as_ref().expect_err("native operation must refuse");
    let build::LocalPreparationRunError::Before { code, .. } = error else {
        panic!("pure preflight/acquisition must precede preparation: {error:?}");
    };
    assert_eq!(*code, expected_code);
    serde_json::to_value(build::LocalPreparationRunReport::new(&outcome))
        .expect("shared native failure projection")
}

#[test]
fn native_permissions_win_before_malformed_workspace_or_unavailable_sources() {
    let fixture = published_fixture(&[]);
    let root = private_directory();
    let journal = initialize(root.path());
    let baseline = fs::read(&journal.database).expect("journal baseline");
    let parent = private_directory();
    let config = fixture.config();
    let missing = fixture.directory.path().join("absent-repository");
    let inaccessible = config.replace(
        fixture.directory.path().to_str().expect("fixture root"),
        missing.to_str().expect("missing source root"),
    );
    assert_ne!(inaccessible, config, "real source selections were replaced");
    assert!(!missing.exists());
    for config in ["not valid workspace = [", inaccessible.as_str()] {
        let request = input(config, root.path(), journal.header, parent.path());
        for flag in [
            "--create-stage",
            "--record-preparation",
            "--advance-writer-generation",
            "--reserve-local-coordination",
            "--allow-coordination-files",
            "--initialize-worktree",
        ] {
            let expected = report(&request, denied(flag), "local_preparation_preflight");
            let arguments = ARGUMENTS
                .iter()
                .copied()
                .filter(|argument| *argument != flag)
                .collect::<Vec<_>>();
            let (output, actual) = result(&request, &arguments);
            assert!(!output.status.success(), "missing {flag}");
            assert_eq!(actual, expected, "same early native refusal for {flag}");
        }

        // With every permission present, the same request reaches the actual acquisition
        // owner and reports its exact syntax/source failure, rather than always refusing.
        let sites = recipe(&request)
            .preparations()
            .map(|(site, _)| site.clone())
            .collect::<Vec<_>>();
        let native =
            build::acquire_publications(config, &sites, Default::default(), Default::default())
                .expect_err("actual malformed workspace or required-source acquisition failure");
        assert_ne!(native.code(), "local_preparation_preflight");
        let expected = report(&request, permitted(), native.code());
        let (output, actual) = result(&request, ARGUMENTS);
        assert!(!output.status.success());
        assert_eq!(actual, expected);
    }
    for site in ["other", "one"] {
        unchanged(&journal, id(site), &baseline);
    }
    assert_eq!(
        fs::read_dir(parent.path()).expect("stage parent").count(),
        0
    );
    assert!(!missing.exists(), "no source fallback or recreation");
}

#[test]
fn disclosure_ceiling_is_validated_before_workspace_parsing() {
    let root = private_directory();
    let journal = initialize(root.path());
    let baseline = fs::read(&journal.database).expect("journal baseline");
    let parent = private_directory();
    let request = input(
        "not valid workspace = [",
        root.path(),
        journal.header,
        parent.path(),
    );
    let mut options = permitted();
    options.preparation.disclosure.rules = 0;
    let pure = build::LocalPreparationRequest {
        output_policy: None,
        qualification: None,
        coordinator: None,
        stage: build::PreparationRequest {
            journal: root.path(),
            expected: journal.header,
            parent: parent.path(),
            name: "other",
            id: id("other"),
        },
        metadata: &metadata(),
        parent_source: None,
        disclosure: &disclosure_policy(),
    }
    .validate(options.preparation)
    .expect_err("native disclosure policy exceeds zero rule ceiling");
    assert!(matches!(
        pure.cause.as_ref(),
        build::LocalPreparationFailure::Checks(_)
    ));
    let actual = report(&request, options, "local_preparation_preflight");
    assert_eq!(actual["message"], pure.to_string());
    for site in ["other", "one"] {
        unchanged(&journal, id(site), &baseline);
    }
    assert_eq!(
        fs::read_dir(parent.path()).expect("stage parent").count(),
        0
    );
}

#[test]
fn duplicate_preparation_ids_are_rejected_for_distinct_site_recipes_before_execution() {
    let root = private_directory();
    let journal = initialize(root.path());
    let baseline = fs::read(&journal.database).expect("journal baseline");
    let parent = private_directory();
    let mut request = input(
        "not valid workspace = [",
        root.path(),
        journal.header,
        parent.path(),
    );
    recipe(&request); // Shape decoding deliberately does not parse workspace TOML.
    assert_ne!(request["sites"][0]["render"], request["sites"][1]["render"]);
    assert_ne!(
        request["sites"][0]["stage_name"],
        request["sites"][1]["stage_name"]
    );
    request["sites"][1]["preparation_id"] = request["sites"][0]["preparation_id"].clone();
    let native = build::LocalPreparationRecipe::parse_json(
        &serde_json::to_vec(&request).expect("complete request"),
        4 * 1024 * 1024,
        2,
    )
    .expect_err("distinct sites cannot share one immutable preparation identity");
    let output = cli(&request.to_string(), ARGUMENTS);
    assert!(!output.status.success());
    assert!(
        output.stdout.is_empty(),
        "decoder failure precedes execution/reporting"
    );
    let diagnostic = String::from_utf8(output.stderr).expect("redacted native diagnostic");
    assert!(
        diagnostic.contains("local_preparation_input"),
        "{diagnostic}"
    );
    assert!(
        diagnostic.contains(native),
        "same native decoder: {diagnostic}"
    );
    for site in ["other", "one"] {
        unchanged(&journal, id(site), &baseline);
    }
    assert_eq!(
        fs::read_dir(parent.path()).expect("stage parent").count(),
        0
    );
}
