//! Native HTTP header parsing and a single origin-form retained-file lookup.

use std::{
    collections::BTreeMap,
    io::{self, Read},
    net::{SocketAddr, TcpStream},
    time::Instant,
};

use gazette_core::{
    preview::Options,
    publication::routing::RoutePath,
    render::MediaType,
    source::{CapturedSource, EntryPath},
};

use crate::{
    encoding::Preferences,
    response::{self, Response},
};

pub(super) fn serve(
    stream: &mut TcpStream,
    address: SocketAddr,
    output: &CapturedSource,
    mount: &RoutePath,
    media_types: &BTreeMap<EntryPath, MediaType>,
    missing_page: &[u8],
    options: Options,
) -> io::Result<()> {
    let deadline = Instant::now() + options.request_timeout;
    let mut bytes = vec![0; options.max_header_bytes];
    let mut length = 0;
    loop {
        stream.set_read_timeout(Some(response::remaining(deadline)?))?;
        match stream.read(&mut bytes[length..]) {
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(read) => length += read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
        response::remaining(deadline)?;
        let mut headers = vec![httparse::EMPTY_HEADER; options.max_headers];
        let mut request = httparse::Request::new(&mut headers);
        match request.parse(&bytes[..length]) {
            Ok(httparse::Status::Partial) if length < bytes.len() => continue,
            Ok(httparse::Status::Partial) | Err(httparse::Error::TooManyHeaders) => {
                return Response::error(431).write(
                    stream,
                    request.method == Some("HEAD"),
                    options.response_timeout,
                    Preferences::default(),
                );
            }
            Err(_) => {
                return Response::error(400).write(
                    stream,
                    request.method == Some("HEAD"),
                    options.response_timeout,
                    Preferences::default(),
                );
            }
            Ok(httparse::Status::Complete(consumed)) => {
                let head = request.method == Some("HEAD");
                let mut encoding = Preferences::default();
                let result = validate(&request, consumed == length, address)
                    .and_then(|path| {
                        encoding = Preferences::parse(request.headers)?;
                        route(
                            path,
                            output,
                            mount,
                            media_types,
                            missing_page,
                            page_navigation(&request),
                        )
                    })
                    .and_then(|response| ranged(response, &request));
                let response = result.unwrap_or_else(Response::error);
                return response.write(stream, head, options.response_timeout, encoding);
            }
        }
    }
}

fn validate<'a>(
    request: &httparse::Request<'_, 'a>,
    complete: bool,
    address: SocketAddr,
) -> Result<&'a str, u16> {
    if !complete {
        return Err(400);
    }
    if request.version != Some(1) {
        return Err(505);
    }
    if !matches!(request.method, Some("GET" | "HEAD")) {
        return Err(405);
    }
    let mut host = None;
    for header in request.headers.iter() {
        if header.name.eq_ignore_ascii_case("host") {
            if host.replace(header.value).is_some() {
                return Err(400);
            }
        } else if [
            "content-length",
            "transfer-encoding",
            "te",
            "upgrade",
            "expect",
        ]
        .iter()
        .any(|name| header.name.eq_ignore_ascii_case(name))
            || (header.name.eq_ignore_ascii_case("connection")
                && header
                    .value
                    .split(|byte| *byte == b',')
                    .any(|token| token.trim_ascii().eq_ignore_ascii_case(b"upgrade")))
        {
            return Err(400);
        }
    }
    if host != Some(address.to_string().as_bytes()) {
        return Err(400);
    }
    request.path.ok_or(400)
}

fn page_navigation(request: &httparse::Request<'_, '_>) -> bool {
    let mut accepts_html = None;
    for header in request.headers.iter() {
        if header.name.eq_ignore_ascii_case("range") {
            return false;
        }
        if header.name.eq_ignore_ascii_case("sec-fetch-dest")
            && !header.value.eq_ignore_ascii_case(b"document")
        {
            return false;
        }
        if header.name.eq_ignore_ascii_case("accept") {
            let accepts = header.value.split(|byte| *byte == b',').any(|value| {
                let mime = value
                    .split(|byte| *byte == b';')
                    .next()
                    .unwrap_or_default()
                    .trim_ascii();
                mime.eq_ignore_ascii_case(b"text/html") || mime == b"*/*"
            });
            accepts_html = Some(accepts_html.unwrap_or(false) || accepts);
        }
    }
    accepts_html.unwrap_or(true)
}

