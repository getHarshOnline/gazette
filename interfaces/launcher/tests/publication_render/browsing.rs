//! Declared directory paths cross the retained native model and Tera boundary.

use serde_json::Value;

use super::{Fixture, PAGE, acquire, bytes, render};

#[test]
fn authored_browsing_reaches_native_model_and_html_without_editorial_reparenting() {
    let page = PAGE.replacen("+++\n", "+++\nbrowsing=['content:directory']\n", 1);
    let index = "+++\n[content]\nid='content:directory'\nsite='site:one'\ntitle='Index <with> & distinctions'\nlifecycle='draft'\nroute={path='/different-index/'}\nstructure={kind='page',family={kind='index'}}\n+++\n# Authored choices\n";
    let template=b"<!doctype html><html><head><title>{{content.title}}</title></head><body>{% if browsing_locations is defined %}{% for location in browsing_locations %}<a data-via=\"{{location.via}}\" href=\"{{location.url}}\">{{location.title}}</a>{% endfor %}{% endif %}{{body_html|safe}}</body></html>";
    let fixture = Fixture::new(&[
        ("theme/templates/page.html", template),
        ("one/page.md", page.as_bytes()),
        ("one/directory.md", index.as_bytes()),
    ]);
    let inputs = acquire(&fixture.config());
    let rendered = render(&inputs).expect("connected native browsing");
    let output = rendered.site().output();
    let model: Value = serde_json::from_slice(bytes(output, "model.json")).unwrap();
    let page = &model["pages"]["content:one"];
    assert_eq!(page["browsing_locations"][0]["index"], "content:directory");
    assert_eq!(page["browsing_locations"][0]["via"], "content:one");
    assert_eq!(
        page["browsing_locations"][0]["url"],
        "https://one.example/mount/different-index/"
    );
    assert!(page["browsing_locations"][0].get("page_count").is_none());
    assert_eq!(
        page["breadcrumbs"][0]["via"], "collection:home",
        "the existing editorial residence is unchanged"
    );
    let html = std::str::from_utf8(bytes(output, "guide/index.html")).unwrap();
    assert!(html.contains("data-via=\"content:one\""));
    assert!(html.contains("Index &lt;with&gt; &amp; distinctions"));
    assert!(!html.contains("Index <with>"));
    assert!(
        model["pages"]["content:directory"]
            .get("browsing_locations")
            .is_none()
    );
}
