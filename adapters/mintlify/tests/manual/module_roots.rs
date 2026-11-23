//! Root pages coexist with descendants without giving up collision checks.

use super::*;

#[test]
fn root_and_component_readmes_keep_routes_links_and_configured_reference_order() {
    for filename in ["README.md", "index.md"] {
        for reference_first in [false, true] {
            let start = format!("docs/{filename}");
            let back = format!("# SDK\n\n[Home](../../docs/{filename}#library)\n");
            let tree = retained(&[
                (&start, b"# Library\n\n[SDK](../sdk/js/README.md#sdk) [API](/library/reference#reference)\n\n![Logo](logo.png)\n"),
                ("sdk/js/README.md", back.as_bytes()),
                ("docs/logo.png", b"image"),
            ]);
            let start_input = selection(&start);
            let sdk_input = selection("sdk/js/README.md");
            let logo_input = selection("docs/logo.png");
            let root = entry("library");
            let sdk = entry("library/sdk/js");
            let logo = entry("library/assets/logo.png");
            let generated = Input::new(
                b"# Reference\n\n[Guide](/library#library) [SDK](/library/sdk/js#sdk)\n",
            );
            let manual = ProjectItem::Manual(ManualSection {
                title: "Manual",
                pages: vec![
                    ManualPage {
                        title: "Library",
                        route: &root,
                        selection: &start_input,
                    },
                    ManualPage {
                        title: "SDK",
                        route: &sdk,
                        selection: &sdk_input,
                    },
                ],
            });
            let reference = ProjectItem::Reference(Reference {
                slug: "reference",
                title: "API",
                pages: None,
                artifact: generated.artifact(Format::Markdown),
            });
            let output = assemble(&[ProjectModule {
                slug: "library",
                title: "Library",
                manual: Some(ManualSource {
                    source: "source:manual",
                    tree: tree.as_input(),
                    assets: vec![ManualAsset {
                        selection: &logo_input,
                        path: &logo,
                    }],
                }),
                navigation: if reference_first {
                    vec![reference, manual]
                } else {
                    vec![manual, reference]
                },
            }])
            .unwrap();
            let home = std::str::from_utf8(output.file(&entry("library.md")).unwrap()).unwrap();
            assert!(home.contains("[SDK](/library/sdk/js#sdk)"));
            assert!(home.contains("[API](/library/reference#reference)"));
            assert!(home.contains("/library/assets/logo.png"));
            assert!(
                std::str::from_utf8(output.file(&entry("library/sdk/js.md")).unwrap())
                    .unwrap()
                    .contains("[Home](/library#library)")
            );
            assert!(output.file(&entry("library/reference.md")).is_some());
            assert_eq!(output.file(&logo), Some(b"image".as_slice()));
            let config: Value =
                serde_json::from_slice(output.file(&entry("docs.json")).unwrap()).unwrap();
            let groups = config["navigation"]["groups"][0]["pages"]
                .as_array()
                .unwrap();
            let manual_index = usize::from(reference_first);
            assert_eq!(groups[manual_index]["group"], "Manual");
            assert_eq!(
                groups[manual_index]["pages"],
                serde_json::json!(["library", "library/sdk/js"])
            );
            assert_eq!(groups[1 - manual_index]["group"], "API");
        }
    }
}

#[test]
fn a_manual_cannot_claim_the_reference_root_or_its_descendants() {
    use gazette_core::documentation::{
        Plan, PlannedManualPage, PlannedManualSection, PlannedProject, PlannedProjectItem,
        PlannedReference,
    };
    for route in ["library/reference", "library/reference/guide"] {
        for reference_first in [false, true] {
            let route = entry(route);
            let input = selection("reference.md");
            let manual = PlannedProjectItem::Manual(PlannedManualSection {
                title: "Manual",
                pages: vec![PlannedManualPage {
                    title: "Guide",
                    route: &route,
                }],
            });
            let reference = PlannedProjectItem::Reference(PlannedReference {
                slug: "reference",
                title: "API",
                format: Format::Markdown,
                selection: &input,
                pages: None,
            });
            let target = Target::Mintlify {
                theme: "mint".into(),
                primary_color: "#123abc".into(),
                light_color: None,
                dark_color: None,
                telemetry: None,
            };
            let plan = Plan {
                name: "Library",
                target: &target,
                projects: vec![PlannedProject {
                    slug: "library",
                    title: "Library",
                    manual_assets: vec![],
                    navigation: if reference_first {
                        vec![reference, manual]
                    } else {
                        vec![manual, reference]
                    },
                }],
            };
            assert!(
                plan.validate()
                    .unwrap_err()
                    .to_string()
                    .contains("overlaps")
            );
        }
    }
}
