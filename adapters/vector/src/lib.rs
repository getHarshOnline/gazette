//! Strict admission of inert retained SVG images without rewriting their bytes.

use std::collections::BTreeSet;

use gazette_core::{
    resource::{Artifact, Error, Limits, Recipe},
    source::TreeInput,
};
use gazette_ports::resource::ResourceCompiler;
use quick_xml::{Reader, XmlVersion, events::Event};

/// Exact parser and policy profile for static SVG resources.
pub const PROFILE: &str = "gazette.static-svg/2;viewport-layout;quick-xml=0.41.0;no-script-style-foreignobject-or-external-reference";

const SVG_NAMESPACE: &[u8] = b"http://www.w3.org/2000/svg";
const EVENTS: usize = 100_000;
const DEPTH: usize = 128;
const ATTRIBUTES: usize = 128;
const NAME_BYTES: usize = 256;
const VALUE_BYTES: usize = 64 << 10;

/// Deterministic exact-byte compiler for the bounded static SVG profile.
#[derive(Clone, Copy, Debug, Default)]
pub struct Compiler;

impl ResourceCompiler for Compiler {
    fn profile(&self) -> &'static str {
        PROFILE
    }

    fn compile(
        &self,
        source: &TreeInput<'_>,
        recipe: &Recipe,
        links: &gazette_core::resource::ModuleLinks,
        limits: Limits,
    ) -> Result<Artifact, Error> {
        let Recipe::SvgImage { entry } = recipe else {
            return Err(Error::Input {
                path: recipe.entry().clone(),
                reason: "static SVG compiler requires an SVG image recipe",
            });
        };
        if !links.is_empty() {
            return Err(Error::Input {
                path: entry.clone(),
                reason: "static SVG recipes do not accept module links",
            });
        }
        limits.validate_input(source.inventory())?;
        let files = source
            .inventory()
            .entries()
            .iter()
            .filter(|entry| matches!(entry.content, gazette_core::source::Content::File { .. }))
            .count();
        if files != 1 {
            return Err(Error::Input {
                path: entry.clone(),
                reason: "static SVG recipe requires exactly one retained file",
            });
        }
        let bytes = source.file(entry).ok_or_else(|| Error::Input {
            path: entry.clone(),
            reason: "static SVG entry must be the sole retained regular file",
        })?;
        if bytes.len() > limits.output_bytes {
            return Err(Error::Limit("static SVG output bytes"));
        }
        let svg = validate(bytes).map_err(|reason| Error::Input {
            path: entry.clone(),
            reason,
        })?;
        Ok(Artifact {
            input: source.inventory().identity(),
            bytes: bytes.to_vec(),
            dependencies: BTreeSet::from([entry.clone()]),
            references: BTreeSet::new(),
            deferred_references: Default::default(),
            image: None,
            svg,
            tool: PROFILE,
        })
    }
}

fn validate(bytes: &[u8]) -> Result<Option<gazette_core::resource::SvgLayout>, &'static str> {
    let text = std::str::from_utf8(bytes).map_err(|_| "static SVG must be UTF-8")?;
    if text
        .strip_prefix('\u{feff}')
        .unwrap_or(text)
        .trim()
        .is_empty()
    {
        return Err("static SVG must contain one SVG root");
    }
    let mut reader = Reader::from_str(text);
    reader.config_mut().check_end_names = true;
    reader.config_mut().trim_text(false);
    let mut depth = 0usize;
    let mut roots = 0usize;
    let mut ended = false;
    let mut geometry = None;
    for _ in 0..EVENTS {
        let event = reader
            .read_event()
            .map_err(|_| "static SVG XML is malformed")?;
        match event {
            Event::Start(tag) => {
                if ended {
                    return Err("static SVG requires exactly one root");
                }
                let sizing = inspect_tag(&tag, depth == 0)?;
                if depth == 0 {
                    geometry = sizing;
                }
                if depth == 0 {
                    roots += 1;
                }
                depth = depth
                    .checked_add(1)
                    .filter(|depth| *depth <= DEPTH)
                    .ok_or("static SVG nesting limit exceeded")?;
            }
            Event::Empty(tag) => {
                if ended {
                    return Err("static SVG requires exactly one root");
                }
                let sizing = inspect_tag(&tag, depth == 0)?;
                if depth == 0 {
                    geometry = sizing;
                }
                if depth == 0 {
                    roots += 1;
                    ended = true;
                }
            }
            Event::End(_) => {
                depth = depth.checked_sub(1).ok_or("static SVG XML is malformed")?;
                if depth == 0 {
                    ended = true;
                }
            }
            Event::Decl(_) | Event::DocType(_) | Event::PI(_) => {
                return Err(
                    "static SVG declarations, DTDs and processing instructions are unsupported",
                );
            }
            Event::Eof => {
                return if roots == 1 && depth == 0 && ended {
                    Ok(geometry)
                } else {
                    Err("static SVG requires exactly one complete SVG root")
                };
            }
            Event::Text(_) | Event::CData(_) | Event::Comment(_) | Event::GeneralRef(_) => (),
        }
    }
    Err("static SVG event limit exceeded")
}

