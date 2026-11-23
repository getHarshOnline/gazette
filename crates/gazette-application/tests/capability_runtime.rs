//! Connected capability acquisition, freeze and render behavior.

use std::{cell::Cell, collections::BTreeMap, path::Path};

use gazette_application::{build, source::PreparationLimits};
use gazette_compiler::toml;
use gazette_core::{
    publication::capability_runtime::{
        AcquisitionRequest, CapabilityAbsence, CapabilityResult, CapabilityResultOutcome,
        CapabilityValue, ProviderEvidence, ProviderRuntimeIdentity, ProviderSnapshot,
        SearchIndexFormat,
    },
    resource,
    source::{
        self, CapturedSource, CommitId, Content, EntryPath, Inventory, InventoryBuilder,
        MetadataBudget, Sha256Digest, Subdirectory, TreeInput,
    },
};
use gazette_ports::{
    capability::CapabilityAcquisition,
    resource::ResourceCompiler,
    source::{RevisionReader, SourceCapture, SourceReader},
};
use serde_json::Value;

const THEME_MANIFEST: &[u8] =
    b"package='theme:base'\nnamespace='base'\ntemplates='templates'\nbrowser_theme_colors={light='#ffffff',dark='#000000'}\nrender={page='page.html'}\n";
const THEME_TEMPLATE: &[u8] = br#"{{ presentation.surface.kind }}|{{ presentation.chrome.skip_to_content_label }}|{{ presentation.chrome.navigation[0].label }}|{{ presentation.browser_theme_colors.light }}|{% if presentation.search %}{{ presentation.search.no_script_destination.route.canonical_url }}{% else %}absent{% endif %}|{{ presentation.capabilities["publication.search/v1"].state }}{% if presentation.capabilities["publication.search/v1"].state == "available" %}|{{ presentation.capabilities["publication.search/v1"].value.index }}|{{ presentation.search.url }}{% endif %}"#;
const PLUGIN_MANIFEST: &[u8] = b"package='capabilities'\n[providers.shared]\nimplements=['publication.search/v1','commerce.selector/v1']\n[resources.'module:unselected']\ninputs=['unselected.ts']\nbuild={kind='browser-module',entry='unselected.ts'}\n";
const PAGE: &[u8] = b"+++\n[content]\nid='content:one'\nsite='site:one'\ntitle='Retained'\nlifecycle='draft'\nroute={path='/'}\nstructure={kind='page',family={kind='article'}}\n+++\n# Retained body\n";

#[test]
fn selected_search_page_is_in_presentation_but_not_its_own_search_corpus() {
    let reader = Reader::default();
    let config = configuration("required", None).replace(
        "machine={search='lexical'}",
        "search_page='content:one'\nmachine={search='lexical'}",
    );
    let mut inputs = acquired(&reader, &config);
    inputs
        .acquire_capabilities(
            &Capabilities {
                calls: Cell::new(0),
                mode: Mode::AvailableSearch,
            },
            toml::Limits::default(),
            Default::default(),
        )
        .unwrap();
    let rendered = render(&inputs).unwrap();
    let tree = rendered.site().output();
    let model: Value = serde_json::from_slice(tree.file(&path("model.json")).unwrap()).unwrap();
    assert_eq!(
        model["pages"]["content:one"]["presentation"]["search"]["page"]["content"]["id"],
        "content:one"
    );
    assert_eq!(
        model["pages"]["content:one"]["presentation"]["search"]["page"]["route"]["url"],
        "https://example.test/"
    );
    let search: Value = serde_json::from_slice(tree.file(&path("search.json")).unwrap()).unwrap();
    assert_eq!(search, serde_json::json!([]));
}

fn path(value: &str) -> EntryPath {
    value.parse().expect("literal entry path")
}

