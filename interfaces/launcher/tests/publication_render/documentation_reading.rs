//! Actual provider pages join discovery without becoming authored content nodes.

use super::*;

#[path = "documentation_reading/project_schema.rs"]
mod project_schema;

fn fixture_config() -> (Fixture, String) {
    fixture_config_with(&[])
}

fn fixture_config_with(overrides: &[(&str, &[u8])]) -> (Fixture, String) {
    let mut files: Vec<(&str, &[u8])> = vec![
        (
            "project/guide.md",
            b"# Library guide\n\nAuthored manual entrance.\n",
        ),
        (
            "project/titled.md",
            b"# Native API title\n\nRetained provider semantics.\n",
        ),
        ("project/untitled.md", b"Retained untitled API semantics.\n"),
    ];
    files.extend_from_slice(overrides);
    let fixture = Fixture::new(&files);
    let machine = curated_llms("collection:home", &[]).replace(
        "llms={enabled=true",
        "llms={coverage='all-pages',enabled=true",
    );
    let docs = "documentation_repository={name='provider-docs',path='/not-opened/provider-docs',public_base_url='https://docs.example/v2/',build={target={kind='mintlify',theme='mint',primary_color='#123456'},modules=[{project='project:provider',slug='library',title='Library',manual={source='source:provider'},navigation=[{kind='manual',title='Start',pages=[{title='Library guide',route='library/guide',input={kind='file',path='guide.md'}}]},{kind='reference',slug='titled',title='API',input={kind='artifact',source='source:provider',selection={kind='file',path='titled.md'},format='markdown'}},{kind='reference',slug='untitled',title='Untitled input',input={kind='artifact',source='source:provider',selection={kind='file',path='untitled.md'},format='markdown'}}]}]}}";
    let mut config = fixture.config().replacen(
        "public_base_url='https://one.example/mount/'",
        &format!("public_base_url='https://one.example/mount/'\nmachine={{search='lexical',{machine}}}\n{docs}"),
        1,
    );
    config.push_str(&format!(
        "\n[[projects]]\nid='project:provider'\nname='Provider library'\nlifecycle='active'\nvisibility='public'\navailability='available'\n[[sources]]\nid='source:provider'\nlocation={{kind='repository',root={:?},commit='{}',subdirectory='project'}}\n",
        fixture.directory.path().to_str().unwrap(), fixture.commit,
    ));
    (fixture, config)
}

#[test]
fn provider_catalog_enters_search_and_llms_with_real_routes_and_no_authored_identity() {
    let (_fixture, config) = fixture_config();
    let documentation = build::acquire_documentation(
        &config,
        &"site:one".parse().unwrap(),
        Default::default(),
        build::PublicationSetAcquisitionLimits::default().sources,
    )
    .expect("native provider capture");
    let assembly = build::assemble_documentation(&documentation, Default::default())
        .expect("actual retained provider assembly");
    let catalog = assembly
        .catalog_capture()
        .expect("validated provider catalog");
    assert_eq!(catalog.entries().len(), 3);
    let mut inputs = acquire(&config);
    let original_ids = {
        let parsed = inputs
            .parse_publication(Default::default(), &gazette_adapter_markdown::Comrak)
            .unwrap();
        let resolved = parsed.resolve(Default::default()).unwrap();
        resolved
            .authored()
            .content()
            .graph()
            .nodes()
            .map(|node| node.id.clone())
            .collect::<Vec<_>>()
    };
    inputs
        .install_documentation_catalog(catalog.clone())
        .expect("same owner/configuration");
    let rendered = render(&inputs).expect("native publication with provider discovery");
    let tree = rendered.site().output();
    let search: Value = serde_json::from_slice(bytes(tree, "search.json")).unwrap();
    let records: Vec<_> = search
        .as_array()
        .unwrap()
        .iter()
        .filter(|record| record.get("documentation_page").is_some())
        .collect();
    assert_eq!(records.len(), 3);
    let llms = std::str::from_utf8(bytes(tree, "llms.txt")).unwrap();
    for entry in catalog.entries() {
        let url = catalog.canonical_url(entry).unwrap().unwrap();
        let record = records
            .iter()
            .find(|record| record["route"]["url"] == url)
            .unwrap();
        assert_eq!(record["route"]["canonical_url"], url);
        assert_eq!(record["content"]["id"], url);
        assert_eq!(record["content"]["documentation"], true);
        assert!(record["content"].get("lifecycle").is_none());
        assert!(record["content"].get("author").is_none());
        assert_eq!(record["projects"][0]["project"]["id"], "project:provider");
        assert!(record["projects"][0].get("relationship").is_none());
        assert_eq!(record["destination"]["kind"], "external");
        assert_eq!(
            record["documentation_page"]["title"],
            serde_json::to_value(&entry.page.title).unwrap()
        );
        assert_eq!(
            record["body"],
            entry.page.body_text.as_deref().unwrap_or("")
        );
        let label = entry
            .page
            .title
            .as_deref()
            .unwrap_or(entry.page.route.as_str());
        assert_eq!(record["content"]["title"], label);
        let line = llms
            .lines()
            .find(|line| line.contains(&url))
            .expect("actual provider endpoint in llms");
        assert_eq!(
            line.matches(&url).count(),
            1,
            "no duplicate HTML link: {line}"
        );
        assert!(
            !line.contains(&format!("{url}.md")),
            "no invented alternate"
        );
    }
    assert_eq!(
        rendered.site().evidence().documentation_catalog,
        Some(catalog.evidence_sha256())
    );
    let parsed = inputs
        .parse_publication(Default::default(), &gazette_adapter_markdown::Comrak)
        .unwrap();
    let registry = parsed.resolve(Default::default()).unwrap();
    assert_eq!(
        registry
            .authored()
            .content()
            .graph()
            .nodes()
            .map(|node| node.id.clone())
            .collect::<Vec<_>>(),
        original_ids,
        "provider discovery does not add or replace authored graph nodes",
    );
    let parent = tempdir().expect("owned CLI output parent");
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments.extend(["--output-parent", parent.path().to_str().unwrap()]);
    let result = cli(&config, &arguments);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let receipt: Value = serde_json::from_slice(&result.stdout).unwrap();
    let directory = std::path::Path::new(receipt["directory"].as_str().unwrap());
    assert_eq!(
        fs::read(directory.join("search.json")).unwrap(),
        bytes(tree, "search.json")
    );
    assert_eq!(
        fs::read(directory.join("llms.txt")).unwrap(),
        bytes(tree, "llms.txt")
    );
}

