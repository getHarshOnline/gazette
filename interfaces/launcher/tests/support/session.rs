//! Real child lifetime and bounded JSONL event observation, shared by CLI session fixtures.

use serde_json::Value;
use std::{
    io::{BufRead, BufReader, Read},
    process::{Child, Command, ExitStatus},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

const TIMEOUT: Duration = Duration::from_secs(10);

pub(super) struct Running {
    pub(super) child: Child,
    pub(super) events: mpsc::Receiver<Value>,
}

impl Running {
    pub(super) fn start(mut command: Command) -> Self {
        let mut child = command.spawn().expect("Gazette session");
        let output = child.stdout.take().expect("event stream");
        let (send, events) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                let event = serde_json::from_str(&line.expect("event line")).expect("JSON event");
                if send.send(event).is_err() {
                    break;
                }
            }
        });
        Self { child, events }
    }

    pub(super) fn event(&self) -> Value {
        self.events
            .recv_timeout(TIMEOUT)
            .expect("bounded session event")
    }

    pub(super) fn wait(&mut self) -> ExitStatus {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            if let Some(status) = self.child.try_wait().expect("native status") {
                return status;
            }
            assert!(Instant::now() < deadline, "session failed to terminate");
            thread::sleep(Duration::from_millis(10));
        }
    }

    pub(super) fn stderr(&mut self) -> String {
        let mut text = String::new();
        self.child
            .stderr
            .take()
            .expect("stderr")
            .read_to_string(&mut text)
            .expect("terminal stderr");
        text
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
