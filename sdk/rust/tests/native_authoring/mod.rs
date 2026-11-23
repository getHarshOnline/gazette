//! Native editor discovery retains overlays, exact roles and metadata-only assets.

use gazette_sdk::authoring::session::{Limits, native::Workspace};
use std::{fs, path::Path};

mod handoff;

const CONFIG: &str = "[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n[[sources]]\nid='source:pages'\nlocation={kind='filesystem',root='pages'}\n[[brands]]\nid='brand:one'\nname='Example'\n[[brands.publications]]\nsite='site:one'\nauthored=[{source='source:pages',target='content'}]\n";
const PAGE: &str = "+++\n[content]\nid='content:one'\nsite='site:one'\ntitle='One'\nlifecycle='draft'\nstructure={kind='page',family={kind='article'}}\n+++\n# One\n";

fn id(path: &Path) -> Result<String, &'static str> {
    path.to_str()
        .map(str::to_owned)
        .ok_or("non-UTF-8 test path")
}

fn fixture(config: &str) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("pages")).unwrap();
    fs::write(root.path().join("gazette.toml"), config).unwrap();
    fs::write(root.path().join("pages/one.md"), PAGE).unwrap();
    root
}

fn open(root: &Path) -> Workspace {
    Workspace::open_workspace(
        root.to_owned(),
        "gazette.toml".parse().unwrap(),
        vec![],
        id,
        Limits::default(),
    )
    .unwrap()
}

#[test]
fn discovers_new_pages_on_first_open_and_never_reads_asset_payloads() {
    let root = fixture(CONFIG);
    let asset = root.path().join("pages/film.mp4");
    fs::File::create(&asset)
        .unwrap()
        .set_len(64_u64 << 30)
        .unwrap();
    let mut editor = open(root.path());
    assert_eq!(editor.session().files().len(), 2);
    assert_eq!(
        editor.session().asset_observations()[&id(&asset).unwrap()].observed_bytes(),
        64_u64 << 30
    );
    let new = root.path().join("pages/two.md");
    let text = PAGE.replace("content:one", "content:two");
    fs::write(&new, &text).unwrap();
    let generation = editor.session().generation();
    assert!(
        editor
            .open(&id(&new).unwrap(), &new, 7, text.clone())
            .unwrap()
    );
    assert_eq!(
        editor.session().files()[&id(&new).unwrap()].version(),
        Some(7)
    );
    assert!(editor.session().generation() > generation);
    let outside = root.path().join("unrelated.md");
    fs::write(&outside, PAGE).unwrap();
    let generation = editor.session().generation();
    assert!(
        !editor
            .open(&id(&outside).unwrap(), &outside, 1, PAGE.into())
            .unwrap()
    );
    assert_eq!(editor.session().generation(), generation);
}

#[test]
fn malformed_configuration_recovers_from_unsaved_text_and_close_restores_native_baseline() {
    let root = fixture("[[principals\n");
    let mut editor = open(root.path());
    let path = root.path().join("gazette.toml");
    let root_id = id(&path).unwrap();
    assert_eq!(editor.session().files().len(), 1);
    editor
        .open(&root_id, &path, 1, "[[principals\n".into())
        .unwrap();
    editor.change(&root_id, 2, CONFIG.into()).unwrap();
    assert_eq!(editor.session().files().len(), 2);
    assert_eq!(fs::read_to_string(&path).unwrap(), "[[principals\n");
    let page = root.path().join("pages/one.md");
    let page_id = id(&page).unwrap();
    let overlay = PAGE.replace("# One", "# Unsaved");
    editor.open(&page_id, &page, 3, overlay.clone()).unwrap();
    editor.change(&root_id, 3, "[[principals\n".into()).unwrap();
    assert_eq!(editor.session().files()[&page_id].text(), overlay);
    editor.change(&root_id, 4, CONFIG.into()).unwrap();
    assert_eq!(editor.session().files()[&page_id].version(), Some(3));
    assert_eq!(editor.session().files()[&page_id].text(), overlay);
    editor.close(&root_id).unwrap();
    assert_eq!(editor.session().files()[&root_id].text(), "[[principals\n");
}

#[test]
fn missing_includes_keep_root_editable_and_unsaved_exact_includes_select_new_files() {
    let root = fixture("includes=['missing.toml']\n");
    let mut editor = open(root.path());
    let path = root.path().join("gazette.toml");
    let root_id = id(&path).unwrap();
    assert!(
        editor
            .configuration_paths()
            .any(|path| path == root.path().join("missing.toml"))
    );
    editor
        .open(&root_id, &path, 1, "includes=['missing.toml']\n".into())
        .unwrap();
    fs::write(root.path().join("missing.toml"), CONFIG).unwrap();
    assert!(
        editor
            .select_path(&root.path().join("missing.toml"))
            .unwrap()
    );
    fs::write(root.path().join("site.toml"), CONFIG).unwrap();
    fs::write(root.path().join("unselected.toml"), [0xff]).unwrap();
    editor
        .change(&root_id, 2, "includes=['site.toml']\n".into())
        .unwrap();
    assert_eq!(editor.session().files().len(), 3);
    assert!(
        editor
            .session()
            .files()
            .contains_key(&id(&root.path().join("site.toml")).unwrap())
    );
    assert!(
        !editor
            .session()
            .files()
            .contains_key(&id(&root.path().join("unselected.toml")).unwrap())
    );
}

