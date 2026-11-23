//! Real retained publication bytes through the SDK's bounded loopback transport.

use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::Path,
    process::Command,
    sync::{Mutex, MutexGuard},
    time::{Duration, Instant},
};

use gazette_sdk::{
    build,
    preview::{self, Options, Poll, PreviewReservation, PreviewSession},
    source::{Content, Sha256Digest},
    workspace::Key,
};
use tempfile::{TempDir, tempdir};

use gazette_test_support as native;

#[path = "preview/watch.rs"]
mod watch;

#[path = "preview/math.rs"]
mod math;

#[path = "preview/session.rs"]
mod session;

#[path = "preview/delivery.rs"]
mod delivery;

#[path = "preview/resource_cache.rs"]
mod resource_cache;

#[path = "preview/deployment.rs"]
mod deployment;

const SITES: [&str; 2] = ["site:one", "site:two"];
const TEMPLATE: &str = "<h1>{{ content.title }}</h1>{{ body_html | safe }}<a href=\"{{ route.canonical_url }}\">canonical</a>";

// Close/rebind assertions share the OS ephemeral-port namespace. Keep unrelated
// fixtures from taking a just-released port; sessions and clients within each
// fixture still run concurrently, including the explicit ownership tests.
static PREVIEW_PORTS: Mutex<()> = Mutex::new(());

struct Fixture {
    directory: TempDir,
    config: String,
    _ports: MutexGuard<'static, ()>,
}

impl Fixture {
    fn new() -> Self {
        Self::with_route("/", None)
    }

    fn with_route(route: &str, alias: Option<&str>) -> Self {
        Self::configured(route, alias, false)
    }

    fn linked() -> Self {
        Self::configured("/", None, true)
    }

