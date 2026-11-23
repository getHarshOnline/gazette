//! Complete private registry preparation through retained SDK inputs and the real CLI.

use super::*;
use serde_json::json;

const JOURNEY: &str = "+++\nprojects=[{project='project:native',kind='context'}]\n[content]\nid='journey:reading'\nsite='site:one'\ntitle='Reading'\nlifecycle='published'\nroute={path='/reading/'}\nstructure={kind='journey',collection='collection:home',members=[{content='content:reading-first',primary=true},{content='content:one',primary=true}]}\n+++\nReading body\n";
const READING_FIRST: &str = "+++\n[content]\nid='content:reading-first'\nsite='site:one'\ntitle='First reading chapter'\nlifecycle='published'\nroute={path='/reading/first/'}\nstructure={kind='page',family={kind='article'}}\n+++\nFirst reading body\n";

fn page() -> String {
    PAGE.replace(
        "[content]\n",
        "[content]\nattributions=[{principal='person:beta',role='author'},{principal='person:alpha',role='rights-holder'},{principal='person:alpha',role='author'}]\n",
    )
    .replace("collections=['collection:home']\n", "")
}

fn fixture() -> Fixture {
    Fixture::new(&[
        ("one/page.md", page().as_bytes()),
        ("one/reading.md", JOURNEY.as_bytes()),
        ("one/first.md", READING_FIRST.as_bytes()),
        ("project/guide.md", PROJECT_DOC.as_bytes()),
    ])
}

fn configuration(fixture: &Fixture, selected: bool) -> String {
    format!(
        "{}\n\
         [[principals]]\nid='person:beta'\nkind='person'\nname='First author'\n\
         [[principals]]\nid='person:alpha'\nkind='person'\nname='Second author'\n\
         [[principals]]\nid='person:unused'\nkind='person'\nname='PRIVATE UNUSED PRINCIPAL'\n\
         [[projects]]\nid='project:unused'\nlifecycle='done'\nvisibility='private'\navailability='unavailable'\n\
         [[sites]]\nid='site:empty'\npublisher='owner:one'\ndeployable=false\n\
         [[placements]]\nrole='primary'\nsite='site:other'\nproject='project:unused'\nrender=false\ndiscoverable=false\n\
         [[taxonomies]]\nid='taxonomy:unused'\nlabel='PRIVATE UNUSED TAXONOMY'\nterms=[{{id='term:z',label='Last'}},{{id='term:a',label='First'}}]\n",
        fixture.project_config(selected)
    )
}

fn report(inputs: &build::AcquiredInputs) -> Value {
    let parsed = inputs
        .parse_publication(Default::default(), &gazette_adapter_markdown::Comrak)
        .expect("retained syntax");
    let resolved = parsed
        .resolve(Default::default())
        .expect("complete source-bound graph");
    let projection = resolved
        .registry(Default::default())
        .expect("private registry projection");
    assert!(std::ptr::eq(
        projection.model().content,
        resolved.authored().content().graph()
    ));
    assert_eq!(
        projection.semantic_sha256(),
        serde_json::from_value::<source::Sha256Digest>(
            serde_json::to_value(&projection).expect("projection")["semantic_sha256"].clone()
        )
        .expect("digest transport")
    );
    serde_json::to_value(projection).expect("complete registry report")
}