#[test]
fn machine_binding_stays_separate_and_relative_native_roots_match_editor_paths() {
    let root = fixture(&format!(
        "machine_binding='config/machine.toml'\n{}",
        CONFIG.replace("root='pages'", "root='machine:pages'")
    ));
    fs::create_dir(root.path().join("config")).unwrap();
    let binding = root.path().join("config/machine.toml");
    fs::write(&binding, "[roots]\npages='../pages'\n").unwrap();
    let mut editor = open(root.path());
    assert_eq!(editor.session().files().len(), 2);
    assert!(
        !editor
            .session()
            .files()
            .contains_key(&id(&binding).unwrap())
    );
    assert!(editor.configuration_paths().any(|path| path == binding));
    let page = root.path().join("pages/two.md");
    let text = PAGE.replace("content:one", "content:two");
    fs::write(&page, &text).unwrap();
    assert!(editor.open(&id(&page).unwrap(), &page, 4, text).unwrap());
}

#[test]
fn failed_discovery_preserves_open_text_and_recovers_without_stale_analysis() {
    let root = fixture(CONFIG);
    let mut editor = open(root.path());
    let page = root.path().join("pages/one.md");
    let page_id = id(&page).unwrap();
    editor.open(&page_id, &page, 10, PAGE.into()).unwrap();
    fs::rename(root.path().join("pages"), root.path().join("held")).unwrap();
    editor.refresh().unwrap();
    assert_eq!(editor.session().files()[&page_id].version(), Some(10));
    assert!(
        !editor
            .session()
            .analysis()
            .as_ref()
            .is_ok_and(|analysis| analysis.structurally_valid())
    );
    editor
        .change(&page_id, 11, PAGE.replace("# One", "# Current"))
        .unwrap();
    fs::rename(root.path().join("held"), root.path().join("pages")).unwrap();
    editor.refresh().unwrap();
    assert_eq!(editor.session().files()[&page_id].version(), Some(11));
    assert!(
        editor.session().files()[&page_id]
            .text()
            .contains("# Current")
    );
}

#[test]
fn conflicting_physical_roles_are_refused_without_choosing_a_placement() {
    let config = CONFIG.replace("authored=[{source='source:pages',target='content'}]", "authored=[{source='source:pages',target='content'},{source='source:pages',target='other'}]");
    let root = fixture(&config);
    let editor = open(root.path());
    assert_eq!(editor.session().files().len(), 1);
    assert!(
        !editor
            .session()
            .analysis()
            .as_ref()
            .is_ok_and(|analysis| analysis.structurally_valid())
    );
}

#[test]
fn exact_manuals_and_package_manifests_leave_unselected_siblings_unopened() {
    let config = format!(
        "{CONFIG}\n[brands.publications.documentation_repository]\nname='manual'\npath='output'\n[brands.publications.documentation_repository.build]\ntarget={{kind='mintlify',theme='mint',primary_color='#123abc'}}\n[[brands.publications.documentation_repository.build.modules]]\nproject='project:one'\nslug='example'\ntitle='Example'\nmanual={{source='source:manual'}}\nnavigation=[{{kind='manual',title='Start',pages=[{{title='Overview',route='example/overview',input={{kind='file',path='README.md'}}}}]}}]\n[[projects]]\nid='project:one'\nlifecycle='active'\nvisibility='public'\navailability='available'\n[[sources]]\nid='source:manual'\nlocation={{kind='filesystem',root='manual'}}\n[[sources]]\nid='source:packages'\nlocation={{kind='filesystem',root='packages'}}\n[[theme_packages]]\npackage='theme:one'\nsource='source:packages'\nmanifest='theme.toml'\n"
    );
    let config = format!(
        "{config}\n[[sources]]\nid='source:packages-two'\nlocation={{kind='filesystem',root='packages-two'}}\n[[theme_packages]]\npackage='theme:two'\nsource='source:packages-two'\nmanifest='theme.toml'\n"
    );
    let root = fixture(&config);
    for directory in ["manual", "packages", "packages-two"] {
        fs::create_dir(root.path().join(directory)).unwrap();
        fs::write(root.path().join(directory).join("unselected.md"), [0xff]).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("absent", root.path().join(directory).join("node_modules"))
            .unwrap();
    }
    fs::write(root.path().join("manual/README.md"), "# Manual").unwrap();
    fs::write(root.path().join("manual/support.md"), "# Support").unwrap();
    fs::write(root.path().join("packages/theme.toml"), "id='theme:one'\n").unwrap();
    fs::write(
        root.path().join("packages-two/theme.toml"),
        "id='theme:two'\n",
    )
    .unwrap();
    let editor = Workspace::open_workspace(
        root.path().to_owned(),
        "gazette.toml".parse().unwrap(),
        vec![gazette_sdk::authoring::session::native::ManualSelection {
            source: "source:manual".parse().unwrap(),
            path: "support.md".parse().unwrap(),
        }],
        id,
        Limits::default(),
    )
    .unwrap();
    assert_eq!(editor.session().files().len(), 6);
    let manual = id(&root.path().join("manual/README.md")).unwrap();
    let support = id(&root.path().join("manual/support.md")).unwrap();
    assert!(editor.session().is_manual(&manual));
    assert!(editor.session().is_manual(&support));
    assert!(
        editor
            .session()
            .package_baseline(&id(&root.path().join("packages/theme.toml")).unwrap())
            .is_some()
    );
    let first = &editor.session().files()[&id(&root.path().join("packages/theme.toml")).unwrap()];
    let second =
        &editor.session().files()[&id(&root.path().join("packages-two/theme.toml")).unwrap()];
    assert_ne!(
        first.path(),
        second.path(),
        "same-named manifests keep their package identities"
    );
}