    fn configured(route: &str, alias: Option<&str>, linked: bool) -> Self {
        let ports = PREVIEW_PORTS
            .lock()
            .expect("exclusive preview fixture ports");
        let directory = tempdir().expect("disposable native publication sources");
        let root = directory.path();
        fs::create_dir_all(root.join("theme/templates")).expect("Theme directory");
        fs::write(
            root.join("theme/theme.toml"),
            if alias.is_some() {
                "package='theme:one'\nnamespace='one'\ntemplates='templates'\nbrowser_theme_colors={light='#ffffff',dark='#000000'}\nrender={page='page.html',alias='alias.html'}\n"
            } else {
                "package='theme:one'\nnamespace='one'\ntemplates='templates'\nbrowser_theme_colors={light='#ffffff',dark='#000000'}\nrender={page='page.html'}\n"
            },
        )
        .expect("Theme manifest");
        let template = if linked {
            format!(
                "{TEMPLATE}{{% for item in related %}}<i data-related=\"{{{{ item.id }}}}\">{{{{ item.site }}}}</i>{{% endfor %}}"
            )
        } else {
            TEMPLATE.to_owned()
        };
        fs::write(root.join("theme/templates/page.html"), template).expect("real Tera template");
        if alias.is_some() {
            fs::write(
                root.join("theme/templates/alias.html"),
                "<a href=\"{{ route.canonical_url }}\">Moved</a>",
            )
            .expect("real alias template");
        }
        let aliases = alias.map_or_else(String::new, |alias| format!(",aliases=[{alias:?}]"));
        for name in ["one", "two"] {
            fs::create_dir(root.join(name)).expect("authored root");
            let foreign = if name == "one" { "two" } else { "one" };
            let related = if linked {
                format!("related=['content:{foreign}']\n")
            } else {
                String::new()
            };
            let crossing = if linked {
                format!("\n[[site:{foreign}.example.invalid/|Related {foreign}]]\n")
            } else {
                String::new()
            };
            fs::write(root.join(name).join("page.md"), format!("+++\n{related}[content]\nid='content:{name}'\nsite='site:{name}'\ntitle='{name}'\ndescription='Retained {name} preview.'\nlifecycle='draft'\nroute={{path={route:?}{aliases}}}\nstructure={{kind='page',family={{kind='article'}}}}\n+++\n# Retained {name}\n{crossing}")).expect("configured authored document");
        }
        fs::create_dir_all(root.join("math/vendor/katex-0.18.4")).expect("math resource directory");
        fs::write(
            root.join("math/render.mjs"),
            include_bytes!("../../../adapters/math/render.mjs"),
        )
        .expect("retained math renderer");
        fs::write(
            root.join("math/vendor/katex-0.18.4/katex.mjs"),
            include_bytes!("../../../adapters/math/vendor/katex-0.18.4/katex.mjs"),
        )
        .expect("retained math import");
        fs::write(
            root.join("math/wrapper.mjs"),
            "import { appendFileSync } from 'node:fs'; const counter = process.argv.pop(); appendFileSync(counter, 'attempt\\n'); await import('./render.mjs');\n",
        )
        .expect("retained wrapper");
        fs::write(
            root.join("math/fail.mjs"),
            "import { appendFileSync } from 'node:fs'; appendFileSync(process.argv[2], 'attempt\\n'); process.stdout.write('native-failure\\n'); process.exitCode = 7;\n",
        )
        .expect("retained failing wrapper");
        if linked {
            fs::write(root.join("one/collection.md"), "+++\n[content]\nid='collection:one'\nsite='site:one'\ntitle='Local inquiry'\nlifecycle='archived'\nroute={path='/collections/local/'}\nstructure={kind='collection'}\n+++\n# Local inquiry\n").expect("native containing collection");
            fs::write(root.join("one/journey.md"), "+++\n[content]\nid='journey:shared'\nsite='site:one'\ntitle='Local journey'\nlifecycle='archived'\nroute={path='/collections/local/journey/'}\nstructure={kind='journey',collection='collection:one',members=[{content='content:one',primary=true}]}\n+++\n# Local journey\n").expect("same-site sole editorial residence");
        }
        git(root, &["init", "--quiet"]);
        git(root, &["add", "theme", "one", "two", "math"]);
        git(
            root,
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--quiet",
                "-m",
                "preview fixture",
            ],
        );
        let commit = git(root, &["rev-parse", "HEAD"]);
        let mut config = String::from(
            "[composition]\ntheme={package='theme:one'}\n[[principals]]\nid='owner:one'\nkind='person'\n",
        );
        for site in SITES {
            config.push_str(&format!(
                "[[sites]]\nid='{site}'\npublisher='owner:one'\ndeployable=false\n"
            ));
        }
        config.push_str("[[brands]]\nid='brand:one'\nname='Preview fixture'\n");
        for name in ["one", "two"] {
            config.push_str(&format!("[[brands.publications]]\nsite='site:{name}'\npublic_base_url='https://{name}.example.invalid/pr%C3%A9fix/'\nauthored=[{{source='source:{name}',target='{name}'}}]\n"));
            if linked {
                config.push_str(&format!("machine={{search='lexical',sitemap=true,json_ld=true,llms={{enabled=true,description_from='content:{name}',representation='markdown-alternate',include_human_link=true,sections=[{{title='Start here',contents=['content:{name}']}}]}},markdown_alternates={{enabled=true,directory_page='index.md',file_page='append-md',media_type='text/markdown',discovery_rel='alternate'}}}}\n"));
            }
        }
        for name in ["theme", "one", "two"] {
            config.push_str(&format!("[[sources]]\nid='source:{name}'\nlocation={{kind='repository',root={:?},commit='{}',subdirectory='{name}'}}\n", root.to_str().expect("UTF-8 disposable root"), commit.trim()));
        }
        config.push_str(
            "[[theme_packages]]\npackage='theme:one'\nsource='source:theme'\nmanifest='theme.toml'\n",
        );
        fs::write(root.join("gazette.toml"), &config).expect("explicit workspace document");
        Self {
            directory,
            config,
            _ports: ports,
        }
    }

    fn acquire(&self, sites: &[&str]) -> build::AcquiredPublicationSet {
        build::acquire_publications(
            &self.config,
            &sites.iter().map(|site| key(site)).collect::<Vec<_>>(),
            Default::default(),
            Default::default(),
        )
        .expect("real exact repository capture")
    }

    fn acquire_local(&self, sites: &[&str]) -> build::AcquiredPublicationSet {
        let workspace = "gazette.toml".parse().expect("workspace path");
        let configuration = gazette_sdk::configuration::load_workspace(
            self.directory.path(),
            &workspace,
            Default::default(),
        )
        .expect("strict modular workspace")
        .into_resolved();
        gazette_application::build::acquire_resolved_publications(
            &gazette_adapter_fs::FileSystem,
            &gazette_adapter_git::Git,
            configuration,
            &sites.iter().map(|site| key(site)).collect::<Vec<_>>(),
            Default::default(),
        )
        .expect("real exact repository capture")
    }
}

