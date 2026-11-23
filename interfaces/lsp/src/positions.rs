//! Checked editor coordinates shared by diagnostics, symbols and navigation.

use crate::Error;
use gazette_sdk::source::Span;
use lsp_types::{Position, Range};

pub(super) fn range(source: &str, span: Span) -> Option<Range> {
    span.slice(source)?;
    Some(Range::new(
        position(source, span.start).ok()?,
        position(source, span.end).ok()?,
    ))
}

pub(super) fn position(source: &str, offset: usize) -> Result<Position, Error> {
    if !source.is_char_boundary(offset) {
        return Err(Error::Protocol("position byte offset is invalid"));
    }
    let mut line = 0u32;
    let mut character = 0u32;
    let mut chars = source.char_indices().peekable();
    while let Some((start, value)) = chars.next() {
        if start >= offset {
            break;
        }
        if value == '\r' {
            if let Some(&(next, '\n')) = chars.peek() {
                // CR and LF are one line separator, with no addressable column
                // between them. Both separator bytes map to the old line's end.
                if next >= offset {
                    break;
                }
                chars.next();
            }
            line = line.checked_add(1).ok_or(Error::Limit("position lines"))?;
            character = 0;
        } else if value == '\n' {
            line = line.checked_add(1).ok_or(Error::Limit("position lines"))?;
            character = 0;
        } else {
            character = character
                .checked_add(value.len_utf16() as u32)
                .ok_or(Error::Limit("position columns"))?;
        }
    }
    Ok(Position::new(line, character))
}

/// Reject non-existent lines/columns and positions inside a UTF-16 surrogate pair.
pub(super) fn offset(source: &str, wanted: Position) -> Result<usize, Error> {
    let mut line = 0u32;
    let mut character = 0u32;
    let mut chars = source.char_indices().peekable();
    while let Some((start, value)) = chars.next() {
        if Position::new(line, character) == wanted {
            return Ok(start);
        }
        if value == '\r' || value == '\n' {
            if line == wanted.line {
                return Err(Error::Protocol("position exceeds line length"));
            }
            if value == '\r' && chars.peek().is_some_and(|(_, next)| *next == '\n') {
                chars.next();
            }
            line = line.checked_add(1).ok_or(Error::Limit("position lines"))?;
            character = 0;
        } else {
            character = character
                .checked_add(value.len_utf16() as u32)
                .ok_or(Error::Limit("position columns"))?;
            if line == wanted.line && character > wanted.character {
                return Err(Error::Protocol(
                    "position is inside a UTF-16 surrogate pair",
                ));
            }
        }
    }
    if Position::new(line, character) == wanted {
        Ok(source.len())
    } else {
        Err(Error::Protocol("position is outside the document"))
    }
}
