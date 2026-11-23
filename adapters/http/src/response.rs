//! One response deadline covers negotiated encoding and close-delimited writes.

use std::{
    io::{self, Write},
    net::TcpStream,
    time::{Duration, Instant},
};

use crate::encoding::{Preferences, Representation};
use gazette_core::{
    publication::routing::RoutePath,
    render::MediaType,
    source::{CapturedSource, EntryPath},
};

pub(super) struct Response<'a> {
    pub status: u16,
    pub mime: &'static str,
    pub body: &'a [u8],
    pub location: Option<String>,
    pub content_range: Option<String>,
    pub accept_ranges: bool,
}

impl Response<'_> {
    pub(super) fn error(status: u16) -> Self {
        Self {
            status,
            mime: "text/plain; charset=utf-8",
            body: b"Preview request refused.\n",
            location: None,
            content_range: None,
            accept_ranges: false,
        }
    }

    pub(super) fn write(
        &self,
        stream: &mut TcpStream,
        head: bool,
        timeout: Duration,
        encoding: Preferences,
    ) -> io::Result<()> {
        let deadline = Instant::now() + timeout;
        let representation = encoding.encode(self.mime, self.body, deadline)?;
        let (status, body) = match &representation {
            Representation::Identity => (self.status, self.body),
            Representation::Gzip(bytes) => (self.status, bytes.as_slice()),
            Representation::Unacceptable => (406, &[][..]),
        };
        let reason = match status {
            200 => "OK",
            206 => "Partial Content",
            416 => "Range Not Satisfiable",
            308 => "Permanent Redirect",
            404 => "Not Found",
            405 => "Method Not Allowed",
            406 => "Not Acceptable",
            431 => "Request Header Fields Too Large",
            505 => "HTTP Version Not Supported",
            _ => "Bad Request",
        };
        let mut header = format!(
            "HTTP/1.1 {} {reason}\r\nContent-Length: {}\r\nContent-Type: {}\r\nConnection: close\r\nCache-Control: no-store\r\nVary: Accept-Encoding\r\nX-Content-Type-Options: nosniff\r\n",
            status,
            body.len(),
            self.mime
        );
        if matches!(representation, Representation::Gzip(_)) {
            header.push_str("Content-Encoding: gzip\r\n");
        }
        if status != 406
            && let Some(location) = &self.location
        {
            header.push_str("Location: ");
            header.push_str(location);
            header.push_str("\r\n");
        }
        if status != 406 && self.accept_ranges {
            header.push_str("Accept-Ranges: bytes\r\n");
        }
        if status != 406
            && let Some(range) = &self.content_range
        {
            header.push_str("Content-Range: ");
            header.push_str(range);
            header.push_str("\r\n");
        }
        if status == 405 {
            header.push_str("Allow: GET, HEAD\r\n");
        }
        header.push_str("\r\n");
        write(stream, header.as_bytes(), deadline)?;
        if !head {
            write(stream, body, deadline)?;
        }
        Ok(())
    }
}

/// Assemble once from retained output. `RoutePath` normalizes every segment to
/// percent-encoded URL spelling, so these exact hrefs need no escaping.
pub(super) fn missing_page(
    mount: &RoutePath,
    output: &CapturedSource,
    search: Option<&RoutePath>,
) -> Vec<u8> {
    let not_found: EntryPath = "404.html".parse().expect("constant not-found output");
    if let Some(bytes) = output.file(&not_found) {
        return bytes.to_vec();
    }
    let mut page = String::from(concat!(
        "<!doctype html><html lang=\"en\"><meta charset=\"utf-8\">",
        "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">",
        "<title>Page not found</title><main><h1>Page not found</h1>",
        "<p>This address does not have a page in the current local preview.</p>"
    ));
    let home: EntryPath = "index.html".parse().expect("constant preview home entry");
    if output.file(&home).is_some() {
        page.push_str(&format!("<p><a href=\"{}\">Home</a></p>", mount.as_str()));
    }
    if let Some(search) = search.filter(|route| output.file(route.output()).is_some()) {
        page.push_str(&format!(
            "<p><a href=\"{}{}\">Search</a></p>",
            mount.as_str(),
            search.as_str().trim_start_matches('/')
        ));
    }
    page.push_str("</main></html>");
    page.into_bytes()
}

fn write(stream: &mut TcpStream, mut bytes: &[u8], deadline: Instant) -> io::Result<()> {
    while !bytes.is_empty() {
        stream.set_write_timeout(Some(remaining(deadline)?))?;
        match stream.write(bytes) {
            Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
            Ok(written) => bytes = &bytes[written..],
            Err(error) if error.kind() == io::ErrorKind::Interrupted => (),
            Err(error) => return Err(error),
        }
    }
    // Success means native writes completed, not that the browser consumed them.
    remaining(deadline).map(|_| ())
}

pub(super) fn remaining(deadline: Instant) -> io::Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|value| !value.is_zero())
        .ok_or_else(|| io::ErrorKind::TimedOut.into())
}

pub(super) fn mime(path: &str, declared: Option<&MediaType>) -> &'static str {
    if let Some(declared) = declared {
        return match declared {
            MediaType::Html => "text/html; charset=utf-8",
            MediaType::Json => "application/json",
            MediaType::JsonLd => "application/ld+json",
            MediaType::WebManifest => "application/manifest+json",
            MediaType::Xml => "application/xml",
            MediaType::Rss => "application/rss+xml",
            MediaType::Atom => "application/atom+xml",
            MediaType::Text => "text/plain; charset=utf-8",
            MediaType::Markdown => "text/markdown; charset=utf-8",
            MediaType::JavaScript => "text/javascript; charset=utf-8",
            MediaType::Css => "text/css; charset=utf-8",
            MediaType::Png => "image/png",
            MediaType::Jpeg => "image/jpeg",
            MediaType::Webp => "image/webp",
            MediaType::Svg => "image/svg+xml",
            MediaType::Woff2 => "font/woff2",
        };
    }
    match path
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" | "map" => "application/json",
        "jsonld" => "application/ld+json",
        "xml" => "application/xml",
        "rss" => "application/rss+xml",
        "atom" => "application/atom+xml",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "vtt" => "text/vtt; charset=utf-8",
        "ico" => "image/vnd.microsoft.icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "txt" => "text/plain; charset=utf-8",
        "md" | "markdown" => "text/markdown; charset=utf-8",
        _ => "application/octet-stream",
    }
}
