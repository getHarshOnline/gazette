//! Explicit source exclusions must not become implicit hidden-file ignores.
use gazette_adapter_fs::FileSystem;
use gazette_core::source::Limits;
use gazette_ports::source::SourceCapture;

#[path = "source/inclusions.rs"]
mod inclusions;

#[test]
fn explicit_exclusions_preserve_other_hidden_files_and_bound_the_capture() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(".git")).unwrap();
    std::fs::write(root.path().join(".git/objects"), vec![0; 1024]).unwrap();
    std::fs::write(root.path().join(".keep"), "kept").unwrap();
    std::fs::write(root.path().join("page.md"), "page").unwrap();
    let filtered = FileSystem
        .capture_excluding(root.path(), &[".git".parse().unwrap()], Limits::default())
        .unwrap();
    let ordinary = FileSystem
        .capture(root.path(), None, Limits::default())
        .unwrap();
    assert!(filtered.file(&".keep".parse().unwrap()).is_some());
    assert!(filtered.file(&"page.md".parse().unwrap()).is_some());
    assert!(filtered.file(&".git/objects".parse().unwrap()).is_none());
    assert!(ordinary.file(&".git/objects".parse().unwrap()).is_some());
    assert!(
        FileSystem
            .capture_excluding(root.path(), &[], Limits::default())
            .is_err()
    );
    assert!(
        FileSystem
            .capture_excluding(
                root.path(),
                &[".git".parse().unwrap(), ".git".parse().unwrap()],
                Limits::default()
            )
            .is_err()
    );
}
