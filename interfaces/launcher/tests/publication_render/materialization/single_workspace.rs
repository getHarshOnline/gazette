//! One modular, distributed workspace reaches actual configured native output.

use std::{collections::BTreeMap, path::Path};

use gazette_sdk::{build, source, store, workspace};
use serde_json::{Value, json};

use super::super::{PAGE, cli};
use super::{Fixture, fs, initialize, session, tempdir};

fn snapshot(path: &Path) -> source::TreeIdentity {
    source::inspect(path, None, Default::default())
        .expect("exact native tree")
        .identity()
}

#[test]
fn one_modular_site_distributed_sources_inherits_theme_disables_provider_and_materializes() {
    let page = PAGE.replace("related=['content:other']\n", "").replace(
        "**Retained** body",
        "**Retained** body. [[site:one.example/distributed/|Distributed contribution]]",
    );
    let distributed = PAGE
        .replace("related=['content:other']\n", "")
        .replace("id='content:one'", "id='content:distributed'")
        .replace("title='One <escaped>'", "title='Distributed contribution'")
        .replace("path='/guide/'", "path='/distributed/'")
        .replace("aliases=['/old.html']", "aliases=['/distributed-old.html']");
    let fixture = Fixture::new(&[
        ("one/page.md", page.as_bytes()),
        ("project/page.md", distributed.as_bytes()),
    ]);
    let configuration = tempdir().expect("separate native configuration owner");
    let destination = tempdir().expect("existing configured output repository");
    let repository = git2::Repository::init(destination.path()).expect("owner Git administration");
    fs::write(
        destination.path().join("owner.txt"),
        b"independent owner bytes",
    )
    .expect("unrelated owner file");
    let git_before = snapshot(repository.path());
    let sources_before = snapshot(fixture.directory.path());
    let disabled_root = configuration.path().join("unavailable-provider");

    fs::write(
        configuration.path().join("gazette.toml"),
        "includes=['defaults.toml','site.toml']\n\
         [build]\nsites=['site:one']\nlocal_drafts='include'\n",
    )
    .expect("exact configured single-site build defaults");
    fs::write(
        configuration.path().join("defaults.toml"),
        "[composition]\ntheme={package='theme:one'}\n\
         [composition.plugins]\n'unavailable-plugin'={state='enabled'}\n\
         [composition.capabilities]\n'publication.search/v1'={state='enabled',provider={kind='plugin',package='unavailable-plugin',provider='search'},requirement='required'}\n",
    )
    .expect("inherited Theme and independently overridable capability");
    let mut site = format!(
        "[[principals]]\nid='owner:one'\nkind='person'\n\
         [[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n\
         [sites.composition.plugins]\n'unavailable-plugin'={{state='disabled'}}\n\
         [sites.composition.capabilities]\n'publication.search/v1'={{state='disabled'}}\n\
         [[brands]]\nid='brand:one'\nname='Distributed single workspace'\n\
         [[brands.publications]]\nsite='site:one'\npublic_base_url='https://one.example/mount/'\n\
         output={{directory={:?}}}\n\
         authored=[{{source='source:one',target='one'}},{{source='source:distributed',target='distributed'}}]\n",
        destination.path().to_str().expect("configured native root")
    );
    for (key, subtree) in [
        ("theme", "theme"),
        ("one", "one"),
        ("distributed", "project"),
    ] {
        site.push_str(&format!(
            "[[sources]]\nid='source:{key}'\nlocation={{kind='repository',root={:?},commit='{}',subdirectory='{subtree}'}}\n",
            fixture.directory.path().to_str().expect("exact source root"),
            fixture.commit
        ));
    }
    site.push_str(&format!(
        "[[sources]]\nid='source:unavailable'\nlocation={{kind='filesystem',root={:?}}}\n\
         [[theme_packages]]\npackage='theme:one'\nsource='source:theme'\nmanifest='theme.toml'\n\
         [[plugin_packages]]\npackage='unavailable-plugin'\nsource='source:unavailable'\nmanifest='missing-plugin.toml'\n",
        disabled_root.to_str().expect("deliberately nonexistent native root")
    ));
    fs::write(configuration.path().join("site.toml"), site)
        .expect("single site and exact source bindings");
    let configuration_before = snapshot(configuration.path());
    let recipe = json!({
        "format":"gazette-local-publication-set/1",
        "workspace":{"kind":"local","root":configuration.path(),"workspace":"gazette.toml"}
    });
    let request =
        build::LocalPublicationRecipe::parse_json(recipe.to_string().as_bytes(), 1 << 20, 1)
            .expect("native configured intent without invocation site override")
            .resolve(Default::default())
            .expect("complete modular configuration");
    assert_eq!(
        request.build_selection().origin,
        workspace::BuildOrigin::Workspace
    );
    assert_eq!(
        request.build_selection().sites,
        ["site:one".parse().unwrap()]
    );

    let (expected_report, expected_inventory, expected_files) = request
        .with_rendered(
            Default::default(),
            Default::default(),
            Default::default(),
            |rendered| {
                assert!(rendered.is_complete());
                assert_eq!(rendered.sites().len(), 1);
                let publication = rendered.sites()[&"site:one".parse().unwrap()]
                    .as_ref()
                    .expect("only selected site succeeds");
                let inputs = publication.inputs();
                let workspace = &inputs.compilation().workspace;
                assert_eq!(
                    workspace.sites().len(),
                    1,
                    "not a subset of a multi-site installation"
                );
                assert_eq!(workspace.content_bindings().count(), 2);
                assert_eq!(
                    inputs
                        .sources()
                        .keys()
                        .map(workspace::Key::as_str)
                        .collect::<Vec<_>>(),
                    ["source:distributed", "source:one", "source:theme"],
                    "disabled inherited provider is not acquired or replaced by a fallback"
                );
                let composition = workspace
                    .composition_for(&"site:one".parse().unwrap())
                    .unwrap();
                assert_eq!(
                    composition.theme().unwrap().origin().scope,
                    workspace::SelectionScope::Workspace
                );
                assert_eq!(
                    composition.theme().unwrap().value().package.as_str(),
                    "theme:one"
                );
                let disabled =
                    &composition.capabilities()[&"publication.search/v1".parse().unwrap()];
                assert_eq!(
                    disabled.value(),
                    &workspace::CapabilitySelection::Disabled {}
                );
                assert_eq!(
                    disabled.origin().scope,
                    workspace::SelectionScope::Site("site:one".parse().unwrap())
                );
                assert_eq!(
                    serde_json::to_value(
                        composition.plugins()[&"unavailable-plugin".parse().unwrap()].value()
                    )
                    .unwrap(),
                    json!({"state": "disabled"})
                );
                let output = publication.site().output();
                let files = output
                    .inventory()
                    .entries()
                    .iter()
                    .filter_map(|entry| {
                        output
                            .file(&entry.path)
                            .map(|bytes| (entry.path.clone(), bytes.to_vec()))
                    })
                    .collect::<BTreeMap<_, _>>();
                for path in ["index.html", "guide/index.html", "distributed/index.html"] {
                    assert!(
                        files.contains_key(path),
                        "complete distributed page output {path}"
                    );
                }
                let guide = std::str::from_utf8(&files["guide/index.html"]).unwrap();
                let attributes =
                    crate::browser_carrier::values(guide, gazette_sdk::html::TextKind::Attribute);
                assert!(
                    attributes
                        .iter()
                        .any(|value| value == "https://one.example/mount/distributed/")
                );
                assert!(
                    !files.contains_key("model.json"),
                    "no unrequested debug artifact"
                );
                (json!(rendered.inspect()), json!(output.inventory()), files)
            },
        )
        .expect("real retained SDK compiler and inherited Theme");
    assert_eq!(expected_report["build_selection"]["origin"], "workspace");
    assert_eq!(
        expected_report["configuration"]["documents"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert!(!disabled_root.exists());

    let (journal, initialized) = initialize();
    let mut header = initialized.header;
    let mut previous = None;
    let mut materialized_tree = None;
    for label in ["first-single-workspace", "reopened-single-workspace-noop"] {
        let id = source::Sha256Digest::of(label.as_bytes());
        let arguments = vec![
            "materialize-sites".to_owned(),
            journal.path().to_str().unwrap().to_owned(),
            "--expected-header".to_owned(),
            serde_json::to_string(&header).unwrap(),
            "--operation".to_owned(),
            format!("site:one={id}"),
            "--local".to_owned(),
            "--allow-read".to_owned(),
            "--allow-write-output".to_owned(),
            "--allow-record-history".to_owned(),
            "--advance-writer-generation".to_owned(),
            "--reserve-local-coordination".to_owned(),
            "--allow-coordination-files".to_owned(),
        ];
        let result = cli(
            &recipe.to_string(),
            &arguments.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(result.stderr.is_empty());
        let reports = std::str::from_utf8(&result.stdout)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(reports.len(), 3);
        assert_eq!(reports[0]["event"], "reserved");
        assert_eq!(reports[1]["event"], "acquired");
        let acquired: store::JournalHeader =
            serde_json::from_value(reports[1]["custody"]["acquired_header"].clone()).unwrap();
        assert_eq!(acquired.writer_fence, header.writer_fence + 1);
        let terminal = &reports[2];
        assert_eq!(terminal["native_status"], "materialization_completed");
        assert_eq!(terminal["details_complete"], true);
        assert_eq!(terminal["close"]["status"], "closed");
        assert_eq!(terminal["detail"].as_object().unwrap().len(), 1);
        let actual = &terminal["detail"]["site:one"];
        assert_eq!(actual["output"]["generated"], json!(expected_inventory));
        let historical = store::inspect_materialization(
            &initialized.database,
            &destination.path().canonicalize().unwrap(),
            Some(&acquired),
            session().read,
        )
        .expect("independently reopened completed native history");
        assert!(historical.frontier.active.is_none());
        let record = historical
            .frontier
            .head
            .expect("verified completed output owner");
        assert_eq!(record.intent.id, id);
        assert_eq!(record.intent.previous, previous);
        record
            .completion
            .as_ref()
            .unwrap()
            .validate(&record.intent)
            .expect("exact durable read-back");
        assert_eq!(json!(record), actual["history"]["record"]);
        assert_eq!(actual["output"]["inventory"], json!(record.intent.expected));
        for (path, bytes) in &expected_files {
            assert_eq!(
                fs::read(destination.path().join(path.as_str())).unwrap(),
                *bytes
            );
        }
        assert_eq!(
            fs::read(destination.path().join("owner.txt")).unwrap(),
            b"independent owner bytes"
        );
        let current = snapshot(destination.path());
        if let Some(prior) = materialized_tree {
            assert_eq!(
                current, prior,
                "reopened owned no-op preserves exact native bytes"
            );
        }
        materialized_tree = Some(current);
        previous = Some(id);
        header = acquired;
    }
    assert!(!disabled_root.exists());
    assert_eq!(snapshot(repository.path()), git_before);
    assert_eq!(snapshot(fixture.directory.path()), sources_before);
    assert_eq!(snapshot(configuration.path()), configuration_before);
}
