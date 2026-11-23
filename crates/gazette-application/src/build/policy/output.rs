//! Complete retained output transport; no filesystem export or policy-rule interpretation.

use std::collections::BTreeMap;

mod framed;

use base64::{display::Base64Display, engine::general_purpose::STANDARD};
use gazette_compiler::authored;
use gazette_core::{
    process,
    publication::{Key, routing::PublicBaseUrl},
    render::MediaType,
    source::{CapturedSource, Content, EntryPath, Inventory, Sha256Digest, TreeIdentity},
};
use serde::{Serialize, Serializer, ser::SerializeMap};

use super::{
    MatchedOutputResponse, OutputPolicyRequirement, OutputResponseError, PolicyCheckError,
    PolicyDocument, PolicyInvocation, command, command_digest, encode, invoke,
    match_output_response, policy_input,
};
use crate::build::{CompositionInspection, DisclosureProbe, PublicationRender, SiteRenderEvidence};

/// A configured zero exit bound to one exact retained output. This is not a certificate,
/// publisher authorization or evidence that the checker implemented the applicable policy.
#[derive(Debug, Serialize)]
pub struct OutputPolicyCheck<'r, 'a> {
    #[serde(flatten)]
    evidence: OutputPolicyEvidence,
    #[serde(skip)]
    rendered: &'r PublicationRender<'a>,
}

/// Owned observation of a completed check. Retained on a later history-write failure,
/// so losing the render borrow cannot erase an already executed checker.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct OutputPolicyEvidence {
    status: &'static str,
    invocation: PolicyInvocation,
    output: TreeIdentity,
    #[serde(skip_serializing_if = "Option::is_none")]
    disclosure_sha256: Option<Sha256Digest>,
    candidate_check_sha256: Option<Sha256Digest>,
    response: Option<MatchedOutputResponse>,
    policy_source: Option<PolicyDocument>,
}

impl<'r, 'a> OutputPolicyCheck<'r, 'a> {
    /// Exact invocation digests without duplicating the private request payload.
    pub fn invocation(&self) -> PolicyInvocation {
        self.evidence.invocation
    }

    /// Complete structural tree identity, distinct from certified artifact identity.
    pub fn output(&self) -> TreeIdentity {
        self.evidence.output
    }

    /// Checker-originated response when independently required; absence is exit-only evidence.
    pub fn response(&self) -> Option<&MatchedOutputResponse> {
        self.evidence.response.as_ref()
    }

    /// Move the original private observation without rerunning or copying response bytes.
    pub(crate) fn into_evidence(self) -> OutputPolicyEvidence {
        self.evidence
    }

    /// The same immutable rendered owner checked by this invocation, not a reconstructed tree.
    pub fn rendered(&self) -> &'r PublicationRender<'a> {
        self.rendered
    }
}

impl<'a> PublicationRender<'a> {
    /// Deliver every retained output byte and its provenance to the workspace-selected checker.
    /// The request distinguishes local output, candidate selection and preview URL projection.
    /// A candidate-review result cannot substitute for this independent output invocation.
    ///
    /// # Errors
    /// Refuses missing command/permission, incomplete bounded encoding and failed native execution.
    /// No prefix, live directory, rewritten output or automatically retried effect is accepted.
    pub fn check_output_policy(
        &self,
        runner: &(impl gazette_ports::process::Runner + gazette_ports::process::CaptureRunner),
        options: process::Options,
        requirement: Option<&OutputPolicyRequirement>,
    ) -> Result<OutputPolicyCheck<'_, 'a>, PolicyCheckError> {
        self.check_output_with_disclosure(runner, options, None, requirement)
    }

