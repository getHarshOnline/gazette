//! Real browser queries over production search code and the emitted discovery corpus.

use super::*;
use crate::{
    browser_carrier::{chrome, dump, values},
    sites::{self, preview},
};
use gazette_sdk::html::TextKind;
use serde_json::json;

use crate::browser_sdk::{SEARCH as SDK, inputs};

const MANIFEST: &str = "package='theme:one'\nnamespace='fixture'\ntemplates='templates'\nbrowser_theme_colors={light='#ffffff',dark='#000000'}\nrender={page='page.html',alias='page.html'}\nresource_requirements=['module:search']\n";
fn plugin() -> String {
    format!(
        "package='fixture-plugin'\n[resources.'module:search']\ninputs={}\nbuild={{kind='browser-module',entry='controls.mjs'}}\n[providers.search]\nimplements=['publication.search/v1']\n",
        inputs(SDK)
    )
}
const TEMPLATE: &str = r#"<main>{{ body_html | safe }}</main><output id="search-observation">Search is unavailable without the selected runtime.</output>{% if presentation.search is defined %}<div id="search-controls" data-corpus="{{ presentation.search.url }}"></div>{% for resource in presentation.theme_resources %}<script type="module" src="{{ resource.url }}"></script>{% endfor %}{% endif %}"#;

// Controls and observation only: every query and failure uses the production SDK.
const CONTROLS: &str = r#"
import { loadIndex } from './search/index.mjs';
const output = document.getElementById('search-observation');
const exact = document.getElementById('search-controls').dataset.corpus;
const options = {profile:'lexical',maxDocuments:100,maxBytes:1048576,maxQueryBytes:1024,maxResults:20,allowNetwork:true,maxResponseBytes:1048576,timeoutMs:4000};
const result = {};
async function failure(url, changes) {
  try { await loadIndex(url, {...options,...changes}); return {unexpected:'accepted'}; }
  catch (error) { return {name:error.name,code:error.code}; }
}
try {
  const index = await loadIndex(exact, options);
  for (const [name, query, fields] of [
    ['title','quasar',{title:10,body:1}],
    ['body','bodytoken',{body:1}],
    ['project','subject',{projects:1}],
    ['empty',' ',undefined], ['absent','neverfindthis',undefined],
    ['hidden','hiddensecret',undefined], ['foreign','foreignsecret',undefined],
    ['literal','run',{body:1}],
  ]) result[name] = index.search(query,{limit:10,...(fields?{fields}:{})});
  const english = await loadIndex(exact,{...options,profile:'english'});
  result.english = english.search('run',{limit:10,fields:{body:1}});
  result.size = index.size;
  result.invalidCorpus = await failure(new URL('model.json',exact).href,{});
  result.missing = await failure(new URL('media/missing.json',exact).href,{});
  result.oversized = await failure(exact,{maxResponseBytes:1});
  result.denied = await failure(exact,{allowNetwork:false});
  const links = document.createElement('nav');
  for (const hit of result.title) {
    const link = document.createElement('a');
    link.href = hit.record.route.canonical_url;
    link.textContent = hit.record.content.title;
    links.append(link);
  }
  document.body.append(links);
  output.textContent = 'SEARCH_OBSERVATION:' + JSON.stringify(result);
} catch (error) { output.textContent = 'SEARCH_FAILURE:' + error.name + ':' + error.code; }
"#;

fn fixture() -> Fixture {
    let plugin = plugin();
    let page = PAGE
        .replace(
            "[content]\n",
            "projects=[{project='project:subject',kind='about'}]\n[content]\n",
        )
        .replace("One <escaped>", "Quasar <img src=x onerror=alert(1)>")
        .replace("**Retained** body", "Unique bodytoken running.");
    let second = PAGE
        .replace("content:one", "content:second")
        .replace("One <escaped>", "Other title")
        .replace("path='/guide/',aliases=['/old.html']", "path='/second/'")
        .replace("**Retained** body", "quasar");
    let hidden = PROJECT_DOC.replace("Retained native documentation", "hiddensecret");
    let foreign = crate::OTHER.replace("Other body", "foreignsecret");
    let mut files = vec![
        ("theme/theme.toml".to_owned(), MANIFEST.as_bytes()),
        ("theme/plugin.toml".to_owned(), plugin.as_bytes()),
        ("theme/templates/page.html".to_owned(), TEMPLATE.as_bytes()),
        ("theme/controls.mjs".to_owned(), CONTROLS.as_bytes()),
        ("one/page.md".to_owned(), page.as_bytes()),
        ("one/second.md".to_owned(), second.as_bytes()),
        ("project/guide.md".to_owned(), hidden.as_bytes()),
        ("other/page.md".to_owned(), foreign.as_bytes()),
    ];
    files.extend(
        SDK.iter()
            .map(|(path, data)| (format!("theme/{path}"), *data)),
    );
    Fixture::new(
        &files
            .iter()
            .map(|(path, data)| (path.as_str(), *data))
            .collect::<Vec<_>>(),
    )
}

