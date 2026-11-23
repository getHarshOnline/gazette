//! Private registry reconciliation and inspection, never public admission or release.

use std::{
    io::{self, Read, Write},
    path::PathBuf,
    process::ExitCode,
};

use gazette_sdk::{build, source, store, toml, workspace};

#[derive(clap::Args)]
pub(super) struct ReconcileArgs {
    /// Existing initialized private store directory, separate from every captured source.
    root: PathBuf,
    /// Publication acquisition consumer; registry replacement still covers the whole workspace.
    #[arg(long)]
    site: workspace::Key,
    /// Complete independently retained journal coordinates; never silently rebound.
    #[arg(long, value_parser = super::journal::parse_header)]
    expected_header: store::JournalHeader,
    /// Replace all registry-owned rows, including removals; no release history is erased.
    #[arg(long)]
    replace_registry: bool,
    /// Permit a separate durable generation increment before registry mutation.
    #[arg(long)]
    advance_writer_generation: bool,
    /// Hold exclusive local directory coordination through mutation and read-back.
    #[arg(long)]
    reserve_local_coordination: bool,
    /// Maximum complete workspace TOML bytes before native source acquisition.
    #[arg(long, default_value_t = toml::Limits::default().source_bytes)]
    max_source_bytes: usize,
    /// Maximum compact complete registry projection bytes, before writer acquisition.
    #[arg(long, default_value_t = build::RegistryLimits::default().bytes)]
    max_registry_bytes: usize,
    /// Maximum complete compact response including newline; reserved before writer acquisition.
    #[arg(long, default_value_t = 256 * 1024)]
    max_output_bytes: usize,
    #[command(flatten)]
    read: super::store::ReadArgs,
}

#[derive(clap::Args)]
pub(super) struct InspectArgs {
    /// Existing private store database; no initialization, migration or reconciliation.
    path: PathBuf,
    /// Complete independently retained journal header; omitted means observation only.
    #[arg(long, value_parser = super::journal::parse_header)]
    expected_header: Option<store::JournalHeader>,
    #[command(flatten)]
    read: super::store::ReadArgs,
}

pub(super) fn inspect(args: InspectArgs) -> ExitCode {
    match crate::command::Kernel::new().execute(crate::command::store::InspectRegistry {
        path: args.path,
        expected: args.expected_header,
        options: args.read.options(),
    }) {
        Ok(report) => super::write_json(&report),
        Err(error) => failure(error.code(), &error),
    }
}

pub(super) fn reconcile(args: ReconcileArgs) -> ExitCode {
    let options = build::RegistryReconcileOptions {
        projection: build::RegistryLimits {
            bytes: args.max_registry_bytes,
        },
        registry: store::RegistryWriteOptions {
            allow_replace: args.replace_registry,
        },
        writer: store::WriterOptions {
            allow_generation_advance: args.advance_writer_generation,
            session: store::SessionOptions {
                allow_local_reservation: args.reserve_local_coordination,
                read: args.read.options(),
            },
        },
    };
    if let Err(error) = options
        .registry
        .validate()
        .and_then(|_| options.writer.validate())
    {
        return failure(error.code(), &error);
    }
    if args.max_output_bytes > 16 << 20 {
        return failure(
            "registry_response_limit",
            &"response ceiling exceeds 16 MiB",
        );
    }
    let reservation =
        match build::registry_output::Reservation::new(args.max_output_bytes.saturating_sub(1)) {
            Ok(reservation) => reservation,
            Err(code) => return failure(code, &"response capacity refused before acquisition"),
        };
    let mut bytes = Vec::new();
    if let Err(error) = io::stdin()
        .lock()
        .take((args.max_source_bytes as u64).saturating_add(1))
        .read_to_end(&mut bytes)
    {
        return failure("workspace_input", &error);
    }
    if bytes.len() > args.max_source_bytes {
        return failure(
            "toml_limit",
            &"complete workspace input exceeds the byte ceiling; no writer acquired",
        );
    }
    let text = match std::str::from_utf8(&bytes) {
        Ok(text) => text,
        Err(error) => return failure("workspace_utf8", &error),
    };
    let result =
        crate::command::Kernel::new().execute(crate::command::coordinator::ReconcileRegistry {
            source: text.into(),
            site: args.site,
            allow_native_reads: true,
            syntax: toml::Limits {
                source_bytes: args.max_source_bytes,
                ..Default::default()
            },
            acquisition: source::PreparationLimits {
                sources: 256,
                entries: source::Limits::default().entries,
                bytes: source::Limits::default().total_bytes,
                per_source: source::RevisionLimits::default(),
            },
            corpus: Default::default(),
            graph: Default::default(),
            root: args.root,
            expected: args.expected_header,
            options,
        });
    let result = match result {
        Ok(receipt) => Ok(receipt),
        Err(error) => match error.into_effect() {
            Ok(error) => Err(error),
            Err(error) => return failure(error.code(), &error),
        },
    };
    let mut encoded = match reservation.finish(&result) {
        Ok(encoded) => encoded,
        Err(error) => {
            let _ = serde_json::to_writer(io::stderr().lock(), &error.report);
            return failure(
                "registry_response_contract",
                &"original outcome retained above; delivery is not bounded; do not retry automatically",
            );
        }
    };
    encoded.bytes.push(b'\n');
    if crate::deliver_json(&encoded.bytes).is_err() {
        let _ = io::stderr().lock().write_all(&encoded.bytes);
        return failure(
            "registry_response_io",
            &"original outcome copied above if stderr is available; no rollback or retry implied",
        );
    }
    if let Err(error) = &result {
        return failure(error.code(), error);
    }
    ExitCode::SUCCESS
}

fn failure(code: &str, error: &impl std::fmt::Display) -> ExitCode {
    let _ = writeln!(
        io::stderr().lock(),
        "gazette: {code}: {error}; no successful registry report returned"
    );
    ExitCode::FAILURE
}
