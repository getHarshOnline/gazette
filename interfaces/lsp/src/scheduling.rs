//! Bounded, single-owner scheduling for editor analysis.

use std::{
    collections::VecDeque,
    sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError},
    time::{Duration, Instant},
};

use lsp_server::{Message, Notification};
use lsp_types::DidChangeTextDocumentParams;

use crate::Error;

/// Pending protocol work is bounded independently from frame bytes.
pub(super) const PENDING_MESSAGES: usize = 64;
const MAXIMUM_MUTATIONS_PER_BATCH: usize = 64;

const INITIAL_QUIET: Duration = Duration::from_millis(4);
const MINIMUM_QUIET: Duration = Duration::from_millis(2);
const MAXIMUM_QUIET: Duration = Duration::from_millis(40);

pub(super) enum Input {
    Message(Message),
    ReadError(Error),
    Eof,
}

struct Queued {
    input: Input,
    generation: Option<u64>,
}

/// Monotone debounce coordinate and bounded input backlog for one LSP session.
///
/// The scheduler owns no source or semantic state. A generation identifies an
/// accepted protocol candidate only; the SDK session remains the owner of the
/// generation attached to diagnostics and authoring queries.
pub(super) struct Scheduler {
    queued: VecDeque<Queued>,
    generation: u64,
    quiet: Duration,
}

impl Default for Scheduler {
    fn default() -> Self {
        Self {
            queued: VecDeque::new(),
            generation: 0,
            quiet: INITIAL_QUIET,
        }
    }
}

impl Scheduler {
    pub(super) fn next(&mut self, receiver: &Receiver<Input>) -> Result<QueuedInput, Error> {
        if self.queued.is_empty() {
            let input = receiver
                .recv()
                .map_err(|_| Error::Protocol("LSP input worker disconnected"))?;
            self.push(input)?;
        }
        self.pop()
    }

    pub(super) fn batch(
        &mut self,
        first: QueuedInput,
        receiver: &Receiver<Input>,
    ) -> Result<MutationBatch, Error> {
        let mut generation = first.generation.ok_or(Error::Protocol(
            "scheduler mutation is missing a generation",
        ))?;
        let mut notifications = vec![first.notification().ok_or(Error::Protocol(
            "scheduler mutation batch requires a notification",
        ))?];
        let mut deadline = Instant::now() + self.quiet;
        while notifications.len() < MAXIMUM_MUTATIONS_PER_BATCH {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            match receiver.recv_timeout(deadline - now) {
                Ok(input) => self.push(input)?,
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => {
                    self.push(Input::Eof)?;
                    break;
                }
            }
            let Some(front) = self.queued.front() else {
                continue;
            };
            if front.generation.is_none() {
                break;
            }
            let next = self.pop()?;
            generation = next
                .generation
                .ok_or(Error::Protocol("scheduled mutation lost its generation"))?;
            coalesce(
                &mut notifications,
                next.notification().ok_or(Error::Protocol(
                    "scheduler mutation batch requires a notification",
                ))?,
            );
            deadline = Instant::now() + self.quiet;
        }
        Ok(MutationBatch {
            notifications,
            generation,
            started: Instant::now(),
        })
    }

    /// Pull already-arrived work into the owned queue before deciding whether a
    /// completed analysis is still current. This never waits or reads source.
    pub(super) fn drain(&mut self, receiver: &Receiver<Input>) -> Result<(), Error> {
        while self.queued.len() < PENDING_MESSAGES {
            match receiver.try_recv() {
                Ok(input) => self.push(input)?,
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return Ok(()),
            }
        }
        Ok(())
    }

    pub(super) fn has_newer_mutation(&self, generation: u64) -> bool {
        self.queued
            .iter()
            .take_while(|input| input.generation.is_some())
            .any(|input| input.generation.is_some_and(|next| next > generation))
    }

    /// Adapt the next quiet interval to observed Gazette selected-set work.
    /// The bounded quarter-duration policy avoids importing Pale's fixed delay.
    pub(super) fn observe(&mut self, elapsed: Duration) {
        let measured = elapsed / 4;
        self.quiet = measured.clamp(MINIMUM_QUIET, MAXIMUM_QUIET);
    }

    fn push(&mut self, input: Input) -> Result<(), Error> {
        let generation = match &input {
            Input::Message(Message::Notification(notification))
                if is_scheduled_mutation(notification) =>
            {
                self.generation = self
                    .generation
                    .checked_add(1)
                    .ok_or(Error::Limit("analysis schedule generation"))?;
                Some(self.generation)
            }
            Input::Message(_) | Input::ReadError(_) | Input::Eof => None,
        };
        self.queued.push_back(Queued { input, generation });
        Ok(())
    }

    fn pop(&mut self) -> Result<QueuedInput, Error> {
        let queued = self
            .queued
            .pop_front()
            .ok_or(Error::Protocol("scheduler queue is empty"))?;
        Ok(QueuedInput {
            input: queued.input,
            generation: queued.generation,
        })
    }
}

pub(super) struct QueuedInput {
    pub(super) input: Input,
    generation: Option<u64>,
}

impl QueuedInput {
    pub(super) fn is_mutation(&self) -> bool {
        self.generation.is_some()
    }

    fn notification(self) -> Option<Notification> {
        match self.input {
            Input::Message(Message::Notification(notification)) => Some(notification),
            Input::Message(Message::Request(_) | Message::Response(_))
            | Input::ReadError(_)
            | Input::Eof => None,
        }
    }
}

pub(super) struct MutationBatch {
    pub(super) notifications: Vec<Notification>,
    pub(super) generation: u64,
    started: Instant,
}

impl MutationBatch {
    pub(super) fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }
}

pub(super) fn is_scheduled_mutation(notification: &Notification) -> bool {
    notification.method == "textDocument/didChange"
}

pub(super) fn is_document_notification(notification: &Notification) -> bool {
    matches!(
        notification.method.as_str(),
        "textDocument/didOpen"
            | "textDocument/didChange"
            | "textDocument/didClose"
            | "textDocument/didSave"
            | "workspace/didChangeWatchedFiles"
    )
}

fn coalesce(pending: &mut Vec<Notification>, next: Notification) {
    let Some((uri, version)) = complete_change(&next) else {
        pending.push(next);
        return;
    };
    let Some(previous) = pending
        .iter_mut()
        .rev()
        .find(|notification| document_uri(notification) == Some(uri.as_str()))
    else {
        pending.push(next);
        return;
    };
    let Some((_, prior_version)) = complete_change(previous) else {
        pending.push(next);
        return;
    };
    if version <= prior_version {
        pending.push(next);
        return;
    }
    *previous = next;
}

fn complete_change(notification: &Notification) -> Option<(String, i32)> {
    if notification.method != "textDocument/didChange" {
        return None;
    }
    let params =
        serde_json::from_value::<DidChangeTextDocumentParams>(notification.params.clone()).ok()?;
    if params.content_changes.len() != 1
        || params.content_changes[0].range.is_some()
        || params.content_changes[0].range_length.is_some()
    {
        return None;
    }
    Some((
        params.text_document.uri.to_string(),
        params.text_document.version,
    ))
}

fn document_uri(notification: &Notification) -> Option<&str> {
    notification
        .params
        .get("textDocument")?
        .get("uri")?
        .as_str()
}
