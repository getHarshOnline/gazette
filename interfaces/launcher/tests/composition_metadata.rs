//! Theme and plugin package metadata is checked from retained Git-bound bytes, not live files.

use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};

use gazette_sdk::{build, source, toml};
use git2::{IndexAddOption, Oid, Repository, Signature};
use serde_json::Value;
use tempfile::{TempDir, tempdir};

const THEME_MANIFEST: &str = "package='theme:base'\n\
    namespace='base'\n\
    templates='templates'\nrender={page='page.html'}\n\
    palettes=[{id='palette:light',browser_theme_colors={light='#ffffff',dark='#000000'}}]\n";
const PLUGIN_MANIFEST: &str = "package='search-plugin'\n\
    [resources.'module:search']\n\
    inputs=['search.js']\n\
    build={kind='browser-module',entry='search.js'}\n\
    [providers.search]\n\
    implements=['publication.search/v1']\n";

struct Fixture {
    root: TempDir,
    commit: Oid,
}

impl Fixture {
    fn new(theme: &str) -> Self {
        Self::with_manifests(theme, PLUGIN_MANIFEST)
    }

    fn with_manifests(theme: &str, plugin: &str) -> Self {
        Self::files(&[
            ("theme.toml", theme.as_bytes()),
            ("plugin.toml", plugin.as_bytes()),
            ("templates/page.html", b"<h1>{{ title }}</h1>\n"),
            (
                "search.js",
                b"throw new Error('metadata check must never execute me');\n",
            ),
        ])
    }

    fn files(files: &[(&str, &[u8])]) -> Self {
        let root = tempdir().expect("disposable native source");
        let repository = Repository::init(root.path()).expect("Git fixture");
        fs::create_dir(root.path().join("selected")).expect("captured root");
        for (path, bytes) in files {
            let path = root.path().join("selected").join(path);
            fs::create_dir_all(path.parent().expect("fixture parent")).expect("fixture subtree");
            fs::write(path, bytes).expect("fixture file");
        }
        let mut index = repository.index().expect("fixture index");
        index
            .add_all(["selected"], IndexAddOption::DEFAULT, None)
            .expect("fixture tracked files");
        let tree = repository
            .find_tree(index.write_tree().expect("fixture tree"))
            .expect("tree");
        let signature = Signature::new(
            "Gazette fixture",
            "fixture@example.invalid",
            &git2::Time::new(0, 0),
        )
        .expect("fixed signature");
        let commit = repository
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                "package fixture",
                &tree,
                &[],
            )
            .expect("temporary fixture commit");
        Self { root, commit }
    }

    fn config(&self) -> String {
        format!(
            "[composition]\n\
             theme={{package='theme:base',palette='palette:light'}}\n\
             [composition.plugins]\n\
             'search-plugin'={{state='enabled'}}\n\
             [composition.capabilities]\n\
             'publication.search/v1'={{state='enabled',provider={{kind='plugin',package='search-plugin',provider='search'}},requirement='optional'}}\n\
             [[principals]]\nid='owner:one'\nkind='person'\n\
             [[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n\
             [[sources]]\nid='source:theme'\n\
             location={{kind='repository',root={:?},commit='{}',subdirectory='selected'}}\n\
             [[theme_packages]]\npackage='theme:base'\nsource='source:theme'\nmanifest='theme.toml'\n\
             [[plugin_packages]]\npackage='search-plugin'\nsource='source:theme'\nmanifest='plugin.toml'\n",
            self.root.path().to_str().expect("native path"),
            self.commit,
        )
    }
}

fn acquire(config: &str) -> build::AcquiredInputs {
    build::acquire_composition(
        config,
        &"site:one".parse().expect("site"),
        toml::Limits::default(),
        source::PreparationLimits {
            sources: 2,
            entries: 64,
            bytes: 64 * 1024,
            per_source: source::RevisionLimits::default(),
        },
    )
    .expect("native capture is independent of package metadata validity")
}

