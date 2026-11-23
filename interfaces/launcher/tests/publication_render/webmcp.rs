//! Actual browser `WebMCP` over retained Gazette page and search projections.

use super::*;
use crate::{
    browser_carrier::{chrome, dump_with_flags, values},
    sites::{self, preview},
};
use gazette_sdk::html::TextKind;
use serde_json::json;

#[path = "../support/webmcp_browser.rs"]
mod native_browser;

use crate::browser_sdk;

const MANIFEST: &str = "package='theme:one'\nnamespace='fixture'\ntemplates='templates'\nbrowser_theme_colors={light='#ffffff',dark='#000000'}\nrender={page='page.html',alias='page.html'}\nresource_requirements=['module:webmcp']\n";
fn plugin(sdk: &[(&str, &[u8])]) -> String {
    format!(
        "package='fixture-plugin'\n[resources.'module:webmcp']\ninputs={}\nbuild={{kind='browser-module',entry='controls.mjs'}}\n",
        browser_sdk::inputs(sdk)
    )
}
const TEMPLATE: &str = r#"<!doctype html><html><body><main><h1>{{ content.title }}</h1>{{ body_html | safe }}<a href="{{ route.url }}">Read this page</a></main><output id="webmcp-observation">Read this page without browser tools.</output>{% if presentation.webmcp is defined %}<script id="webmcp-page" type="application/json">{{ presentation.webmcp | json_script }}</script><script id="webmcp-selection" type="application/json">{{ presentation.capabilities['publication.webmcp/v1'] | json_script }}</script><div id="webmcp-search" data-corpus="{{ presentation.search.url }}"></div>{% for resource in presentation.theme_resources %}<script type="module" src="{{ resource.url }}"></script>{% endfor %}{% endif %}</body></html>"#;

