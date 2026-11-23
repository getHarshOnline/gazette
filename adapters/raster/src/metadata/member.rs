//! Exact zlib-member consumption through the pinned native codec; no pixel interpretation.

use flate2::{Decompress, FlushDecompress, Status};
use gazette_core::raster::Error;

use super::Budget;

pub(super) struct Member {
    decoder: Decompress,
    done: bool,
    latin1_bytes: usize,
}

impl Member {
    pub(super) fn new() -> Self {
        Self {
            decoder: Decompress::new(true),
            done: false,
            latin1_bytes: 0,
        }
    }

    pub(super) fn decoded(&self) -> Result<usize, Error> {
        usize::try_from(self.decoder.total_out())
            .map_err(|_| Error::Limit("metadata decoded bytes"))
    }

    pub(super) fn latin1_bytes(&self) -> usize {
        self.latin1_bytes
    }

    pub(super) fn feed(&mut self, mut input: &[u8], budget: &mut Budget) -> Result<(), Error> {
        let mut scratch = [0; 8192];
        loop {
            if self.done {
                return if input.is_empty() {
                    Ok(())
                } else {
                    Err(Error::Decode)
                };
            }
            let available = scratch.len().min(budget.remaining());
            let before_in = self.decoder.total_in();
            let before_out = self.decoder.total_out();
            let status = self
                .decoder
                .decompress(input, &mut scratch[..available], FlushDecompress::None)
                .map_err(|_| Error::Decode)?;
            let consumed =
                usize::try_from(self.decoder.total_in() - before_in).map_err(|_| Error::Decode)?;
            let produced = usize::try_from(self.decoder.total_out() - before_out)
                .map_err(|_| Error::Decode)?;
            budget.charge(produced)?;
            self.latin1_bytes = self
                .latin1_bytes
                .checked_add(produced)
                .and_then(|bytes| {
                    bytes.checked_add(
                        scratch[..produced]
                            .iter()
                            .filter(|&&byte| byte >= 128)
                            .count(),
                    )
                })
                .ok_or(Error::Limit("metadata UTF-8 expansion"))?;
            input = input.get(consumed..).ok_or(Error::Decode)?;
            if status == Status::StreamEnd {
                self.done = true;
                continue;
            }
            if consumed == 0 && produced == 0 {
                return if available == 0 {
                    Err(Error::Limit("metadata decoded bytes"))
                } else if input.is_empty() {
                    Ok(())
                } else {
                    Err(Error::Decode)
                };
            }
            if input.is_empty() && produced < available {
                return Ok(());
            }
        }
    }

    pub(super) fn finish(&mut self, budget: &mut Budget) -> Result<(), Error> {
        self.feed(&[], budget)?;
        if self.done {
            Ok(())
        } else {
            Err(Error::Decode)
        }
    }
}
