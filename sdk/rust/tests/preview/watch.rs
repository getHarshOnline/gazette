//! Connected development captures and complete generations through the real SDK.

use super::*;
use gazette_sdk::preview::watch::{self, Control, Event, Generation};
use std::io;

fn run(
    fixture: &Fixture,
    local_urls: bool,
    observe: impl FnMut(Event<'_>) -> io::Result<Control>,
) -> Result<(), watch::Error> {
    run_with_math(fixture, local_urls, None, observe)
}

pub(super) fn run_with_math(
    fixture: &Fixture,
    local_urls: bool,
    math_generation: Option<build::MathGenerationOptions>,
    observe: impl FnMut(Event<'_>) -> io::Result<Control>,
) -> Result<(), watch::Error> {
    let model = "model.json".parse().expect("model path");
    let configuration =
        gazette_application::configuration::resolve(&fixture.config, Default::default())
            .expect("fixed workspace compilation");
    watch::run(
        watch::Request {
            workspace: &configuration,
            build_selection: configuration
                .compilation()
                .workspace
                .build_selection(Some(&SITES.map(key)))
                .expect("exact invocation sites"),
            render: build::PublicationSetRenderOptions {
                derive_selection: false,
                preview_link_bases: None,
                selection: None,
                review_responses: None,
                preview_urls: None,
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
            },
            addresses: addresses(&SITES),
        },
        watch::Options {
            local_urls,
            math_generation,
            interval: Duration::from_millis(100),
            listeners: permission(),
            ..Default::default()
        },
        observe,
    )
}

fn text<'a>(current: &'a Generation, site: &str, path: &str) -> &'a str {
    std::str::from_utf8(
        current.sites()[&key(site)]
            .output()
            .file(&path.parse().expect("output path"))
            .expect("complete generation output"),
    )
    .expect("UTF-8 fixture output")
}

