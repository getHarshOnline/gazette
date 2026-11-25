//! Reading context uses the same parsed graph, discovery custody and routed renderer.

use super::*;
use std::collections::BTreeMap;

fn fixture(hidden: &[&str]) -> (String, Vec<(String, String)>) {
    let mut config = configuration().replace(
        "site='site:home'\npublic_base_url",
        &format!("site='site:home'\nmachine={{markdown_alternates={ALTERNATES}}}\npublic_base_url"),
    );
    if !hidden.is_empty() {
        config.push_str("\n[[projects]]\nid='project:hidden'\nlifecycle='active'\nvisibility='private'\navailability='unavailable'\n[[placements]]\nsite='site:home'\nproject='project:hidden'\nrole='primary'\nrender=true\ndiscoverable=false\ndocuments={source='source:private',target='hidden'}\n");
    }
    let article = "{kind='page',family={kind='article'}}";
    let records = [
        (
            "collection:root",
            "root",
            "Root collection",
            "{kind='collection'}",
        ),
        (
            "collection:child",
            "child",
            "Subcollection",
            "{kind='collection',parent='collection:root'}",
        ),
        (
            "journey:ordered",
            "journey",
            "Ordered journey",
            "{kind='journey',collection='collection:child',members=[{content='content:z',primary=true},{content='content:m',primary=true},{content='content:a',primary=true}]}",
        ),
        ("content:z", "z", "Zed article", article),
        ("content:m", "m", "Middle 🦀 article", article),
        ("content:a", "a", "Alpha article", article),
        ("content:direct", "direct", "Direct child", article),
    ];
    let files = records
        .into_iter()
        .map(|(id, slug, title, structure)| {
            let mut source = document(
                id,
                "home",
                title,
                "published",
                &format!("/{slug}/"),
                structure,
            );
            if id == "content:direct" {
                source = source.replacen("+++\n", "+++\ncollections=['collection:child']\n", 1);
            }
            let directory = if hidden.contains(&id) {
                "hidden"
            } else {
                "home"
            };
            (format!("{directory}/{slug}.md"), source)
        })
        .collect();
    (config, files)
}

fn parse(files: &[(String, String)]) -> ParsedCorpus<'_> {
    ParsedCorpus::parse(
        files.iter().map(|(path, bytes)| Input {
            path: path.parse().unwrap(),
            bytes: bytes.as_bytes(),
        }),
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("retained native TOML and Comrak AST")
}

fn alternate(owner: &authored::ResolvedAuthored<'_, '_>, id: &str) -> authored::MarkdownAlternate {
    let route = owner.content().route(&key(id)).unwrap();
    owner
        .project_site_pages(&key("site:home"))
        .unwrap()
        .markdown_alternates(&[], &[], 1 << 20)
        .unwrap()
        .into_iter()
        .find(|alternate| alternate.html_url == route.url)
        .expect("same complete canonical alternate plan")
}

fn page<'a>(
    owner: &'a authored::ResolvedAuthored<'_, '_>,
    discovery: &authored::Discovery<'_>,
    id: &str,
) -> authored::PageProjection<'a> {
    owner
        .project_discovery_page(
            &key(id),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
            discovery,
        )
        .expect("ordinary complete native page projection")
}

fn markdown(
    owner: &authored::ResolvedAuthored<'_, '_>,
    discovery: &authored::Discovery<'_>,
    id: &str,
) -> String {
    String::from_utf8(
        page(owner, discovery, id)
            .markdown_alternate(&alternate(owner, id), discovery, 1 << 20)
            .unwrap(),
    )
    .unwrap()
}

