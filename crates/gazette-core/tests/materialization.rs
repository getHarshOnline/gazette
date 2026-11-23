//! Configured-output history retains exact native facts without granting effects.

use gazette_core::{
    destination::{OutputAdministrationBinding, OutputBinding, OutputIdentity, OutputObservation},
    source::{Content, Inventory, InventoryBuilder, Limits, Sha256Digest},
    store::{
        Error, JournalHeader, MaterializationCompletion, MaterializationCompletionInput,
        MaterializationEffect, MaterializationIntent, MaterializationWriteError,
        MaterializationWriteOptions,
    },
};

fn inventory(files: &[(&str, &[u8])]) -> Inventory {
    let mut inventory = InventoryBuilder::new(Limits::default());
    for (name, bytes) in files {
        inventory
            .insert(
                name.parse().expect("valid confined entry"),
                Content::File {
                    bytes: bytes.len() as u64,
                    sha256: Sha256Digest::of(bytes),
                },
            )
            .expect("complete structural observation");
    }
    inventory.finish()
}

fn binding(inode: u64, administration: Option<OutputAdministrationBinding>) -> OutputBinding {
    OutputBinding::new(
        std::env::temp_dir().join("gazette-materialization-contract-not-created"),
        OutputIdentity::new(7, inode),
        administration,
    )
    .expect("absolute native facts, no filesystem effect")
}

fn intent() -> MaterializationIntent {
    let evidence = r#"{"profile":"local","source":"retained"}"#.to_owned();
    MaterializationIntent {
        id: Sha256Digest::of(b"exact-operation"),
        site: "site:example".parse().expect("typed site"),
        binding: binding(11, None),
        previous: Some(Sha256Digest::of(b"preceding-completed-head")),
        observed: inventory(&[("user.txt", b"user")]),
        expected: inventory(&[("index.html", b"page"), ("user.txt", b"user")]),
        generated: inventory(&[("index.html", b"page")]),
        protected: vec![std::env::temp_dir().join("selected-source-not-created")],
        evidence_sha256: Sha256Digest::of(evidence.as_bytes()),
        evidence,
        created: JournalHeader {
            schema_version: 1,
            journal_id: [9; 32],
            journal_epoch: 1,
            writer_fence: 3,
        },
    }
}

#[test]
fn original_payload_keeps_ownership_subset_and_before_after_distinct() {
    let intent = intent();
    let input = intent.as_input();
    assert_eq!(input.id, intent.id);
    assert_eq!(input.site, &intent.site);
    assert_eq!(input.binding, &intent.binding);
    assert_eq!(input.previous, intent.previous);
    assert_eq!(input.observed, &intent.observed);
    assert_eq!(input.expected, &intent.expected);
    assert_eq!(input.generated, &intent.generated);
    assert_eq!(input.protected, intent.protected);
    assert_eq!(input.evidence, intent.evidence);
    assert_eq!(input.evidence_sha256, intent.evidence_sha256);
    assert_ne!(input.observed.identity(), input.expected.identity());
    assert_ne!(input.generated.identity(), input.expected.identity());
    let json = serde_json::to_value(&intent).expect("private historical report");
    assert_eq!(json["binding"]["root"]["device"], 7);
    assert_eq!(json["binding"]["root"]["inode"], 11);
    assert_eq!(
        json["generated"]["entries"]
            .as_array()
            .expect("entries")
            .len(),
        1
    );
    assert_eq!(
        json["expected"]["entries"]
            .as_array()
            .expect("entries")
            .len(),
        2
    );
    assert_eq!(json["created"]["writer_fence"], 3);
}

#[test]
fn completion_requires_complete_expected_tree_and_original_native_binding() {
    let intent = intent();
    let read_back = OutputObservation::new(
        inventory(&[("index.html", b"page"), ("user.txt", b"user")]),
        intent.binding.clone(),
    );
    MaterializationCompletionInput {
        intent: &intent,
        read_back: &read_back,
    }
    .validate()
    .expect("complete expected bytes and native facts");
    for read_back in [
        OutputObservation::new(
            inventory(&[("index.html", b"page")]),
            intent.binding.clone(),
        ),
        OutputObservation::new(
            inventory(&[("index.html", b"page"), ("user.txt", b"USER")]),
            intent.binding.clone(),
        ),
        OutputObservation::new(
            inventory(&[
                ("index.html", b"page"),
                ("user.txt", b"user"),
                ("extra.txt", b"extra"),
            ]),
            intent.binding.clone(),
        ),
        OutputObservation::new(
            inventory(&[("index.html", b"page"), ("user.txt", b"user")]),
            binding(12, None),
        ),
        OutputObservation::new(
            inventory(&[("index.html", b"page"), ("user.txt", b"user")]),
            binding(
                11,
                Some(OutputAdministrationBinding::Directory(OutputIdentity::new(
                    7, 20,
                ))),
            ),
        ),
        OutputObservation::new(
            inventory(&[("index.html", b"page"), ("user.txt", b"user")]),
            binding(
                11,
                Some(OutputAdministrationBinding::File(OutputIdentity::new(
                    7, 20,
                ))),
            ),
        ),
    ] {
        assert!(matches!(
            MaterializationCompletionInput {
                intent: &intent,
                read_back: &read_back
            }
            .validate(),
            Err(Error::MaterializationVerification)
        ));
    }
}

#[test]
fn history_permission_is_explicit_and_failures_do_not_claim_filesystem_rollback() {
    assert!(matches!(
        MaterializationWriteOptions::default().validate(),
        Err(Error::MaterializationPermission)
    ));
    MaterializationWriteOptions { allow_record: true }
        .validate()
        .expect("record permission only");
    let error = MaterializationWriteError::from(Error::MaterializationPending);
    assert_eq!(error.effect, MaterializationEffect::NotAttempted);
    assert_eq!(error.cause.code(), "store_materialization_pending");
    let diagnostic = error.to_string();
    assert!(diagnostic.contains("never blindly retry"));
    assert!(diagnostic.contains("preserve destination and journal"));
}

#[test]
fn historical_completion_keeps_exact_output_and_original_journal_lineage() {
    let intent = intent();
    let completion = MaterializationCompletion {
        intent_id: intent.id,
        output: intent.expected.identity(),
        entries: intent.expected.entries().len() as u64,
        bytes: intent.expected.total_bytes(),
        recorded: intent.created,
    };
    completion
        .validate(&intent)
        .expect("same-generation completion representation");
    let mut later = completion.clone();
    later.recorded.writer_fence += 1;
    later
        .validate(&intent)
        .expect("later record in the same original journal epoch");
    for change in 0..8 {
        let mut changed = completion.clone();
        match change {
            0 => changed.intent_id = Sha256Digest::of(b"different intent"),
            1 => changed.output = intent.generated.identity(),
            2 => changed.entries += 1,
            3 => changed.bytes += 1,
            4 => changed.recorded.schema_version += 1,
            5 => changed.recorded.journal_id[0] ^= 1,
            6 => changed.recorded.journal_epoch += 1,
            7 => changed.recorded.writer_fence -= 1,
            _ => unreachable!("bounded qualification variants"),
        }
        assert!(matches!(
            changed.validate(&intent),
            Err(Error::MaterializationVerification)
        ));
    }
}
