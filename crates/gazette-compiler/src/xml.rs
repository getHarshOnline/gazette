//! Bounded XML inspection over retained bytes; no DTD, native reads or execution.

use std::fmt;

use quick_xml::{Reader, events::Event};
use xml::{
    Encoding,
    common::XmlVersion,
    namespace::{NS_XML_URI, NS_XMLNS_URI},
    reader::{ParserConfig, XmlEvent},
};

/// Exact lexical guard and semantic parser profile; not feed/SVG schema approval.
pub const PROFILE: &str = "gazette-xml-probe/2;quick-xml=0.41.0;xml=1.4.0";

const SVG: &str = "http://www.w3.org/2000/svg";
const XHTML: &str = "http://www.w3.org/1999/xhtml";

/// Namespace-resolved structure requiring more than generic XML text decoding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmbeddedElement {
    /// SVG `script`, irrespective of type, references or body presence.
    SvgScript,
    /// SVG `style`; XML decoding does not inspect its embedded stylesheet.
    SvgStyle,
    /// SVG `foreignObject`; child content is not qualified by XML well-formedness.
    SvgForeignObject,
    /// Any element in the XHTML namespace, not an HTML parser result.
    Xhtml,
}

/// Per-file bounds, independent of the caller's aggregate comparison budget.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Original byte length, checked before either parser allocates.
    pub input_bytes: usize,
    /// Maximum events in each parser pass, including document boundaries.
    pub events: usize,
    /// Maximum element nesting, with the root at depth one.
    pub depth: usize,
    /// Maximum attributes per element, including namespace declarations.
    pub attributes: usize,
    /// Maximum raw attribute/element name bytes; also a native parser bound.
    pub name_bytes: usize,
    /// Maximum emitted value bytes; raw attributes/CDATA and native buffers are bounded too.
    pub value_bytes: usize,
    /// Aggregate string bytes delivered to the speculative consumer.
    pub decoded_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            input_bytes: 1 << 20,
            events: 100_000,
            depth: 128,
            attributes: 128,
            name_bytes: 1024,
            value_bytes: 64 << 10,
            decoded_bytes: 2 << 20,
        }
    }
}

/// The XML representation inspected, without interpreting an embedded language.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextKind {
    /// Adjacent text and CDATA, with native reference and newline normalization.
    Text,
    /// One decoded, normalized ordinary attribute value.
    Attribute,
    /// Decoded unnamespaced SVG attribute with a case-sensitive `on` prefix.
    /// This conservative prefix is not an exhaustive browser-handler inventory.
    EventAttribute,
    /// Decoded unnamespaced SVG `style` attribute, still requiring CSS inspection.
    StyleAttribute,
    /// Empty-valued marker emitted once at the native start of this structure,
    /// including empty elements. Subsequent text/CDATA channels remain unchanged.
    EmbeddedElement(EmbeddedElement),
    /// One explicitly declared namespace URI, including unused declarations.
    Namespace,
    /// Comment text; reference-looking spelling remains literal.
    Comment,
    /// Processing instruction target or data; never executed or fetched.
    ProcessingInstruction,
}

/// Inspection failed; previously delivered observations are not accepted evidence.
#[derive(Debug)]
pub enum Error<E> {
    /// A configured bound was reached before complete inspection.
    Limit(&'static str),
    /// Only UTF-8 input is supported; no lossy decoding is performed.
    Utf8,
    /// Native parsing or namespace validation failed, possibly at a parser limit.
    /// Parser payload-bearing error messages are deliberately not exposed.
    Malformed,
    /// DTDs, non-1.0 declarations or non-UTF-8 encodings are unsupported.
    Unsupported,
    /// The consumer stopped inspection immediately.
    Consumer(E),
}

impl<E: fmt::Display> fmt::Display for Error<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Limit(resource) => write!(f, "XML {resource} ceiling exceeded"),
            Self::Utf8 => f.write_str("XML candidate is not UTF-8"),
            Self::Malformed => f.write_str("XML syntax, namespace or parser bound rejected"),
            Self::Unsupported => f.write_str("XML profile is unsupported"),
            Self::Consumer(error) => error.fmt(f),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for Error<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Consumer(error) => Some(error),
            _ => None,
        }
    }
}

