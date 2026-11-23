//! Real pending custody after signature/native joins; no consumption or dispatch.

use super::{
    Prepared, STANDARD, batch_options, declaration, fs, options, read, release, sign, store, verify,
};
use base64::Engine as _;

fn acquire(prepared: &mut Prepared) -> store::Writer {
    let original = prepared.header;
    let mut writer =
        store::acquire_writer(prepared._journal_root.path(), original, options().writer)
            .expect("hold real native generation before issuer constructs its binding");
    prepared.header = writer.inspect().expect("held generation read-back").header;
    assert_ne!(prepared.header, original);
    writer
}

fn registration_options() -> release::RegisterAuthorizationOptions {
    release::RegisterAuthorizationOptions {
        verification: release::PreparedAuthorizationOptions {
            preparations: batch_options(),
            ..Default::default()
        },
        record: store::ReleaseAuthorizationWriteOptions { allow_record: true },
    }
}

fn request<'a>(
    prepared: &'a Prepared,
    authorization: &'a [u8],
    binding: &'a release::Binding,
) -> release::PreparedAuthorizationRequest<'a> {
    release::PreparedAuthorizationRequest {
        authorization,
        binding,
        preparations: prepared.request(&prepared.selections),
    }
}

fn row_counts(prepared: &Prepared) -> (i64, i64) {
    let connection = rusqlite::Connection::open_with_flags(
        &prepared.database,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .expect("independent read-only custody inspection");
    let counts = connection
        .query_row(
            "SELECT (SELECT count(*) FROM gazette_release_authorizations), \
             (SELECT count(*) FROM gazette_release_authorization_preparations)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("exact header and preparation-link cardinality");
    connection
        .close()
        .expect("close read-only fixture inspection");
    counts
}

#[test]
fn exact_registration_replays_and_reopens_without_reissuing_authority() {
    let mut prepared = Prepared::new();
    let original_records = prepared.records.clone();
    let mut writer = acquire(&mut prepared);
    let (body, binding) = declaration(&prepared.inspect());
    let record_id = body.record_id.clone();
    let expires_at = body.expires_at;
    let (authorization, binding) = sign(body, binding);
    let verified = verify(&prepared, &prepared.selections, &authorization, &binding)
        .expect("real signature and complete two-target native preparation verification");
    let receipt = writer
        .register_release_authorization(
            request(&prepared, &authorization, &binding),
            registration_options(),
        )
        .expect("verified pending registration under already-held generation");
    assert!(receipt.inserted);
    assert_eq!(receipt.journal, prepared.header);
    assert_eq!(receipt.record.created, prepared.header);
    assert_eq!(receipt.record.authorization.as_bytes(), authorization);
    assert_eq!(receipt.record.binding.as_signed(), Some(&binding));
    assert_eq!(
        receipt.record.preparations_sha256,
        verified.preparations_sha256
    );
    assert_eq!(
        receipt.record.preparation_ids,
        prepared
            .selections
            .iter()
            .map(|item| item.preparation_id)
            .collect::<Vec<_>>()
    );
    let snapshot = prepared.snapshot();
    let replay = writer
        .register_release_authorization(
            request(&prepared, &authorization, &binding),
            registration_options(),
        )
        .expect("same exact bytes and bindings replay in the same held generation");
    assert!(!replay.inserted);
    assert_eq!(replay.record, receipt.record);
    assert_eq!(replay.journal, receipt.journal);
    prepared.unchanged(&snapshot);
    writer.close().expect("close successful held lifetime");
    let historical = store::inspect_release_authorization(
        &prepared.database,
        &record_id,
        Some(&prepared.header),
        read(),
    )
    .expect("reopen exact pending custody without a clock or signing operation");
    assert_eq!(historical.journal.header, prepared.header);
    assert_eq!(historical.record.as_ref(), Some(&receipt.record));
    assert_eq!(row_counts(&prepared), (1, 2));
    for record in original_records {
        assert_eq!(
            store::inspect_preparation(
                &prepared.database,
                record.intent.id,
                Some(&prepared.header),
                read()
            )
            .expect("original preparation retained under current journal inspection")
            .record,
            Some(record),
            "registration must not rebind preparation creation/check/worktree coordinates"
        );
    }

    let mut expired = binding.clone();
    expired.observed_at = expires_at;
    assert!(matches!(
        release::verify(&authorization, &expired, Default::default()),
        Err(release::Error::Time)
    ));
    let after_expiry = store::inspect_release_authorization(
        &prepared.database,
        &record_id,
        Some(&prepared.header),
        read(),
    )
    .expect("historical custody remains inspectable, not newly authorized");
    assert_eq!(after_expiry.record.as_ref(), Some(&receipt.record));
    assert_eq!(
        after_expiry
            .record
            .expect("original authorization remains in historical custody")
            .binding
            .observed_at(),
        &binding.observed_at
    );

    let mut next_writer = acquire(&mut prepared);
    let error = next_writer
        .register_release_authorization(
            request(&prepared, &authorization, &binding),
            registration_options(),
        )
        .expect_err("reacquisition must not rewrite or renew the received signed fence");
    assert!(
        matches!(error, release::RegisterAuthorizationError::Store(ref failure)
        if matches!(failure.cause, store::Error::JournalMismatch)
        && failure.effect == store::ReleaseAuthorizationEffect::NotAttempted)
    );
    next_writer
        .close()
        .expect("pre-storage mismatch does not poison writer");
    let retained = store::inspect_release_authorization(
        &prepared.database,
        &record_id,
        Some(&prepared.header),
        read(),
    )
    .expect("historical record retains original coordinates after writer advance");
    assert_eq!(retained.record, Some(receipt.record));
    assert_eq!(row_counts(&prepared), (1, 2));
}

#[test]
fn canonical_equivalent_transport_and_nonce_collision_do_not_replace_pending_custody() {
    for changed_record in [false, true] {
        let mut prepared = Prepared::new();
        let mut writer = acquire(&mut prepared);
        let (body, binding) = declaration(&prepared.inspect());
        let original_id = body.record_id.clone();
        let (authorization, binding) = sign(body.clone(), binding);
        let receipt = writer
            .register_release_authorization(
                request(&prepared, &authorization, &binding),
                registration_options(),
            )
            .expect("initial pending record");
        let (changed, changed_binding, changed_id) = if changed_record {
            let mut other = body;
            other.record_id.push_str("-other");
            let other_id = other.record_id.clone();
            let (bytes, other_binding) = sign(other, binding.clone());
            (bytes, other_binding, other_id)
        } else {
            let mut bytes = b" \n".to_vec();
            bytes.extend_from_slice(&authorization);
            bytes.push(b'\n');
            (bytes, binding.clone(), original_id.clone())
        };
        verify(&prepared, &prepared.selections, &changed, &changed_binding)
            .expect("conflict is not an invalid signature or native preparation");
        if !changed_record {
            assert_eq!(
                changed_binding.canonical_body_sha256,
                binding.canonical_body_sha256
            );
        }
        let error = writer
            .register_release_authorization(
                request(&prepared, &changed, &changed_binding),
                registration_options(),
            )
            .expect_err("same canonical meaning does not replace exact bytes; nonce is unique");
        assert!(
            matches!(error, release::RegisterAuthorizationError::Store(ref failure)
            if matches!(failure.cause, store::Error::ReleaseAuthorizationConflict))
        );
        // Native transaction refusal invalidates the held lifetime. Its original
        // error/effect is evidence; independent read-back, not close, proves rows.
        let _closed = writer.close();
        let historical = store::inspect_release_authorization(
            &prepared.database,
            &original_id,
            Some(&prepared.header),
            read(),
        )
        .expect("inspect original custody after conflicting attempt");
        assert_eq!(historical.record, Some(receipt.record));
        assert_eq!(row_counts(&prepared), (1, 2));
        if changed_record {
            assert!(
                store::inspect_release_authorization(
                    &prepared.database,
                    &changed_id,
                    Some(&prepared.header),
                    read(),
                )
                .expect("inspect refused second identity")
                .record
                .is_none()
            );
        }
    }
}

#[test]
fn permission_signature_fence_and_native_output_fail_before_pending_rows() {
    for case in ["permission", "signature", "fence", "output"] {
        let mut prepared = Prepared::new();
        let before_held = prepared.header;
        let mut writer = acquire(&mut prepared);
        let (mut body, mut binding) = declaration(&prepared.inspect());
        if case == "fence" {
            body.writer_fence = before_held.writer_fence;
            binding.journal = before_held;
        }
        let record_id = body.record_id.clone();
        let (mut authorization, binding) = sign(body, binding);
        let mut settings = registration_options();
        match case {
            "permission" => settings.record.allow_record = false,
            "signature" => {
                let mut body: release::Authorization = serde_json::from_slice(&authorization)
                    .expect("decode actual signed fixture before signature corruption");
                body.publisher_authorizations[0]
                    .signature_envelope
                    .signature = STANDARD.encode([0_u8; 64]);
                authorization =
                    serde_json::to_vec(&body).expect("encode controlled invalid-signature fixture");
            }
            "output" => fs::write(
                prepared.records[0].intent.address.path().join("model.json"),
                b"changed output",
            )
            .expect("actual disposable output drift after signing"),
            "fence" => {}
            _ => unreachable!(),
        }
        let snapshot = prepared.snapshot();
        let error = writer
            .register_release_authorization(request(&prepared, &authorization, &binding), settings)
            .expect_err("failed prerequisite cannot create pending custody");
        match case {
            "permission" | "fence" => assert!(
                matches!(
                    error, release::RegisterAuthorizationError::Store(ref failure)
                        if failure.effect == store::ReleaseAuthorizationEffect::NotAttempted
                        && match case {
                            "permission" => matches!(failure.cause, store::Error::ReleaseAuthorizationPermission),
                            _ => matches!(failure.cause, store::Error::JournalMismatch),
                        }
                ),
                "{case}: {error}"
            ),
            "signature" => assert!(
                matches!(
                    error,
                    release::RegisterAuthorizationError::Verification(
                        release::PreparedAuthorizationError::Authorization(_)
                    )
                ),
                "{error}"
            ),
            "output" => assert!(
                matches!(
                    error,
                    release::RegisterAuthorizationError::Verification(
                        release::PreparedAuthorizationError::Preparation(_)
                    )
                ),
                "{error}"
            ),
            _ => unreachable!(),
        }
        prepared.unchanged(&snapshot);
        assert_eq!(
            writer
                .inspect()
                .expect("pre-write refusal leaves writer usable")
                .header,
            prepared.header
        );
        writer.close().expect("close untouched native writer");
        assert!(
            store::inspect_release_authorization(
                &prepared.database,
                &record_id,
                Some(&prepared.header),
                read(),
            )
            .expect("independent absence proof")
            .record
            .is_none()
        );
        assert_eq!(row_counts(&prepared), (0, 0), "{case}");
    }
}
