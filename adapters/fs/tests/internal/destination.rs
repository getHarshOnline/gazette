//! Deterministic root-replacement check at the native observation boundary.

use std::{
    fs,
    os::unix::{fs::symlink, net::UnixListener},
};
use tempfile::tempdir;

use gazette_core::{
    destination::Error,
    source::{Content, Limits, Sha256Digest},
};
use gazette_ports::destination::OutputReader;

use super::{DestinationRoots, FileSystem, OutputAdministration, Root, SourceError};

#[test]
fn recheck_rejects_a_replacement_directory_at_the_same_path() {
    let outer = tempdir().expect("fixture");
    let root = outer.path().join("content");
    fs::create_dir(&root).expect("content");
    let observed = Root::observe(&root).expect("observation");
    fs::rename(&root, outer.path().join("original")).expect("move fixture");
    fs::create_dir(&root).expect("replacement");
    assert!(matches!(
        observed.recheck(&root),
        Err(SourceError::Changed(_))
    ));
}

#[test]
fn administration_recheck_allows_directory_updates_but_refuses_in_place_pointer_changes() {
    for pointer in [false, true] {
        let owner = tempdir().expect("real administrative observation");
        let administration = owner.path().join(".git");
        if pointer {
            fs::write(&administration, b"gitdir: ../original\n").expect("pointer");
        } else {
            fs::create_dir(&administration).expect("directory");
        }
        let roots = DestinationRoots::observe_output(owner.path(), &[]).expect("held output root");
        let held =
            OutputAdministration::observe(roots.directory()).expect("held administrative handle");
        let before = roots.binding(&held).expect("stable identity facts");
        if pointer {
            fs::write(&administration, b"gitdir: ../changed-longer-coordinate\n")
                .expect("same pointer inode changed in place");
            assert!(
                matches!(held.recheck(roots.directory()), Err(SourceError::Changed(ref path)) if path == std::path::Path::new(".git"))
            );
            let now = OutputAdministration::observe(roots.directory()).expect("fresh pointer");
            assert_eq!(
                before,
                roots.binding(&now).expect(
                    "identity binding alone does not claim pointer-content continuity across calls"
                )
            );
        } else {
            fs::write(administration.join("native-log"), b"ordinary Git activity")
                .expect("normal directory metadata update");
            held.recheck(roots.directory())
                .expect("administrative directory timestamps/size are not bound");
            let now = OutputAdministration::observe(roots.directory()).expect("same directory");
            assert_eq!(before, roots.binding(&now).expect("stable native identity"));
            assert_eq!(
                FileSystem
                    .read_output(owner.path(), &[], Limits::default())
                    .expect("fresh complete output")
                    .binding(),
                &before
            );
        }
    }
}

#[test]
fn output_reader_excludes_only_root_git_directory_without_traversing_or_mutating_it() {
    let root = tempdir().expect("output fixture");
    let administration = root.path().join(".git");
    fs::create_dir(&administration).expect("root administration");
    let noisy = vec![b'x'; 4096];
    fs::write(administration.join("objects"), &noisy).expect("unrelated administration bytes");
    fs::create_dir(administration.join("nested")).expect("deep administration");
    fs::write(administration.join("nested/.GIT"), b"not output").expect("unrelated internal name");
    symlink("unreadable-absent-target", administration.join("link"))
        .expect("administrative symlink");
    fs::write(root.path().join("index.html"), b"a").expect("user output");
    let inventory = FileSystem
        .read_output(
            root.path(),
            &[],
            Limits {
                entries: 1,
                depth: 1,
                file_bytes: 1,
                total_bytes: 1,
            },
        )
        .expect("administration is neither traversed nor charged as output");
    assert_eq!(inventory.inventory().entries().len(), 1);
    assert_eq!(
        inventory.inventory().entries()[0].path.as_str(),
        "index.html"
    );
    assert_eq!(
        inventory.inventory().entries()[0].content,
        Content::File {
            bytes: 1,
            sha256: Sha256Digest::of(b"a")
        }
    );
    assert_eq!(inventory.inventory().total_bytes(), 1);
    assert_eq!(
        fs::read(administration.join("objects")).expect("preserved administration"),
        noisy
    );
    assert_eq!(
        fs::read(administration.join("nested/.GIT")).expect("preserved internal bytes"),
        b"not output"
    );
    assert_eq!(
        fs::read_link(administration.join("link")).expect("symlink not followed or replaced"),
        std::path::Path::new("unreadable-absent-target")
    );
}

#[test]
fn output_reader_preserves_git_pointer_and_observes_every_user_file_without_adopting_a_marker() {
    let root = tempdir().expect("output fixture");
    let pointer = b"gitdir: ../absent-administration\n";
    fs::write(root.path().join(".git"), pointer).expect("worktree pointer");
    fs::write(root.path().join(".gazette-owned"), b"not authority").expect("user marker");
    fs::write(root.path().join("user.txt"), b"retained user bytes").expect("unowned user file");
    let inventory = FileSystem
        .read_output(root.path(), &[], Limits::default())
        .expect("pointer is held, never interpreted as a repository or ownership grant");
    assert_eq!(
        inventory
            .inventory()
            .entries()
            .iter()
            .map(|entry| entry.path.as_str())
            .collect::<Vec<_>>(),
        [".gazette-owned", "user.txt"]
    );
    assert_eq!(
        inventory.inventory().entries()[0].content,
        Content::File {
            bytes: 13,
            sha256: Sha256Digest::of(b"not authority")
        }
    );
    assert_eq!(
        inventory.inventory().entries()[1].content,
        Content::File {
            bytes: 19,
            sha256: Sha256Digest::of(b"retained user bytes")
        }
    );
    assert_eq!(
        fs::read(root.path().join(".git")).expect("unchanged pointer bytes"),
        pointer
    );
    assert_eq!(
        fs::read(root.path().join("user.txt")).expect("unchanged user file"),
        b"retained user bytes"
    );
}