fn configuration(requirement: &str, absence: Option<&str>) -> String {
    let absence = absence
        .map(|reason| format!(",allow_absence='{reason}'"))
        .unwrap_or_default();
    format!(
        "[composition]\n\
         theme={{package='theme:base'}}\n\
         navigation=[{{label='Home',destination='content:one'}}]\n\
         [composition.chrome]\n\
         skip_to_content_label='Skip'\n\
         navigation_label='Primary'\n\
         footer_label='Footer'\n\
         [composition.chrome.shell]\n\
         close_label='Close'\n\
         [composition.chrome.shell.search]\n\
         trigger_label='Search'\n\
         title='Search'\n\
         field_label='Query'\n\
         loading_message='Loading'\n\
         empty_message='Empty'\n\
         unavailable_message='Unavailable'\n\
         no_script_destination='content:one'\n\
         [composition.chrome.shell.settings]\n\
         trigger_label='Settings'\n\
         title='Settings'\n\
         color_mode_label='Color'\n\
         color_mode_system_label='System'\n\
         color_mode_light_label='Light'\n\
         color_mode_dark_label='Dark'\n\
         motion_label='Motion'\n\
         motion_system_label='System'\n\
         motion_reduce_label='Reduce'\n\
         motion_allow_label='Allow'\n\
         [composition.plugins]\n\
         'capabilities'={{state='enabled'}}\n\
         [composition.capabilities]\n\
         'publication.search/v1'={{state='enabled',provider={{kind='plugin',package='capabilities',provider='shared'}},requirement='{requirement}'{absence}}}\n\
         'commerce.selector/v1'={{state='enabled',provider={{kind='plugin',package='capabilities',provider='shared'}},requirement='optional',allow_absence='missing'}}\n\
         [[principals]]\n\
         id='owner:one'\n\
         kind='person'\n\
         [[sites]]\n\
         id='site:one'\n\
         publisher='owner:one'\n\
         deployable=false\n\
         [[brands]]\n\
         id='brand:one'\n\
         name='Example'\n\
         [[brands.publications]]\n\
         site='site:one'\n\
         public_base_url='https://example.test/'\n\
         authored=[{{source='source:content',target='authored'}}]\n\
         machine={{search='lexical'}}\n\
         [[sources]]\n\
         id='source:theme'\n\
         location={{kind='repository',root='/fixture/theme',commit='1111111111111111111111111111111111111111',subdirectory='input'}}\n\
         [[sources]]\n\
         id='source:plugin'\n\
         location={{kind='repository',root='/fixture/plugin',commit='1111111111111111111111111111111111111111',subdirectory='input'}}\n\
         [[sources]]\n\
         id='source:content'\n\
         location={{kind='repository',root='/fixture/content',commit='1111111111111111111111111111111111111111',subdirectory='input'}}\n\
         [[theme_packages]]\n\
         package='theme:base'\n\
         source='source:theme'\n\
         manifest='theme.toml'\n\
         [[plugin_packages]]\n\
         package='capabilities'\n\
         source='source:plugin'\n\
         manifest='plugin.toml'\n"
    )
}

fn captured(root: &Path, limits: source::Limits) -> Result<CapturedSource, source::Error> {
    let entries: &[(&str, &[u8])] = if root == Path::new("/fixture/theme") {
        &[
            ("theme.toml", THEME_MANIFEST),
            ("templates/page.html", THEME_TEMPLATE),
        ]
    } else if root == Path::new("/fixture/plugin") {
        &[
            ("plugin.toml", PLUGIN_MANIFEST),
            ("unselected.ts", b"export const neverCompiled = true;"),
        ]
    } else {
        assert_eq!(root, Path::new("/fixture/content"));
        &[("authored.md", PAGE)]
    };
    let files = entries
        .iter()
        .map(|(name, bytes)| (path(name), bytes.to_vec()))
        .collect::<BTreeMap<_, _>>();
    let mut builder = InventoryBuilder::new(limits);
    if root == Path::new("/fixture/theme") {
        builder.insert(path("templates"), Content::Directory)?;
    }
    for (path, bytes) in &files {
        builder.insert(
            path.clone(),
            Content::File {
                bytes: bytes.len() as u64,
                sha256: Sha256Digest::of(bytes),
            },
        )?;
    }
    CapturedSource::from_parts(builder.finish(), files)
}

#[derive(Default)]
struct Reader {
    reads: Cell<usize>,
    sealed: Cell<bool>,
}

impl Reader {
    fn observe(&self, subdirectory: &Subdirectory) {
        assert!(!self.sealed.get(), "rendering cannot reopen source inputs");
        assert_eq!(subdirectory, &"input".parse().expect("subdirectory"));
        self.reads.set(self.reads.get() + 1);
    }
}