// Observation controls only: registration, snapshots and ranking use the captured SDK.
// Do not top-level-await getTools: native discovery waits for document loading to finish.
const CONTROLS: &str = r#"
import { registerReadOnlyTools } from './webmcp/index.mjs';
import { loadIndex } from './search/index.mjs';
async function run() {
  const output = document.getElementById('webmcp-observation');
  let stage = 'native-api';
  output.textContent = 'WEBMCP_STAGE:' + stage;
  try {
    const context = document.modelContext;
    if (!context || typeof context.getTools !== 'function' || typeof context.executeTool !== 'function') {
      output.textContent = 'WEBMCP_UNSUPPORTED:' + JSON.stringify({native:false}); return;
    }
    const result = {before:(await context.getTools()).map(tool=>tool.name)};
    const selection = JSON.parse(document.getElementById('webmcp-selection').textContent);
    const page = JSON.parse(document.getElementById('webmcp-page').textContent);
    const limits = {maxInputBytes:1024,maxOutputBytes:1048576,maxResults:10,maxNodes:10000,maxDepth:32,maxConcurrent:2,timeoutMs:4000};
    const index = await loadIndex(document.getElementById('webmcp-search').dataset.corpus,
      {profile:'lexical',maxDocuments:100,maxBytes:1048576,maxQueryBytes:1024,maxResults:10,allowNetwork:true,maxResponseBytes:1048576,timeoutMs:4000});
    let started, cancelled = false, closed = false, delivered;
    const callbackAborted = new Promise(resolve=>{delivered=resolve;});
    const callbacks = {
      currentPage: ({signal}) => { if (signal.aborted) throw signal.reason; return page; },
      search: (query,{limit,signal}) => {
        if (query === '__cancel__' || query === '__close__') {
          return new Promise((_,reject)=>{
            signal.addEventListener('abort',()=>{
              if(query==='__cancel__') {
                cancelled=true;
                delivered({name:signal.reason?.name,code:signal.reason?.code});
              } else closed=true;
              reject(signal.reason);
            },{once:true});
            started();
          });
        }
        if (signal.aborted) throw signal.reason;
        return index.search(query,{limit});
      },
    };
    stage = 'registration';
    output.textContent = 'WEBMCP_STAGE:' + stage;
    const registration = await registerReadOnlyTools(document,{tools:selection.value.tools,limits,...callbacks});
    const tools = await context.getTools();
    result.tools = tools.map(tool=>({name:tool.name,annotations:tool.annotations,inputSchema:tool.inputSchema}));
    const current = tools.find(tool=>tool.name==='current_page');
    const search = tools.find(tool=>tool.name==='search');
    // The native testing API accepts the schema's input object directly.
    const execute = async (tool,input,options) => {
      const result = await context.executeTool(tool,input,options);
      return typeof result === 'string' ? JSON.parse(result) : result;
    };
    stage = 'public-data';
    output.textContent = 'WEBMCP_STAGE:' + stage;
    result.current = await execute(current,{});
    result.search = await execute(search,{query:'quasar',limit:5});
    result.directSearch = index.search('quasar',{limit:5});
    result.hidden = await execute(search,{query:'hiddensecret',limit:5});
    result.foreign = await execute(search,{query:'foreignsecret',limit:5});
    stage = 'native-cancel';
    output.textContent = 'WEBMCP_STAGE:' + stage;
    const begun = new Promise(resolve=>{started=resolve;});
    const abort = new AbortController();
    const pending = execute(search,{query:'__cancel__',limit:1},{signal:abort.signal}).then(()=>({unexpected:'accepted'}),error=>({name:error.name}));
    await begun;
    let deadlineTimer;
    const callbackDeadline = new Promise((_,reject)=>{
      deadlineTimer=setTimeout(()=>reject(new Error('native callback cancellation deadline')),2000);
    });
    abort.abort();
    try {
      [result.cancellation,result.callbackAbort] = await Promise.all([pending,Promise.race([callbackAborted,callbackDeadline])]);
    } finally {clearTimeout(deadlineTimer);}
    result.callbackCancelled = cancelled;
    stage = 'native-close';
    output.textContent = 'WEBMCP_STAGE:' + stage;
    const closing = new Promise(resolve=>{started=resolve;});
    const active = execute(search,{query:'__close__',limit:1}).then(()=>({unexpected:'accepted'}),error=>({name:error.name}));
    await closing; registration.close(); result.closedExecution = await active;
    result.callbackClosed = closed;
    result.after = (await context.getTools()).map(tool=>tool.name);
    try { await execute(current,{}); result.removed = {unexpected:'accepted'}; }
    catch(error) { result.removed = {name:error.name}; }
    stage = 'selected-set';
    try { await registerReadOnlyTools(document,{tools:[],limits,...callbacks}); result.empty={unexpected:'accepted'}; }
    catch(error) { result.empty={code:error.code}; }
    try { await registerReadOnlyTools(document,{tools:['publish'],limits,...callbacks}); result.unknown={unexpected:'accepted'}; }
    catch(error) { result.unknown={code:error.code}; }
    const only = await registerReadOnlyTools(document,{tools:['current_page'],limits,currentPage:callbacks.currentPage});
    result.only = (await context.getTools()).map(tool=>tool.name);
    try { await execute(search,{query:'quasar',limit:1}); result.unselected={unexpected:'accepted'}; }
    catch(error) { result.unselected={name:error.name}; }
    only.close(); result.final = (await context.getTools()).map(tool=>tool.name);
    output.textContent = 'WEBMCP_OBSERVATION:' + JSON.stringify(result);
  } catch(error) { output.textContent = 'WEBMCP_FAILURE:' + JSON.stringify({stage,name:error.name,code:error.code,message:error.message}); }
}
void run();
"#;

