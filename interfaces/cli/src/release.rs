//! Private bounded transport over the shared pure release-signature verifier.

use std::{
    io::{self, Read, Write},
    process::ExitCode,
};

use gazette_sdk::release;
use serde::Deserialize;
use serde_json::value::RawValue;

use crate::{
    command::{self, Kernel},
    json_input::Object,
};

#[derive(clap::Args)]
pub(super) struct Args {
    /// Maximum complete stdin envelope bytes, checked before JSON decoding.
    #[arg(long, default_value_t = release::Limits::default().bytes)]
    max_bytes: usize,
    /// Maximum decoded nodes in each authorization or binding input.
    #[arg(long, default_value_t = release::Limits::default().nodes)]
    max_nodes: usize,
    /// Maximum nesting depth in each authorization or binding input.
    #[arg(long, default_value_t = release::Limits::default().depth)]
    max_depth: usize,
    /// Maximum targets checked by the shared verifier.
    #[arg(long, default_value_t = release::Limits::default().targets)]
    max_targets: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request<'a> {
    #[serde(borrow)]
    authorization: &'a RawValue,
    #[serde(borrow)]
    binding: &'a RawValue,
}

pub(super) fn run(args: Args) -> ExitCode {
    let mut bytes = Vec::new();
    if io::stdin()
        .lock()
        .take((args.max_bytes as u64).saturating_add(1))
        .read_to_end(&mut bytes)
        .is_err()
    {
        return failure("release_input");
    }
    if bytes.len() > args.max_bytes {
        return failure("release_input_limit");
    }
    let Object(request): Object<Request<'_>> = match serde_json::from_slice(&bytes) {
        Ok(request) => request,
        Err(_) => return failure("release_request"),
    };
    let limits = release::Limits {
        bytes: args.max_bytes,
        nodes: args.max_nodes,
        depth: args.max_depth,
        targets: args.max_targets,
    };
    match Kernel::new().execute(command::release::VerifyAuthorization {
        authorization_json: request.authorization.get().as_bytes().to_vec(),
        binding_json: request.binding.get().as_bytes().to_vec(),
        limits,
    }) {
        Ok(report) => super::write_json(&report),
        Err(error) => failure(error.code()),
    }
}

fn failure(code: &'static str) -> ExitCode {
    // Parser/verification messages can contain private input. Only stable codes
    // cross this boundary; verification never performs or authorizes a release.
    let _ = writeln!(
        io::stderr().lock(),
        "gazette: {code}: no verification report produced; no release effects"
    );
    ExitCode::FAILURE
}