fn configuration(fixture: &Fixture, enabled: bool) -> String {
    let selection = if enabled {
        "{state='enabled',provider={kind='plugin',package='fixture-plugin',provider='search'},requirement='optional',allow_absence='unsupported'}"
    } else {
        "{state='disabled'}"
    };
    let config = fixture
        .project_config(true)
        .replace(
            "id='project:subject'\n",
            "id='project:subject'\nname='Subject library'\n",
        )
        .replace("discoverable=true", "discoverable=false")
        .replacen(
            "[[principals]]",
            &format!(
                "[composition.plugins]\n'fixture-plugin'={{state='enabled'}}\n[composition.capabilities]\n'publication.search/v1'={selection}\n[composition.resources]\n'module:search'={{state='enabled',resource={{owner={{kind='plugin',package='fixture-plugin'}},resource='module:search'}},output='/assets/search.js',use='site'}}\n[[principals]]"
            ),
            1,
        )
        .replace(
            "authored=[{source='source:one',target='one'}]",
            "authored=[{source='source:one',target='one',static_assets={route_prefix='/media/'}}]",
        );
    let mut config = if enabled { enable(&config) } else { config };
    config.push_str(
        "[[plugin_packages]]\npackage='fixture-plugin'\nsource='source:theme'\nmanifest='plugin.toml'\n",
    );
    config
}

fn start(config: &str) -> (preview::Process, String) {
    let mut process = preview::Process::with_arguments(
        &sites::request(config, &["site:one", "site:other"]),
        &["--local-urls"],
    );
    let ready = process.event();
    assert_eq!(ready["event"], "ready", "{ready}");
    let base = ready["sites"]["site:one"]["url"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        base,
        format!("http://{}/mount/", preview::address(&ready, "site:one"))
    );
    (process, base)
}

