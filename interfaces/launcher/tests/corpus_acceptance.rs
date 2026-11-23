//! Opt-in native owner-corpus proof; no owner configuration or content is rewritten.
//!
//! Run with absolute `GAZETTE_CORPUS_WORKSPACE` and `GAZETTE_CORPUS_RECIPE` paths:
//! ```sh
//! cargo test -p gazette-launcher --test corpus_acceptance --locked --offline -- --ignored --nocapture
//! ```
//! The configured pinned math provider executes. Outputs and the explicit neutral
//! Theme exist only in one private disposable directory, not owner repositories.

#[allow(dead_code)]
#[path = "support/mcp.rs"]
mod mcp_carrier;

#[path = "corpus_acceptance/authoring.rs"]
mod retained_authoring;

use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use gazette_sdk::{build, configuration, source, toml, workspace};
use serde_json::{Value, json};

const INPUT_BYTES: usize = 1 << 20;
const SITES: [&str; 6] = [
    "site:getharsh",
    "site:rawthoughts",
    "site:causality",
    "site:post-agi-economics",
    "site:sleepwalker",
    "site:dao-studio",
];
const NEUTRAL: &str = "theme:acceptance-neutral";
const RETAINED_ONLY: [&str; 2] = [".DS_Store", "collections/.DS_Store"];
const RETENTION_BINDINGS: [(&str, &str, &[&str]); 2] = [
    ("site:getharsh", "source:getharsh-content", &RETAINED_ONLY),
    (
        "site:rawthoughts",
        "source:rawthoughts-content",
        &[".DS_Store"],
    ),
];
const MANIFEST: &str = "package='theme:acceptance-neutral'\nnamespace='acceptance'\n\
templates='templates'\nbrowser_theme_colors={light='#ffffff',dark='#000000'}\n\
render={page='page.html',alias='alias.html',graph='graph.html'}\n";
const PAGE: &str = "<!doctype html><html><body data-theme=\"acceptance-neutral\">\
<article data-content=\"{{ content.id }}\">\
{% if headings | filter(attribute='level', value=1) | length == 0 %}<h1>{{ content.title }}</h1>{% endif %}\
<main>{{ body_html | safe }}</main>\
{% if notes is defined %}<aside id=\"acceptance-notes\">{{ notes.body_html | safe }}</aside>{% endif %}\
{% if presentation.markdown_alternate is defined %}<a rel=\"alternate\" type=\"text/markdown\" href=\"{{ presentation.markdown_alternate.url }}\">Markdown</a>{% endif %}\
<script id=\"acceptance-links\" type=\"application/json\">{{ links | json_script }}</script>\
<script id=\"acceptance-editorial\" type=\"application/json\">{\"content\":{{ content | json_script }},\"site\":{{ site | json_script }},\"attributions\":{{ attributions | json_script }},\"collections\":{{ collections | json_script }},\"journeys\":{{ journeys | json_script }},\"breadcrumbs\":{{ breadcrumbs | json_script }},\"steps\":{{ steps | json_script }},\"children\":{{ children | json_script }},\"presentation\":{{ presentation | json_script }}{% if listing is defined %},\"listing\":{{ listing | json_script }}{% endif %}{% if notes is defined %},\"notes\":{{ notes | json_script }}{% endif %}}</script>\
{% if route is defined %}<script id=\"acceptance-route\" type=\"application/json\">{{ route | json_script }}</script>{% endif %}\
{% if legal_processing is defined %}<script id=\"acceptance-processing\" type=\"application/json\">{{ legal_processing | json_script }}</script>{% endif %}\
{% if json_ld is defined %}<script type=\"application/ld+json\">{{ json_ld | json_script }}</script>{% endif %}\
</article></body></html>";
const ALIAS: &str = "<a href=\"{{ route.canonical_url }}\">Moved</a>";
const GRAPH: &str = "<!doctype html><h1>Neutral structural graph</h1>\
<script id=\"acceptance-graph\" type=\"application/json\">{{ nodes | json_script }}</script>\
<script id=\"acceptance-relations\" type=\"application/json\">{\"memberships\":{{ memberships | json_script }},\"containers\":{{ containers | json_script }},\"journeys\":{{ journeys | json_script }}{% if listings is defined %},\"listings\":{{ listings | json_script }}{% endif %}}</script>";

type Files = BTreeMap<String, BTreeMap<String, Vec<u8>>>;

fn selected_path(name: &str) -> PathBuf {
    let path = PathBuf::from(env::var_os(name).unwrap_or_else(|| panic!("set {name}")));
    assert!(
        path.is_absolute(),
        "{name} must be an explicit absolute path"
    );
    path
}

fn syntax() -> toml::Limits {
    toml::Limits {
        source_bytes: INPUT_BYTES,
        ..Default::default()
    }
}

fn segment(name: &str) -> toml::PathSegment {
    toml::PathSegment::Key(name.to_owned())
}

fn edits(text: &str, replacements: &[(Vec<toml::PathSegment>, String)]) -> String {
    let parsed = toml::ParsedToml::parse(text, syntax()).expect("native configuration syntax");
    let mut ranges = replacements
        .iter()
        .map(|(path, value)| {
            let span = parsed.span_at(path).expect("exact original native field");
            (span, value)
        })
        .collect::<Vec<_>>();
    ranges.sort_by_key(|(span, _)| span.start);
    assert!(ranges.windows(2).all(|r| r[0].0.end <= r[1].0.start));
    let mut changed = text.to_owned();
    for (span, value) in ranges.into_iter().rev() {
        changed.replace_range(span.start..span.end, value);
    }
    toml::ParsedToml::parse(&changed, syntax()).expect("substitution is valid native syntax");
    changed
}