fn git(root: &Path, arguments: &[&str]) -> String {
    let output = Command::new(native::git())
        .args(arguments)
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_DATE", "2000-01-01T00:00:00Z")
        .env("GIT_COMMITTER_DATE", "2000-01-01T00:00:00Z")
        .output()
        .expect("disposable native Git operation");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("Git text output")
}

fn key(value: &str) -> Key {
    value.parse().expect("explicit site key")
}

fn render(
    inputs: &build::AcquiredPublicationSet,
    fail_second: bool,
) -> build::RenderedPublicationSet<'_> {
    render_using(inputs, fail_second, None)
}

fn render_using<'a>(
    inputs: &'a build::AcquiredPublicationSet,
    fail_second: bool,
    reservation: Option<&preview::Reservation>,
) -> build::RenderedPublicationSet<'a> {
    let model = "model.json".parse().expect("machine output");
    let collision = "index.html".parse().expect("colliding machine output");
    let options = build::PublicationSetRenderOptions {
        derive_selection: false,
        preview_link_bases: None,
        selection: None,
        review_responses: None,
        preview_urls: None,
        sites: inputs
            .sites()
            .keys()
            .map(|site| {
                (
                    site.clone(),
                    build::SiteRenderOptions {
                        render_year: None,
                        graph: None,
                        model_output: Some(if fail_second && site == &key("site:two") {
                            &collision
                        } else {
                            &model
                        }),
                        limits: Default::default(),
                    },
                )
            })
            .collect(),
    };
    match reservation {
        Some(reservation) => preview::render(inputs, reservation, options, Default::default()),
        None => build::render_publications(inputs, options, Default::default()),
    }
    .expect("shared compiler phase")
}

fn addresses(sites: &[&str]) -> BTreeMap<Key, SocketAddr> {
    sites
        .iter()
        .map(|site| (key(site), "127.0.0.1:0".parse().expect("explicit loopback")))
        .collect()
}

fn permission() -> Options {
    Options {
        allow_listeners: true,
        ..Default::default()
    }
}

fn get(session: &mut impl PreviewSession, site: &str, path: &str) -> Vec<u8> {
    let response = response(session, site, path);
    assert_eq!(response.status, 200, "exact retained route: {path}");
    response.body
}

struct HttpResponse {
    status: u16,
    content_type: String,
    body: Vec<u8>,
}

fn response(session: &mut impl PreviewSession, site: &str, path: &str) -> HttpResponse {
    let address = session.addresses()[&key(site)];
    let mut client =
        TcpStream::connect_timeout(&address, Duration::from_secs(2)).expect("real loopback client");
    client
        .set_read_timeout(Some(Duration::from_secs(3)))
        .expect("bounded response read");
    client
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .expect("native HTTP request");
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match session.poll().expect("bounded native poll") {
            Poll::Responded => break,
            Poll::Idle => {
                assert!(
                    Instant::now() < deadline,
                    "queued native connection not serviced"
                );
                std::thread::yield_now();
            }
            Poll::Disconnected => panic!("complete request lost its response"),
        }
    }
    let mut response = Vec::new();
    client
        .read_to_end(&mut response)
        .expect("complete native response");
    let split = response
        .windows(4)
        .position(|bytes| bytes == b"\r\n\r\n")
        .expect("HTTP header boundary");
    let header = std::str::from_utf8(&response[..split]).expect("HTTP header text");
    assert!(header.starts_with("HTTP/1.1 "), "{header}");
    let status = header
        .split_whitespace()
        .nth(1)
        .expect("HTTP status")
        .parse()
        .expect("numeric status");
    let body = response[split + 4..].to_vec();
    let length = header
        .lines()
        .find_map(|line| {
            line.split_once(':')
                .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                .map(|(_, value)| value.trim().parse::<usize>().expect("response length"))
        })
        .expect("bounded exact response length");
    assert_eq!(length, body.len());
    let content_type = header
        .lines()
        .find_map(|line| {
            line.split_once(':')
                .filter(|(name, _)| name.eq_ignore_ascii_case("content-type"))
                .map(|(_, value)| value.trim().to_owned())
        })
        .expect("native representation header");
    HttpResponse {
        status,
        content_type,
        body,
    }
}