#[test]
fn retained_http_output_stays_available_during_delayed_capture_and_switches_as_a_set() {
    use gazette_ports::source::{SourceCapture, SourceReader};
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    };

    struct DelayedSource {
        armed: AtomicBool,
        started: mpsc::Sender<()>,
        released: mpsc::Receiver<()>,
    }
    impl SourceReader for DelayedSource {
        fn read(
            &self,
            root: &Path,
            subtree: Option<&gazette_core::source::Subdirectory>,
            limits: gazette_core::source::Limits,
        ) -> Result<gazette_core::source::Inventory, gazette_core::source::Error> {
            gazette_adapter_fs::FileSystem.read(root, subtree, limits)
        }
    }
    impl SourceCapture for DelayedSource {
        fn capture(
            &self,
            root: &Path,
            subtree: Option<&gazette_core::source::Subdirectory>,
            limits: gazette_core::source::Limits,
        ) -> Result<gazette_core::source::CapturedSource, gazette_core::source::Error> {
            if self.armed.swap(false, Ordering::AcqRel) {
                self.started
                    .send(())
                    .expect("announce a blocked native capture");
                self.released
                    .recv_timeout(Duration::from_secs(5))
                    .expect("client releases bounded capture");
            }
            gazette_adapter_fs::FileSystem.capture(root, subtree, limits)
        }
    }
    fn request(address: SocketAddr) -> io::Result<String> {
        let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(1))?;
        stream.set_read_timeout(Some(Duration::from_secs(1)))?;
        stream.set_write_timeout(Some(Duration::from_secs(1)))?;
        write!(
            stream,
            "GET /pr%C3%A9fix/ HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
        )?;
        let mut response = String::new();
        stream.read_to_string(&mut response)?;
        Ok(response)
    }
    let fixture = Fixture::new();
    let configuration =
        gazette_application::configuration::resolve(&fixture.config, Default::default()).unwrap();
    let (started, blocked) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let source = DelayedSource {
        armed: AtomicBool::new(false),
        started,
        released,
    };
    let mut waiting = Some((blocked, release));
    let mut old_client = None;
    let mut new_client = None;
    let mut held = BTreeMap::new();
    let deadline = Instant::now() + Duration::from_secs(20);
    gazette_application::preview::watch::run(
        gazette_application::preview::watch::Adapters {
            host_files: None,
            social_card_helper: None,
            source: &source,
            revisions: &gazette_adapter_git::Git,
            templates: &gazette_adapter_tera::Compiler,
            resources: &gazette_adapter_javascript::Compiler,
            raster: &gazette_sdk::raster::inspect,
            markdown: &gazette_adapter_markdown::Comrak,
            runner: &gazette_adapter_process::Runner,
            server: &gazette_adapter_http::Http,
        },
        watch::Request {
            workspace: &configuration,
            build_selection: configuration
                .compilation()
                .workspace
                .build_selection(Some(&SITES.map(key)))
                .unwrap(),
            render: build::PublicationSetRenderOptions {
                derive_selection: false,
                preview_link_bases: None,
                selection: None,
                review_responses: None,
                preview_urls: None,
                sites: SITES
                    .into_iter()
                    .map(|site| {
                        (
                            key(site),
                            build::SiteRenderOptions {
                                render_year: None,
                                graph: None,
                                model_output: None,
                                limits: Default::default(),
                            },
                        )
                    })
                    .collect(),
            },
            addresses: addresses(&SITES),
        },
        watch::Options {
            interval: Duration::from_millis(100),
            listeners: permission(),
            ..Default::default()
        },
        |event| {
            assert!(Instant::now() < deadline, "bounded concurrent preview test");
            match event {
                Event::Ready { addresses, .. } => {
                    held = addresses.clone();
                    for site in ["one", "two"] {
                        let path = fixture.directory.path().join(site).join("page.md");
                        fs::write(
                            &path,
                            fs::read_to_string(&path)?.replace("Retained", "Changed"),
                        )?;
                    }
                    source.armed.store(true, Ordering::Release);
                    let addresses = addresses.clone();
                    let (blocked, release) = waiting.take().unwrap();
                    old_client = Some(std::thread::spawn(move || {
                        blocked.recv_timeout(Duration::from_secs(10)).unwrap();
                        let responses: Vec<_> = addresses
                            .values()
                            .map(|address| request(*address))
                            .collect();
                        // Always unblock acquisition before an assertion can fail.
                        release.send(()).unwrap();
                        for response in responses {
                            let response = response
                                .expect("retained HTTP responds while acquisition is blocked");
                            assert!(response.starts_with("HTTP/1.1 200"), "{response}");
                            assert!(response.contains("Retained"));
                            assert!(!response.contains("Changed"));
                        }
                    }));
                }
                Event::Rebuilt {
                    generation,
                    addresses,
                    ..
                } => {
                    assert_eq!(generation, 2);
                    assert_eq!(addresses, &held);
                    old_client.take().unwrap().join().unwrap();
                    let addresses = addresses.clone();
                    new_client = Some(std::thread::spawn(move || {
                        for address in addresses.values() {
                            let response = request(*address).unwrap();
                            assert!(response.contains("Changed"));
                            assert!(!response.contains("Retained"));
                        }
                    }));
                }
                Event::Poll
                    if new_client
                        .as_ref()
                        .is_some_and(std::thread::JoinHandle::is_finished) =>
                {
                    new_client.take().unwrap().join().unwrap();
                    return Ok(Control::Stop);
                }
                Event::RebuildFailed { error, .. } => panic!("unexpected rebuild failure: {error}"),
                Event::Closed => {
                    for address in held.values() {
                        TcpListener::bind(address).expect("joined transport releases every socket");
                    }
                }
                _ => (),
            }
            Ok(Control::Continue)
        },
    )
    .expect("native watch serves through acquisition and swaps complete outputs");
    assert!(old_client.is_none() && new_client.is_none());
}

