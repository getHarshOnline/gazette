//! Actual resolver-to-JSON-LD semantics, not independent processor certification.

#[path = "json_ld/public.rs"]
mod public;

use super::{doc, fixtures, key, parse};
use gazette_compiler::{authored, workspace};
use gazette_core::render::LimitedWriter;
use serde_json::{Value, json};

fn configuration() -> workspace::Compilation {
    workspace::compile(configuration_source(), Default::default())
        .expect("native owner declarations")
}

fn configuration_source() -> &'static str {
    "[[principals]]\nid='owner:one'\nkind='organization'\n\
         [[principals]]\nid='principal:writer'\nkind='person'\n\
         [[principals]]\nid='principal:rights'\nkind='organization'\n\
         [[principals]]\nid='principal:unused'\nkind='person'\n\
         [[sites]]\nid='site:a'\npublisher='owner:one'\ndeployable=false\n\
         [[sites]]\nid='site:b'\npublisher='owner:one'\ndeployable=false\n\
         [[projects]]\nid='project:p'\nlifecycle='done'\nvisibility='private'\navailability='unavailable'\n\
         [[projects]]\nid='project:q'\nlifecycle='planned'\nvisibility='public'\navailability='gated'\n\
         [[projects]]\nid='project:source'\nlifecycle='active'\nvisibility='private'\navailability='unavailable'\n\
         [[sources]]\nid='source:private'\nlocation={kind='repository',root='/private/json-ld-proof',commit='1111111111111111111111111111111111111111',subdirectory='secret'}\n\
         [[extensions]]\nid='schema:custom'\nschema={type='object',additionalProperties=false}\n\
         [[brands]]\nid='brand:one'\nname='Not a publisher or author'\n\
         [[brands.publications]]\nsite='site:a'\npublic_base_url='https://one.test/mount/'\n\
         [[placements]]\nrole='primary'\nsite='site:a'\nproject='project:source'\nrender=true\ndiscoverable=false\ndocuments={source='source:private',target='native'}\n\
         [[taxonomies]]\nid='taxonomy:a'\nlabel='First scope'\nlanguage='EN-us'\nterms=[{id='term:shared',label='First meaning'},{id='term:unused',label='Never emit this'}]\n\
         [[taxonomies]]\nid='taxonomy:b'\nlabel='Second scope'\nterms=[{id='term:shared',label='Second meaning'}]\n"
}

fn files() -> Vec<(&'static str, String)> {
    let mut files = fixtures();
    files[4].0 = "native/article.md";
    files[4].1 = files[4].1.replace(
        "lifecycle='draft'",
        "lifecycle='draft'\nlanguage='EN-us'\nroute={path='/article/',aliases=['/retired/']}\n\
         attributions=[{principal='principal:rights',role='rights-holder'},{principal='principal:writer',role='author'},{principal='principal:writer',role='rights-holder'}]\n\
         taxonomies=[{taxonomy='taxonomy:a',term='term:shared'},{taxonomy='taxonomy:b',term='term:shared'}]",
    );
    files
}

fn with_resolved(inspect: impl FnOnce(&authored::ResolvedAuthored<'_, '_>)) {
    let compilation = configuration();
    let files = files();
    let corpus = parse(&files, Default::default());
    let resolved =
        authored::resolve(&compilation, &corpus, Default::default()).expect("one resolved corpus");
    inspect(&resolved);
}

