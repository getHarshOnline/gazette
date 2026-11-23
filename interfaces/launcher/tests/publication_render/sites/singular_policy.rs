//! The singular CLI consumes the same explicit override and workspace draft policy.

use std::{fs, path::Path};

use super::{Fixture, Value, acquire_set, build, bytes, cli, json, key, options};

#[test]
fn singular_cli_override_preserves_workspace_draft_policy_and_optional_model_bytes() {
    for policy in ["include", "exclude"] {
        let fixture = Fixture::new(&[]);
        let config = format!(
            "[build]\nsites=['site:other']\nlocal_drafts='{policy}'\n{}",
            fixture.config().replace(
                "authored=[{source='source:one',target='one'}]",
                "authored=[{source='source:one',target='one'}]\nmachine={json_ld=true}",
            )
        );
        let inputs = acquire_set(&config, &["site:one"]);
        let page = "page.html".parse().unwrap();
        let alias = "alias.html".parse().unwrap();
        let model = "debug.model".parse().unwrap();
        let render_options = || {
            let mut selected = options(&["site:one"], &page, &alias, &model);
            selected
                .sites
                .get_mut(&key("site:one"))
                .unwrap()
                .render_year = Some(gazette_sdk::authored::RenderYear::new(2040).unwrap());
            selected
        };
        let reference = build::render_publications(&inputs, render_options(), Default::default())
            .expect("same shared native selection and render");
        assert!(reference.is_complete());
        let reference = reference.sites()[&key("site:one")].as_ref().unwrap().site();
        let reference_model: Value =
            serde_json::from_slice(bytes(reference.output(), "debug.model"))
                .expect("native shared machine model");
        let pages = reference_model["pages"]
            .as_object()
            .expect("exact page map");
        assert!(pages.contains_key("collection:home"));
        assert_eq!(pages.contains_key("content:one"), policy == "include");
        assert_eq!(pages.len(), if policy == "include" { 2 } else { 1 });
        assert!(!pages.contains_key("content:other"));
        let parent = tempfile::tempdir().expect("explicit disposable retention parent");
        for request_model in [false, true] {
            let mut selected = render_options();
            if !request_model {
                selected
                    .sites
                    .get_mut(&key("site:one"))
                    .unwrap()
                    .model_output = None;
            }
            let expected = build::render_publications(&inputs, selected, Default::default())
                .expect("pure SDK output over the retained capture");
            assert!(expected.is_complete());
            let expected = expected.sites()[&key("site:one")].as_ref().unwrap().site();
            let mut flags = vec![
                "render-site",
                "--local",
                "--render-year",
                "2040",
                "--site",
                "site:one",
                "--output-parent",
                parent.path().to_str().unwrap(),
            ];
            if request_model {
                flags.extend(["--model-output", "debug.model"]);
            }
            let output = cli(&config, &flags);
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let report: Value = serde_json::from_slice(&output.stdout).expect("actual CLI report");
            assert_eq!(report["status"], "local_retained");
            assert_eq!(
                report["build_selection"],
                json!({
                    "origin":"invocation", "sites":["site:one"], "local_drafts":policy,
                })
            );
            assert_eq!(
                report["inventory"],
                serde_json::to_value(expected.output().inventory()).unwrap()
            );
            assert_eq!(
                report["evidence"],
                serde_json::to_value(expected.evidence()).unwrap()
            );
            let retained = Path::new(
                report["directory"]
                    .as_str()
                    .expect("native retained directory"),
            );
            for entry in expected.output().inventory().entries() {
                if let Some(bytes) = expected.output().file(&entry.path) {
                    assert_eq!(
                        fs::read(retained.join(entry.path.as_str())).unwrap(),
                        bytes,
                        "exact staged bytes at {}",
                        entry.path
                    );
                    if entry.path != model {
                        assert_eq!(
                            Some(bytes),
                            reference.output().file(&entry.path),
                            "debug serialization does not change {}",
                            entry.path
                        );
                    }
                }
            }
            assert!(retained.join("index.html").is_file());
            assert_eq!(
                retained.join("guide/index.html").is_file(),
                policy == "include"
            );
            assert_eq!(retained.join("old.html").is_file(), policy == "include");
            assert!(!retained.join("elsewhere/index.html").exists());
            assert!(
                !retained.join("model.json").exists(),
                "no invented debug destination"
            );
            let json_ld: Value = serde_json::from_slice(
                &fs::read(retained.join("site.jsonld")).unwrap(),
            )
            .expect("selected machine output remains available without debug serialization");
            assert!(
                !json_ld["@graph"]
                    .as_array()
                    .expect("native graph")
                    .is_empty()
            );
            if request_model {
                let actual: Value =
                    serde_json::from_slice(&fs::read(retained.join("debug.model")).unwrap())
                        .unwrap();
                assert_eq!(actual, reference_model);
                assert_eq!(report["evidence"]["model_output"], "debug.model");
                if policy == "include" {
                    assert_eq!(
                        actual["pages"]["content:one"]["content"]["lifecycle"],
                        "draft"
                    );
                    assert!(
                        fs::read_to_string(retained.join("guide/index.html"))
                            .unwrap()
                            .contains(
                                actual["pages"]["content:one"]["body_html"]
                                    .as_str()
                                    .unwrap()
                            )
                    );
                }
            } else {
                assert!(!retained.join("debug.model").exists());
                assert!(report["evidence"].get("model_output").is_none());
            }
        }
    }
}

