//! MCP projection over Kernel-owned editor-safe selected-buffer sessions.

use std::sync::Arc;

use gazette_cli::command::{
    Kernel,
    authoring::{self as command, InspectionFailure, Mutation},
};
use rmcp::model::{CallToolResult, ErrorData};
use serde::Serialize;

use crate::tools::{encode, failure};

mod queries;
mod wire;
pub(super) use wire::{Input, Route};

pub(super) struct Sessions {
    kernel: Arc<Kernel>,
}

#[derive(Serialize, schemars::JsonSchema)]
struct Acknowledgement<'a> {
    session: &'a str,
    #[schemars(range(max = u64::MAX))]
    generation: u64,
    changed: bool,
}

impl Default for Sessions {
    fn default() -> Self {
        Self::new(Arc::new(Kernel::new()))
    }
}

impl Sessions {
    pub(super) fn new(kernel: Arc<Kernel>) -> Self {
        Self { kernel }
    }

    pub(super) fn execute(
        &self,
        input: Input,
        maximum: usize,
    ) -> Result<CallToolResult, ErrorData> {
        if matches!(input, Input::Start(_) | Input::Import(_)) {
            let mut random = [0u8; 32];
            getrandom::fill(&mut random).map_err(|_| {
                ErrorData::internal_error("Authoring handle entropy unavailable", None)
            })?;
            let handle = random
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            // An undeliverably large acknowledgement must not allocate a hidden
            // session. Carrier loss after insertion is bounded by absolute expiry.
            let reservation = encode(
                &Acknowledgement {
                    session: &handle,
                    generation: if matches!(input, Input::Start(_)) {
                        1
                    } else {
                        u64::MAX
                    },
                    changed: true,
                },
                false,
                maximum,
            )?;
            if reservation.is_error == Some(true) {
                return Ok(reservation);
            }
            let started = match input {
                Input::Start(selection) => {
                    command::start_selection(&self.kernel, handle, selection)
                }
                Input::Import(input) => input
                    .into_context()
                    .map_err(command::Failure::Native)
                    .and_then(|context| {
                        self.kernel
                            .execute(command::ImportEditorContext { handle, context })
                    }),
                _ => unreachable!("handled only session creation"),
            };
            return match started {
                Ok(acknowledgement) => acknowledge(&acknowledgement, maximum),
                Err(error) => command_failure(&error, maximum),
            };
        }
        let handle = input.handle().to_owned();
        let generation = input.generation();
        if input.mutates() {
            // Prove the largest mutation acknowledgement fits before changing
            // SDK state; cancellation/lost carrier still cannot undo an effect.
            let capacity = encode(
                &Acknowledgement {
                    session: &handle,
                    generation: u64::MAX,
                    changed: false,
                },
                false,
                maximum,
            )?;
            if capacity.is_error == Some(true) {
                return Ok(capacity);
            }
        }
        match input {
            Input::Start(_) | Input::Import(_) => {
                unreachable!("handled before looking up a session")
            }
            Input::Status(input) => inspect(
                &self.kernel,
                input.session,
                None,
                |session| queries::status(&handle, session, maximum),
                maximum,
            ),
            Input::Origin(input) => inspect(
                &self.kernel,
                input.session,
                generation,
                |session| match session.origin(input.generation, &input.query) {
                    Ok(value) => encode(&value, false, maximum),
                    Err(error) => failure(
                        error.code(),
                        "Current workspace origin is unavailable; no prior result was substituted.",
                        maximum,
                    ),
                },
                maximum,
            ),
            Input::Diagnostics(input) => inspect(
                &self.kernel,
                input.session,
                generation,
                |session| queries::diagnostics(session, &input.document, maximum),
                maximum,
            ),
            Input::PagePackage(input) => inspect(
                &self.kernel,
                input.session,
                generation,
                |session| queries::page_package(session, &input.document, maximum),
                maximum,
            ),
            Input::Symbols(input) => inspect(
                &self.kernel,
                input.session,
                generation,
                |session| queries::symbols(session, &input.document, maximum),
                maximum,
            ),
            Input::Search(input) => inspect(
                &self.kernel,
                input.session,
                generation,
                |session| queries::search(session, &input.query, maximum),
                maximum,
            ),
            Input::Hover(input) => inspect(
                &self.kernel,
                input.session,
                generation,
                |session| queries::hover(session, &input.document, input.offset, maximum),
                maximum,
            ),
            Input::Assistance(input) => inspect(
                &self.kernel,
                input.session,
                generation,
                |session| queries::assistance(session, &input.document, input.offset, maximum),
                maximum,
            ),
            Input::ComponentAssistance(input) => inspect(
                &self.kernel,
                input.session,
                generation,
                |session| {
                    queries::component_assistance(session, &input.document, input.offset, maximum)
                },
                maximum,
            ),
            Input::Completion(input) => inspect(
                &self.kernel,
                input.session,
                generation,
                |session| queries::completion(session, &input.document, input.offset, maximum),
                maximum,
            ),
            Input::Repair(input) => inspect(
                &self.kernel,
                input.session.clone(),
                generation,
                |session| queries::repair(session, &input, maximum),
                maximum,
            ),
            Input::HeadingCompletion(input) => inspect(
                &self.kernel,
                input.session,
                generation,
                |session| {
                    queries::heading_completion(session, &input.document, input.offset, maximum)
                },
                maximum,
            ),
            Input::Definition(input) => inspect(
                &self.kernel,
                input.session,
                generation,
                |session| {
                    queries::navigation(session, &input.document, input.offset, None, maximum)
                },
                maximum,
            ),
            Input::References(input) => inspect(
                &self.kernel,
                input.session,
                generation,
                |session| {
                    queries::navigation(
                        session,
                        &input.document,
                        input.offset,
                        Some(input.include_declaration),
                        maximum,
                    )
                },
                maximum,
            ),
            Input::End(input) => match self.kernel.execute(command::End {
                handle: input.session,
                generation: input.generation,
            }) {
                Ok(acknowledgement) => acknowledge(&acknowledgement, maximum),
                Err(error) => command_failure(&error, maximum),
            },
            Input::Open(input) => mutate(
                &self.kernel,
                input.session,
                input.generation,
                Mutation::Open {
                    document: input.document,
                    version: input.version,
                    text: input.text,
                },
                maximum,
            ),
            Input::Change(input) => mutate(
                &self.kernel,
                input.session,
                input.generation,
                Mutation::Change {
                    document: input.document,
                    version: input.version,
                    text: input.text,
                },
                maximum,
            ),
            Input::Close(input) => mutate(
                &self.kernel,
                input.session,
                input.generation,
                Mutation::Close {
                    document: input.document,
                },
                maximum,
            ),
        }
    }
}

