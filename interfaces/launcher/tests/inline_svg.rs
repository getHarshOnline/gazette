//! The native CLI retains external SVG bytes while embedding a validated occurrence.
use serde_json::Value;
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="300" height="90" viewBox="0 0 100 100" aria-labelledby="label"><title id="label">A live label</title><text x="10" y="30" fill="var(--color-ink, #292722)" font-family="var(--type-family-reading, sans-serif)">Readable text</text></svg>"##;

#[test]
fn cli_embeds_page_package_svg_without_changing_fallback_or_markdown_identity() {
    let source = tempfile::tempdir().unwrap();
    let output = tempfile::tempdir().unwrap();
    for (name, bytes) in [
        (
            "theme/theme.toml",
            "package='theme:one'\nnamespace='fixture'\ntemplates='templates'\nbrowser_theme_colors={light='#ffffff',dark='#000000'}\nrender={page='page.html'}\n",
        ),
        ("theme/templates/page.html", "{{ body_html | safe }}"),
        (
            "content/guide/index.md",
            "+++\n[[images]]\ndefault='figure.svg'\ninline=true\nwidth=480\n[content]\nid='content:guide'\nsite='site:one'\ntitle='Guide'\nlifecycle='published'\nroute={path='/guide/'}\nstructure={kind='page',family={kind='article'}}\n+++\n# Guide\n\n![Meaningful alt](figure.svg \"Caption title\")\n",
        ),
        ("content/guide/figure.svg", SVG),
    ] {
        let path = source.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    let config = format!(
        "[composition]\ntheme={{package='theme:one'}}\n[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n[[brands]]\nid='brand:one'\nname='Fixture'\n[[brands.publications]]\nsite='site:one'\npublic_base_url='https://example.test/'\nauthored=[{{source='source:content',target='content'}}]\nmachine={{markdown_alternates={{enabled=true,directory_page='index.md',file_page='append-md',media_type='text/markdown',discovery_rel='alternate'}}}}\n[[sources]]\nid='source:content'\nlocation={{kind='filesystem',root={:?}}}\n[[sources]]\nid='source:theme'\nlocation={{kind='filesystem',root={:?}}}\n[[theme_packages]]\npackage='theme:one'\nsource='source:theme'\nmanifest='theme.toml'\n",
        source.path().join("content").to_str().unwrap(),
        source.path().join("theme").to_str().unwrap()
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args([
            "render-site",
            "--local",
            "--site",
            "site:one",
            "--model-output",
            "model.json",
            "--output-parent",
        ])
        .arg(output.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(config.as_bytes())
        .unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let receipt: Value = serde_json::from_slice(&result.stdout).unwrap();
    let stage = Path::new(receipt["directory"].as_str().unwrap());
    let model: Value =
        serde_json::from_slice(&fs::read(stage.join("model.json")).unwrap()).unwrap();
    let image = &model["pages"]["content:guide"]["images"][0];
    let address = image["address"].as_str().unwrap();
    let html = fs::read_to_string(stage.join("guide/index.html")).unwrap();
    assert!(html.contains(&format!("data-gazette-image-ref=\"{address}\"")));
    assert!(html.contains(&format!("<img hidden data-gazette-image=\"{address}\"")));
    assert!(html.contains("var(--type-family-reading, sans-serif)"));
    assert!(html.contains("width=\"480\" height=\"144\""));
    assert!(html.contains("viewBox=\"0 0 100 100\""));
    assert_eq!(image["width"], 480);
    assert_eq!(image["alt"], "Meaningful alt");
    assert_eq!(image["title"], "Caption title");
    assert_eq!(
        image["destination"],
        "https://example.test/guide/figure.svg"
    );
    assert_eq!(
        fs::read_to_string(stage.join("guide/figure.svg")).unwrap(),
        SVG
    );
    let markdown = fs::read_to_string(stage.join("guide/index.md")).unwrap();
    assert!(
        markdown
            .contains("![Meaningful alt](https://example.test/guide/figure.svg \"Caption title\")")
    );
    assert!(!markdown.contains("<svg"));
}
