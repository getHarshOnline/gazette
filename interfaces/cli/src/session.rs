//! Bounded JSONL session control shared by read and local writer lifetimes.

use gazette_sdk::store::Error;
use serde::Serialize;
use std::io::{self, Read, Write};

pub(super) fn emit(event: impl Serialize) -> Result<(), Error> {
    let mut output = io::stdout().lock();
    serde_json::to_writer(&mut output, &event)
        .map_err(|error| Error::Io(io::Error::other(error)))?;
    output
        .write_all(b"\n")
        .and_then(|()| output.flush())
        .map_err(Error::Io)
}

pub(super) fn wait_for_close() -> Result<(), Error> {
    // One byte (including Enter), or EOF. No commands, SQL or unbounded buffer.
    io::stdin()
        .lock()
        .read(&mut [0])
        .map(|_| ())
        .map_err(Error::Io)
}