#[test]
fn exact_single_and_multi_site_preview_serves_retained_human_and_machine_bytes() {
    for selected in [&SITES[..1], &SITES[..]] {
        let fixture = Fixture::new();
        let inputs = fixture.acquire(selected);
        fs::write(
            fixture.directory.path().join("one/page.md"),
            "changed after capture",
        )
        .expect("native source change");
        fs::remove_file(fixture.directory.path().join("two/page.md"))
            .expect("native source removal");
        fs::remove_file(fixture.directory.path().join("theme/templates/page.html"))
            .expect("native Theme removal");
        let rendered = render(&inputs, false);
        assert!(rendered.is_complete());
        let evidence = serde_json::to_value(rendered.inspect()).expect("retained private evidence");
        let mut session = preview::bind(&rendered, addresses(selected), permission())
            .expect("complete loopback set");
        assert_eq!(
            session.addresses().keys().cloned().collect::<Vec<_>>(),
            selected.iter().map(|site| key(site)).collect::<Vec<_>>()
        );
        for site in selected {
            let tree = rendered.sites()[&key(site)]
                .as_ref()
                .expect("complete site")
                .site()
                .output();
            for (request, output) in [
                ("/pr%C3%A9fix/", "index.html"),
                ("/pr%C3%A9fix/model.json", "model.json"),
            ] {
                assert_eq!(
                    get(&mut session, site, request),
                    tree.file(&output.parse().expect("output path"))
                        .expect("retained exact file")
                );
            }
            for outside in ["/", "/model.json", "/pr%C3%A9fix-other/model.json"] {
                assert_eq!(
                    response(&mut session, site, outside).status,
                    404,
                    "no flattening or same-prefix namesake: {outside}"
                );
            }
            let model: serde_json::Value = serde_json::from_slice(
                tree.file(&"model.json".parse().expect("model path"))
                    .expect("model bytes"),
            )
            .expect("real compiled model");
            assert_eq!(model["site"], *site);
            assert!(
                std::str::from_utf8(
                    tree.file(&"index.html".parse().expect("HTML path"))
                        .expect("HTML")
                )
                .expect("UTF-8 HTML")
                .contains("example.invalid"),
                "canonical URLs are not rewritten to loopback"
            );
        }
        session.close();
        assert_eq!(
            serde_json::to_value(rendered.inspect()).expect("same evidence"),
            evidence
        );
    }
}

#[test]
fn producer_media_types_override_extensionless_and_misleading_generated_paths() {
    use gazette_sdk::render::MediaType;

    for (page_route, alias_route, model_name) in [
        ("/article", "/legacy.css", "model.html"),
        ("/article.json", "/legacy", "model"),
    ] {
        let fixture = Fixture::with_route(page_route, Some(alias_route));
        let inputs = fixture.acquire(&SITES[..1]);
        let model = model_name.parse().expect("explicit model output");
        let rendered = build::render_publications(
            &inputs,
            build::PublicationSetRenderOptions {
                derive_selection: false,
                preview_link_bases: None,
                selection: None,
                review_responses: None,
                preview_urls: None,
                sites: BTreeMap::from([(
                    key("site:one"),
                    build::SiteRenderOptions {
                        render_year: None,
                        graph: None,
                        model_output: Some(&model),
                        limits: Default::default(),
                    },
                )]),
            },
            Default::default(),
        )
        .expect("same real publication compiler");
        assert!(rendered.is_complete());
        let site = rendered.sites()[&key("site:one")]
            .as_ref()
            .expect("rendered selected site")
            .site();
        let before = site.output().inventory().identity();
        let mut session = preview::bind(&rendered, addresses(&SITES[..1]), permission())
            .expect("native preview of producer-declared artifacts");
        for (path, declared, content_type) in [
            (
                page_route.trim_start_matches('/'),
                MediaType::Html,
                "text/html; charset=utf-8",
            ),
            (
                alias_route.trim_start_matches('/'),
                MediaType::Html,
                "text/html; charset=utf-8",
            ),
            (model_name, MediaType::Json, "application/json"),
        ] {
            let entry = path.parse().expect("exact emitted entry");
            assert_eq!(
                site.media_types().get(&entry),
                Some(&declared),
                "the real producer, not the test or transport, assigns representation"
            );
            let observed = response(&mut session, "site:one", &format!("/pr%C3%A9fix/{path}"));
            assert_eq!(observed.status, 200);
            assert_eq!(
                observed.content_type, content_type,
                "filename must not override producer meaning for {path}"
            );
            assert_eq!(
                observed.body,
                site.output().file(&entry).expect("real generated payload")
            );
        }
        let machine: serde_json::Value = serde_json::from_slice(
            site.output()
                .file(&model)
                .expect("configured machine bytes"),
        )
        .expect("generated model remains JSON regardless of name");
        assert_eq!(machine["site"], "site:one");
        session.close();
        assert_eq!(site.output().inventory().identity(), before);
    }
}

