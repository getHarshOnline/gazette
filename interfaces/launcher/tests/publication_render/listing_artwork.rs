//! Native retained acquisition, listing model, HTML and static bytes share one image selection.
use super::{Fixture, acquire, bytes, render};
use serde_json::Value;
#[test]
fn native_listing_artwork_preserves_original_image_bytes_and_escaped_serving_urls() {
    let collection = "+++\n[listing_artwork]\ndefault={source='source:one',entry='media/image.webp'}\n[content]\nid='collection:picture'\nsite='site:one'\ntitle='Picture collection'\nlifecycle='published'\nroute={path='/picture/'}\nstructure={kind='collection'}\n+++\nAn unchanged account.\n";
    let index = "+++\n[content]\nid='content:directory'\nsite='site:one'\ntitle='Directory'\nlifecycle='published'\nroute={path='/directory/'}\nstructure={kind='page',family={kind='index'}}\n+++\n";
    let template=b"<h1>{{content.title}}</h1>{{body_html|safe}}{% if listing is defined %}{% for item in listing.items %}{% if item.artwork is defined %}<img alt=\"\" src=\"{{item.artwork.default}}\">{% endif %}{% endfor %}{% endif %}";
    let image = b"opaque retained WebP transport";
    let fixture = Fixture::new(&[
        ("one/picture.md", collection.as_bytes()),
        ("one/directory.md", index.as_bytes()),
        ("one/media/image.webp", image),
        ("theme/templates/page.html", template),
    ]);
    let configuration=fixture.config().replace("target='one'}","target='one',static_assets={route_prefix='/assets/input/'}}").replace("public_base_url='https://one.example/mount/'","public_base_url='https://one.example/mount/'\nlistings=[{content='content:directory',select={kind='root-collections'},order={first=[],remaining='title-then-id'},pagination={page_size=10,next_pages='page/{number}/'}}]");
    let inputs = acquire(&configuration);
    let rendered = render(&inputs).unwrap();
    let output = rendered.site().output();
    let model: Value = serde_json::from_slice(bytes(output, "model.json")).unwrap();
    let items = model["pages"]["content:directory"]["listing"]["items"]
        .as_array()
        .unwrap();
    let selected = items
        .iter()
        .find(|item| item["id"] == "collection:picture")
        .unwrap();
    assert_eq!(
        selected["artwork"]["default"],
        "https://one.example/mount/assets/input/media/image.webp"
    );
    assert_eq!(selected["artwork"].as_object().unwrap().len(), 1);
    assert_eq!(bytes(output, "assets/input/media/image.webp"), image);
    let html = std::str::from_utf8(bytes(output, "directory/index.html")).unwrap();
    assert!(html.contains(
        "https:&#x2F;&#x2F;one.example&#x2F;mount&#x2F;assets&#x2F;input&#x2F;media&#x2F;image.webp"
    ));
    assert!(!html.contains("source:one"));
}
