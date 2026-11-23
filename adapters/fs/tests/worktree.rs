//! Deployment output uses the ordinary confined reader, not Git ignore rules.

use gazette_adapter_fs::FileSystem;
use gazette_core::{
    destination,
    source::{Error as SourceError, Limits, Sha256Digest},
};
use gazette_ports::{release::WorktreeReader, source::SourceReader};
use std::{fs, os::unix::fs::symlink};

#[test]
fn only_root_metadata_is_omitted_and_source_reads_are_unchanged() {
    let root = tempfile::tempdir().expect("disposable worktree");
    fs::write(root.path().join("payload.txt"), b"exact\r\n\0\xff")
        .expect("write exact binary payload");
    fs::write(root.path().join(".gitignore"), b"payload.txt\n")
        .expect("write ignored-payload rule");
    let expected = FileSystem
        .read(root.path(), None, Limits::default())
        .expect("observe complete source before metadata");
    fs::create_dir(root.path().join(".git")).expect("create real administrative directory");
    fs::write(root.path().join(".git/private"), b"not publication output")
        .expect("write private administrative bytes");
    let output = FileSystem
        .read_worktree(root.path(), &[], Limits::default())
        .expect("observe publication output with root metadata excluded");
    assert_eq!(output, expected);
    let ordinary = FileSystem
        .read(root.path(), None, Limits::default())
        .expect("ordinary source still includes administrative bytes");
    assert_ne!(ordinary.identity(), output.identity());
    assert!(
        ordinary
            .entries()
            .iter()
            .any(|entry| entry.path.as_str() == ".git/private")
    );
    // This reader intentionally makes no claim that the administrative bytes are Git-valid.
    assert_eq!(
        fs::read(root.path().join(".git/private")).expect("read preserved private metadata"),
        b"not publication output"
    );
}

#[test]
fn absent_file_link_and_aliased_worktree_metadata_are_not_accepted() {
    let root = tempfile::tempdir().expect("create disposable worktree root");
    let other = tempfile::tempdir().expect("create independent symlink target");
    assert!(
        FileSystem
            .read_worktree(root.path(), &[], Limits::default())
            .is_err()
    );
    let git = root.path().join(".git");
    fs::write(&git, b"gitdir: elsewhere\n").expect("write forbidden metadata pointer file");
    assert!(
        FileSystem
            .read_worktree(root.path(), &[], Limits::default())
            .is_err()
    );
    fs::remove_file(&git).expect("remove disposable pointer before symlink fixture");
    symlink(other.path(), &git).expect("create forbidden metadata symlink");
    assert!(
        FileSystem
            .read_worktree(root.path(), &[], Limits::default())
            .is_err()
    );
    fs::remove_file(&git).expect("remove disposable metadata symlink");
    fs::create_dir(&git).expect("create real metadata directory");
    let alias = other.path().join("worktree-alias");
    symlink(root.path(), &alias).expect("create forbidden root symlink");
    assert!(
        FileSystem
            .read_worktree(&alias, &[], Limits::default())
            .is_err()
    );
}

#[test]
fn nested_metadata_and_symlinked_output_fail_without_partial_inventory() {
    let root = tempfile::tempdir().expect("create nested-metadata fixture root");
    fs::create_dir(root.path().join(".git")).expect("create root metadata directory");
    fs::write(root.path().join("index.html"), b"valid first output")
        .expect("write output that must survive refusal");
    fs::create_dir_all(root.path().join("nested/.git")).expect("create forbidden nested metadata");
    assert!(
        FileSystem
            .read_worktree(root.path(), &[], Limits::default())
            .is_err()
    );
    fs::remove_dir(root.path().join("nested/.git"))
        .expect("remove disposable nested metadata before symlink fixture");
    symlink("../index.html", root.path().join("nested/link"))
        .expect("create forbidden nested output symlink");
    assert!(
        FileSystem
            .read_worktree(root.path(), &[], Limits::default())
            .is_err()
    );
    assert_eq!(
        fs::read(root.path().join("index.html")).expect("read preserved output after refusals"),
        b"valid first output"
    );
}

#[test]
fn output_changes_and_ignored_untracked_files_change_exact_identity() {
    let root = tempfile::tempdir().expect("create output-identity fixture root");
    fs::create_dir(root.path().join(".git")).expect("create real root metadata");
    fs::write(root.path().join(".gitignore"), b"*.tmp\n").expect("write ignored-extension rule");
    fs::write(root.path().join("page.html"), b"before").expect("write original page bytes");
    let first = FileSystem
        .read_worktree(root.path(), &[], Limits::default())
        .expect("observe original complete output");
    fs::write(root.path().join("page.html"), b"after").expect("change actual page bytes");
    let second = FileSystem
        .read_worktree(root.path(), &[], Limits::default())
        .expect("observe changed complete output");
    assert_ne!(first.identity(), second.identity());
    fs::write(root.path().join("secret.tmp"), b"must be inspected")
        .expect("add ignored untracked file");
    let third = FileSystem
        .read_worktree(root.path(), &[], Limits::default())
        .expect("observe all output including ignored file");
    assert_ne!(second.identity(), third.identity());
    assert!(
        third
            .entries()
            .iter()
            .any(|entry| entry.path.as_str() == "secret.tmp")
    );
    assert_eq!(
        Sha256Digest::of(
            &fs::read(root.path().join("page.html")).expect("read changed page bytes")
        ),
        Sha256Digest::of(b"after")
    );
}