#[test]
fn incomplete_or_mismatched_sets_and_unacknowledged_listeners_never_become_preview() {
    let fixture = Fixture::new();
    let inputs = fixture.acquire(&SITES);
    let rendered = render(&inputs, false);
    for bindings in [
        BTreeMap::new(),
        addresses(&SITES[..1]),
        addresses(&["site:one", "site:unknown"]),
    ] {
        assert!(matches!(
            preview::bind(&rendered, bindings, permission()),
            Err(preview::Error::Refused(_))
        ));
    }
    assert!(matches!(
        preview::bind(&rendered, addresses(&SITES), Options::default()),
        Err(preview::Error::Refused(_))
    ));
    let partial = render(&inputs, true);
    assert!(!partial.is_complete());
    assert!(partial.sites()[&key("site:one")].is_ok());
    assert!(partial.sites()[&key("site:two")].is_err());
    assert!(matches!(
        preview::bind(&partial, addresses(&SITES), permission()),
        Err(preview::Error::Refused(_))
    ));
    let mut session = preview::bind(&rendered, addresses(&SITES), permission())
        .expect("prior refusals preserve reusable complete output");
    assert!(!get(&mut session, "site:two", "/pr%C3%A9fix/model.json").is_empty());
    session.close();
}

#[test]
fn independent_sessions_release_only_their_own_sockets_on_close_and_drop() {
    let fixture = Fixture::new();
    let inputs = fixture.acquire(&SITES);
    let rendered = render(&inputs, false);
    let first =
        preview::bind(&rendered, addresses(&SITES), permission()).expect("first native session");
    let first_addresses = first.addresses().clone();
    let mut second = preview::bind(&rendered, addresses(&SITES), permission())
        .expect("independent native session");
    for address in second.addresses().values() {
        assert!(!first_addresses.values().any(|first| first == address));
    }
    drop(first);
    let rebound = preview::bind(&rendered, first_addresses, permission())
        .expect("drop releases exact listeners");
    assert!(!get(&mut second, "site:one", "/pr%C3%A9fix/").is_empty());
    let released = rebound.addresses().clone();
    rebound.close();
    for address in released.values() {
        drop(TcpListener::bind(address).expect("close releases exact socket"));
    }
    assert!(!get(&mut second, "site:two", "/pr%C3%A9fix/").is_empty());
    second.close();
}

#[test]
fn late_native_bind_failure_does_not_leave_a_successful_subset_listening() {
    let fixture = Fixture::new();
    let inputs = fixture.acquire(&SITES);
    let rendered = render(&inputs, false);
    let first = TcpListener::bind("127.0.0.1:0").expect("reserve first disposable address");
    let first_address = first.local_addr().expect("first exact address");
    let occupied = TcpListener::bind("127.0.0.1:0").expect("occupied second disposable address");
    let second_address = occupied.local_addr().expect("second exact address");
    drop(first);
    let selected = BTreeMap::from([
        (key("site:one"), first_address),
        (key("site:two"), second_address),
    ]);
    assert!(matches!(
        preview::bind(&rendered, selected, permission()),
        Err(preview::Error::Io(_))
    ));
    drop(
        TcpListener::bind(first_address)
            .expect("failed complete startup released its first listener"),
    );
    assert_eq!(
        occupied
            .local_addr()
            .expect("unrelated owner remains alive"),
        second_address
    );
}