impl SourceReader for Reader {
    fn read(
        &self,
        _: &Path,
        _: Option<&Subdirectory>,
        _: source::Limits,
    ) -> Result<Inventory, source::Error> {
        panic!("fixture requires retained source capture")
    }
}

impl SourceCapture for Reader {
    fn capture(
        &self,
        root: &Path,
        subdirectory: Option<&Subdirectory>,
        limits: source::Limits,
    ) -> Result<CapturedSource, source::Error> {
        self.observe(subdirectory.expect("explicit subdirectory"));
        captured(root, limits)
    }
}

impl RevisionReader for Reader {
    fn read_revision(
        &self,
        root: &Path,
        subdirectory: &Subdirectory,
        commit: &CommitId,
        limits: source::Limits,
        _: &mut MetadataBudget,
    ) -> Result<Inventory, source::Error> {
        self.observe(subdirectory);
        assert_eq!(commit.as_str(), "1111111111111111111111111111111111111111");
        Ok(captured(root, limits)?.into_parts().0)
    }

    fn read_gitlink(
        &self,
        _: &Path,
        _: &CommitId,
        _: &Subdirectory,
        _: &mut MetadataBudget,
    ) -> Result<CommitId, source::Error> {
        panic!("fixture uses exact repository revisions")
    }
}

fn acquired_with_reads(
    reader: &Reader,
    configuration: &str,
    expected_reads: usize,
) -> build::AcquiredInputs {
    let inputs = build::acquire_publication(
        reader,
        reader,
        configuration,
        &"site:one".parse().expect("site"),
        toml::Limits::default(),
        PreparationLimits {
            sources: 3,
            entries: 8,
            bytes: 4096,
            per_source: source::RevisionLimits::default(),
        },
    )
    .expect("exact publication inputs");
    assert_eq!(reader.reads.get(), expected_reads);
    reader.sealed.set(true);
    inputs
}

fn acquired(reader: &Reader, configuration: &str) -> build::AcquiredInputs {
    acquired_with_reads(reader, configuration, 6)
}

fn gazette_configuration(
    search: bool,
    provider: &str,
    capability: &str,
    requirement: &str,
    absence: Option<&str>,
) -> String {
    let mut configuration = configuration(requirement, absence);
    configuration = configuration.replace("'capabilities'={state='enabled'}\n", "");
    configuration = configuration.replace(
        "'publication.search/v1'={state='enabled',provider={kind='plugin',package='capabilities',provider='shared'}",
        &format!(
            "'{capability}'={{state='enabled',provider={{kind='gazette',provider='{provider}'}}"
        ),
    );
    configuration = configuration.replace(
        "'commerce.selector/v1'={state='enabled',provider={kind='plugin',package='capabilities',provider='shared'},requirement='optional',allow_absence='missing'}\n",
        "",
    );
    configuration.replace(
        "machine={search='lexical'}",
        if search {
            "machine={search='lexical'}"
        } else {
            "machine={}"
        },
    )
}

#[derive(Clone, Copy)]
enum Mode {
    AvailableSearch,
    MismatchedSearch,
    Missing,
    Incomplete,
    WrongIdentity,
    Extra,
    Duplicate,
    TypedMismatch,
    InvalidEvidence,
}

struct Capabilities {
    calls: Cell<usize>,
    mode: Mode,
}

