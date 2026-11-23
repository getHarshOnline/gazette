//! Small configured sitemap partitions exercise real compilation, selection and staging.

use super::*;
use gazette_sdk::{authored, publication::content::Disposition};
use std::collections::BTreeSet;

fn partitioned(fixture: &Fixture) -> String {
    fixture.config().replace(
        "public_base_url=",
        "machine={sitemap=true,sitemap_urls_per_file=1}\npublic_base_url=",
    )
}

fn assert_partition(tree: &source::CapturedSource) {
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).expect("shared model");
    let urls: BTreeSet<_> = model["pages"]
        .as_object()
        .expect("canonical page map")
        .values()
        .map(|page| {
            page["route"]["canonical_url"]
                .as_str()
                .expect("canonical URL")
        })
        .collect();
    assert_eq!(urls.len(), 2, "small explicit partition fixture");
    let index = std::str::from_utf8(bytes(tree, "sitemap.xml")).expect("native XML index");
    assert!(index.contains("<sitemapindex xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">"));
    assert!(index.ends_with("</sitemapindex>"));
    assert_eq!(index.matches("<sitemap>").count(), 2);
    assert_eq!(index.matches("<loc>").count(), 2);
    let mut found = BTreeSet::new();
    for number in 1..=2 {
        let name = format!("sitemap-{number:05}.xml");
        assert!(index.contains(&format!("<loc>https://one.example/mount/{name}</loc>")));
        let child = std::str::from_utf8(bytes(tree, &name)).expect("actual child XML");
        assert!(child.contains("<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">"));
        assert!(child.ends_with("</urlset>"));
        assert_eq!(child.matches("<url>").count(), 1);
        assert_eq!(child.matches("<loc>").count(), 1);
        let matched: Vec<_> = urls
            .iter()
            .filter(|url| child.contains(&format!("<loc>{url}</loc>")))
            .collect();
        assert_eq!(
            matched.len(),
            1,
            "child selects one exact shared canonical URL"
        );
        assert!(
            found.insert(*matched[0]),
            "no duplicated canonical entry across children"
        );
        for absent in [
            "other.example",
            "/elsewhere/",
            "/old.html",
            "/copy/",
            "/private/",
        ] {
            assert!(
                !child.contains(absent),
                "non-primary or excluded route leaked: {absent}"
            );
        }
    }
    assert_eq!(
        found, urls,
        "complete shared selected URL union, no truncation"
    );
    assert_eq!(
        tree.inventory()
            .entries()
            .iter()
            .filter(|entry| {
                let name = entry.path.as_str();
                name.starts_with("sitemap") && name.ends_with(".xml")
            })
            .count(),
        3,
        "only root index and its two actual children"
    );
}

#[test]
fn configured_sitemap_index_stages_exact_mounted_children_and_retained_repeat() {
    let other = super::super::OTHER.replace(
        "route={path='/elsewhere/'}",
        "route={path='/elsewhere/',mirrors=[{site='site:one',path='/copy/'}]}",
    );
    let fixture = Fixture::new(&[("other/page.md", other.as_bytes())]);
    let config = partitioned(&fixture);
    let inputs = acquire(&config);
    let rendered = render(&inputs).expect("complete partitioned native publication");
    let tree = rendered.site().output();
    assert_partition(tree);
    assert!(
        tree.file(&"old.html".parse().expect("alias path"))
            .is_some()
    );
    assert!(
        tree.file(&"copy/index.html".parse().expect("mirror path"))
            .is_some()
    );
    let parent = tempdir().expect("CLI staging parent");
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments.extend(["--output-parent", parent.path().to_str().expect("parent")]);
    let result = cli(&config, &arguments);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let receipt: Value = serde_json::from_slice(&result.stdout).expect("native CLI receipt");
    assert_eq!(
        receipt["inventory"],
        serde_json::to_value(tree.inventory()).expect("SDK inventory")
    );
    let directory = std::path::Path::new(receipt["directory"].as_str().expect("retained output"));
    for path in ["sitemap.xml", "sitemap-00001.xml", "sitemap-00002.xml"] {
        assert_eq!(
            fs::read(directory.join(path)).expect("staged native XML"),
            bytes(tree, path)
        );
    }
    fs::write(
        fixture.directory.path().join("one/page.md"),
        b"changed after source capture",
    )
    .expect("disposable source mutation");
    let repeat = render(&inputs).expect("retained rebuild without native rereads");
    assert_eq!(repeat.site().output().inventory(), tree.inventory());
    for entry in tree.inventory().entries() {
        assert_eq!(
            repeat.site().output().file(&entry.path),
            tree.file(&entry.path)
        );
    }
}

#[test]
fn selected_sitemap_index_uses_exact_candidate_page_union() {
    let private = "+++\n[content]\nid='content:private'\nsite='site:one'\ntitle='Private'\nlifecycle='published'\nroute={path='/private/'}\nstructure={kind='page',family={kind='article'}}\n+++\nExcluded private text\n";
    let page = PAGE.replace("lifecycle='draft'", "lifecycle='published'");
    let fixture = Fixture::new(&[
        ("one/page.md", page.as_bytes()),
        ("one/private.md", private.as_bytes()),
    ]);
    let inputs = acquire(&partitioned(&fixture));
    let parsed = inputs
        .parse_publication(Default::default(), &gazette_adapter_markdown::Comrak)
        .expect("retained publication syntax");
    let resolved = parsed
        .resolve(Default::default())
        .expect("full original semantic graph");
    let intent = authored::Selection {
        content: resolved
            .authored()
            .content()
            .graph()
            .nodes()
            .map(|node| {
                (
                    node.id.clone(),
                    if node.id.as_str() == "content:private" {
                        Disposition::Exclude
                    } else {
                        Disposition::Include
                    },
                )
            })
            .collect(),
        assets: Default::default(),
    };
    let selected = resolved
        .into_selected(intent)
        .expect("explicit complete selection");
    let rendered = build::render_selected_publication(
        &selected,
        build::SiteRenderOptions {
            render_year: None,
            model_output: Some(&"model.json".parse().expect("model")),
            graph: None,
            limits: Default::default(),
        },
        Default::default(),
    )
    .expect("selected sitemap uses same selected model");
    assert_partition(rendered.site().output());
    assert!(
        rendered
            .site()
            .output()
            .file(&"private/index.html".parse().expect("excluded page"))
            .is_none()
    );
    let parent = tempdir().expect("SDK selected staging parent");
    let staged = build::stage_publication(&rendered, parent.path(), Default::default(), 16 << 10)
        .expect("actual selected stage");
    assert_eq!(
        fs::read(staged.path().join("sitemap.xml")).expect("staged index"),
        bytes(rendered.site().output(), "sitemap.xml")
    );
    staged.close().expect("release only disposable stage");
}

#[test]
fn sitemap_index_child_collisions_and_aggregate_budget_refuse_complete_output() {
    // Keep the aggregate output ceiling above the earlier site-model encoding
    // ceiling, so the shortfall reaches sitemap packaging rather than rendering.
    let template = format!("{}<!--{}-->", super::super::TEMPLATE, "x".repeat(32 << 10));
    let fixture = Fixture::new(&[("theme/templates/page.html", template.as_bytes())]);
    let config = partitioned(&fixture);
    failed_without_stage(&config, "sitemap-00001.xml", "source_inventory");
    let collision = PAGE.replace("path='/guide/'", "path='/sitemap-00002.xml'");
    let conflicting = Fixture::new(&[("one/page.md", collision.as_bytes())]);
    failed_without_stage(&partitioned(&conflicting), "model.json", "source_inventory");
    let inputs = acquire(&config);
    let complete = render(&inputs).expect("complete bounded baseline");
    let total = complete.site().output().inventory().total_bytes();
    let model = "model.json".parse().expect("model");
    let options = |total_bytes| build::SiteRenderOptions {
        render_year: Some(gazette_sdk::authored::RenderYear::new(2040).expect("fixture year")),
        model_output: Some(&model),
        graph: None,
        limits: source::Limits {
            total_bytes,
            ..Default::default()
        },
    };
    let exact = build::render_publication(&inputs, options(total), Default::default())
        .expect("exact aggregate output bound admits all children and index");
    assert_eq!(
        exact.site().output().inventory(),
        complete.site().output().inventory()
    );
    // The site model is emitted after the sitemap package. Leave one byte too
    // little for the complete package itself, without reducing its per-file bound.
    let model_bytes = bytes(complete.site().output(), "model.json").len() as u64;
    let before_model = total - model_bytes;
    assert!(
        before_model - 1 > model_bytes,
        "aggregate shortfall must leave the earlier complete model encoding in bounds"
    );
    let failure = build::render_publication(&inputs, options(before_model - 1), Default::default())
        .expect_err("aggregate shortfall cannot return a partial sitemap or drop pages");
    assert_eq!(failure.code(), "source_limit");
}