#[test]
fn source_overlap_is_rejected_in_both_directions() {
    let root = tempfile::tempdir().expect("create source-overlap fixture root");
    let child = root.path().join("child");
    fs::create_dir_all(child.join(".git")).expect("create nested independent metadata fixture");
    fs::create_dir(root.path().join(".git")).expect("create parent root metadata");
    assert!(
        FileSystem
            .read_worktree(root.path(), std::slice::from_ref(&child), Limits::default())
            .is_err()
    );
    assert!(
        FileSystem
            .read_worktree(&child, &[root.path().to_owned()], Limits::default())
            .is_err()
    );
    assert!(
        FileSystem
            .read_worktree(&child, std::slice::from_ref(&child), Limits::default())
            .is_err()
    );
}

#[test]
fn complete_output_limits_apply_even_when_metadata_is_excluded() {
    let root = tempfile::tempdir().expect("create output-bound fixture root");
    fs::create_dir(root.path().join(".git")).expect("create excluded metadata directory");
    fs::write(root.path().join(".git/native"), vec![1; 1024])
        .expect("write excluded metadata bytes");
    fs::write(root.path().join("page"), b"12345").expect("write exactly five output bytes");
    let mut limits = Limits {
        total_bytes: 5,
        ..Default::default()
    };
    assert!(FileSystem.read_worktree(root.path(), &[], limits).is_ok());
    limits.total_bytes = 4;
    assert!(FileSystem.read_worktree(root.path(), &[], limits).is_err());
}

#[test]
fn case_variant_metadata_is_never_excluded_as_literal_root_metadata() {
    for relative in [".GIT", ".GiT", "nested/.GIT", "nested/.GiT"] {
        let root = tempfile::tempdir().expect("case-variant fixture root");
        if relative.starts_with("nested/") {
            fs::create_dir(root.path().join(".git")).expect("valid literal root metadata");
        }
        // Root variants stand alone, so this also works on case-insensitive
        // macOS volumes without attempting two colliding sibling directories.
        fs::create_dir_all(root.path().join(relative)).expect("actual case-variant directory");
        fs::write(
            root.path().join(relative).join("private"),
            b"administrative bytes",
        )
        .expect("case-variant metadata payload");
        let ordinary = FileSystem
            .read(root.path(), None, Limits::default())
            .expect("ordinary source reader includes case-variant entries");
        let private_entry = format!("{relative}/private");
        assert!(
            ordinary
                .entries()
                .iter()
                .any(|entry| entry.path.as_str() == private_entry)
        );
        assert!(
            FileSystem
                .read_worktree(root.path(), &[], Limits::default())
                .is_err(),
            "{relative}"
        );
        assert_eq!(
            fs::read(root.path().join(relative).join("private"))
                .expect("case-variant metadata remains untouched"),
            b"administrative bytes"
        );
    }
}

#[test]
fn entry_and_depth_ceilings_cover_the_complete_output_but_not_root_metadata() {
    let root = tempfile::tempdir().expect("structural-limit fixture root");
    fs::create_dir_all(root.path().join(".git/objects/deeper"))
        .expect("excluded metadata deeper than output ceiling");
    fs::write(root.path().join(".git/objects/deeper/private"), b"excluded")
        .expect("excluded metadata does not consume output entries");
    fs::create_dir(root.path().join("nested")).expect("one output directory");
    fs::write(root.path().join("nested/page"), b"page").expect("one nested output file");
    let exact = Limits {
        entries: 2,
        depth: 2,
        ..Default::default()
    };
    let accepted = FileSystem
        .read_worktree(root.path(), &[], exact)
        .expect("two output entries at exact depth two");
    assert_eq!(accepted.entries().len(), 2);
    assert_eq!(accepted.total_bytes(), 4);
    let entries = FileSystem
        .read_worktree(
            root.path(),
            &[],
            Limits {
                entries: 1,
                ..exact
            },
        )
        .expect_err("second real output entry exceeds entry ceiling");
    assert!(matches!(
        entries,
        destination::Error::Observation(SourceError::Limit {
            resource: "entry count",
            ..
        })
    ));
    let depth = FileSystem
        .read_worktree(root.path(), &[], Limits { depth: 1, ..exact })
        .expect_err("nested page exceeds depth ceiling");
    assert!(matches!(
        depth,
        destination::Error::Observation(SourceError::Limit {
            resource: "depth",
            ..
        })
    ));
    assert_eq!(
        FileSystem
            .read_worktree(root.path(), &[], exact)
            .expect("failed limited reads did not alter output"),
        accepted
    );
}
