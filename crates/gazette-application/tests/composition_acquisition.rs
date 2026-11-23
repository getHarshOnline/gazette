//! Observable application ordering/budgets; native adapter behavior is tested by CLI fixtures.

use std::{cell::RefCell, collections::BTreeMap, path::Path, sync::Arc};

use gazette_application::{build, source::PreparationLimits};
use gazette_compiler::toml;
use gazette_core::source::{
    CapturedSource, CommitId, Content, Error, Inventory, InventoryBuilder, Limits, MetadataBudget,
    RevisionLimits, Sha256Digest, Subdirectory,
};
use gazette_ports::source::{RevisionReader, SourceCapture, SourceReader};

const CONFIG: &str = r#"
[composition]
theme = {package='theme:base'}
[composition.plugins]
'plugin-a'={state='enabled'}
'plugin-b'={state='enabled'}
[composition.capabilities]
'commerce.catalog/v1' = {state='enabled',provider={kind='plugin',package='plugin-a',provider='shared'},requirement='required'}
'commerce.inventory/v1' = {state='enabled',provider={kind='plugin',package='plugin-b',provider='other'},requirement='required'}
[[principals]]
id='owner:one'
kind='organization'
[[sites]]
id='site:one'
publisher='owner:one'
deployable=false
[[sources]]
id='source:a'
location={kind='repository',root='/fixture/a',commit='1111111111111111111111111111111111111111',subdirectory='inputs'}
[[sources]]
id='source:b'
location={kind='repository',root='/fixture/b',commit='1111111111111111111111111111111111111111',subdirectory='inputs'}
[[theme_packages]]
package='theme:base'
source='source:a'
manifest='theme.toml'
[[plugin_packages]]
package='plugin-a'
source='source:a'
manifest='plugin.toml'
[[plugin_packages]]
package='plugin-b'
source='source:b'
manifest='plugin.toml'
"#;

const GAZETTE_PROVIDER_CONFIG: &str = r#"
[composition]
theme = {package='theme:base'}
[composition.capabilities]
'publication.search/v1' = {state='enabled',provider={kind='gazette',provider='native-search'},requirement='required'}
[[principals]]
id='owner:one'
kind='organization'
[[sites]]
id='site:one'
publisher='owner:one'
deployable=false
[[sources]]
id='source:a'
location={kind='repository',root='/fixture/a',commit='1111111111111111111111111111111111111111',subdirectory='inputs'}
[[sources]]
id='source:b'
location={kind='repository',root='/fixture/b',commit='1111111111111111111111111111111111111111',subdirectory='inputs'}
[[theme_packages]]
package='theme:base'
source='source:a'
manifest='theme.toml'
[[plugin_packages]]
package='plugin-b'
source='source:b'
manifest='plugin.toml'
"#;

const THEME_BASE: &[u8] =
    b"package='theme:base'\nnamespace='base'\ntemplates='templates'\nbrowser_theme_colors={light='#ffffff',dark='#000000'}\nrender={page='page.html'}\n";
const THEME_STABLE: &[u8] = b"package='theme:stable'\nnamespace='stable'\ntemplates='templates'\nbrowser_theme_colors={light='#ffffff',dark='#000000'}\nrender={page='page.html'}\n";
const THEME_AUTHORED: &[u8] = b"package='theme:authored'\nnamespace='authored'\ntemplates='templates'\nbrowser_theme_colors={light='#ffffff',dark='#000000'}\nrender={page='page.html'}\n";
const THEME_DEPENDENT: &[u8] = b"package='theme:dependent'\nnamespace='dependent'\ntemplates='templates'\nbrowser_theme_colors={light='#ffffff',dark='#000000'}\nrender={page='page.html'}\ndependencies=[{package='theme:shared'}]\n";
const THEME_SHARED: &[u8] = b"package='theme:shared'\nnamespace='shared'\ntemplates='templates'\n";
const PLUGIN_A: &[u8] = b"package='plugin-a'\n[providers.shared]\nimplements=['commerce.catalog/v1']\n[providers.other]\nimplements=['commerce.inventory/v1']\n[providers.later]\nimplements=['commerce.selector/v1']\n";
const PLUGIN_B: &[u8] = b"package='plugin-b'\n[providers.shared]\nimplements=['commerce.catalog/v1']\n[providers.other]\nimplements=['commerce.inventory/v1']\n[providers.later]\nimplements=['commerce.selector/v1']\n";
const PLUGIN_C: &[u8] = b"package='plugin-c'\n[providers.shared]\nimplements=['commerce.catalog/v1']\n[providers.other]\nimplements=['commerce.inventory/v1']\n[providers.later]\nimplements=['commerce.selector/v1']\n";