#[test]
fn reserved_local_urls_drive_cross_site_html_and_machine_outputs_without_changing_production() {
    let fixture = Fixture::linked();
    let inputs = fixture.acquire(&SITES);
    let production = render(&inputs, false);
    assert!(production.is_complete());
    assert!(production.preview_urls().is_none());
    let reservation = preview::reserve(&inputs, addresses(&SITES), permission())
        .expect("reserve before local compilation");
    let held = reservation.addresses().clone();
    let urls = reservation.urls().clone();
    for address in held.values() {
        assert!(
            TcpListener::bind(address).is_err(),
            "ports held before rendering"
        );
    }
    let local = render_using(&inputs, false, Some(&reservation));
    assert!(local.is_complete());
    assert_eq!(local.preview_urls(), Some(&urls));
    assert_eq!(
        serde_json::to_value(local.inspect()).expect("private set evidence")["preview_urls"],
        serde_json::to_value(&urls).expect("exact URL intent")
    );
    for address in held.values() {
        assert!(
            TcpListener::bind(address).is_err(),
            "rendering does not release or replace listeners"
        );
    }
    for (site, name, foreign, foreign_name) in [
        ("site:one", "one", "site:two", "two"),
        ("site:two", "two", "site:one", "one"),
    ] {
        let publication = local.sites()[&key(site)]
            .as_ref()
            .expect("complete local site");
        assert_eq!(publication.preview_urls(), Some(&urls));
        assert!(
            std::ptr::eq(
                publication.preview_urls().expect("site map"),
                local.preview_urls().expect("set map")
            ),
            "one retained map shared across set and site"
        );
        let tree = publication.site().output();
        let model: serde_json::Value = serde_json::from_slice(
            tree.file(&"model.json".parse().expect("model path"))
                .expect("model bytes"),
        )
        .expect("real machine model");
        let page = &model["pages"][format!("content:{name}")];
        assert_eq!(page["route"]["canonical_url"], urls[&key(site)].as_str());
        let public_url = format!("https://{name}.example.invalid/pr%C3%A9fix/");
        assert_eq!(page["route"]["public_url"], public_url);
        assert_eq!(page["json_ld"]["@id"], format!("{public_url}#article"));
        assert_eq!(page["json_ld"]["@type"], "Article");
        assert_eq!(page["json_ld"]["url"], urls[&key(site)].as_str());
        assert_eq!(page["json_ld"]["mainEntityOfPage"]["@id"], public_url);
        assert_eq!(page["related"][0]["id"], format!("content:{foreign_name}"));
        assert_eq!(page["related"][0]["site"], foreign);
        assert_eq!(
            page["links"][0]["destination"],
            urls[&key(foreign)].as_str()
        );
        assert_eq!(
            page["links"][0]["content"],
            format!("content:{foreign_name}")
        );
        assert_eq!(page["links"][0]["site"], foreign);
        assert_eq!(
            page["links"][0]["relationships"],
            serde_json::json!(["related"])
        );
        if name == "one" {
            assert_eq!(page["journeys"][0]["journey"]["id"], "journey:shared");
            assert_eq!(page["journeys"][0]["journey"]["site"], site);
            assert_eq!(page["journeys"][0]["primary"], true);
            assert_eq!(page["journeys"][0]["total"], 1);
            assert!(page["journeys"][0]["previous"].is_null());
            assert!(page["journeys"][0]["next"].is_null());
        } else {
            assert_eq!(page["journeys"], serde_json::json!([]));
            assert_eq!(page["collections"], serde_json::json!([]));
        }
        let html = std::str::from_utf8(
            tree.file(&"index.html".parse().expect("HTML path"))
                .expect("HTML bytes"),
        )
        .expect("HTML UTF-8");
        assert!(
            html.contains(&format!("href=\"{}\"", urls[&key(foreign)].as_str())),
            "real Tera cross-site navigation uses the reserved foreign socket"
        );
        for path in ["search.json", "site.jsonld", "sitemap.xml", "llms.txt"] {
            let bytes = tree
                .file(&path.parse().expect("machine path"))
                .expect("enabled real machine output");
            let text = std::str::from_utf8(bytes).expect("UTF-8 machine output");
            assert!(
                text.contains(urls[&key(site)].as_str()),
                "{site}: {path} must use the same local route model"
            );
            match path {
                "site.jsonld" => {
                    let graph: serde_json::Value = serde_json::from_slice(bytes).unwrap();
                    let article = graph["@graph"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|entry| entry["identifier"] == format!("content:{name}"))
                        .expect("local article in the public graph");
                    assert_eq!(article["@id"], format!("{public_url}#article"));
                    assert_eq!(article["url"], urls[&key(site)].as_str());
                }
                "search.json" => {
                    let search: serde_json::Value = serde_json::from_slice(bytes).unwrap();
                    let record = search
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|entry| entry["content"]["id"] == format!("content:{name}"))
                        .expect("local article in search");
                    assert_eq!(record["route"]["url"], urls[&key(site)].as_str());
                    assert_eq!(record["route"]["canonical_url"], urls[&key(site)].as_str());
                    assert_eq!(record["route"]["public_url"], public_url);
                }
                _ => assert!(
                    !text.contains("example.invalid"),
                    "{path} navigation must use local destinations"
                ),
            }
        }
    }
    let mut session = preview::attach(reservation, &local).expect("attach original sockets");
    assert_eq!(session.addresses(), &held);
    for site in SITES {
        assert_eq!(
            get(&mut session, site, "/pr%C3%A9fix/"),
            local.sites()[&key(site)]
                .as_ref()
                .expect("site")
                .site()
                .output()
                .file(&"index.html".parse().expect("entry"))
                .expect("retained bytes")
        );
    }
    session.close();
    let rebuilt = render(&inputs, false);
    assert!(rebuilt.is_complete());
    assert!(rebuilt.preview_urls().is_none());
    for site in SITES {
        let expected = production.sites()[&key(site)]
            .as_ref()
            .expect("production site")
            .site()
            .output();
        let actual = rebuilt.sites()[&key(site)]
            .as_ref()
            .expect("rebuilt production site")
            .site()
            .output();
        assert_eq!(
            actual.inventory().identity(),
            expected.inventory().identity()
        );
        for entry in expected.inventory().entries() {
            if matches!(entry.content, Content::File { .. }) {
                assert_eq!(actual.file(&entry.path), expected.file(&entry.path));
            }
        }
    }
    assert_eq!(
        serde_json::to_value(rebuilt.inspect()).expect("rebuilt evidence"),
        serde_json::to_value(production.inspect()).expect("original evidence")
    );
}