fn run(config: &str, flags: &[&str]) -> Output {
    let cwd = tempdir().expect("unrelated empty cwd");
    let mut child = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args(["check", "--stdin"])
        .args(flags)
        .current_dir(cwd.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Gazette executable");
    let written = child
        .stdin
        .take()
        .expect("stdin")
        .write_all(config.as_bytes());
    let output = child.wait_with_output().expect("completion");
    if let Err(error) = written {
        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
        assert!(!output.status.success());
    }
    assert!(
        fs::read_dir(cwd.path()).expect("cwd").next().is_none(),
        "no render, provider execution output or store created"
    );
    output
}

fn check(config: &str) -> Output {
    run(
        config,
        &["--acquire-composition", "site:one", "--check-composition"],
    )
}

fn snapshot(root: &Path) -> source::Inventory {
    source::inspect(root, None, source::Limits::default()).expect("complete fixture inventory")
}

fn rejected<T, E>(result: Result<T, E>) -> E {
    match result {
        Err(error) => error,
        Ok(_) => panic!("invalid metadata must not return a complete result"),
    }
}

#[test]
fn retained_manifest_drives_sdk_cli_parity_without_execution_or_live_rereads() {
    let fixture = Fixture::new(THEME_MANIFEST);
    let config = fixture.config();
    let before = snapshot(fixture.root.path());
    let acquired = acquire(&config);
    let checked = acquired
        .check_metadata(toml::Limits::default())
        .expect("matching selected package metadata");
    let mut expected =
        serde_json::to_vec_pretty(&checked.inspect()).expect("one shared metadata report");
    expected.push(b'\n');
    let output = check(&config);
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
    assert_eq!(output.stdout, expected);
    let report: Value = serde_json::from_slice(&output.stdout).expect("metadata report");
    let themes = report["theme_packages"]
        .as_object()
        .expect("complete Theme package map");
    let plugins = report["plugin_packages"]
        .as_object()
        .expect("complete plugin package map");
    assert_eq!(themes.len(), 1);
    assert_eq!(plugins.len(), 1);
    let theme = &themes["theme:base"];
    let plugin = &plugins["search-plugin"];
    assert_eq!(theme["manifest"], "theme.toml");
    assert_eq!(
        theme["manifest_sha256"],
        serde_json::to_value(source::Sha256Digest::of(THEME_MANIFEST.as_bytes()))
            .expect("exact Theme manifest digest")
    );
    assert_eq!(theme["declaration"]["namespace"], "base");
    assert_eq!(theme["declaration"]["templates"], "templates");
    assert_eq!(plugin["manifest"], "plugin.toml");
    assert_eq!(
        plugin["identity"]["manifest"],
        serde_json::to_value(source::Sha256Digest::of(PLUGIN_MANIFEST.as_bytes()))
            .expect("exact plugin manifest digest")
    );
    assert_eq!(
        plugin["declaration"]["resources"]["module:search"]["inputs"][0],
        "search.js"
    );
    assert_eq!(
        report["acquisition"]["sources"]["source:theme"]["verification"]["commit"],
        fixture.commit.to_string()
    );
    assert_eq!(check(&config).stdout, expected);
    assert_eq!(
        snapshot(fixture.root.path()),
        before,
        "metadata validation does not alter selected or unselected bytes"
    );
    fs::write(
        fixture.root.path().join("selected/theme.toml"),
        "package='theme:changed'\n",
    )
    .expect("later owner-native fixture edit");
    fs::write(
        fixture.root.path().join("selected/templates/page.html"),
        "changed native template",
    )
    .expect("later template edit");
    let retained = acquired
        .check_metadata(toml::Limits::default())
        .expect("validation uses captured manifest and inputs");
    assert_eq!(
        serde_json::to_value(retained.inspect()).expect("retained report"),
        serde_json::from_slice::<Value>(&expected).expect("original report")
    );
    assert!(
        !check(&config).status.success(),
        "fresh native acquisition must reject modified sources"
    );
    assert_eq!(
        acquired.sources()[&"source:theme".parse().expect("source")]
            .file(&"theme.toml".parse().expect("path")),
        Some(THEME_MANIFEST.as_bytes())
    );
}

#[test]
fn wrong_selected_identity_palette_provider_and_capability_fail_even_when_optional() {
    let fixture = Fixture::new(THEME_MANIFEST);
    let original = fixture.config();
    let unknown_theme = original.replace(
        "theme={package='theme:base',palette='palette:light'}",
        "theme={package='theme:unknown',palette='palette:light'}",
    );
    assert!(
        build::acquire_composition(
            &unknown_theme,
            &"site:one".parse().expect("site"),
            toml::Limits::default(),
            source::PreparationLimits {
                sources: 2,
                entries: 64,
                bytes: 64 * 1024,
                per_source: source::RevisionLimits::default(),
            },
        )
        .is_err(),
        "an unbound selected Theme fails before acquisition"
    );
    assert_eq!(check(&unknown_theme).status.code(), Some(1));

    for (selected, replacement) in [
        ("palette='palette:light'", "palette='palette:unknown'"),
        ("provider='search'", "provider='unknown'"),
        ("'publication.search/v1'", "'commerce.catalog/v1'"),
    ] {
        let config = original.replace(selected, replacement);
        let acquired = acquire(&config);
        let error = rejected(acquired.check_metadata(toml::Limits::default()));
        assert_eq!(error.code(), "package_selection", "{replacement}: {error}");
        let output = check(&config);
        assert_eq!(output.status.code(), Some(1), "{replacement}: {output:?}");
        assert!(
            output.stdout.is_empty(),
            "no successful subset or silent optional degradation"
        );
    }
}

#[test]
fn exported_provider_does_not_substitute_for_required_capability_selection() {
    let theme = format!("{THEME_MANIFEST}[capabilities]\nrequired=['publication.search/v1']\n");
    let fixture = Fixture::new(&theme);
    let config = fixture.config().replace(
        "[composition.capabilities]\n'publication.search/v1'={state='enabled',provider={kind='plugin',package='search-plugin',provider='search'},requirement='optional'}\n",
        "",
    );
    let acquired = acquire(&config);
    let error = rejected(acquired.check_metadata(toml::Limits::default()));
    assert_eq!(error.code(), "package_selection");
    let output = check(&config);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty());
}