const EXTERNAL_FACT: &str = "\n[[fact_sources]]\ntarget={kind='principal-name',principal='owner:one'}\nsource={kind='external',binding='input:weave'}\n";

#[test]
fn resolved_build_selection_cannot_change_its_workspace_policy_before_capture() {
    use gazette_compiler::workspace::LocalDrafts;

    let source = format!("[build]\nsites=['site:one']\nlocal_drafts='include'\n{CONFIG}");
    let configuration = gazette_application::configuration::resolve(&source, Default::default())
        .expect("retained configured workspace");
    let selected = configuration
        .compilation()
        .workspace
        .build_selection(None)
        .expect("exact configured intent");
    let mut cases = Vec::new();
    let mut changed_policy = selected.clone();
    changed_policy.local_drafts = Some(LocalDrafts::Exclude);
    cases.push(changed_policy);
    let mut absent_policy = selected.clone();
    absent_policy.local_drafts = None;
    cases.push(absent_policy);
    let mut empty = selected.clone();
    empty.sites.clear();
    cases.push(empty);
    let mut unrelated = selected.clone();
    unrelated.sites[0] = "site:unknown".parse().unwrap();
    cases.push(unrelated);
    let mut repeated = selected.clone();
    repeated.sites.push(selected.sites[0].clone());
    cases.push(repeated);
    for selection in cases {
        let reader = Recording::default();
        for result in [
            build::acquire_selected_publications(
                &reader,
                &reader,
                configuration.clone(),
                &selection,
                Default::default(),
            ),
            build::acquire_development_selected_publications(
                &reader,
                &reader,
                configuration.clone(),
                &selection,
                Default::default(),
            ),
        ] {
            assert!(matches!(
                result,
                Err(build::PublicationSetError::Selection(_))
            ));
        }
        assert!(
            reader.reads.borrow().is_empty(),
            "invalid intent performs no native capture"
        );
    }
    let reader = Recording::default();
    let bounded = build::PublicationSetAcquisitionLimits {
        sites: 0,
        ..Default::default()
    };
    assert!(matches!(
        build::acquire_selected_publications(&reader, &reader, configuration, &selected, bounded,),
        Err(build::PublicationSetError::Selection(_))
    ));
    assert!(reader.reads.borrow().is_empty());
}

#[test]
fn unresolved_facts_stop_every_shared_acquisition_before_native_reads() {
    let config = format!(
        "{CONFIG}{EXTERNAL_FACT}\n[[brands]]\nid='brand:one'\nname='Local brand'\n[[brands.publications]]\nsite='site:one'\nauthored=[{{source='source:a',target='content'}}]\n"
    );
    let reader = Recording::default();
    let site = "site:one".parse().expect("site");
    let assert_unavailable = |error: build::CompositionError| {
        let build::CompositionError::Facts(error) = error else {
            panic!("exact unresolved fact outcome")
        };
        assert_eq!(error.code(), "publication_facts_unavailable");
        assert_eq!(error.target.principal().as_str(), "owner:one");
        assert_eq!(error.binding.as_str(), "input:weave");
    };
    assert_unavailable(acquire(&reader, &config, limits()).expect_err("composition"));
    assert_unavailable(
        build::acquire_publication(
            &reader,
            &reader,
            &config,
            &site,
            toml::Limits::default(),
            limits(),
        )
        .expect_err("publication"),
    );
    for result in [
        build::acquire_publications(
            &reader,
            &reader,
            &config,
            std::slice::from_ref(&site),
            toml::Limits::default(),
            build::PublicationSetAcquisitionLimits::default(),
        ),
        build::acquire_development_publications(
            &reader,
            &reader,
            &config,
            std::slice::from_ref(&site),
            toml::Limits::default(),
            build::PublicationSetAcquisitionLimits::default(),
        ),
    ] {
        let build::PublicationSetError::Acquisition(error) = result.expect_err("complete set")
        else {
            panic!("shared acquisition refusal")
        };
        assert_unavailable(error);
    }
    assert!(matches!(
        build::acquire_site(
            &reader,
            &reader,
            &config,
            &site,
            toml::Limits::default(),
            limits()
        ),
        Err(build::Error::Facts(_))
    ));
    assert!(
        reader.reads.borrow().is_empty(),
        "no source or revision read before exact unavailable"
    );
}