#[test]
fn explicit_brand_parent_survives_application_and_cli_without_role_inheritance() {
    let fixture = fixture();
    let config = format!(
        "{}\n[[brands]]\nid='brand:parent'\nname='Independent parent brand'\n",
        configuration(&fixture, true)
    );
    let unparented = report(&acquire(&config));
    let parented_config = config.replace(
        "id='brand:one'\n",
        "id='brand:one'\nparent='brand:parent'\n",
    );
    assert_ne!(parented_config, config, "exact native brand anchor");
    let parented = report(&acquire(&parented_config));
    assert_eq!(
        unparented["model"]["brands"]["brand:one"]["parent"],
        Value::Null
    );
    assert_eq!(
        parented["model"]["brands"]["brand:one"]["parent"],
        "brand:parent"
    );
    assert_eq!(
        parented["model"]["brands"]["brand:parent"]["parent"],
        Value::Null
    );
    assert_ne!(parented["semantic_sha256"], unparented["semantic_sha256"]);
    assert_eq!(parented["model"]["sites"], unparented["model"]["sites"]);
    assert_eq!(
        parented["model"]["principals"],
        unparented["model"]["principals"]
    );
    let output = cli(
        &parented_config,
        &["check", "--publication", "site:one", "--registry"],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).expect("native CLI projection"),
        parented
    );
}