#[test]
fn optional_provider_selection_does_not_satisfy_required_theme_capability() {
    let theme = format!("{THEME_MANIFEST}[capabilities]\nrequired=['publication.search/v1']\n");
    let fixture = Fixture::new(&theme);
    let required = fixture
        .config()
        .replace("requirement='optional'", "requirement='required'");
    acquire(&required)
        .check_metadata(toml::Limits::default())
        .expect("required selection should satisfy the Theme requirement");
    assert!(check(&required).status.success());

    let optional = "'publication.search/v1'={state='enabled',provider={kind='plugin',package='search-plugin',provider='search'},requirement='optional'}\n";
    let optional_with_absence = "'publication.search/v1'={state='enabled',provider={kind='plugin',package='search-plugin',provider='search'},requirement='optional',allow_absence='no-native-acquisition'}\n";
    let workspace = fixture.config().replace(optional, optional_with_absence);
    let site = fixture
        .config()
        .replace(&format!("[composition.capabilities]\n{optional}"), "")
        .replace(
            "deployable=false\n",
            &format!("deployable=false\n[sites.composition.capabilities]\n{optional_with_absence}"),
        );

    for (scope, config) in [("workspace", workspace), ("site", site)] {
        let acquired = acquire(&config);
        let error = rejected(acquired.check_metadata(toml::Limits::default()));
        assert_eq!(error.code(), "package_selection", "{scope}: {error}");
        assert!(
            error
                .to_string()
                .contains("requires an enabled capability provider with required failure policy"),
            "{scope}: {error}"
        );
        let output = check(&config);
        assert_eq!(output.status.code(), Some(1), "{scope}: {output:?}");
        assert!(output.stdout.is_empty(), "{scope}");
    }
}

