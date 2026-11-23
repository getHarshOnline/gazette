//! Effective composition reaches exact pinned native sources through the real SDK and CLI.

use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};

use gazette_sdk::{build, source, toml, workspace};
use git2::{Oid, Repository, Signature};
use tempfile::{TempDir, tempdir};

struct Fixture {
    root: TempDir,
    commit: Oid,
}

impl Fixture {
    fn new(bytes: &[u8]) -> Self {
        let root = tempdir().expect("disposable repository");
        let repository = Repository::init(root.path()).expect("local Git");
        fs::create_dir(root.path().join("selected")).expect("selected subtree");
        fs::write(root.path().join("selected/input.txt"), bytes).expect("fixture content");
        let blob = repository.blob(bytes).expect("native blob");
        let mut subtree = repository.treebuilder(None).expect("subtree");
        subtree.insert("input.txt", blob, 0o100644).expect("entry");
        let subtree = subtree.write().expect("subtree identity");
        let mut tree = repository.treebuilder(None).expect("root tree");
        tree.insert("selected", subtree, 0o040000)
            .expect("tree entry");
        let tree = repository
            .find_tree(tree.write().expect("tree identity"))
            .expect("tree");
        let signature = Signature::new(
            "Gazette fixture",
            "fixture@example.invalid",
            &git2::Time::new(0, 0),
        )
        .expect("fixed fixture signature");
        let commit = repository
            .commit(Some("HEAD"), &signature, &signature, "fixture", &tree, &[])
            .expect("temporary fixture commit");
        Self { root, commit }
    }
}

fn declaration(key: &str, root: &Path, commit: &str) -> String {
    format!(
        "[[sources]]\nid='{key}'\nlocation={{kind='repository',root={:?},commit='{commit}',subdirectory='selected'}}\n",
        root.to_str().expect("UTF8 fixture path")
    )
}

fn configuration(theme: &Fixture, local: &Fixture, absent: &Path) -> String {
    let mut text = String::from(
        "[composition]\n\
theme={package='theme:shadowed'}\n\
[composition.plugins]\n\
'theme-capabilities'={state='enabled'}\n\
'shadowed-capabilities'={state='enabled'}\n\
[composition.capabilities]\n\
'publication.search/v1'={state='enabled',provider={kind='plugin',package='theme-capabilities',provider='search'},requirement='required'}\n\
'commerce.catalog/v1'={state='enabled',provider={kind='plugin',package='theme-capabilities',provider='shared'},requirement='required'}\n\
'commerce.inventory/v1'={state='enabled',provider={kind='plugin',package='shadowed-capabilities',provider='shadowed'},requirement='required'}\n\
[[principals]]\nid='owner:one'\nkind='organization'\n\
[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n\
[sites.composition]\ntheme={package='theme:selected'}\n\
[sites.composition.plugins]\n\
'shadowed-capabilities'={state='disabled'}\n\
'local-capabilities'={state='enabled'}\n\
[sites.composition.capabilities]\n\
'commerce.inventory/v1'={state='disabled'}\n\
'commerce.selector/v1'={state='enabled',provider={kind='plugin',package='local-capabilities',provider='local'},requirement='optional'}\n\
[[sites]]\nid='site:other'\npublisher='owner:one'\ndeployable=true\n",
    );
    for binding in [
        declaration("source:theme", theme.root.path(), &theme.commit.to_string()),
        declaration("source:local", local.root.path(), &local.commit.to_string()),
        declaration(
            "source:absent",
            absent,
            "0000000000000000000000000000000000000000",
        ),
    ] {
        text.push_str(&binding);
    }
    text.push_str(
        "[[theme_packages]]\npackage='theme:shadowed'\nsource='source:theme'\nmanifest='shadowed-theme.toml'\n\
[[theme_packages]]\npackage='theme:selected'\nsource='source:theme'\nmanifest='selected-theme.toml'\n\
[[plugin_packages]]\npackage='theme-capabilities'\nsource='source:theme'\nmanifest='theme-plugin.toml'\n\
[[plugin_packages]]\npackage='local-capabilities'\nsource='source:local'\nmanifest='local-plugin.toml'\n\
[[plugin_packages]]\npackage='shadowed-capabilities'\nsource='source:absent'\nmanifest='shadowed-plugin.toml'\n",
    );
    text
}