#[test]
fn complete_registry_keeps_catalogs_states_order_and_native_provenance_separate() {
    let fixture = fixture();
    let config = configuration(&fixture, true);
    let inputs = acquire(&config);
    let report = report(&inputs);
    assert_eq!(report["schema"], "gazette.registry/v1");
    assert_eq!(report["status"], "resolved-unadmitted");
    let model = &report["model"];
    assert_eq!(model["sites"].as_object().expect("complete sites").len(), 3);
    assert_eq!(model["sites"]["site:empty"]["brand"], Value::Null);
    assert_eq!(model["sites"]["site:empty"]["public_base_url"], Value::Null);
    assert_eq!(model["sites"]["site:one"]["publisher"], "owner:one");
    assert_eq!(model["brands"]["brand:one"]["name"], "Explicit bindings");
    assert_eq!(
        model["principals"]
            .as_object()
            .expect("all principals")
            .len(),
        4
    );
    assert_eq!(
        model["principals"]["person:unused"]["name"],
        "PRIVATE UNUSED PRINCIPAL"
    );
    assert_eq!(
        model["projects"].as_object().expect("all projects").len(),
        3
    );
    assert_eq!(
        model["projects"]["project:unused"],
        json!({
            "id":"project:unused", "lifecycle":"done", "visibility":"private", "availability":"unavailable"
        })
    );
    assert_eq!(
        model["placements"]
            .as_array()
            .expect("all placements")
            .len(),
        2
    );
    assert_eq!(
        model["placements"][1],
        json!({
            "site":"site:other", "project":"project:unused", "role":"primary", "render":false, "discoverable":false
        })
    );
    assert_eq!(
        model["taxonomies"]["taxonomy:unused"]["terms"][0]["id"],
        "term:a"
    );
    let graph = &model["content"];
    assert_eq!(graph["nodes"].as_object().expect("all content").len(), 6);
    assert_eq!(graph["nodes"]["content:one"]["lifecycle"], "draft");
    assert_eq!(graph["nodes"]["content:other"]["lifecycle"], "archived");
    assert_eq!(graph["nodes"]["content:other"]["site"], "site:other");
    assert_eq!(
        graph["nodes"]["content:one"]["attributions"],
        json!([
            {"principal":"person:beta","role":"author"},
            {"principal":"person:alpha","role":"rights-holder"},
            {"principal":"person:alpha","role":"author"}
        ])
    );
    assert_eq!(
        graph["nodes"]["journey:reading"]["structure"]["members"],
        json!([
            {"content":"content:reading-first","primary":true},
            {"content":"content:one","primary":true}
        ])
    );
    assert_eq!(
        graph["memberships"],
        json!([{"content":"doc:native","collection":"collection:home"}]),
        "direct collection residence stays separate from ordered journey residence"
    );
    assert_eq!(
        graph["related"],
        json!([
            {"source":"content:one","target":"content:other"},
            {"source":"doc:native","target":"content:other"}
        ])
    );
    assert_eq!(
        graph["project_links"]["content:one"]["project:native"]["relation"],
        "mentions"
    );
    assert_eq!(
        graph["project_links"]["doc:native"]["project:subject"]["relation"],
        "about"
    );
    assert!(
        graph["project_links"]["doc:native"]
            .get("project:native")
            .is_none()
    );
    assert_eq!(
        model["provenance"]["doc:native"],
        json!({
            "source":"source:native", "entry":"guide.md", "project":"project:native"
        })
    );
    assert_eq!(
        model["provenance"]["content:one"],
        json!({
            "source":"source:one", "entry":"page.md", "project":null
        })
    );
    assert_eq!(
        model["routes"]["content:other"]["canonical_url"],
        "https://other.example/elsewhere/"
    );
    assert_eq!(
        model["routes"]["content:one"]["aliases"][0]["output"],
        "old.html"
    );
    assert_eq!(
        report["configuration_sha256"],
        json!(source::Sha256Digest::of(config.as_bytes()))
    );
    assert_eq!(
        report["documents"]
            .as_object()
            .expect("all document identities")
            .len(),
        6
    );
    assert_eq!(
        report["documents"]["content:one"],
        json!(source::Sha256Digest::of(page().as_bytes()))
    );
    assert_eq!(
        report["documents"]["doc:native"],
        json!(source::Sha256Digest::of(PROJECT_DOC.as_bytes()))
    );

    let output = cli(
        &config,
        &["check", "--publication", "site:one", "--registry"],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).expect("actual CLI report"),
        report
    );
    let local = render(&inputs).expect("same shared graph remains renderable");
    let site: Value =
        serde_json::from_slice(bytes(local.site().output(), "model.json")).expect("local model");
    assert_eq!(
        site["pages"]["collection:home"]["route"]["public_url"],
        "https://one.example/mount/"
    );
    let parsed = inputs
        .parse_publication(Default::default(), &gazette_adapter_markdown::Comrak)
        .expect("same retained native syntax");
    let resolved = parsed
        .resolve(Default::default())
        .expect("same canonical owner");
    let authored = resolved.authored();
    let discovery = authored.discovery().expect("source-bound discovery view");
    assert_eq!(
        graph,
        &json!(authored.content().graph()),
        "registry retains the complete canonical graph, including its structural edges"
    );
    for (id, local) in site["pages"].as_object().expect("local pages") {
        let key = id.parse().expect("native content identity");
        let node = authored
            .content()
            .graph()
            .node(&key)
            .expect("canonical node");
        assert_eq!(
            graph["nodes"][id],
            json!(node),
            "registry does not substitute discovery metadata for canonical state"
        );
        assert_eq!(
            local["content"],
            json!(discovery.node(node).expect("eligible native listing"))
        );
        let mut registry_route = model["routes"][id].clone();
        let canonical = registry_route
            .as_object_mut()
            .expect("registry route")
            .remove("canonical")
            .expect("explicit nullable canonical endpoint");
        assert_eq!(canonical, local["content"]["route"]["canonical"]);
        let mut local_route = local["route"].clone();
        assert_eq!(
            local_route
                .as_object_mut()
                .unwrap()
                .remove("public_url")
                .unwrap(),
            json!(authored.content().route(&key).unwrap().public_url)
        );
        assert_eq!(local_route, registry_route);
    }
    assert!(
        site["pages"]["journey:reading"]["content"]["structure"]
            .get("members")
            .is_none(),
        "automatic metadata does not expand original journey endpoints"
    );
    assert_eq!(
        graph["nodes"]["journey:reading"]["structure"]["members"]
            .as_array()
            .expect("canonical declaration remains complete")
            .len(),
        2
    );
    assert!(
        site["pages"].get("content:other").is_none(),
        "registry completeness is not site leakage"
    );
}

