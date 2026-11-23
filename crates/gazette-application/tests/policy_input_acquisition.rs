//! Real acquisition/metadata owners with observable retained-source ports; native Git is
//! qualified separately by connected SDK/CLI preparation fixtures.

use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use gazette_application::{build, source::PreparationLimits};
use gazette_core::source::{
    CapturedSource, CommitId, Content, EntryPath, Error, Inventory, InventoryBuilder, Limits,
    MetadataBudget, Sha256Digest, Subdirectory,
};
use gazette_ports::source::{RevisionReader, SourceCapture, SourceReader};

const POLICY: &[u8] = br#"{"fictional_price":42}"#;
const PAGE: &[u8] = b"retained authored input";
const CONFIG: &str = r#"
policy_check={program='/not-executed/checker',directory='/fixture/control'}
policy_source={source='source:z-policy',entry='policy.json'}
[composition]
theme={package='theme:one'}
[[principals]]
id='owner:one'
kind='organization'
[[sites]]
id='site:one'
publisher='owner:one'
deployable=false
[[sites]]
id='site:two'
publisher='owner:one'
deployable=false
[[brands]]
id='brand:one'
name='Fixture'
[[brands.publications]]
site='site:one'
authored=[{source='source:a-content',target='one'}]
[[brands.publications]]
site='site:two'
authored=[{source='source:a-content',target='two'}]
[[sources]]
id='source:a-content'
location={kind='repository',root='/fixture/content',commit='1111111111111111111111111111111111111111',subdirectory='inputs'}
[[sources]]
id='source:z-policy'
location={kind='repository',root='/fixture/policy',commit='1111111111111111111111111111111111111111',subdirectory='inputs'}
[[sources]]
id='source:m-theme'
location={kind='repository',root='/fixture/theme',commit='1111111111111111111111111111111111111111',subdirectory='inputs'}
[[theme_packages]]
package='theme:one'
source='source:m-theme'
manifest='manifest.toml'
"#;

