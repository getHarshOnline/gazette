use super::*;
use gazette_sdk::authoring::session::native::handoff::{self, Manifest, Overlay};

fn manifest(editor: &Workspace) -> Manifest {
    serde_json::from_value(serde_json::to_value(editor.session().snapshot()).unwrap()).unwrap()
}

fn changed(editor: &mut Workspace, root: &Path) -> Overlay {
    let path = root.join("pages/one.md");
    let text = PAGE.replace("# One", "# Unsaved café 🦀");
    let id = id(&path).unwrap();
    editor.open(&id, &path, 7, text.clone()).unwrap();
    Overlay {
        id,
        version: 7,
        text,
    }
}

#[test]
fn only_changed_text_recreates_clean_open_versions_and_opaque_observations() {
    let root = fixture(CONFIG);
    fs::File::create(root.path().join("pages/video.mp4"))
        .unwrap()
        .set_len(64_u64 << 30)
        .unwrap();
    let mut editor = open(root.path());
    let configuration = root.path().join("gazette.toml");
    editor
        .open(
            &id(&configuration).unwrap(),
            &configuration,
            4,
            CONFIG.into(),
        )
        .unwrap();
    let overlay = changed(&mut editor, root.path());
    let pins = manifest(&editor);
    let imported = handoff::restore(
        editor.handoff_selection(),
        &pins,
        vec![overlay],
        id,
        Limits::default(),
    )
    .unwrap();
    let mut actual = serde_json::to_value(imported.snapshot()).unwrap();
    actual["generation"] = serde_json::json!(pins.generation);
    assert_eq!(actual, serde_json::to_value(pins).unwrap());
    assert_eq!(
        fs::read_to_string(root.path().join("pages/one.md")).unwrap(),
        PAGE
    );
}

#[test]
fn changed_saved_text_is_refused_even_when_an_overlay_could_hide_it() {
    let root = fixture(CONFIG);
    let mut editor = open(root.path());
    let overlay = changed(&mut editor, root.path());
    let pins = manifest(&editor);
    fs::write(
        root.path().join("pages/one.md"),
        PAGE.replace("# One", "# Someone else's save"),
    )
    .unwrap();
    assert!(
        handoff::restore(
            editor.handoff_selection(),
            &pins,
            vec![overlay],
            id,
            Limits::default()
        )
        .is_err()
    );
}

#[test]
fn missing_extra_duplicate_or_redundant_overlay_never_returns_a_partial_session() {
    let root = fixture(CONFIG);
    let mut editor = open(root.path());
    let overlay = changed(&mut editor, root.path());
    let pins = manifest(&editor);
    assert!(
        handoff::restore(
            editor.handoff_selection(),
            &pins,
            vec![],
            id,
            Limits::default()
        )
        .is_err()
    );
    let duplicate = Overlay {
        id: overlay.id.clone(),
        version: overlay.version,
        text: overlay.text.clone(),
    };
    assert!(
        handoff::restore(
            editor.handoff_selection(),
            &pins,
            vec![overlay, duplicate],
            id,
            Limits::default()
        )
        .is_err()
    );
    let editor = open(root.path());
    let extra = Overlay {
        id: id(&root.path().join("pages/one.md")).unwrap(),
        version: 1,
        text: PAGE.into(),
    };
    assert!(
        handoff::restore(
            editor.handoff_selection(),
            &manifest(&editor),
            vec![extra],
            id,
            Limits::default()
        )
        .is_err()
    );
}

#[test]
fn machine_binding_changes_are_detected_before_reconstruction() {
    let config = format!(
        "machine_binding='config/machine.toml'\n{}",
        CONFIG.replace("root='pages'", "root='machine:pages'")
    );
    let root = fixture(&config);
    fs::create_dir(root.path().join("config")).unwrap();
    let binding = root.path().join("config/machine.toml");
    fs::write(&binding, "[roots]\npages='../pages'\n").unwrap();
    let editor = open(root.path());
    let pins = manifest(&editor);
    assert!(
        handoff::restore(
            editor.handoff_selection(),
            &pins,
            vec![],
            id,
            Limits::default()
        )
        .is_ok()
    );
    fs::write(binding, "# Later binding\n[roots]\npages='../pages'\n").unwrap();
    assert!(
        handoff::restore(
            editor.handoff_selection(),
            &pins,
            vec![],
            id,
            Limits::default()
        )
        .is_err()
    );
}

#[test]
fn newly_discovered_or_removed_sources_and_changed_asset_observations_are_refused() {
    for case in ["new", "removed", "asset"] {
        let root = fixture(CONFIG);
        let asset = root.path().join("pages/video.mp4");
        fs::write(&asset, [0]).unwrap();
        let editor = open(root.path());
        let pins = manifest(&editor);
        match case {
            "new" => fs::write(
                root.path().join("pages/two.md"),
                PAGE.replace("content:one", "content:two"),
            )
            .unwrap(),
            "removed" => fs::remove_file(root.path().join("pages/one.md")).unwrap(),
            _ => fs::write(asset, [0, 1]).unwrap(),
        }
        assert!(
            handoff::restore(
                editor.handoff_selection(),
                &pins,
                vec![],
                id,
                Limits::default()
            )
            .is_err(),
            "{case}"
        );
    }
}

#[test]
fn unsaved_configuration_repair_is_replayed_before_content() {
    let root = fixture("includes=['missing.toml']\n");
    fs::write(root.path().join("site.toml"), CONFIG).unwrap();
    let mut editor = open(root.path());
    let path = root.path().join("gazette.toml");
    let text = "includes=['site.toml']\n".to_owned();
    let root_id = id(&path).unwrap();
    editor.open(&root_id, &path, 2, text.clone()).unwrap();
    let page = changed(&mut editor, root.path());
    let overlays = vec![
        page,
        Overlay {
            id: root_id,
            version: 2,
            text,
        },
    ];
    let imported = handoff::restore(
        editor.handoff_selection(),
        &manifest(&editor),
        overlays,
        id,
        Limits::default(),
    )
    .unwrap();
    assert!(imported.snapshot().structurally_valid);
}

#[test]
fn saved_corpus_above_transfer_ceiling_replays_one_small_edit_with_native_limits() {
    let root = fixture(CONFIG);
    for number in 0..26 {
        let text = format!(
            "{}\n{}\n",
            PAGE.replace("content:one", &format!("content:page-{number}")),
            "x".repeat(512 * 1024)
        );
        fs::write(root.path().join(format!("pages/page-{number}.md")), text).unwrap();
    }
    let mut editor = open(root.path());
    let overlay = changed(&mut editor, root.path());
    let pins = manifest(&editor);
    assert!(
        pins.sources
            .iter()
            .map(|source| source.bytes)
            .sum::<usize>()
            > 12 * 1024 * 1024
    );
    assert!(overlay.text.len() < 1024);
    let imported = handoff::restore(
        editor.handoff_selection(),
        &pins,
        vec![overlay],
        id,
        Limits::default(),
    )
    .unwrap();
    assert_eq!(imported.snapshot().sources.len(), pins.sources.len());
    assert_eq!(
        imported.files()[&id(&root.path().join("pages/one.md")).unwrap()].version(),
        Some(7)
    );
    assert!(
        handoff::restore(
            editor.handoff_selection(),
            &pins,
            vec![changed(&mut editor, root.path())],
            id,
            Limits {
                source_bytes: 12 * 1024 * 1024,
                ..Limits::default()
            }
        )
        .is_err(),
        "compact transfer does not bypass native retention limits"
    );
}