impl CapabilityAcquisition for Capabilities {
    fn acquire(
        &self,
        request: &AcquisitionRequest,
    ) -> Result<ProviderSnapshot, gazette_ports::capability::Failure> {
        self.calls.set(self.calls.get() + 1);
        assert_eq!(request.site().as_str(), "site:one");
        let ProviderRuntimeIdentity::Plugin { package, provider } = request.provider() else {
            panic!("fixture selects an exact plugin runtime")
        };
        assert_eq!(package.package.as_str(), "capabilities");
        assert_eq!(provider.as_str(), "shared");
        assert_eq!(
            request
                .capabilities()
                .iter()
                .map(|capability| capability.as_str())
                .collect::<Vec<_>>(),
            ["commerce.selector/v1", "publication.search/v1"]
        );
        let mut results = request
            .capabilities()
            .iter()
            .map(|capability| CapabilityResult {
                capability: capability.clone(),
                outcome: if capability.as_str() == "publication.search/v1"
                    && matches!(
                        self.mode,
                        Mode::AvailableSearch | Mode::MismatchedSearch | Mode::InvalidEvidence
                    ) {
                    CapabilityResultOutcome::Available {
                        value: CapabilityValue::PublicationSearchV1 {
                            index: if matches!(self.mode, Mode::MismatchedSearch) {
                                "/wrong-search.json".parse().expect("mismatched route")
                            } else {
                                "/search.json".parse().expect("search route")
                            },
                            format: SearchIndexFormat::GazetteDocumentsV1,
                        },
                        evidence: ProviderEvidence {
                            source: if matches!(self.mode, Mode::InvalidEvidence) {
                                "\n".into()
                            } else {
                                "private:search-source".into()
                            },
                            revision: "private-revision-7".into(),
                        },
                    }
                } else {
                    CapabilityResultOutcome::Unavailable {
                        reason: CapabilityAbsence::Missing,
                    }
                },
            })
            .collect::<Vec<_>>();
        if matches!(self.mode, Mode::Incomplete) {
            results.pop();
        }
        if matches!(self.mode, Mode::Extra) {
            results[0].capability = "commerce.inventory/v1"
                .parse()
                .expect("extra capability identity");
        }
        if matches!(self.mode, Mode::Duplicate) {
            results[1] = results[0].clone();
        }
        if matches!(self.mode, Mode::TypedMismatch) {
            results[0].outcome = CapabilityResultOutcome::Available {
                value: CapabilityValue::PublicationSearchV1 {
                    index: "/search.json".parse().expect("search route"),
                    format: SearchIndexFormat::GazetteDocumentsV1,
                },
                evidence: ProviderEvidence {
                    source: "private:mismatch".into(),
                    revision: "private-revision".into(),
                },
            };
        }
        results.reverse();
        let provider = if matches!(self.mode, Mode::WrongIdentity) {
            ProviderRuntimeIdentity::Gazette {
                provider: "static-search".parse().expect("provider"),
            }
        } else {
            request.provider().clone()
        };
        Ok(ProviderSnapshot::new(provider, results))
    }
}

struct NoResources;

impl ResourceCompiler for NoResources {
    fn compile(
        &self,
        _: &TreeInput<'_>,
        _: &resource::Recipe,
        _: &resource::ModuleLinks,
        _: resource::Limits,
    ) -> Result<resource::Artifact, resource::Error> {
        panic!("capability selection must not select a plugin resource")
    }
}

fn render(
    inputs: &build::AcquiredInputs,
) -> Result<build::PublicationRender<'_>, build::PublicationRenderError> {
    inputs.render_publication(
        &gazette_adapter_tera::Compiler,
        &NoResources,
        &gazette_core::raster::unavailable,
        build::SiteRenderOptions {
            render_year: None,
            model_output: Some(&path("model.json")),
            graph: None,
            limits: source::Limits::default(),
        },
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
}