#[test]
fn wrong_reservation_incomplete_output_and_injected_maps_never_attach() {
    let fixture = Fixture::new();
    let inputs = fixture.acquire(&SITES);
    assert!(matches!(
        preview::reserve(&inputs, addresses(&SITES[..1]), permission()),
        Err(preview::Error::Refused(_))
    ));
    let original =
        preview::reserve(&inputs, addresses(&SITES), permission()).expect("original reservation");
    let local = render_using(&inputs, false, Some(&original));
    let wrong = preview::reserve(&inputs, addresses(&SITES), permission())
        .expect("independent live reservation");
    let wrong_addresses = wrong.addresses().clone();
    assert_ne!(wrong.urls(), original.urls());
    assert!(matches!(
        preview::attach(wrong, &local),
        Err(preview::Error::Refused(_))
    ));
    for address in wrong_addresses.values() {
        TcpListener::bind(address).expect("failed wrong attachment releases every wrong socket");
    }
    for address in original.addresses().values() {
        assert!(
            TcpListener::bind(address).is_err(),
            "unrelated original reservation remains owned"
        );
    }
    assert!(
        matches!(
            preview::bind(&local, addresses(&SITES), permission()),
            Err(preview::Error::Refused(_))
        ),
        "local output cannot silently acquire replacement listeners"
    );
    let injected = build::PublicationSetRenderOptions {
        derive_selection: false,
        preview_link_bases: None,
        selection: None,
        review_responses: None,
        preview_urls: Some(original.urls().clone()),
        sites: BTreeMap::new(),
    };
    assert!(matches!(
        preview::render(&inputs, &original, injected, Default::default()),
        Err(build::PublicationSetError::Selection(
            "preview URLs come only from the held reservation"
        ))
    ));
    let model = "model.json".parse().expect("model path");
    for foreign_base in [None, Some("https://external.example.invalid/pr%C3%A9fix/")] {
        let mut invalid_urls = original.urls().clone();
        if let Some(base) = foreign_base {
            invalid_urls.insert(
                key("site:two"),
                base.parse()
                    .expect("well-formed but prohibited preview base"),
            );
        } else {
            invalid_urls.remove(&key("site:two"));
        }
        let options = build::PublicationSetRenderOptions {
            derive_selection: false,
            preview_link_bases: None,
            selection: None,
            review_responses: None,
            preview_urls: Some(invalid_urls),
            sites: SITES
                .into_iter()
                .map(|site| {
                    (
                        key(site),
                        build::SiteRenderOptions {
                            render_year: None,
                            graph: None,
                            model_output: Some(&model),
                            limits: Default::default(),
                        },
                    )
                })
                .collect(),
        };
        let failure = build::render_publications(&inputs, options, Default::default())
            .expect_err("no incomplete or nonloopback shared preview model");
        assert_eq!(
            failure.code(),
            if foreign_base.is_some() {
                "publication_preview_urls"
            } else {
                "publication_set_selection"
            }
        );
    }
    drop(original);
    let reservation =
        preview::reserve(&inputs, addresses(&SITES), permission()).expect("new exact set");
    let released = reservation.addresses().clone();
    let partial = render_using(&inputs, true, Some(&reservation));
    assert!(!partial.is_complete());
    assert!(partial.sites()[&key("site:one")].is_ok());
    assert!(partial.sites()[&key("site:two")].is_err());
    assert_eq!(partial.preview_urls(), Some(reservation.urls()));
    assert!(matches!(
        preview::attach(reservation, &partial),
        Err(preview::Error::Refused(_))
    ));
    for address in released.values() {
        TcpListener::bind(address).expect("partial-render refusal releases entire reservation");
    }
}