#[test]
fn semantic_identity_ignores_toml_order_comments_and_consuming_site_not_changed_facts() {
    let fixture = fixture();
    let config = configuration(&fixture, true);
    let first = report(&acquire(&config));
    let one = "[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n";
    let other = "[[sites]]\nid='site:other'\npublisher='owner:one'\ndeployable=false\n";
    let reordered = format!(
        "# Exact source evidence differs\n{}",
        config
            .replace(&format!("{one}{other}"), &format!("{other}{one}"))
            .replace(
                "terms=[{id='term:z',label='Last'},{id='term:a',label='First'}]",
                "terms=[{id='term:a',label='First'},{id='term:z',label='Last'}]"
            )
    );
    assert!(
        reordered.contains(&format!("{other}{one}")),
        "actual reordered fixture"
    );
    let inputs = build::acquire_publication(
        &reordered,
        &"site:other".parse().expect("other consumer"),
        Default::default(),
        source::PreparationLimits {
            sources: 8,
            entries: 128,
            bytes: 1024 * 1024,
            per_source: Default::default(),
        },
    )
    .expect("independent consumer over complete workspace");
    let second = report(&inputs);
    assert_eq!(first["model"], second["model"]);
    assert_eq!(first["semantic_sha256"], second["semantic_sha256"]);
    assert_eq!(first["documents"], second["documents"]);
    assert_ne!(
        first["configuration_sha256"],
        second["configuration_sha256"]
    );
    let changed = report(&acquire(
        &config.replace("Explicit bindings", "Changed explicit name"),
    ));
    assert_ne!(first["semantic_sha256"], changed["semantic_sha256"]);
    assert_eq!(first["documents"], changed["documents"]);
    let output = cli(
        &reordered,
        &["check", "--publication", "site:other", "--registry"],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).expect("other-site CLI"),
        second
    );
}

#[test]
fn raw_document_and_configuration_evidence_do_not_masquerade_as_structural_identity() {
    let first = fixture();
    let changed_page = page().replace(
        "**Retained** body",
        "A different retained body without structural changes",
    );
    let second = Fixture::new(&[
        ("one/page.md", changed_page.as_bytes()),
        ("one/reading.md", JOURNEY.as_bytes()),
        ("one/first.md", READING_FIRST.as_bytes()),
        ("project/guide.md", PROJECT_DOC.as_bytes()),
    ]);
    let first = report(&acquire(&configuration(&first, true)));
    let second = report(&acquire(&configuration(&second, true)));
    assert_eq!(first["model"], second["model"]);
    assert_eq!(first["semantic_sha256"], second["semantic_sha256"]);
    assert_ne!(
        first["configuration_sha256"],
        second["configuration_sha256"]
    );
    assert_ne!(
        first["documents"]["content:one"],
        second["documents"]["content:one"]
    );
    assert_eq!(
        second["documents"]["content:one"],
        json!(source::Sha256Digest::of(changed_page.as_bytes()))
    );
    for unchanged in [
        "collection:home",
        "content:other",
        "content:reading-first",
        "journey:reading",
        "doc:native",
    ] {
        assert_eq!(
            first["documents"][unchanged],
            second["documents"][unchanged]
        );
    }
}

#[test]
fn empty_content_is_a_complete_registry_with_catalogs_and_site_scope() {
    let fixture = Fixture::new(&[
        ("one/assets/input.bin", b"one"),
        ("other/assets/input.bin", b"other"),
    ]);
    let config = configuration(&fixture, false)
        .replace("subdirectory='one'", "subdirectory='one/assets'")
        .replace("subdirectory='other'", "subdirectory='other/assets'");
    let projected = report(&acquire(&config));
    let model = &projected["model"];
    assert_eq!(model["sites"].as_object().expect("all sites").len(), 3);
    assert_eq!(
        model["principals"]
            .as_object()
            .expect("all principals")
            .len(),
        4
    );
    assert_eq!(
        model["projects"].as_object().expect("all projects").len(),
        3
    );
    assert_eq!(
        model["placements"]
            .as_array()
            .expect("all disabled placements")
            .len(),
        2
    );
    assert!(
        model["placements"]
            .as_array()
            .expect("placements")
            .iter()
            .all(|placement| placement["render"] == false)
    );
    for field in ["nodes", "project_links"] {
        assert_eq!(model["content"][field], json!({}));
    }
    for field in ["memberships", "related", "projects"] {
        assert_eq!(model["content"][field], json!([]));
    }
    assert_eq!(model["routes"], json!({}));
    assert_eq!(model["provenance"], json!({}));
    assert_eq!(projected["documents"], json!({}));
    let output = cli(
        &config,
        &["check", "--publication", "site:one", "--registry"],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).expect("complete empty report"),
        projected
    );
}

