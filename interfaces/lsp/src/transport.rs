//! Size-first LSP framing. Native message types do not own allocation policy.

use std::io::{BufRead, Read, Write};

use gazette_sdk::render::LimitedWriter;
use lsp_server::{Message, Notification, Request};
use serde::Serialize;

use crate::{Error, Limits};

pub(super) fn read(reader: &mut impl BufRead, limits: Limits) -> Result<Option<Message>, Error> {
    let mut header_bytes = 0usize;
    let mut length = None;
    loop {
        let remaining = limits.header_bytes.saturating_sub(header_bytes);
        let mut line = Vec::new();
        let count = reader
            .by_ref()
            .take(remaining.saturating_add(1) as u64)
            .read_until(b'\n', &mut line)?;
        if count == 0 {
            return if header_bytes == 0 {
                Ok(None)
            } else {
                Err(Error::Protocol("incomplete headers"))
            };
        }
        if count > remaining {
            return Err(Error::Limit("header bytes"));
        }
        header_bytes += count;
        if !line.ends_with(b"\r\n") || !line.is_ascii() {
            return Err(Error::Protocol("headers require ASCII and CRLF"));
        }
        if line == b"\r\n" {
            break;
        }
        let header = std::str::from_utf8(&line[..line.len() - 2])
            .map_err(|_| Error::Protocol("invalid header"))?;
        let (name, value) = header
            .split_once(':')
            .ok_or(Error::Protocol("header requires a colon"))?;
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err(Error::Protocol("invalid header name"));
        }
        let value = value.trim();
        if name.eq_ignore_ascii_case("content-length") {
            if length.is_some()
                || value.is_empty()
                || !value.bytes().all(|byte| byte.is_ascii_digit())
            {
                return Err(Error::Protocol("duplicate or invalid Content-Length"));
            }
            let bytes = value
                .parse::<usize>()
                .map_err(|_| Error::Limit("message bytes"))?;
            if bytes > limits.message_bytes {
                return Err(Error::Limit("message bytes"));
            }
            length = Some(bytes);
        } else if name.eq_ignore_ascii_case("content-type") {
            let mut parts = value.split(';').map(str::trim);
            if !parts
                .next()
                .is_some_and(|mime| mime.eq_ignore_ascii_case("application/vscode-jsonrpc"))
                || parts.any(|part| {
                    !part.eq_ignore_ascii_case("charset=utf-8")
                        && !part.eq_ignore_ascii_case("charset=utf8")
                })
            {
                return Err(Error::Protocol("unsupported content type or encoding"));
            }
        }
    }
    let length = length.ok_or(Error::Protocol("missing Content-Length"))?;
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    let value = gazette_sdk::json::parse(&body)
        .map_err(|_| Error::Protocol("invalid or duplicate-key JSON"))?;
    let object = value
        .as_object()
        .ok_or(Error::Protocol("message must be an object"))?;
    if object.get("jsonrpc").and_then(serde_json::Value::as_str) != Some("2.0") {
        return Err(Error::Protocol("jsonrpc must be 2.0"));
    }
    if object.contains_key("method") {
        if object
            .keys()
            .any(|key| !matches!(key.as_str(), "jsonrpc" | "id" | "method" | "params"))
            || object.get("id").is_some_and(serde_json::Value::is_null)
            || object
                .get("params")
                .is_some_and(|params| !params.is_null() && !params.is_object())
        {
            return Err(Error::Protocol("invalid request or notification shape"));
        }
    } else {
        return Err(Error::Protocol("unsolicited response or missing method"));
    }
    // Untagged Message deserialization could reinterpret an invalid request ID
    // as a notification by ignoring that field. The explicit wire discriminator
    // must choose its native type before decoding, with no fallback variant.
    let message = if object.contains_key("id") {
        serde_json::from_value::<Request>(value).map(Message::Request)
    } else {
        serde_json::from_value::<Notification>(value).map(Message::Notification)
    };
    message
        .map(Some)
        .map_err(|_| Error::Protocol("invalid native LSP message"))
}

pub(super) fn write(
    writer: &mut impl Write,
    message: &Message,
    maximum: usize,
) -> Result<(), Error> {
    #[derive(Serialize)]
    struct Envelope<'a> {
        jsonrpc: &'static str,
        #[serde(flatten)]
        message: &'a Message,
    }
    let mut bytes = LimitedWriter::new(Vec::new(), maximum);
    let encoded = serde_json::to_writer(
        &mut bytes,
        &Envelope {
            jsonrpc: "2.0",
            message,
        },
    );
    if bytes.exceeded() {
        return Err(Error::Limit("output bytes"));
    }
    encoded.map_err(Error::Encoding)?;
    let bytes = bytes.into_inner();
    write!(writer, "Content-Length: {}\r\n\r\n", bytes.len())?;
    writer.write_all(&bytes)?;
    writer.flush()?;
    Ok(())
}