#[test]
fn frozen_search_is_acquired_once_replayed_and_independent_from_plugin_resources() {
    let reader = Reader::default();
    let mut inputs = acquired(&reader, &configuration("required", None));
    let acquisition = Capabilities {
        calls: Cell::new(0),
        mode: Mode::AvailableSearch,
    };
    inputs
        .acquire_capabilities(&acquisition, toml::Limits::default(), Default::default())
        .expect("one exact admitted provider group freezes");
    assert_eq!(acquisition.calls.get(), 1);
    let capture = inputs.capability_capture().expect("frozen capture");
    assert_eq!(capture.groups().len(), 1);
    let runtime = capture
        .groups()
        .values()
        .next()
        .expect("provider group")
        .request
        .provider();
    let ProviderRuntimeIdentity::Plugin { package, .. } = runtime else {
        panic!("capture retains exact plugin package identity")
    };
    assert_eq!(package.package.as_str(), "capabilities");

    let first = render(&inputs).expect("frozen search render");
    let tree = first.site().output();
    let html = std::str::from_utf8(tree.file(&path("index.html")).expect("Tera output"))
        .expect("UTF-8 HTML");
    assert_eq!(
        html,
        "page|Skip|Home|#ffffff|https:&#x2F;&#x2F;example.test&#x2F;|available|&#x2F;search.json|https:&#x2F;&#x2F;example.test&#x2F;search.json"
    );
    assert!(tree.file(&path("search.json")).is_some());
    let model_bytes = tree.file(&path("model.json")).expect("site model");
    let model: Value = serde_json::from_slice(model_bytes).expect("site model JSON");
    assert_eq!(
        model["capabilities"]["publication.search/v1"],
        serde_json::json!({
            "state": "available",
            "value": {
                "contract": "publication-search-v1",
                "index": "/search.json",
                "format": "gazette-documents-v1"
            }
        })
    );
    let public = String::from_utf8_lossy(model_bytes);
    assert!(!public.contains("private:search-source"));
    assert!(!public.contains("private-revision-7"));
    let capability = "publication.search/v1".parse().expect("capability");
    let build::CapabilityOutcome::Available { evidence } =
        &first.site().evidence().capabilities[&capability].outcome
    else {
        panic!("available private evidence")
    };
    assert_eq!(evidence.source, "private:search-source");
    assert_eq!(evidence.revision, "private-revision-7");
    assert!(first.site().evidence().resources.is_empty());
    let identity = tree.inventory().identity();
    drop(first);

    let replay = render(&inputs).expect("deterministic frozen replay");
    assert_eq!(replay.site().output().inventory().identity(), identity);
    assert_eq!(acquisition.calls.get(), 1, "render never reacquires");
    assert_eq!(reader.reads.get(), 6);
}

#[test]
fn provider_snapshot_requires_exact_membership_before_freeze() {
    let reader = Reader::default();
    let mut inputs = acquired(&reader, &configuration("required", None));
    let acquisition = Capabilities {
        calls: Cell::new(0),
        mode: Mode::Incomplete,
    };
    let error = inputs
        .acquire_capabilities(&acquisition, toml::Limits::default(), Default::default())
        .expect_err("partial provider results cannot freeze");
    assert_eq!(error.code(), "capability_snapshot_invalid");
    assert!(inputs.capability_capture().is_none());
    assert_eq!(acquisition.calls.get(), 1);
}

#[test]
fn adversarial_snapshots_and_reacquisition_are_refused_before_freeze() {
    for mode in [
        Mode::WrongIdentity,
        Mode::Extra,
        Mode::Duplicate,
        Mode::TypedMismatch,
        Mode::InvalidEvidence,
    ] {
        let reader = Reader::default();
        let mut inputs = acquired(&reader, &configuration("required", None));
        let acquisition = Capabilities {
            calls: Cell::new(0),
            mode,
        };
        let error = inputs
            .acquire_capabilities(&acquisition, toml::Limits::default(), Default::default())
            .expect_err("invalid provider snapshot cannot freeze");
        assert_eq!(error.code(), "capability_snapshot_invalid");
        assert!(inputs.capability_capture().is_none());
        assert_eq!(acquisition.calls.get(), 1);
    }

    let reader = Reader::default();
    let mut inputs = acquired(&reader, &configuration("required", None));
    let acquisition = Capabilities {
        calls: Cell::new(0),
        mode: Mode::AvailableSearch,
    };
    inputs
        .acquire_capabilities(&acquisition, toml::Limits::default(), Default::default())
        .expect("first exact capture");
    let error = inputs
        .acquire_capabilities(&acquisition, toml::Limits::default(), Default::default())
        .expect_err("frozen capture cannot be reacquired");
    assert_eq!(error.code(), "capability_capture_exists");
    assert_eq!(acquisition.calls.get(), 1);
}

#[test]
fn frozen_search_route_must_match_the_gazette_emitted_artifact() {
    let reader = Reader::default();
    let mut inputs = acquired(&reader, &configuration("required", None));
    let acquisition = Capabilities {
        calls: Cell::new(0),
        mode: Mode::MismatchedSearch,
    };
    inputs
        .acquire_capabilities(&acquisition, toml::Limits::default(), Default::default())
        .expect("complete typed snapshot freezes before publication parity validation");
    let error = render(&inputs).expect_err("mismatched provider route cannot reach Tera");
    assert_eq!(error.code(), "publication_uncompiled_capability");
    assert_eq!(acquisition.calls.get(), 1);
}