#[test]
fn explicit_local_fact_selection_reuses_local_name_and_existing_acquisition() {
    let local = CONFIG.replace(
        "kind='organization'",
        "kind='organization'\nname='Local owner'",
    );
    let config = format!(
        "{local}\n[[fact_sources]]\ntarget={{kind='principal-name',principal='owner:one'}}\nsource={{kind='local'}}\n"
    );
    let reader = Recording::default();
    let acquired = acquire(&reader, &config, limits()).expect("existing capture path");
    assert!(!reader.reads.borrow().is_empty());
    assert_eq!(
        acquired
            .compilation()
            .workspace
            .principals()
            .next()
            .expect("owner")
            .name
            .as_deref(),
        Some("Local owner")
    );
}

#[derive(Debug, PartialEq, Eq)]
struct Read {
    kind: &'static str,
    root: String,
    entries: usize,
    bytes: u64,
}

#[derive(Default)]
struct Recording {
    reads: RefCell<Vec<Read>>,
    mismatch: bool,
    mismatched_roots: Vec<&'static str>,
}

impl Recording {
    fn record(&self, kind: &'static str, root: &Path, limits: Limits) {
        self.reads.borrow_mut().push(Read {
            kind,
            root: root.to_str().expect("fixture root").to_owned(),
            entries: limits.entries,
            bytes: limits.total_bytes,
        });
    }
}

fn fixture_files(root: &Path) -> BTreeMap<gazette_core::source::EntryPath, Vec<u8>> {
    let (theme, plugin) = match root.to_str() {
        Some("/fixture/a") => (Some(THEME_BASE), Some(PLUGIN_A)),
        Some("/fixture/b") => (None, Some(PLUGIN_B)),
        Some("/fixture/c") => (Some(THEME_STABLE), Some(PLUGIN_C)),
        Some("/fixture/authored") => (Some(THEME_AUTHORED), None),
        Some("/fixture/dependent") => (Some(THEME_DEPENDENT), None),
        Some("/fixture/shared") => (Some(THEME_SHARED), None),
        Some("/fixture/unrelated") => (Some(THEME_STABLE), None),
        other => panic!("unexpected fixture root: {other:?}"),
    };
    let mut files = BTreeMap::from([("input".parse().expect("fixture path"), b"data".to_vec())]);
    if let Some(theme) = theme {
        files.insert(
            "theme.toml".parse().expect("Theme manifest path"),
            theme.to_vec(),
        );
    }
    if let Some(plugin) = plugin {
        files.insert(
            "plugin.toml".parse().expect("plugin manifest path"),
            plugin.to_vec(),
        );
    }
    files
}

fn fixture_bytes(root: &Path) -> u64 {
    fixture_files(root)
        .values()
        .map(|bytes| bytes.len() as u64)
        .sum()
}

fn fixture_entries(root: &Path) -> usize {
    fixture_files(root).len()
}

fn inventory(
    files: &BTreeMap<gazette_core::source::EntryPath, Vec<u8>>,
    limits: Limits,
) -> Result<Inventory, Error> {
    let mut builder = InventoryBuilder::new(limits);
    for (path, bytes) in files {
        builder.insert(
            path.clone(),
            Content::File {
                bytes: bytes.len() as u64,
                sha256: Sha256Digest::of(bytes),
            },
        )?;
    }
    Ok(builder.finish())
}

impl SourceReader for Recording {
    fn read(&self, _: &Path, _: Option<&Subdirectory>, _: Limits) -> Result<Inventory, Error> {
        panic!("acquisition must retain bytes, not reopen an inventory-only observation")
    }
}

impl SourceCapture for Recording {
    fn capture(
        &self,
        root: &Path,
        subdirectory: Option<&Subdirectory>,
        limits: Limits,
    ) -> Result<CapturedSource, Error> {
        assert_eq!(
            subdirectory,
            Some(&"inputs".parse().expect("fixture subtree"))
        );
        self.record("capture", root, limits);
        let files = fixture_files(root);
        CapturedSource::from_parts(inventory(&files, limits)?, files)
    }
}

