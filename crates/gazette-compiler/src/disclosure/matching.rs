//! Bounded byte tokenization; native standard-library IP parsing owns address syntax.

use std::net::{IpAddr, SocketAddr};

#[derive(Clone, Copy)]
pub(super) enum PathKind {
    Posix,
    Windows,
}

impl PathKind {
    fn separator(self, byte: u8) -> bool {
        byte == b'/' || matches!(self, Self::Windows) && byte == b'\\'
    }
}

pub(super) fn path_kind(root: &str) -> Option<PathKind> {
    if root.is_empty() || root.chars().any(char::is_control) {
        return None;
    }
    let bytes = root.as_bytes();
    let (kind, rest, minimum) = if root.starts_with("\\\\") || root.starts_with("//") {
        (PathKind::Windows, &root[2..], 2)
    } else if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\')
    {
        (PathKind::Windows, &root[3..], 0)
    } else if let Some(rest) = root.strip_prefix('/') {
        (PathKind::Posix, rest, 0)
    } else {
        return None;
    };
    if rest.is_empty() {
        return (minimum == 0).then_some(kind);
    }
    let mut count = 0;
    for component in
        rest.split(|c: char| c == '/' || matches!(kind, PathKind::Windows) && c == '\\')
    {
        if component.is_empty() || matches!(component, "." | "..") {
            return None;
        }
        count += 1;
    }
    (count >= minimum).then_some(kind)
}

fn path_boundary(byte: u8) -> bool {
    byte.is_ascii_whitespace() || b"\"'`<>[](){};,".contains(&byte)
}

pub(super) fn path(bytes: &[u8], root: &str) -> bool {
    if root.is_empty() {
        return false;
    }
    // Declaration validation already scanned components once, outside payload matching.
    let kind = if root.starts_with('/') && !root.starts_with("//") {
        PathKind::Posix
    } else {
        PathKind::Windows
    };
    let root = root.as_bytes();
    bytes
        .windows(root.len())
        .enumerate()
        .any(|(start, candidate)| {
            if candidate != root {
                return false;
            }
            let before = start == 0
                || path_boundary(bytes[start - 1])
                || matches!(bytes[start - 1], b':' | b'=')
                || start >= 7 && &bytes[start - 7..start] == b"file://";
            let end = start + root.len();
            before
                && (kind.separator(root[root.len() - 1])
                    || end == bytes.len()
                    || kind.separator(bytes[end])
                    || path_boundary(bytes[end]))
        })
}

pub(super) fn hostname(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 253
        && name.is_ascii()
        && name.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label.as_bytes()[0].is_ascii_alphanumeric()
                && label.as_bytes()[label.len() - 1].is_ascii_alphanumeric()
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

fn host_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_') || !byte.is_ascii()
}

pub(super) fn host(bytes: &[u8], name: &str) -> bool {
    bytes.split(|byte| !host_byte(*byte)).any(|candidate| {
        let candidate = candidate.strip_suffix(b".").unwrap_or(candidate);
        candidate.eq_ignore_ascii_case(name.as_bytes())
    })
}

fn v4_mask(prefix: u8) -> Option<u32> {
    match prefix {
        0 => Some(0),
        1..=32 => Some(u32::MAX << (32 - prefix)),
        _ => None,
    }
}

fn v6_mask(prefix: u8) -> Option<u128> {
    match prefix {
        0 => Some(0),
        1..=128 => Some(u128::MAX << (128 - prefix)),
        _ => None,
    }
}

pub(super) fn network(address: IpAddr, prefix: u8) -> bool {
    match address {
        IpAddr::V4(address) => {
            v4_mask(prefix).is_some_and(|mask| u32::from(address) & mask == u32::from(address))
        }
        IpAddr::V6(address) => {
            v6_mask(prefix).is_some_and(|mask| u128::from(address) & mask == u128::from(address))
        }
    }
}

fn member(network: IpAddr, prefix: u8, candidate: IpAddr) -> bool {
    match (network, candidate) {
        (IpAddr::V4(network), IpAddr::V4(candidate)) => {
            v4_mask(prefix).is_some_and(|mask| u32::from(candidate) & mask == u32::from(network))
        }
        (IpAddr::V6(network), IpAddr::V6(candidate)) => {
            v6_mask(prefix).is_some_and(|mask| u128::from(candidate) & mask == u128::from(network))
        }
        _ => false,
    }
}

fn ip_byte(byte: u8) -> bool {
    host_byte(byte) || matches!(byte, b':' | b'%')
}

// Conservative textual ceiling; brackets delimit a native IPv6 address token.
const IP_BYTES: usize = 53;

pub(super) fn ip(bytes: &[u8], network: IpAddr, prefix: u8) -> bool {
    bytes.split(|byte| !ip_byte(*byte)).any(|candidate| {
        if candidate.is_empty() || candidate.len() > IP_BYTES || candidate.contains(&b'%') {
            return false;
        }
        let Ok(text) = std::str::from_utf8(candidate) else {
            return false;
        };
        let address = text
            .parse::<IpAddr>()
            .ok()
            .or_else(|| text.parse::<SocketAddr>().ok().map(|socket| socket.ip()));
        address.is_some_and(|address| member(network, prefix, address))
    })
}
