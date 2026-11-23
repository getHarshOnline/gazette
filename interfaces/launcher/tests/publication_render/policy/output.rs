//! The configured output checker receives every retained byte, not a review projection.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use gazette_sdk::{build, process, publication::content::Disposition, source};
use serde_json::Value;

use super::{Fixture, PAGE, acquire, configured, fixture, fs, permitted, selection, tempdir};

fn render<'s>(
    selected: &build::SelectedPublication<'_, 's>,
    model_output: &str,
) -> build::PublicationRender<'s> {
    let model_output = model_output.parse().expect("machine output");
    build::render_selected_publication(
        selected,
        build::SiteRenderOptions {
            render_year: None,
            model_output: Some(&model_output),
            graph: None,
            limits: Default::default(),
        },
        Default::default(),
    )
    .expect("complete retained output")
}

#[test]
fn output_checker_receives_complete_selected_bytes_and_rechecks_frozen_output() {
    const BINARY: &[u8] = &[0, 255, 13, 10, 128, 0, 42];
    let published = PAGE.replace("lifecycle='draft'", "lifecycle='published'");
    let fixture = Fixture::new(&[
        ("one/page.md", published.as_bytes()),
        ("one/archive.zip", BINARY),
        ("one/.private.txt", b"hidden retained output\r\n"),
        (
            "theme/templates/alternate.html",
            b"<main>Alternate {{ content.title }} {{ body_html | safe }}</main>",
        ),
    ]);
    let observation = tempdir().expect("private output-check observation");
    let result = observation.path().join("output.json");
    let (configuration, command) = configured(&fixture, &result, 0);
    let configuration = configuration.replace(
        "authored=[{source='source:one',target='one'}]",
        "authored=[{source='source:one',target='one',static_assets={route_prefix='/assets/'}}]",
    );
    let inputs = acquire(&configuration);
    let parsed = inputs
        .parse_publication(Default::default(), &gazette_adapter_markdown::Comrak)
        .expect("retained native syntax");
    let resolved = parsed
        .resolve(Default::default())
        .expect("resolved publication");
    let mut decisions = selection(&resolved);
    for path in ["one/archive.zip", "one/.private.txt"] {
        decisions.assets.insert(
            path.parse().expect("exact selected asset"),
            Disposition::Include,
        );
    }
    let expected_selection = serde_json::to_value(&decisions).expect("complete selection intent");
    let selected = resolved.into_selected(decisions).expect("closed candidate");
    let review =
        build::review_selected(&selected, Default::default()).expect("separate candidate review");
    let rendered = render(&selected, "model.json");
    let tree = rendered.site().output();
    let identity = tree.inventory().identity();
    assert_eq!(
        tree.file(&"assets/archive.zip".parse().expect("binary output")),
        Some(BINARY)
    );
    assert_eq!(
        tree.file(&"assets/.private.txt".parse().expect("hidden output")),
        Some(b"hidden retained output\r\n".as_slice())
    );
    let checked =
        build::check_output_policy(&rendered, permitted(), None).expect("real output checker");
    let received = fs::read(&result).expect("actual child input");
    let request: Value = serde_json::from_slice(&received).expect("exact output-check JSON");
    assert_eq!(request["format"], "gazette-output-policy/1");
    assert_eq!(request["mode"], "local");
    assert_eq!(
        request["acquisition"],
        serde_json::to_value(inputs.inspect()).expect("native acquisition")
    );
    assert_eq!(request["selection"], expected_selection);
    assert_eq!(request["preview_urls"], Value::Null);
    assert_eq!(
        request["render"],
        serde_json::to_value(rendered.site().evidence()).expect("native render evidence")
    );
    assert_eq!(
        request["inventory"],
        serde_json::to_value(tree.inventory()).expect("complete inventory")
    );
    assert_eq!(
        request["media_types"],
        serde_json::to_value(rendered.site().media_types()).expect("producer media")
    );
    let supplied = request["files_base64"]
        .as_object()
        .expect("complete byte map");
    let expected_files = tree
        .inventory()
        .entries()
        .iter()
        .filter_map(|entry| {
            tree.file(&entry.path).map(|bytes| {
                (
                    entry.path.as_str().to_owned(),
                    Value::String(STANDARD.encode(bytes)),
                )
            })
        })
        .collect::<serde_json::Map<String, Value>>();
    assert_eq!(
        supplied, &expected_files,
        "no hidden/binary file omission or extra invented entry"
    );
    for (path, encoded) in supplied {
        let decoded = STANDARD
            .decode(encoded.as_str().expect("standard padded Base64"))
            .expect("faithful byte decoding");
        assert_eq!(
            decoded.as_slice(),
            tree.file(&path.parse().expect("exact output path"))
                .expect("retained bytes")
        );
    }
    assert!(std::ptr::eq(checked.rendered(), &rendered));
    assert_eq!(checked.output(), identity);
    assert_eq!(
        checked.invocation().request_sha256,
        source::Sha256Digest::of(&received)
    );
    assert_ne!(
        checked.invocation().request_sha256,
        review.sha256(),
        "candidate review cannot substitute for complete output bytes"
    );
    assert_eq!(
        checked.invocation().command_sha256,
        source::Sha256Digest::of(&serde_json::to_vec(&command).expect("selected command"))
    );
    assert_eq!(
        serde_json::to_value(&checked).expect("private output receipt"),
        serde_json::json!({
            "status":"output-policy-check-passed-unverified", "invocation":checked.invocation(), "output":identity,
            "candidate_check_sha256": null, "response": null, "policy_source": null
        })
    );

    fs::write(
        fixture.directory.path().join("one/archive.zip"),
        b"changed binary source",
    )
    .expect("disposable source edit");
    fs::write(
        fixture.directory.path().join("theme/templates/page.html"),
        b"changed native template",
    )
    .expect("disposable template edit");
    fs::write(&result, b"second invocation must replace this sentinel")
        .expect("fresh observation sentinel");
    let repeated = build::check_output_policy(
        &rendered,
        process::Options {
            max_input_bytes: received.len(),
            ..permitted()
        },
        None,
    )
    .expect("exact stdin ceiling and retained output after native changes");
    assert_eq!(
        fs::read(&result).expect("fresh second child observation"),
        received
    );
    assert_eq!(repeated.invocation(), checked.invocation());
    assert_eq!(tree.inventory().identity(), identity);
    for entry in tree.inventory().entries() {
        let Some(bytes) = tree.file(&entry.path) else {
            continue;
        };
        let text = String::from_utf8_lossy(bytes);
        for private in [
            "output-policy-check-passed-unverified",
            "gazette-output-policy/1",
            command.program.as_str(),
        ] {
            assert!(
                !text.contains(private),
                "{} leaked checker receipt/configuration",
                entry.path
            );
        }
    }

    let alternate = render(&selected, "alternate-model.json");
    assert_ne!(alternate.site().output().inventory().identity(), identity);
    fs::write(&result, b"alternate invocation sentinel").expect("fresh alternate observation");
    let changed = build::check_output_policy(&alternate, permitted(), None)
        .expect("different retained render selection");
    assert_ne!(
        changed.invocation().request_sha256,
        checked.invocation().request_sha256
    );
    assert_ne!(fs::read(&result).expect("actual changed input"), received);
}