/// Inspect one complete XML 1.0 document without changing its retained bytes.
///
/// A lexical guard rejects DTDs and duplicate attributes before the semantic
/// parser can discard namespace declarations. Only that second parser supplies
/// decoded meaning. Explicit declarations are retained as lexical coordinates
/// so unused URIs are inspected without inventing implicit namespace findings.
/// Adjacent text/CDATA joins, but text across elements, comments or PIs does not.
/// No schema, `XInclude`, stylesheet, SVG script, URL or embedded format is resolved.
/// Known SVG/XHTML contexts have typed markers; namesakes in other namespaces do not.
///
/// Consumer calls are speculative until whole-input success. Discard all consumer
/// effects on any later error. Bounds constrain input and parser work, not a hard
/// CPU/allocation sandbox; native string checks can overshoot by a token/reference.
///
/// # Errors
/// Rejects malformed/unsupported input, resource exhaustion and consumer failure.
pub fn inspect<E>(
    bytes: &[u8],
    limits: Limits,
    mut consume: impl FnMut(TextKind, &str) -> Result<(), E>,
) -> Result<(), Error<E>> {
    if bytes.len() > limits.input_bytes {
        return Err(Error::Limit("input bytes"));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| Error::Utf8)?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let declarations = preflight(text, limits)?;
    let mut declarations = declarations.into_iter();
    let config = ParserConfig::new()
        .trim_whitespace(false)
        .whitespace_to_characters(true)
        .cdata_to_characters(true)
        .coalesce_characters(true)
        .ignore_comments(false)
        .ignore_root_level_whitespace(false)
        .ignore_end_of_stream(false)
        .allow_multiple_root_elements(false)
        .replace_unknown_entity_references(false)
        .ignore_invalid_encoding_declarations(false)
        .override_encoding(Some(Encoding::Utf8))
        .max_entity_expansion_length(0)
        .max_entity_expansion_depth(0)
        .max_name_length(limits.name_bytes)
        .max_attributes(limits.attributes)
        .max_attribute_length(limits.value_bytes)
        .max_data_length(limits.value_bytes);
    let mut reader = config.create_reader(text.as_bytes());
    let mut remaining = limits.decoded_bytes;
    let mut emit = |kind, value: &str| {
        if value.len() > limits.value_bytes {
            return Err(Error::Limit("value bytes"));
        }
        remaining = remaining
            .checked_sub(value.len())
            .ok_or(Error::Limit("decoded bytes"))?;
        consume(kind, value).map_err(Error::Consumer)
    };
    for _ in 0..limits.events {
        match reader.next().map_err(|_| Error::Malformed)? {
            XmlEvent::StartDocument {
                version, encoding, ..
            } => {
                if version != XmlVersion::Version10 || !encoding.eq_ignore_ascii_case("UTF-8") {
                    return Err(Error::Unsupported);
                }
            }
            XmlEvent::StartElement {
                name,
                attributes,
                namespace,
            } => {
                for prefix in declarations.next().ok_or(Error::Malformed)? {
                    let uri = namespace.get(&prefix).ok_or(Error::Malformed)?;
                    if uri == NS_XMLNS_URI || (uri == NS_XML_URI && prefix != "xml") {
                        return Err(Error::Malformed);
                    }
                    emit(TextKind::Namespace, uri)?;
                }
                let svg = name.namespace.as_deref() == Some(SVG);
                let embedded = if name.namespace.as_deref() == Some(SVG) {
                    match name.local_name.as_str() {
                        "script" => Some(EmbeddedElement::SvgScript),
                        "style" => Some(EmbeddedElement::SvgStyle),
                        "foreignObject" => Some(EmbeddedElement::SvgForeignObject),
                        _ => None,
                    }
                } else if name.namespace.as_deref() == Some(XHTML) {
                    Some(EmbeddedElement::Xhtml)
                } else {
                    None
                };
                if let Some(embedded) = embedded {
                    emit(TextKind::EmbeddedElement(embedded), "")?;
                }
                for attribute in attributes {
                    let kind = if svg && attribute.name.namespace.is_none() {
                        match attribute.name.local_name.as_str() {
                            "style" => TextKind::StyleAttribute,
                            name if name.starts_with("on") => TextKind::EventAttribute,
                            _ => TextKind::Attribute,
                        }
                    } else {
                        TextKind::Attribute
                    };
                    emit(kind, &attribute.value)?;
                }
            }
            XmlEvent::Characters(value) | XmlEvent::CData(value) | XmlEvent::Whitespace(value) => {
                emit(TextKind::Text, &value)?;
            }
            XmlEvent::Comment(value) => emit(TextKind::Comment, &value)?,
            XmlEvent::ProcessingInstruction { name, data } => {
                emit(TextKind::ProcessingInstruction, &name)?;
                if let Some(data) = data {
                    emit(TextKind::ProcessingInstruction, &data)?;
                }
            }
            XmlEvent::EndElement { .. } => (),
            XmlEvent::Doctype { .. } => return Err(Error::Unsupported),
            XmlEvent::EndDocument => {
                return if declarations.next().is_none() {
                    Ok(())
                } else {
                    Err(Error::Malformed)
                };
            }
        }
    }
    Err(Error::Limit("events"))
}

