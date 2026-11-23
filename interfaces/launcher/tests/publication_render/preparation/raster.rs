//! Native PNG bytes remain identical through rendering, metadata checks, `SQLite` and Git.

use gazette_sdk::{raster, release_commit as release};

use super::super::{
    probe::raster::{CLEAN, decode},
    worktree::metadata,
};
use super::{
    Value, acquire, build, bytes, check::disclosure_policy, fs, initialize, options,
    private_directory, published_fixture, read, render, request,
};

#[test]
fn png_publication_reaches_durable_checked_local_preparation_without_reencoding() {
    let png = decode(CLEAN);
    let inspected_image = raster::metadata(raster::Format::Png, &png, Default::default())
        .expect("actual native PNG metadata profile");
    let fixture = published_fixture(&[("one/image.png", &png)]);
    let config = fixture.config().replace(
        "authored=[{source='source:one',target='one'}]",
        "authored=[{source='source:one',target='one',static_assets={route_prefix='/media/authored/'}}]",
    );
    let inputs = acquire(&config);
    fs::write(
        fixture.directory.path().join("one/image.png"),
        b"not reacquired",
    )
    .expect("later native image mutation");
    let rendered = render(&inputs).expect("real retained publication including selected PNG asset");
    let output = rendered.site().output();
    let image_path = "media/authored/image.png";
    assert_eq!(bytes(output, image_path), png);
    let original = output.inventory().identity();
    let journal_root = private_directory();
    let journal = initialize(journal_root.path());
    let parent = private_directory();
    let recipe = metadata();
    let policy = disclosure_policy();
    let completed = build::prepare_local(
        &rendered,
        build::LocalPreparationRequest {
            output_policy: None,
            qualification: None,
            coordinator: None,
            stage: request(journal_root.path(), journal.header, parent.path(), "png"),
            metadata: &recipe,
            parent_source: None,
            disclosure: &policy,
        },
        build::LocalPreparationOptions {
            stage: options(),
            worktree: release::WorktreeOptions {
                allow_initialize: true,
                ..Default::default()
            },
            ..Default::default()
        },
    )
    .expect("PNG-bearing local publication passes complete metadata checks and native custody");
    assert_eq!(completed.record.intent.output, original);
    assert!(completed.record.retention.is_some());
    let check = completed
        .record
        .check
        .as_ref()
        .expect("persisted exact checks");
    let evidence: Value =
        serde_json::from_str(&check.evidence).expect("independently encoded durable evidence");
    assert_eq!(evidence["policy"]["status"], "not-configured");
    assert_eq!(
        evidence["disclosure"]["status"],
        "required-supported-profile-evidence"
    );
    assert!(
        evidence["disclosure"]["files"]
            .as_array()
            .expect("complete file coverage")
            .iter()
            .any(|file| file["path"] == image_path
                && file["sha256"] == inspected_image.sha256.to_string()
                && file["decoding"]["raster-metadata"]["format"] == "png"
                && file["decoding"]["raster-metadata"]["profile"] == inspected_image.tool)
    );
    let worktree = completed
        .record
        .worktree
        .as_ref()
        .expect("durable Git recipe");
    assert_eq!(worktree.check_sha256, Some(check.evidence_sha256));
    let commit = &worktree
        .completion
        .as_ref()
        .expect("complete native Git custody")
        .report
        .commit;
    let root = completed.record.intent.address.path();
    for path in [image_path, "guide/index.html", "model.json"] {
        assert_eq!(
            fs::read(root.join(path)).expect("real staged bytes"),
            bytes(output, path)
        );
    }
    let protected = [fixture.directory.path().to_owned()];
    let observed = release::inspect_journal_worktree(
        release::JournalWorktreeInspectionRequest {
            database: &journal.database,
            preparation_id: completed.record.intent.id,
            expected_header: Some(&completed.journal),
            commit,
            protected_roots: &protected,
        },
        release::JournalWorktreeInspectionOptions {
            read: read(),
            ..Default::default()
        },
    )
    .expect("independent SQLite history plus exact Git and filesystem read-back");
    assert_eq!(
        observed.history.record.expect("persisted preparation"),
        completed.record
    );
    assert_eq!(output.inventory().identity(), original);
    assert_eq!(
        fs::read(fixture.directory.path().join("one/image.png"))
            .expect("native source remains changed"),
        b"not reacquired"
    );
}
