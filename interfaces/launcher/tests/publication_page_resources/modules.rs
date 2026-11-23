//! Two retained entry points share one canonical stateful module through native ESM linking.

use super::*;

const SHARED: &str = "import { step } from './increment.ts'; globalThis.initializations = (globalThis.initializations || 0) + 1; let count: number = 0; export function next(): number { count += step; return count; }";
const MODULE_MANIFEST: &str = "package='shared-modules'\n[resources.'module:state']\ninputs=['state.ts','increment.ts']\nbuild={kind='browser-module',entry='state.ts'}\n";
const MODULE_SELECTION: &str = "'module:state'={state='enabled',resource={owner={kind='plugin',package='shared-modules'},resource='module:state'},output='/assets/shared-state.js',use='dependency'}\n";
const TRANSITIVE_MANIFEST: &str = "package='shared-modules'\n[resources.'module:state']\ninputs=['state.ts']\nbuild={kind='browser-module',entry='state.ts',imports={'@publication/base'='module:base'}}\n[resources.'module:base']\ninputs=['base.ts']\nbuild={kind='browser-module',entry='base.ts'}\n";
const TRANSITIVE_SELECTION: &str = "'module:state'={state='enabled',resource={owner={kind='plugin',package='shared-modules'},resource='module:state'},output='/assets/shared-state.js',use='dependency'}\n'module:base'={state='enabled',resource={owner={kind='plugin',package='shared-modules'},resource='module:base'},output='/assets/base.js',use='dependency'}\n";
const FIRST: &str = "import { answer } from './dep.ts'; import { next } from '@publication/state'; export const value: number = answer + next();";
const SECOND: &str =
    "import { next } from '@publication/state'; export const value: number = next();";
const MODULE_TEMPLATE: &str = r#"{{ body_html | safe }}<script type="importmap">{{ presentation.browser_modules.import_map | json_encode | safe }}</script>{% for module in presentation.browser_modules.preloads %}<link rel="modulepreload" href="{{ module.url }}" data-sha256="{{ module.sha256 }}">{% endfor %}{% for resource in presentation.theme_resources %}<script data-shared="yes" type="module" src="{{ resource.url }}"></script>{% endfor %}{% for resource in presentation.resources %}<script data-local="yes" type="module" src="{{ resource.url }}"></script>{% endfor %}"#;

fn fixture() -> Fixture {
    let resources = RESOURCES.replace(
        "entry='main.ts'",
        "entry='main.ts',imports={'@publication/state'='module:state'}",
    );
    let second = "[[resources]]\ninputs=['second.ts']\nbuild={kind='browser-module',entry='second.ts',imports={'@publication/state'='module:state'}}\noutput='/assets/second-entry.js'\n";
    Fixture::new(
        false,
        &[
            (
                "theme/theme.toml",
                THEME_MANIFEST_WITHOUT_RESOURCES.as_bytes(),
            ),
            (
                "content/one.md",
                page("one", "/one/", &resources).as_bytes(),
            ),
            ("content/two.md", page("two", "/two/", second).as_bytes()),
            ("content/main.ts", FIRST.as_bytes()),
            ("content/second.ts", SECOND.as_bytes()),
            ("project/plugin.toml", MODULE_MANIFEST.as_bytes()),
            ("project/state.ts", SHARED.as_bytes()),
            ("project/increment.ts", b"export const step: number = 1;"),
            ("theme/templates/page.html", MODULE_TEMPLATE.as_bytes()),
        ],
    )
}

fn configuration(fixture: &Fixture, selection: &str) -> String {
    let mut config = fixture
        .configuration()
        .replace(
            "[composition.plugins]",
            "[composition.plugins]\n'shared-modules'={state='enabled'}",
        )
        .replacen("[[principals]]", &format!("{selection}[[principals]]"), 1);
    config.push_str(&format!(
        "[[sources]]\nid='source:modules'\nlocation={{kind='repository',root={:?},commit='{}',subdirectory='project'}}\n[[plugin_packages]]\npackage='shared-modules'\nsource='source:modules'\nmanifest='plugin.toml'\n",
        fixture.root.path().to_str().expect("fixture root"), fixture.commit,
    ));
    config
}