impl RevisionReader for Recording {
    fn read_revision(
        &self,
        root: &Path,
        subdirectory: &Subdirectory,
        commit: &CommitId,
        limits: Limits,
        _: &mut MetadataBudget,
    ) -> Result<Inventory, Error> {
        assert_eq!(subdirectory, &"inputs".parse().expect("fixture subtree"));
        assert_eq!(commit.as_str(), "1111111111111111111111111111111111111111");
        self.record("revision", root, limits);
        let mismatch = (self.mismatch && root == Path::new("/fixture/b"))
            || self
                .mismatched_roots
                .iter()
                .any(|path| root == Path::new(path));
        let mut files = fixture_files(root);
        if mismatch {
            files.insert("input".parse().expect("fixture path"), b"diff".to_vec());
        }
        inventory(&files, limits)
    }

    fn read_gitlink(
        &self,
        _: &Path,
        _: &CommitId,
        _: &Subdirectory,
        _: &mut MetadataBudget,
    ) -> Result<CommitId, Error> {
        panic!("repository selections must not invent gitlink discovery")
    }
}

fn limits() -> PreparationLimits {
    let first = Path::new("/fixture/a");
    let second = Path::new("/fixture/b");
    let third = Path::new("/fixture/c");
    PreparationLimits {
        sources: 2,
        entries: fixture_entries(first) + fixture_entries(second).max(fixture_entries(third)),
        bytes: fixture_bytes(first) + fixture_bytes(second).max(fixture_bytes(third)),
        per_source: RevisionLimits::default(),
    }
}

fn acquire(
    reader: &Recording,
    config: &str,
    limits: PreparationLimits,
) -> Result<build::AcquiredInputs, build::CompositionError> {
    build::acquire_composition(
        reader,
        reader,
        config,
        &"site:one".parse().expect("site"),
        toml::Limits::default(),
        limits,
    )
}

#[test]
fn gazette_provider_does_not_capture_a_plugin_and_fails_explicitly_at_admission() {
    let reader = Recording::default();
    let root = Path::new("/fixture/a");
    let acquired = acquire(
        &reader,
        GAZETTE_PROVIDER_CONFIG,
        PreparationLimits {
            sources: 1,
            entries: fixture_entries(root),
            bytes: fixture_bytes(root),
            per_source: RevisionLimits::default(),
        },
    )
    .expect("Gazette-owned provider adds no plugin source acquisition");
    assert_eq!(
        reader
            .reads
            .borrow()
            .iter()
            .map(|read| (read.kind, read.root.as_str()))
            .collect::<Vec<_>>(),
        [("capture", "/fixture/a"), ("revision", "/fixture/a")]
    );
    let error = acquired
        .check_metadata(toml::Limits::default())
        .expect_err("native Gazette provider catalog is not implemented");
    assert_eq!(error.code(), "provider_unavailable");
    assert!(error.to_string().contains("unsupported or unconfigured"));
    assert!(error.to_string().contains("native-search"));
}

#[test]
fn configuration_and_source_count_fail_before_any_port_read() {
    let reader = Recording::default();
    let invalid = CONFIG.replace(
        "theme = {package='theme:base'}",
        "theme = {package='theme:missing'}",
    );
    assert!(matches!(
        acquire(&reader, &invalid, limits()),
        Err(build::CompositionError::Configuration(_))
    ));
    assert!(reader.reads.borrow().is_empty());
    assert!(matches!(
        acquire(
            &reader,
            CONFIG,
            PreparationLimits {
                sources: 1,
                ..limits()
            }
        ),
        Err(build::CompositionError::TooManySources { limit: 1 })
    ));
    assert!(reader.reads.borrow().is_empty());
}

#[test]
fn shared_keys_are_captured_once_with_remaining_bounds_before_each_read() {
    let reader = Recording::default();
    let acquired = acquire(&reader, CONFIG, limits()).expect("complete set");
    assert_eq!(acquired.sources().len(), 2);
    let budget = limits();
    let expected: Vec<_> = [
        ("/fixture/a", budget.entries, budget.bytes),
        (
            "/fixture/b",
            budget.entries - fixture_entries(Path::new("/fixture/a")),
            budget.bytes - fixture_bytes(Path::new("/fixture/a")),
        ),
    ]
    .into_iter()
    .flat_map(|(root, entries, bytes)| {
        ["capture", "revision"].map(|kind| Read {
            kind,
            root: root.to_owned(),
            entries,
            bytes,
        })
    })
    .collect();
    assert_eq!(*reader.reads.borrow(), expected);
    for input in acquired.sources().values() {
        assert_eq!(
            input.file(&"input".parse().expect("path")),
            Some(b"data".as_slice())
        );
    }
}

