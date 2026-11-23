//! One XML transport and complete XML 1.0 check for both XML feed formats.

use std::convert::Infallible;

use gazette_core::render::LimitedWriter;
use quick_xml::{
    Writer,
    events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event},
};

use super::FeedError;

pub(super) const NAMESPACE: &str = "urn:gazette:syndication";

pub(super) struct Xml {
    writer: Writer<LimitedWriter<Vec<u8>>>,
    maximum: usize,
}

impl Xml {
    pub(super) fn new(maximum: usize) -> Result<Self, FeedError> {
        let mut xml = Self {
            writer: Writer::new(LimitedWriter::new(Vec::new(), maximum)),
            maximum,
        };
        xml.event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))?;
        Ok(xml)
    }

    fn event(&mut self, event: Event<'_>) -> Result<(), FeedError> {
        let result = self.writer.write_event(event);
        if self.writer.get_ref().exceeded() {
            return Err(FeedError::Limit);
        }
        result.map_err(|error| FeedError::Encoding(error.to_string()))
    }

    pub(super) fn start(
        &mut self,
        name: &str,
        attributes: &[(&str, &str)],
    ) -> Result<(), FeedError> {
        let mut tag = BytesStart::new(name);
        tag.extend_attributes(attributes.iter().copied());
        self.event(Event::Start(tag))
    }

    pub(super) fn end(&mut self, name: &str) -> Result<(), FeedError> {
        self.event(Event::End(BytesEnd::new(name)))
    }

    pub(super) fn element(
        &mut self,
        name: &str,
        attributes: &[(&str, &str)],
        text: &str,
    ) -> Result<(), FeedError> {
        if text.len() > self.maximum {
            return Err(FeedError::Limit);
        }
        self.start(name, attributes)?;
        // XML normalizes literal CR even inside otherwise escaped text. Emit a
        // character reference instead; escape each remaining chunk natively.
        for (index, chunk) in text.split('\r').enumerate() {
            if index != 0 {
                self.event(Event::Text(BytesText::from_escaped("&#13;")))?;
            }
            // Bound quick-xml's escaped temporary independently of the page's
            // size. UTF-8 boundaries and CR references preserve exact text.
            let mut rest = chunk;
            while !rest.is_empty() {
                let end = rest.floor_char_boundary(rest.len().min(8 * 1024));
                self.event(Event::Text(BytesText::new(&rest[..end])))?;
                rest = &rest[end..];
            }
        }
        self.end(name)
    }

    pub(super) fn metadata(&mut self, metadata: &impl serde::Serialize) -> Result<(), FeedError> {
        // Bounded lexical serialization only, not a second semantic projection.
        // The shared page role owns metadata and discovery filtering for every format.
        let bytes = super::encode(metadata, self.maximum)?;
        let text =
            std::str::from_utf8(&bytes).map_err(|error| FeedError::Encoding(error.to_string()))?;
        self.element("gazette:metadata", &[], text)
    }

    pub(super) fn finish(self) -> Result<Vec<u8>, FeedError> {
        let bytes = self.writer.into_inner().into_inner();
        // Escaping does not validate XML characters. Reuse the complete native
        // reader, including its lexical/namespace guard, before yielding bytes.
        let limits = crate::xml::Limits {
            input_bytes: self.maximum,
            events: bytes.len().saturating_add(2),
            value_bytes: self.maximum,
            decoded_bytes: self.maximum,
            ..crate::xml::Limits::default()
        };
        crate::xml::inspect(&bytes, limits, |_, _| Ok::<(), Infallible>(()))
            .map_err(|error| FeedError::Encoding(error.to_string()))?;
        Ok(bytes)
    }
}