#[test]
fn unknown_schema_fields_and_dangling_metadata_never_produce_checked_composition() {
    for (theme, plugin, code) in [
        (
            THEME_MANIFEST.replace("namespace='base'", "namespace='base'\nunknown=true"),
            PLUGIN_MANIFEST.to_owned(),
            "toml_invalid",
        ),
        (
            THEME_MANIFEST.to_owned(),
            PLUGIN_MANIFEST.replace(
                "package='search-plugin'",
                "package='search-plugin'\nunknown=true",
            ),
            "toml_invalid",
        ),
        (
            THEME_MANIFEST.to_owned(),
            PLUGIN_MANIFEST.replace("inputs=['search.js']", "inputs=['search.js']\nunknown=true"),
            "toml_invalid",
        ),
        (
            THEME_MANIFEST.to_owned(),
            PLUGIN_MANIFEST.replace("entry='search.js'", "entry='missing.js'"),
            "plugin_manifest_invalid",
        ),
    ] {
        let fixture = Fixture::with_manifests(&theme, &plugin);
        let config = fixture.config();
        let acquired = acquire(&config);
        let error = rejected(acquired.check_metadata(toml::Limits::default()));
        assert_eq!(error.code(), code, "{theme}\n{plugin}: {error}");
        let output = check(&config);
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn missing_package_bindings_manifests_and_inputs_are_not_guessed_or_synthesized() {
    let fixture = Fixture::new(THEME_MANIFEST);
    let missing_binding = fixture.config().replace(
        "[[theme_packages]]\npackage='theme:base'\nsource='source:theme'\nmanifest='theme.toml'\n",
        "",
    );
    assert!(
        build::acquire_composition(
            &missing_binding,
            &"site:one".parse().expect("site"),
            toml::Limits::default(),
            source::PreparationLimits {
                sources: 2,
                entries: 64,
                bytes: 64 * 1024,
                per_source: source::RevisionLimits::default(),
            },
        )
        .is_err(),
        "the selected Theme requires an explicit package binding before acquisition"
    );
    assert_eq!(check(&missing_binding).status.code(), Some(1));

    for scenario in [
        "manifest missing",
        "Theme input missing",
        "Theme input is directory",
        "plugin input missing",
        "plugin input is directory",
    ] {
        let theme = match scenario {
            "Theme input missing" => format!(
                "{THEME_MANIFEST}[[resources]]\nid='stylesheet:theme'\ninputs=['missing.css']\nbuild={{kind='stylesheet',entry='missing.css'}}\n"
            ),
            "Theme input is directory" => format!(
                "{THEME_MANIFEST}[[resources]]\nid='stylesheet:theme'\ninputs=['templates']\nbuild={{kind='stylesheet',entry='templates'}}\n"
            ),
            _ => THEME_MANIFEST.to_owned(),
        };
        let plugin = match scenario {
            "plugin input missing" => PLUGIN_MANIFEST.replace("search.js", "missing.js"),
            "plugin input is directory" => PLUGIN_MANIFEST.replace("search.js", "templates"),
            _ => PLUGIN_MANIFEST.to_owned(),
        };
        let fixture = Fixture::with_manifests(&theme, &plugin);
        let mut config = fixture.config();
        if scenario == "manifest missing" {
            config = config.replace("manifest='theme.toml'", "manifest='absent.toml'");
        }
        let before = snapshot(fixture.root.path());
        let acquired = acquire(&config);
        let error = rejected(acquired.check_metadata(toml::Limits::default()));
        assert_eq!(error.code(), "package_input", "{scenario}: {error}");
        let output = check(&config);
        assert_eq!(output.status.code(), Some(1), "{scenario}: {output:?}");
        assert!(output.stdout.is_empty());
        assert_eq!(snapshot(fixture.root.path()), before);
    }
}

#[test]
fn metadata_check_requires_explicit_acquisition_and_respects_parser_bounds() {
    let fixture = Fixture::new(THEME_MANIFEST);
    let config = fixture.config();
    let without_acquisition = run(&config, &["--check-composition"]);
    assert_eq!(
        without_acquisition.status.code(),
        Some(2),
        "{without_acquisition:?}"
    );
    assert!(without_acquisition.stdout.is_empty());
    for flags in [
        vec!["--check-composition", "--origin", "{}"],
        vec!["--check-composition", "--acquire-site", "site:one"],
        vec![
            "--check-composition",
            "--acquire-composition",
            "site:one",
            "--origin",
            "{}",
        ],
        vec![
            "--check-composition",
            "--acquire-composition",
            "site:one",
            "--parse-documents",
        ],
    ] {
        let output = run(&config, &flags);
        assert_eq!(
            output.status.code(),
            Some(2),
            "conflicting/orphaned modes: {flags:?}: {output:?}"
        );
        assert!(output.stdout.is_empty());
    }
    for flag in [
        "--max-corpus-files",
        "--max-corpus-bytes",
        "--max-corpus-nodes",
        "--max-corpus-metadata-nodes",
    ] {
        for acquisition in [None, Some("--acquire-site"), Some("--acquire-composition")] {
            let mut flags = vec!["--check-composition", flag, "1"];
            if let Some(acquisition) = acquisition {
                flags.extend([acquisition, "site:one"]);
            }
            let output = run(&config, &flags);
            assert_eq!(
                output.status.code(),
                Some(2),
                "indirect requirement must not suppress a conflict: {flags:?}: {output:?}"
            );
            assert!(output.stdout.is_empty());
        }
    }
    let acquired = acquire(&config);
    for limits in [
        toml::Limits {
            source_bytes: THEME_MANIFEST.len() - 1,
            ..toml::Limits::default()
        },
        toml::Limits {
            nodes: 0,
            ..toml::Limits::default()
        },
    ] {
        assert!(
            acquired.check_metadata(limits).is_err(),
            "retained metadata uses the shared bounded parser"
        );
    }
}

#[test]
fn nested_manifest_paths_remain_relative_to_the_captured_source_root() {
    let fixture = Fixture::files(&[
        ("meta/theme.toml", THEME_MANIFEST.as_bytes()),
        ("plugin.toml", PLUGIN_MANIFEST.as_bytes()),
        ("templates/page.html", b"<p>root-relative template</p>\n"),
        ("search.js", b"/* root-relative input, never executed */\n"),
    ]);
    let config = fixture
        .config()
        .replace("manifest='theme.toml'", "manifest='meta/theme.toml'");
    let acquired = acquire(&config);
    let checked = acquired
        .check_metadata(toml::Limits::default())
        .expect("package-manifest parent is not an implicit path base");
    let output = check(&config);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).expect("CLI report"),
        serde_json::to_value(checked.inspect()).expect("SDK report")
    );
}