#[test]
fn filesystem_source_is_captured_once_without_git_lineage_or_revision_reads() {
    let config = "[composition]\n\
theme={package='theme:authored'}\n\
[[principals]]\n\
id='owner:one'\n\
kind='organization'\n\
[[sites]]\n\
id='site:one'\n\
publisher='owner:one'\n\
deployable=false\n\
[[sources]]\n\
id='source:authored'\n\
location={kind='filesystem',root='/fixture/authored',subdirectory='inputs'}\n\
[[theme_packages]]\n\
package='theme:authored'\n\
source='source:authored'\n\
manifest='theme.toml'\n";
    let reader = Recording::default();
    let acquired = acquire(&reader, config, limits()).expect("retained filesystem tree");
    assert_eq!(
        *reader.reads.borrow(),
        [Read {
            kind: "capture",
            root: "/fixture/authored".into(),
            entries: limits().entries,
            bytes: limits().bytes,
        }]
    );
    let input = acquired
        .sources()
        .get(&"source:authored".parse().expect("source key"))
        .expect("captured source");
    assert!(matches!(
        input,
        gazette_application::source::AcquiredInput::Filesystem(_)
    ));
    assert_eq!(
        input.file(&"input".parse().expect("path")),
        Some(b"data".as_slice())
    );
    let evidence = serde_json::to_value(acquired.inspect()).expect("private evidence");
    let source = &evidence["sources"]["source:authored"];
    assert_eq!(source["kind"], "filesystem");
    assert!(source.get("observation").is_some());
    assert!(source.get("verification").is_none());
    assert!(!source.to_string().contains("commit"));
}

#[test]
fn late_revision_failure_returns_no_partial_acquisition() {
    let reader = Recording {
        mismatch: true,
        ..Recording::default()
    };
    let error = acquire(&reader, CONFIG, limits()).expect_err("late revision mismatch");
    assert!(
        matches!(error, build::CompositionError::Source { binding, error: Error::ContentMismatch(_) } if binding.as_str() == "source:b")
    );
    assert_eq!(
        reader.reads.borrow().len(),
        4,
        "first capture and verification completed before the second failed"
    );
}

fn publication_config() -> String {
    format!(
        "{CONFIG}{}",
        r#"
[[sites]]
id='site:two'
publisher='owner:one'
deployable=false
[[sites]]
id='site:unrequested'
publisher='owner:one'
deployable=false
[sites.composition]
theme={package='theme:base'}
[[brands]]
id='brand:one'
name='Publication fixture'
[[brands.publications]]
site='site:one'
authored=[{source='source:a',target='one'}]
[[brands.publications]]
site='site:two'
authored=[{source='source:a',target='two'}]
"#
    )
}

fn acquire_publications(
    reader: &Recording,
    config: &str,
    sites: &[&str],
    limits: build::PublicationSetAcquisitionLimits,
) -> Result<build::AcquiredPublicationSet, build::PublicationSetError> {
    let sites: Vec<_> = sites
        .iter()
        .map(|site| site.parse().expect("fixture site key"))
        .collect();
    build::acquire_publications(
        reader,
        reader,
        config,
        &sites,
        toml::Limits::default(),
        limits,
    )
}

fn publication_limits() -> build::PublicationSetAcquisitionLimits {
    build::PublicationSetAcquisitionLimits {
        sites: 2,
        sources: limits(),
    }
}

#[test]
fn publication_site_views_share_one_capture_and_ignore_unrequested_package() {
    let reader = Recording::default();
    let acquired = acquire_publications(
        &reader,
        &publication_config(),
        &["site:two", "site:one"],
        publication_limits(),
    )
    .expect("exact two-site acquisition");
    assert_eq!(
        acquired
            .sites()
            .keys()
            .map(|key| key.as_str())
            .collect::<Vec<_>>(),
        ["site:one", "site:two"]
    );
    let first = acquired.sites()[&"site:one".parse().expect("first site")]
        .as_ref()
        .expect("first site acquired");
    let second = acquired.sites()[&"site:two".parse().expect("second site")]
        .as_ref()
        .expect("second site acquired");
    assert_eq!(first.site().as_str(), "site:one");
    assert_eq!(second.site().as_str(), "site:two");
    assert!(std::ptr::eq(first.compilation(), second.compilation()));
    assert!(std::ptr::eq(first.sources(), second.sources()));
    assert_eq!(first.sources().len(), 2);
    let budget = limits();
    let expected: Vec<_> = [
        ("/fixture/a", budget.entries, budget.bytes),
        (
            "/fixture/b",
            budget.entries - fixture_entries(Path::new("/fixture/a")),
            budget.bytes - fixture_bytes(Path::new("/fixture/a")),
        ),
    ]
    .into_iter()
    .flat_map(|(root, entries, bytes)| {
        ["capture", "revision"].map(|kind| Read {
            kind,
            root: root.to_owned(),
            entries,
            bytes,
        })
    })
    .collect();
    assert_eq!(*reader.reads.borrow(), expected);
    let source = "source:a".parse().expect("shared source");
    let path = "input".parse().expect("retained path");
    let first_bytes = first.sources()[&source]
        .file(&path)
        .expect("first retained bytes");
    let second_bytes = second.sources()[&source]
        .file(&path)
        .expect("second retained bytes");
    assert_eq!(first_bytes, b"data");
    assert!(std::ptr::eq(first_bytes, second_bytes));
}