#[test]
fn configured_watch_retains_workspace_selection_and_never_invents_model_output() {
    let fixture = Fixture::new();
    let config = format!(
        "[build]\nsites=['site:two','site:one']\nlocal_drafts='include'\n{}",
        fixture.config
    );
    let configuration = gazette_application::configuration::resolve(&config, Default::default())
        .expect("fixed configured workspace");
    let selected = configuration
        .compilation()
        .workspace
        .build_selection(None)
        .expect("native configured selection before any listener or capture");
    let expected = selected.clone();
    let mut ready = false;
    watch::run(
        watch::Request {
            workspace: &configuration,
            build_selection: selected,
            render: build::PublicationSetRenderOptions {
                derive_selection: false,
                preview_link_bases: None,
                sites: SITES
                    .into_iter()
                    .map(|site| {
                        (
                            key(site),
                            build::SiteRenderOptions {
                                render_year: None,
                                model_output: None,
                                graph: None,
                                limits: Default::default(),
                            },
                        )
                    })
                    .collect(),
                selection: None,
                review_responses: None,
                preview_urls: None,
            },
            addresses: addresses(&SITES),
        },
        watch::Options {
            local_urls: false,
            interval: Duration::from_millis(100),
            listeners: permission(),
            ..Default::default()
        },
        |event| match event {
            Event::Ready { current, .. } => {
                ready = true;
                assert_eq!(current.inputs().build_selection(), &expected);
                assert_eq!(
                    expected.origin,
                    gazette_sdk::workspace::BuildOrigin::Workspace
                );
                assert_eq!(expected.sites, [key("site:two"), key("site:one")]);
                for site in current.sites().values() {
                    assert!(site.evidence().model_output.is_none());
                    assert!(site.output().file(&"model.json".parse().unwrap()).is_none());
                    assert!(site.output().file(&"index.html".parse().unwrap()).is_some());
                }
                Ok(Control::Stop)
            }
            Event::RebuildFailed { error, .. } => panic!("configured watch failed: {error}"),
            _ => Ok(Control::Continue),
        },
    )
    .expect("connected native configured watch");
    assert!(ready);
}

#[test]
fn dirty_capture_rebuilds_shared_outputs_and_recovers_same_bytes_without_new_generation() {
    let fixture = Fixture::linked();
    let pinned = fixture.acquire(&SITES);
    let source = fixture.directory.path().join("one/page.md");
    let original = fs::read_to_string(&source).expect("committed page");
    let first = original.replace("Retained one", "Dirty initial");
    fs::write(&source, &first).expect("explicit dirty development source");
    let template_path = fixture.directory.path().join("theme/templates/page.html");
    let template = fs::read_to_string(&template_path).expect("committed shared template");
    let changed_template = format!("<aside>Shared update</aside>{template}");
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut events = Vec::new();
    let mut held = BTreeMap::new();
    let mut retained = None;
    run(&fixture, true, |event| {
        assert!(
            Instant::now() < deadline,
            "bounded watch fixture progression"
        );
        match event {
            Event::Ready {
                generation,
                current,
                addresses,
                local_urls,
            } => {
                assert_eq!(generation, 1);
                assert!(text(current, "site:one", "index.html").contains("Dirty initial"));
                assert!(
                    current
                        .inputs()
                        .sources()
                        .values()
                        .all(|input| input.is_development())
                );
                assert_ne!(
                    current.inputs().sources()[&key("source:one")]
                        .inventory()
                        .identity(),
                    pinned.sources()[&key("source:one")].inventory().identity()
                );
                assert_eq!(
                    current.inputs().compilation().source_sha256,
                    pinned.compilation().source_sha256,
                    "dirty capture does not alter configured pins"
                );
                assert_eq!(current.urls(), local_urls);
                held = addresses.clone();
                events.push("ready");
                fs::write(&source, first.replace("Dirty initial", "Rebuilt content"))?;
                fs::write(&template_path, &changed_template)?;
            }
            Event::Rebuilt {
                generation,
                current,
                addresses,
                local_urls,
            } => {
                assert_eq!(generation, 2);
                assert_eq!(addresses, &held);
                assert_eq!(current.urls(), local_urls);
                for site in SITES {
                    let html = text(current, site, "index.html");
                    assert!(
                        html.contains("Shared update"),
                        "shared template updates every site"
                    );
                    let model: serde_json::Value =
                        serde_json::from_str(text(current, site, "model.json")).unwrap();
                    let name = site.strip_prefix("site:").unwrap();
                    let page = &model["pages"][format!("content:{name}")];
                    let public_url = format!("https://{name}.example.invalid/pr%C3%A9fix/");
                    assert_eq!(page["route"]["url"], local_urls[&key(site)].as_str());
                    assert_eq!(
                        page["route"]["canonical_url"],
                        local_urls[&key(site)].as_str()
                    );
                    assert_eq!(page["route"]["public_url"], public_url);
                    assert_eq!(page["json_ld"]["@id"], format!("{public_url}#article"));
                    assert_eq!(page["json_ld"]["url"], local_urls[&key(site)].as_str());
                }
                assert!(text(current, "site:one", "index.html").contains("Rebuilt content"));
                assert!(text(current, "site:one", "search.json").contains("Rebuilt content"));
                retained = Some((
                    current.input_identity().clone(),
                    current
                        .sites()
                        .iter()
                        .map(|(key, site)| (key.clone(), site.output().inventory().identity()))
                        .collect::<BTreeMap<_, _>>(),
                ));
                events.push("rebuilt");
                fs::write(&template_path, "{% if %}")?;
            }
            Event::RebuildFailed { generation, error } => {
                assert_eq!(generation, 2, "failed render cannot advance generation");
                assert_eq!(error.code(), "preview_incomplete_generation");
                for address in held.values() {
                    assert!(
                        TcpListener::bind(address).is_err(),
                        "failed build retains every listener"
                    );
                }
                events.push("failed");
                fs::write(&template_path, &changed_template)?;
            }
            Event::Recovered {
                generation,
                current,
                addresses,
                ..
            } => {
                assert_eq!(
                    generation, 2,
                    "restoring identical input is recovery, not new output"
                );
                assert_eq!(addresses, &held);
                let (identity, outputs) = retained.as_ref().expect("prior complete generation");
                assert_eq!(current.input_identity(), identity);
                for (key, expected) in outputs {
                    assert_eq!(
                        current.sites()[key].output().inventory().identity(),
                        *expected
                    );
                }
                events.push("recovered");
                return Ok(Control::Stop);
            }
            Event::Closed => {
                for address in held.values() {
                    TcpListener::bind(address).expect("closed delivered after listener release");
                }
                events.push("closed");
            }
            Event::Poll => (),
        }
        Ok(Control::Continue)
    })
    .expect("real watch completes under explicit observer control");
    assert_eq!(
        events,
        ["ready", "rebuilt", "failed", "recovered", "closed"]
    );
}

