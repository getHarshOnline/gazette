//! Real CLI lifecycle serves either explicit preview profile without a coordinator.

use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    process::{Child, ChildStdout, Command, Stdio},
    time::Duration,
};

use super::*;

#[path = "preview/watch.rs"]
mod watch;

const ARGUMENTS: [&str; 7] = [
    "preview-sites",
    "--local",
    "--allow-listeners",
    "--listen",
    "site:one=127.0.0.1:0",
    "--listen",
    "site:other=127.0.0.1:0",
];

#[test]
fn documentation_link_map_flag_changes_links_without_a_reader_script_or_request_edit() {
    use gazette_sdk::html::{self, TextKind};
    use std::convert::Infallible;
    let page = format!(
        "{}\n[Developer types](https://docs.example.test/types?view=api#options)\n\n`https://docs.example.test/types?view=api#options`\n",
        super::super::PAGE
    );
    let fixture = Fixture::new(&[("one/page.md", page.as_bytes())]);
    let original = request(&fixture.config(), &SITES);
    let mut process = Process::with_arguments(
        &original,
        &[
            "--local-urls",
            "--preview-link-bases-json",
            r#"{"https://docs.example.test/":"http://127.0.0.1:3909/"}"#,
        ],
    );
    let ready = process.event();
    let served = response(
        address(&ready, "site:one"),
        "GET",
        "/mount/guide/index.html",
    );
    let mut attributes = Vec::new();
    html::inspect(body(&served), html::Limits::default(), |kind, value| {
        if kind == TextKind::Attribute {
            attributes.push(value.to_owned());
        }
        Ok::<_, Infallible>(())
    })
    .unwrap();
    assert!(
        attributes
            .iter()
            .any(|value| value == "http://127.0.0.1:3909/types?view=api#options")
    );
    assert!(
        String::from_utf8_lossy(body(&served))
            .contains("https://docs.example.test/types?view=api#options")
    );
    assert!(original.get("preview_link_bases").is_none());
    process.close();
}

#[test]
fn documentation_link_map_flag_refuses_competing_or_invalid_destinations() {
    let fixture = Fixture::new(&[]);
    let map = r#"{"https://docs.example.test/":"http://127.0.0.1:3909/"}"#;
    let mut original = request(&fixture.config(), &SITES);
    original["preview_link_bases"] = serde_json::from_str(map).unwrap();
    for (input, selected) in [
        (original, map),
        (
            request(&fixture.config(), &SITES),
            r#"{"https://docs.example.test/":"https://public.example.test/"}"#,
        ),
        (
            request(&fixture.config(), &SITES),
            r#"{"https://docs.example.test/":"http://127.0.0.1:3909/","https://docs.example.test/":"http://127.0.0.1:3910/"}"#,
        ),
    ] {
        let mut args = ARGUMENTS.to_vec();
        args.extend([
            "--plan",
            "--local-urls",
            "--preview-link-bases-json",
            selected,
        ]);
        let failed = cli(&input.to_string(), &args);
        assert!(!failed.status.success());
        assert!(failed.stdout.is_empty());
        assert!(String::from_utf8_lossy(&failed.stderr).contains("preview_link_bases"));
    }
}

pub(crate) struct Process {
    child: Child,
    output: BufReader<ChildStdout>,
}

impl Process {
    fn start(request: &Value) -> Self {
        Self::with_arguments(request, &[])
    }

    pub(crate) fn with_arguments(request: &Value, extra: &[&str]) -> Self {
        let (child, output) = Self::spawn(request, extra);
        Self { child, output }
    }

    fn spawn(request: &Value, extra: &[&str]) -> (Child, BufReader<ChildStdout>) {
        let mut child = Command::new(env!("CARGO_BIN_EXE_gazette"))
            .args(ARGUMENTS)
            .args(extra)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("direct local preview process");
        let output = BufReader::new(child.stdout.take().expect("JSONL event pipe"));
        let input = child.stdin.as_mut().expect("initial JSONL and close input");
        serde_json::to_writer(&mut *input, request).expect("exact existing request schema");
        input.write_all(b"\n").expect("initial framing delimiter");
        input.flush().expect("deliver initial request");
        (child, output)
    }