#[test]
fn configured_multisite_stage_and_probe_refuse_before_deleted_content_sources_are_opened() {
    let fixture = Fixture::new(&[]);
    let config = super::configured::configuration(&fixture, "include");
    let recipe = super::configured::recipe(&config);
    drop(fixture);
    let parent = tempfile::tempdir().expect("unchanged output parent");
    fs::write(parent.path().join("keep.txt"), b"preserved").unwrap();
    let publication = json!({"recipe_json":recipe.to_string(),"local":true,"allow_read":true});
    crate::mcp_carrier::initialized_interact("2026-07-28", |initialized, send| {
        assert_eq!(initialized["result"]["protocolVersion"], "2026-07-28");
        let catalog = send(json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}));
        for (index, (name, arguments)) in [
            (
                "stage_rendered_site",
                json!({"publication":publication,"create_stage":true,
                "output_parent":parent.path(),"directory_bytes":1024}),
            ),
            (
                "probe_rendered_site",
                json!({"publication":publication,
                "rules_json":json!({"format":"gazette-disclosure-output-input/1",
                    "rules":[{"id":"rule:fixture","literal":"PRIVATE TOKEN"}]}).to_string()}),
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let tool = catalog["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .find(|tool| tool["name"] == name)
                .expect("native descriptor");
            jsonschema::validator_for(&tool["inputSchema"])
                .unwrap()
                .validate(&arguments)
                .expect("actual configured request matches discovery");
            let response = send(json!({"jsonrpc":"2.0","id":index+2,"method":"tools/call",
                "params":{"name":name,"arguments":arguments}}));
            assert!(response.get("error").is_none(), "{response}");
            assert_eq!(response["result"]["isError"], true);
            let evidence = &response["result"]["structuredContent"];
            assert_eq!(evidence["code"], "publication_site_selection", "{response}");
            let text: Value =
                serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap())
                    .unwrap();
            assert_eq!(&text, evidence);
        }
        let healthy = send(json!({"jsonrpc":"2.0","id":4,"method":"tools/list","params":{}}));
        assert!(healthy.get("error").is_none());
    });
    assert_eq!(
        fs::read(parent.path().join("keep.txt")).unwrap(),
        b"preserved"
    );
    assert_eq!(
        fs::read_dir(parent.path()).unwrap().count(),
        1,
        "no staging began"
    );
}