#[test]
fn selected_theme_acquires_only_its_declared_cross_source_dependency_closure() {
    let config = r#"
[composition]
theme={package='theme:dependent'}
[[principals]]
id='owner:one'
kind='organization'
[[sites]]
id='site:one'
publisher='owner:one'
deployable=false
[[sources]]
id='source:dependent'
location={kind='repository',root='/fixture/dependent',commit='1111111111111111111111111111111111111111',subdirectory='inputs'}
[[sources]]
id='source:shared'
location={kind='repository',root='/fixture/shared',commit='1111111111111111111111111111111111111111',subdirectory='inputs'}
[[sources]]
id='source:unrelated'
location={kind='repository',root='/fixture/unrelated',commit='1111111111111111111111111111111111111111',subdirectory='inputs'}
[[theme_packages]]
package='theme:dependent'
source='source:dependent'
manifest='theme.toml'
[[theme_packages]]
package='theme:shared'
source='source:shared'
manifest='theme.toml'
[[theme_packages]]
package='theme:stable'
source='source:unrelated'
manifest='theme.toml'
"#;
    let reader = Recording::default();
    let acquired = acquire(&reader, config, limits()).expect("declared Theme graph");
    assert_eq!(acquired.sources().len(), 2);
    assert!(
        acquired
            .sources()
            .contains_key(&"source:dependent".parse().expect("root source"))
    );
    assert!(
        acquired
            .sources()
            .contains_key(&"source:shared".parse().expect("dependency source"))
    );
    assert!(
        reader
            .reads
            .borrow()
            .iter()
            .all(|read| read.root != "/fixture/unrelated")
    );
}

#[test]
fn invalid_publication_sets_fail_before_capture_or_revision_ports() {
    let reader = Recording::default();
    let config = publication_config();
    for (sites, limit) in [
        (vec![], publication_limits()),
        (vec!["site:one", "site:one"], publication_limits()),
        (
            vec!["site:one", "site:two"],
            build::PublicationSetAcquisitionLimits {
                sites: 1,
                ..publication_limits()
            },
        ),
    ] {
        assert!(matches!(
            acquire_publications(&reader, &config, &sites, limit),
            Err(build::PublicationSetError::Selection(_))
        ));
        assert!(reader.reads.borrow().is_empty());
    }
    let unknown = acquire_publications(
        &reader,
        &config,
        &["site:one", "site:missing"],
        publication_limits(),
    )
    .expect_err("unknown site is refused by effective selection before acquisition");
    assert_eq!(unknown.code(), "publication_set_selection");
    assert!(matches!(
        unknown,
        build::PublicationSetError::Selection("effective build selection names an undeclared site")
    ));
    assert!(reader.reads.borrow().is_empty());
    assert!(matches!(
        acquire_publications(
            &reader,
            &config,
            &["site:unrequested"],
            publication_limits()
        ),
        Err(build::PublicationSetError::Acquisition(
            build::CompositionError::MissingPublication(_)
        ))
    ));
    assert!(reader.reads.borrow().is_empty());
    assert!(matches!(
        acquire_publications(
            &reader,
            &config,
            &["site:one", "site:two"],
            build::PublicationSetAcquisitionLimits {
                sources: PreparationLimits {
                    sources: 1,
                    ..limits()
                },
                ..publication_limits()
            },
        ),
        Err(build::PublicationSetError::Acquisition(
            build::CompositionError::TooManySources { limit: 1 }
        ))
    ));
    assert!(reader.reads.borrow().is_empty());
}