#[test]
fn provider_absence_requires_optional_exact_permission() {
    for (requirement, permission, succeeds) in [
        ("required", None, false),
        ("optional", None, false),
        ("optional", Some("stale"), false),
        ("optional", Some("missing"), true),
    ] {
        let reader = Reader::default();
        let mut inputs = acquired(&reader, &configuration(requirement, permission));
        let acquisition = Capabilities {
            calls: Cell::new(0),
            mode: Mode::Missing,
        };
        inputs
            .acquire_capabilities(&acquisition, toml::Limits::default(), Default::default())
            .expect("explicit unavailable outcomes form a complete snapshot");
        let result = render(&inputs);
        assert_eq!(result.is_ok(), succeeds, "{requirement} {permission:?}");
        if let Err(error) = result {
            assert_eq!(error.code(), "publication_uncompiled_capability");
        }
        assert_eq!(acquisition.calls.get(), 1);
    }
}

#[test]
fn direct_local_static_search_and_unconfigured_absence_share_the_emitted_artifact() {
    for (search, requirement, absence, expected_state) in [
        (true, "required", None, "available"),
        (false, "optional", Some("unconfigured"), "unavailable"),
    ] {
        let reader = Reader::default();
        let configuration = gazette_configuration(
            search,
            "static-search",
            "publication.search/v1",
            requirement,
            absence,
        );
        let mut inputs = acquired_with_reads(&reader, &configuration, 4);
        let configuration_sha256 = inputs.compilation().source_sha256.to_string();
        inputs
            .acquire_direct_local_capabilities(toml::Limits::default(), Default::default())
            .expect("closed Gazette catalog acquisition");
        let capture = inputs.capability_capture().expect("frozen native capture");
        let runtime = capture
            .groups()
            .values()
            .next()
            .expect("Gazette provider group")
            .request
            .provider();
        assert!(matches!(
            runtime,
            ProviderRuntimeIdentity::Gazette { provider }
                if provider.as_str() == "static-search"
        ));
        let rendered = render(&inputs).expect("native capability render");
        let tree = rendered.site().output();
        let model_bytes = tree.file(&path("model.json")).expect("site model");
        let model: Value = serde_json::from_slice(model_bytes).expect("site model JSON");
        assert_eq!(
            model["capabilities"]["publication.search/v1"]["state"],
            expected_state
        );
        assert_eq!(tree.file(&path("search.json")).is_some(), search);
        let public = String::from_utf8_lossy(model_bytes);
        assert!(!public.contains("static-search"));
        assert!(!public.contains("gazette:static-search"));
        let capability = "publication.search/v1".parse().expect("capability");
        assert!(matches!(
            &rendered.site().evidence().capabilities[&capability].runtime,
            ProviderRuntimeIdentity::Gazette { provider }
                if provider.as_str() == "static-search"
        ));
        if search {
            let build::CapabilityOutcome::Available { evidence } =
                &rendered.site().evidence().capabilities[&capability].outcome
            else {
                panic!("enabled native search has available evidence")
            };
            assert_eq!(evidence.source, "gazette:static-search");
            assert_eq!(evidence.revision, configuration_sha256);
        }
    }
}

#[test]
fn direct_local_plugin_is_typed_unsupported_with_exact_runtime_identity() {
    let reader = Reader::default();
    let configuration = configuration("optional", Some("unsupported"))
        .replace("allow_absence='missing'", "allow_absence='unsupported'");
    let mut inputs = acquired(&reader, &configuration);
    inputs
        .acquire_direct_local_capabilities(toml::Limits::default(), Default::default())
        .expect("admitted plugin receives complete unsupported snapshot");
    let rendered = render(&inputs).expect("exact optional absence is permitted");
    assert_eq!(
        rendered.site().evidence().capabilities.len(),
        2,
        "complete exact provider membership"
    );
    for evidence in rendered.site().evidence().capabilities.values() {
        assert!(matches!(
            evidence.runtime,
            ProviderRuntimeIdentity::Plugin { ref package, ref provider }
                if package.package.as_str() == "capabilities" && provider.as_str() == "shared"
        ));
        assert!(matches!(
            evidence.outcome,
            build::CapabilityOutcome::Unavailable {
                reason: CapabilityAbsence::Unsupported
            }
        ));
    }
    let public = String::from_utf8_lossy(
        rendered
            .site()
            .output()
            .file(&path("model.json"))
            .expect("model"),
    );
    assert!(!public.contains("source:plugin"));
    assert!(!public.contains("\"provider\""));
    assert!(!public.contains("\"runtime\""));
    assert!(!public.contains("manifest_sha256"));
}

