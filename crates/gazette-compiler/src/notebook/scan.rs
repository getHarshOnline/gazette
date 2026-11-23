//! Pre-public scan of a retained notebook before its exact bytes become a download.
//!
//! The authored page binding calls this only when a page selects a notebook download.
//! It reads the validated JSON tree: notebook metadata, every cell source, metadata,
//! attachment and stored output, tracebacks included. A finding names the field and
//! its class, never the matched text. Passing is not disclosure or rights approval.

use std::{borrow::Cow, net::IpAddr};

use serde::Serialize;
use serde_json::Value;

use super::ParsedNotebook;

/// Kind of private material that refuses a notebook download.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScanClass {
    /// Absolute local path, or kernel or execution metadata holding a path.
    LocalPath,
    /// Email address, `user@host` form or username metadata.
    Identity,
    /// Key prefix, private-key block or long high-entropy credential assignment.
    Secret,
    /// `file:` URL, loopback or private-network host.
    PrivateLink,
}

impl ScanClass {
    /// Stable diagnostic class.
    pub fn code(self) -> &'static str {
        match self {
            Self::LocalPath => "notebook_download_local_path",
            Self::Identity => "notebook_download_identity",
            Self::Secret => "notebook_download_secret",
            Self::PrivateLink => "notebook_download_private_link",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::LocalPath => "an absolute local path",
            Self::Identity => "a username or email address",
            Self::Secret => "a token or secret",
            Self::PrivateLink => "a link into private material",
        }
    }
}

/// The first field that refuses a download. It never carries the matched value.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct ScanFinding {
    /// Class of the matched material.
    pub class: ScanClass,
    /// Zero-based cell, absent for notebook metadata.
    pub cell: Option<usize>,
    /// Field path such as `cells[3].outputs[0].traceback`; object keys that could
    /// themselves hold the matched value appear as ordinals.
    pub field: String,
}

impl ScanFinding {
    /// Stable diagnostic class.
    pub fn code(&self) -> &'static str {
        self.class.code()
    }
}

impl std::fmt::Display for ScanFinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} holds {}; remove it from the notebook or leave its download unselected",
            self.field,
            self.class.label()
        )
    }
}

impl ParsedNotebook<'_> {
    /// Scan metadata, cell sources, attachments and stored outputs for private material.
    ///
    /// Returns the first finding in document order, or none when the notebook passes.
    pub fn scan_for_download(&self) -> Option<ScanFinding> {
        let value = self.value();
        if let Some(metadata) = value.get("metadata") {
            let mut path = String::from("metadata");
            if let Some(class) = walk(metadata, &mut path, None) {
                return Some(ScanFinding {
                    class,
                    cell: None,
                    field: path,
                });
            }
        }
        for (index, cell) in value
            .get("cells")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            let mut path = format!("cells[{index}]");
            if let Some(class) = walk(cell, &mut path, None) {
                return Some(ScanFinding {
                    class,
                    cell: Some(index),
                    field: path,
                });
            }
        }
        None
    }
}

/// Walk one subtree, leaving `path` at the refusing field when a class is returned.
fn walk(value: &Value, path: &mut String, key: Option<&str>) -> Option<ScanClass> {
    match value {
        Value::String(text) => key
            .and_then(|key| keyed(key, text))
            .or_else(|| classify(text)),
        Value::Array(items) if items.iter().all(Value::is_string) => {
            // Multiline sources and tracebacks are one field; report it, not a line.
            items.iter().find_map(|item| {
                item.as_str().and_then(|text| {
                    key.and_then(|key| keyed(key, text))
                        .or_else(|| classify(text))
                })
            })
        }
        Value::Array(items) => items.iter().enumerate().find_map(|(index, item)| {
            let length = path.len();
            path.push_str(&format!("[{index}]"));
            let found = walk(item, path, key);
            if found.is_none() {
                path.truncate(length);
            }
            found
        }),
        Value::Object(members) => members
            .iter()
            .enumerate()
            .find_map(|(index, (name, item))| {
                let length = path.len();
                if let Some(class) = classify(name) {
                    path.push_str(&format!("{{#{index}}}"));
                    return Some(class);
                }
                if plain_key(name) {
                    path.push('.');
                    path.push_str(name);
                } else {
                    path.push_str(&format!("{{#{index}}}"));
                }
                let found = walk(item, path, Some(name));
                if found.is_none() {
                    path.truncate(length);
                }
                found
            }),
        _ => None,
    }
}

fn plain_key(name: &str) -> bool {
    name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-./+".contains(&byte))
}

/// Metadata keys whose values are private by role, whatever their shape.
fn keyed(key: &str, text: &str) -> Option<ScanClass> {
    let key = key.to_ascii_lowercase();
    if matches!(
        key.as_str(),
        "path"
            | "cwd"
            | "executable"
            | "interpreter"
            | "interpreter_path"
            | "argv"
            | "env"
            | "connection_file"
            | "resource_dir"
            | "root_dir"
            | "notebook_dir"
            | "working_directory"
    ) && (text.contains('/') || text.contains('\\'))
    {
        return Some(ScanClass::LocalPath);
    }
    if matches!(
        key.as_str(),
        "user" | "username" | "user_name" | "login" | "user_id" | "userid"
    ) && !text.trim().is_empty()
    {
        return Some(ScanClass::Identity);
    }
    None
}

