//! Content negotiation and bounded lossless content encoding of retained text.

use std::{
    io::{self, Write},
    time::Instant,
};

use flate2::{Compression, write::GzEncoder};
use gazette_core::render::LimitedWriter;

#[derive(Clone, Copy)]
pub(super) struct Preferences {
    gzip: u16,
    identity: u16,
    prefer_gzip: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            gzip: 0,
            identity: 1000,
            prefer_gzip: false,
        }
    }
}

pub(super) enum Representation {
    Identity,
    Gzip(Vec<u8>),
    Unacceptable,
}

impl Preferences {
    pub(super) fn parse(headers: &[httparse::Header<'_>]) -> Result<Self, u16> {
        let (mut gzip, mut identity, mut wildcard) = (None, None, None);
        for header in headers
            .iter()
            .filter(|h| h.name.eq_ignore_ascii_case("accept-encoding"))
        {
            let value = std::str::from_utf8(header.value).map_err(|_| 400u16)?;
            for item in value
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
            {
                let mut parts = item.split(';');
                let coding = parts.next().unwrap_or_default().trim();
                if coding.is_empty() || !coding.bytes().all(token) {
                    return Err(400);
                }
                let quality = match parts.next() {
                    None => 1000,
                    Some(parameter) => {
                        let (name, value) = parameter.trim().split_once('=').ok_or(400u16)?;
                        if !name.eq_ignore_ascii_case("q") {
                            return Err(400);
                        }
                        quality(value)?
                    }
                };
                if parts.next().is_some() {
                    return Err(400);
                }
                let slot = if coding.eq_ignore_ascii_case("gzip")
                    || coding.eq_ignore_ascii_case("x-gzip")
                {
                    &mut gzip
                } else if coding.eq_ignore_ascii_case("identity") {
                    &mut identity
                } else if coding == "*" {
                    &mut wildcard
                } else {
                    continue;
                };
                // A duplicate cannot undo an explicit refusal in another field line.
                *slot = Some(slot.map_or(quality, |prior: u16| prior.min(quality)));
            }
        }
        let gzip = gzip.or(wildcard).unwrap_or(0);
        Ok(Self {
            gzip,
            identity: identity.unwrap_or(if wildcard == Some(0) { 0 } else { 1000 }),
            prefer_gzip: identity.is_none_or(|identity| gzip >= identity),
        })
    }

    pub(super) fn encode(
        self,
        mime: &str,
        body: &[u8],
        deadline: Instant,
    ) -> io::Result<Representation> {
        let text = mime.starts_with("text/")
            || matches!(
                mime,
                "application/json"
                    | "application/ld+json"
                    | "application/manifest+json"
                    | "application/xml"
                    | "application/rss+xml"
                    | "application/atom+xml"
                    | "image/svg+xml"
            );
        if self.gzip > 0 && self.prefer_gzip && text && (body.len() >= 1024 || self.identity == 0) {
            // Only one response buffer exists, bounded by its admitted source plus
            // gzip framing. No cross-request cache outlives a replaced capture.
            let writer = LimitedWriter::new(Vec::new(), body.len().saturating_add(1024));
            let mut encoder = GzEncoder::new(writer, Compression::default());
            let result = (|| {
                for chunk in body.chunks(16 * 1024) {
                    super::response::remaining(deadline)?;
                    encoder.write_all(chunk)?;
                }
                encoder.try_finish()?;
                super::response::remaining(deadline)?;
                Ok::<_, io::Error>(())
            })();
            if !encoder.get_ref().exceeded() {
                result?;
                let bytes = encoder.finish()?.into_inner();
                if bytes.len() < body.len() || self.identity == 0 {
                    return Ok(Representation::Gzip(bytes));
                }
            }
        }
        Ok(if self.identity > 0 {
            Representation::Identity
        } else {
            Representation::Unacceptable
        })
    }
}

fn token(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

fn quality(value: &str) -> Result<u16, u16> {
    let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
    if !matches!(whole, "0" | "1")
        || fraction.len() > 3
        || !fraction.bytes().all(|b| b.is_ascii_digit())
        || (whole == "1" && fraction.bytes().any(|b| b != b'0'))
    {
        return Err(400);
    }
    Ok(if whole == "1" {
        1000
    } else {
        fraction
            .bytes()
            .enumerate()
            .map(|(i, b)| u16::from(b - b'0') * [100, 10, 1][i])
            .sum()
    })
}