    pub(crate) fn event(&mut self) -> Value {
        let mut line = String::new();
        let read = self
            .output
            .read_line(&mut line)
            .expect("bounded lifecycle event");
        if read == 0 {
            // Do not block on stderr while a child might still be live. End this
            // exact disposable process first, then collect its failure evidence.
            let _ = self.child.kill();
            let status = self.child.wait().expect("failed preview process exits");
            let mut error = String::new();
            self.child
                .stderr
                .take()
                .expect("preview diagnostic pipe")
                .read_to_string(&mut error)
                .expect("terminated preview diagnostics");
            panic!("preview ended before a lifecycle event ({status}): {error}");
        }
        serde_json::from_str(&line).expect("complete JSONL lifecycle object")
    }

    pub(crate) fn close(&mut self) {
        self.child
            .stdin
            .take()
            .expect("live close input")
            .write_all(b"x")
            .expect("one-byte close command");
        assert_eq!(self.event(), json!({"event":"closed"}));
        assert!(self.child.wait().expect("closed process exits").success());
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        // Only this disposable child is killed if an assertion interrupted close.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub(crate) fn address(ready: &Value, site: &str) -> SocketAddr {
    ready["sites"][site]["address"]
        .as_str()
        .expect("native listener address")
        .parse()
        .expect("valid exact socket")
}

fn response(address: SocketAddr, method: &str, path: &str) -> Vec<u8> {
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(3))
        .expect("ready loopback listener");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("bounded test response wait");
    stream
        .set_write_timeout(Some(Duration::from_secs(3)))
        .expect("bounded test request write");
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
    )
    .expect("actual HTTP request");
    let mut bytes = Vec::new();
    stream
        .read_to_end(&mut bytes)
        .expect("complete native HTTP response");
    bytes
}

fn body(response: &[u8]) -> &[u8] {
    let end = response
        .windows(4)
        .position(|value| value == b"\r\n\r\n")
        .expect("HTTP header terminator");
    assert!(
        response.starts_with(b"HTTP/1.1 200 "),
        "successful exact asset response"
    );
    &response[end + 4..]
}

#[test]
fn plan_only_discloses_both_preview_profiles_without_native_work() {
    let fixture = Fixture::new(&[]);
    let request = request(&fixture.config(), &SITES);
    let former_root = fixture
        .directory
        .path()
        .to_str()
        .expect("fixture path")
        .to_owned();
    drop(fixture);
    let held = [
        TcpListener::bind("127.0.0.1:0").expect("hold first exact endpoint"),
        TcpListener::bind("127.0.0.1:0").expect("hold second exact endpoint"),
    ];
    let bindings = [
        format!("site:one={}", held[0].local_addr().unwrap()),
        format!("site:other={}", held[1].local_addr().unwrap()),
    ];

    for (extra, status, input_mode, source_reads) in [
        (
            vec!["--local-urls"],
            "local-cross-site-noncertified",
            "pinned",
            "once",
        ),
        (
            vec!["--local-urls", "--watch"],
            "local-cross-site-noncertified",
            "dirty-content-addressed",
            "repeated",
        ),
        (
            vec![],
            "local-production-linked-noncertified",
            "pinned",
            "once",
        ),
    ] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_gazette"))
            .args([
                "preview-sites",
                "--local",
                "--allow-listeners",
                "--plan",
                "--listen",
                &bindings[0],
                "--listen",
                &bindings[1],
            ])
            .args(extra)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start no-effect planner");
        serde_json::to_writer(child.stdin.as_mut().unwrap(), &request).expect("write exact recipe");
        drop(child.stdin.take());
        let output = child.wait_with_output().expect("finish no-effect planner");
        assert!(
            output.status.success(),
            "planner failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let plan: Value = serde_json::from_slice(&output.stdout).expect("typed effect plan");
        assert_eq!(plan["status"], status);
        assert_eq!(plan["input_mode"], input_mode);
        assert_eq!(plan["sites"], json!(SITES));
        assert_eq!(plan["effects"]["source_reads"], source_reads);
        assert_eq!(plan["effects"]["derived_state_writes"], false);
        assert_eq!(plan["effects"]["output_writes"], false);
        assert_eq!(plan["effects"]["listeners"], true);
        assert_eq!(plan["effects"]["network"]["loopback"], true);
        assert_eq!(plan["effects"]["network"]["outbound"], false);
        assert_eq!(plan["effects"]["git"]["reads"], true);
        assert_eq!(plan["effects"]["git"]["mutation"], false);
        assert_eq!(plan["effects"]["release_authority_consumption"], false);
        assert_eq!(plan["effects"]["unconfined_process_execution"], false);
        assert_eq!(
            plan["requested_listeners"]["site:one"],
            held[0].local_addr().unwrap().to_string()
        );
        assert_eq!(
            plan["requested_listeners"]["site:other"],
            held[1].local_addr().unwrap().to_string()
        );
        assert!(
            !String::from_utf8_lossy(&output.stdout).contains(&former_root),
            "portable plan omits source host paths"
        );
    }
}