#[test]
fn unchanged_failure_notifies_once_then_changed_input_and_recovery_notify() {
    let fixture = Fixture::linked();
    let path = fixture.directory.path().join("theme/templates/page.html");
    let original = fs::read_to_string(&path).unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut failures = 0;
    let mut first_failure = None;
    let mut changed = false;
    let mut recovered = false;
    run(&fixture, true, |event| {
        assert!(
            Instant::now() < deadline,
            "bounded failure coalescing fixture"
        );
        match event {
            Event::Ready { .. } => fs::write(&path, "{% if %}")?,
            Event::RebuildFailed { .. } => {
                failures += 1;
                if failures == 1 {
                    first_failure = Some(Instant::now());
                } else {
                    assert!(
                        changed && failures == 2,
                        "unchanged failure must stay quiet"
                    );
                    fs::write(&path, &original)?;
                }
            }
            Event::Poll
                if !changed
                    && first_failure
                        .is_some_and(|at| at.elapsed() > Duration::from_millis(350)) =>
            {
                assert_eq!(failures, 1);
                changed = true;
                fs::write(&path, "{% if %}\nChanged invalid input")?;
            }
            Event::Recovered { generation, .. } => {
                assert_eq!(generation, 1);
                recovered = true;
                return Ok(Control::Stop);
            }
            Event::Rebuilt { .. } => panic!("invalid input cannot replace output"),
            Event::Closed | Event::Poll => (),
        }
        Ok(Control::Continue)
    })
    .unwrap();
    assert!(recovered);
    assert_eq!(failures, 2);
}