#[test]
fn unknown_gazette_provider_or_capability_fails_metadata_admission() {
    for (provider, capability, code) in [
        ("unknown", "publication.search/v1", "provider_unavailable"),
        ("static-search", "commerce.selector/v1", "package_selection"),
    ] {
        let reader = Reader::default();
        let configuration =
            gazette_configuration(true, provider, capability, "optional", Some("unsupported"));
        let mut inputs = acquired_with_reads(&reader, &configuration, 4);
        let error = inputs
            .acquire_direct_local_capabilities(toml::Limits::default(), Default::default())
            .expect_err("unknown Gazette catalog binding must fail before dispatch");
        assert_eq!(error.code(), code);
        assert!(inputs.capability_capture().is_none());
    }
}

#[test]
fn direct_local_set_failure_preserves_the_exact_site_association() {
    let configuration = gazette_configuration(
        true,
        "static-search",
        "publication.search/v1",
        "required",
        None,
    )
    .replace(
        "[[brands]]",
        "[[sites]]\nid='site:other'\npublisher='owner:one'\ndeployable=false\n[sites.composition.capabilities]\n'publication.search/v1'={state='enabled',provider={kind='gazette',provider='unknown'},requirement='required'}\n[[brands]]",
    )
    .replace(
        "[[sources]]\nid='source:theme'",
        "[[brands.publications]]\nsite='site:other'\npublic_base_url='https://other.example.test/'\nauthored=[{source='source:content',target='other-authored'}]\nmachine={search='lexical'}\n[[sources]]\nid='source:theme'",
    );
    let reader = Reader::default();
    let sites = [
        "site:one".parse().expect("site"),
        "site:other".parse().expect("site"),
    ];
    let mut inputs = build::acquire_publications(
        &reader,
        &reader,
        &configuration,
        &sites,
        toml::Limits::default(),
        build::PublicationSetAcquisitionLimits {
            sites: 2,
            sources: PreparationLimits {
                sources: 3,
                entries: 8,
                bytes: 4096,
                per_source: source::RevisionLimits::default(),
            },
        },
    )
    .expect("shared exact source capture");
    let error = inputs
        .acquire_direct_local_capabilities(toml::Limits::default(), Default::default())
        .expect_err("one invalid site binding fails the direct-local set");
    let build::PublicationSetError::Capability { site, error } = error else {
        panic!("site-associated capability failure")
    };
    assert_eq!(site.as_str(), "site:other");
    assert_eq!(error.code(), "provider_unavailable");
}

fn browser_configuration() -> String {
    gazette_configuration(true, "static-search", "publication.search/v1", "required", None)
        .replace("[[principals]]", "'publication.analytics/v1'={state='enabled',provider={kind='gazette',provider='google-analytics-client'},requirement='required'}\n'publication.webmcp/v1'={state='enabled',provider={kind='gazette',provider='webmcp-client'},requirement='required'}\n[[principals]]")
        .replace("machine={search='lexical'}", "machine={search='lexical'}\nprocessing={analytics={enabled=true,provider='google-analytics',measurement_id='G-ABC123'},webmcp={enabled=true,tools=['current_page','search']}}")
}