#[test]
fn provider_preview_links_keep_canonical_metadata_and_respect_project_visibility() {
    let (_fixture, original) = fixture_config();
    for visibility in ["public", "unlisted", "private"] {
        let config = original
            .replace("visibility='public'", &format!("visibility='{visibility}'"))
            .replacen(
                "public_base_url='https://one.example/mount/'",
                "local_port=41235\npublic_base_url='https://one.example/mount/'",
                1,
            );
        let input = serde_json::json!({
            "format": "gazette-local-publication-set/1",
            "workspace": {"kind": "supplied", "source": config},
            "sites": [{"site": "site:one", "model_output": "model.json"}],
            "addressing": "local",
            "preview_link_bases": {"https://docs.example/v2/": "http://127.0.0.1:41234/v2/"}
        });
        let request = build::LocalPublicationRecipe::parse_json(
            &serde_json::to_vec(&input).unwrap(),
            1 << 20,
            1,
        )
        .unwrap();
        request
            .with_rendered(
                Default::default(),
                Default::default(),
                Default::default(),
                Default::default(),
                |set| {
                    assert!(set.is_complete());
                    let output = set
                        .sites()
                        .get(&"site:one".parse().unwrap())
                        .unwrap()
                        .as_ref()
                        .unwrap();
                    let tree = output.site().output();
                    let search: Value = serde_json::from_slice(bytes(tree, "search.json")).unwrap();
                    let records: Vec<_> = search
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter(|record| record.get("documentation_page").is_some())
                        .collect();
                    let llms = std::str::from_utf8(bytes(tree, "llms.txt")).unwrap();
                    if visibility == "public" {
                        assert_eq!(records.len(), 3);
                        for record in records {
                            let canonical = record["route"]["canonical_url"].as_str().unwrap();
                            assert!(canonical.starts_with("https://docs.example/v2/"));
                            assert_eq!(record["content"]["id"], canonical);
                            let reading = record["route"]["url"].as_str().unwrap();
                            assert!(reading.starts_with("http://127.0.0.1:41234/v2/"));
                            assert!(llms.contains(reading));
                            assert!(!llms.contains(canonical));
                        }
                    } else {
                        assert!(records.is_empty());
                        assert!(!llms.contains("docs.example/v2/"));
                        assert!(!llms.contains("127.0.0.1:41234/v2/"));
                    }
                },
            )
            .expect("same native recipe with explicit documentation link mapping");
    }
}

fn generated_fixture_config() -> (Fixture, String) {
    let (fixture, config) = fixture_config();
    let executable = fs::read("/bin/sh").expect("native POSIX shell");
    let generated = format!(
        "input={{kind='generated',source='source:provider',recipe={{program='/bin/sh',executable={{bytes={},sha256='{}'}},arguments=[{{kind='literal',value='-c'}},{{kind='literal',value={}}},{{kind='literal',value='reference-copy'}},{{kind='input',path='titled.md'}},{{kind='output'}}],environment={{}},version_arguments=['-c','printf fixture-documentation/1'],expected_version_sha256='{}',output={{kind='file',path='reference.md'}},format='markdown'}}}}",
        executable.len(),
        source::Sha256Digest::of(&executable),
        serde_json::to_string("/bin/cat \"$1\" > \"$2\"").unwrap(),
        source::Sha256Digest::of(b"fixture-documentation/1"),
    );
    let config = config.replace(
        "input={kind='artifact',source='source:provider',selection={kind='file',path='titled.md'},format='markdown'}",
        &generated,
    );
    (fixture, config)
}