#[test]
fn all_selected_manifest_bytes_and_nodes_share_the_same_aggregate_budget() {
    let fixture = Fixture::new(THEME_MANIFEST);
    let mut config = fixture.config().replace(
        "package='search-plugin'\nsource='source:theme'\nmanifest='plugin.toml'",
        "package='search-plugin'\nsource='source:second'\nmanifest='plugin.toml'",
    );
    config.push_str(&format!(
        "[[sources]]\nid='source:second'\n\
         location={{kind='repository',root={:?},commit='{}',subdirectory='selected'}}\n",
        fixture.root.path().to_str().expect("fixture path"),
        fixture.commit,
    ));
    let acquired = acquire(&config);
    assert_eq!(
        acquired.sources().len(),
        2,
        "distinct selected bindings are not collapsed by native path"
    );
    let theme_syntax = toml::ParsedToml::parse(THEME_MANIFEST, toml::Limits::default())
        .expect("Theme manifest parser");
    let plugin_syntax = toml::ParsedToml::parse(PLUGIN_MANIFEST, toml::Limits::default())
        .expect("plugin manifest parser");
    assert!(acquired.check_metadata(toml::Limits::default()).is_ok());
    for limits in [
        toml::Limits {
            source_bytes: THEME_MANIFEST.len().max(PLUGIN_MANIFEST.len()),
            ..toml::Limits::default()
        },
        toml::Limits {
            nodes: theme_syntax.node_count().max(plugin_syntax.node_count()),
            ..toml::Limits::default()
        },
    ] {
        assert!(
            acquired.check_metadata(limits).is_err(),
            "each manifest fits separately but the complete set exceeds the shared budget"
        );
    }
    assert!(
        acquired
            .check_metadata(toml::Limits {
                source_bytes: THEME_MANIFEST.len() + PLUGIN_MANIFEST.len(),
                nodes: theme_syntax.node_count() + plugin_syntax.node_count(),
                ..toml::Limits::default()
            })
            .is_ok(),
        "exact complete-set byte/node budget"
    );
}