/// Classify free text in a fixed order: secrets, links, paths, then identities.
fn classify(text: &str) -> Option<ScanClass> {
    let text = strip_terminal_escapes(text);
    let text = text.as_ref();
    if secret(text) {
        Some(ScanClass::Secret)
    } else if private_link(text) {
        Some(ScanClass::PrivateLink)
    } else if local_path(text) {
        Some(ScanClass::LocalPath)
    } else if identity(text) {
        Some(ScanClass::Identity)
    } else {
        None
    }
}

/// Tracebacks colour paths with terminal escapes; remove them so boundaries hold.
fn strip_terminal_escapes(text: &str) -> Cow<'_, str> {
    if !text.contains('\u{1b}') {
        return Cow::Borrowed(text);
    }
    let mut plain = String::with_capacity(text.len());
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\u{1b}' {
            if characters.peek() == Some(&'[') {
                characters.next();
                for next in characters.by_ref() {
                    if next.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        plain.push(character);
    }
    Cow::Owned(plain)
}

fn token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'
}

/// Maximal runs of ASCII letters, digits, `_` and `-`.
fn tokens(text: &str) -> impl Iterator<Item = &str> {
    text.split(|character: char| !(character.is_ascii() && token_byte(character as u8)))
        .filter(|token| !token.is_empty())
}

fn mixed(value: &str) -> bool {
    value.bytes().any(|byte| byte.is_ascii_digit())
        && value.bytes().any(|byte| byte.is_ascii_alphabetic())
}

fn secret(text: &str) -> bool {
    if text.contains("-----BEGIN") && text.contains("PRIVATE KEY-----") {
        return true;
    }
    let prefixed = tokens(text).any(|token| {
        let tail = |prefix: &str, minimum: usize| {
            token
                .strip_prefix(prefix)
                .is_some_and(|tail| tail.len() >= minimum && mixed(tail))
        };
        let aws = (token.starts_with("AKIA") || token.starts_with("ASIA"))
            && token.len() == 20
            && token[4..]
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit());
        aws || tail("sk-", 20)
            || ["sk_live_", "sk_test_", "rk_live_"]
                .iter()
                .any(|prefix| tail(prefix, 16))
            || ["ghp_", "gho_", "ghu_", "ghs_", "ghr_"]
                .iter()
                .any(|prefix| tail(prefix, 30))
            || tail("github_pat_", 20)
            || ["xoxa-", "xoxb-", "xoxe-", "xoxp-", "xoxr-", "xoxs-"]
                .iter()
                .any(|prefix| tail(prefix, 10))
            || token.starts_with("AIza") && token.len() == 39
    });
    prefixed || assignment(text)
}

/// `name = value` or `name: value` where the name means a credential and the value
/// is long, mixed and high in entropy. Calls such as `token = read_token()` pass.
fn assignment(text: &str) -> bool {
    let bytes = text.as_bytes();
    for (at, byte) in bytes.iter().enumerate() {
        if !matches!(byte, b'=' | b':') {
            continue;
        }
        let mut end = at;
        while end > 0 && bytes[end - 1] == b' ' {
            end -= 1;
        }
        if end > 0 && matches!(bytes[end - 1], b'"' | b'\'') {
            end -= 1;
        }
        let mut start = end;
        while start > 0 && (token_byte(bytes[start - 1]) || bytes[start - 1] == b'.') {
            start -= 1;
        }
        if start == end || !credential_name(&text[start..end]) {
            continue;
        }
        let mut begin = at + 1;
        while begin < bytes.len() && matches!(bytes[begin], b' ' | b'=') {
            begin += 1;
        }
        if begin < bytes.len() && matches!(bytes[begin], b'"' | b'\'' | b'`') {
            begin += 1;
        }
        let mut finish = begin;
        while finish < bytes.len()
            && (token_byte(bytes[finish]) || b"+/=.~".contains(&bytes[finish]))
        {
            finish += 1;
        }
        let value = &text[begin..finish];
        if bytes.get(finish) == Some(&b'(') || value.len() < 16 || !mixed(value) {
            continue;
        }
        if entropy(value) >= 3.5 {
            return true;
        }
    }
    false
}

fn credential_name(name: &str) -> bool {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut previous_lower = false;
    for character in name.chars() {
        if matches!(character, '_' | '-' | '.') {
            parts.push(std::mem::take(&mut current));
            previous_lower = false;
            continue;
        }
        if character.is_ascii_uppercase() && previous_lower {
            parts.push(std::mem::take(&mut current));
        }
        previous_lower = character.is_ascii_lowercase();
        current.push(character.to_ascii_lowercase());
    }
    parts.push(current);
    parts.iter().any(|part| {
        matches!(
            part.as_str(),
            "token" | "secret" | "key" | "password" | "passwd" | "apikey" | "pwd"
        )
    })
}