fn cli_without_stdout(input: &str, arguments: &[&str]) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("native command with closed receipt pipe");
    drop(child.stdout.take());
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn multisite_cli_keeps_generated_documentation_when_stdout_closes() {
    let (_fixture, config) = generated_fixture_config();
    let stages = tempdir().expect("independent private generator parent");
    let recipe = serde_json::json!({
        "format": "gazette-local-publication-set/1",
        "workspace": { "kind": "supplied", "source": config },
        "sites": [{ "site": "site:one" }],
        "render_year": 2026,
        "documentation_generation": { "parent": stages.path(), "allow_execute": true }
    });
    let result = cli_without_stdout(
        &recipe.to_string(),
        &["render-sites", "--local", "--allow-execute"],
    );
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("output_io"));
    let receipt: Value = serde_json::from_slice(
        result
            .stderr
            .splitn(2, |byte| *byte == b'\n')
            .nth(1)
            .unwrap(),
    )
    .expect("complete publication inspection and original native delivery");
    let documents = receipt["documentation"].as_array().unwrap();
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0]["is_error"], false);
    let native = &documents[0]["response"];
    assert_eq!(native["status"], "documentation-assembled-local");
    let jobs = native["jobs"].as_array().unwrap();
    assert_eq!(jobs.len(), 1);
    let generated = std::path::Path::new(jobs[0]["output"].as_str().unwrap());
    assert_eq!(
        fs::read(generated.join("reference.md")).unwrap(),
        b"# Native API title\n\nRetained provider semantics.\n"
    );
    assert!(receipt["sites"]["site:one"]["inventory"]["entries"].is_array());
}

#[test]
fn direct_cli_keeps_generated_documentation_custody_through_render_stage_and_delivery() {
    let (_fixture, config) = generated_fixture_config();
    for (fail_output, lost_stdout) in [(false, false), (true, false), (false, true)] {
        let stages = tempdir().expect("independent private generator parent");
        let output = tempdir().expect("independent publication parent");
        let missing = output.path().join("does-not-exist");
        let output_parent = if fail_output {
            missing.as_path()
        } else {
            output.path()
        };
        let mut arguments = RENDER_ARGUMENTS.to_vec();
        arguments.extend([
            "--allow-execute",
            "--documentation-generation-parent",
            stages.path().to_str().unwrap(),
            "--output-parent",
            output_parent.to_str().unwrap(),
        ]);
        let result = if lost_stdout {
            cli_without_stdout(&config, &arguments)
        } else {
            cli(&config, &arguments)
        };
        assert_eq!(
            result.status.success(),
            !fail_output && !lost_stdout,
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let receipt_bytes = if lost_stdout {
            assert!(String::from_utf8_lossy(&result.stderr).contains("output_io"));
            result
                .stderr
                .splitn(2, |byte| *byte == b'\n')
                .nth(1)
                .unwrap()
        } else {
            &result.stdout
        };
        let receipt: Value = serde_json::from_slice(receipt_bytes)
            .expect("complete result and native documentation custody");
        let documents = receipt["documentation"]
            .as_array()
            .expect("original documentation deliveries");
        assert_eq!(documents.len(), 1);
        assert_eq!(documents[0]["site"], "site:one");
        assert_eq!(documents[0]["is_error"], false);
        let native = &documents[0]["response"];
        assert_eq!(native["status"], "documentation-assembled-local");
        let jobs = native["jobs"].as_array().unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(
            jobs[0]["evidence"]["version_text"],
            "fixture-documentation/1"
        );
        let generated_root = std::path::Path::new(jobs[0]["output"].as_str().unwrap());
        assert_eq!(
            fs::read(generated_root.join("reference.md")).unwrap(),
            b"# Native API title\n\nRetained provider semantics.\n"
        );
        if fail_output {
            assert_eq!(receipt["status"], "documentation-publication-failed");
            assert_eq!(receipt["stage"]["phase"], "preflight");
            assert!(!missing.exists());
        } else {
            assert_eq!(receipt["status"], "local_retained");
            let retained = std::path::Path::new(receipt["directory"].as_str().unwrap());
            let search: Value =
                serde_json::from_slice(&fs::read(retained.join("search.json")).unwrap()).unwrap();
            assert!(
                search
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|record| record["content"]["title"] == "Native API title"
                        && record["content"]["documentation"] == true)
            );
        }
    }
}