#[derive(Default)]
struct Sources {
    reads: RefCell<Vec<(&'static str, PathBuf)>>,
    drift: Cell<bool>,
    wrong_policy_revision: bool,
}

fn tree(root: &Path, policy: &[u8], limits: Limits) -> Result<CapturedSource, Error> {
    let files: Vec<(&str, &[u8])> = match root.to_str().expect("fixture root") {
        "/fixture/content" => vec![("page.md", PAGE), ("policy.json", policy)],
        "/fixture/policy" => vec![("policy.json", policy)],
        "/fixture/theme" => vec![
            (
                "manifest.toml",
                b"package='theme:one'\nnamespace='one'\ntemplates='templates'\nbrowser_theme_colors={light='#ffffff',dark='#000000'}\nrender={page='page.html'}\n",
            ),
            ("templates/page.html", b"{{ body_html | safe }}"),
        ],
        _ => panic!("only declared sources may be read"),
    };
    let mut inventory = InventoryBuilder::new(limits);
    if root == Path::new("/fixture/theme") {
        inventory.insert("templates".parse().expect("directory"), Content::Directory)?;
    }
    let mut retained = BTreeMap::new();
    for (path, bytes) in files {
        let path: EntryPath = path.parse().expect("fixture file");
        inventory.insert(
            path.clone(),
            Content::File {
                bytes: bytes.len() as u64,
                sha256: Sha256Digest::of(bytes),
            },
        )?;
        retained.insert(path, bytes.to_vec());
    }
    CapturedSource::from_parts(inventory.finish(), retained)
}

impl SourceReader for Sources {
    fn read(&self, _: &Path, _: Option<&Subdirectory>, _: Limits) -> Result<Inventory, Error> {
        panic!("retained acquisition must not use an inventory-only reread")
    }
}
impl SourceCapture for Sources {
    fn capture(
        &self,
        root: &Path,
        subdirectory: Option<&Subdirectory>,
        limits: Limits,
    ) -> Result<CapturedSource, Error> {
        assert_eq!(
            subdirectory,
            Some(&"inputs".parse().expect("explicit subtree"))
        );
        self.reads.borrow_mut().push(("capture", root.into()));
        tree(
            root,
            if self.drift.get() {
                b"changed policy"
            } else {
                POLICY
            },
            limits,
        )
    }
}
impl RevisionReader for Sources {
    fn read_revision(
        &self,
        root: &Path,
        subdirectory: &Subdirectory,
        commit: &CommitId,
        limits: Limits,
        _: &mut MetadataBudget,
    ) -> Result<Inventory, Error> {
        assert_eq!(subdirectory, &"inputs".parse().expect("explicit subtree"));
        assert_eq!(commit.as_str(), "1111111111111111111111111111111111111111");
        self.reads.borrow_mut().push(("revision", root.into()));
        Ok(tree(
            root,
            if self.wrong_policy_revision && root == Path::new("/fixture/policy") {
                b"wrong revision"
            } else {
                POLICY
            },
            limits,
        )?
        .into_parts()
        .0)
    }
    fn read_gitlink(
        &self,
        _: &Path,
        _: &CommitId,
        _: &Subdirectory,
        _: &mut MetadataBudget,
    ) -> Result<CommitId, Error> {
        panic!("no undeclared submodule discovery")
    }
}

fn acquire(
    sources: &Sources,
    config: &str,
    limits: PreparationLimits,
) -> Result<build::AcquiredPublicationSet, build::PublicationSetError> {
    build::acquire_publications(
        sources,
        sources,
        config,
        &[
            "site:two".parse().expect("site two"),
            "site:one".parse().expect("site one"),
        ],
        Default::default(),
        build::PublicationSetAcquisitionLimits {
            sources: limits,
            ..Default::default()
        },
    )
}

#[test]
fn shared_policy_is_captured_once_required_before_theme_and_not_parsed_as_a_manifest() {
    let sources = Sources::default();
    let acquired = acquire(
        &sources,
        CONFIG,
        build::PublicationSetAcquisitionLimits::default().sources,
    )
    .expect("complete policy and content input");
    assert_eq!(
        *sources.reads.borrow(),
        ["/fixture/content", "/fixture/policy", "/fixture/theme"]
            .into_iter()
            .flat_map(|root| ["capture", "revision"].map(|kind| (kind, PathBuf::from(root))))
            .collect::<Vec<_>>()
    );
    let policy_key = "source:z-policy".parse().expect("source key");
    let policy_path = "policy.json".parse().expect("entry");
    let first = acquired.sites()[&"site:one".parse().expect("site")]
        .as_ref()
        .expect("first site");
    let second = acquired.sites()[&"site:two".parse().expect("site")]
        .as_ref()
        .expect("second site");
    assert!(std::ptr::eq(first.sources(), second.sources()));
    let original = first.sources()[&policy_key]
        .file(&policy_path)
        .expect("retained policy");
    assert_eq!(original, POLICY);
    sources.drift.set(true);
    for input in [first, second] {
        let metadata = input
            .check_metadata(Default::default())
            .expect("policy JSON is not interpreted as a package manifest");
        let inspection = metadata.inspect();
        assert_eq!(inspection.theme_packages.len(), 1);
        assert!(inspection.plugin_packages.is_empty());
        assert!(
            inspection
                .theme_packages
                .values()
                .all(|package| package.source != &policy_key)
        );
        assert_eq!(
            input.sources()[&policy_key].file(&policy_path),
            Some(POLICY)
        );
    }
    assert_eq!(
        sources.reads.borrow().len(),
        6,
        "no policy or source reread during metadata checks"
    );
    assert_eq!(
        acquired.source_roots().expect("protected source roots"),
        ["/fixture/content", "/fixture/policy", "/fixture/theme"].map(PathBuf::from)
    );

    let single = Sources::default();
    let input = build::acquire_publication(
        &single,
        &single,
        CONFIG,
        &"site:one".parse().expect("site"),
        Default::default(),
        build::PublicationSetAcquisitionLimits::default().sources,
    )
    .expect("single site includes identical prerequisite");
    assert_eq!(
        input.sources()[&policy_key].file(&policy_path),
        Some(POLICY)
    );
    input
        .check_metadata(Default::default())
        .expect("single-site policy also stays outside package manifests");
}

#[test]
fn policy_and_content_sharing_one_source_do_not_duplicate_capture_or_budget() {
    let sources = Sources::default();
    let config = CONFIG.replace(
        "policy_source={source='source:z-policy'",
        "policy_source={source='source:a-content'",
    );
    let acquired = acquire(
        &sources,
        &config,
        PreparationLimits {
            sources: 2,
            ..build::PublicationSetAcquisitionLimits::default().sources
        },
    )
    .expect("one source can serve both roles");
    assert_eq!(acquired.sources().len(), 2);
    assert_eq!(sources.reads.borrow().len(), 4);
    assert!(
        !acquired
            .source_roots()
            .expect("exact protected set")
            .contains(&PathBuf::from("/fixture/policy")),
        "unselected declared source is not acquired or protected"
    );
}

#[test]
fn required_policy_revision_or_budget_failure_aborts_the_whole_set_before_theme_reads() {
    let sources = Sources {
        wrong_policy_revision: true,
        ..Default::default()
    };
    let error = acquire(
        &sources,
        CONFIG,
        build::PublicationSetAcquisitionLimits::default().sources,
    )
    .expect_err("policy failure is global, not a missing site result");
    assert!(
        matches!(error, build::PublicationSetError::Acquisition(build::CompositionError::Source {binding, error: Error::ContentMismatch(_)} ) if binding.as_str() == "source:z-policy")
    );
    assert_eq!(sources.reads.borrow().len(), 4);
    assert!(
        sources
            .reads
            .borrow()
            .iter()
            .all(|(_, path)| path != Path::new("/fixture/theme"))
    );
    let before = Sources::default();
    assert!(matches!(
        acquire(
            &before,
            CONFIG,
            PreparationLimits {
                sources: 2,
                ..build::PublicationSetAcquisitionLimits::default().sources
            }
        ),
        Err(build::PublicationSetError::Acquisition(
            build::CompositionError::TooManySources { limit: 2 }
        ))
    ));
    assert!(before.reads.borrow().is_empty());
    for limits in [
        PreparationLimits {
            bytes: (PAGE.len() + POLICY.len() * 2 - 1) as u64,
            ..build::PublicationSetAcquisitionLimits::default().sources
        },
        PreparationLimits {
            entries: 2,
            ..build::PublicationSetAcquisitionLimits::default().sources
        },
    ] {
        let bounded = Sources::default();
        let error = acquire(&bounded, CONFIG, limits)
            .expect_err("required policy consumes the same whole-acquisition ceiling");
        assert!(
            matches!(error, build::PublicationSetError::Acquisition(build::CompositionError::Source {binding, ..}) if binding.as_str() == "source:z-policy")
        );
        assert_eq!(
            bounded.reads.borrow().len(),
            3,
            "policy capture refused before revision or presentation"
        );
    }
}
