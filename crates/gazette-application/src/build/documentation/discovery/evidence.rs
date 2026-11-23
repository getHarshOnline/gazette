//! One bounded encoding at catalog admission, never a render-time serialization.

use super::{DocumentationError, Sha256Digest, invalid};
use std::io::{self, Write};

struct Bounded {
    bytes: Vec<u8>,
    maximum: usize,
}
impl Write for Bounded {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.maximum.saturating_sub(self.bytes.len()) {
            return Err(io::Error::other("catalog evidence byte bound exceeded"));
        }
        self.bytes
            .try_reserve(bytes.len())
            .map_err(|_| io::Error::other("catalog evidence allocation failed"))?;
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
pub(super) fn digest(
    value: &impl serde::Serialize,
    maximum: usize,
) -> Result<Sha256Digest, DocumentationError> {
    identity(value, maximum).map(|(digest, _)| digest)
}
pub(super) fn identity(
    value: &impl serde::Serialize,
    maximum: usize,
) -> Result<(Sha256Digest, usize), DocumentationError> {
    let mut output = Bounded {
        bytes: Vec::new(),
        maximum,
    };
    serde_json::to_writer(&mut output, value)
        .map_err(|_| invalid("catalog evidence encoding exceeds its bound"))?;
    Ok((Sha256Digest::of(&output.bytes), output.bytes.len()))
}