fn route<'a>(
    target: &str,
    output: &'a CapturedSource,
    mount: &RoutePath,
    media_types: &BTreeMap<EntryPath, MediaType>,
    missing_page: &'a [u8],
    page_navigation: bool,
) -> Result<Response<'a>, u16> {
    if !target.starts_with('/')
        || target.starts_with("//")
        || target
            .bytes()
            .any(|byte| byte <= b' ' || byte == 0x7f || matches!(byte, b'#' | b'\\'))
    {
        return Err(400);
    }
    let (encoded, query) = target
        .split_once('?')
        .map_or((target, None), |(path, query)| (path, Some(query)));
    for (index, byte) in target.bytes().enumerate() {
        if byte == b'%'
            && !target
                .as_bytes()
                .get(index + 1..index + 3)
                .is_some_and(|pair| pair.iter().all(u8::is_ascii_hexdigit))
        {
            return Err(400);
        }
    }
    let full: RoutePath = encoded.parse().map_err(|_| 400u16)?;
    if mount.as_str() != "/" && full.as_str() == mount.as_str().trim_end_matches('/') {
        return Ok(redirect(mount.as_str().to_owned(), query));
    }
    let relative = full.as_str().strip_prefix(mount.as_str()).ok_or(404u16)?;
    let route: RoutePath = format!("/{relative}").parse().map_err(|_| 400u16)?;
    if let Some(bytes) = output.file(route.output()) {
        return Ok(Response {
            status: 200,
            mime: response::mime(route.output().as_str(), media_types.get(route.output())),
            body: bytes,
            location: None,
            content_range: None,
            accept_ranges: matches!(
                response::mime(route.output().as_str(), media_types.get(route.output())),
                "video/mp4" | "video/webm"
            ),
        });
    }
    if !relative.is_empty() && !relative.ends_with('/') {
        let index: RoutePath = format!("/{relative}/").parse().map_err(|_| 400u16)?;
        if output.file(index.output()).is_some() {
            return Ok(redirect(format!("{}/", full.as_str()), query));
        }
    }
    let last = relative
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or_default();
    if page_navigation && (!last.contains('.') || last.ends_with(".html") || last.ends_with(".htm"))
    {
        return Ok(Response {
            status: 404,
            mime: "text/html; charset=utf-8",
            body: missing_page,
            location: None,
            content_range: None,
            accept_ranges: false,
        });
    }
    Err(404)
}

fn redirect(mut path: String, query: Option<&str>) -> Response<'static> {
    if let Some(query) = query {
        path.push('?');
        path.push_str(query);
    }
    Response {
        status: 308,
        mime: "text/plain; charset=utf-8",
        body: b"",
        location: Some(path),
        content_range: None,
        accept_ranges: false,
    }
}

// Single retained MP4 byte ranges; no multipart allocation or mutable file access.
// Unknown/malformed/multipart ranges are ignored. Valid unsatisfied ranges return 416.
fn ranged<'a>(
    mut response: Response<'a>,
    request: &httparse::Request<'_, '_>,
) -> Result<Response<'a>, u16> {
    let mut ranges = request
        .headers
        .iter()
        .filter(|header| header.name.eq_ignore_ascii_case("range"));
    let Some(range) = ranges.next() else {
        return Ok(response);
    };
    if ranges.next().is_some() {
        return Err(400);
    }
    if !response.accept_ranges {
        return Err(400);
    }
    // HEAD ignores Range; without a matching validator, If-Range requires the full body.
    if request.method != Some("GET")
        || request
            .headers
            .iter()
            .any(|header| header.name.eq_ignore_ascii_case("if-range"))
    {
        return Ok(response);
    }
    let Ok(value) = std::str::from_utf8(range.value) else {
        return Ok(response);
    };
    let Some(value) = value.strip_prefix("bytes=") else {
        return Ok(response);
    };
    let Some((start, end)) = value.split_once('-') else {
        return Ok(response);
    };
    let number = |value: &str| {
        (!value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
            .then(|| value.parse::<usize>().ok())
            .flatten()
    };
    let length = response.body.len();
    let (start, end) = if start.is_empty() {
        let Some(suffix) = number(end) else {
            return Ok(response);
        };
        if suffix == 0 {
            (length, length)
        } else {
            (length.saturating_sub(suffix), length.saturating_sub(1))
        }
    } else {
        let Some(start) = number(start) else {
            return Ok(response);
        };
        let end = if end.is_empty() {
            length.saturating_sub(1)
        } else {
            let Some(end) = number(end) else {
                return Ok(response);
            };
            if end < start {
                return Ok(response);
            }
            end.min(length.saturating_sub(1))
        };
        (start, end)
    };
    if length == 0 || start >= length {
        response.status = 416;
        response.body = b"";
        response.content_range = Some(format!("bytes */{length}"));
    } else {
        response.status = 206;
        response.body = &response.body[start..=end];
        response.content_range = Some(format!("bytes {start}-{end}/{length}"));
    }
    Ok(response)
}