#[test]
fn output_checker_refuses_permission_and_incomplete_stdin_before_child_effects() {
    let fixture = fixture();
    let observation = tempdir().expect("disposable checker result");
    let result = observation.path().join("output.json");
    let (configuration, _) = configured(&fixture, &result, 0);
    let inputs = acquire(&configuration);
    let rendered = super::super::render(&inputs).expect("unselected complete local render");
    let error = build::check_output_policy(&rendered, Default::default(), None)
        .expect_err("explicit output-check permission");
    assert!(matches!(
        error,
        build::PolicyCheckError::Refused(process::Error {
            failure: process::Failure::Permission,
            cleanup: process::Cleanup::NotStarted,
            ..
        })
    ));
    assert!(!result.exists());
    build::check_output_policy(&rendered, permitted(), None)
        .expect("measure actual complete stdin");
    let received = fs::read(&result).expect("complete child input");
    let request: Value = serde_json::from_slice(&received).expect("unselected request");
    assert_eq!(request["selection"], Value::Null);
    let sentinel = b"must survive preflight refusal";
    fs::write(&result, sentinel).expect("no stale success observation");
    let error = build::check_output_policy(
        &rendered,
        process::Options {
            max_input_bytes: received.len() - 1,
            ..permitted()
        },
        None,
    )
    .expect_err("one missing byte prevents all input delivery");
    assert!(matches!(
        error,
        build::PolicyCheckError::Encoding(gazette_sdk::render::Error::Limit("policy stdin bytes"))
    ));
    assert_eq!(fs::read(&result).expect("no child rewrite"), sentinel);

    let unconfigured = acquire(&fixture.config());
    let unconfigured =
        super::super::render(&unconfigured).expect("standalone render without checker");
    assert!(matches!(
        build::check_output_policy(&unconfigured, permitted(), None)
            .expect_err("no synthetic output-check approval"),
        build::PolicyCheckError::Unconfigured
    ));
    assert_eq!(
        fs::read(&result).expect("unconfigured operation does not spawn"),
        sentinel
    );
}