#[test]
fn retained_only_text_is_observed_and_source_filters_apply_to_discovery() {
    let config = CONFIG
        .replace(
            "kind='filesystem',root='pages'",
            "kind='filtered-filesystem',root='pages',exclude=['excluded']",
        )
        .replace(
            "target='content'",
            "target='content',retained_only=['private.md']",
        );
    let root = fixture(&config);
    fs::write(root.path().join("pages/private.md"), [0xff]).unwrap();
    fs::create_dir(root.path().join("pages/excluded")).unwrap();
    fs::write(root.path().join("pages/excluded/two.md"), [0xff]).unwrap();
    let editor = open(root.path());
    assert_eq!(editor.session().files().len(), 2);
    assert_eq!(editor.session().asset_observations().len(), 1);
    assert!(
        editor
            .session()
            .asset_observations()
            .contains_key(&id(&root.path().join("pages/private.md")).unwrap())
    );
}

#[test]
#[ignore = "requires an explicitly selected native workspace; performs no source writes"]
fn existing_workspace_startup_and_unsaved_repair() {
    #[derive(serde::Deserialize)]
    struct Support {
        source: gazette_sdk::workspace::Key,
        path: gazette_sdk::configuration::EntryPath,
        expected_native: std::path::PathBuf,
        #[serde(default)]
        navigation: bool,
    }
    let root = std::path::PathBuf::from(
        std::env::var_os("GAZETTE_AUTHORING_WORKSPACE").expect("explicit workspace path"),
    );
    let support: Vec<Support> = std::env::var_os("GAZETTE_AUTHORING_MANUALS")
        .map(|path| serde_json::from_slice(&fs::read(path).unwrap()).unwrap())
        .unwrap_or_default();
    let manuals = support
        .iter()
        .filter(|manual| !manual.navigation)
        .map(
            |manual| gazette_sdk::authoring::session::native::ManualSelection {
                source: manual.source.clone(),
                path: manual.path.clone(),
            },
        )
        .collect();
    let mut editor = Workspace::open_workspace(
        root.clone(),
        "gazette.toml".parse().unwrap(),
        manuals,
        id,
        Limits::default(),
    )
    .unwrap();
    assert!(
        editor.session().analysis().is_ok(),
        "{:?}",
        editor.session().analysis().as_ref().err()
    );
    let count = editor.session().files().len();
    assert!(count > 1);
    for manual in support {
        assert!(
            editor
                .session()
                .is_manual(&id(&manual.expected_native).unwrap()),
            "retained manual changed native identity"
        );
    }
    let root_path = root.join("gazette.toml");
    let root_id = id(&root_path).unwrap();
    let text = editor.session().files()[&root_id].text().to_owned();
    let generation = editor.session().generation();
    editor.open(&root_id, &root_path, 1, text.clone()).unwrap();
    editor.change(&root_id, 2, "[[principals\n".into()).unwrap();
    assert!(
        !editor
            .session()
            .analysis()
            .as_ref()
            .is_ok_and(|analysis| analysis.structurally_valid())
    );
    editor.change(&root_id, 3, text.clone()).unwrap();
    assert!(editor.session().analysis().is_ok());
    assert_eq!(editor.session().files().len(), count);
    assert!(editor.session().generation() > generation);
    assert_eq!(fs::read_to_string(root_path).unwrap(), text);
    eprintln!(
        "native editor: {} text files, {} observed assets, {} configuration watch paths",
        count,
        editor.session().asset_observations().len(),
        editor.configuration_paths().count()
    );
}