#[test]
fn renamed_inputs_and_deleted_recreated_template_use_fresh_confined_captures() {
    let fixture = Fixture::new();
    let original = fixture.directory.path().join("one/page.md");
    let renamed = fixture.directory.path().join("one/renamed.md");
    let template = fixture.directory.path().join("theme/templates/page.html");
    let mut events = Vec::new();
    let mut identity = None;
    let deadline = Instant::now() + Duration::from_secs(15);
    run(&fixture, false, |event| {
        assert!(
            Instant::now() < deadline,
            "bounded native rename/delete fixture"
        );
        match event {
            Event::Ready { current, .. } => {
                identity = Some(current.input_identity().clone());
                events.push("ready");
                fs::rename(&original, &renamed)?;
            }
            Event::Rebuilt {
                generation: 2,
                current,
                ..
            } => {
                assert_ne!(
                    Some(current.input_identity()),
                    identity.as_ref(),
                    "path changes affect capture identity"
                );
                let source = &current.inputs().sources()[&key("source:one")];
                assert!(source.file(&"page.md".parse().expect("old path")).is_none());
                assert!(
                    source
                        .file(&"renamed.md".parse().expect("new path"))
                        .is_some()
                );
                assert!(text(current, "site:one", "index.html").contains("Retained one"));
                events.push("renamed");
                fs::remove_file(&template)?;
            }
            Event::RebuildFailed { generation, .. } => {
                assert_eq!(generation, 2);
                events.push("deleted");
                fs::write(&template, format!("<aside>Recreated</aside>{TEMPLATE}"))?;
            }
            Event::Rebuilt {
                generation: 3,
                current,
                local_urls,
                ..
            } => {
                for site in SITES {
                    assert!(text(current, site, "index.html").contains("Recreated"));
                    assert_ne!(
                        current.urls()[&key(site)],
                        local_urls[&key(site)],
                        "production-linked watch preserves public bases"
                    );
                    assert!(text(current, site, "model.json").contains("example.invalid"));
                }
                events.push("recreated");
                return Ok(Control::Stop);
            }
            Event::Closed => events.push("closed"),
            Event::Poll => (),
            _ => panic!("unexpected generation transition"),
        }
        Ok(Control::Continue)
    })
    .expect("rename/delete/recreate watch completes");
    assert_eq!(
        events,
        ["ready", "renamed", "deleted", "recreated", "closed"]
    );
}

#[test]
fn observer_failure_releases_listeners_and_poll_stop_is_bounded() {
    let fixture = Fixture::new();
    for fail in [true, false] {
        let mut held = BTreeMap::new();
        let mut polls = 0;
        let mut closed = false;
        let result = run(&fixture, true, |event| {
            match event {
                Event::Ready { addresses, .. } => {
                    held = addresses.clone();
                    if fail {
                        return Err(io::Error::other("test observer rejected readiness"));
                    }
                }
                Event::Poll => {
                    polls += 1;
                    return Ok(Control::Stop);
                }
                Event::Closed => closed = true,
                _ => panic!("stop must precede any rebuild"),
            }
            Ok(Control::Continue)
        });
        if fail {
            assert!(matches!(result, Err(watch::Error::Observer(_))));
            assert_eq!(polls, 0);
            assert!(
                !closed,
                "failed observer delivery is not a normal closed receipt"
            );
        } else {
            result.expect("poll checkpoint stops without another acquisition");
            assert_eq!(polls, 1);
            assert!(closed);
        }
        assert_eq!(held.len(), 2);
        for address in held.values() {
            TcpListener::bind(address).expect("termination releases complete listener set");
        }
    }
}

#[test]
fn invalid_interval_precedes_configuration_parsing_and_any_observation() {
    let fixture = Fixture::new();
    let configuration =
        gazette_application::configuration::resolve(&fixture.config, Default::default())
            .expect("fixed workspace compilation");
    for interval in [
        Duration::ZERO,
        Duration::from_millis(99),
        Duration::from_secs(3601),
    ] {
        let error = watch::run(
            watch::Request {
                workspace: &configuration,
                build_selection: configuration
                    .compilation()
                    .workspace
                    .build_selection(Some(&SITES.map(key)))
                    .expect("exact invocation sites"),
                render: build::PublicationSetRenderOptions {
                    derive_selection: false,
                    preview_link_bases: None,
                    sites: BTreeMap::new(),
                    selection: None,
                    review_responses: None,
                    preview_urls: None,
                },
                addresses: BTreeMap::new(),
            },
            watch::Options {
                interval,
                listeners: permission(),
                ..Default::default()
            },
            |_| panic!("invalid interval must precede startup and observer events"),
        )
        .expect_err("invalid polling bound");
        assert_eq!(error.code(), "preview_watch_options");
        assert!(matches!(
            error,
            watch::Error::Options("watch interval must be between 100 ms and one hour")
        ));
    }
}

