//! Actual target encoding, not a provider build or a second module-tree producer.

use super::{Config, Error, Group, Input, Page, ReferenceGroup, encode};
use serde_json::{Value, json};

fn configuration(pages: Vec<Page>, ceiling: u64) -> Result<Vec<u8>, Error> {
    encode(
        &Config {
            preview_link_bases: None,
            public_base_url: None,
            name: "References",
            theme: "mint",
            primary_color: "#123abc",
            light_color: None,
            dark_color: None,
            telemetry: None,
            usage: None,
            metadata: &Default::default(),
        },
        vec![Group {
            group: "Project",
            pages: vec![Page::Reference(ReferenceGroup {
                group: "Library".into(),
                input: Input::Pages(pages),
            })],
        }],
        ceiling,
    )
}

#[test]
fn flat_pages_keep_the_existing_configuration_bytes() {
    let result = configuration(vec![Page::Path("project/library/README".into())], 4096)
        .expect("flat configuration");
    assert_eq!(
        std::str::from_utf8(&result).expect("UTF-8"),
        concat!(
            "{\n",
            "  \"$schema\": \"https://mintlify.com/docs.json\",\n",
            "  \"colors\": {\n    \"primary\": \"#123abc\"\n  },\n",
            "  \"name\": \"References\",\n",
            "  \"navigation\": {\n    \"groups\": [\n      {\n",
            "        \"group\": \"Project\",\n        \"pages\": [\n          {\n",
            "            \"group\": \"Library\",\n            \"pages\": [\n",
            "              \"project/library/README\"\n",
            "            ]\n          }\n        ]\n      }\n    ]\n  },\n",
            "  \"theme\": \"mint\"\n}\n",
        )
    );
}

fn modules() -> Vec<Page> {
    vec![
        Page::Path("project/reference/index".into()),
        Page::Group {
            group: "HTTP".into(),
            pages: vec![
                Page::Path("project/reference/HTTP/index".into()),
                Page::Path("project/reference/HTTP/Client".into()),
                Page::Group {
                    group: "Response".into(),
                    pages: vec![Page::Path("project/reference/HTTP/Response/index".into())],
                },
            ],
        },
    ]
}

#[test]
fn nested_modules_preserve_exact_outer_ownership_routes_and_order() {
    let result = configuration(modules(), 4096).expect("nested configuration");
    assert_eq!(result, configuration(modules(), 4096).expect("repeat"));
    let document: Value = serde_json::from_slice(&result).expect("complete JSON");
    assert_eq!(
        document["navigation"]["groups"],
        json!([{
            "group":"Project",
            "pages":[{
                "group":"Library",
                "pages":["project/reference/index", {
                    "group":"HTTP",
                    "pages":["project/reference/HTTP/index", "project/reference/HTTP/Client", {
                        "group":"Response",
                        "pages":["project/reference/HTTP/Response/index"]
                    }]
                }]
            }]
        }])
    );
}

#[test]
fn recursive_navigation_uses_the_existing_exact_configuration_byte_ceiling() {
    let expected = configuration(modules(), 4096).expect("complete configuration");
    let size = u64::try_from(expected.len()).expect("bounded fixture length");
    assert_eq!(configuration(modules(), size).expect("exact fit"), expected);
    assert!(matches!(
        configuration(modules(), size - 1),
        Err(Error::Tree(gazette_core::source::Error::Limit { .. }))
    ));
}

#[test]
fn multiple_projects_get_a_selector_with_their_exact_sections_and_routes() {
    let config = Config {
        preview_link_bases: None,
        public_base_url: None,
        name: "Developer documentation",
        theme: "mint",
        primary_color: "#123abc",
        light_color: None,
        dark_color: None,
        telemetry: None,
        usage: None,
        metadata: &Default::default(),
    };
    let result = encode(
        &config,
        vec![
            Group {
                group: "First library",
                pages: vec![Page::Group {
                    group: "Architecture".into(),
                    pages: vec![Page::Path("first/architecture".into())],
                }],
            },
            Group {
                group: "Second library",
                pages: vec![Page::Reference(ReferenceGroup {
                    group: "HTTP API".into(),
                    input: Input::Openapi(super::OpenApi {
                        source: "artifacts/second/openapi.json".into(),
                        directory: "second/api".into(),
                    }),
                })],
            },
        ],
        4096,
    )
    .unwrap();
    let document: Value = serde_json::from_slice(&result).unwrap();
    assert_eq!(
        document["navigation"],
        json!({"dropdowns":[
            {"dropdown":"First library","pages":[{
                "group":"Architecture","pages":["first/architecture"]
            }]},
            {"dropdown":"Second library","pages":[{
                "group":"HTTP API","openapi":{
                    "source":"artifacts/second/openapi.json","directory":"second/api"
                }
            }]}
        ]})
    );
}