    pub(crate) fn check_output_with_disclosure(
        &self,
        runner: &(impl gazette_ports::process::Runner + gazette_ports::process::CaptureRunner),
        options: process::Options,
        disclosure: Option<&DisclosureProbe<'_>>,
        requirement: Option<&OutputPolicyRequirement>,
    ) -> Result<OutputPolicyCheck<'_, 'a>, PolicyCheckError> {
        let command = command(self.inputs(), options)?;
        if let Some(required) = requirement {
            required.check_command(command)?;
        }
        let policy_source = policy_input(self.inputs(), requirement.map(|r| &r.policy))?;
        let disclosure_sha256 = disclosure
            .map(|report| {
                encode(report, options.max_input_bytes, "policy disclosure bytes")
                    .map(|bytes| Sha256Digest::of(&bytes))
                    .map_err(PolicyCheckError::Encoding)
            })
            .transpose()?;
        let site = self.site();
        let candidate_check_sha256 = self
            .candidate_check()
            .map(|check| {
                encode(
                    check,
                    options.max_input_bytes,
                    "policy candidate evidence bytes",
                )
                .map(|bytes| Sha256Digest::of(&bytes))
                .map_err(PolicyCheckError::Encoding)
            })
            .transpose()?;
        let framed = self
            .inputs()
            .compilation()
            .workspace
            .policy_output_transport()
            == gazette_compiler::workspace::PolicyOutputTransport::FramedV2;
        let request = Request {
            format: if framed {
                "gazette-output-policy/2"
            } else {
                "gazette-output-policy/1"
            },
            repository_source_provenance: crate::build::repository_source_provenance(),
            mode: "local",
            acquisition: self.inputs().inspect(),
            selection: self.selection(),
            candidate_check: self.candidate_check(),
            preview_urls: self.preview_urls(),
            destination: self
                .inputs()
                .compilation()
                .workspace
                .destination(self.inputs().site()),
            render: site.evidence(),
            inventory: site.output().inventory(),
            media_types: site.media_types(),
            files_base64: (!framed).then(|| Files(site.output())),
            disclosure,
            policy_source: policy_source.as_ref(),
        };
        let (invocation, response) = if framed {
            framed::invoke(
                &request,
                site.output(),
                runner,
                command,
                options,
                requirement,
            )?
        } else {
            let input = encode(&request, options.max_input_bytes, "policy stdin bytes")
                .map_err(PolicyCheckError::Encoding)?;
            let (invocation, response) = if let Some(required) = requirement {
                let invocation = PolicyInvocation {
                    request_sha256: Sha256Digest::of(&input),
                    command_sha256: command_digest(command)?,
                };
                let captured = runner
                    .capture(process::CaptureRequest {
                        request: process::Request {
                            command,
                            input: &input,
                            options,
                        },
                        max_output_bytes: required.response_bytes,
                        max_stderr_bytes: None,
                    })
                    .map_err(|source| PolicyCheckError::Execution { invocation, source })?;
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
                (invocation, Some(response))
            } else {
                (invoke(runner, command, options, &input)?, None)
            };
            (invocation, response)
        };
        Ok(OutputPolicyCheck {
            evidence: OutputPolicyEvidence {
                status: "output-policy-check-passed-unverified",
                invocation,
                output: site.output().inventory().identity(),
                disclosure_sha256,
                candidate_check_sha256,
                response,
                policy_source,
            },
            rendered: self,
        })
    }
}

#[derive(Serialize)]
struct Request<'a> {
    format: &'static str,
    repository_source_provenance: crate::build::RepositorySourceProvenance,
    mode: &'static str,
    acquisition: CompositionInspection<'a>,
    selection: Option<&'a authored::Selection>,
    candidate_check: Option<&'a super::PolicyCheck>,
    preview_urls: Option<&'a BTreeMap<Key, PublicBaseUrl>>,
    destination: Option<&'a gazette_core::release::PublicationDestination>,
    render: &'a SiteRenderEvidence,
    inventory: &'a Inventory,
    media_types: &'a BTreeMap<EntryPath, MediaType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    files_base64: Option<Files<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    disclosure: Option<&'a DisclosureProbe<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    policy_source: Option<&'a PolicyDocument>,
}

// Stream borrowed payloads through the existing bounded JSON writer, without constructing
// a second file map or allocating a complete base64 string before enforcing the ceiling.
struct Files<'a>(&'a CapturedSource);

impl Serialize for Files<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        for entry in self.0.inventory().entries() {
            if matches!(entry.content, Content::File { .. }) {
                let bytes = self.0.file(&entry.path).ok_or_else(|| {
                    <S::Error as serde::ser::Error>::custom("retained output payload missing")
                })?;
                map.serialize_entry(&entry.path, &Payload(bytes))?;
            }
        }
        map.end()
    }
}

struct Payload<'a>(&'a [u8]);

impl Serialize for Payload<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&Base64Display::new(self.0, &STANDARD))
    }
}