#[test]
fn production_preview_plan_is_fresh_pinned_and_has_no_release_or_provider_effect() {
    let fixture = Fixture::new(&[]);
    let request = request(&fixture.config(), &SITES);
    let held = [
        TcpListener::bind("127.0.0.1:0").expect("hold first exact endpoint"),
        TcpListener::bind("127.0.0.1:0").expect("hold second exact endpoint"),
    ];
    let first = format!("site:one={}", held[0].local_addr().unwrap());
    let second = format!("site:other={}", held[1].local_addr().unwrap());
    let mut child = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args([
            "production-preview",
            "--local",
            "--registry",
            "unconfigured",
            "--artifact",
            "fresh-render",
            "--allow-listeners",
            "--plan",
            "--listen",
            &first,
            "--listen",
            &second,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start production-preview planner");
    serde_json::to_writer(child.stdin.as_mut().unwrap(), &request).expect("exact recipe");
    drop(child.stdin.take());
    let output = child.wait_with_output().expect("planner exits");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let plan: Value = serde_json::from_slice(&output.stdout).expect("typed plan");
    assert_eq!(plan["status"], "production_linked_no_push_noncertified");
    assert_eq!(plan["registry"], "unconfigured");
    assert_eq!(plan["artifact"], "fresh_render");
    assert_eq!(plan["sites"], json!(SITES));
    assert_eq!(plan["effects"]["source_reads"], "once");
    assert_eq!(plan["effects"]["derived_state_writes"], false);
    assert_eq!(plan["effects"]["output_writes"], false);
    assert_eq!(plan["effects"]["network"]["loopback"], true);
    assert_eq!(plan["effects"]["network"]["outbound"], false);
    assert_eq!(plan["effects"]["git"]["mutation"], false);
    assert_eq!(plan["effects"]["release_authority_consumption"], false);
    assert_eq!(plan["effects"]["unconfined_process_execution"], false);
}

#[test]
fn complete_two_site_preview_serves_exact_sdk_bytes_and_keeps_production_links() {
    let fixture = Fixture::new(&[]);
    let config = fixture.config();
    let inputs = acquire_set(&config, &SITES);
    let page = "page.html".parse().expect("page template");
    let alias = "alias.html".parse().expect("alias template");
    let model = "model.json".parse().expect("model output");
    let mut sdk_options = options(&SITES, &page, &alias, &model);
    let year = gazette_sdk::authored::RenderYear::new(2040).expect("fixture year");
    for site in sdk_options.sites.values_mut() {
        site.render_year = Some(year);
    }
    let expected = build::render_publications(&inputs, sdk_options, Default::default())
        .expect("independent actual SDK render");
    let mut cli_request = request(&config, &SITES);
    cli_request["render_year"] = json!(year);
    let mut process = Process::start(&cli_request);
    let ready = process.event();
    assert_eq!(ready["event"], "ready");
    assert_eq!(ready["status"], "local-production-linked-noncertified");
    assert_eq!(ready["sites"].as_object().expect("exact site set").len(), 2);
    assert!(
        !ready
            .to_string()
            .contains(fixture.directory.path().to_str().expect("fixture path"))
    );
    // Startup already retained inputs. Serving must not reread this native source.
    fs::write(
        fixture.directory.path().join("one/page.md"),
        b"changed after readiness",
    )
    .expect("test-owned source drift");
    let addresses = SITES.map(|site| address(&ready, site));
    for (site, socket) in SITES.into_iter().zip(addresses) {
        let rendered = expected.sites()[&key(site)]
            .as_ref()
            .expect("complete expected site");
        let output = rendered.site().output();
        assert_eq!(
            ready["sites"][site]["output_identity"],
            serde_json::to_value(output.inventory().identity()).expect("exact identity projection")
        );
        let file = if site == "site:one" {
            "guide/index.html"
        } else {
            "elsewhere/index.html"
        };
        let mount = if site == "site:one" { "/mount/" } else { "/" };
        assert_eq!(
            ready["sites"][site]["url"],
            format!("http://{socket}{mount}")
        );
        let http = response(socket, "GET", &format!("{mount}{file}"));
        assert_eq!(body(&http), bytes(output, file));
        let html = std::str::from_utf8(body(&http)).expect("rendered HTML");
        let (content, canonical, attribute) = if site == "site:one" {
            (
                "content:one",
                "https://one.example/mount/guide/",
                "https:&#x2F;&#x2F;one.example&#x2F;mount&#x2F;guide&#x2F;",
            )
        } else {
            (
                "content:other",
                "https://other.example/elsewhere/",
                "https:&#x2F;&#x2F;other.example&#x2F;elsewhere&#x2F;",
            )
        };
        // The existing fixture's Tera attribute is autoescaped, including '/'.
        // Assert that exact production href, not a raw URL substring in HTML.
        assert!(html.contains(&format!("<a href=\"{attribute}\">canonical</a>")));
        let machine = response(socket, "GET", &format!("{mount}model.json"));
        assert_eq!(body(&machine), bytes(output, "model.json"));
        let model: Value = serde_json::from_slice(body(&machine)).expect("served shared model");
        assert_eq!(model["pages"][content]["route"]["canonical_url"], canonical);
        assert!(body(&response(socket, "HEAD", &format!("{mount}{file}"))).is_empty());
    }
    process.close();
    for socket in addresses {
        TcpListener::bind(socket).expect("closed event follows release of every listener");
    }
}

#[test]
fn initial_eof_and_buffered_close_byte_both_emit_ready_then_closed() {
    let fixture = Fixture::new(&[]);
    let initial = request(&fixture.config(), &SITES).to_string();
    for input in [initial.clone(), format!("{initial}\nx")] {
        let result = cli(&input, &ARGUMENTS);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let events = String::from_utf8(result.stdout)
            .expect("JSONL events")
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).expect("complete event"))
            .collect::<Vec<_>>();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["event"], "ready");
        assert_eq!(events[1], json!({"event":"closed"}));
        for site in SITES {
            TcpListener::bind(address(&events[0], site)).expect("EOF/byte closes sockets");
        }
    }
}

