use super::*;

#[test]
fn file_reader_rejects_changed_input_before_returning_a_digest() {
    let source = tempfile::tempdir().expect("source");
    let path = source.path().join("file");
    std::fs::write(&path, b"abc").expect("file");
    let mut handle = File::open(&path).expect("open");
    let before = handle.metadata().expect("metadata");
    std::fs::write(&path, b"different bytes").expect("concurrent edit");
    assert!(matches!(
        hash_file(&mut handle, &path, &before, 64, None),
        Err(Error::Changed(_))
    ));
}

#[test]
fn selected_capture_reads_only_exact_named_files_and_required_ancestors() {
    use gazette_ports::source::SelectedSourceCapture;

    let source = tempfile::tempdir().expect("source");
    std::fs::create_dir(source.path().join("sites")).expect("sites directory");
    std::fs::write(source.path().join("gazette.toml"), b"root").expect("root file");
    std::fs::write(source.path().join("sites/one.toml"), b"site").expect("site file");
    std::fs::write(source.path().join("ignored.toml"), b"not selected")
        .expect("unselected sibling");
    let root: EntryPath = "gazette.toml".parse().expect("root path");
    let site: EntryPath = "sites/one.toml".parse().expect("site path");

    let captured = FileSystem
        .capture_selected(
            source.path(),
            &[site.clone(), root.clone()],
            Limits::default(),
        )
        .expect("exact selected capture");
    let paths: Vec<_> = captured
        .inventory()
        .entries()
        .iter()
        .map(|entry| entry.path.as_str())
        .collect();
    assert_eq!(paths, ["gazette.toml", "sites", "sites/one.toml"]);
    assert_eq!(captured.file(&root), Some(b"root".as_slice()));
    assert_eq!(captured.file(&site), Some(b"site".as_slice()));
    assert!(
        captured
            .inventory()
            .entries()
            .iter()
            .all(|entry| entry.path.as_str() != "ignored.toml")
    );
}

#[test]
fn selected_capture_rejects_duplicates_links_and_case_aliases() {
    use gazette_ports::source::SelectedSourceCapture;
    use std::os::unix::fs::symlink;

    let source = tempfile::tempdir().expect("source");
    std::fs::write(source.path().join("Gazette.toml"), b"exact case").expect("case fixture");
    symlink("Gazette.toml", source.path().join("linked.toml")).expect("link fixture");
    let exact: EntryPath = "Gazette.toml".parse().expect("exact path");
    let alias: EntryPath = "gazette.toml".parse().expect("case alias");
    let link: EntryPath = "linked.toml".parse().expect("link path");

    assert!(matches!(
        FileSystem.capture_selected(source.path(), &[exact.clone(), exact], Limits::default()),
        Err(Error::InvalidInventory {
            reason: "duplicate selected path",
            ..
        })
    ));
    assert!(matches!(
        FileSystem.capture_selected(source.path(), &[link], Limits::default()),
        Err(Error::Unsupported(_))
    ));
    assert!(matches!(
        FileSystem.capture_selected(source.path(), &[alias], Limits::default()),
        Err(Error::InvalidName(_))
    ));
}
