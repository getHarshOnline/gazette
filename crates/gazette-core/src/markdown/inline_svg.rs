//! A small static SVG vocabulary safe to serialize inside an HTML document.

use std::collections::BTreeSet;

use crate::source::Sha256Digest;

/// Decoded XML events supplied by a parser, before inline admission.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SvgEvent {
    /// An element and its decoded attributes. Empty elements use a matching end event.
    Start {
        /// Exact unprefixed SVG element name.
        name: String,
        /// Decoded names and values, including the root namespace declaration.
        attributes: Vec<(String, String)>,
    },
    /// Exact matching element name.
    End(String),
    /// Decoded character data, never markup.
    Text(String),
}

/// Admitted static shapes and live text. Its fields cannot carry unchecked markup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InlineSvg {
    events: Vec<SvgEvent>,
    bytes: usize,
}

impl InlineSvg {
    /// Admit decoded SVG with one root, closed names, local accessibility references
    /// and bounded structure. Styling stays in presentation attributes.
    ///
    /// # Errors
    /// Reject unsupported elements/attributes, malformed nesting, duplicate IDs,
    /// unresolved references and input above the fixed one-MiB profile ceiling.
    pub fn new(events: Vec<SvgEvent>) -> Result<Self, &'static str> {
        if events.len() > 100_000 {
            return Err("inline SVG event limit exceeded");
        }
        let mut stack = Vec::new();
        let mut ids = BTreeSet::new();
        let mut references = Vec::new();
        let mut roots = 0;
        let mut bytes = 0usize;
        for event in &events {
            match event {
                SvgEvent::Start { name, attributes } => {
                    bytes = bytes.saturating_add(name.len());
                    if !element(name) || stack.len() >= 64 || attributes.len() > 64 {
                        return Err("inline SVG element or structure is unsupported");
                    }
                    if stack.is_empty() {
                        roots += 1;
                        if name != "svg" || roots != 1 {
                            return Err("inline SVG requires one svg root");
                        }
                    } else if name == "svg"
                        || stack
                            .last()
                            .is_some_and(|parent| matches!(*parent, "title" | "desc"))
                    {
                        return Err("inline SVG child element is unsupported");
                    }
                    let mut names = BTreeSet::new();
                    let mut namespace = false;
                    for (key, value) in attributes {
                        bytes = bytes.saturating_add(key.len()).saturating_add(value.len());
                        if value.len() > 64 << 10
                            || !names.insert(key.as_str())
                            || value
                                .chars()
                                .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
                        {
                            return Err("inline SVG attribute is invalid or duplicated");
                        }
                        match key.as_str() {
                            "xmlns"
                                if stack.is_empty() && value == "http://www.w3.org/2000/svg" =>
                            {
                                namespace = true
                            }
                            "id" => {
                                if !identifier(value) || !ids.insert(value.as_str()) {
                                    return Err("inline SVG ID is invalid or duplicated");
                                }
                            }
                            "aria-labelledby" | "aria-describedby" => {
                                if value.trim().is_empty() {
                                    return Err("inline SVG reference is empty");
                                }
                                for id in value.split_ascii_whitespace() {
                                    if !identifier(id) {
                                        return Err("inline SVG reference is invalid");
                                    }
                                    references.push(id);
                                }
                            }
                            "role" if value == "img" => (),
                            key if attribute(key) && presentation_value(value) => (),
                            _ => return Err("inline SVG attribute or value is unsupported"),
                        }
                    }
                    if stack.is_empty() && !namespace {
                        return Err("inline SVG root requires its exact namespace");
                    }
                    stack.push(name.as_str());
                }
                SvgEvent::End(name) => {
                    bytes = bytes.saturating_add(name.len());
                    if stack.pop() != Some(name.as_str()) {
                        return Err("inline SVG elements are unbalanced");
                    }
                }
                SvgEvent::Text(value) => {
                    bytes = bytes.saturating_add(value.len());
                    if (stack.is_empty() && !value.trim().is_empty())
                        || value
                            .chars()
                            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
                    {
                        return Err("inline SVG character data is invalid");
                    }
                }
            }
            if bytes > 1 << 20 {
                return Err("inline SVG byte limit exceeded");
            }
        }
        if roots != 1 || !stack.is_empty() || references.iter().any(|id| !ids.contains(id)) {
            return Err("inline SVG root or local reference is incomplete");
        }
        Ok(Self { events, bytes })
    }

    /// Retained decoded input size, for the enclosing Markdown plan's budget.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// Apply a declared display width to the inline occurrence. The retained
    /// external image is unchanged; intrinsic dimensions preserve its viewport
    /// ratio, with the view box supplying a ratio when dimensions are absent.
    ///
    /// # Errors
    /// Refuses a resized representation beyond the same structure and byte limits.
    pub fn with_width(mut self, width: std::num::NonZeroU32) -> Result<Self, &'static str> {
        let Some(SvgEvent::Start { attributes, .. }) = self
            .events
            .iter_mut()
            .find(|event| matches!(event, SvgEvent::Start { .. }))
        else {
            return Ok(self);
        };
        let attribute = |name: &str| {
            attributes
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.as_str())
        };
        let layout = crate::resource::SvgLayout::from_root(
            attribute("width"),
            attribute("height"),
            attribute("viewBox"),
        );
        let dimensions = layout.and_then(|layout| layout.width().zip(layout.height()));
        let height = layout
            .map(|layout| f64::from(width.get()) * layout.height_per_width())
            .filter(|height| height.is_finite() && *height > 0.0);
        if !attributes.iter().any(|(key, _)| key == "viewBox")
            && let Some((original_width, original_height)) = dimensions
        {
            attributes.push((
                "viewBox".into(),
                format!("0 0 {original_width} {original_height}"),
            ));
        }
        attributes.retain(|(key, _)| key != "width" && (height.is_none() || key != "height"));
        attributes.push(("width".into(), width.to_string()));
        if let Some(height) = height {
            attributes.push(("height".into(), height.to_string()));
        }
        Self::new(self.events)
    }

    /// DOM IDs emitted for this occurrence, for collision checks with its host page.
    pub fn scoped_ids<'a>(&'a self, occurrence: &'a str) -> impl Iterator<Item = String> + 'a {
        self.events.iter().filter_map(move |event| {
            let SvgEvent::Start { attributes, .. } = event else {
                return None;
            };
            attributes
                .iter()
                .find(|(key, _)| key == "id")
                .map(|(_, id)| {
                    format!(
                        "gazette-svg-{}-{id}",
                        Sha256Digest::of(occurrence.as_bytes())
                    )
                })
        })
    }

    /// Emit canonical markup with IDs scoped to this exact image occurrence.
    /// The same figure used twice therefore has independent title/description IDs.
    ///
    /// # Errors
    /// Returns a formatting error when the caller's bounded writer refuses output.
    pub fn write(&self, output: &mut impl std::fmt::Write, occurrence: &str) -> std::fmt::Result {
        let prefix = format!("gazette-svg-{}-", Sha256Digest::of(occurrence.as_bytes()));
        for event in &self.events {
            match event {
                SvgEvent::Start { name, attributes } => {
                    write!(output, "<{name}")?;
                    for (key, value) in attributes {
                        write!(output, " {key}=\"")?;
                        if key == "id" {
                            output.write_str(&prefix)?;
                        }
                        if matches!(key.as_str(), "aria-labelledby" | "aria-describedby") {
                            for (index, id) in value.split_ascii_whitespace().enumerate() {
                                if index > 0 {
                                    output.write_char(' ')?;
                                }
                                output.write_str(&prefix)?;
                                output.write_str(id)?;
                            }
                        } else {
                            escaped(output, value)?;
                        }
                        output.write_char('"')?;
                    }
                    output.write_char('>')?;
                }
                SvgEvent::End(name) => write!(output, "</{name}>")?,
                SvgEvent::Text(value) => escaped(output, value)?,
            }
        }
        Ok(())
    }
}

