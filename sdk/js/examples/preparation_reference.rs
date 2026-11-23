//! Test-only native initialization, preparation and independent journal/Git read-back.

use std::{
    io::{Read, Write},
    path::Path,
};

use gazette_sdk::{build, release_commit, source, store};
use serde_json::{Value, json};

fn read() -> store::ReadOptions {
    store::ReadOptions {
        allow_coordination_files: true,
        ..Default::default()
    }
}

fn options() -> build::LocalPreparationRunOptions {
    build::LocalPreparationRunOptions {
        allow_read: true,
        preparation: build::LocalPreparationOptions {
            stage: build::PreparationOptions {
                allow_stage: true,
                record: store::PreparationWriteOptions { allow_record: true },
                writer: store::WriterOptions {
                    allow_generation_advance: true,
                    session: store::SessionOptions {
                        allow_local_reservation: true,
                        read: read(),
                    },
                },
                ..Default::default()
            },
            worktree: release_commit::WorktreeOptions {
                allow_initialize: true,
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    }
}

fn main() {
    let mut bytes = Vec::new();
    std::io::stdin()
        .lock()
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .expect("synthetic request");
    assert!(bytes.len() <= 16 * 1024 * 1024, "bounded fixture request");
    let request: Value = serde_json::from_slice(&bytes).expect("fixture JSON");
    let result = match request["operation"]
        .as_str()
        .expect("explicit fixture operation")
    {
        "initialize" => serde_json::to_value(
            store::initialize(
                Path::new(request["journal"].as_str().expect("disposable journal")),
                store::InitializeOptions {
                    allow_create: true,
                    session: store::SessionOptions {
                        allow_local_reservation: true,
                        read: read(),
                    },
                    ..Default::default()
                },
            )
            .expect("fresh native journal"),
        )
        .expect("receipt"),
        "prepare" => {
            let bytes = serde_json::to_vec(&request["recipe"]).expect("exact recipe");
            let recipe = build::LocalPreparationRecipe::parse_json(&bytes, 16 * 1024 * 1024, 2)
                .expect("native recipe");
            let result = recipe.execute(options());
            serde_json::to_value(build::LocalPreparationRunReport::new(&result))
                .expect("native report")
        }
        "inspect" => {
            let database = Path::new(request["database"].as_str().expect("exact database"));
            let observed = store::inspect_journal(database, None, read()).expect("native journal");
            let protected =
                [Path::new(request["source"].as_str().expect("source root")).to_owned()];
            let records = request["ids"].as_array().expect("bounded IDs");
            assert!(records.len() <= 2, "two-site fixture");
            let records = records
                .iter()
                .map(|id| {
                    let id: source::Sha256Digest = serde_json::from_value(id.clone()).expect("ID");
                    let inspected =
                        store::inspect_preparation(database, id, Some(&observed.header), read())
                            .expect("independent preparation inspection");
                    if let Some(completion) = inspected
                        .record
                        .as_ref()
                        .and_then(|record| record.worktree.as_ref())
                        .and_then(|worktree| worktree.completion.as_ref())
                    {
                        release_commit::inspect_journal_worktree(
                            release_commit::JournalWorktreeInspectionRequest {
                                database,
                                preparation_id: id,
                                expected_header: Some(&observed.header),
                                commit: &completion.report.commit,
                                protected_roots: &protected,
                            },
                            release_commit::JournalWorktreeInspectionOptions {
                                read: read(),
                                ..Default::default()
                            },
                        )
                        .expect("native journal, retained output and independent Git read-back");
                    }
                    serde_json::to_value(inspected.record).expect("native optional record")
                })
                .collect::<Vec<_>>();
            json!({"header":observed.header,"records":records})
        }
        _ => panic!("unsupported fixture operation"),
    };
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(&mut stdout, &result).expect("complete fixture response");
    stdout.write_all(b"\n").expect("framing");
}