#[test]
fn compact_registry_budget_is_exact_and_failure_never_emits_a_partial_cli_report() {
    let fixture = fixture();
    let config = configuration(&fixture, true);
    let inputs = acquire(&config);
    let parsed = inputs
        .parse_publication(Default::default(), &gazette_adapter_markdown::Comrak)
        .expect("syntax");
    let resolved = parsed.resolve(Default::default()).expect("complete graph");
    let projection = resolved.registry(Default::default()).expect("full report");
    let compact = serde_json::to_vec(&projection).expect("compact report");
    let exact = resolved
        .registry(build::RegistryLimits {
            bytes: compact.len(),
        })
        .expect("exact inclusive bound");
    assert_eq!(serde_json::to_vec(&exact).expect("exact report"), compact);
    for limit in [0, compact.len() - 1] {
        let error = resolved
            .registry(build::RegistryLimits { bytes: limit })
            .expect_err("whole report exceeds bound");
        assert_eq!(error.code(), "registry_encoding");
        let output = cli(
            &config,
            &[
                "check",
                "--publication",
                "site:one",
                "--registry",
                "--max-registry-bytes",
                &limit.to_string(),
            ],
        );
        assert!(!output.status.success());
        assert!(
            output.stdout.is_empty(),
            "failed report cannot leak a prefix"
        );
        assert!(String::from_utf8_lossy(&output.stderr).contains("registry_encoding"));
    }
    let output = cli(
        &config,
        &[
            "check",
            "--publication",
            "site:one",
            "--registry",
            "--max-registry-bytes",
            &compact.len().to_string(),
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).expect("bounded CLI report"),
        serde_json::from_slice::<Value>(&compact).expect("SDK report")
    );
}

#[test]
fn registry_flags_cannot_select_other_modes_or_skip_required_publication() {
    for arguments in [
        vec!["check", "--registry"],
        vec!["check", "--max-registry-bytes", "1"],
        vec![
            "check",
            "--publication",
            "site:one",
            "--max-registry-bytes",
            "1",
        ],
        vec![
            "check",
            "--publication",
            "site:one",
            "--registry",
            "--review",
        ],
        vec![
            "check",
            "--publication",
            "site:one",
            "--registry",
            "--max-review-bytes",
            "1",
        ],
        vec![
            "check",
            "--publication",
            "site:one",
            "--registry",
            "--origin",
            "{}",
        ],
        vec![
            "check",
            "--publication",
            "site:one",
            "--registry",
            "--acquire-site",
            "site:one",
        ],
        vec![
            "check",
            "--publication",
            "site:one",
            "--registry",
            "--parse-documents",
        ],
    ] {
        let output = cli("", &arguments);
        assert_eq!(
            output.status.code(),
            Some(2),
            "{arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.is_empty(), "{arguments:?}");
    }
}

#[test]
fn prepared_registry_uses_retained_inputs_after_native_source_changes() {
    let fixture = fixture();
    let inputs = acquire(&configuration(&fixture, true));
    let first = report(&inputs);
    fs::write(
        fixture.directory.path().join("one/page.md"),
        b"invalid after capture",
    )
    .expect("disposable native change");
    fs::remove_file(fixture.directory.path().join("project/guide.md"))
        .expect("disposable native removal");
    fs::write(
        fixture.directory.path().join("theme/theme.toml"),
        b"invalid after capture",
    )
    .expect("disposable presentation change");
    assert_eq!(first, report(&inputs), "no native reread or stale fallback");
}