#[test]
fn permissions_framing_and_exact_listeners_refuse_without_readiness() {
    let input = request("intentionally invalid workspace", &SITES).to_string();
    let denied = cli(
        &input,
        &[
            "preview-sites",
            "--local",
            "--listen",
            "site:one=127.0.0.1:0",
        ],
    );
    assert!(!denied.status.success());
    assert!(denied.stdout.is_empty());
    assert!(String::from_utf8_lossy(&denied.stderr).contains("preview_refused"));
    for extra in [
        vec!["--listen", "site:one=0.0.0.0:0"],
        vec!["--listen", "site:one=127.0.0.1:0"],
        vec![
            "--listen",
            "site:one=127.0.0.1:0",
            "--listen",
            "site:one=127.0.0.1:0",
        ],
    ] {
        let mut args = vec!["preview-sites", "--local", "--allow-listeners"];
        args.extend(extra);
        let result = cli(&input, &args);
        assert!(!result.status.success());
        assert!(result.stdout.is_empty());
        assert!(String::from_utf8_lossy(&result.stderr).contains("preview_refused"));
    }
    let malformed = cli("{\n", &ARGUMENTS);
    assert!(!malformed.status.success());
    assert!(malformed.stdout.is_empty());
    assert!(String::from_utf8_lossy(&malformed.stderr).contains("publication_set_input"));
    let mut tiny = ARGUMENTS.to_vec();
    tiny.extend(["--max-source-bytes", "1"]);
    let oversized = cli("{}\n", &tiny);
    assert!(!oversized.status.success());
    assert!(String::from_utf8_lossy(&oversized.stderr).contains("publication_set_input_limit"));
}

