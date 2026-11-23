//! Checked parser coordinates over original UTF-8, including CRLF and lone CR.

use comrak::nodes::Sourcepos;
use gazette_core::markdown::Error;
use gazette_core::source::{Location, Precision, Span};
use std::ops::Range;

pub(crate) struct Lines<'a> {
    source: &'a str,
    starts: Vec<usize>,
}

impl<'a> Lines<'a> {
    pub(crate) fn new(source: &'a str) -> Self {
        let bytes = source.as_bytes();
        let mut starts = vec![0];
        let mut index = 0;
        while index < bytes.len() {
            match bytes[index] {
                b'\r' => {
                    index += 1;
                    if bytes.get(index) == Some(&b'\n') {
                        index += 1;
                    }
                    starts.push(index);
                }
                b'\n' => {
                    index += 1;
                    starts.push(index);
                }
                _ => index += 1,
            }
        }
        Self { source, starts }
    }

    pub(crate) fn location(&self, position: Sourcepos) -> Result<Location, Error> {
        let start_line = position
            .start
            .line
            .checked_sub(1)
            .ok_or(Error::InvalidSource)?;
        let end_line = position
            .end
            .line
            .checked_sub(1)
            .ok_or(Error::InvalidSource)?;
        let start = self
            .starts
            .get(start_line)
            .ok_or(Error::InvalidSource)?
            .checked_add(
                position
                    .start
                    .column
                    .checked_sub(1)
                    .ok_or(Error::InvalidSource)?,
            )
            .ok_or(Error::InvalidSource)?;
        let end = self
            .starts
            .get(end_line)
            .ok_or(Error::InvalidSource)?
            .checked_add(position.end.column)
            .ok_or(Error::InvalidSource)?;
        if start
            > *self
                .starts
                .get(start_line + 1)
                .unwrap_or(&self.source.len())
            || end > *self.starts.get(end_line + 1).unwrap_or(&self.source.len())
        {
            return Err(Error::InvalidSource);
        }
        let span = Span::checked(self.source, start..end).ok_or(Error::InvalidSource)?;
        Ok(Location {
            span,
            precision: Precision::Construct,
        })
    }

    pub(crate) fn exact(&self, range: Range<usize>) -> Result<Location, Error> {
        let span = Span::checked(self.source, range).ok_or(Error::InvalidSource)?;
        Ok(Location {
            span,
            precision: Precision::Exact,
        })
    }

    pub(crate) fn slice(&self, range: Range<usize>) -> Result<&'a str, Error> {
        self.source.get(range).ok_or(Error::InvalidSource)
    }

    /// Recover only a parser-recognized header; this does not recognize directive syntax.
    pub(crate) fn directive_header(
        &self,
        position: Sourcepos,
        fence: usize,
    ) -> Result<(Location, &'a str), Error> {
        let location = self.location(position)?;
        let start = location.span.start;
        let tail = self.source.get(start..).ok_or(Error::InvalidSource)?;
        let line = tail
            .split(['\r', '\n'])
            .next()
            .ok_or(Error::InvalidSource)?;
        if fence < 3
            || !line
                .as_bytes()
                .get(..fence)
                .is_some_and(|bytes| bytes.iter().all(|byte| *byte == b':'))
        {
            return Err(Error::InvalidSource);
        }
        let span = Span::checked(self.source, start + fence..start + line.len())
            .ok_or(Error::InvalidSource)?;
        Ok((
            Location {
                span,
                precision: Precision::Exact,
            },
            span.slice(self.source).ok_or(Error::InvalidSource)?,
        ))
    }
}