fn entropy(value: &str) -> f64 {
    let mut counts = [0usize; 256];
    for byte in value.bytes() {
        counts[usize::from(byte)] += 1;
    }
    let length = value.len() as f64;
    counts
        .iter()
        .filter(|&&count| count > 0)
        .map(|&count| {
            let share = count as f64 / length;
            -share * share.log2()
        })
        .sum()
}

fn boundary(previous: Option<u8>) -> bool {
    previous.is_none_or(|byte| byte.is_ascii_whitespace() || b"\"'`([{<=,;:|>".contains(&byte))
}

fn private_link(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    if lower.match_indices("file:/").any(|(at, _)| {
        at.checked_sub(1)
            .is_none_or(|index| !bytes[index].is_ascii_alphanumeric())
    }) {
        return true;
    }
    for (at, _) in lower.match_indices("://") {
        let mut start = at;
        while start > 0
            && (bytes[start - 1].is_ascii_alphanumeric() || b"+.-".contains(&bytes[start - 1]))
        {
            start -= 1;
        }
        let end = lower[at..]
            .find(|character: char| character.is_whitespace() || "\"'`<>)]}".contains(character))
            .map_or(lower.len(), |offset| at + offset);
        if start < at
            && let Ok(url) = url::Url::parse(&lower[start..end])
            && url.host_str().is_some_and(private_host)
        {
            return true;
        }
    }
    // A bare development server address such as `localhost:8888`.
    lower.match_indices("localhost:").any(|(at, _)| {
        boundary(at.checked_sub(1).map(|index| bytes[index]))
            && bytes
                .get(at + "localhost:".len())
                .is_some_and(u8::is_ascii_digit)
    })
}

fn private_host(host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(address) = host.parse::<IpAddr>() {
        return match address {
            IpAddr::V4(address) => {
                address.is_loopback()
                    || address.is_private()
                    || address.is_link_local()
                    || address.is_unspecified()
            }
            IpAddr::V6(address) => {
                address.is_loopback()
                    || address.is_unspecified()
                    || (address.segments()[0] & 0xfe00) == 0xfc00
                    || (address.segments()[0] & 0xffc0) == 0xfe80
            }
        };
    }
    host == "localhost"
        || [
            ".localhost",
            ".local",
            ".internal",
            ".lan",
            ".home.arpa",
            ".intranet",
        ]
        .iter()
        .any(|suffix| host.ends_with(suffix))
}

const ROOTS: &[&str] = &[
    "Users",
    "home",
    "root",
    "private",
    "tmp",
    "var",
    "opt",
    "usr",
    "mnt",
    "media",
    "Volumes",
    "srv",
    "Library",
    "System",
    "Applications",
    "nix",
    "run",
    "workspace",
    "workspaces",
    "content",
    "scratch",
    "data",
];

fn local_path(text: &str) -> bool {
    let bytes = text.as_bytes();
    for (at, byte) in bytes.iter().enumerate() {
        let previous = at.checked_sub(1).map(|index| bytes[index]);
        match byte {
            b'/' if boundary(previous) => {
                let rest = &text[at + 1..];
                if let Some((root, after)) = rest.split_once('/')
                    && ROOTS.contains(&root)
                    && after.bytes().next().is_some_and(token_byte)
                {
                    return true;
                }
            }
            b'\\'
                if boundary(previous)
                    && bytes.get(at + 1) == Some(&b'\\')
                    && bytes.get(at + 2).is_some_and(u8::is_ascii_alphanumeric) =>
            {
                return true;
            }
            letter if letter.is_ascii_alphabetic() && boundary(previous) => {
                if bytes.get(at + 1) == Some(&b':')
                    && matches!(bytes.get(at + 2), Some(b'\\' | b'/'))
                    && bytes
                        .get(at + 3)
                        .is_some_and(|next| next.is_ascii_alphanumeric() || *next == b'\\')
                {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

fn identity(text: &str) -> bool {
    let bytes = text.as_bytes();
    for (at, _) in text.match_indices('@') {
        let mut start = at;
        while start > 0
            && (bytes[start - 1].is_ascii_alphanumeric() || b"._%+-".contains(&bytes[start - 1]))
        {
            start -= 1;
        }
        let local = &text[start..at];
        if local.is_empty()
            || local == "git"
            || !boundary(start.checked_sub(1).map(|index| bytes[index]))
        {
            continue;
        }
        let mut end = at + 1;
        while end < bytes.len()
            && (bytes[end].is_ascii_alphanumeric() || b".-".contains(&bytes[end]))
        {
            end += 1;
        }
        let domain = text[at + 1..end].trim_end_matches('.');
        if domain.is_empty() {
            continue;
        }
        let email = domain.rsplit_once('.').is_some_and(|(name, top)| {
            !name.is_empty() && top.len() >= 2 && top.bytes().all(|byte| byte.is_ascii_alphabetic())
        });
        // A shell prompt such as `someone@machine:~$` names a local account.
        let prompt = bytes.get(at + 1 + domain.len()) == Some(&b':')
            && domain
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-');
        if email || prompt {
            return true;
        }
    }
    false
}
