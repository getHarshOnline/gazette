//! Shallow discovery reuses native authored facts without inventing relationships or identities.
use std::collections::BTreeMap;

use gazette_compiler::{
    authored,
    corpus::{Input, ParsedCorpus},
    workspace,
};
use serde_json::json;

fn configuration() -> workspace::Compilation {
    workspace::compile(
        "[[principals]]\nid='person:writer'\nkind='person'\nname='Writer & Researcher'\nwebsite={kind='site',site='site:b'}\nsame_as=['https://identity.test/writer']\n\
         [[sites]]\nid='site:a'\npublisher='person:writer'\ndeployable=false\n\
         [[sites]]\nid='site:b'\npublisher='person:writer'\ndeployable=false\n\
         [[sources]]\nid='source:one'\nlocation={kind='filesystem',root='/not-read'}\n\
         [[brands]]\nid='brand:a'\nname='A'\n[[brands.publications]]\nsite='site:a'\npublic_base_url='https://a.test/press/'\nauthored=[{source='source:one',target='a',static_assets={route_prefix='/assets/'}}]\n\
         [[brands]]\nid='brand:b'\nname='B'\n[[brands.publications]]\nsite='site:b'\npublic_base_url='https://b.test/research/'\nauthored=[{source='source:one',target='b',static_assets={route_prefix='/assets/'}}]\nlistings=[{content='page:index',select={kind='standalone-articles'},order={first=[],remaining='title-then-id'},pagination={page_size=15,next_pages='page/{number}/'}}]\ntaxonomy_pages=[{taxonomy='taxonomy:first',route='/subjects/',template='taxonomy.html'},{taxonomy='taxonomy:first',term='term:shared',route='/subjects/a/',template='term.html'},{taxonomy='taxonomy:third',route='/third/',template='taxonomy.html'}]\n\
         [[taxonomies]]\nid='taxonomy:first'\nlabel='First scope'\nterms=[{id='term:shared',label='First meaning'}]\n\
         [[taxonomies]]\nid='taxonomy:second'\nlabel='Second scope'\nterms=[{id='term:shared',label='Second meaning'}]\n\
         [[taxonomies]]\nid='taxonomy:third'\nlabel='Index-only scope'\nterms=[{id='term:shared',label='Still no term landing'}]\n",
        Default::default(),
    )
    .unwrap()
}

fn files() -> Vec<(String, Vec<u8>)> {
    [
        ("a/source.md", "+++\nrelated=['page:target','page:bare']\n[content]\nid='page:source'\nsite='site:a'\ntitle='Source'\nlifecycle='published'\nroute={path='/source/'}\nstructure={kind='page',family={kind='article'}}\n+++\n# Source\n"),
        ("b/target.md", "+++\n[listing_artwork]\ndefault={source='source:one',entry='art.webp'}\n[content]\nid='page:target'\nsite='site:b'\ntitle='Target <with> precision'\ndescription='Authored account.'\nlifecycle='published'\nlanguage='en-US'\npublished='2026-10-27'\nmodified='2026-10-28T12:30:00.125+05:30'\nattributions=[{principal='person:writer',role='author'}]\ntaxonomies=[{taxonomy='taxonomy:first',term='term:shared'},{taxonomy='taxonomy:second',term='term:shared'},{taxonomy='taxonomy:third',term='term:shared'}]\nroute={path='/target/'}\nstructure={kind='page',family={kind='article'}}\n+++\n# Target\nNever copy this target body into a recommendation.\n"),
        ("b/bare.md", "+++\n[content]\nid='page:bare'\nsite='site:b'\ntitle='Bare'\nlifecycle='published'\nroute={path='/bare/'}\nstructure={kind='page',family={kind='article'}}\n+++\n# Bare\n"),
        ("b/index.md", "+++\n[content]\nid='page:index'\nsite='site:b'\ntitle='Existing index'\nlifecycle='published'\nroute={path='/blog/'}\nstructure={kind='page',family={kind='index'}}\n+++\n"),
    ]
    .into_iter()
    .map(|(path, text)| (path.to_owned(), text.as_bytes().to_vec()))
    .chain(std::iter::once(("b/art.webp".to_owned(), b"retained opaque WebP".to_vec())))
    .collect()
}