fn fixture() -> Fixture {
    let sdk = browser_sdk::webmcp();
    let plugin = plugin(&sdk);
    let page = PAGE.replace("One <escaped>", "Quasar <escaped>");
    let hidden = PROJECT_DOC.replace("Retained native documentation", "hiddensecret");
    let foreign = OTHER.replace("Other body", "foreignsecret");
    let mut files = vec![
        ("theme/theme.toml".to_owned(), MANIFEST.as_bytes()),
        ("theme/plugin.toml".to_owned(), plugin.as_bytes()),
        ("theme/templates/page.html".to_owned(), TEMPLATE.as_bytes()),
        ("theme/controls.mjs".to_owned(), CONTROLS.as_bytes()),
        ("one/page.md".to_owned(), page.as_bytes()),
        ("project/guide.md".to_owned(), hidden.as_bytes()),
        ("other/page.md".to_owned(), foreign.as_bytes()),
    ];
    files.extend(
        sdk.iter()
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
    let capability = if enabled {
        "{state='enabled',provider={kind='gazette',provider='webmcp-client'},requirement='required'}"
    } else {
        "{state='disabled'}"
    };
    let config = fixture.project_config(true)
        .replace("discoverable=true", "discoverable=false")
        .replace("id='site:other'\npublisher='owner:one'\ndeployable=false\n", "id='site:other'\npublisher='owner:one'\ndeployable=false\n[sites.composition.capabilities]\n'publication.webmcp/v1'={state='disabled'}\n")
        .replacen("[[principals]]", &format!("[composition.plugins]\n'fixture-plugin'={{state='enabled'}}\n[composition.capabilities]\n'publication.webmcp/v1'={capability}\n[composition.resources]\n'module:webmcp'={{state='enabled',resource={{owner={{kind='plugin',package='fixture-plugin'}},resource='module:webmcp'}},output='/assets/webmcp.js',use='site'}}\n[[principals]]"),1)
        .replacen("public_base_url=", &format!("machine={{search='lexical'}}\nprocessing={{webmcp={{enabled={enabled},tools=['current_page','search']}}}}\npublic_base_url="),1);
    format!(
        "{config}[[plugin_packages]]\npackage='fixture-plugin'\nsource='source:theme'\nmanifest='plugin.toml'\n"
    )
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

fn observation(dom: &str) -> Value {
    values(dom, TextKind::Text)
        .iter()
        .find_map(|value| value.strip_prefix("WEBMCP_OBSERVATION:").map(str::to_owned))
        .map(|value| serde_json::from_str(&value).expect("complete actual browser observation"))
        .unwrap_or_else(|| {
            let diagnostics = values(dom, TextKind::Text)
                .into_iter()
                .filter(|text| text.starts_with("WEBMCP_"))
                .collect::<Vec<_>>();
            panic!(
                "native WebMCP acceptance unavailable or failed: {diagnostics:?}; bounded DOM={}",
                dom.chars().take(4096).collect::<String>()
            )
        })
}

#[test]
#[ignore = "requires explicit installed Chrome with native WebMCP testing feature"]
fn native_chrome_executes_selected_retained_public_tools_and_cancels_then_unregisters() {
    let browser = chrome();
    let fixture = fixture();
    let config = configuration(&fixture, true);
    let inputs = acquire(&config);
    assert!(
        inputs.capability_capture().is_some(),
        "native SDK acquisition already freezes selected bindings"
    );
    let rendered = render(&inputs).expect("retained native WebMCP resource compilation");
    let model: Value =
        serde_json::from_slice(bytes(rendered.site().output(), "model.json")).unwrap();
    assert_eq!(
        model["capabilities"]["publication.webmcp/v1"]["value"],
        json!({"contract":"publication-webmcp-v1","tools":["current_page","search"]})
    );
    let evidence = serde_json::to_value(rendered.site().evidence()).unwrap();
    for (path, data) in &browser_sdk::webmcp() {
        assert_eq!(
            evidence["resources"]["module:webmcp"]["dependencies"][*path],
            source::Sha256Digest::of(data).to_string(),
            "exact production SDK bytes {path}"
        );
    }
    let (mut server, base) = start(&config);
    let dom = native_browser::observe(&browser, &format!("{base}guide/"));
    let observed = observation(&dom);
    assert_eq!(
        observed["before"],
        json!([]),
        "import has no native registration effects"
    );
    let names: Vec<_> = observed["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["current_page", "search"]);
    for tool in observed["tools"].as_array().unwrap() {
        assert_eq!(tool["annotations"]["readOnlyHint"], true);
        let schema: Value = match tool["inputSchema"].as_str() {
            Some(serialized) => serde_json::from_str(serialized).expect("native schema JSON"),
            None => tool["inputSchema"].clone(),
        };
        assert_eq!(schema["additionalProperties"], false);
    }
    let page = &model["pages"]["content:one"];
    assert_eq!(observed["current"]["body_text"], page["body_text"]);
    let mut expected_route = page["route"].clone();
    expected_route["url"] = json!(format!("{base}guide/"));
    expected_route["canonical_url"] = json!(format!("{base}guide/"));
    expected_route["aliases"][0]["url"] = json!(format!("{base}old.html"));
    assert_eq!(observed["current"]["route"], expected_route);
    let mut expected_content = page["content"].clone();
    expected_content["resolved_route"] = expected_route;
    assert_eq!(observed["current"]["content"], expected_content);
    let mut fields: Vec<_> = observed["current"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    fields.sort_unstable();
    assert_eq!(
        fields,
        ["body_text", "content", "route"],
        "explicit public projection, not private evidence or reconstructed DOM facts"
    );
    assert_eq!(
        observed["search"], observed["directSearch"],
        "real production ranking and immutable records"
    );
    assert_eq!(
        observed["search"][0]["record"]["content"]["id"],
        "content:one"
    );
    assert_eq!(
        observed["search"][0]["record"]["route"]["url"],
        format!("{base}guide/")
    );
    assert_eq!(observed["hidden"], json!([]));
    assert_eq!(observed["foreign"], json!([]));
    assert_eq!(observed["cancellation"]["name"], "AbortError");
    assert_eq!(observed["callbackAbort"]["code"], "webmcp_cancelled");
    assert_eq!(
        observed["callbackCancelled"], true,
        "native cancellation reaches selected callback"
    );
    assert!(observed["closedExecution"].get("name").is_some());
    assert_eq!(observed["callbackClosed"], true);
    assert_eq!(observed["after"], json!([]));
    assert!(observed["removed"].get("name").is_some());
    assert_eq!(observed["empty"]["code"], "webmcp_options");
    assert_eq!(observed["unknown"]["code"], "webmcp_options");
    assert_eq!(observed["only"], json!(["current_page"]));
    assert!(observed["unselected"].get("name").is_some());
    assert_eq!(observed["final"], json!([]));
    let no_js = dump_with_flags(
        &browser,
        &format!("{base}guide/"),
        false,
        &["--enable-features=WebMCP"],
    );
    let text = values(&no_js, TextKind::Text).join(" ");
    assert!(text.contains("Quasar <escaped>"));
    assert!(text.contains("Retained"));
    assert!(text.contains("Read this page without browser tools."));
    assert!(!text.contains("WEBMCP_OBSERVATION:"));
    server.close();
}

#[test]
#[ignore = "requires explicit installed Chrome with native WebMCP testing feature"]
fn disabled_webmcp_keeps_readable_publication_without_loading_or_registering_tools() {
    let browser = chrome();
    let fixture = fixture();
    let config = configuration(&fixture, false);
    let inputs = acquire(&config);
    let rendered = render(&inputs).expect("disabled native capability remains a readable site");
    let model: Value =
        serde_json::from_slice(bytes(rendered.site().output(), "model.json")).unwrap();
    assert!(model["capabilities"].get("publication.webmcp/v1").is_none());
    let html = std::str::from_utf8(bytes(rendered.site().output(), "guide/index.html")).unwrap();
    assert!(!html.contains("webmcp-page"));
    assert!(
        !html.contains("type=\"module\""),
        "explicitly selected resource does not activate browser runtime without the selected semantic capability"
    );
    assert!(
        rendered
            .site()
            .output()
            .file(&"assets/webmcp.js".parse().unwrap())
            .is_some(),
        "compiled resource and semantic activation remain independent"
    );
    let (mut server, base) = start(&config);
    let dom = dump_with_flags(
        &browser,
        &format!("{base}guide/"),
        true,
        &["--enable-features=WebMCP"],
    );
    let text = values(&dom, TextKind::Text).join(" ");
    assert!(text.contains("Quasar <escaped>"));
    assert!(text.contains("Read this page without browser tools."));
    assert!(!text.contains("WEBMCP_OBSERVATION:"));
    assert!(!text.contains("WEBMCP_FAILURE:"));
    server.close();
}

#[test]
fn retained_webmcp_module_captures_complete_production_dependency_set() {
    let fixture = fixture();
    let inputs = acquire(&configuration(&fixture, true));
    let rendered = render(&inputs).expect("production browser-tool resource closure");
    let model: Value =
        serde_json::from_slice(bytes(rendered.site().output(), "model.json")).unwrap();
    let page = &model["pages"]["content:one"];
    let public = &page["presentation"]["webmcp"];
    assert_eq!(public["content"]["id"], "content:one");
    assert_eq!(public["body_text"], page["body_text"]);
    assert_eq!(public.as_object().unwrap().len(), 3);
    assert!(public.get("source").is_none());
    assert!(public["content"].get("members").is_none());
    assert!(
        model["pages"]["doc:native"]["presentation"]
            .get("webmcp")
            .is_none()
    );
    let alias = std::str::from_utf8(bytes(rendered.site().output(), "old.html")).unwrap();
    assert!(!alias.contains("id=\"webmcp-page\""));
    let evidence = serde_json::to_value(rendered.site().evidence()).unwrap();
    for (path, data) in &browser_sdk::webmcp() {
        assert_eq!(
            evidence["resources"]["module:webmcp"]["dependencies"][*path],
            source::Sha256Digest::of(data).to_string()
        );
    }
}