fn inspect<F>(
    kernel: &Kernel,
    handle: String,
    generation: Option<u64>,
    inspect: F,
    maximum: usize,
) -> Result<CallToolResult, ErrorData>
where
    F: FnOnce(&gazette_cli::sdk::authoring::session::Session) -> Result<CallToolResult, ErrorData>,
{
    match kernel.execute(command::Inspect {
        handle,
        generation,
        inspect,
    }) {
        Ok(result) => Ok(result),
        Err(InspectionFailure::Session(error)) => command_failure(&error, maximum),
        Err(InspectionFailure::Projection(error)) => Err(error),
    }
}

fn mutate(
    kernel: &Kernel,
    handle: String,
    generation: u64,
    mutation: Mutation,
    maximum: usize,
) -> Result<CallToolResult, ErrorData> {
    match kernel.execute(command::Mutate {
        handle,
        generation,
        mutation,
    }) {
        Ok(acknowledgement) => acknowledge(&acknowledgement, maximum),
        Err(error) => command_failure(&error, maximum),
    }
}

fn acknowledge(
    acknowledgement: &command::Acknowledgement,
    maximum: usize,
) -> Result<CallToolResult, ErrorData> {
    encode(
        &Acknowledgement {
            session: &acknowledgement.session,
            generation: acknowledgement.generation,
            changed: acknowledgement.changed,
        },
        false,
        maximum,
    )
}

fn command_failure(error: &command::Failure, maximum: usize) -> Result<CallToolResult, ErrorData> {
    if matches!(error, command::Failure::HandleCollision) {
        return Err(ErrorData::internal_error(error.reason(), None));
    }
    failure(error.code(), error.reason(), maximum)
}

#[cfg(test)]
#[path = "../tests/internal/authoring.rs"]
mod tests;
