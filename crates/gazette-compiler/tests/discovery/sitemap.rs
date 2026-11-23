//! Partitioned discovery exercises the existing resolved corpus and native XML writer.

use super::*;
use gazette_core::source::{CapturedSource, Limits as OutputLimits};

fn configured(count: usize) -> String {
    configuration().replace(
        "site='site:home'\npublic_base_url",
        &format!("site='site:home'\nmachine={{sitemap=true,sitemap_urls_per_file={count}}}\npublic_base_url"),
    )
}

fn bytes<'a>(tree: &'a CapturedSource, path: &str) -> &'a [u8] {
    tree.file(&path.parse().expect("output path"))
        .expect("complete output")
}

#[test]
fn partition_setting_is_bounded_site_owned_and_requires_sitemap_selection() {
    for count in [1, 50_000] {
        let compiled = workspace::compile(&configured(count), Default::default()).unwrap();
        assert_eq!(
            compiled
                .workspace
                .publication_for(&key("site:home"))
                .unwrap()
                .machine
                .sitemap_urls_per_file,
            Some(count)
        );
        assert_eq!(
            compiled
                .workspace
                .publication_for(&key("site:foreign"))
                .unwrap()
                .machine
                .sitemap_urls_per_file,
            None
        );
    }
    for invalid in [
        configured(0),
        configured(50_001),
        configured(1).replace("sitemap=true", "sitemap=false"),
        configured(1).replace("sitemap=true,", ""),
        configured(1).replace("sitemap_urls_per_file=1", "sitemap_urls_per_file=-1"),
        configured(1).replace("sitemap_urls_per_file=1", "sitemap_urls_per_file='1'"),
        configured(1).replace(
            "sitemap_urls_per_file=1",
            "sitemap_urls_per_file=1,sitemap_urls_per_file=2",
        ),
    ] {
        assert!(
            workspace::compile(&invalid, Default::default()).is_err(),
            "reject {invalid}"
        );
    }
}

#[test]
fn small_sets_preserve_exact_single_file_and_partitioned_union_is_the_same_selection() {
    with_pages(&configured(2), &files(), |pages, _| {
        let tree = pages
            .sitemaps(OutputLimits::default())
            .expect("single sitemap");
        assert_eq!(tree.inventory().entries().len(), 1);
        assert_eq!(bytes(&tree, "sitemap.xml"), pages.sitemap(1 << 20).unwrap());
    });
    with_pages(&configured(1), &files(), |pages, _| {
        let tree = pages
            .sitemaps(OutputLimits::default())
            .expect("partitioned set");
        assert_eq!(tree.inventory().entries().len(), 3);
        assert_eq!(
            xml_values(bytes(&tree, "sitemap.xml")),
            [
                "https://home.test/mount/sitemap-00001.xml",
                "https://home.test/mount/sitemap-00002.xml",
            ]
        );
        assert!(
            std::str::from_utf8(bytes(&tree, "sitemap.xml"))
                .unwrap()
                .contains("<sitemapindex xmlns=")
        );
        let all: Vec<_> = ["sitemap-00001.xml", "sitemap-00002.xml"]
            .into_iter()
            .flat_map(|path| xml_values(bytes(&tree, path)))
            .collect();
        assert_eq!(
            all,
            pages
                .pages()
                .map(|page| page.route.canonical_url.clone())
                .collect::<Vec<_>>()
        );
        for entry in tree.inventory().entries() {
            let value = std::str::from_utf8(tree.file(&entry.path).unwrap()).unwrap();
            for absent in ["lastmod", "foreign.test", "retired", "Private body"] {
                assert!(!value.contains(absent));
            }
        }
        let repeated = pages.sitemaps(OutputLimits::default()).unwrap();
        assert_eq!(tree.inventory(), repeated.inventory());
    });
}

#[test]
fn byte_partitioning_measures_escaped_entries_without_using_aggregate_as_a_threshold() {
    let mut source = files();
    for (_, text) in &mut source {
        *text = text
            .replace("path='/a/'", &format!("path='/a{}/'", "x".repeat(600)))
            .replace("path='/z/'", &format!("path='/z{}/'", "x".repeat(600)));
    }
    let mut single_file_ceiling = 0;
    with_pages(&configured(1), &source, |pages, _| {
        let tree = pages.sitemaps(OutputLimits::default()).unwrap();
        single_file_ceiling = bytes(&tree, "sitemap-00001.xml").len() as u64;
    });
    with_pages(&configured(50_000), &source, |pages, _| {
        let tree = pages
            .sitemaps(OutputLimits {
                file_bytes: single_file_ceiling,
                ..OutputLimits::default()
            })
            .expect("split by exact XML bytes");
        assert_eq!(tree.inventory().entries().len(), 3);
        for entry in tree.inventory().entries() {
            assert!(tree.file(&entry.path).unwrap().len() as u64 <= single_file_ceiling);
        }
        let expected = tree.inventory().total_bytes();
        let exact = OutputLimits {
            entries: 3,
            file_bytes: single_file_ceiling,
            total_bytes: expected,
            ..OutputLimits::default()
        };
        assert_eq!(tree.inventory(), pages.sitemaps(exact).unwrap().inventory());
        let failed = pages
            .sitemaps(OutputLimits {
                total_bytes: expected - 1,
                ..exact
            })
            .unwrap_err();
        assert_eq!(failed.code(), "source_limit");
        assert_eq!(
            pages
                .sitemaps(OutputLimits {
                    file_bytes: single_file_ceiling - 1,
                    ..exact
                })
                .unwrap_err()
                .code(),
            "discovery_limit"
        );
    });
}

#[test]
fn index_and_inventory_bounds_reject_whole_set_without_truncation() {
    with_pages(&configured(1), &files(), |pages, _| {
        let tree = pages.sitemaps(OutputLimits::default()).unwrap();
        let index_length = bytes(&tree, "sitemap.xml").len() as u64;
        assert!(index_length > bytes(&tree, "sitemap-00001.xml").len() as u64);
        assert_eq!(
            pages
                .sitemaps(OutputLimits {
                    file_bytes: index_length - 1,
                    ..OutputLimits::default()
                })
                .unwrap_err()
                .code(),
            "discovery_limit"
        );
        assert!(
            pages
                .sitemaps(OutputLimits {
                    entries: 2,
                    ..OutputLimits::default()
                })
                .is_err()
        );
        assert!(
            pages
                .sitemaps(OutputLimits {
                    depth: 0,
                    ..OutputLimits::default()
                })
                .is_err()
        );
        let mut reversed = files();
        reversed.reverse();
        with_pages(&configured(1), &reversed, |other, _| {
            assert_eq!(
                tree.inventory(),
                other.sitemaps(OutputLimits::default()).unwrap().inventory()
            );
        });
    });
}