#[test]
fn nonzero_output_checker_retains_the_exact_output_invocation() {
    let fixture = fixture();
    let observation = tempdir().expect("disposable checker result");
    let result = observation.path().join("output.json");
    let (configuration, command) = configured(&fixture, &result, 7);
    let inputs = acquire(&configuration);
    let rendered = super::super::render(&inputs).expect("retained local output");
    let identity = rendered.site().output().inventory().identity();
    let error = build::check_output_policy(&rendered, permitted(), None)
        .expect_err("nonzero checker is not output approval");
    let build::PolicyCheckError::Rejected { invocation, exit } = error else {
        panic!("completed native checker must preserve its nonzero exit");
    };
    let received = fs::read(&result).expect("nonzero child received the output");
    assert_eq!(exit.code, Some(7));
    assert_eq!(
        invocation.request_sha256,
        source::Sha256Digest::of(&received)
    );
    assert_eq!(
        invocation.command_sha256,
        source::Sha256Digest::of(&serde_json::to_vec(&command).expect("exact configured command"))
    );
    let request: Value = serde_json::from_slice(&received).expect("complete rejected request");
    assert_eq!(
        request["inventory"],
        serde_json::to_value(rendered.site().output().inventory())
            .expect("unchanged full inventory")
    );
    assert_eq!(rendered.site().output().inventory().identity(), identity);
}

#[test]
fn explicit_framed_output_delivers_exact_inventory_order_and_binds_whole_stream() {
    const BINARY: &[u8] = &[255, 0, 13, 10, 128, 8];
    let published = PAGE.replace("lifecycle='draft'", "lifecycle='published'");
    let fixture = Fixture::new(&[
        ("one/page.md", published.as_bytes()),
        ("one/data.zip", BINARY),
    ]);
    let observation = tempdir().unwrap();
    let result = observation.path().join("framed.bin");
    let (config, _) = configured(&fixture, &result, 0);
    let config = format!("policy_output_transport='framed-v2'\n{config}").replace(
        "authored=[{source='source:one',target='one'}]",
        "authored=[{source='source:one',target='one',static_assets={route_prefix='/assets/'}}]",
    );
    let inputs = acquire(&config);
    let parsed = build::parse_publication(&inputs, Default::default()).unwrap();
    let resolved = parsed.resolve(Default::default()).unwrap();
    let mut decisions = selection(&resolved);
    decisions
        .assets
        .insert("one/data.zip".parse().unwrap(), Disposition::Include);
    let selected = resolved.into_selected(decisions).unwrap();
    let rendered = render(&selected, "model.json");
    let error = build::check_output_policy(
        &rendered,
        process::Options {
            max_input_bytes: 1,
            ..permitted()
        },
        None,
    )
    .expect_err("metadata ceiling before native effects");
    assert_eq!(error.code(), "policy_encoding");
    assert!(!result.exists());
    let checked = build::check_output_policy(&rendered, permitted(), None).unwrap();
    let bytes = fs::read(&result).unwrap();
    let magic = b"gazette-output-policy/2\n";
    assert!(bytes.starts_with(magic));
    let mut cursor = magic.len();
    let take_length = |cursor: &mut usize| {
        let n = u64::from_be_bytes(bytes[*cursor..*cursor + 8].try_into().unwrap());
        *cursor += 8;
        usize::try_from(n).unwrap()
    };
    let header_len = take_length(&mut cursor);
    let header: Value = serde_json::from_slice(&bytes[cursor..cursor + header_len]).unwrap();
    cursor += header_len;
    assert_eq!(header["format"], "gazette-output-policy/2");
    assert!(header.get("files_base64").is_none());
    let output = rendered.site().output();
    assert_eq!(
        header["inventory"],
        serde_json::to_value(output.inventory()).unwrap()
    );
    for entry in output.inventory().entries() {
        if let Some(payload) = output.file(&entry.path) {
            let length = take_length(&mut cursor);
            assert_eq!(length, payload.len());
            assert_eq!(&bytes[cursor..cursor + length], payload);
            cursor += length;
        }
    }
    assert_eq!(
        cursor,
        bytes.len(),
        "no omitted members or trailing frame bytes"
    );
    assert_eq!(
        checked.invocation().request_sha256,
        source::Sha256Digest::of(&bytes)
    );
    assert_eq!(checked.output(), output.inventory().identity());
}