#[test]
fn preview_preparation_is_refused_before_permissions_or_native_paths_are_used() {
    let fixture = Fixture::new();
    let inputs = fixture.acquire(&SITES);
    let reservation =
        preview::reserve(&inputs, addresses(&SITES), permission()).expect("local reservation");
    let local = render_using(&inputs, false, Some(&reservation));
    assert!(local.is_complete());
    let journal = fixture.directory.path().join("absent-journal");
    let parent = fixture.directory.path().join("absent-stage-parent");
    let request = || build::PreparationRequest {
        journal: &journal,
        parent: &parent,
        name: "preview-stage",
        id: Sha256Digest::of(b"preview-refusal-only"),
        expected: gazette_sdk::store::JournalHeader {
            schema_version: 1,
            journal_id: [7; 32],
            journal_epoch: 1,
            writer_fence: 1,
        },
    };
    let failed = build::prepare_stage(
        local.sites()[&key("site:one")]
            .as_ref()
            .expect("local site"),
        request(),
        Default::default(),
    )
    .expect_err("preview refuses even before default missing permission");
    assert!(matches!(
        failed.cause.as_ref(),
        build::PreparationFailure::Preview
    ));
    assert!(!failed.writer_acquired);
    assert!(failed.journal.is_none() && failed.receipt.is_none() && failed.stage.is_none());
    assert!(!journal.exists() && !parent.exists());
    let production = render(&inputs, false);
    let ordinary = build::prepare_stage(
        production.sites()[&key("site:one")]
            .as_ref()
            .expect("production site"),
        request(),
        Default::default(),
    )
    .expect_err("same request without preview requires explicit candidate selection first");
    assert!(matches!(
        ordinary.cause.as_ref(),
        build::PreparationFailure::Selection
    ));
    assert!(!ordinary.writer_acquired);
    assert!(ordinary.journal.is_none() && ordinary.receipt.is_none() && ordinary.stage.is_none());
    assert!(!journal.exists() && !parent.exists());
}