#[test]
fn failed_site_or_listener_startup_never_announces_a_reduced_set() {
    let fixture = Fixture::new(&[]);
    let mut selected = request(&fixture.config(), &SITES);
    selected["sites"][1]["model_output"] = json!("elsewhere/index.html");
    let failed = cli(&selected.to_string(), &ARGUMENTS);
    assert!(!failed.status.success());
    assert!(failed.stdout.is_empty());
    let diagnostic = String::from_utf8_lossy(&failed.stderr);
    assert!(
        diagnostic.contains("site:other: source_inventory:"),
        "{diagnostic}"
    );
    assert!(diagnostic.contains("elsewhere/index.html"), "{diagnostic}");
    assert!(diagnostic.contains("all listeners closed"));

    let occupied = TcpListener::bind("127.0.0.1:0").expect("test-owned occupied socket");
    let held = occupied.local_addr().expect("occupied address");
    let free = TcpListener::bind("127.0.0.1:0").expect("available test socket");
    let available = free.local_addr().expect("available address");
    drop(free);
    let first = format!("site:one={available}");
    let second = format!("site:other={held}");
    let failed = cli(
        &request(&fixture.config(), &SITES).to_string(),
        &[
            "preview-sites",
            "--local",
            "--allow-listeners",
            "--listen",
            &first,
            "--listen",
            &second,
        ],
    );
    assert!(!failed.status.success());
    assert!(failed.stdout.is_empty());
    assert!(String::from_utf8_lossy(&failed.stderr).contains("preview_io"));
    TcpListener::bind(available).expect("failed complete-set bind releases an earlier listener");
}