#[test]
#[ignore = "requires explicit real Chrome provider qualification"]
fn chrome_queries_the_retained_production_search_module_and_reports_typed_failures() {
    let browser = chrome();
    let fixture = fixture();
    let config = configuration(&fixture, true);
    let inputs = acquire(&config);
    let rendered = render(&inputs).expect("native retained search package compilation");
    let evidence = serde_json::to_value(rendered.site().evidence()).unwrap();
    for (path, data) in SDK {
        assert_eq!(
            evidence["resources"]["module:search"]["dependencies"][*path],
            source::Sha256Digest::of(data).to_string(),
            "actual SDK source dependency {path}"
        );
    }
    let bundle = std::str::from_utf8(bytes(rendered.site().output(), "assets/search.js")).unwrap();
    assert!(bundle.contains("SEARCH_OBSERVATION:"));
    let vendor = std::str::from_utf8(SDK.last().unwrap().1).unwrap();
    let notice_end = vendor.find("*/").expect("production provider licence") + 2;
    let notice = &vendor[..notice_end];
    assert!(notice.contains("Copyright (C) 2017 Oliver Nightingale"));
    assert!(notice.contains("Copyright (C) 2017 Wei Song"));
    assert!(notice.contains("THE SOFTWARE IS PROVIDED"));
    assert!(
        bundle.contains(notice),
        "complete production MIT copyright, permission and disclaimer preserved in emitted JavaScript"
    );
    assert!(
        !bundle.contains("from './search/index.mjs'"),
        "native owner bundles the captured closure"
    );
    let corpus: Vec<Value> =
        serde_json::from_slice(bytes(rendered.site().output(), "search.json")).unwrap();
    assert_eq!(
        corpus.len(),
        3,
        "two articles and one collection, not aliases or hidden docs"
    );
    let (mut server, base) = start(&config);
    // Preview and SDK already captured exact inputs; neither requires the native files again.
    fs::remove_dir_all(fixture.directory.path().join("theme")).unwrap();
    fs::remove_dir_all(fixture.directory.path().join("one")).unwrap();
    let repeated = render(&inputs).expect("retained rerender without live sources");
    assert_eq!(
        rendered.site().output().inventory().identity(),
        repeated.site().output().inventory().identity()
    );
    let dom = dump(&browser, &format!("{base}guide/"), true);
    let text = values(&dom, TextKind::Text);
    let observed = text
        .iter()
        .find_map(|value| value.strip_prefix("SEARCH_OBSERVATION:"))
        .unwrap_or_else(|| panic!("no production browser result: {dom}"));
    let result: Value = serde_json::from_str(observed).unwrap();
    assert_eq!(result["size"], 3);
    for (query, id) in [
        ("title", "content:one"),
        ("body", "content:one"),
        ("project", "content:one"),
        ("english", "content:one"),
    ] {
        assert_eq!(
            result[query][0]["record"]["content"]["id"], id,
            "{query}: {result}"
        );
        assert_eq!(
            result[query][0]["record"]["route"]["canonical_url"],
            format!("{base}guide/")
        );
    }
    let project = result["project"][0]["record"]["projects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|link| link["project"]["id"] == "project:subject")
        .expect("same graph-owned project identity in returned metadata");
    assert_eq!(project["project"]["name"], "Subject library");
    let ranked = result["title"].as_array().unwrap();
    assert_eq!(ranked.len(), 2);
    assert!(ranked[0]["score"].as_f64().unwrap() > ranked[1]["score"].as_f64().unwrap());
    for query in ["empty", "absent", "hidden", "foreign", "literal"] {
        assert_eq!(result[query], json!([]), "{query}");
    }
    for (name, code) in [
        ("invalidCorpus", "search_data"),
        ("missing", "search_response"),
        ("oversized", "search_limit"),
        ("denied", "search_permission"),
    ] {
        assert_eq!(result[name], json!({"name":"SearchError","code":code}));
    }
    assert!(!dom.contains("<img src=\"x\""));
    assert!(values(&dom, TextKind::EventAttribute).is_empty());
    assert!(
        text.iter()
            .any(|value| value == "Quasar <img src=x onerror=alert(1)>")
    );
    let attributes = values(&dom, TextKind::Attribute);
    assert!(attributes.contains(&format!("{base}guide/")));
    assert!(!attributes.contains(&format!("{base}old.html")));
    server.close();
}

#[test]
#[ignore = "requires explicit real Chrome provider qualification"]
fn chrome_does_not_start_search_when_workspace_explicitly_disables_it() {
    let browser = chrome();
    let fixture = fixture();
    let config = configuration(&fixture, false);
    let inputs = acquire(&config);
    let rendered = render(&inputs).unwrap();
    let model: Value =
        serde_json::from_slice(bytes(rendered.site().output(), "model.json")).unwrap();
    assert!(
        rendered
            .site()
            .output()
            .file(&"assets/search.js".parse().unwrap())
            .is_some(),
        "disabled search semantics do not rewrite the independent site resource"
    );
    assert!(
        rendered
            .site()
            .output()
            .file(&"search.json".parse().unwrap())
            .is_none()
    );
    assert!(model["capabilities"].get("publication.search/v1").is_none());
    assert!(model["resources"].get("module:search").is_some());
    for page in model["pages"].as_object().unwrap().values() {
        assert_eq!(
            page["presentation"]["theme_resources"][0]["id"],
            "module:search"
        );
        assert!(page["presentation"].get("search").is_none());
    }
    let (mut server, base) = start(&config);
    let dom = dump(&browser, &format!("{base}guide/"), true);
    assert!(!dom.contains("<script"));
    assert!(!dom.contains("SEARCH_OBSERVATION:"));
    assert!(
        values(&dom, TextKind::Text)
            .iter()
            .any(|value| value == "Search is unavailable without the selected runtime.")
    );
    server.close();
}

#[test]
fn retained_search_module_captures_complete_production_dependency_set() {
    let fixture = fixture();
    let inputs = acquire(&configuration(&fixture, true));
    let rendered = render(&inputs).expect("production search resource closure");
    let evidence = serde_json::to_value(rendered.site().evidence()).unwrap();
    for (path, data) in SDK {
        assert_eq!(
            evidence["resources"]["module:search"]["dependencies"][*path],
            source::Sha256Digest::of(data).to_string()
        );
    }
}