#[test]
fn output_reader_without_git_still_enforces_all_inventory_limits() {
    let root = tempdir().expect("output fixture");
    fs::create_dir(root.path().join("nested")).expect("output directory");
    fs::write(root.path().join("nested/user.txt"), b"user").expect("user file");
    let exact = Limits {
        entries: 2,
        depth: 2,
        file_bytes: 4,
        total_bytes: 4,
    };
    let inventory = FileSystem
        .read_output(root.path(), &[], exact)
        .expect("Git absence is valid");
    assert_eq!(
        inventory
            .inventory()
            .entries()
            .iter()
            .map(|entry| entry.path.as_str())
            .collect::<Vec<_>>(),
        ["nested", "nested/user.txt"]
    );
    assert_eq!(inventory.inventory().total_bytes(), 4);
    for limits in [
        Limits {
            entries: 1,
            ..exact
        },
        Limits { depth: 1, ..exact },
        Limits {
            file_bytes: 3,
            ..exact
        },
        Limits {
            total_bytes: 3,
            ..exact
        },
    ] {
        assert!(
            matches!(
                FileSystem.read_output(root.path(), &[], limits),
                Err(Error::Observation(SourceError::Limit { .. }))
            ),
            "user inventory remains bounded under {limits:?}"
        );
    }
    assert!(
        !root.path().join(".git").exists(),
        "observation creates no administration"
    );
    assert_eq!(
        fs::read(root.path().join("nested/user.txt")).expect("unmodified file"),
        b"user"
    );
}

#[test]
fn output_reader_refuses_nested_and_case_variant_git_entries() {
    for path in [".GIT", ".Git", "nested/.git", "nested/.GIT"] {
        for directory in [false, true] {
            let root = tempdir().expect("output fixture");
            fs::create_dir(root.path().join("nested")).expect("user directory");
            let entry = root.path().join(path);
            if directory {
                fs::create_dir(&entry).expect("invalid administrative directory");
            } else {
                fs::write(&entry, b"not permitted output").expect("invalid administrative file");
            }
            let error = FileSystem
                .read_output(root.path(), &[], Limits::default())
                .expect_err("only exact root .git is excluded");
            assert!(
                matches!(error, Error::Observation(SourceError::InvalidName(ref invalid)) if invalid == std::path::Path::new(path)),
                "exact refused path {path}: {error}"
            );
        }
    }
}

#[test]
fn output_reader_refuses_root_administration_and_descendant_symlinks_and_special_files() {
    let root = tempdir().expect("output fixture");
    let link = root.path().join("linked-root");
    let target = root.path().join("target");
    fs::create_dir(&target).expect("target directory");
    symlink(&target, &link).expect("root symlink");
    for spelling in [
        link.clone(),
        link.join("."),
        std::path::PathBuf::from(format!("{}/", link.display())),
    ] {
        assert!(
            matches!(
                FileSystem.read_output(&spelling, &[], Limits::default()),
                Err(Error::Observation(SourceError::Io { .. }))
            ),
            "root symlink cannot be disguised by separators"
        );
    }
    for name in [".git", "user-link"] {
        let output = tempdir().expect("symlink output fixture");
        symlink(&target, output.path().join(name)).expect("unsafe descendant");
        assert!(
            matches!(FileSystem.read_output(output.path(), &[], Limits::default()), Err(Error::Observation(SourceError::Unsupported(ref path))) if path == std::path::Path::new(name))
        );
    }
    for name in [".git", "user-socket"] {
        let output = tempdir().expect("special output fixture");
        let path = output.path().join(name);
        let listener = UnixListener::bind(&path).expect("native special file");
        assert!(
            matches!(FileSystem.read_output(output.path(), &[], Limits::default()), Err(Error::Observation(SourceError::Unsupported(ref path))) if path == std::path::Path::new(name))
        );
        assert!(
            FileSystem
                .read_output(&path, &[], Limits::default())
                .is_err(),
            "special root is not an output directory"
        );
        drop(listener);
    }
}

#[test]
fn output_reader_preserves_protected_source_separation_in_both_ancestry_directions() {
    let root = tempdir().expect("separation fixture");
    let output = root.path().join("output");
    let nested = output.join("protected");
    let independent = root.path().join("independent");
    fs::create_dir_all(&nested).expect("nested source");
    fs::create_dir(&independent).expect("disjoint source");
    fs::write(output.join("user.txt"), b"untouched").expect("user output");
    for protected in [output.clone(), nested, root.path().to_owned()] {
        assert!(
            matches!(FileSystem.read_output(&output, std::slice::from_ref(&protected), Limits::default()), Err(Error::SourceOverlap { source, destination }) if source == protected && destination == output),
            "equal, contained and containing sources all remain protected"
        );
    }
    let inventory = FileSystem
        .read_output(&output, &[independent], Limits::default())
        .expect("explicit disjoint source is allowed");
    assert_eq!(inventory.inventory().total_bytes(), 9);
    assert_eq!(
        fs::read(output.join("user.txt")).expect("no mutation"),
        b"untouched"
    );
}