fn destinations(markdown: &str) -> Vec<String> {
    ParsedMarkdown::parse(
        markdown,
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .unwrap()
    .inspect()
    .unwrap()
    .links
    .into_iter()
    .map(|link| link.destination)
    .collect()
}

#[test]
fn alternate_preserves_subject_source_credit_as_distinct_from_page_authorship() {
    let (mut config, mut files) = fixture(&[]);
    config = config.replace(
        "id='principal:publisher'\nkind='person'",
        "id='principal:publisher'\nkind='organization'\nname='Paper Press'",
    );
    config.push_str(
        "\n[[principals]]\nid='person:developer'\nkind='person'\nname='Ada Maintainer'\n\
         website={kind='external',url='https://author.test/about/'}\n\
         [[projects]]\nid='project:engine'\nkind='software-application'\nname='Paper Engine'\n\
         lifecycle='active'\nvisibility='public'\navailability='available'\n\
         repository='https://code.test/paper'\n\
         source={license_url='https://code.test/paper/license',citation_url='https://code.test/paper/citation',author='person:developer',rights_holder='principal:publisher'}\n",
    );
    for (path, source) in &mut files {
        let relation = match path.as_str() {
            "home/m.md" => "about",
            "home/a.md" => "mentions",
            _ => continue,
        };
        *source = attributed(
            source.replacen(
                "[content]",
                &format!("projects=[{{project='project:engine',kind='{relation}'}}]\n[content]"),
                1,
            ),
            "{principal='principal:publisher',role='rights-holder'}",
        );
    }
    let compilation = workspace::compile(&config, Default::default()).unwrap();
    let corpus = parse(&files);
    let owner = authored::resolve(&compilation, &corpus, Default::default()).unwrap();
    let discovery = owner.discovery().unwrap();
    let subject = markdown(&owner, &discovery, "content:m");
    assert!(subject.contains("Rights holder: Paper Press"));
    assert!(!subject.contains("\nAuthor:"));
    assert!(subject.contains("Source code for [Paper Engine](https://code.test/paper) is credited to [Ada Maintainer](https://author.test/about/)."));
    assert!(!markdown(&owner, &discovery, "content:a").contains("Ada Maintainer"));
}

#[test]
fn native_collection_subcollection_and_declared_journey_order_use_resolved_public_and_local_links()
{
    let (config, files) = fixture(&[]);
    let compilation = workspace::compile(&config, Default::default()).unwrap();
    let corpus = parse(&files);
    for local in [false, true] {
        let resolved = authored::resolve(&compilation, &corpus, Default::default()).unwrap();
        let owner = if local {
            resolved
                .with_preview_urls(BTreeMap::from([
                    (
                        key("site:home"),
                        "http://127.0.0.1:4321/mount/".parse().unwrap(),
                    ),
                    (
                        key("site:foreign"),
                        "http://127.0.0.1:4322/".parse().unwrap(),
                    ),
                ]))
                .unwrap()
        } else {
            resolved
        };
        let discovery = owner.discovery().unwrap();
        let base = if local {
            "http://127.0.0.1:4321/mount/"
        } else {
            "https://home.test/mount/"
        };
        let root = markdown(&owner, &discovery, "collection:root");
        assert!(
            root.contains(&format!(
                "## Collection contents\n\n- [Subcollection]({base}child/)\n"
            )),
            "{root}"
        );
        let child = markdown(&owner, &discovery, "collection:child");
        assert!(child.contains("## Collection contents"));
        for (label, slug) in [("Direct child", "direct"), ("Ordered journey", "journey")] {
            assert!(
                child.contains(&format!("- [{label}]({base}{slug}/)\n")),
                "{child}"
            );
        }
        let journey_page = page(&owner, &discovery, "journey:ordered");
        assert_eq!(
            journey_page
                .data
                .steps
                .iter()
                .map(|step| step.id.as_str())
                .collect::<Vec<_>>(),
            ["content:z", "content:m", "content:a"]
        );
        let journey = markdown(&owner, &discovery, "journey:ordered");
        assert!(journey.contains(&format!("## Journey sequence\n\n1. [Zed article]({base}z/)\n2. [Middle 🦀 article]({base}m/)\n3. [Alpha article]({base}a/)\n")), "{journey}");
        let middle = markdown(&owner, &discovery, "content:m");
        assert!(middle.contains(&format!("Reading path: [Root collection]({base}root/) > [Subcollection]({base}child/) > [Ordered journey]({base}journey/) > [Middle 🦀 article]({base}m/)")), "{middle}");
        assert!(middle.contains(&format!("Previous in journey: [Zed article]({base}z/)")));
        assert!(middle.contains(&format!("Next in journey: [Alpha article]({base}a/)")));
        let direct = markdown(&owner, &discovery, "content:direct");
        assert!(direct.contains(&format!("Reading path: [Root collection]({base}root/) > [Subcollection]({base}child/) > [Direct child]({base}direct/)")));
        assert!(!direct.contains("in journey:"));
        for output in [&root, &child, &journey, &middle, &direct] {
            assert!(destinations(output).iter().all(|url| url.starts_with(base)));
            for private in [
                "+++",
                "source:private",
                "/private/discovery-fixture",
                "home/",
                "members=",
            ] {
                assert!(!output.contains(private), "{output}");
            }
        }
    }
}

#[test]
fn hidden_member_omits_whole_journey_context_without_a_shortened_false_sequence() {
    let (config, files) = fixture(&["content:m", "content:direct"]);
    let compilation = workspace::compile(&config, Default::default()).unwrap();
    let corpus = parse(&files);
    let owner = authored::resolve(&compilation, &corpus, Default::default()).unwrap();
    let discovery = owner.discovery().unwrap();
    assert!(!discovery.contains(&key("content:m")));
    assert!(
        discovery.contains(&key("journey:ordered")),
        "the container itself is selected"
    );
    let journey_page = page(&owner, &discovery, "journey:ordered");
    assert_eq!(
        journey_page.data.steps.len(),
        3,
        "retained structural model is never shrunk"
    );
    let journey = markdown(&owner, &discovery, "journey:ordered");
    assert!(!journey.contains("## Journey sequence"));
    for hidden in ["Middle 🦀 article", "/m/", "1. [Zed", "2. [Alpha"] {
        assert!(!journey.contains(hidden), "{journey}");
    }
    let child = markdown(&owner, &discovery, "collection:child");
    for hidden in ["Direct child", "/direct/", "Ordered journey", "/journey/"] {
        assert!(!child.contains(hidden), "{child}");
    }
    for id in ["content:z", "content:a"] {
        let article = markdown(&owner, &discovery, id);
        for generated in [
            "Reading path:",
            "Previous in journey:",
            "Next in journey:",
            "Middle 🦀 article",
            "/m/",
        ] {
            assert!(!article.contains(generated), "{article}");
        }
    }
}

#[test]
fn hidden_ancestor_omits_whole_reading_path_but_not_an_independently_complete_journey() {
    let (config, files) = fixture(&["collection:child"]);
    let compilation = workspace::compile(&config, Default::default()).unwrap();
    let corpus = parse(&files);
    let owner = authored::resolve(&compilation, &corpus, Default::default()).unwrap();
    let discovery = owner.discovery().unwrap();
    let middle = markdown(&owner, &discovery, "content:m");
    for hidden in ["Reading path:", "Subcollection", "/child/"] {
        assert!(!middle.contains(hidden), "{middle}");
    }
    assert!(middle.contains("Previous in journey: [Zed article]"));
    assert!(middle.contains("Next in journey: [Alpha article]"));
    let root = markdown(&owner, &discovery, "collection:root");
    assert!(!root.contains("Subcollection"));
    assert!(!root.contains("## Collection contents"));
}

#[test]
fn exact_byte_limits_and_foreign_owner_refusal_apply_before_any_partial_artifact() {
    let (config, files) = fixture(&[]);
    let compilation = workspace::compile(&config, Default::default()).unwrap();
    let corpus = parse(&files);
    let owner = authored::resolve(&compilation, &corpus, Default::default()).unwrap();
    let foreign = authored::resolve(&compilation, &corpus, Default::default()).unwrap();
    let discovery = owner.discovery().unwrap();
    let foreign_discovery = foreign.discovery().unwrap();
    for id in [
        "collection:root",
        "collection:child",
        "journey:ordered",
        "content:m",
    ] {
        let page = page(&owner, &discovery, id);
        let alternate = alternate(&owner, id);
        let complete = page
            .markdown_alternate(&alternate, &discovery, 1 << 20)
            .unwrap();
        assert_eq!(
            page.markdown_alternate(&alternate, &discovery, complete.len())
                .unwrap(),
            complete
        );
        assert_eq!(
            page.markdown_alternate(&alternate, &discovery, complete.len() - 1)
                .unwrap_err()
                .code(),
            "discovery_limit"
        );
        assert_eq!(
            page.markdown_alternate(&alternate, &foreign_discovery, 1 << 20)
                .unwrap_err()
                .code(),
            "site_discovery_owner",
            "value-equivalent re-resolution is not the same custody"
        );
    }
}