fn projection(resolved: &authored::ResolvedAuthored<'_, '_>, id: &str) -> Value {
    let page = resolved
        .project_page(
            &key(id),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("retained page projection");
    serde_json::to_value(page.json_ld()).expect("JSON-LD serialization")
}

fn identifiers(values: &Value) -> Vec<&str> {
    values
        .as_array()
        .expect("explicit sequence")
        .iter()
        .map(|value| {
            value["identifier"]
                .as_str()
                .expect("original literal identity")
        })
        .collect()
}

#[test]
fn breadcrumbs_and_neighbors_project_the_same_paths_with_distinct_article_page_identity() {
    with_resolved(|resolved| {
        let page = resolved
            .project_page(
                &key("article:a"),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .expect("article");
        let value = serde_json::to_value(page.json_ld()).expect("linked data");
        assert_eq!(value["@type"], "Article");
        assert!(
            value.get("breadcrumb").is_none(),
            "breadcrumb belongs on WebPage, not Article"
        );
        let presentation = &value["mainEntityOfPage"];
        assert_eq!(presentation["@type"], "WebPage");
        assert_eq!(presentation["identifier"], value["identifier"]);
        assert_ne!(presentation["@id"], value["@id"]);
        assert_eq!(presentation["url"], value["url"]);
        assert_eq!(value["gazette:breadcrumbs"]["@type"], "@json");
        assert_eq!(
            value["gazette:breadcrumbs"]["@value"],
            serde_json::to_value(&page.data.breadcrumbs).expect("same view")
        );
        let trails = presentation["breadcrumb"].as_array().expect("all trails");
        assert_eq!(trails.len(), page.data.breadcrumbs.len());
        for (trail, native) in trails.iter().zip(&page.data.breadcrumbs) {
            assert_eq!(trail["@type"], "BreadcrumbList");
            assert_eq!(trail["gazette:via"]["identifier"], native.via.as_str());
            assert_eq!(trail["numberOfItems"], native.items.len());
            let items = trail["itemListElement"].as_array().expect("ordered items");
            assert_eq!(items.len(), native.items.len());
            for (index, (item, link)) in items.iter().zip(&native.items).enumerate() {
                assert_eq!(item["@type"], "ListItem");
                assert_eq!(item["position"], index + 1);
                assert_eq!(item["item"]["identifier"], link.node.id.as_str());
                assert_eq!(item["item"]["name"], link.node.title);
                assert_eq!(
                    item["item"].get("url").and_then(Value::as_str),
                    link.route.map(|route| route.canonical_url.as_str())
                );
            }
            assert_eq!(
                items.last().expect("self endpoint")["item"]["@id"],
                presentation["@id"]
            );
        }
        let membership = &value["gazette:journeys"][0];
        assert_eq!(membership["gazette:position"], 1);
        assert_eq!(membership["gazette:total"], 2);
        assert_eq!(membership["gazette:primary"], true);
        assert_eq!(membership["gazette:previous"]["identifier"], "docs:local");
        assert_eq!(
            membership["gazette:previous"]["creativeWorkStatus"],
            "archived"
        );
        assert_eq!(
            membership["gazette:previous"]["gazette:site"]["identifier"],
            "site:a"
        );
        assert!(membership["gazette:previous"].get("url").is_none());
        assert!(membership.get("gazette:next").is_none());
        let docs = projection(resolved, "docs:local");
        assert_eq!(membership["gazette:previous"]["@id"], docs["@id"]);
        assert_eq!(
            docs["gazette:journeys"][0]["gazette:next"]["@id"],
            presentation["@id"]
        );
        assert!(docs.get("mainEntityOfPage").is_none());
        assert!(docs["breadcrumb"].is_array());
        let collection = projection(resolved, "collection:root");
        assert_eq!(collection["@type"], "CollectionPage");
        assert_eq!(
            collection["breadcrumb"][0]["itemListElement"][0]["item"]["@id"],
            collection["@id"]
        );
    });
}

#[test]
fn author_rights_holder_and_publisher_are_separate_exact_roles_without_inference() {
    with_resolved(|resolved| {
        let value = projection(resolved, "article:a");
        assert_eq!(
            value["@context"],
            json!({"@version":1.1,"@vocab":"https://schema.org/","gazette":"urn:gazette:publication:","url":{"@id":"https://schema.org/url","@type":"@id"}})
        );
        assert!(
            value["publisher"].get("@type").is_none(),
            "a bare publisher key does not invent a principal kind"
        );
        assert_eq!(value["publisher"]["identifier"], "owner:one");
        assert_eq!(identifiers(&value["author"]), ["principal:writer"]);
        assert_eq!(value["author"][0]["@type"], "Person");
        assert_eq!(
            identifiers(&value["copyrightHolder"]),
            ["principal:rights", "principal:writer"]
        );
        assert_eq!(value["copyrightHolder"][0]["@type"], "Organization");
        assert_eq!(value["copyrightHolder"][1]["@type"], "Person");
        assert_eq!(
            value["author"][0]["@id"], value["copyrightHolder"][1]["@id"],
            "one principal across two distinct roles"
        );
        assert_ne!(value["publisher"]["@id"], value["author"][0]["@id"]);
        let ordered = value["gazette:attributions"]["@list"]
            .as_array()
            .expect("declared role order is an explicit JSON-LD list");
        assert_eq!(
            ordered
                .iter()
                .map(|role| role["gazette:role"].as_str().expect("role"))
                .collect::<Vec<_>>(),
            ["rights-holder", "author", "rights-holder"]
        );
        assert_eq!(
            ordered[0]["gazette:principal"]["identifier"],
            "principal:rights"
        );
        assert_eq!(
            ordered[1]["gazette:principal"]["@id"],
            value["author"][0]["@id"]
        );
        let unattributed = projection(resolved, "article:empty");
        assert!(unattributed.get("author").is_none());
        assert!(unattributed.get("copyrightHolder").is_none());
        assert_eq!(unattributed["gazette:attributions"], json!({"@list":[]}));
        for unexpected in [
            "principal:unused",
            "Not a publisher or author",
            "email",
            "license",
            "sameAs",
            "copyrightYear",
        ] {
            assert!(
                !value.to_string().contains(unexpected),
                "invented identity or rights: {unexpected}"
            );
        }
    });
}

#[test]
fn project_subjects_mentions_context_evidence_and_source_provenance_do_not_collapse() {
    with_resolved(|resolved| {
        let value = projection(resolved, "article:a");
        assert_eq!(identifiers(&value["about"]), ["project:p"]);
        assert_eq!(identifiers(&value["mentions"]), ["project:q"]);
        assert_eq!(value["creativeWorkStatus"], "draft");
        for (node, lifecycle, visibility, availability) in [
            (&value["about"][0], "done", "private", "unavailable"),
            (&value["mentions"][0], "planned", "public", "gated"),
        ] {
            assert_eq!(node["@type"], "gazette:Project");
            assert_eq!(node["gazette:lifecycle"], lifecycle);
            assert_eq!(node["gazette:visibility"], visibility);
            assert_eq!(node["gazette:availability"], availability);
            assert!(node.get("offers").is_none() && node.get("url").is_none());
        }
        let relations = value["gazette:projectRelations"]
            .as_array()
            .expect("all existing relations");
        assert_eq!(relations.len(), 2);
        assert_eq!(relations[0]["gazette:relation"], "about");
        assert_eq!(relations[1]["gazette:relation"], "mentions");
        let evidence = relations[0]["gazette:evidence"]
            .as_array()
            .expect("both declaration origins");
        assert_eq!(evidence.len(), 2);
        assert_eq!(evidence[0]["gazette:source"]["identifier"], "article:a");
        assert_eq!(evidence[0]["gazette:kind"], "about");
        assert_eq!(
            evidence[1]["gazette:source"]["identifier"],
            "collection:root"
        );
        assert_eq!(evidence[1]["gazette:kind"], "context");
        assert_eq!(
            relations[1]["gazette:evidence"][0]["gazette:source"]["identifier"],
            "journey:z"
        );
        assert_eq!(
            relations[1]["gazette:evidence"][0]["gazette:kind"],
            "context"
        );
        assert_eq!(
            value["gazette:projectDeclarations"]
                .as_array()
                .expect("direct declarations")
                .len(),
            1
        );
        assert_eq!(
            value["gazette:projectDeclarations"][0]["gazette:kind"],
            "about"
        );
        assert_eq!(
            value["gazette:provenance"]["gazette:project"]["identifier"],
            "project:source"
        );
        assert_eq!(
            value["gazette:provenance"]["gazette:site"]["identifier"],
            "site:a"
        );
        assert!(
            !relations
                .iter()
                .any(|relation| relation["gazette:project"]["identifier"] == "project:source")
        );
        let root = projection(resolved, "collection:root");
        assert!(root.get("about").is_none() && root.get("mentions").is_none());
        assert_eq!(
            root["gazette:projectDeclarations"][0]["gazette:kind"],
            "context"
        );
        let unrelated = projection(resolved, "article:empty");
        assert!(
            unrelated.get("about").is_none() && unrelated.get("mentions").is_none(),
            "editorial relatedness does not inherit subjects"
        );
    });
}

#[test]
fn editorial_residence_related_references_and_ordered_members_keep_distinct_meaning() {
    with_resolved(|resolved| {
        let page = projection(resolved, "article:a");
        assert_eq!(
            identifiers(&page["gazette:collections"]),
            Vec::<&str>::new()
        );
        assert_eq!(identifiers(&page["isPartOf"]), ["journey:z"]);
        let memberships = page["gazette:journeys"]
            .as_array()
            .expect("containing journey");
        assert_eq!(memberships.len(), 1);
        assert_eq!(memberships[0]["gazette:journey"]["identifier"], "journey:z");
        assert_eq!(memberships[0]["gazette:primary"], true);
        assert_eq!(
            memberships[0]["gazette:journey"]["creativeWorkStatus"],
            "archived"
        );
        assert_eq!(identifiers(&page["gazette:editorialRelated"]), ["docs:b"]);
        assert_eq!(
            projection(resolved, "docs:local")["gazette:collections"],
            json!([]),
            "journey's collection does not become a leaf's direct membership"
        );
        let journey = projection(resolved, "journey:z");
        assert_eq!(journey["@type"], "CollectionPage");
        assert_eq!(journey["mainEntity"]["@type"], "ItemList");
        assert_eq!(journey["mainEntity"]["numberOfItems"], 2);
        let items = journey["mainEntity"]["itemListElement"]
            .as_array()
            .expect("ordered item records");
        assert_eq!(items.len(), 2);
        for (index, (id, primary)) in [("docs:local", true), ("article:a", true)]
            .into_iter()
            .enumerate()
        {
            assert_eq!(items[index]["@type"], "ListItem");
            assert_eq!(
                items[index]["position"],
                index + 1,
                "explicit position survives unordered JSON-LD arrays"
            );
            assert_eq!(items[index]["item"]["identifier"], id);
            assert_eq!(items[index]["gazette:primary"], primary);
            assert_eq!(items[index]["item"]["@id"], projection(resolved, id)["@id"]);
        }
        assert_eq!(
            identifiers(&projection(resolved, "collection:root")["hasPart"]),
            ["collection:child", "journey:a"]
        );
        assert!(
            page.get("mainEntity").is_none(),
            "an article is not itself the enclosing journey"
        );
    });
}

#[test]
fn composite_taxonomy_identity_and_detailed_json_literals_preserve_exact_scoped_facts() {
    with_resolved(|resolved| {
        let page = resolved
            .project_page(
                &key("article:a"),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .expect("page");
        let value = serde_json::to_value(page.json_ld()).expect("JSON-LD");
        let terms = value["gazette:classifications"]
            .as_array()
            .expect("classification records");
        assert_eq!(terms.len(), 2);
        assert_eq!(terms[0]["identifier"], "term:shared");
        assert_eq!(terms[1]["identifier"], "term:shared");
        assert_ne!(
            terms[0]["@id"], terms[1]["@id"],
            "term identity includes taxonomy scope"
        );
        assert_eq!(terms[0]["name"], "First meaning");
        assert_eq!(terms[1]["name"], "Second meaning");
        assert_eq!(terms[0]["inDefinedTermSet"]["identifier"], "taxonomy:a");
        assert_eq!(terms[1]["inDefinedTermSet"]["identifier"], "taxonomy:b");
        assert_eq!(terms[0]["gazette:languageScope"], "en-US");
        assert!(terms[1].get("gazette:languageScope").is_none());
        assert_eq!(value["inLanguage"], "en-US");
        assert!(!value.to_string().contains("Never emit this"));
        for (field, expected) in [
            (
                "gazette:route",
                serde_json::to_value(page.data.route.expect("explicit route")).expect("route"),
            ),
            (
                "gazette:structure",
                serde_json::to_value(&page.data.content.structure).expect("structure"),
            ),
            (
                "gazette:headings",
                serde_json::to_value(page.data.headings).expect("retained headings"),
            ),
        ] {
            assert_eq!(value[field]["@type"], "@json");
            assert_eq!(
                value[field]["@value"], expected,
                "exact non-RDF detail survives: {field}"
            );
            assert_eq!(value[field].as_object().expect("literal object").len(), 2);
        }
        assert_eq!(value["text"], "नमस्ते\nBody strong.\n");
        for private in [
            "/private/json-ld-proof",
            "source:private",
            "native/article.md",
            "body_html",
            "source_sha256",
        ] {
            assert!(
                !value.to_string().contains(private),
                "private rendering evidence: {private}"
            );
        }
        assert!(
            value.get("gazette:derivation").is_none(),
            "explicit authored metadata has no invented native derivation"
        );
    });
}

#[test]
fn internal_document_scoped_identity_is_stable_without_becoming_a_canonical_url() {
    let compilation = configuration();
    let original = files();
    let mut moved = files();
    moved[4].0 = "native/renamed.md";
    moved[4].1 = moved[4].1.replace("path='/article/'", "path='/changed/'");
    let first = parse(&original, Default::default());
    let second = parse(&moved, Default::default());
    let first =
        authored::resolve(&compilation, &first, Default::default()).expect("original graph");
    let second = authored::resolve(&compilation, &second, Default::default()).expect("moved graph");
    let a = projection(&first, "article:a");
    let b = projection(&second, "article:a");
    assert_eq!(a["identifier"], "article:a");
    assert!(
        a["@id"]
            .as_str()
            .expect("blank identifier")
            .starts_with("_:content_")
    );
    assert_eq!(a["@id"], b["@id"]);
    assert_eq!(a["url"], "https://one.test/mount/article/");
    assert_eq!(b["url"], "https://one.test/mount/changed/");
    assert_ne!(a["@id"], a["url"]);
    assert!(
        projection(&first, "docs:b").get("url").is_none(),
        "no inferred URL from identity or filename"
    );
    assert_ne!(
        a["@id"],
        projection(&first, "article:empty")["@id"],
        "same titles do not merge content identity"
    );
    assert_eq!(
        a["gazette:projectRelations"][0]["gazette:evidence"][0]["gazette:source"]["@id"],
        a["@id"]
    );
}

#[test]
fn native_page_families_are_not_all_reclassified_as_articles() {
    let compilation = configuration();
    for (family, expected) in [
        ("{kind='article'}", "Article"),
        ("{kind='documentation'}", "WebPage"),
        ("{kind='project'}", "WebPage"),
        ("{kind='index'}", "WebPage"),
        ("{kind='utility'}", "WebPage"),
        (
            "{kind='extension',schema='schema:custom',data={}}",
            "WebPage",
        ),
    ] {
        let text = doc(
            "content:family",
            "site:a",
            "archived",
            "",
            &format!("kind='page'\nfamily={family}"),
            "Body\n",
        );
        let files = vec![("unbound.md", text)];
        let corpus = parse(&files, Default::default());
        let resolved =
            authored::resolve(&compilation, &corpus, Default::default()).expect("actual family");
        let value = projection(&resolved, "content:family");
        assert_eq!(value["@type"], expected);
        assert_eq!(value["creativeWorkStatus"], "archived");
        assert!(value.get("author").is_none() && value.get("copyrightHolder").is_none());
        assert!(value.get("url").is_none() && value.get("gazette:provenance").is_none());
    }
}

#[test]
fn streaming_serialization_obeys_the_callers_exact_bound_and_remains_repeatable() {
    with_resolved(|resolved| {
        let page = resolved
            .project_page(
                &key("article:a"),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .expect("page");
        let expected = serde_json::to_vec(&page.json_ld()).expect("complete fixture output");
        let mut exact = LimitedWriter::new(Vec::new(), expected.len());
        serde_json::to_writer(&mut exact, &page.json_ld()).expect("exact serialized ceiling");
        assert!(!exact.exceeded());
        assert_eq!(exact.into_inner(), expected);
        let mut below = LimitedWriter::new(Vec::new(), expected.len() - 1);
        assert!(serde_json::to_writer(&mut below, &page.json_ld()).is_err());
        assert!(below.exceeded(), "no successful truncated document");
        assert_eq!(
            serde_json::to_vec(&page.json_ld()).expect("independent repeat"),
            expected
        );
    });
}

#[test]
fn authored_images_are_search_metadata_and_transport_inventory_stays_in_inspection() {
    use gazette_compiler::corpus::{Input, ParsedCorpus};
    use gazette_core::source::Sha256Digest;

    let config = configuration_source().replace(
        "documents={source='source:private',target='native'}",
        "documents={source='source:private',target='native',static_assets={route_prefix='/media/'}}\nnative_markdown={route_prefix='/docs/',lifecycle='published',language='EN-us'}",
    );
    let compilation = workspace::compile(&config, Default::default())
        .expect("configured native and static transports");
    let body = "# Native **guide**\n\n![diagram](pic.png \"Figure\")\n";
    let image = b"retained opaque image bytes\0\xff";
    let corpus = ParsedCorpus::parse(
        [
            Input {
                path: "native/README.md".parse().expect("native source"),
                bytes: body.as_bytes(),
            },
            Input {
                path: "native/pic.png".parse().expect("asset source"),
                bytes: image,
            },
        ],
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("real retained corpus");
    let resolved = authored::resolve(&compilation, &corpus, Default::default())
        .expect("actual native lowering and asset resolution");
    let node = resolved
        .content()
        .graph()
        .nodes()
        .next()
        .expect("native content");
    let page = resolved
        .project_page(
            &node.id,
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("native page with selected asset");
    assert_eq!(
        page.data.assets.len(),
        1,
        "fixture must exercise a populated asset, not an empty vector"
    );
    let asset = page.data.assets[0];
    assert_eq!(asset.content(), image);
    assert_eq!(asset.sha256, Sha256Digest::of(image));
    assert_eq!(asset.url, "https://one.test/mount/media/pic.png");
    assert_eq!(asset.output.as_str(), "media/pic.png");
    let derivation = page
        .data
        .derivation
        .expect("native-only metadata evidence exists");
    assert!(derivation.language_from_profile);
    let value = serde_json::to_value(page.json_ld()).expect("native JSON-LD");
    assert!(value.get("gazette:assets").is_none());
    assert_eq!(
        value["image"],
        json!([{
            "@type": "ImageObject", "contentUrl": "https://one.test/mount/media/pic.png", "description": "diagram", "name": "Figure"
        }])
    );
    let inspected = serde_json::to_value(&page).expect("inspection remains complete");
    assert_eq!(inspected["assets"][0]["bytes"], image.len());
    assert_eq!(
        inspected["assets"][0]["sha256"],
        serde_json::to_value(Sha256Digest::of(image)).unwrap()
    );
    assert_eq!(
        value["gazette:derivation"],
        json!({"@type":"@json","@value":serde_json::to_value(derivation).expect("original derivation")})
    );
    assert_eq!(
        value["gazette:derivation"]["@value"]["title"]["kind"],
        "heading"
    );
    assert_eq!(
        value["gazette:derivation"]["@value"]["title"]["span"]["start"],
        0
    );
    assert_eq!(
        value["gazette:derivation"]["@value"]["language_from_profile"],
        true
    );
    assert_eq!(value["name"], "Native guide");
    assert_eq!(value["@type"], "WebPage");
    assert_eq!(value["inLanguage"], "en-US");
    assert_eq!(value["text"], "Native guide\ndiagram\n");
    assert_eq!(value["url"], "https://one.test/mount/docs/");
    assert!(
        value.get("about").is_none() && value.get("mentions").is_none(),
        "native project provenance never declares a subject"
    );
    for private in [
        "native/README.md",
        "native/pic.png",
        "/private/json-ld-proof",
        "source:private",
        "retained opaque image bytes",
    ] {
        assert!(
            !value.to_string().contains(private),
            "private source or payload leaked: {private}"
        );
    }
}

#[test]
fn foreign_content_stays_shallow_and_equal_literal_keys_do_not_merge_typed_namespaces() {
    let config = configuration_source().replace(
        "[[placements]]",
        "[[brands.publications]]\nsite='site:b'\npublic_base_url='https://foreign.test/'\n[[placements]]",
    );
    let compilation =
        workspace::compile(&config, Default::default()).expect("two actual routed sites");
    let local = doc(
        "owner:one",
        "site:a",
        "draft",
        "related=['site:a']\n",
        "kind='page'\nfamily={kind='article'}",
        "Local body\n",
    );
    let foreign = doc("site:a", "site:b", "archived", "related=['owner:one']\nprojects=[{project='project:p',kind='about'}]\n", "kind='page'\nfamily={kind='documentation'}", "FOREIGN BODY MUST NOT RECURSE\n")
        .replace("lifecycle='archived'", "lifecycle='archived'\nlanguage='en-US'\nroute={path='/foreign/'}\nattributions=[{principal='principal:writer',role='author'}]\ntaxonomies=[{taxonomy='taxonomy:a',term='term:shared'}]");
    let files = vec![("local.md", local), ("foreign.md", foreign)];
    let corpus = parse(&files, Default::default());
    let resolved = authored::resolve(&compilation, &corpus, Default::default())
        .expect("content identities remain separate from workspace namespaces");
    let local = projection(&resolved, "owner:one");
    let foreign = projection(&resolved, "site:a");
    assert_eq!(local["identifier"], local["publisher"]["identifier"]);
    assert_ne!(
        local["@id"], local["publisher"]["@id"],
        "content and principal with the same literal key are distinct nodes"
    );
    assert_eq!(local["gazette:site"]["identifier"], foreign["identifier"]);
    assert_ne!(
        local["gazette:site"]["@id"], foreign["@id"],
        "site and content with the same literal key are distinct nodes"
    );
    assert_eq!(
        foreign["url"], "https://foreign.test/foreign/",
        "fixture has real foreign routes"
    );
    assert_eq!(foreign["author"][0]["identifier"], "principal:writer");
    assert_eq!(foreign["about"][0]["identifier"], "project:p");
    assert_eq!(foreign["text"], "FOREIGN BODY MUST NOT RECURSE\n");
    let references = local["gazette:editorialRelated"]
        .as_array()
        .expect("one foreign endpoint");
    assert_eq!(references.len(), 1);
    let reference = &references[0];
    assert_eq!(reference["@id"], foreign["@id"]);
    assert_eq!(reference["identifier"], "site:a");
    assert_eq!(reference["gazette:site"]["identifier"], "site:b");
    assert_eq!(reference["@type"], "WebPage");
    assert_eq!(reference["creativeWorkStatus"], "archived");
    assert_eq!(reference["inLanguage"], "en-US");
    let mut fields = reference
        .as_object()
        .expect("shallow endpoint")
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    fields.sort_unstable();
    assert_eq!(
        fields,
        [
            "@id",
            "@type",
            "creativeWorkStatus",
            "gazette:site",
            "identifier",
            "inLanguage",
            "name",
            "url"
        ]
    );
    for excluded in [
        "text",
        "author",
        "publisher",
        "about",
        "gazette:projectRelations",
        "gazette:classifications",
        "gazette:editorialRelated",
        "gazette:structure",
        "gazette:route",
    ] {
        assert!(
            reference.get(excluded).is_none(),
            "foreign record recursively expanded: {excluded}"
        );
    }
    assert!(!local.to_string().contains("FOREIGN BODY MUST NOT RECURSE"));
    assert!(!local.to_string().contains("principal:writer"));
    assert_eq!(
        reference["url"], foreign["url"],
        "the existing resolved endpoint URL survives without recursively copying its body or roles"
    );
}

#[test]
fn explicit_relationship_nodes_and_journey_members_retain_already_resolved_urls() {
    let compilation = workspace::compile(
        &configuration_source().replace("[[sites]]\nid='site:b'", "[[brands]]\nid='brand:foreign'\nname='Foreign'\n[[brands.publications]]\nsite='site:b'\npublic_base_url='https://foreign.test/'\n[[sites]]\nid='site:b'"),
        Default::default(),
    ).unwrap();
    let files = files()
        .into_iter()
        .map(|(path, text)| {
            if text.contains("route={") {
                (path, text)
            } else {
                let id = text
                    .lines()
                    .find_map(|line| {
                        line.strip_prefix("id='")
                            .and_then(|line| line.strip_suffix('\''))
                    })
                    .unwrap();
                let route = format!("route={{path='/{}/'}}\n", id.replace(':', "-"));
                (
                    path,
                    text.replace(
                        "[content.structure]",
                        &format!("{route}[content.structure]"),
                    ),
                )
            }
        })
        .collect::<Vec<_>>();
    let corpus = parse(&files, Default::default());
    let resolved = authored::resolve(&compilation, &corpus, Default::default()).unwrap();
    let article = projection(&resolved, "article:a");
    assert_eq!(
        article["isPartOf"][0]["url"],
        "https://one.test/mount/journey-z/"
    );
    assert_eq!(
        article["gazette:editorialRelated"][0]["url"],
        "https://foreign.test/docs-b/"
    );
    let journey = projection(&resolved, "journey:z");
    let items = &journey["mainEntity"]["itemListElement"];
    assert_eq!(
        items[0]["item"]["url"],
        "https://one.test/mount/docs-local/"
    );
    assert_eq!(items[1]["item"]["url"], "https://one.test/mount/article/");
    assert_eq!(items[1]["item"]["@type"], "Article");
    let root = projection(&resolved, "collection:root");
    assert!(
        root["hasPart"]
            .as_array()
            .unwrap()
            .iter()
            .all(|node| node["url"].as_str().is_some())
    );
}
