//! Pure admission of the pinned `KaTeX` `MathML` profile, not a general `MathML` sanitizer.

use gazette_core::{
    markdown::Error,
    mathml::{self, Attribute, Validator},
};
use quick_xml::{Reader, events::Event};
use xml::{
    Encoding,
    namespace::{NS_XML_URI, NS_XMLNS_URI},
    reader::{ParserConfig, XmlEvent},
};

const BYTES: usize = 128 << 10;
const EVENTS: usize = 16_384;

/// Admit one complete generated fragment against its exact formula and display mode.
///
/// XML parsers own syntax, references and namespaces; only the adapter's finite
/// presentation vocabulary is accepted. Built-in XML escapes/numeric references
/// are decoded, but declarations, custom entities and external effects are refused.
/// No bytes are rewritten and no generator authenticity or browser proof is implied.
///
/// # Errors
/// Return `InvalidPlan` for unsafe, malformed or mismatched output, or `Limit` for
/// a named bound. No partial fragment becomes admitted on a later failure.
pub fn validate(mathml: &str, tex: &str, display: bool) -> Result<(), Error> {
    if mathml.len() > BYTES || tex.len() > mathml::MAX_ANNOTATION_BYTES {
        return Err(Error::Limit("math bytes"));
    }
    let opening = if display {
        "<math xmlns=\"http://www.w3.org/1998/Math/MathML\" display=\"block\">"
    } else {
        "<math xmlns=\"http://www.w3.org/1998/Math/MathML\">"
    };
    if tex.is_empty() || !mathml.starts_with(opening) || !mathml.ends_with("</math>") {
        return Err(Error::InvalidPlan);
    }
    lexical(mathml)?;
    let parser = ParserConfig::new()
        .ignore_comments(false)
        .ignore_root_level_whitespace(false)
        .coalesce_characters(false)
        .allow_multiple_root_elements(false)
        .override_encoding(Some(Encoding::Utf8))
        .max_entity_expansion_length(0)
        .max_entity_expansion_depth(0)
        .max_name_length(64)
        .max_attributes(mathml::MAX_ATTRIBUTES)
        .max_attribute_length(mathml::MAX_ATTRIBUTE_BYTES)
        .max_data_length(mathml::MAX_ATTRIBUTE_BYTES);
    let mut validator = Validator::new(Some(tex), Some(display));
    for (index, event) in parser
        .create_reader(mathml.as_bytes())
        .into_iter()
        .enumerate()
    {
        if index >= EVENTS {
            return Err(Error::Limit("math events"));
        }
        match event.map_err(|_| Error::InvalidPlan)? {
            XmlEvent::StartDocument { .. } => (),
            XmlEvent::StartElement {
                name,
                attributes,
                namespace,
            } => {
                if name.prefix.is_some()
                    || name.namespace.as_deref() != Some(mathml::NAMESPACE)
                    || namespace.0.iter().any(|(prefix, uri)| {
                        !((prefix.is_empty() && uri == mathml::NAMESPACE)
                            || (prefix == "xml" && uri == NS_XML_URI)
                            || (prefix == "xmlns" && uri == NS_XMLNS_URI))
                    })
                    || attributes.iter().any(|attribute| {
                        attribute.name.prefix.is_some() || attribute.name.namespace.is_some()
                    })
                {
                    return Err(Error::InvalidPlan);
                }
                let tag = name.local_name;
                let attributes = attributes
                    .iter()
                    .map(|attribute| Attribute {
                        name: attribute.name.local_name.as_str(),
                        value: attribute.value.as_str(),
                    })
                    .collect::<Vec<_>>();
                validator.start(&tag, &attributes).map_err(shared_error)?;
            }
            XmlEvent::EndElement { name } => {
                validator.end(&name.local_name).map_err(shared_error)?;
            }
            XmlEvent::Characters(value) | XmlEvent::Whitespace(value) => {
                validator.text(&value).map_err(shared_error)?;
            }
            XmlEvent::EndDocument => {
                return validator.finish().map_err(shared_error);
            }
            _ => return Err(Error::InvalidPlan),
        }
    }
    Err(Error::InvalidPlan)
}

fn shared_error(error: mathml::Error) -> Error {
    match error {
        mathml::Error::Invalid => Error::InvalidPlan,
        mathml::Error::Limit(resource) => Error::Limit(resource),
    }
}

// The semantic reader folds namespace declarations. Keep native lexical
// duplicate checks enabled and admit a declaration only at the exact root.
fn lexical(text: &str) -> Result<(), Error> {
    let mut reader = Reader::from_str(text);
    let mut depth = 0usize;
    for _ in 0..EVENTS {
        let event = reader.read_event().map_err(|_| Error::InvalidPlan)?;
        match &event {
            Event::Start(tag) | Event::Empty(tag) => {
                if depth >= mathml::MAX_DEPTH || tag.name().as_ref().len() > 64 {
                    return Err(Error::Limit("math structure"));
                }
                for (index, attribute) in tag.attributes().enumerate() {
                    if index >= mathml::MAX_ATTRIBUTES {
                        return Err(Error::Limit("math attributes"));
                    }
                    let attribute = attribute.map_err(|_| Error::InvalidPlan)?;
                    let name = attribute.key.as_ref();
                    if name.len() > 64 || attribute.value.len() > mathml::MAX_ATTRIBUTE_BYTES {
                        return Err(Error::Limit("math attribute bytes"));
                    }
                    if (name == b"xmlns" && depth != 0) || name.contains(&b':') {
                        return Err(Error::InvalidPlan);
                    }
                }
                if matches!(event, Event::Start(_)) {
                    depth += 1;
                }
            }
            Event::End(_) => depth = depth.checked_sub(1).ok_or(Error::InvalidPlan)?,
            Event::Text(_) | Event::GeneralRef(_) => (),
            Event::Eof => {
                return if depth == 0 {
                    Ok(())
                } else {
                    Err(Error::InvalidPlan)
                };
            }
            _ => return Err(Error::InvalidPlan),
        }
    }
    Err(Error::Limit("math events"))
}
