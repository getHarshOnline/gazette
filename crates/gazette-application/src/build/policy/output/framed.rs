//! Inventory-ordered raw frames over retained bytes; no export or native path trust.

use gazette_core::{
    process,
    source::{CapturedSource, Content, Sha256Digest},
};
use serde::Serialize;

use super::{
    MatchedOutputResponse, OutputPolicyRequirement, OutputResponseError, PolicyCheckError,
    PolicyInvocation, command_digest, encode, match_output_response,
};

const MAGIC: &[u8] = b"gazette-output-policy/2\n";

pub(super) fn invoke(
    metadata: &impl Serialize,
    output: &CapturedSource,
    runner: &impl gazette_ports::process::CaptureRunner,
    command: &process::Command,
    options: process::Options,
    requirement: Option<&OutputPolicyRequirement>,
) -> Result<(PolicyInvocation, Option<MatchedOutputResponse>), PolicyCheckError> {
    let limits = gazette_core::source::Limits::default();
    let inventory = output.inventory();
    if inventory.entries().len() > limits.entries || inventory.total_bytes() > limits.total_bytes {
        return Err(limit("policy framed output entries or bytes"));
    }
    let metadata = encode(
        metadata,
        options.max_input_bytes,
        "policy framed metadata bytes",
    )
    .map_err(PolicyCheckError::Encoding)?;
    let metadata_length = (metadata.len() as u64).to_be_bytes();
    let mut lengths = Vec::new();
    let mut payloads = Vec::new();
    for entry in inventory.entries() {
        if let Content::File { bytes, sha256 } = entry.content {
            if bytes > limits.file_bytes {
                return Err(limit("policy framed file bytes"));
            }
            let payload = output
                .file(&entry.path)
                .ok_or_else(|| limit("policy framed member"))?;
            if payload.len() as u64 != bytes || Sha256Digest::of(payload) != sha256 {
                return Err(limit("policy framed member identity"));
            }
            lengths.push(bytes.to_be_bytes());
            payloads.push(payload);
        }
    }
    let mut segments: Vec<&[u8]> = Vec::with_capacity(3 + payloads.len() * 2);
    segments.extend([MAGIC, metadata_length.as_slice(), metadata.as_slice()]);
    for (length, payload) in lengths.iter().zip(payloads) {
        segments.push(length.as_slice());
        segments.push(payload);
    }
    let bytes = segments.iter().try_fold(0usize, |total, segment| {
        total
            .checked_add(segment.len())
            .ok_or_else(|| limit("policy framed stdin bytes"))
    })?;
    let request = process::SegmentedCaptureRequest {
        command,
        segments: &segments,
        allow_execute: options.allow_execute,
        max_input_bytes: bytes,
        timeout: options.timeout,
        max_output_bytes: requirement.map(|r| r.response_bytes),
        max_stderr_bytes: None,
    };
    request.validate().map_err(PolicyCheckError::Refused)?;
    let invocation = PolicyInvocation {
        request_sha256: Sha256Digest::of_slices(segments.iter().copied()),
        command_sha256: command_digest(command)?,
    };
    let captured = runner
        .capture_segmented(request)
        .map_err(|source| PolicyCheckError::Execution { invocation, source })?;
    let Some(required) = requirement else {
        if captured.exit.code != Some(0) {
            return Err(PolicyCheckError::Rejected {
                invocation,
                exit: captured.exit,
            });
        }
        return Ok((invocation, None));
    };
    let matched = if captured.exit.code == Some(0) {
        match_output_response(invocation.request_sha256, &captured.stdout, required)
    } else {
        Err(OutputResponseError::Exit)
    };
    let response = matched.map_err(|source| PolicyCheckError::OutputResponse {
        invocation,
        captured,
        source,
    })?;
    Ok((invocation, Some(response)))
}

fn limit(resource: &'static str) -> PolicyCheckError {
    PolicyCheckError::Encoding(gazette_core::render::Error::Limit(resource))
}