fn inspect_tag(
    tag: &quick_xml::events::BytesStart<'_>,
    root: bool,
) -> Result<Option<gazette_core::resource::SvgLayout>, &'static str> {
    let name = tag.name();
    let name = name.as_ref();
    if name.is_empty() || name.len() > NAME_BYTES || name.contains(&b':') {
        return Err("static SVG element names must be bounded and unprefixed");
    }
    let local = ascii_lower(name);
    if root && local != b"svg" {
        return Err("static SVG root must be svg");
    }
    if matches!(local.as_slice(), b"script" | b"style" | b"foreignobject") {
        return Err("static SVG executable or foreign content is forbidden");
    }
    let mut namespace = false;
    let (mut width, mut height, mut view_box) = (None, None, None);
    for (index, attribute) in tag.attributes().enumerate() {
        if index >= ATTRIBUTES {
            return Err("static SVG attribute limit exceeded");
        }
        let attribute =
            attribute.map_err(|_| "static SVG attributes are malformed or duplicated")?;
        let key = attribute.key.as_ref();
        if key.is_empty() || key.len() > NAME_BYTES {
            return Err("static SVG attribute name is invalid");
        }
        let value = attribute
            .normalized_value(XmlVersion::default())
            .map_err(|_| "static SVG attribute value is malformed")?;
        if value.len() > VALUE_BYTES {
            return Err("static SVG attribute value limit exceeded");
        }
        if root {
            match key {
                b"width" => width = Some(value.to_string()),
                b"height" => height = Some(value.to_string()),
                b"viewBox" => view_box = Some(value.to_string()),
                _ => (),
            }
        }
        let lower_key = ascii_lower(key);
        let lower_value = value.to_ascii_lowercase();
        if root && lower_key == b"xmlns" {
            namespace = value.as_bytes() == SVG_NAMESPACE;
            continue;
        }
        if lower_key.contains(&b':') {
            return Err("static SVG prefixed attributes are unsupported");
        }
        if lower_key == b"style" || (lower_key.starts_with(b"on") && !lower_key.contains(&b':')) {
            return Err("static SVG inline style and event attributes are forbidden");
        }
        if lower_key == b"xmlns" || lower_key.starts_with(b"xmlns:") {
            return Err("static SVG admits only its exact default namespace");
        }
        let reference = lower_key == b"href";
        if reference && !local_fragment(&value) {
            return Err("static SVG references must be local fragments");
        }
        if (lower_value.contains("url(") && !local_url_fragment(lower_value.trim()))
            || lower_value.contains("://")
            || lower_value.starts_with("//")
            || lower_value.contains("data:")
            || lower_value.contains("javascript:")
        {
            return Err("static SVG external or embedded references are forbidden");
        }
    }
    if root && !namespace {
        return Err("static SVG root requires the exact SVG namespace");
    }
    Ok(gazette_core::resource::SvgLayout::from_root(
        width.as_deref(),
        height.as_deref(),
        view_box.as_deref(),
    ))
}

fn ascii_lower(value: &[u8]) -> Vec<u8> {
    value.iter().map(u8::to_ascii_lowercase).collect()
}

fn local_fragment(value: &str) -> bool {
    value.strip_prefix('#').is_some_and(|fragment| {
        !fragment.is_empty()
            && fragment.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':')
            })
    })
}

fn local_url_fragment(value: &str) -> bool {
    value
        .strip_prefix("url(")
        .and_then(|value| value.strip_suffix(')'))
        .is_some_and(local_fragment)
}