#[test]
fn publication_set_passes_remaining_whole_set_bounds_to_later_capture() {
    let budget = limits();
    let first_entries = fixture_entries(Path::new("/fixture/a"));
    let first_bytes = fixture_bytes(Path::new("/fixture/a"));
    let second_entries = fixture_entries(Path::new("/fixture/b"));
    let second_bytes = fixture_bytes(Path::new("/fixture/b"));
    for (source_limits, remaining_entries, remaining_bytes) in [
        (
            PreparationLimits {
                bytes: first_bytes + second_bytes - 1,
                ..budget
            },
            budget.entries - first_entries,
            second_bytes - 1,
        ),
        (
            PreparationLimits {
                entries: first_entries + second_entries - 1,
                ..budget
            },
            second_entries - 1,
            budget.bytes - first_bytes,
        ),
    ] {
        let reader = Recording::default();
        let acquired = acquire_publications(
            &reader,
            &publication_config(),
            &["site:one", "site:two"],
            build::PublicationSetAcquisitionLimits {
                sources: source_limits,
                ..publication_limits()
            },
        )
        .expect("package-source failure remains a site outcome");
        assert_eq!(acquired.sites().len(), 2);
        assert_eq!(
            acquired.sources().len(),
            1,
            "successful content remains retained"
        );
        assert_eq!(acquired.source_failures().len(), 1);
        for outcome in acquired.sites().values() {
            let error = outcome
                .as_ref()
                .expect_err("both sites need the failed provider");
            assert!(
                matches!(error.as_ref(), build::CompositionError::Source { binding, .. }
                if binding.as_str() == "source:b")
            );
        }
        let reads = reader.reads.borrow();
        assert_eq!(
            reads.len(),
            3,
            "second capture fails before revision verification"
        );
        assert_eq!(reads[0].root, "/fixture/a");
        assert_eq!(reads[1].kind, "revision");
        assert_eq!(
            reads[2],
            Read {
                kind: "capture",
                root: "/fixture/b".to_owned(),
                entries: remaining_entries,
                bytes: remaining_bytes,
            }
        );
    }
}

fn isolated_package_config() -> String {
    let config = publication_config();
    format!(
        "{config}{}",
        r#"
[[sources]]
id='source:c'
location={kind='repository',root='/fixture/c',commit='1111111111111111111111111111111111111111',subdirectory='inputs'}
[[theme_packages]]
package='theme:stable'
source='source:c'
manifest='theme.toml'
[[plugin_packages]]
package='plugin-c'
source='source:c'
manifest='plugin.toml'
[[sites]]
id='site:healthy'
publisher='owner:one'
deployable=false
[sites.composition]
theme={package='theme:stable'}
[sites.composition.plugins]
'plugin-c'={state='enabled'}
'plugin-b'={state='disabled'}
[sites.composition.capabilities]
'commerce.inventory/v1'={state='enabled',provider={kind='plugin',package='plugin-c',provider='other'},requirement='required'}
[[brands.publications]]
site='site:healthy'
authored=[{source='source:a',target='healthy'}]
"#
    )
}

fn isolated_package_limits() -> build::PublicationSetAcquisitionLimits {
    build::PublicationSetAcquisitionLimits {
        sites: 3,
        sources: PreparationLimits {
            sources: 3,
            ..limits()
        },
    }
}

fn package_reads() -> Vec<Read> {
    let budget = limits();
    let remaining_entries = budget.entries - fixture_entries(Path::new("/fixture/a"));
    let remaining_bytes = budget.bytes - fixture_bytes(Path::new("/fixture/a"));
    [
        ("/fixture/a", budget.entries, budget.bytes),
        ("/fixture/b", remaining_entries, remaining_bytes),
        ("/fixture/c", remaining_entries, remaining_bytes),
    ]
    .into_iter()
    .flat_map(|(root, entries, bytes)| {
        ["capture", "revision"].map(|kind| Read {
            kind,
            root: root.to_owned(),
            entries,
            bytes,
        })
    })
    .collect()
}