fn parse(files: &[(String, Vec<u8>)]) -> ParsedCorpus<'_> {
    ParsedCorpus::parse(
        files.iter().map(|(path, bytes)| Input {
            path: path.parse().unwrap(),
            bytes,
        }),
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .unwrap()
}

#[test]
fn recommendation_metadata_matches_listing_facts_without_changing_ranking_or_graph() {
    let files = files();
    let corpus = parse(&files);
    let config = configuration();
    let resolved = authored::resolve(&config, &corpus, Default::default()).unwrap();
    let discovery = resolved.discovery().unwrap();
    let page = resolved
        .project_page(
            &"page:source".parse().unwrap(),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    let recommendations = json!(page.data.recommendations);
    let items = recommendations["items"].as_array().unwrap();
    assert_eq!(recommendations["algorithm"], "gazette-related/5");
    assert_eq!(
        items
            .iter()
            .map(|v| v["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["page:bare", "page:target"]
    );
    for item in items {
        assert_eq!(item["score"], 120);
        assert_eq!(item["reasons"], json!(["outgoing-related"]));
        assert_eq!(item["destination"]["kind"], "cross-site");
        assert_eq!(item["destination"]["site"], "site:b");
        assert!(item.get("body").is_none() && item.get("structure").is_none());
    }
    let target = &items[1];
    let listings = resolved
        .project_listing_pages(&"site:b".parse().unwrap(), &discovery)
        .unwrap();
    let listing = json!(listings);
    let listed = listing[0]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["id"] == "page:target")
        .unwrap();
    for field in ["artwork", "language", "published", "modified"] {
        assert_eq!(
            target[field], listed[field],
            "one native shallow fact: {field}"
        );
    }
    assert_eq!(
        target["attributions"][0]["principal"],
        listed["attributions"][0]["principal"]
    );
    assert_eq!(
        target["attributions"][0]["website"]["url"],
        listed["attributions"][0]["website"]["url"]
    );
    assert_eq!(
        target["attributions"][0]["website"]["destination"]["kind"],
        "cross-site"
    );
    assert_eq!(
        listed["attributions"][0]["website"]["destination"]["kind"],
        "internal-native"
    );
    assert_eq!(
        target["artwork"]["default"],
        "https://b.test/research/assets/art.webp"
    );
    assert_eq!(
        target["attributions"][0]["principal"]["same_as"],
        json!(["https://identity.test/writer"])
    );
    assert_eq!(target["published"], "2026-10-27");
    assert_eq!(target["modified"], "2026-10-28T12:30:00.125+05:30");
    for absent in [
        "artwork",
        "language",
        "published",
        "modified",
        "attributions",
        "classifications",
    ] {
        assert!(
            items[0].get(absent).is_none(),
            "bare recommendation invents no {absent}"
        );
    }
    assert!(
        !recommendations
            .to_string()
            .contains("Never copy this target body")
    );
    let linked = json!(page.json_ld());
    assert!(
        linked.get("image").is_none(),
        "decorative listing artwork creates no semantic image assertion"
    );
    assert!(
        linked.get("about").is_none(),
        "recommendations create no subject assertion"
    );
    assert_eq!(
        resolved.content().graph().related().len(),
        2,
        "recommendations add no graph edges"
    );
}

#[test]
fn classification_destinations_use_exact_site_taxonomy_term_and_separate_public_urls() {
    let files = files();
    let corpus = parse(&files);
    let config = configuration();
    let resolved = authored::resolve(&config, &corpus, Default::default())
        .unwrap()
        .with_preview_urls(BTreeMap::from([
            (
                "site:a".parse().unwrap(),
                "http://127.0.0.1:4400/press/".parse().unwrap(),
            ),
            (
                "site:b".parse().unwrap(),
                "http://127.0.0.1:4401/research/".parse().unwrap(),
            ),
        ]))
        .unwrap();
    let local = resolved
        .project_page(
            &"page:target".parse().unwrap(),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    let classified = json!(local.data.classifications);
    assert_eq!(
        classified[0]["destination"]["url"],
        "http://127.0.0.1:4401/research/subjects/a/"
    );
    assert_eq!(
        classified[0]["destination"]["canonical_url"],
        "https://b.test/research/subjects/a/"
    );
    assert_eq!(
        classified[0]["destination"]["destination"]["kind"],
        "internal-native"
    );
    assert_eq!(
        classified[0]["taxonomy"]["destination"]["url"],
        "http://127.0.0.1:4401/research/subjects/"
    );
    assert!(classified[1].get("destination").is_none());
    assert!(
        classified[1]["taxonomy"].get("destination").is_none(),
        "equal term key never borrows another taxonomy route"
    );
    assert_eq!(classified[1]["term"]["label"], "Second meaning");
    assert!(
        classified[2].get("destination").is_none(),
        "a taxonomy index is not a missing term's fallback"
    );
    assert_eq!(
        classified[2]["taxonomy"]["destination"]["url"],
        "http://127.0.0.1:4401/research/third/"
    );
    let source = resolved
        .project_page(
            &"page:source".parse().unwrap(),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    let suggested = json!(source.data.recommendations);
    let target = suggested["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["id"] == "page:target")
        .unwrap();
    assert_eq!(
        target["classifications"][0]["destination"]["destination"]["kind"],
        "cross-site"
    );
    assert_eq!(
        target["classifications"][0]["destination"]["destination"]["site"],
        "site:b"
    );
    let graph = resolved
        .project_site_graph(&"site:b".parse().unwrap(), Default::default())
        .unwrap();
    assert_eq!(json!(graph)["classifications"]["page:target"], classified);
    let linked = json!(local.json_ld());
    assert_eq!(linked["headline"], linked["name"]);
    assert!(
        !linked["gazette:classifications"][0]["@id"]
            .as_str()
            .unwrap()
            .contains("subjects"),
        "navigation route does not replace concept identity"
    );
}

#[test]
fn repeated_configured_classification_addresses_refuse_before_destination_fanout() {
    let terms = (0..12)
        .map(|i| format!("{{id='term:t{i}',label='T{i}'}}"))
        .collect::<Vec<_>>()
        .join(",");
    let memberships = (0..12)
        .map(|i| format!("{{taxonomy='taxonomy:one',term='term:t{i}'}}"))
        .collect::<Vec<_>>()
        .join(",");
    let long = "x".repeat(600);
    let config = workspace::compile(&format!(
        "principals=[{{id='person:one',kind='person'}}]\nsites=[{{id='site:a',publisher='person:one',deployable=false}}]\nsources=[{{id='source:one',location={{kind='filesystem',root='/not-read'}}}}]\n\
         [[brands]]\nid='brand:a'\nname='A'\n[[brands.publications]]\nsite='site:a'\npublic_base_url='https://a.test/'\nauthored=[{{source='source:one',target='a'}}]\ntaxonomy_pages=[{{taxonomy='taxonomy:one',route='/{long}/',template='taxonomy.html'}}]\n\
         [[taxonomies]]\nid='taxonomy:one'\nlabel='One'\nterms=[{terms}]\n"), Default::default()).unwrap();
    let text = format!(
        "+++\n[content]\nid='page:a'\nsite='site:a'\ntitle='A'\nlifecycle='published'\ntaxonomies=[{memberships}]\nroute={{path='/a/'}}\nstructure={{kind='page',family={{kind='article'}}}}\n+++\n# A\n"
    );
    let corpus = ParsedCorpus::parse(
        [Input {
            path: "a/one.md".parse().unwrap(),
            bytes: text.as_bytes(),
        }],
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .unwrap();
    let limits = gazette_core::publication::content::Limits {
        text_bytes: 4096,
        ..Default::default()
    };
    let resolved = authored::resolve(&config, &corpus, limits).unwrap();
    assert!(matches!(
        resolved.project_page(
            &"page:a".parse().unwrap(),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak
        ),
        Err(authored::ProjectionError::Limit(
            "classification destination URI bytes"
        ))
    ));
    assert!(matches!(
        resolved.project_site_graph(&"site:a".parse().unwrap(), limits),
        Err(authored::SiteGraphError::Limit(
            "classification destination URI bytes"
        ))
    ));
}