#[test]
fn independent_workspace_and_site_provider_sources_do_not_merge_theme_ownership() {
    const PROVIDER: &str = "package='independent-plugin'\n[resources.'module:independent']\ninputs=['independent.js']\nbuild={kind='browser-module',entry='independent.js'}\n[providers.independent]\nimplements=['publication.search/v1']\n";
    let theme = Fixture::new(THEME_MANIFEST);
    let provider = Fixture::files(&[
        ("plugin.toml", PROVIDER.as_bytes()),
        (
            "independent.js",
            b"throw new Error('do not execute metadata inputs');\n",
        ),
    ]);
    for scope in ["workspace", "site"] {
        let entry = "'publication.search/v1'={state='enabled',provider={kind='plugin',package='independent-plugin',provider='independent'},requirement='optional'}\n";
        let mut config = if scope == "workspace" {
            theme
                .config()
                .replace("'search-plugin'={state='enabled'}", "'independent-plugin'={state='enabled'}")
                .replace(
                    "'publication.search/v1'={state='enabled',provider={kind='plugin',package='search-plugin',provider='search'},requirement='optional'}\n",
                    entry,
                )
        } else {
            theme
                .config()
                .replace("[composition.plugins]\n'search-plugin'={state='enabled'}\n", "")
                .replace(
                    "'publication.search/v1'={state='enabled',provider={kind='plugin',package='search-plugin',provider='search'},requirement='optional'}\n",
                    "",
                )
                .replacen(
                    "[[sources]]",
                    &format!("[sites.composition.plugins]\n'independent-plugin'={{state='enabled'}}\n[sites.composition.capabilities]\n{entry}[[sources]]"),
                    1,
                )
        };
        config = config.replace(
            "package='search-plugin'\nsource='source:theme'\nmanifest='plugin.toml'",
            "package='independent-plugin'\nsource='source:provider'\nmanifest='plugin.toml'",
        );
        config.push_str(&format!(
            "[[sources]]\nid='source:provider'\n\
             location={{kind='repository',root={:?},commit='{}',subdirectory='selected'}}\n",
            provider
                .root
                .path()
                .to_str()
                .expect("independent provider root"),
            provider.commit,
        ));
        let before = (snapshot(theme.root.path()), snapshot(provider.root.path()));
        let acquired = acquire(&config);
        let checked = acquired
            .check_metadata(toml::Limits::default())
            .expect("independent plugin provider is admitted separately from the Theme");
        let report = serde_json::to_value(checked.inspect()).expect("cross-source report");
        assert_eq!(
            report["theme_packages"]
                .as_object()
                .expect("Theme set")
                .len(),
            1
        );
        assert_eq!(
            report["plugin_packages"]
                .as_object()
                .expect("plugin set")
                .len(),
            1
        );
        assert_eq!(
            report["plugin_packages"]["independent-plugin"]["source"],
            "source:provider"
        );
        assert_eq!(
            report["acquisition"]["sources"]["source:provider"]["verification"]["commit"],
            provider.commit.to_string()
        );
        assert_eq!(
            report["acquisition"]["sources"]["source:theme"]["verification"]["commit"],
            theme.commit.to_string()
        );
        let output = check(&config);
        assert!(output.status.success(), "{scope}: {output:?}");
        assert_eq!(
            serde_json::from_slice::<Value>(&output.stdout).expect("CLI metadata"),
            report
        );
        let theme_nodes = toml::ParsedToml::parse(THEME_MANIFEST, toml::Limits::default())
            .expect("Theme AST")
            .node_count();
        let provider_nodes = toml::ParsedToml::parse(PROVIDER, toml::Limits::default())
            .expect("provider AST")
            .node_count();
        for limits in [
            toml::Limits {
                source_bytes: THEME_MANIFEST.len().max(PROVIDER.len()),
                ..toml::Limits::default()
            },
            toml::Limits {
                nodes: theme_nodes.max(provider_nodes),
                ..toml::Limits::default()
            },
        ] {
            assert_eq!(
                rejected(acquired.check_metadata(limits)).code(),
                "toml_limit",
                "aggregate ceiling across distinct repositories"
            );
        }
        assert!(
            acquired
                .check_metadata(toml::Limits {
                    source_bytes: THEME_MANIFEST.len() + PROVIDER.len(),
                    nodes: theme_nodes + provider_nodes,
                    ..toml::Limits::default()
                })
                .is_ok()
        );
        let wrong_source = config.replace("source='source:provider'", "source='source:theme'");
        assert_eq!(
            rejected(acquire(&wrong_source).check_metadata(toml::Limits::default())).code(),
            "package_selection",
            "configured provider elsewhere cannot satisfy the wrong selected source"
        );
        assert_eq!(
            (snapshot(theme.root.path()), snapshot(provider.root.path())),
            before
        );
    }
}