#[test]
fn failed_shared_plugin_is_captured_once_and_later_healthy_source_keeps_remaining_budget() {
    let reader = Recording {
        mismatch: true,
        ..Recording::default()
    };
    let acquired = acquire_publications(
        &reader,
        &isolated_package_config(),
        &["site:two", "site:healthy", "site:one"],
        isolated_package_limits(),
    )
    .expect("one package-source failure does not abort other source acquisition");
    assert_eq!(
        *reader.reads.borrow(),
        package_reads(),
        "one read per key; retained source:a is neither retried nor refunded after source:b failure"
    );
    assert_eq!(
        acquired
            .sites()
            .keys()
            .map(|key| key.as_str())
            .collect::<Vec<_>>(),
        ["site:healthy", "site:one", "site:two"]
    );
    assert_eq!(
        acquired
            .sources()
            .keys()
            .map(|key| key.as_str())
            .collect::<Vec<_>>(),
        ["source:a", "source:c"]
    );
    assert_eq!(acquired.source_failures().len(), 1);
    let failed = &acquired.source_failures()[&"source:b".parse().expect("failed source")];
    assert!(matches!(failed.as_ref(), build::CompositionError::Source {
        binding, error: Error::ContentMismatch(_),
    } if binding.as_str() == "source:b"));
    for site in ["site:one", "site:two"] {
        let error = acquired.sites()[&site.parse().expect("dependent site")]
            .as_ref()
            .expect_err("shared plugin package failed");
        assert!(
            Arc::ptr_eq(error, failed),
            "one failure observation fans out by identity"
        );
    }
    let healthy = acquired.sites()[&"site:healthy".parse().expect("healthy site")]
        .as_ref()
        .expect("later independent package selection acquired");
    assert!(std::ptr::eq(healthy.sources(), acquired.sources()));
    assert!(std::ptr::eq(healthy.compilation(), acquired.compilation()));
    assert_eq!(
        healthy.sources()[&"source:c".parse().expect("healthy source")]
            .file(&"input".parse().expect("retained path")),
        Some(b"data".as_slice())
    );
}

#[test]
fn all_failed_sites_keep_exact_membership_and_every_failed_source() {
    let reader = Recording {
        mismatched_roots: vec!["/fixture/b", "/fixture/c"],
        ..Recording::default()
    };
    let config = isolated_package_config().replace(
        "[composition.capabilities]",
        "[composition.capabilities]\n'commerce.selector/v1'={state='enabled',provider={kind='plugin',package='plugin-c',provider='later'},requirement='required'}",
    )
    .replace(
        "[composition.plugins]",
        "[composition.plugins]\n'plugin-c'={state='enabled'}",
    );
    let acquired = acquire_publications(
        &reader,
        &config,
        &["site:two", "site:one", "site:healthy"],
        isolated_package_limits(),
    )
    .expect("all package failures are explicit outcomes, not a vanished site set");
    assert_eq!(*reader.reads.borrow(), package_reads());
    assert_eq!(
        acquired
            .source_failures()
            .keys()
            .map(|key| key.as_str())
            .collect::<Vec<_>>(),
        ["source:b", "source:c"]
    );
    assert_eq!(
        acquired.sources().len(),
        1,
        "complete content is still retained"
    );
    assert_eq!(acquired.sites().len(), 3);
    for (site, first_failed) in [
        ("site:one", "source:b"),
        ("site:two", "source:b"),
        ("site:healthy", "source:c"),
    ] {
        let error = acquired.sites()[&site.parse().expect("site key")]
            .as_ref()
            .expect_err("every requested site has a package-source failure");
        assert!(
            Arc::ptr_eq(
                error,
                &acquired.source_failures()[&first_failed.parse().expect("source key")]
            ),
            "site chooses its first failed dependency in stable key order; other failures stay in the ledger"
        );
    }
}

#[test]
fn required_content_failure_is_global_before_lexically_earlier_package_reads() {
    let reader = Recording {
        mismatch: true,
        ..Recording::default()
    };
    let config = isolated_package_config().replace(
        "authored=[{source='source:a'",
        "authored=[{source='source:b'",
    );
    let result = acquire_publications(
        &reader,
        &config,
        &["site:healthy", "site:one", "site:two"],
        isolated_package_limits(),
    );
    assert!(matches!(result,
        Err(build::PublicationSetError::Acquisition(build::CompositionError::Source {
            binding, error: Error::ContentMismatch(_),
        })) if binding.as_str() == "source:b"));
    assert_eq!(
        *reader.reads.borrow(),
        ["capture", "revision"].map(|kind| Read {
            kind,
            root: "/fixture/b".to_owned(),
            entries: limits().entries,
            bytes: limits().bytes,
        }),
        "required content is verified before source:a or source:c package reads"
    );
}