#[test]
fn lost_ready_report_closes_listeners_and_never_claims_success() {
    let fixture = Fixture::new(&[]);
    let input = request(&fixture.config(), &SITES).to_string();
    let available =
        SITES.map(|_| TcpListener::bind("127.0.0.1:0").expect("reserve disposable address"));
    let sockets = available
        .each_ref()
        .map(|listener| listener.local_addr().expect("reserved address"));
    drop(available);
    let first = format!("site:one={}", sockets[0]);
    let second = format!("site:other={}", sockets[1]);
    let mut child = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args([
            "preview-sites",
            "--local",
            "--allow-listeners",
            "--listen",
            &first,
            "--listen",
            &second,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("preview with a deliberately lost event consumer");
    drop(
        child
            .stdout
            .take()
            .expect("close read end before readiness"),
    );
    child
        .stdin
        .as_mut()
        .expect("initial request input")
        .write_all(format!("{input}\n").as_bytes())
        .expect("initial JSON line");
    let result = child
        .wait_with_output()
        .expect("lost report terminates preview process");
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("preview_delivery"));
    assert!(String::from_utf8_lossy(&result.stderr).contains("all listeners closed"));
    for socket in sockets {
        TcpListener::bind(socket).expect("lost ready receipt released each actual socket");
    }
}

#[test]
fn explicit_local_urls_join_both_sites_and_assets_without_changing_configured_urls() {
    use gazette_sdk::html::{self, TextKind};
    use std::convert::Infallible;

    const ASSET: &[u8] = b"exact retained local download\n";
    let page = format!(
        "{}\n[Download](asset.txt)\n[[site:other.example/foreign/|Foreign collection]]\n",
        super::super::PAGE
            .replace(
                "related=['content:other']",
                "related=['content:other','collection:foreign']",
            )
            .replace("lifecycle='draft'", "lifecycle='published'"),
    );
    let foreign = super::super::HOME
        .replace("collection:home", "collection:foreign")
        .replace("site:one", "site:other")
        .replace("path='/'", "path='/foreign/'");
    let template = format!(
        "{}{{% for trail in breadcrumbs %}}{{% for item in trail.items %}}{{% if item.route %}}<a href=\"{{{{ item.route.canonical_url }}}}\">{{{{ item.node.title }}}}</a>{{% endif %}}{{% endfor %}}{{% endfor %}}",
        super::super::TEMPLATE,
    );
    let fixture = Fixture::new(&[
        ("one/page.md", page.as_bytes()),
        ("one/asset.txt", ASSET),
        ("other/foreign.md", foreign.as_bytes()),
        ("theme/templates/page.html", template.as_bytes()),
    ]);
    let config = fixture.config().replace(
        "authored=[{source='source:one',target='one'}]",
        "authored=[{source='source:one',target='one',static_assets={route_prefix='/media/'}}]",
    );
    let config_path = fixture.directory.path().join("workspace.toml");
    fs::write(&config_path, &config).expect("test-owned original workspace config");
    let request = request(&config, &SITES);
    let mut process = Process::with_arguments(&request, &["--local-urls"]);
    let ready = process.event();
    assert_eq!(ready["status"], "local-cross-site-noncertified");
    assert_eq!(
        ready["sites"]
            .as_object()
            .expect("complete ready set")
            .len(),
        2
    );
    let one = address(&ready, "site:one");
    let other = address(&ready, "site:other");
    let one_base = format!("http://{one}/mount/");
    let other_base = format!("http://{other}/");
    assert_eq!(ready["sites"]["site:one"]["url"], one_base);
    assert_eq!(ready["sites"]["site:other"]["url"], other_base);
    let canonical = format!("{one_base}guide/");
    let cross_site = format!("{other_base}foreign/");
    let asset = format!("{one_base}media/asset.txt");
    let served = response(one, "GET", "/mount/guide/index.html");
    let mut attributes = Vec::new();
    html::inspect(body(&served), html::Limits::default(), |kind, value| {
        if kind == TextKind::Attribute {
            attributes.push(value.to_owned());
        }
        Ok::<_, Infallible>(())
    })
    .expect("real HTML decoder checks complete served page");
    for expected in [&canonical, &cross_site, &asset] {
        assert!(attributes.contains(expected), "exact local link {expected}");
    }
    let one_model = response(one, "GET", "/mount/model.json");
    let one_model: Value = serde_json::from_slice(body(&one_model)).expect("local site model");
    let page_model = &one_model["pages"]["content:one"];
    assert_eq!(page_model["route"]["canonical_url"], canonical);
    let trails = page_model["breadcrumbs"]
        .as_array()
        .expect("sole local residence");
    assert_eq!(trails.len(), 1);
    assert_eq!(trails[0]["via"], "collection:home");
    assert_eq!(trails[0]["items"][0]["node"]["site"], "site:one");
    assert_eq!(trails[0]["items"][0]["route"]["canonical_url"], one_base);
    assert!(
        page_model["related"]
            .as_array()
            .expect("retained cross-site references")
            .iter()
            .any(|node| node["id"] == "collection:foreign" && node["site"] == "site:other")
    );
    assert_eq!(
        page_model["links"]
            .as_array()
            .expect("bound authored links")
            .iter()
            .find(|link| link["content"] == "collection:foreign")
            .expect("exact foreign reference")["destination"],
        cross_site
    );
    assert_eq!(page_model["assets"][0]["url"], asset);
    assert_eq!(
        one_model["assets"][&asset]["sha256"],
        json!(source::Sha256Digest::of(ASSET))
    );
    assert_eq!(body(&response(one, "GET", "/mount/media/asset.txt")), ASSET);
    let foreign_page = response(other, "GET", "/foreign/index.html");
    assert!(
        std::str::from_utf8(body(&foreign_page))
            .expect("foreign HTML")
            .contains("Home body")
    );
    let other_model = response(other, "GET", "/model.json");
    let other_model: Value = serde_json::from_slice(body(&other_model)).expect("other site model");
    assert_eq!(
        other_model["pages"]["collection:foreign"]["route"]["canonical_url"],
        cross_site
    );
    assert_eq!(
        other_model["pages"]["content:other"]["route"]["canonical_url"],
        format!("{other_base}elsewhere/")
    );
    process.close();
    for socket in [one, other] {
        TcpListener::bind(socket).expect("local-URL shutdown releases original reservation");
    }
    assert_eq!(
        fs::read_to_string(config_path).expect("original configuration"),
        config
    );
    assert_eq!(
        fs::read(fixture.directory.path().join("one/page.md")).expect("original authored input"),
        page.as_bytes()
    );
    let inputs = acquire_set(&config, &SITES);
    let page = "page.html".parse().expect("page template");
    let alias = "alias.html".parse().expect("alias template");
    let model = "model.json".parse().expect("model output");
    let production = build::render_publications(
        &inputs,
        options(&SITES, &page, &alias, &model),
        Default::default(),
    )
    .expect("subsequent independent production-linked render");
    let production = production.sites()[&key("site:one")]
        .as_ref()
        .expect("complete normal output");
    let model: Value = serde_json::from_slice(bytes(production.site().output(), "model.json"))
        .expect("normal model");
    assert_eq!(
        model["pages"]["content:one"]["route"]["canonical_url"],
        "https://one.example/mount/guide/"
    );
    assert_eq!(
        model["pages"]["content:one"]["assets"][0]["url"],
        "https://one.example/mount/media/asset.txt"
    );
    assert_eq!(
        model["pages"]["content:one"]["links"]
            .as_array()
            .expect("production-bound links")
            .iter()
            .find(|link| link["content"] == "collection:foreign")
            .expect("same foreign identity")["destination"],
        "https://other.example/foreign/"
    );
}

#[test]
fn local_url_render_failure_releases_the_complete_reserved_set_without_readiness() {
    let fixture = Fixture::new(&[]);
    let available = SITES.map(|_| TcpListener::bind("127.0.0.1:0").expect("reserve test socket"));
    let sockets = available
        .each_ref()
        .map(|listener| listener.local_addr().expect("test address"));
    drop(available);
    let first = format!("site:one={}", sockets[0]);
    let second = format!("site:other={}", sockets[1]);
    let arguments = [
        "preview-sites",
        "--local",
        "--local-urls",
        "--allow-listeners",
        "--listen",
        &first,
        "--listen",
        &second,
    ];
    let mut selected = request(&fixture.config(), &SITES);
    selected["sites"][1]["model_output"] = json!("elsewhere/index.html");
    let result = cli(&selected.to_string(), &arguments);
    assert!(!result.status.success());
    assert!(
        result.stdout.is_empty(),
        "no successful subset is announced"
    );
    let diagnostic = String::from_utf8_lossy(&result.stderr);
    assert!(
        diagnostic.contains("site:other: source_inventory:"),
        "{diagnostic}"
    );
    assert!(diagnostic.contains("elsewhere/index.html"), "{diagnostic}");
    assert!(diagnostic.contains("all listeners closed"));
    for socket in sockets {
        TcpListener::bind(socket).expect("render failure releases every reserved listener");
    }
    let denied = cli(
        "unread invalid input",
        &[
            "preview-sites",
            "--local",
            "--local-urls",
            "--listen",
            &first,
            "--listen",
            &second,
        ],
    );
    assert!(!denied.status.success());
    assert!(denied.stdout.is_empty());
    assert!(String::from_utf8_lossy(&denied.stderr).contains("preview_refused"));
}

#[test]
fn concurrent_local_url_processes_remain_independent_after_one_is_killed() {
    let fixture = Fixture::new(&[]);
    let config = fixture.config();
    let config_path = fixture.directory.path().join("workspace.toml");
    fs::write(&config_path, &config).expect("original test-owned configuration");
    let input = request(&config, &SITES);
    let mut first = Process::with_arguments(&input, &["--local-urls"]);
    let first_ready = first.event();
    let mut second = Process::with_arguments(&input, &["--local-urls"]);
    let second_ready = second.event();
    let first_sockets = SITES.map(|site| address(&first_ready, site));
    let second_sockets = SITES.map(|site| address(&second_ready, site));
    assert_eq!(
        first_sockets
            .into_iter()
            .chain(second_sockets)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        4,
        "simultaneously held loopback reservations cannot share sockets"
    );
    for ready in [&first_ready, &second_ready] {
        assert_eq!(ready["status"], "local-cross-site-noncertified");
        assert_eq!(
            ready["sites"]
                .as_object()
                .expect("complete process set")
                .len(),
            2
        );
        for (site, content, mount, route) in [
            ("site:one", "content:one", "/mount/", "guide/"),
            ("site:other", "content:other", "/", "elsewhere/"),
        ] {
            let socket = address(ready, site);
            let canonical = format!("http://{socket}{mount}{route}");
            let model = response(socket, "GET", &format!("{mount}model.json"));
            let model: Value = serde_json::from_slice(body(&model)).expect("live process model");
            assert_eq!(model["pages"][content]["route"]["canonical_url"], canonical);
            let page = response(socket, "GET", &format!("{mount}{route}index.html"));
            let html = std::str::from_utf8(body(&page)).expect("live process page");
            assert!(
                html.contains(&format!(
                    "<a href=\"{}\">canonical</a>",
                    canonical.replace('/', "&#x2F;")
                )),
                "HTML must use this process's exact model URL"
            );
        }
    }
    let before = second_sockets.map(|socket| {
        let mount = if socket == second_sockets[0] {
            "/mount/"
        } else {
            "/"
        };
        response(socket, "GET", &format!("{mount}model.json"))
    });
    first
        .child
        .kill()
        .expect("kill only the first exact test-owned preview process");
    assert!(
        !first
            .child
            .wait()
            .expect("killed process is reaped")
            .success()
    );
    let mut tail = String::new();
    first
        .output
        .read_to_string(&mut tail)
        .expect("terminated event pipe closes");
    assert!(
        tail.is_empty(),
        "crash cannot emit a graceful closed receipt"
    );
    for socket in first_sockets {
        TcpListener::bind(socket).expect("process death releases each of its own listeners");
    }
    for (index, socket) in second_sockets.into_iter().enumerate() {
        let mount = if index == 0 { "/mount/" } else { "/" };
        let after = response(socket, "GET", &format!("{mount}model.json"));
        assert_eq!(
            body(&after),
            body(&before[index]),
            "other process remains live with unchanged URLs"
        );
    }
    second.close();
    for socket in second_sockets {
        TcpListener::bind(socket).expect("normal close releases the surviving session");
    }
    assert_eq!(
        fs::read_to_string(config_path).expect("configuration after both sessions"),
        config
    );
    let inputs = acquire_set(&config, &SITES);
    let page = "page.html".parse().expect("page template");
    let alias = "alias.html".parse().expect("alias template");
    let model = "model.json".parse().expect("model output");
    let normal = build::render_publications(
        &inputs,
        options(&SITES, &page, &alias, &model),
        Default::default(),
    )
    .expect("production-linked render after crashed and concurrent previews");
    for (site, content, canonical) in [
        (
            "site:one",
            "content:one",
            "https://one.example/mount/guide/",
        ),
        (
            "site:other",
            "content:other",
            "https://other.example/elsewhere/",
        ),
    ] {
        let site = normal.sites()[&key(site)]
            .as_ref()
            .expect("complete normal site");
        let model: Value = serde_json::from_slice(bytes(site.site().output(), "model.json"))
            .expect("normal model");
        assert_eq!(model["pages"][content]["route"]["canonical_url"], canonical);
    }
}

#[test]
fn explicit_corpus_byte_limit_reaches_preview_acquisition_and_rendering() {
    let padding = vec![0_u8; 16 * 1024 * 1024];
    let fixture = Fixture::new(&[("one/padding.png", &padding)]);
    let config = fixture.config().replace(
        "target='one'}",
        "target='one',static_assets={route_prefix='/media/'}}",
    );
    let request = request(&config, &SITES);
    let mut process = Process::with_arguments(&request, &["--max-corpus-bytes", "17825792"]);
    let ready = process.event();
    assert_eq!(ready["event"], "ready");
    for site in SITES {
        let path = if site == "site:one" {
            "/mount/"
        } else {
            "/elsewhere/"
        };
        let _ = body(&response(address(&ready, site), "GET", path));
    }
    process.close();
}