#[test]
fn one_provider_serving_multiple_capabilities_never_skips_later_contract_checks() {
    let plugin = PLUGIN_MANIFEST.replace(
        "implements=['publication.search/v1']",
        "implements=['commerce.inventory/v1','publication.search/v1']",
    );
    let fixture = Fixture::with_manifests(THEME_MANIFEST, &plugin);
    let config = fixture.config().replace(
        "[[principals]]",
        "'commerce.inventory/v1'={state='enabled',provider={kind='plugin',package='search-plugin',provider='search'},requirement='required'}\n[[principals]]",
    );
    let acquired = acquire(&config);
    let checked = acquired
        .check_metadata(toml::Limits::default())
        .expect("one provider supplies both selected contracts");
    let output = check(&config);
    assert!(output.status.success(), "{output:?}");
    let report = serde_json::to_value(checked.inspect()).expect("shared provider metadata");
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).expect("CLI report"),
        report
    );
    assert_eq!(
        report["plugin_packages"]["search-plugin"]["declaration"]["providers"]["search"]["implements"],
        serde_json::json!(["commerce.inventory/v1", "publication.search/v1"])
    );
    let invalid = config.replace("'commerce.inventory/v1'", "'commerce.selector/v1'");
    let error = rejected(acquire(&invalid).check_metadata(toml::Limits::default()));
    assert_eq!(
        error.code(),
        "package_selection",
        "a previously checked provider cannot authorize a later unsupported capability"
    );
    assert!(error.to_string().contains("does not implement"));
    let output = check(&invalid);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty());
}

#[test]
fn metadata_paths_match_captured_names_without_case_or_unicode_normalization_fallback() {
    for (actual_directory, selected_directory, actual_input, selected_input) in [
        ("templates", "Templates", "search.js", "search.js"),
        ("templates", "templates", "search.js", "Search.js"),
        ("templates", "templates", "café.js", "cafe\u{301}.js"),
        ("thème", "the\u{300}me", "search.js", "search.js"),
    ] {
        let plugin = format!(
            "package='search-plugin'\n[resources.'module:search']\ninputs=['{selected_directory}/page.html','{selected_input}']\nbuild={{kind='browser-module',entry='{selected_input}'}}\n[providers.search]\nimplements=['publication.search/v1']\n"
        );
        let template = format!("{actual_directory}/page.html");
        let fixture = Fixture::files(&[
            ("theme.toml", THEME_MANIFEST.as_bytes()),
            ("plugin.toml", plugin.as_bytes()),
            (&template, b"<p>exact template bytes</p>\n"),
            (actual_input, b"/* exact provider input */\n"),
        ]);
        let config = fixture.config();
        let acquired = acquire(&config);
        let captured = &acquired.sources()[&"source:theme".parse().expect("source")];
        assert!(
            captured
                .file(&template.parse().expect("exact template path"))
                .is_some()
        );
        assert!(
            captured
                .file(&actual_input.parse().expect("exact provider path"))
                .is_some()
        );
        let error = rejected(acquired.check_metadata(toml::Limits::default()));
        assert_eq!(
            error.code(),
            "package_input",
            "metadata spelling is not native filesystem alias resolution"
        );
        let expected_path = if actual_directory != selected_directory {
            format!("{selected_directory}/page.html")
        } else {
            selected_input.to_owned()
        };
        match error {
            build::MetadataError::Input { path, .. } => {
                assert_eq!(path.as_str(), expected_path)
            }
            other => panic!("precise rejected input required: {other}"),
        }
        let output = check(&config);
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(output.stdout.is_empty());
    }
}