fn native_esm(tree: &source::CapturedSource) {
    let base = "https://one.example/mount/";
    let first = format!("{base}assets/a-second.js");
    let second = format!("{base}assets/second-entry.js");
    let sources: BTreeMap<_, _> = [
        "assets/a-second.js",
        "assets/second-entry.js",
        "assets/shared-state.js",
    ]
    .into_iter()
    .map(|path| {
        (
            format!("{base}{path}"),
            std::str::from_utf8(bytes(tree, path)).expect("compiled ESM"),
        )
    })
    .collect();
    // Native Node module linking consumes exact retained outputs and canonical identifiers.
    // The finite linker can only resolve the three supplied modules; no rewriting or live reads.
    let script = r#"
import { SourceTextModule, createContext } from 'node:vm';
let input = ''; for await (const chunk of process.stdin) input += chunk;
const { sources, entries } = JSON.parse(input);
const context = createContext({}); const cache = new Map();
function module(url) {
  if (!Object.hasOwn(sources, url)) throw new Error('unretained module');
  if (!cache.has(url)) cache.set(url, new SourceTextModule(sources[url], { identifier: url, context }));
  return cache.get(url);
}
const values = [];
for (const url of entries) {
  const entry = module(url);
  await entry.link((specifier, referencing) => module(new URL(specifier, referencing.identifier).href));
  await entry.evaluate(); values.push(entry.namespace.value);
}
process.stdout.write(JSON.stringify({ values, initializations: context.initializations, modules: [...cache.keys()].sort() }));
"#;
    let mut child = Command::new(super::native::node())
        .args([
            "--experimental-vm-modules",
            "--input-type=module",
            "--eval",
            script,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("existing native Node ESM fixture runtime");
    child
        .stdin
        .take()
        .expect("native stdin")
        .write_all(
            &serde_json::to_vec(&json!({"sources":sources,"entries":[first,second]}))
                .expect("retained module input"),
        )
        .expect("complete retained module input");
    let result = child.wait_with_output().expect("native ESM completion");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let observed: Value =
        serde_json::from_slice(&result.stdout).expect("native module observations");
    assert_eq!(observed["values"], json!([43, 2]));
    assert_eq!(
        observed["initializations"], 1,
        "both entries use one stateful module instance"
    );
    assert_eq!(
        observed["modules"]
            .as_array()
            .expect("exact module cache")
            .len(),
        3
    );
}

#[test]
fn two_entry_points_share_one_retained_cross_source_module_without_autoloading_it() {
    let fixture = fixture();
    let config = configuration(&fixture, MODULE_SELECTION);
    let inputs = acquire(&config);
    assert!(
        inputs
            .sources()
            .contains_key(&"source:modules".parse::<Key>().expect("source"))
    );
    let rendered = render(&inputs).expect("cross-source module composition");
    let tree = rendered.site().output();
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).expect("shared model");
    let url = "https://one.example/mount/assets/shared-state.js";
    assert_eq!(
        model["browser_modules"]["import_map"]["imports"]["@publication/state"],
        url
    );
    assert_eq!(
        model["browser_modules"]["modules"]["module:state"]["url"],
        url
    );
    assert_eq!(
        model["browser_modules"]["modules"]["module:state"]["dependencies"],
        json!([])
    );
    for (content, index) in [("content:one", 1), ("content:two", 0)] {
        assert_eq!(
            model["pages"][content]["presentation"]["resources"][index]["references"],
            json!({(url):"assets/shared-state.js"})
        );
        assert!(
            model["pages"][content]["presentation"]["theme_resources"]
                .as_array()
                .expect("ordered Theme resources")
                .iter()
                .all(|resource| resource["id"] != "module:state"),
            "a page import does not become a Theme requirement"
        );
        assert_eq!(
            model["pages"][content]["presentation"]["browser_modules"]["import_map"],
            model["browser_modules"]["import_map"]
        );
        let preloads = model["pages"][content]["presentation"]["browser_modules"]["preloads"]
            .as_array()
            .expect("page-specific preload closure");
        assert_eq!(preloads.len(), 2);
        assert_eq!(
            preloads[0]["url"],
            "https://one.example/mount/assets/shared.js"
        );
        assert_eq!(preloads[1]["url"], url);
    }
    for path in ["one/index.html", "two/index.html"] {
        let html = std::str::from_utf8(bytes(tree, path))
            .expect("Theme HTML")
            .replace("&#x2F;", "/");
        assert!(html.contains("<script type=\"importmap\">"));
        assert!(html.contains("@publication/state"));
        assert!(html.contains(&format!("<link rel=\"modulepreload\" href=\"{url}\"")));
        assert!(
            !html.contains(&format!("<script type=\"module\" src=\"{url}\"")),
            "an import-only module must never become an execution script"
        );
    }
    assert_eq!(
        tree.inventory()
            .entries()
            .iter()
            .filter(|entry| entry.path.as_str() == "assets/shared-state.js")
            .count(),
        1
    );
    let evidence =
        serde_json::to_value(rendered.site().evidence()).expect("private native evidence");
    let module = &evidence["resources"]["module:state"];
    assert_eq!(module["source"], "source:modules");
    assert_eq!(
        module["resource"],
        json!({"owner":{"kind":"plugin","package":"shared-modules"},"resource":"module:state"})
    );
    assert_eq!(module["url"], url);
    assert_eq!(
        module["dependencies"]
            .as_object()
            .expect("retained local module closure")
            .len(),
        2
    );
    assert_eq!(
        module["dependencies"]["state.ts"],
        source::Sha256Digest::of(SHARED.as_bytes()).to_string()
    );
    native_esm(tree);

    let output = tempdir().expect("fresh native CLI stage");
    let result = cli(&config, output.path());
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let receipt: Value = serde_json::from_slice(&result.stdout).expect("CLI receipt");
    let staged = source::inspect(
        Path::new(receipt["directory"].as_str().expect("stage")),
        None,
        source::Limits::default(),
    )
    .expect("native read-back");
    assert_eq!(staged.identity(), tree.inventory().identity());

    fs::write(
        fixture.root.path().join("project/state.ts"),
        "throw new Error('not retained');",
    )
    .expect("change only disposable native source after capture");
    let replay =
        render(&inputs).expect("retained cross-source compilation never reopens native inputs");
    assert_eq!(
        tree.inventory().identity(),
        replay.site().output().inventory().identity()
    );
    assert_eq!(
        bytes(tree, "assets/shared-state.js"),
        bytes(replay.site().output(), "assets/shared-state.js")
    );
}

#[test]
fn missing_or_disabled_shared_module_refuses_rendering_after_activated_plugin_admission() {
    let fixture = fixture();
    for selection in ["", "'module:state'={state='disabled'}\n"] {
        let config = configuration(&fixture, selection);
        let inputs = acquire(&config);
        assert!(
            inputs
                .sources()
                .contains_key(&"source:modules".parse::<Key>().expect("source")),
            "explicit plugin activation captures its exact package source"
        );
        let metadata = inputs
            .check_metadata(toml::Limits::default())
            .expect("explicitly activated plugin package admits independently");
        assert!(
            metadata.inspect().plugin_packages.contains_key(
                &"shared-modules"
                    .parse::<gazette_sdk::workspace::PluginPackageId>()
                    .expect("plugin package"),
            ),
            "admitted package presence does not select one of its resources"
        );
        let error = render(&inputs).expect_err("no unselected or disabled source fallback");
        assert_eq!(error.code(), "resource_input");
    }
}

#[test]
fn import_and_preload_projection_uses_actual_transitive_module_edges() {
    let resources = RESOURCES.replace(
        "entry='main.ts'",
        "entry='main.ts',imports={'@publication/state'='module:state'}",
    );
    let second = "[[resources]]\ninputs=['second.ts']\nbuild={kind='browser-module',entry='second.ts',imports={'@publication/state'='module:state'}}\noutput='/assets/second-entry.js'\n";
    let fixture = Fixture::new(
        false,
        &[
            (
                "theme/theme.toml",
                THEME_MANIFEST_WITHOUT_RESOURCES.as_bytes(),
            ),
            (
                "content/one.md",
                page("one", "/one/", &resources).as_bytes(),
            ),
            ("content/two.md", page("two", "/two/", second).as_bytes()),
            ("content/main.ts", FIRST.as_bytes()),
            ("content/second.ts", SECOND.as_bytes()),
            ("project/plugin.toml", TRANSITIVE_MANIFEST.as_bytes()),
            (
                "project/state.ts",
                b"import { step } from '@publication/base'; export function next(){ return step; }",
            ),
            ("project/base.ts", b"export const step = 1;"),
        ],
    );
    let inputs = acquire(&configuration(&fixture, TRANSITIVE_SELECTION));
    let rendered = render(&inputs).expect("transitive shared module graph");
    let tree = rendered.site().output();
    assert!(
        std::str::from_utf8(bytes(tree, "assets/a-second.js"))
            .expect("compiled entry")
            .contains("shared-state.js"),
        "the retained entry must preserve its actual shared-module edge"
    );
    assert!(
        tree.file(&"assets/shared-state.js".parse().unwrap())
            .is_some()
    );
    assert!(tree.file(&"assets/base.js".parse().unwrap()).is_some());
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).expect("shared model");
    let browser = &model["browser_modules"];
    assert_eq!(
        browser["import_map"]["imports"],
        json!({
            "@publication/base":"https://one.example/mount/assets/base.js",
            "@publication/state":"https://one.example/mount/assets/shared-state.js"
        })
    );
    assert_eq!(
        browser["modules"]["module:state"]["dependencies"],
        json!(["module:base"])
    );
    assert_eq!(browser["modules"]["module:base"]["dependencies"], json!([]));
    for content in ["content:one", "content:two"] {
        let preloads = model["pages"][content]["presentation"]["browser_modules"]["preloads"]
            .as_array()
            .expect("exact page closure");
        assert_eq!(preloads.len(), 3);
        assert_eq!(
            preloads[0]["url"],
            "https://one.example/mount/assets/base.js"
        );
        assert_eq!(
            preloads[1]["url"],
            "https://one.example/mount/assets/shared.js"
        );
        assert_eq!(
            preloads[2]["url"],
            "https://one.example/mount/assets/shared-state.js"
        );
    }
}

