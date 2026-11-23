//! Decode retained XML into the closed inline SVG value, without native reads.
use gazette_core::markdown::{InlineSvg, SvgEvent};
use quick_xml::{
    Reader, XmlVersion,
    events::{BytesStart, Event},
};

pub(super) fn parse(bytes: &[u8], maximum: usize) -> Result<InlineSvg, &'static str> {
    if bytes.len() > maximum.min(1 << 20) {
        return Err("inline SVG input byte limit exceeded");
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "inline SVG requires UTF-8")?;
    let mut reader = Reader::from_str(text);
    reader.config_mut().check_end_names = true;
    let mut events = Vec::new();
    for _ in 0..100_000 {
        match reader
            .read_event()
            .map_err(|_| "inline SVG XML is malformed")?
        {
            Event::Start(tag) => events.push(start(&tag)?),
            Event::Empty(tag) => {
                let opening = start(&tag)?;
                let SvgEvent::Start { name, .. } = &opening else {
                    unreachable!()
                };
                let closing = SvgEvent::End(name.clone());
                events.push(opening);
                events.push(closing);
            }
            Event::End(tag) => events.push(SvgEvent::End(
                std::str::from_utf8(tag.name().as_ref())
                    .map_err(|_| "inline SVG name must be UTF-8")?
                    .to_owned(),
            )),
            Event::Text(value) => events.push(SvgEvent::Text(
                value
                    .xml10_content()
                    .map_err(|_| "inline SVG text is invalid")?
                    .into_owned(),
            )),
            Event::GeneralRef(value) => {
                let spelling = value.decode().map_err(|_| "inline SVG entity is invalid")?;
                let entity = format!("&{spelling};");
                events.push(SvgEvent::Text(
                    quick_xml::escape::unescape(&entity)
                        .map_err(|_| "inline SVG entity is unsupported")?
                        .into_owned(),
                ));
            }
            Event::Comment(_) => (),
            Event::Eof => return InlineSvg::new(events),
            _ => {
                return Err(
                    "inline SVG declarations, DTDs, processing instructions and CDATA are unsupported",
                );
            }
        }
    }
    Err("inline SVG event limit exceeded")
}

fn start(tag: &BytesStart<'_>) -> Result<SvgEvent, &'static str> {
    let name = std::str::from_utf8(tag.name().as_ref())
        .map_err(|_| "inline SVG name must be UTF-8")?
        .to_owned();
    let mut attributes = Vec::new();
    for attribute in tag.attributes() {
        if attributes.len() >= 64 {
            return Err("inline SVG attribute limit exceeded");
        }
        let attribute =
            attribute.map_err(|_| "inline SVG attributes are malformed or duplicated")?;
        let key = std::str::from_utf8(attribute.key.as_ref())
            .map_err(|_| "inline SVG attribute name must be UTF-8")?
            .to_owned();
        let value = attribute
            .normalized_value(XmlVersion::default())
            .map_err(|_| "inline SVG attribute value is invalid")?
            .into_owned();
        attributes.push((key, value));
    }
    Ok(SvgEvent::Start { name, attributes })
}