fn limits() -> source::PreparationLimits {
    source::PreparationLimits {
        sources: 2,
        entries: 2,
        bytes: 6,
        per_source: source::RevisionLimits::default(),
    }
}

fn run(config: &str, extra: &[&str]) -> Output {
    let cwd = tempdir().expect("independent empty working directory");
    let mut process = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args(["check", "--stdin", "--acquire-composition", "site:one"])
        .args(extra)
        .current_dir(cwd.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("direct Gazette executable");
    let written = process
        .stdin
        .take()
        .expect("stdin")
        .write_all(config.as_bytes());
    let output = process.wait_with_output().expect("completion");
    if let Err(error) = written {
        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
        assert!(!output.status.success());
    }
    assert!(
        fs::read_dir(cwd.path()).expect("cwd").next().is_none(),
        "no generated output, database or discovery side effects"
    );
    output
}

fn acquire(
    config: &str,
    limits: source::PreparationLimits,
) -> Result<build::AcquiredInputs, build::CompositionError> {
    build::acquire_composition(
        config,
        &"site:one".parse().expect("site key"),
        toml::Limits::default(),
        limits,
    )
}

fn failed(output: &Output, code: i32) {
    assert_eq!(output.status.code(), Some(code), "{output:?}");
    assert!(
        output.stdout.is_empty(),
        "no successful partial composition: {output:?}"
    );
}

#[test]
fn effective_selection_deduplicates_sources_and_retains_bytes_without_source_writes() {
    let theme = Fixture::new(b"abc");
    let local = Fixture::new(b"xyz");
    let missing_parent = tempdir().expect("absence owner");
    let absent = missing_parent.path().join("not-created");
    let config = configuration(&theme, &local, &absent);
    let before: Vec<_> = [&theme, &local]
        .into_iter()
        .map(|fixture| {
            source::inspect(fixture.root.path(), None, source::Limits::default())
                .expect("whole repository before")
        })
        .collect();
    let acquired = acquire(&config, limits())
        .expect("two distinct selected sources fit exact aggregate ceilings");
    assert_eq!(
        acquired.compilation(),
        &workspace::compile(&config, toml::Limits::default()).expect("shared compiler")
    );
    assert_eq!(acquired.site().as_str(), "site:one");
    assert_eq!(
        acquired
            .sources()
            .keys()
            .map(workspace::Key::as_str)
            .collect::<Vec<_>>(),
        ["source:local", "source:theme"]
    );
    for (key, bytes) in [("source:theme", b"abc"), ("source:local", b"xyz")] {
        let input = acquired
            .sources()
            .get(&key.parse().expect("source key"))
            .expect("effective source");
        assert_eq!(
            input.file(&"input.txt".parse().expect("path")),
            Some(bytes.as_slice())
        );
        assert_eq!(input.inventory().total_bytes(), 3);
    }
    let mut expected = serde_json::to_vec_pretty(&acquired.inspect()).expect("shared inspection");
    expected.push(b'\n');
    let flags = [
        "--max-placements",
        "2",
        "--max-acquired-entries",
        "2",
        "--max-acquired-bytes",
        "6",
    ];
    let output = run(&config, &flags);
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
    assert_eq!(output.stdout, expected);
    assert_eq!(run(&config, &flags).stdout, expected);
    assert!(
        !absent.exists(),
        "shadowed/disabled and other-site inputs are never initialized"
    );
    for (fixture, snapshot) in [&theme, &local].into_iter().zip(before) {
        assert_eq!(
            source::inspect(fixture.root.path(), None, source::Limits::default())
                .expect("after reads"),
            snapshot
        );
    }
    fs::write(theme.root.path().join("selected/input.txt"), b"new")
        .expect("change disposable source after capture");
    assert_eq!(
        acquired
            .sources()
            .get(&"source:theme".parse().expect("key"))
            .expect("retained")
            .file(&"input.txt".parse().expect("path")),
        Some(b"abc".as_slice())
    );
    let mut retained = serde_json::to_vec_pretty(&acquired.inspect()).expect("retained evidence");
    retained.push(b'\n');
    assert_eq!(retained, expected);
    failed(&run(&config, &[]), 1);
}

#[test]
fn sdk_and_cli_acquire_plain_filesystem_sources_without_git_or_fallback() {
    let root = tempdir().expect("plain local source");
    fs::create_dir(root.path().join("authored")).expect("authored subtree");
    fs::write(root.path().join("authored/input.txt"), b"plain").expect("authored bytes");
    let config = format!(
        "[composition]\ntheme={{package='theme:local'}}\n\
[[principals]]\nid='owner:one'\nkind='organization'\n\
[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n\
[[sources]]\nid='source:authored'\n\
location={{kind='filesystem',root={:?},subdirectory='authored'}}\n\
[[theme_packages]]\npackage='theme:local'\nsource='source:authored'\nmanifest='theme.toml'\n",
        root.path().to_str().expect("UTF-8 source root")
    );
    let acquired = acquire(
        &config,
        source::PreparationLimits {
            sources: 1,
            entries: 1,
            bytes: 5,
            per_source: source::RevisionLimits::default(),
        },
    )
    .expect("SDK filesystem acquisition");
    let input = acquired
        .sources()
        .get(&"source:authored".parse().expect("source key"))
        .expect("retained input");
    assert!(matches!(input, source::AcquiredInput::Filesystem(_)));
    assert_eq!(
        input.file(&"input.txt".parse().expect("entry")),
        Some(b"plain".as_slice())
    );
    let output = run(
        &config,
        &[
            "--max-placements",
            "1",
            "--max-acquired-entries",
            "1",
            "--max-acquired-bytes",
            "5",
        ],
    );
    assert!(output.status.success(), "{output:?}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).expect("CLI evidence");
    let location = &report["compilation"]["workspace"]["sources"]["source:authored"]["location"];
    assert_eq!(location["kind"], "filesystem");
    assert_eq!(location["subdirectory"], "authored");
    assert!(location.get("commit").is_none());
    let evidence = &report["sources"]["source:authored"];
    assert_eq!(evidence["kind"], "filesystem");
    assert!(evidence.get("verification").is_none());
    assert!(!evidence.to_string().contains("commit"));
}

#[test]
fn selected_missing_changed_and_wrong_pin_sources_do_not_fall_back_or_silently_degrade() {
    for scenario in ["missing", "changed", "wrong pin"] {
        let theme = Fixture::new(b"abc");
        let local = Fixture::new(b"xyz");
        let parent = tempdir().expect("absence owner");
        let absent = parent.path().join("missing");
        let mut config = configuration(&theme, &local, &absent);
        match scenario {
            "missing" => {
                config = config.replace(
                    local.root.path().to_str().expect("path"),
                    absent.to_str().expect("absent path"),
                )
            }
            "changed" => fs::write(local.root.path().join("selected/input.txt"), b"bad")
                .expect("dirty fixture"),
            _ => {
                config = config.replace(
                    &local.commit.to_string(),
                    "0000000000000000000000000000000000000000",
                )
            }
        }
        assert!(
            workspace::compile(&config, toml::Limits::default()).is_ok(),
            "configuration stays structurally valid"
        );
        let failure = acquire(&config, limits())
            .expect_err("optional selection cannot discard failed evidence");
        let build::CompositionError::Source { binding, error } = &failure else {
            panic!("expected selected-source failure: {failure:?}")
        };
        assert_eq!(binding.as_str(), "source:local");
        let expected_code = match scenario {
            "missing" => "source_io",
            "changed" => "source_content_mismatch",
            _ => "source_revision_mismatch",
        };
        assert_eq!(error.code(), expected_code);
        assert_eq!(failure.code(), expected_code);
        let output = run(&config, &[]);
        failed(&output, 1);
        assert!(String::from_utf8_lossy(&output.stderr).contains(expected_code));
        assert!(!absent.exists());
        assert_eq!(
            fs::read(theme.root.path().join("selected/input.txt")).expect("other source preserved"),
            b"abc"
        );
    }
}

#[test]
fn aggregate_limits_bound_the_distinct_effective_source_set() {
    let theme = Fixture::new(b"abc");
    let local = Fixture::new(b"xyz");
    let parent = tempdir().expect("absence owner");
    let config = configuration(&theme, &local, &parent.path().join("missing"));
    assert!(acquire(&config, limits()).is_ok());
    for (bounded, flags) in [
        (
            source::PreparationLimits {
                sources: 1,
                ..limits()
            },
            ["--max-placements", "1"],
        ),
        (
            source::PreparationLimits {
                entries: 1,
                ..limits()
            },
            ["--max-acquired-entries", "1"],
        ),
        (
            source::PreparationLimits {
                bytes: 5,
                ..limits()
            },
            ["--max-acquired-bytes", "5"],
        ),
    ] {
        let error = acquire(&config, bounded).expect_err("aggregate bound");
        if bounded.sources == 1 {
            assert!(matches!(
                error,
                build::CompositionError::TooManySources { limit: 1 }
            ));
        } else {
            assert!(
                matches!(&error, build::CompositionError::Source {
                binding, error: source::Error::Limit { .. },
            } if binding.as_str() == "source:theme"),
                "{error:?}"
            );
        }
        let output = run(&config, &flags);
        failed(&output, 1);
        assert!(String::from_utf8_lossy(&output.stderr).contains(error.code()));
    }
}

#[test]
fn whole_workspace_validation_and_explicit_operation_selection_precede_acquisition() {
    let theme = Fixture::new(b"abc");
    let local = Fixture::new(b"xyz");
    let parent = tempdir().expect("absence owner");
    let config = configuration(&theme, &local, &parent.path().join("missing"));
    let invalid = config.replacen(
        "theme={package='theme:shadowed'}",
        "theme={package='theme:unknown'}",
        1,
    );
    assert!(
        workspace::compile(&invalid, toml::Limits::default()).is_err(),
        "even shadowed declarations require exact valid references"
    );
    let error = acquire(&invalid, limits()).expect_err("complete configuration checked first");
    assert!(
        error
            .to_string()
            .contains("workspace_theme_package_reference"),
        "{error}"
    );
    failed(&run(&invalid, &[]), 1);
    let missing_site = build::acquire_composition(
        &config,
        &"site:missing".parse().expect("key"),
        toml::Limits::default(),
        limits(),
    );
    assert!(
        matches!(missing_site, Err(build::CompositionError::UnknownSite(site)) if site.as_str() == "site:missing")
    );
    for flags in [
        vec!["--acquire-site", "site:one"],
        vec!["--parse-documents"],
        vec!["--max-corpus-files", "1"],
        vec!["--max-corpus-bytes", "1"],
        vec!["--max-corpus-nodes", "1"],
        vec!["--max-corpus-metadata-nodes", "1"],
        vec![
            "--origin",
            r#"{"kind":"site","identity":"site:one","field":["id"]}"#,
        ],
        vec![
            "--composition-origin",
            r#"{"site":"site:one","field":{"kind":"theme"}}"#,
        ],
    ] {
        failed(&run(&config, &flags), 2);
    }
}

#[test]
fn later_sorted_failure_reports_the_exact_binding_without_a_successful_prefix() {
    let theme = Fixture::new(b"abc");
    let local = Fixture::new(b"xyz");
    let parent = tempdir().expect("absence owner");
    let config = configuration(&theme, &local, &parent.path().join("missing"));
    let valid = acquire(&config, limits()).expect("both exact source pins initially match");
    assert_eq!(
        valid
            .sources()
            .keys()
            .map(workspace::Key::as_str)
            .collect::<Vec<_>>(),
        ["source:local", "source:theme"]
    );
    fs::write(theme.root.path().join("selected/input.txt"), b"bad")
        .expect("change later source only");
    let error =
        acquire(&config, limits()).expect_err("later sorted source rejects the whole capture");
    assert!(
        matches!(&error, build::CompositionError::Source {
        binding, error: source::Error::ContentMismatch(_),
    } if binding.as_str() == "source:theme"),
        "{error:?}"
    );
    let output = run(&config, &[]);
    failed(&output, 1);
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    assert!(diagnostic.contains("source_content_mismatch"));
    assert!(diagnostic.contains("source:theme"));
    assert_eq!(
        fs::read(local.root.path().join("selected/input.txt")).expect("earlier source untouched"),
        b"xyz"
    );
    assert_eq!(
        valid
            .sources()
            .get(&"source:theme".parse().expect("key"))
            .expect("prior retained source")
            .file(&"input.txt".parse().expect("path")),
        Some(b"abc".as_slice())
    );
}

#[test]
fn distinct_source_keys_with_identical_coordinates_remain_independently_budgeted() {
    let shared = Fixture::new(b"abc");
    let parent = tempdir().expect("absence owner");
    let config = configuration(&shared, &shared, &parent.path().join("missing"));
    let acquired = acquire(&config, limits()).expect("two key bindings fit their combined budget");
    assert_eq!(acquired.sources().len(), 2);
    let first = acquired
        .sources()
        .get(&"source:local".parse().expect("key"))
        .expect("local binding");
    let second = acquired
        .sources()
        .get(&"source:theme".parse().expect("key"))
        .expect("theme binding");
    assert_eq!(first.inventory(), second.inventory());
    assert_eq!(
        acquired
            .sources()
            .values()
            .map(|input| input.inventory().total_bytes())
            .sum::<u64>(),
        6
    );
    for (bounded, flags) in [
        (
            source::PreparationLimits {
                sources: 1,
                ..limits()
            },
            ["--max-placements", "1"],
        ),
        (
            source::PreparationLimits {
                entries: 1,
                ..limits()
            },
            ["--max-acquired-entries", "1"],
        ),
        (
            source::PreparationLimits {
                bytes: 3,
                ..limits()
            },
            ["--max-acquired-bytes", "3"],
        ),
    ] {
        let error = acquire(&config, bounded)
            .expect_err("equal native coordinates do not erase distinct configured identities");
        if bounded.sources == 1 {
            assert!(matches!(
                &error,
                build::CompositionError::TooManySources { limit: 1 }
            ));
        } else {
            assert!(
                matches!(&error, build::CompositionError::Source {
                binding, error: source::Error::Limit { .. },
            } if binding.as_str() == "source:theme"),
                "{error:?}"
            );
        }
        let output = run(&config, &flags);
        failed(&output, 1);
        assert!(String::from_utf8_lossy(&output.stderr).contains(error.code()));
    }
}

#[test]
fn empty_composition_captures_nothing_under_zero_allowances_without_inventing_a_theme() {
    let config = "[[principals]]\nid='owner:one'\nkind='organization'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n";
    let acquired = acquire(
        config,
        source::PreparationLimits {
            sources: 0,
            entries: 0,
            bytes: 0,
            ..limits()
        },
    )
    .expect("an empty desired composition is an empty acquisition, not build readiness");
    assert!(acquired.sources().is_empty());
    assert_eq!(acquired.site().as_str(), "site:one");
    let composition = acquired
        .compilation()
        .workspace
        .composition_for(acquired.site())
        .expect("known site");
    assert!(composition.theme().is_none());
    assert!(composition.capabilities().is_empty());
    let output = run(
        config,
        &[
            "--max-placements",
            "0",
            "--max-acquired-entries",
            "0",
            "--max-acquired-bytes",
            "0",
        ],
    );
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
    let mut expected = serde_json::to_vec_pretty(&acquired.inspect()).expect("empty report");
    expected.push(b'\n');
    assert_eq!(output.stdout, expected);
    let wire: serde_json::Value = serde_json::from_slice(&output.stdout).expect("machine report");
    assert_eq!(wire["sources"], serde_json::json!({}));
}