fn supplied_configuration(
    documents: &BTreeMap<source::EntryPath, String>,
    root: &source::EntryPath,
    includes: &[source::EntryPath],
    outputs: &BTreeMap<String, PathBuf>,
    neutral: &Path,
) -> (String, BTreeMap<source::EntryPath, String>) {
    // This is a bounded acceptance input, not a replacement include resolver.
    // The native loader has already admitted this exact document set. These
    // documents contain explicit top-level table headers; the final native
    // compiler must still admit the complete supplied document independently.
    let mut assembled = String::new();
    let mut modular = BTreeMap::new();
    let mut changed_sites = BTreeSet::new();
    let mut changed_outputs = BTreeSet::new();
    let mut changed_retention = BTreeSet::new();
    for path in std::iter::once(root).chain(includes) {
        let text = &documents[path];
        let parsed = toml::ParsedToml::parse(text, syntax()).expect("original syntax");
        let value: Value = parsed
            .deserialize()
            .expect("native bounded value projection");
        let mut replacements = Vec::new();
        if let Some(sites) = value["sites"].as_array() {
            for (index, site) in sites.iter().enumerate() {
                let id = site["id"].as_str().expect("native site identity");
                if outputs.contains_key(id) {
                    assert!(changed_sites.insert(id.to_owned()));
                    replacements.push((
                        vec![
                            segment("sites"),
                            toml::PathSegment::Index(index),
                            segment("composition"),
                            segment("theme"),
                        ],
                        format!("{{package={NEUTRAL:?}}}"),
                    ));
                }
            }
        }
        if let Some(brands) = value["brands"].as_array() {
            for (brand, declaration) in brands.iter().enumerate() {
                for (publication, settings) in declaration["publications"]
                    .as_array()
                    .expect("native publication declarations")
                    .iter()
                    .enumerate()
                {
                    let site = settings["site"].as_str().expect("publication site");
                    if let Some(directory) = outputs.get(site) {
                        assert!(changed_outputs.insert(site.to_owned()));
                        replacements.push((
                            vec![
                                segment("brands"),
                                toml::PathSegment::Index(brand),
                                segment("publications"),
                                toml::PathSegment::Index(publication),
                                segment("output"),
                                segment("directory"),
                            ],
                            serde_json::to_string(
                                directory.to_str().expect("UTF-8 temporary output"),
                            )
                            .expect("TOML basic string"),
                        ));
                        if let Some((_, selected_source, entries)) = RETENTION_BINDINGS
                            .iter()
                            .find(|(selected_site, _, _)| *selected_site == site)
                        {
                            for (binding, source) in settings["authored"]
                                .as_array()
                                .expect("original authored selections")
                                .iter()
                                .enumerate()
                            {
                                if source["source"].as_str() != Some(*selected_source) {
                                    continue;
                                }
                                assert!(
                                    changed_retention.insert(*selected_source),
                                    "one exact observed Content binding per source"
                                );
                                assert!(
                                    source.get("retained_only").is_none(),
                                    "original owner configuration has no acceptance policy"
                                );
                                let field = vec![
                                    segment("brands"),
                                    toml::PathSegment::Index(brand),
                                    segment("publications"),
                                    toml::PathSegment::Index(publication),
                                    segment("authored"),
                                    toml::PathSegment::Index(binding),
                                ];
                                let span =
                                    parsed.span_at(&field).expect("exact native inline binding");
                                let original = text[span.start..span.end].trim_end();
                                let table = original
                                    .strip_suffix('}')
                                    .expect("native inline binding table");
                                replacements.push((
                                    field,
                                    format!(
                                        "{table}, retained_only={} }}",
                                        serde_json::to_string(entries).unwrap()
                                    ),
                                ));
                            }
                        }
                    }
                }
            }
        }
        let changed = edits(text, &replacements);
        modular.insert(path.clone(), changed.clone());
        assembled.push_str(&if path == root {
            edits(&changed, &[(vec![segment("includes")], "[]".to_owned())])
        } else {
            changed
        });
        assembled.push('\n');
    }
    let expected: BTreeSet<_> = SITES.iter().map(|s| (*s).to_owned()).collect();
    assert_eq!(changed_sites, expected);
    assert_eq!(changed_outputs, expected);
    assert_eq!(
        changed_retention,
        RETENTION_BINDINGS
            .iter()
            .map(|(_, source, _)| *source)
            .collect(),
        "explicit acceptance-only disposition for exact observed opaque files"
    );
    // Existing world bindings remain declared but unselected. Their unavailable
    // manifests must not be mistaken for a neutral-package dependency.
    let neutral_declarations = format!(
        "\n[[sources]]\nid='source:acceptance-neutral'\nlocation={{kind='filesystem',root={:?}}}\n\
         [[theme_packages]]\npackage='{NEUTRAL}'\nsource='source:acceptance-neutral'\nmanifest='gazette-theme.toml'\n",
        neutral.to_str().expect("UTF-8 temporary Theme"),
    );
    assembled.push_str(&neutral_declarations);
    modular
        .get_mut(root)
        .expect("original root")
        .push_str(&neutral_declarations);
    workspace::compile(&assembled, syntax()).expect("independently admitted supplied workspace");
    (assembled, modular)
}

fn bytes<'a>(files: &'a Files, site: &str, path: &str) -> &'a [u8] {
    files
        .get(site)
        .expect("selected site")
        .get(path)
        .unwrap_or_else(|| panic!("missing {site}/{path}"))
}

fn text<'a>(files: &'a Files, site: &str, path: &str) -> &'a str {
    std::str::from_utf8(bytes(files, site, path)).expect("native UTF-8 output")
}

fn output_files(rendered: &build::RenderedPublicationSet<'_>) -> Files {
    rendered
        .sites()
        .iter()
        .map(|(site, outcome)| {
            let tree = outcome
                .as_ref()
                .expect("all six actual outcomes")
                .site()
                .output();
            let files = tree
                .inventory()
                .entries()
                .iter()
                .filter_map(|entry| match entry.content {
                    source::Content::Directory => None,
                    source::Content::File { .. } => Some((
                        entry.path.to_string(),
                        tree.file(&entry.path)
                            .expect("every inventoried file is retained")
                            .to_vec(),
                    )),
                })
                .collect();
            (site.to_string(), files)
        })
        .collect()
}

fn script(html: &str, id: &str) -> Value {
    let marker = format!("<script id=\"{id}\" type=\"application/json\">");
    let body = html
        .split_once(&marker)
        .expect("neutral template consumed native context")
        .1;
    let encoded = body.split_once("</script>").expect("script end").0;
    serde_json::from_str(encoded).unwrap_or_else(|error| {
        let tail: String = encoded
            .chars()
            .rev()
            .take(160)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        panic!(
            "native JSON script {id}: {error}; {} bytes; final characters {tail:?}",
            encoded.len()
        );
    })
}

fn decoded_attributes(html: &str) -> Vec<String> {
    let mut values = Vec::new();
    gazette_sdk::html::inspect(html.as_bytes(), Default::default(), |kind, value| {
        if kind == gazette_sdk::html::TextKind::Attribute {
            values.push(value.to_owned());
        }
        Ok::<_, std::convert::Infallible>(())
    })
    .expect("native decoding of representative rendered attributes");
    values
}

fn json_ld(html: &str) -> Value {
    let body = html
        .split_once("<script type=\"application/ld+json\">")
        .expect("direct native linked-data projection")
        .1;
    serde_json::from_str(body.split_once("</script>").expect("linked-data end").0)
        .expect("whole native linked-data value")
}

fn ids(items: &Value, field: &str) -> Vec<String> {
    items
        .as_array()
        .expect("native ordered reference array")
        .iter()
        .map(|item| {
            item[field]
                .as_str()
                .expect("native reference identity")
                .to_owned()
        })
        .collect()
}