// Names only, not another decoded model. One row per opening/empty element.
fn preflight<E>(text: &str, limits: Limits) -> Result<Vec<Vec<String>>, Error<E>> {
    let mut reader = Reader::from_str(text);
    reader.config_mut().enable_all_checks(true);
    let mut depth = 0usize;
    let mut declarations = Vec::new();
    for _ in 0..limits.events {
        let event = reader.read_event().map_err(|_| Error::Malformed)?;
        match &event {
            Event::Start(tag) | Event::Empty(tag) => {
                if depth >= limits.depth {
                    return Err(Error::Limit("depth"));
                }
                if tag.name().as_ref().len() > limits.name_bytes {
                    return Err(Error::Limit("name bytes"));
                }
                let mut prefixes = Vec::new();
                for (index, attribute) in tag.attributes().enumerate() {
                    if index >= limits.attributes {
                        return Err(Error::Limit("attributes"));
                    }
                    // Keep native duplicate checks enabled, including xmlns attributes.
                    let attribute = attribute.map_err(|_| Error::Malformed)?;
                    let name = attribute.key.as_ref();
                    if name.len() > limits.name_bytes {
                        return Err(Error::Limit("name bytes"));
                    }
                    if attribute.value.len() > limits.value_bytes {
                        return Err(Error::Limit("value bytes"));
                    }
                    let prefix = if name == b"xmlns" {
                        Some(b"".as_slice())
                    } else {
                        name.strip_prefix(b"xmlns:")
                    };
                    if let Some(prefix) = prefix {
                        prefixes.push(
                            std::str::from_utf8(prefix)
                                .map_err(|_| Error::Utf8)?
                                .to_owned(),
                        );
                    }
                }
                declarations.push(prefixes);
                if matches!(event, Event::Start(_)) {
                    depth += 1;
                }
            }
            Event::End(_) => depth = depth.checked_sub(1).ok_or(Error::Malformed)?,
            Event::Decl(declaration) => {
                if declaration
                    .version()
                    .map_err(|_| Error::Malformed)?
                    .as_ref()
                    != b"1.0"
                {
                    return Err(Error::Unsupported);
                }
                if let Some(encoding) = declaration.encoding()
                    && !encoding
                        .map_err(|_| Error::Malformed)?
                        .eq_ignore_ascii_case(b"UTF-8")
                {
                    return Err(Error::Unsupported);
                }
            }
            Event::DocType(_) => return Err(Error::Unsupported),
            Event::CData(value) if value.len() > limits.value_bytes => {
                return Err(Error::Limit("value bytes"));
            }
            Event::Eof => {
                return if depth == 0 {
                    Ok(declarations)
                } else {
                    Err(Error::Malformed)
                };
            }
            _ => (),
        }
    }
    Err(Error::Limit("events"))
}
