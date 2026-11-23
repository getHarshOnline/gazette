//! Existing administrative storage only: no recreation or output adoption.

use super::*;
use gazette_core::release::WorktreeSyncOptions;
use gazette_ports::release::WorktreeSync;

fn sync_options() -> WorktreeSyncOptions {
    WorktreeSyncOptions {
        allow_synchronize: true,
        ..Default::default()
    }
}

#[test]
fn repeated_sync_preserves_all_native_bytes_and_ignores_output_ownership() {
    let temporary = tempfile::tempdir().expect("disposable synchronization fixture");
    let root = temporary.path().join("deployment");
    let input = source();
    stage(&root, &input);
    let report = Git
        .prepare_worktree(&root, input.as_input(), &recipe(), None, options())
        .expect("real independent metadata");
    let before = history::native_files(&root);
    for _ in 0..2 {
        assert_eq!(
            Git.synchronize_worktree(
                &root,
                &report.commit.commit,
                report.commit.output,
                &recipe(),
                sync_options()
            )
            .expect("explicit repeated native flush"),
            report
        );
        assert_eq!(history::native_files(&root), before);
    }
    // Git verifies committed objects, not current publication bytes. The FS owner
    // and application join must reject this changed output separately.
    fs::write(root.join("payload.txt"), b"different output").expect("alter test-owned output");
    let changed = history::native_files(&root);
    assert_eq!(
        Git.synchronize_worktree(
            &root,
            &report.commit.commit,
            report.commit.output,
            &recipe(),
            sync_options()
        )
        .expect("administrative synchronization only"),
        report
    );
    assert_eq!(history::native_files(&root), changed);
}

#[test]
fn permission_and_bounds_refuse_without_native_changes() {
    let temporary = tempfile::tempdir().expect("disposable refusal fixture");
    let root = temporary.path().join("deployment");
    let input = source();
    stage(&root, &input);
    let report = Git
        .prepare_worktree(&root, input.as_input(), &recipe(), None, options())
        .expect("real independent metadata");
    let before = history::native_files(&root);
    let mut metadata_limited = sync_options();
    metadata_limited.limits.output.metadata_bytes = 0;
    let mut content_limited = sync_options();
    content_limited.limits.output.content.total_bytes = 0;
    for options in [
        WorktreeSyncOptions::default(),
        WorktreeSyncOptions {
            storage_bytes: 0,
            ..sync_options()
        },
        WorktreeSyncOptions {
            storage_bytes: 1,
            ..sync_options()
        },
        metadata_limited,
        content_limited,
    ] {
        let error = Git
            .synchronize_worktree(
                &root,
                &report.commit.commit,
                report.commit.output,
                &recipe(),
                options,
            )
            .expect_err("pre-effect refusal");
        assert!(!error.attempted, "{}", error.reason);
        assert_eq!(history::native_files(&root), before);
    }
}

#[test]
fn altered_head_index_and_configuration_are_never_repaired() {
    for corruption in ["head", "index", "config"] {
        let temporary = tempfile::tempdir().expect("disposable corrupt repository");
        let root = temporary.path().join("deployment");
        let input = source();
        stage(&root, &input);
        let report = Git
            .prepare_worktree(&root, input.as_input(), &recipe(), None, options())
            .expect("real independent metadata");
        match corruption {
            "head" => fs::write(root.join(".git/HEAD"), b"ref: refs/heads/missing\n")
                .expect("change native HEAD"),
            "index" => {
                let repository = Repository::open(&root).expect("native fixture repository");
                let mut index = repository.index().expect("native index");
                index
                    .remove_path(Path::new("payload.txt"))
                    .expect("remove index member");
                index.write().expect("persist malformed selection");
            }
            _ => fs::write(
                root.join(".git/config"),
                b"[include]\npath=/missing-unread\n",
            )
            .expect("change owned config"),
        }
        let before = history::native_files(&root);
        let error = Git
            .synchronize_worktree(
                &root,
                &report.commit.commit,
                report.commit.output,
                &recipe(),
                sync_options(),
            )
            .expect_err("native corruption before flush");
        assert!(!error.attempted);
        assert_eq!(history::native_files(&root), before);
    }
}