#[test]
fn one_site_cannot_publish_one_alias_for_two_module_identities() {
    let first = RESOURCES.replace(
        "entry='main.ts'",
        "entry='main.ts',imports={'@publication/state'='module:state'}",
    );
    let second = "[[resources]]\ninputs=['second.ts']\nbuild={kind='browser-module',entry='second.ts',imports={'@publication/state'='module:other'}}\noutput='/assets/second-entry.js'\n";
    let manifest = format!(
        "{MODULE_MANIFEST}[resources.'module:other']\ninputs=['other.ts']\nbuild={{kind='browser-module',entry='other.ts'}}\n"
    );
    let fixture = Fixture::new(
        false,
        &[
            ("content/one.md", page("one", "/one/", &first).as_bytes()),
            ("content/two.md", page("two", "/two/", second).as_bytes()),
            ("content/main.ts", FIRST.as_bytes()),
            ("content/second.ts", SECOND.as_bytes()),
            ("project/plugin.toml", manifest.as_bytes()),
            ("project/state.ts", SHARED.as_bytes()),
            ("project/increment.ts", b"export const step: number = 1;"),
            ("project/other.ts", b"export const other = true;"),
        ],
    );
    let selections = format!(
        "{MODULE_SELECTION}'module:other'={{state='enabled',resource={{owner={{kind='plugin',package='shared-modules'}},resource='module:other'}},output='/assets/other.js',use='dependency'}}\n"
    );
    failure(&configuration(&fixture, &selections), "resource_input");
}