/// Cross-check actual neutral Theme contexts against independently emitted graph/search.
fn editorial_outputs(files: &Files, compilation: &workspace::Compilation) {
    let mut keywords = 0;
    let mut journeys = 0;
    let mut notes = 0;
    let mut continuations = 0;
    for site in SITES {
        let site_key = site.parse().expect("site identity");
        let declared_site = compilation
            .workspace
            .site(&site_key)
            .expect("configured site");
        let graph = script(text(files, site, "graph/index.html"), "acceptance-graph");
        let relations = script(
            text(files, site, "graph/index.html"),
            "acceptance-relations",
        );
        let search: Vec<Value> = serde_json::from_slice(bytes(files, site, "search.json"))
            .expect("actual search records");
        let search_ids: BTreeSet<_> = search
            .iter()
            .map(|record| record["content"]["id"].as_str().unwrap())
            .collect();
        assert_eq!(
            search_ids.len(),
            search.len(),
            "notes and continuations do not create duplicate search identities"
        );
        assert_eq!(
            search_ids,
            graph
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect()
        );
        let mut primary = BTreeMap::new();
        for (path, body) in &files[site] {
            if !path.ends_with(".html") {
                continue;
            }
            let html = std::str::from_utf8(body).expect("actual HTML");
            if !html.contains("<article data-content=") {
                continue;
            }
            let context = script(html, "acceptance-editorial");
            let content = &context["content"];
            let id = content["id"].as_str().expect("actual page identity");
            let record = search
                .iter()
                .find(|record| record["content"]["id"] == id)
                .expect("same discovery identity");
            let linked = json_ld(html);
            if let Some(listing) = context.get("listing") {
                let projected = relations["listings"]
                    .as_array()
                    .expect("configured listing projections")
                    .iter()
                    .find(|view| view["output"].as_str() == Some(path.as_str()))
                    .expect("same graph listing output");
                assert_eq!(
                    listing, projected,
                    "Theme/graph complete listing parity at {site}/{path}"
                );
                assert_eq!(
                    linked["mainEntity"]["numberOfItems"],
                    listing["items"].as_array().unwrap().len()
                );
                let linked_items = Value::Array(
                    linked["mainEntity"]["itemListElement"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|entry| entry["item"].clone())
                        .collect(),
                );
                assert_eq!(
                    ids(&linked_items, "identifier"),
                    ids(&listing["items"], "id")
                );
            }
            for field in [
                "id",
                "site",
                "title",
                "description",
                "keywords",
                "schema_type",
                "lifecycle",
                "language",
                "attributions",
                "taxonomies",
            ] {
                assert_eq!(
                    content.get(field),
                    graph[id].get(field),
                    "Theme/graph {field} parity at {site}/{path}"
                );
                assert_eq!(
                    content.get(field),
                    record["content"].get(field),
                    "Theme/search {field} parity at {site}/{path}"
                );
            }
            assert_eq!(context["site"], record["site"]);
            assert_eq!(context["site"]["publisher"], json!(declared_site.publisher));
            assert_eq!(
                context["site"]["rights_holder"],
                json!(declared_site.rights_holder)
            );
            assert_eq!(
                context["site"]["default_author"],
                json!(declared_site.default_author)
            );
            for (actual, declared) in context["site"]["roles"]
                .as_array()
                .unwrap()
                .iter()
                .zip(&declared_site.roles)
            {
                assert_eq!(actual["principal"]["id"], json!(declared.principal));
                assert_eq!(actual["role"], json!(declared.role));
            }
            assert_eq!(
                context["site"]["roles"].as_array().unwrap().len(),
                declared_site.roles.len()
            );
            assert_eq!(context["attributions"], record["attributions"]);
            let authored = content["attributions"]
                .as_array()
                .map_or(&[][..], Vec::as_slice);
            let resolved = context["attributions"].as_array().unwrap();
            assert_eq!(authored.len(), resolved.len());
            for (declared, actual) in authored.iter().zip(resolved) {
                assert_eq!(declared["principal"], actual["principal"]["id"]);
                assert_eq!(declared["role"], actual["role"]);
            }
            for (name, role) in [("author", "author"), ("copyrightHolder", "rights-holder")] {
                let expected: Vec<_> = authored
                    .iter()
                    .filter(|row| row["role"] == role)
                    .map(|row| row["principal"].as_str().unwrap().to_owned())
                    .collect();
                let actual = linked
                    .get(name)
                    .map(|rows| ids(rows, "identifier"))
                    .unwrap_or_default();
                assert_eq!(actual, expected, "site roles do not invent page {name}");
            }
            if content.get("keywords").is_some() {
                keywords += 1;
                assert_eq!(linked["keywords"], content["keywords"]);
            }
            if let Some(subtype) = content.get("schema_type") {
                assert_eq!(&linked["@type"], subtype);
            }
            if context.get("notes").is_some() {
                notes += 1;
                let commentary = &context["notes"];
                assert_eq!(commentary, &record["notes"]);
                for prohibited in ["id", "content", "route", "canonical_url"] {
                    assert!(
                        commentary.get(prohibited).is_none(),
                        "notes have no independent {prohibited}"
                    );
                }
                assert!(
                    commentary["headings"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .all(|heading| heading["level"].as_u64().unwrap() > 1)
                );
            }
            if context["listing"]["page_number"].as_u64().unwrap_or(1) > 1 {
                continuations += 1;
            } else {
                assert!(
                    primary.insert(id.to_owned(), context).is_none(),
                    "one primary rendered identity"
                );
            }
        }
        let mut parents = BTreeMap::new();
        for edge in relations["containers"].as_array().unwrap() {
            assert!(
                parents
                    .insert(
                        edge["child"].as_str().unwrap().to_owned(),
                        edge["parent"].as_str().unwrap().to_owned()
                    )
                    .is_none()
            );
        }
        for edge in relations["memberships"].as_array().unwrap() {
            assert!(
                parents
                    .insert(
                        edge["content"].as_str().unwrap().to_owned(),
                        edge["collection"].as_str().unwrap().to_owned()
                    )
                    .is_none(),
                "direct collection residence is unique"
            );
        }
        for edge in relations["journeys"].as_array().unwrap() {
            assert_eq!(edge["primary"], true);
            assert!(
                parents
                    .insert(
                        edge["content"].as_str().unwrap().to_owned(),
                        edge["journey"].as_str().unwrap().to_owned()
                    )
                    .is_none(),
                "journey residence never overlaps a direct collection"
            );
        }
        let publication = compilation
            .workspace
            .publication_for(&site_key)
            .expect("configured publication");
        for declaration in &publication.listings {
            let mut pages: Vec<_> = relations["listings"]
                .as_array()
                .expect("configured reading views")
                .iter()
                .filter(|page| page["content"] == declaration.content.as_str())
                .collect();
            pages.sort_by_key(|page| page["page_number"].as_u64().unwrap());
            assert!(
                !pages.is_empty(),
                "selected existing index has a generated reading view"
            );
            let mut references = Vec::new();
            for (index, page) in pages.iter().enumerate() {
                assert_eq!(page["page_number"], index + 1);
                assert_eq!(page["page_count"], pages.len());
                assert_eq!(page["total_items"], pages[0]["total_items"]);
                assert!(
                    page["items"].as_array().unwrap().len() <= declaration.pagination.page_size
                );
                assert_eq!(
                    page["previous"].as_str(),
                    index
                        .checked_sub(1)
                        .map(|previous| pages[previous]["url"].as_str().unwrap())
                );
                assert_eq!(
                    page["next"].as_str(),
                    pages
                        .get(index + 1)
                        .map(|next| next["url"].as_str().unwrap())
                );
                references.extend(ids(&page["items"], "id"));
            }
            assert_eq!(
                references.len(),
                pages[0]["total_items"].as_u64().unwrap() as usize
            );
            assert_eq!(
                references.iter().collect::<BTreeSet<_>>().len(),
                references.len(),
                "pagination does not duplicate existing identities"
            );
            let order = json!(declaration.order);
            if let Some(first) = order["first"].as_array() {
                let priority: Vec<_> = first
                    .iter()
                    .map(|id| id.as_str().unwrap().to_owned())
                    .collect();
                assert!(
                    references.starts_with(&priority),
                    "explicit owner listing priority is not alphabetical reconstruction"
                );
            }
        }
        for (id, context) in &primary {
            let mut ancestry = vec![id.clone()];
            while let Some(parent) = parents.get(ancestry.last().unwrap()) {
                assert!(!ancestry.contains(parent), "native containment is acyclic");
                ancestry.push(parent.clone());
            }
            ancestry.reverse();
            let trails = context["breadcrumbs"].as_array().unwrap();
            assert_eq!(trails.len(), 1, "one editorial residence at {id}");
            let actual: Vec<_> = trails[0]["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|item| item["node"]["id"].as_str().unwrap().to_owned())
                .collect();
            assert_eq!(actual, ancestry, "root-first declared ancestry at {id}");
            if context["content"]["structure"]["kind"] == "journey" {
                journeys += 1;
                let members = ids(&context["content"]["structure"]["members"], "content");
                assert_eq!(
                    ids(&context["steps"], "id"),
                    members,
                    "authored journey order reaches Theme"
                );
                for (position, member) in members.iter().enumerate() {
                    let membership = &primary[member]["journeys"][0];
                    assert_eq!(membership["journey"]["id"], id.as_str());
                    assert_eq!(membership["position"], position);
                    assert_eq!(membership["total"], members.len());
                    assert_eq!(
                        membership["previous"]["node"]["id"].as_str(),
                        position.checked_sub(1).map(|index| members[index].as_str())
                    );
                    assert_eq!(
                        membership["next"]["node"]["id"].as_str(),
                        members.get(position + 1).map(String::as_str)
                    );
                }
            }
            if context["content"]["structure"]["kind"] == "collection" {
                let mut expected: Vec<_> = parents
                    .iter()
                    .filter(|(_, parent)| *parent == id)
                    .map(|(child, _)| child.clone())
                    .collect();
                expected.sort_unstable();
                assert_eq!(
                    ids(&context["children"], "id"),
                    expected,
                    "direct children follow explicit containment only"
                );
            }
        }
    }
    assert!(
        keywords > 0 && journeys > 0 && notes > 0,
        "real corpus must exercise authored metadata, journeys and notes"
    );
    println!(
        "actual editorial outputs: {keywords} keyword-bearing contexts, {journeys} ordered journeys, {notes} article notes, {continuations} listing continuation outputs; counts are observed, not fixed acceptance inputs"
    );
}

/// Compare a generated context section through the existing native Markdown reader.
fn markdown_context_block(markdown: &str, marker: &str, expected: &[&str]) {
    let (_, tail) = markdown
        .rsplit_once(marker)
        .unwrap_or_else(|| panic!("missing resolved Markdown reading context {marker:?}"));
    let end = [
        "\n\nReading path: ",
        "\nPrevious in journey: ",
        "\nNext in journey: ",
        "\n\n## Collection contents\n",
        "\n\n## Journey sequence\n",
    ]
    .into_iter()
    .filter_map(|next| tail.find(next))
    .min()
    .unwrap_or(tail.len());
    let parsed = gazette_sdk::markdown::parse(&tail[..end], Default::default())
        .expect("native reading-context Markdown parser");
    let inspected = parsed.inspect().expect("native reading-context links");
    assert_eq!(
        inspected
            .links
            .iter()
            .filter(|link| link.emitted)
            .map(|link| link.destination.as_str())
            .collect::<Vec<_>>(),
        expected,
        "resolved serving addresses and declared order at {marker:?}"
    );
}

/// Check every emitted Markdown link against the complete selected output set.
/// External destinations are not fetched; fragments remain native source-link evidence.
fn markdown_outputs(files: &Files, compilation: &workspace::Compilation, local: bool) {
    let sites: Vec<_> = SITES.iter().map(|site| site.parse().unwrap()).collect();
    let local_urls = compilation
        .workspace
        .local_urls(&sites)
        .expect("configured local bases");
    let mut checked = 0;
    for site in SITES {
        let site_key = site.parse().unwrap();
        let base = if local {
            local_urls.get(&site_key)
        } else {
            compilation.workspace.public_base_url(&site_key)
        }
        .expect("selected serving base");
        for (path, body) in &files[site] {
            if !path.ends_with(".md") && path != "llms.txt" {
                continue;
            }
            let markdown = std::str::from_utf8(body).expect("actual emitted Markdown");
            let inspection = gazette_sdk::markdown::parse(markdown, Default::default())
                .expect("native Markdown parser")
                .inspect()
                .expect("native emitted-link inspection");
            for link in inspection.links.iter().filter(|link| link.emitted) {
                let without_fragment = link.destination.split(['?', '#']).next().unwrap();
                let absolute = without_fragment.starts_with("http://")
                    || without_fragment.starts_with("https://");
                let selected = if absolute {
                    sites.iter().find_map(|candidate| {
                        let candidate_base = if local {
                            local_urls.get(candidate)
                        } else {
                            compilation.workspace.public_base_url(candidate)
                        }
                        .unwrap();
                        without_fragment
                            .starts_with(candidate_base.as_str())
                            .then_some((candidate, candidate_base))
                    })
                } else if without_fragment.contains(':') || without_fragment.starts_with("//") {
                    None
                } else {
                    Some((&site_key, base))
                };
                let Some((target_site, target_base)) = selected else {
                    continue;
                };
                let origin = format!("/{path}")
                    .parse()
                    .expect("actual emitted file route");
                let reference = if without_fragment.is_empty() {
                    target_base.resolve_output(&origin)
                } else if without_fragment.ends_with('/') {
                    format!("{without_fragment}index.html")
                } else {
                    without_fragment.to_owned()
                };
                let route = target_base
                    .resource_target(&origin, &reference)
                    .expect("selected destination stays inside serving namespace");
                assert!(
                    files[target_site.as_str()].contains_key(route.output().as_str()),
                    "{site}/{path}: emitted Markdown target {} is absent from selected output",
                    link.destination
                );
                checked += 1;
            }
        }
        let publication = compilation.workspace.publication_for(&site_key).unwrap();
        let configuration = publication
            .machine
            .llms
            .as_ref()
            .expect("explicit curation");
        let llms = gazette_sdk::markdown::parse(text(files, site, "llms.txt"), Default::default())
            .unwrap()
            .inspect()
            .unwrap();
        let contexts: BTreeMap<_, _> = files[site]
            .iter()
            .filter_map(|(_, body)| {
                let html = std::str::from_utf8(body).ok()?;
                if !html.contains("<article data-content=") {
                    return None;
                }
                let context = script(html, "acceptance-editorial");
                if context["listing"]["page_number"].as_u64().unwrap_or(1) > 1 {
                    return None;
                }
                Some((
                    context["content"]["id"].as_str().unwrap().to_owned(),
                    context,
                ))
            })
            .collect();
        let routes: BTreeMap<_, _> = files[site]
            .values()
            .filter_map(|body| {
                let html = std::str::from_utf8(body).ok()?;
                if !html.contains("<article data-content=") {
                    return None;
                }
                let context = script(html, "acceptance-editorial");
                if context["listing"]["page_number"].as_u64().unwrap_or(1) > 1 {
                    return None;
                }
                let route = script(html, "acceptance-route");
                Some((
                    context["content"]["id"].as_str().unwrap().to_owned(),
                    route["url"].as_str().unwrap().to_owned(),
                ))
            })
            .collect();
        let graph = script(text(files, site, "graph/index.html"), "acceptance-graph");
        let eligible = |node: &Value| {
            let id = node["id"].as_str().expect("native endpoint identity");
            if graph.get(id).is_none() {
                return false;
            }
            // Automatic child endpoints are shallow DiscoveryNodes. Join their
            // identity to the owning rendered context, not an invented member list.
            let owning = contexts.get(id).expect("selected endpoint owning context");
            assert_eq!(
                node["structure"]["kind"], owning["content"]["structure"]["kind"],
                "shallow endpoint retains its owning native kind"
            );
            if node["structure"]["kind"] != "journey" {
                return true;
            }
            let members = &owning["content"]["structure"]["members"];
            assert_eq!(
                ids(&owning["steps"], "id"),
                ids(members, "content"),
                "owning journey sequence retains every declared member in authored order"
            );
            members
                .as_array()
                .expect("owning declared ordered journey")
                .iter()
                .all(|member| graph.get(member["content"].as_str().unwrap()).is_some())
        };
        let address = |id: &str| routes[id].as_str();
        for context in contexts.values() {
            let Some(alternate) = context["presentation"]["markdown_alternate"]["url"].as_str()
            else {
                assert!(
                    context["content"]["route"]["canonical"].as_str().is_some(),
                    "only an explicit syndicated copy omits its own Markdown counterpart"
                );
                continue;
            };
            let route = base
                .resource_target(&"/".parse().unwrap(), alternate)
                .expect("native alternate coordinate");
            let markdown = text(files, site, route.output().as_str());
            for trail in context["breadcrumbs"].as_array().unwrap() {
                let items = trail["items"].as_array().unwrap();
                if items.len() > 1 {
                    let addresses: Vec<_> = items
                        .iter()
                        .map(|item| address(item["node"]["id"].as_str().unwrap()))
                        .collect();
                    markdown_context_block(markdown, "\n\nReading path: ", &addresses);
                }
            }
            for membership in context["journeys"].as_array().unwrap() {
                for (field, marker) in [
                    ("previous", "\nPrevious in journey: "),
                    ("next", "\nNext in journey: "),
                ] {
                    if let Some(id) = membership[field]["node"]["id"].as_str() {
                        markdown_context_block(markdown, marker, &[address(id)]);
                    }
                }
            }
            let (marker, nodes) = match context["content"]["structure"]["kind"].as_str() {
                Some("collection") => (
                    "\n\n## Collection contents\n",
                    context["children"].as_array().unwrap(),
                ),
                Some("journey") if eligible(&context["content"]) => (
                    "\n\n## Journey sequence\n",
                    context["steps"].as_array().unwrap(),
                ),
                _ => continue,
            };
            let addresses: Vec<_> = nodes
                .iter()
                .filter(|node| eligible(node))
                .map(|node| address(node["id"].as_str().unwrap()))
                .collect();
            if !addresses.is_empty() {
                markdown_context_block(markdown, marker, &addresses);
            }
        }
        let expected: Vec<_> = configuration
            .sections
            .iter()
            .flat_map(|section| section.contents.iter())
            .flat_map(|id| {
                let context = &contexts[id.as_str()];
                let mut urls = vec![
                    context["presentation"]["markdown_alternate"]["url"]
                        .as_str()
                        .unwrap()
                        .to_owned(),
                ];
                if configuration.include_human_link {
                    urls.push(
                        context["presentation"]["markdown_alternate"]["html_url"]
                            .as_str()
                            .unwrap()
                            .to_owned(),
                    );
                }
                urls
            })
            .collect();
        assert_eq!(
            llms.links
                .iter()
                .filter(|link| link.emitted)
                .map(|link| link.destination.clone())
                .collect::<Vec<_>>(),
            expected,
            "curation order and intentional repetition, not an exhaustive sitemap"
        );
        let description =
            contexts[configuration.description_from.as_str()]["content"]["description"]
                .as_str()
                .expect("explicit selected description");
        assert!(
            llms.text.contains(description),
            "configured description reaches curated entrypoint"
        );
    }
    assert!(checked > 0, "actual ecosystem Markdown links were checked");
    println!(
        "actual {} Markdown target closure: {checked} selected ecosystem links; external URLs not fetched and fragment validity remains native source-link evidence",
        if local { "local" } else { "public" }
    );
}

fn complete(report: &Value, supplied: &str) {
    let outcomes = report["sites"].as_object().expect("site outcomes");
    assert_eq!(outcomes.len(), SITES.len());
    let failures = outcomes
        .iter()
        .filter(|(_, outcome)| outcome["status"] == "failed")
        .map(|(site, outcome)| {
            let message = outcome["message"].as_str().expect("native failure message");
            json!({
                "site":site,
                "code":outcome["code"],
                "message":message.chars().take(2048).collect::<String>(),
                "message_truncated":message.chars().count() > 2048,
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        report["status"],
        "local_set_rendered",
        "failed site diagnostics only: {}",
        json!(failures)
    );
    assert_eq!(report["build_selection"]["origin"], "invocation");
    assert_eq!(report["build_selection"]["sites"], json!(SITES));
    assert_eq!(report["build_selection"]["local_drafts"], "include");
    assert_eq!(
        report["configuration"],
        json!({"kind":"supplied","source_sha256":source::Sha256Digest::of(supplied.as_bytes())})
    );
    assert!(
        report["source_failures"]
            .as_object()
            .expect("exact source failures")
            .is_empty()
    );
    assert!(
        !report["math"]["invocations"]
            .as_array()
            .expect("real pinned math generation")
            .is_empty()
    );
}

fn native_cli(recipe: &Value) -> Value {
    let mut child = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args(["render-sites", "--local", "--max-source-bytes", "1048576"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("native Gazette CLI");
    child
        .stdin
        .take()
        .expect("CLI stdin")
        .write_all(recipe.to_string().as_bytes())
        .expect("exact supplied recipe");
    let output = child.wait_with_output().expect("native CLI completion");
    assert!(
        output.status.success(),
        "CLI failed: {}\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("complete native CLI report")
}

fn provider_deadline(recipe: &Value) -> Duration {
    let math = &recipe["math_generation"];
    assert_eq!(
        math["allow_execute"], true,
        "this proof preserves explicit provider execution"
    );
    let milliseconds = math["timeout_ms"]
        .as_u64()
        .expect("explicit provider deadline");
    let formulas = math["formulas"].as_u64().expect("explicit formula ceiling");
    assert!((1..=4096).contains(&formulas));
    assert!((1..=30_000).contains(&milliseconds));
    // Budget normal 256-formula batches from the recipe's bounded selection plus
    // one minute for local capture/render. Byte-limited splits can exhaust this
    // independent harness deadline; that is a reported refusal, not a skip or retry.
    Duration::from_millis(milliseconds * formulas.div_ceil(256)) + Duration::from_secs(60)
}

#[test]
#[ignore = "manual owner-corpus qualification; needs GAZETTE_CORPUS_WORKSPACE and GAZETTE_CORPUS_RECIPE and the configured pinned math provider"]
fn six_owner_publications_render_over_sdk_cli_initialized_mcp_and_materialize_neutral_outputs() {
    let workspace_root = selected_path("GAZETTE_CORPUS_WORKSPACE");
    let recipe_path = selected_path("GAZETTE_CORPUS_RECIPE");
    let original_recipe = fs::read(&recipe_path).expect("explicit owner recipe");
    let parsed_recipe = build::LocalPublicationRecipe::parse_json(&original_recipe, INPUT_BYTES, 6)
        .expect("original strict owner recipe");
    let workspace_path = match parsed_recipe.workspace() {
        build::WorkspaceInput::Local { workspace, .. } => workspace.clone(),
        build::WorkspaceInput::Supplied { .. } => {
            panic!("select the original native workspace recipe")
        }
    };
    assert_eq!(
        parsed_recipe.addressing(),
        build::PublicationAddressing::Public
    );
    let mut recipe: Value = serde_json::from_slice(&original_recipe).expect("original recipe JSON");
    assert_eq!(
        recipe["sites"]
            .as_array()
            .expect("explicit owner sites")
            .iter()
            .map(|s| s["site"].as_str().expect("site"))
            .collect::<Vec<_>>(),
        SITES
    );

    let loaded =
        configuration::load_workspace(&workspace_root, &workspace_path, Default::default())
            .expect("native original include-set admission, without Theme/source effects");
    let original_inventory = loaded.inventory().identity();
    let original_documents = json!(loaded.documents().documents);
    let original = json!(loaded.documents().compilation.workspace);
    let root_text = std::str::from_utf8(loaded.source(&workspace_path).expect("root capture"))
        .expect("root UTF-8");
    let includes = workspace::included_documents(root_text, syntax())
        .expect("native nonrecursive include selection");
    let documents = std::iter::once(&workspace_path)
        .chain(&includes)
        .map(|path| {
            (
                path.clone(),
                std::str::from_utf8(loaded.source(path).expect("exact selected document"))
                    .expect("UTF-8 configuration")
                    .to_owned(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let original_resolved = loaded.into_resolved();

    let temporary = tempfile::Builder::new()
        .prefix("gazette-corpus-acceptance-")
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .expect("one small private disposable acceptance tree");
    let neutral = temporary.path().join("neutral");
    fs::create_dir_all(neutral.join("templates")).expect("owned neutral package");
    for (path, body) in [
        ("gazette-theme.toml", MANIFEST),
        ("templates/page.html", PAGE),
        ("templates/alias.html", ALIAS),
        ("templates/graph.html", GRAPH),
    ] {
        fs::write(neutral.join(path), body).expect("explicit neutral bytes only");
    }
    let outputs = SITES
        .iter()
        .map(|site| {
            let directory = temporary
                .path()
                .join("outputs")
                .join(site.strip_prefix("site:").expect("site key"));
            fs::create_dir_all(&directory).expect("existing temporary output");
            git2::Repository::init(&directory).expect("native Git administration to preserve");
            fs::write(
                directory.join("owner.txt"),
                b"unrelated temporary owner bytes",
            )
            .expect("owner sentinel");
            ((*site).to_owned(), directory)
        })
        .collect::<BTreeMap<_, _>>();
    let administration = outputs
        .iter()
        .map(|(site, directory)| {
            (
                site.clone(),
                source::inspect(&directory.join(".git"), None, Default::default())
                    .expect("original Git admin")
                    .identity(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let (supplied, authoring_configuration) =
        supplied_configuration(&documents, &workspace_path, &includes, &outputs, &neutral);
    recipe["workspace"] = json!({"kind":"supplied","source":supplied});
    let mut restored = recipe.clone();
    restored["workspace"] = serde_json::from_slice::<Value>(&original_recipe)
        .expect("original JSON")["workspace"]
        .clone();
    assert_eq!(
        restored,
        serde_json::from_slice::<Value>(&original_recipe).expect("unmodified recipe semantics")
    );

    let request =
        build::LocalPublicationRecipe::parse_json(recipe.to_string().as_bytes(), INPUT_BYTES, 6)
            .expect("strict neutral recipe");
    let resolved = request
        .resolve(syntax())
        .expect("exact neutral configuration and intent");
    let sites = resolved.sites().cloned().collect::<Vec<_>>();
    let mut inputs = build::acquire_publications(&supplied, &sites, syntax(), Default::default())
        .expect("one retained six-site acquisition");
    assert_eq!(inputs.build_selection(), resolved.build_selection());
    for (site, selected_source, entries) in RETENTION_BINDINGS {
        let content_source: workspace::Key = selected_source.parse().unwrap();
        let original_capture = &inputs.sources()[&content_source];
        let binding = inputs
            .compilation()
            .workspace
            .publication_for(&site.parse().unwrap())
            .unwrap()
            .authored
            .iter()
            .find(|binding| binding.source == content_source)
            .expect("exact selected authored binding");
        assert_eq!(
            binding
                .retained_only
                .iter()
                .map(source::EntryPath::as_str)
                .collect::<Vec<_>>(),
            entries
        );
        for entry in entries {
            let entry: source::EntryPath = entry.parse().unwrap();
            let retained_bytes = original_capture
                .file(&entry)
                .expect("exact opaque metadata bytes remain captured");
            let record = original_capture
                .inventory()
                .entries()
                .iter()
                .find(|record| record.path == entry)
                .expect("complete source inventory remains intact");
            assert_eq!(
                record.content,
                source::Content::File {
                    bytes: retained_bytes.len() as u64,
                    sha256: source::Sha256Digest::of(retained_bytes)
                },
                "retained opaque bytes preserve exact source identity"
            );
        }
    }
    let limits = build::PublicationSetRenderLimits::default();
    request
        .acquire_math(&mut inputs, limits.publication.corpus)
        .expect("one pinned math acquisition for both address projections");
    request
        .acquire_diagrams(&mut inputs, limits.publication.corpus)
        .expect("one selected diagram acquisition for both address projections");
    retained_authoring::prove(&inputs, &workspace_path, &authoring_configuration);
    let rendered =
        build::render_publications(&inputs, resolved.options(Default::default()), limits)
            .expect("public render from retained owner inputs");
    let (expected, files) = {
        let report = json!(rendered.inspect());
        complete(&report, &supplied);
        let files = output_files(&rendered);
        for emitted in files.values() {
            assert!(
                emitted
                    .keys()
                    .all(|path| !path.split('/').any(|part| part == ".DS_Store")),
                "explicit retained-only metadata must not leak into generated output"
            );
        }
        let plan = build::plan_outputs(&rendered, &BTreeMap::new(), Default::default())
            .expect("complete configured destination preflight");
        assert_eq!(plan.sites().len(), 6);
        let receipts = build::materialize_outputs(plan, true)
            .expect("actual local writes and complete readback");
        for (site, receipt) in &receipts {
            let tree = rendered.sites()[site]
                .as_ref()
                .expect("rendered site")
                .site()
                .output();
            assert_eq!(receipt.generated(), tree.inventory());
            for (path, body) in &files[site.as_str()] {
                assert_eq!(
                    fs::read(receipt.root().join(path)).expect("actual materialized file"),
                    *body
                );
            }
            assert_eq!(
                fs::read(receipt.root().join("owner.txt")).expect("owner sentinel survives"),
                b"unrelated temporary owner bytes"
            );
            assert_eq!(
                source::inspect(&receipt.root().join(".git"), None, Default::default())
                    .expect("preserved Git administration")
                    .identity(),
                administration[site.as_str()]
            );
        }
        let repeated = build::plan_outputs(&rendered, &receipts, Default::default())
            .expect("exact receipt-owned repeated plan");
        assert!(
            repeated
                .sites()
                .values()
                .all(|plan| plan.changes().is_empty())
        );
        let repeated =
            build::materialize_outputs(repeated, true).expect("complete no-op native readback");
        for (site, receipt) in repeated {
            assert_eq!(receipt.inventory(), receipts[&site].inventory());
        }
        (report, files)
    };

    // The two projections share one immutable acquired input and provider result
    // set. This is retained replay, not an atomic claim about upstream observations.
    let local_urls = inputs
        .compilation()
        .workspace
        .local_urls(&sites)
        .expect("all six configured loopback ports and unchanged public prefixes");
    let mut local_options = resolved.options(Default::default());
    local_options.preview_urls = Some(local_urls.clone());
    let local = build::render_publications(&inputs, local_options, limits)
        .expect("local address projection without acquisition or provider execution");
    let local_report = json!(local.inspect());
    complete(&local_report, &supplied);
    assert_eq!(local_report["preview_urls"], json!(local_urls));
    assert!(expected["preview_urls"].is_null());
    for field in [
        "build_selection",
        "configuration",
        "compilation",
        "sources",
        "source_failures",
        "math",
        "diagrams",
        "selection",
        "candidate_checks",
    ] {
        assert_eq!(
            local_report[field], expected[field],
            "retained {field} stays unchanged"
        );
    }
    let local_files = output_files(&local);
    editorial_outputs(&files, inputs.compilation());
    editorial_outputs(&local_files, inputs.compilation());
    markdown_outputs(&files, inputs.compilation(), false);
    markdown_outputs(&local_files, inputs.compilation(), true);
    let mut internal_projected = 0;
    let mut cross_site_projected = 0;
    for site in SITES {
        let key = site.parse().expect("site identity");
        let base = inputs
            .compilation()
            .workspace
            .public_base_url(&key)
            .expect("configured public base");
        let local_base = local_urls[&key].as_str();
        assert_eq!(
            files[site].keys().collect::<Vec<_>>(),
            local_files[site].keys().collect::<Vec<_>>(),
            "addressing does not change selected output paths"
        );
        assert_eq!(
            script(text(&files, site, "graph/index.html"), "acceptance-graph"),
            script(
                text(&local_files, site, "graph/index.html"),
                "acceptance-graph"
            ),
            "all selected graph content identities and relationships survive projection"
        );
        assert!(
            text(&local_files, site, "index.md").contains(&format!("[Read HTML]({local_base})"))
        );
        assert!(text(&local_files, site, "llms.txt").contains(&format!("{local_base}index.md")));
        for (path, public_bytes) in &files[site] {
            if !path.ends_with(".html") {
                continue;
            }
            let public_html = std::str::from_utf8(public_bytes).expect("public HTML");
            if !public_html.contains("<article data-content=") {
                continue;
            }
            let local_html = text(&local_files, site, path);
            let public_route = script(public_html, "acceptance-route");
            let local_route = script(local_html, "acceptance-route");
            let public_url = public_route["url"].as_str().expect("public serving URL");
            let suffix = public_url
                .strip_prefix(base.as_str())
                .expect("page under configured base");
            assert_eq!(local_route["url"], format!("{local_base}{suffix}"));
            assert_eq!(local_route["output"], public_route["output"]);
            let public_links = script(public_html, "acceptance-links");
            let local_links = script(local_html, "acceptance-links");
            let public_links = public_links
                .as_array()
                .expect("retained public link annotations");
            let local_links = local_links
                .as_array()
                .expect("retained local link annotations");
            assert_eq!(local_links.len(), public_links.len());
            for (public_link, local_link) in public_links.iter().zip(local_links) {
                let mut public_identity = public_link.clone();
                let mut local_identity = local_link.clone();
                let public_destination = public_identity
                    .as_object_mut()
                    .expect("link annotation")
                    .remove("destination")
                    .expect("public destination");
                let local_destination = local_identity
                    .as_object_mut()
                    .expect("link annotation")
                    .remove("destination")
                    .expect("local destination");
                assert_eq!(
                    local_identity, public_identity,
                    "link identity, target, relationship and presentation stay unchanged at {site}/{path}"
                );
                if local_destination != public_destination {
                    let destination_site =
                        public_link["site"].as_str().expect("projected target site");
                    let destination_key = destination_site.parse().expect("target identity");
                    let target_base = inputs
                        .compilation()
                        .workspace
                        .public_base_url(&destination_key)
                        .expect("target public base");
                    let destination = public_destination.as_str().expect("public destination");
                    let suffix = destination
                        .strip_prefix(target_base.as_str())
                        .expect("projected destination under its exact configured site");
                    assert_eq!(
                        local_destination,
                        format!("{}{suffix}", local_urls[&destination_key].as_str())
                    );
                    if destination_site == site {
                        internal_projected += 1;
                    } else {
                        cross_site_projected += 1;
                    }
                }
            }
        }
        let public_pages = expected["sites"][site]["evidence"]["pages"]
            .as_object()
            .expect("public page evidence");
        let local_pages = local_report["sites"][site]["evidence"]["pages"]
            .as_object()
            .expect("local page evidence");
        assert_eq!(
            public_pages.keys().collect::<Vec<_>>(),
            local_pages.keys().collect::<Vec<_>>()
        );
        for (page, evidence) in public_pages {
            assert_eq!(
                local_pages[page]["source"], evidence["source"],
                "exact original source identity remains retained"
            );
            assert_eq!(local_pages[page]["template"], evidence["template"]);
        }
    }
    assert!(
        internal_projected > 0,
        "actual internal symbolic references project locally"
    );
    assert!(
        cross_site_projected > 0,
        "actual cross-publication symbolic references project locally"
    );
    for path in [
        "collections/useful-compute/index.html",
        "collections/useful-compute/a-capacity-promise-needs-a-clock/index.html",
    ] {
        for projection in [&files, &local_files] {
            let html = text(projection, "site:rawthoughts", path);
            let attributes = decoded_attributes(html);
            let links = script(html, "acceptance-links");
            for link in links
                .as_array()
                .expect("real retained reference annotations")
            {
                let destination = link["destination"]
                    .as_str()
                    .expect("emitted link destination");
                assert!(
                    attributes.iter().any(|attribute| attribute == destination),
                    "real rendered anchor has the exact decoded {destination}, not just a matching model"
                );
            }
            let route = script(html, "acceptance-route");
            let url = route["url"].as_str().expect("serving route");
            let alternate = format!("{url}index.md");
            assert!(
                attributes.iter().any(|attribute| attribute == &alternate),
                "Theme receives the projected Markdown alternate"
            );
        }
    }

    for site in SITES {
        let publication = original_resolved
            .compilation()
            .workspace
            .publication_for(&site.parse().expect("site"))
            .expect("original publication");
        assert!(text(&files, site, "index.html").contains("acceptance-neutral"));
        assert!(text(&files, site, "index.md").len() > 20);
        assert!(text(&files, site, "graph/index.html").contains("acceptance-graph"));
        assert!(
            !serde_json::from_slice::<Value>(bytes(&files, site, "site.jsonld"))
                .expect("shared JSON-LD")
                .is_null()
        );
        assert!(
            !serde_json::from_slice::<Value>(bytes(&files, site, "search.json"))
                .expect("shared search")
                .as_array()
                .expect("search records")
                .is_empty()
        );
        let llms = text(&files, site, "llms.txt");
        assert!(llms.contains("[HTML]("));
        assert!(llms.contains("index.md"));
        let mut position = 0;
        for section in &publication
            .machine
            .llms
            .as_ref()
            .expect("configured curation")
            .sections
        {
            let marker = format!("## {}", section.title);
            position += llms[position..]
                .find(&marker)
                .expect("configured section order")
                + marker.len();
        }
        for binding in &publication.legal {
            let html = text(&files, site, binding.route.output().as_str());
            assert!(html.contains(binding.id.as_str()));
            assert_eq!(
                script(html, "acceptance-processing"),
                json!({"hosting":{"provider":"github-pages"},"analytics":false,"subscription":false,"discussion":{"enabled":false},"webmcp":false})
            );
        }
    }
    let notes = "collections/useful-compute/a-capacity-promise-needs-a-clock/index.html";
    let article = text(&files, "site:rawthoughts", notes);
    assert_eq!(
        article.matches("<h1").count(),
        1,
        "authored H1 is not duplicated"
    );
    let main = article
        .split_once("<main>")
        .expect("authored body region")
        .1
        .split_once("</main>")
        .expect("authored body region end")
        .0;
    assert_eq!(main.matches("<h1").count(), 1, "the H1 remains body-owned");
    let commentary = article
        .split_once("<aside id=\"acceptance-notes\">")
        .expect("separate article-owned commentary")
        .1
        .split_once("</aside>")
        .expect("commentary region end")
        .0;
    assert!(commentary.contains("A seven-day robotics experiment"));
    assert_eq!(
        commentary.matches("<h1").count(),
        0,
        "notes stay subordinate"
    );
    assert!(commentary.contains("<h2"), "native notes heading survives");
    let listing = text(&files, "site:rawthoughts", "blog/index.html");
    assert_eq!(
        listing.matches("<h1").count(),
        1,
        "bodyless listing gets one title"
    );
    assert!(listing.contains("<h1>Articles</h1>"));
    assert!(
        text(
            &files,
            "site:causality",
            "collections/changing-descriptions/from-summary-to-repair/budgeted-questions/index.html"
        )
        .contains("<math")
    );
    let rawthoughts = original_resolved
        .compilation()
        .workspace
        .source(&"source:rawthoughts-content".parse().expect("source"))
        .expect("actual owner asset source");
    let asset_root = match &rawthoughts.location {
        workspace::SourceLocation::Filesystem {
            root,
            subdirectory: None,
        } => original_resolved
            .source_root(root)
            .expect("selected owner source root"),
        _ => panic!("this opt-in proof selects the declared owner filesystem asset root"),
    };
    let asset: source::EntryPath = "collections/india-ai/testing-indias-ai-crossing/tracing-the-infrastructure-story/operating-and-payment.svg".parse().expect("owned asset entry");
    let asset_capture = configuration::capture_selected(
        &asset_root,
        std::slice::from_ref(&asset),
        Default::default(),
    )
    .expect("actual retained owner asset bytes");
    let actual_asset = files["site:rawthoughts"]
        .iter()
        .find(|(path, _)| path.ends_with("/operating-and-payment.svg"))
        .expect("materialized owner asset");
    assert_eq!(
        actual_asset.1.as_slice(),
        asset_capture.file(&asset).expect("retained SVG")
    );
    for document in documents.values() {
        let value: Value = toml::ParsedToml::parse(document, syntax())
            .expect("retained syntax")
            .deserialize()
            .expect("native value");
        if let Some(mounts) = value["documentation"]["mounts"].as_array() {
            for mount in mounts {
                assert_eq!(mount["render"], false);
                assert_eq!(mount["discoverable"], false);
                assert!(
                    expected["sources"]
                        .get(mount["source"].as_str().expect("disabled source"))
                        .is_none(),
                    "disabled manuals are not read"
                );
            }
        }
    }
    assert_eq!(
        native_cli(&recipe),
        expected,
        "real CLI uses the same selected bytes and semantics"
    );
    let report_bytes = serde_json::to_vec(&expected).expect("complete report encoding");
    let result_bytes = serde_json::to_vec(&json!({
        "isError":false,
        "structuredContent":expected,
        "content":[{"type":"text","text":std::str::from_utf8(&report_bytes).expect("JSON UTF-8")}]
    }))
    .expect("complete MCP result encoding");
    let transport = gazette_mcp::TransportLimits::new(1 << 20, 2 << 20)
        .expect("finite complete-corpus response budget");
    assert!(
        result_bytes.len() <= transport.max_response_bytes(),
        "complete native result needs {} bytes, not a truncated report",
        result_bytes.len()
    );
    println!(
        "complete publication MCP result: {} bytes, selected response ceiling {}",
        result_bytes.len(),
        transport.max_response_bytes()
    );
    mcp_carrier::initialized_interact_with_transport_limits(
        "2026-07-28",
        provider_deadline(&recipe),
        transport,
        |initialized, send| {
            assert_eq!(initialized["result"]["protocolVersion"], "2026-07-28");
            let catalog = send(json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}));
            let tool = catalog["result"]["tools"]
                .as_array()
                .expect("initialized tool discovery")
                .iter()
                .find(|tool| tool["name"] == "render_sites")
                .expect("actual render tool");
            let arguments = json!({"recipe_json":recipe.to_string(),"local":true,"allow_read":true,"max_input_bytes":INPUT_BYTES,"syntax":syntax()});
            jsonschema::validator_for(&tool["inputSchema"])
                .expect("input schema")
                .validate(&arguments)
                .expect("real discovered request contract");
            let response = send(
                json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"render_sites","arguments":arguments}}),
            );
            assert!(response.get("error").is_none(), "{response}");
            assert_eq!(response["result"]["isError"], false, "{response}");
            let actual = &response["result"]["structuredContent"];
            jsonschema::validator_for(&tool["outputSchema"])
                .expect("output schema")
                .validate(actual)
                .expect("actual native report");
            assert_eq!(
                *actual, expected,
                "whole initialized MCP report and every output digest"
            );
            assert_eq!(
                serde_json::from_str::<Value>(
                    response["result"]["content"][0]["text"]
                        .as_str()
                        .expect("readable report")
                )
                .expect("JSON text carrier"),
                *actual
            );
        },
    );
    let unchanged =
        configuration::load_workspace(&workspace_root, &workspace_path, Default::default())
            .expect("owner configuration remains native and unchanged");
    assert_eq!(unchanged.inventory().identity(), original_inventory);
    assert_eq!(json!(unchanged.documents().documents), original_documents);
    assert_eq!(json!(unchanged.documents().compilation.workspace), original);
    assert_eq!(
        fs::read(recipe_path).expect("unchanged owner recipe"),
        original_recipe
    );
    println!(
        "six owner publications: SDK/CLI/initialized MCP output identity, retained public/local projection, notes, math, legal processing, SVG, curated Markdown/llms, graph and synchronous configured materialization qualified; original document evidence: {original_documents}"
    );
}