#[test]
fn public_browser_bindings_freeze_replay_without_loading_or_selecting_resources() {
    let reader = Reader::default();
    let mut inputs = acquired_with_reads(&reader, &browser_configuration(), 4);
    inputs
        .acquire_direct_local_capabilities(toml::Limits::default(), Default::default())
        .expect("exact public client bindings freeze without network");
    assert_eq!(inputs.capability_capture().unwrap().groups().len(), 3);
    let first = render(&inputs).expect("same native presentation and model projection");
    let bytes = first.site().output().file(&path("model.json")).unwrap();
    let model: Value = serde_json::from_slice(bytes).unwrap();
    assert_eq!(
        model["capabilities"]["publication.analytics/v1"]["value"],
        serde_json::json!({"contract":"publication-analytics-v1","measurement_id":"G-ABC123"})
    );
    assert_eq!(
        model["capabilities"]["publication.webmcp/v1"]["value"],
        serde_json::json!({"contract":"publication-webmcp-v1","tools":["current_page","search"]})
    );
    let public = String::from_utf8_lossy(bytes);
    for private in [
        "google-analytics-client",
        "webmcp-client",
        "configuration_sha256",
        "gazette:",
    ] {
        assert!(
            !public.contains(private),
            "private provider evidence leaked: {private}"
        );
    }
    let html = String::from_utf8_lossy(first.site().output().file(&path("index.html")).unwrap());
    assert!(!html.contains("googletagmanager"));
    assert!(!html.contains("modelContext"));
    assert_eq!(
        render(&inputs)
            .unwrap()
            .site()
            .output()
            .inventory()
            .identity(),
        first.site().output().inventory().identity()
    );
    assert_eq!(reader.reads.get(), 4, "render cannot acquire again");
}

struct ChangedBrowserBinding {
    change_analytics: bool,
}

impl CapabilityAcquisition for ChangedBrowserBinding {
    fn acquire(
        &self,
        request: &AcquisitionRequest,
    ) -> Result<ProviderSnapshot, gazette_ports::capability::Failure> {
        let results = request
            .capabilities()
            .iter()
            .map(|capability| {
                let value = match capability.as_str() {
                    "publication.analytics/v1" => CapabilityValue::PublicationAnalyticsV1 {
                        measurement_id: if self.change_analytics {
                            "G-OTHER"
                        } else {
                            "G-ABC123"
                        }
                        .parse()
                        .unwrap(),
                    },
                    "publication.webmcp/v1" => CapabilityValue::PublicationWebMcpV1 {
                        tools: vec![gazette_core::publication::browser::WebMcpTool::CurrentPage]
                            .try_into()
                            .unwrap(),
                    },
                    "publication.search/v1" => CapabilityValue::PublicationSearchV1 {
                        index: "/search.json".parse().unwrap(),
                        format: SearchIndexFormat::GazetteDocumentsV1,
                    },
                    _ => panic!("unexpected selected contract"),
                };
                CapabilityResult {
                    capability: capability.clone(),
                    outcome: CapabilityResultOutcome::Available {
                        value,
                        evidence: ProviderEvidence {
                            source: "fixture:exact-provider".into(),
                            revision: "revision-one".into(),
                        },
                    },
                }
            })
            .collect();
        Ok(ProviderSnapshot::new(request.provider().clone(), results))
    }
}

#[test]
fn provider_cannot_replace_measurement_or_tool_selection_and_partial_capture_is_refused() {
    for change_analytics in [true, false] {
        let reader = Reader::default();
        let mut inputs = acquired_with_reads(&reader, &browser_configuration(), 4);
        let error = inputs
            .acquire_capabilities(
                &ChangedBrowserBinding { change_analytics },
                toml::Limits::default(),
                Default::default(),
            )
            .expect_err("exact configured public binding is immutable");
        assert_eq!(error.code(), "capability_snapshot_invalid");
        assert!(error.to_string().contains("exact configured binding"));
        assert!(
            inputs.capability_capture().is_none(),
            "no partial provider capture"
        );
    }
}

#[test]
fn native_browser_provider_must_implement_the_exact_selected_contract() {
    let reader = Reader::default();
    let configuration = browser_configuration().replace(
        "provider='webmcp-client'",
        "provider='google-analytics-client'",
    );
    let mut inputs = acquired_with_reads(&reader, &configuration, 4);
    let error = inputs
        .acquire_direct_local_capabilities(toml::Limits::default(), Default::default())
        .expect_err("wrong native provider cannot satisfy the tool contract");
    assert_eq!(error.code(), "package_selection");
    assert!(inputs.capability_capture().is_none());
}
