//! Explicit site roles cross actual pinned Tera, SDK output and native CLI staging.

use std::convert::Infallible;

use gazette_sdk::html::{self, TextKind};
use serde_json::json;

use super::*;

const ROLE_TEMPLATE: &[u8] = b"<!doctype html><html><head><title>Roles</title><script type=\"application/ld+json\">{{ json_ld | json_script }}</script></head><body><p>publisher={{ site.publisher }}</p>{% for binding in site.roles %}<p>{{ binding.role }}={{ binding.principal.id }}:{{ binding.principal.name }}</p>{% endfor %}{{ body_html | safe }}</body></html>";
const GRAPH_TEMPLATE: &[u8] = b"<!doctype html><html><head><title>Roles</title></head><body><script type=\"application/json\">{{ roles | json_script }}</script></body></html>";

fn json_scripts(bytes: &[u8]) -> Vec<Value> {
    let mut values = Vec::new();
    html::inspect(bytes, Default::default(), |kind, value| {
        if kind == TextKind::JsonScript {
            values.push(serde_json::from_str(value).expect("native JSON script"));
        }
        Ok::<_, Infallible>(())
    })
    .expect("native HTML tokenizer");
    values
}

#[test]
fn explicit_roles_render_through_pinned_theme_and_cli_without_sibling_or_author_leakage() {
    let fixture = Fixture::new(&[
        ("theme/templates/page.html", ROLE_TEMPLATE),
        ("theme/templates/graph.html", GRAPH_TEMPLATE),
    ]);
    let config = format!("{}\n[[principals]]\nid='principal:controller'\nkind='organization'\nname='Controller & operator'\n[[principals]]\nid='principal:unused'\nkind='person'\nname='UNRELATED ROSTER CANARY'\n", fixture.config())
        .replace("id='site:one'\n", "id='site:one'\nroles=[{principal='principal:controller',role='site-operator'},{principal='principal:controller',role='canon-authority'},{principal='principal:controller',role='data-controller'}]\n")
        .replace("public_base_url=", "machine={json_ld=true}\npublic_base_url=");
    for (site, content, output) in [
        ("site:one", "content:one", "guide/index.html"),
        ("site:other", "content:other", "elsewhere/index.html"),
    ] {
        let inputs = build::acquire_publication(
            &config,
            &site.parse().expect("configured site"),
            Default::default(),
            source::PreparationLimits {
                sources: 8,
                entries: 128,
                bytes: 1024 * 1024,
                per_source: source::RevisionLimits::default(),
            },
        )
        .expect("exact committed Theme and authored inputs");
        let rendered = build::render_publication(
            &inputs,
            build::SiteRenderOptions {
                render_year: Some(
                    gazette_sdk::authored::RenderYear::new(2040).expect("fixture year"),
                ),
                model_output: Some(&"model.json".parse().expect("site model")),
                graph: Some(build::GraphRenderOptions {
                    output: &"graph.html".parse().expect("graph output"),
                    limits: Default::default(),
                }),
                limits: Default::default(),
            },
            Default::default(),
        )
        .expect("actual pinned Tera rendering");
        let tree = rendered.site().output();
        let model: Value =
            serde_json::from_slice(bytes(tree, "model.json")).expect("emitted site model");
        let page = &model["pages"][content];
        let html = std::str::from_utf8(bytes(tree, output)).expect("native HTML bytes");
        let scripts = json_scripts(bytes(tree, output));
        assert_eq!(scripts.len(), 1);
        let linked = &scripts[0];
        let aggregate: Value =
            serde_json::from_slice(bytes(tree, "site.jsonld")).expect("selected JSON-LD artifact");
        let node = aggregate["@graph"]
            .as_array()
            .expect("site nodes")
            .iter()
            .find(|node| node["identifier"] == content)
            .expect("same selected content");
        assert_eq!(node, &page["json_ld"]);
        assert_eq!(node["@id"], linked["@id"]);
        assert_eq!(node["publisher"], linked["publisher"]);
        super::json_ld::assert_public(node);
        super::json_ld::assert_public(linked);
        let parsed = build::parse_publication(&inputs, Default::default()).unwrap();
        let resolved = parsed.resolve(Default::default()).unwrap();
        let projection = resolved
            .authored()
            .project_page(
                &content.parse().unwrap(),
                &gazette_sdk::raster::inspect,
                &gazette_adapter_markdown::Comrak,
            )
            .unwrap();
        let native = json!(projection.json_ld());
        assert_eq!(
            json_scripts(bytes(tree, "graph.html")),
            [model["graph"]["roles"].clone()]
        );
        assert_eq!(page["site"]["publisher"], "owner:one");
        assert_eq!(linked["publisher"]["identifier"], "owner:one");
        assert!(linked.get("author").is_none());
        assert!(node.get("author").is_none());
        assert_eq!(page["attributions"], json!([]));
        assert_eq!(native["gazette:attributions"], json!({"@list":[]}));
        if site == "site:one" {
            let roles = page["site"]["roles"].as_array().expect("bound role views");
            assert_eq!(roles.len(), 3);
            for (index, role) in ["canon-authority", "data-controller", "site-operator"]
                .iter()
                .enumerate()
            {
                assert!(html.contains(&format!(
                    "{role}=principal:controller:Controller &amp; operator"
                )));
                assert_eq!(roles[index]["role"], *role);
                assert_eq!(
                    roles[index]["principal"],
                    model["graph"]["principals"]["principal:controller"]
                );
                assert_eq!(
                    model["graph"]["roles"][index],
                    json!({"principal":"principal:controller","role":role})
                );
                assert_eq!(
                    native["gazette:siteRoles"]["@list"][index]["gazette:principal"]["identifier"],
                    "principal:controller"
                );
                assert_eq!(
                    native["gazette:siteRoles"]["@list"][index]["gazette:site"]["identifier"],
                    site
                );
            }
        } else {
            assert_eq!(page["site"]["roles"], json!([]));
            assert_eq!(model["graph"]["roles"], json!([]));
            assert_eq!(native["gazette:siteRoles"], json!({"@list":[]}));
            assert!(!model.to_string().contains("principal:controller"));
            assert!(!html.contains("Controller &amp; operator"));
        }
        assert!(!model.to_string().contains("UNRELATED ROSTER CANARY"));
        assert!(!html.contains("UNRELATED ROSTER CANARY"));
        let parent = tempdir().expect("private CLI output parent");
        let mut arguments = RENDER_ARGUMENTS.to_vec();
        arguments[3] = site;
        arguments.extend([
            "--graph-output",
            "graph.html",
            "--output-parent",
            parent.path().to_str().expect("stage parent"),
        ]);
        let result = cli(&config, &arguments);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let receipt: Value =
            serde_json::from_slice(&result.stdout).expect("actual CLI staged result");
        assert_eq!(
            receipt["inventory"],
            serde_json::to_value(tree.inventory()).expect("SDK inventory")
        );
        let directory = std::path::Path::new(receipt["directory"].as_str().expect("native stage"));
        for path in [output, "model.json", "graph.html", "site.jsonld"] {
            assert_eq!(
                fs::read(directory.join(path)).expect("read native CLI bytes"),
                bytes(tree, path)
            );
        }
    }
}
