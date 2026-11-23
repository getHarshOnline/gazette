//! Native custody keeps its operation's capacity even when preview delivery fails.

use super::*;

#[test]
fn documentation_event_preserves_large_native_custody_on_delivery_failure() {
    let event = serde_json::json!({"event":"documentation","custody":{"original":"x".repeat((1 << 20) + 1)}});
    let mut expected = serde_json::to_vec(&event).unwrap();
    expected.push(b'\n');
    let mut diagnostics = Vec::new();
    let failed = deliver_documentation_event(
        &event,
        |bytes| {
            assert_eq!(bytes, expected);
            Err(io::Error::from(io::ErrorKind::BrokenPipe))
        },
        &mut diagnostics,
    );
    assert_eq!(failed.unwrap_err().kind(), io::ErrorKind::BrokenPipe);
    let start = diagnostics.iter().position(|byte| *byte == b'\n').unwrap() + 1;
    assert_eq!(&diagnostics[start..], expected);
    diagnostics.clear();
    deliver_documentation_event(
        &event,
        |bytes| {
            assert_eq!(bytes, expected);
            Ok(())
        },
        &mut diagnostics,
    )
    .unwrap();
    assert!(diagnostics.is_empty());
}

struct NativeFixture(std::path::PathBuf);
impl Drop for NativeFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// Obtain the sealed receipt from its real native operation. A large configured
// brand label makes the original report large without thousands of native files.
fn large_native_receipt() -> (NativeFixture, gazette_sdk::build::DocumentationReceipt) {
    use gazette_sdk::{build, source};
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .join(format!(
            "preview-documentation-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    fs::create_dir(&root).unwrap();
    let fixture = NativeFixture(fs::canonicalize(root).unwrap());
    for directory in ["content", "docs", "theme/templates", "jobs"] {
        fs::create_dir_all(fixture.0.join(directory)).unwrap();
    }
    fs::write(fixture.0.join("content/index.md"),"+++\n[content]\nid='content:one'\nsite='site:one'\ntitle='One'\nlifecycle='draft'\nroute={path='/'}\nstructure={kind='page',family={kind='article'}}\n+++\n# One\n").unwrap();
    fs::write(fixture.0.join("theme/theme.toml"),"package='theme:one'\nnamespace='one'\ntemplates='templates'\nbrowser_theme_colors={light='#ffffff',dark='#000000'}\nrender={page='page.html'}\n").unwrap();
    fs::write(
        fixture.0.join("theme/templates/page.html"),
        "{{ body_html | safe }}",
    )
    .unwrap();
    fs::write(fixture.0.join("docs/start.md"), "# Start\n").unwrap();
    fs::write(
        fixture.0.join("docs/guide.md"),
        "# API\nOriginal provider words.\n",
    )
    .unwrap();
    fs::write(
        fixture.0.join("docs/generate.sh"),
        "/bin/cat \"$1\" > \"$2\"\n",
    )
    .unwrap();
    let program = fs::canonicalize("/bin/sh").unwrap();
    let executable = fs::read(&program).unwrap();
    let generated = format!(
        "input={{kind='generated',source='source:docs',recipe={{program={:?},executable={{bytes={},sha256='{}'}},arguments=[{{kind='input',path='generate.sh'}},{{kind='input',path='guide.md'}},{{kind='output'}}],version_arguments=['-c',\"printf 'preview-receipt/1\\n'\"],expected_version_sha256='{}',output={{kind='file',path='api.md'}},format='markdown'}}}}",
        program.to_str().unwrap(),
        executable.len(),
        source::Sha256Digest::of(&executable),
        source::Sha256Digest::of(b"preview-receipt/1\n")
    );
    let mut workspace = format!(
        "[build]\nsites=['site:one']\nlocal_drafts='include'\n[composition]\ntheme={{package='theme:one'}}\n[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n[[brands]]\nid='brand:one'\nname='{}'\n[[brands.publications]]\nsite='site:one'\npublic_base_url='https://one.example/'\nauthored=[{{source='source:content',target='content'}}]\ndocumentation_repository={{name='docs',path='/not-opened/docs',public_base_url='https://docs.example/',build={{target={{kind='mintlify',theme='mint',primary_color='#123456'}},modules=[{{project='project:library',slug='library',title='Library',manual={{source='source:docs'}},navigation=[{{kind='manual',title='Start',pages=[{{title='Start',route='library/start',input={{kind='file',path='start.md'}}}}]}},{{kind='reference',slug='reference',title='Reference',{generated}}}]}}]}}}}\n[[projects]]\nid='project:library'\nlifecycle='active'\nvisibility='public'\navailability='available'\n",
        "x".repeat((1 << 20) + 1)
    );
    for name in ["content", "docs", "theme"] {
        workspace.push_str(&format!(
            "[[sources]]\nid='source:{name}'\nlocation={{kind='filesystem',root={:?}}}\n",
            fixture.0.join(name).to_str().unwrap()
        ));
    }
    workspace.push_str(
        "[[theme_packages]]\npackage='theme:one'\nsource='source:theme'\nmanifest='theme.toml'\n",
    );
    let limits = build::PublicationSetAcquisitionLimits::default();
    let mut inputs = build::acquire_publications(
        &workspace,
        &["site:one".parse().unwrap()],
        gazette_sdk::toml::Limits {
            source_bytes: 4 << 20,
            ..Default::default()
        },
        limits,
    )
    .unwrap();
    let mut request = build::DocumentationGenerationRequest::new(fixture.0.join("jobs"));
    request.allow_execute = true;
    let receipt = build::generate_publication_documentation(
        &mut inputs,
        limits.sources,
        Default::default(),
        Some(&request),
        None,
        |execution| match execution {
            build::DocumentationPublicationExecution::Ready { receipts, .. } => receipts[0].clone(),
            build::DocumentationPublicationExecution::Refused { error, .. } => panic!("{error}"),
            build::DocumentationPublicationExecution::Native {
                execution: build::documentation_output::Execution::Delivered { response, .. },
                ..
            } => panic!("{}", String::from_utf8_lossy(&response.bytes)),
            _ => panic!("native receipt unavailable"),
        },
    );
    assert!(receipt.bytes().len() > 1 << 20);
    (fixture, receipt)
}

fn state(receipt: gazette_sdk::build::DocumentationReceipt) -> runner::State {
    runner::State {
        status: runner::Status::Local,
        input_mode: runner::InputMode::Pinned,
        generation: 1,
        stale: false,
        input_identity: preview::watch::InputIdentity {
            workspace: gazette_sdk::source::Sha256Digest::of(b"workspace"),
            sources: BTreeMap::new(),
            documentation: BTreeMap::new(),
        },
        build_selection: gazette_sdk::workspace::BuildSelection {
            sites: vec!["site:one".parse().unwrap()],
            origin: gazette_sdk::workspace::BuildOrigin::Invocation,
            local_drafts: Some(gazette_sdk::workspace::LocalDrafts::Include),
        },
        sites: BTreeMap::new(),
        documentation: vec![receipt],
    }
}

#[test]
fn runner_events_keep_large_original_receipts_with_bounded_metadata() {
    let (_fixture, receipt) = large_native_receipt();
    let current = state(receipt.clone());
    for event in [
        runner::Event::Ready {
            state: current.clone(),
        },
        runner::Event::Changed {
            state: current.clone(),
        },
        runner::Event::Recovered { state: current },
        runner::Event::RebuildFailed {
            generation: 1,
            stale: true,
            code: "fixture_refusal",
            documentation: vec![receipt.clone()],
        },
    ] {
        let mut expected = serde_json::to_vec(&event).unwrap();
        expected.push(b'\n');
        assert!(expected.len() > 1 << 20);
        let mut diagnostics = Vec::new();
        deliver_runner_event(
            event.clone(),
            |bytes| {
                assert_eq!(bytes, expected);
                Ok(())
            },
            &mut diagnostics,
        )
        .unwrap();
        assert!(diagnostics.is_empty());
        assert!(
            deliver_runner_event(
                event,
                |_| Err(io::Error::from(io::ErrorKind::BrokenPipe)),
                &mut diagnostics
            )
            .is_err()
        );
        let start = diagnostics.iter().position(|byte| *byte == b'\n').unwrap() + 1;
        assert_eq!(&diagnostics[start..], expected);
    }
    let mut oversized = state(receipt);
    oversized.build_selection.sites =
        vec![format!("site:{}", "x".repeat(1 << 20)).parse().unwrap()];
    let mut diagnostics = Vec::new();
    assert!(
        deliver_runner_event(
            runner::Event::Ready {
                state: oversized.clone()
            },
            |_| panic!("oversized metadata must not be delivered"),
            &mut diagnostics
        )
        .is_err()
    );
    let start = diagnostics.iter().position(|byte| *byte == b'\n').unwrap() + 1;
    let retained: serde_json::Value = serde_json::from_slice(&diagnostics[start..]).unwrap();
    assert_eq!(retained["status"], "preview-metadata-refused");
    assert_eq!(
        retained["documentation"],
        serde_json::to_value(&oversized.documentation).unwrap()
    );
    let mut oversized_header = state(oversized.documentation[0].clone());
    oversized_header.documentation[0].site =
        format!("site:{}", "x".repeat(1 << 20)).parse().unwrap();
    assert!(
        deliver_runner_event(
            runner::Event::Ready {
                state: oversized_header
            },
            |_| panic!("receipt owner labels belong to the metadata ceiling"),
            &mut Vec::new(),
        )
        .is_err()
    );
    // Removing receipts cannot bypass the original ordinary-event ceiling either.
    oversized.documentation.clear();
    assert!(emit_runner(runner::Event::Ready { state: oversized }).is_err());
}