#[test]
fn same_byte_rewrites_and_unobserved_bursts_do_not_create_generations() {
    let fixture = Fixture::new();
    let path = fixture.directory.path().join("one/page.md");
    let original = fs::read(&path).expect("original native page");
    let mut ready = None;
    let mut closed = false;
    run(&fixture, true, |event| {
        match event {
            Event::Ready { generation, .. } => {
                assert_eq!(generation, 1);
                // All writes finish before acquisition resumes. Neither mtime
                // nor a transient byte sequence becomes a compilation input.
                fs::write(&path, b"temporary incomplete save")?;
                fs::write(&path, &original)?;
                fs::write(&path, &original)?;
                ready = Some(Instant::now());
            }
            Event::Poll => {
                if ready.expect("initial generation").elapsed() >= Duration::from_millis(350) {
                    return Ok(Control::Stop);
                }
            }
            Event::Closed => closed = true,
            _ => panic!("identical final bytes must not produce rebuild, failure or recovery"),
        }
        Ok(Control::Continue)
    })
    .expect("unchanged captures remain the same generation over several intervals");
    assert!(closed);
    assert_eq!(fs::read(path).expect("final native bytes"), original);
}

#[test]
fn development_capture_does_not_relax_the_configured_git_pin() {
    let mut fixture = Fixture::new();
    let pin = git(fixture.directory.path(), &["rev-parse", "HEAD"]);
    fixture.config = fixture.config.replace(pin.trim(), &"0".repeat(40));
    let mut observed = false;
    let error = run(&fixture, true, |_| {
        observed = true;
        Ok(Control::Stop)
    })
    .expect_err("development mode still verifies the explicitly configured baseline");
    assert!(matches!(error, watch::Error::Build(_)));
    assert!(!observed, "wrong baseline cannot become a ready generation");
    assert_eq!(git(fixture.directory.path(), &["rev-parse", "HEAD"]), pin);
}

#[test]
fn production_linked_development_generation_cannot_prepare_a_release() {
    let fixture = Fixture::new();
    let journal = fixture.directory.path().join("absent-watch-journal");
    let parent = fixture.directory.path().join("absent-watch-stage-parent");
    let mut checked = false;
    run(&fixture, false, |event| {
        if let Event::Ready { current, .. } = event {
            let rendered = render(current.inputs(), false);
            assert!(
                rendered.preview_urls().is_none(),
                "no local-URL marker is needed to prohibit development release"
            );
            let site = rendered.sites()[&key("site:one")]
                .as_ref()
                .expect("development site");
            assert!(
                site.inputs()
                    .sources()
                    .values()
                    .all(|source| source.is_development())
            );
            let error = build::prepare_stage(
                site,
                build::PreparationRequest {
                    journal: &journal,
                    parent: &parent,
                    name: "watch-stage",
                    id: Sha256Digest::of(b"watch-refusal"),
                    expected: gazette_sdk::store::JournalHeader {
                        schema_version: 1,
                        journal_id: [9; 32],
                        journal_epoch: 1,
                        writer_fence: 1,
                    },
                },
                Default::default(),
            )
            .expect_err("developer output refused before missing permissions and native paths");
            assert!(matches!(
                error.cause.as_ref(),
                build::PreparationFailure::Preview
            ));
            assert!(!error.writer_acquired);
            assert!(error.journal.is_none() && error.receipt.is_none() && error.stage.is_none());
            assert!(!journal.exists() && !parent.exists());
            checked = true;
            return Ok(Control::Stop);
        }
        Ok(Control::Continue)
    })
    .expect("read-only development rendering and release refusal");
    assert!(checked);
}