fn escaped(output: &mut impl std::fmt::Write, value: &str) -> std::fmt::Result {
    for c in value.chars() {
        match c {
            '&' => output.write_str("&amp;")?,
            '<' => output.write_str("&lt;")?,
            '>' => output.write_str("&gt;")?,
            '"' => output.write_str("&quot;")?,
            _ => output.write_char(c)?,
        }
    }
    Ok(())
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}

fn element(value: &str) -> bool {
    matches!(
        value,
        "svg"
            | "g"
            | "title"
            | "desc"
            | "path"
            | "text"
            | "tspan"
            | "rect"
            | "circle"
            | "ellipse"
            | "line"
            | "polyline"
            | "polygon"
    )
}

fn attribute(value: &str) -> bool {
    matches!(
        value,
        "width"
            | "height"
            | "viewBox"
            | "preserveAspectRatio"
            | "x"
            | "y"
            | "x1"
            | "y1"
            | "x2"
            | "y2"
            | "dx"
            | "dy"
            | "cx"
            | "cy"
            | "r"
            | "rx"
            | "ry"
            | "d"
            | "points"
            | "transform"
            | "fill"
            | "stroke"
            | "stroke-width"
            | "stroke-linecap"
            | "stroke-linejoin"
            | "stroke-dasharray"
            | "stroke-dashoffset"
            | "opacity"
            | "fill-opacity"
            | "stroke-opacity"
            | "fill-rule"
            | "clip-rule"
            | "vector-effect"
            | "font-family"
            | "font-size"
            | "font-weight"
            | "font-style"
            | "text-anchor"
            | "dominant-baseline"
            | "letter-spacing"
            | "textLength"
            | "lengthAdjust"
    )
}

fn presentation_value(value: &str) -> bool {
    if !value.is_ascii()
        || value
            .bytes()
            .any(|b| !(b.is_ascii_alphanumeric() || b" -_#%.,()+\t\r\n".contains(&b)))
    {
        return false;
    }
    if let Some(inner) = value.strip_prefix("var(").and_then(|v| v.strip_suffix(')')) {
        let (name, fallback) = inner
            .split_once(',')
            .map_or((inner, None), |(a, b)| (a, Some(b)));
        return name.strip_prefix("--").is_some_and(identifier)
            && fallback
                .is_none_or(|value| !value.trim().is_empty() && !value.contains(['(', ')']));
    }
    // URL-bearing properties are absent; only these value functions are useful
    // in the admitted paint/geometry attributes. Escaped tokens are never accepted.
    let mut rest = value;
    while let Some(open) = rest.find('(') {
        let name = rest[..open].trim();
        if !matches!(
            name,
            "rgb"
                | "rgba"
                | "hsl"
                | "hsla"
                | "translate"
                | "scale"
                | "rotate"
                | "matrix"
                | "skewX"
                | "skewY"
        ) {
            return false;
        }
        let Some(close) = rest[open + 1..].find(')') else {
            return false;
        };
        if rest[open + 1..open + close + 1].contains('(') {
            return false;
        }
        rest = rest[open + close + 2..].trim_start();
    }
    !rest.contains(')')
}
