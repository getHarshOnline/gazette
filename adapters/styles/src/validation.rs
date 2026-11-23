//! Validate emitted CSS without becoming a second compiler or resource resolver.

use std::collections::BTreeSet;

use gazette_core::resource::Error;
use lightningcss::{
    printer::PrinterOptions,
    properties::{
        custom::{Function, Token, TokenOrValue},
        font::{FamilyName, FontFamily},
    },
    rules::{
        CssRule,
        font_face::{FontFaceProperty, FontFaceRule, FontFormat, Source},
    },
    stylesheet::{ParserOptions, StyleSheet},
    traits::ToCss,
    values::{image::Image, resolution::Resolution, url::Url},
    visit_types,
    visitor::{Visit, VisitTypes, Visitor},
};

pub(crate) fn css(source: &str, limit: usize) -> Result<BTreeSet<String>, Error> {
    let mut sheet = StyleSheet::parse(
        source,
        ParserOptions {
            error_recovery: false,
            ..ParserOptions::default()
        },
    )
    .map_err(|error| Error::Compile(format!("emitted CSS: {error}")))?;
    let mut resources = Resources {
        references: BTreeSet::new(),
        bytes: 0,
        limit,
    };
    sheet.visit(&mut resources)?;
    Ok(resources.references)
}

struct Resources {
    references: BTreeSet<String>,
    bytes: usize,
    limit: usize,
}

fn unsupported(reason: &'static str) -> Error {
    Error::Compile(format!("emitted CSS: {reason}"))
}

fn custom_font_descriptor(
    name: &str,
    value: &[TokenOrValue<'_>],
) -> Result<Option<&'static str>, Error> {
    let name = name.to_ascii_lowercase();
    let value = value
        .iter()
        .filter(|token| !token.is_whitespace())
        .collect::<Vec<_>>();
    let percentage = || {
        matches!(
            value.as_slice(),
            [TokenOrValue::Token(Token::Percentage { unit_value, .. })]
                if unit_value.is_finite() && *unit_value >= 0.0
        )
    };
    match name.as_str() {
        "font-display" => {
            let [TokenOrValue::Token(Token::Ident(value))] = value.as_slice() else {
                return Err(unsupported("invalid font-display descriptor"));
            };
            if !["auto", "block", "swap", "fallback", "optional"]
                .iter()
                .any(|candidate| value.eq_ignore_ascii_case(candidate))
            {
                return Err(unsupported("invalid font-display descriptor"));
            }
            Ok(Some("font-display"))
        }
        "size-adjust" if percentage() => Ok(Some("size-adjust")),
        "ascent-override" | "descent-override" | "line-gap-override" => {
            let normal = matches!(
                value.as_slice(),
                [TokenOrValue::Token(Token::Ident(value))] if value.eq_ignore_ascii_case("normal")
            );
            if normal || percentage() {
                Ok(Some(match name.as_str() {
                    "ascent-override" => "ascent-override",
                    "descent-override" => "descent-override",
                    _ => "line-gap-override",
                }))
            } else {
                Err(unsupported("invalid font metric override descriptor"))
            }
        }
        "size-adjust" => Err(unsupported("invalid size-adjust descriptor")),
        _ => Ok(None),
    }
}

fn local_font_name(name: &FamilyName<'_>) -> Result<(), Error> {
    let encoded = name
        .to_css_string(PrinterOptions::default())
        .map_err(|_| unsupported("invalid local font name"))?;
    if encoded.len() > 256 {
        return Err(unsupported(
            "serialized local font name exceeds 256 UTF-8 bytes",
        ));
    }
    // FamilyName keeps its decoded string private. Its maintained serializer and
    // tokenizer preserve CSS escapes without interpreting names as paths or URLs.
    let mut input = cssparser::ParserInput::new(&encoded);
    let mut parser = cssparser::Parser::new(&mut input);
    let decoded = if let Ok(value) = parser.try_parse(|parser| parser.expect_string_cloned()) {
        value.to_string()
    } else {
        let mut words = Vec::new();
        while !parser.is_exhausted() {
            words.push(
                parser
                    .expect_ident_cloned()
                    .map_err(|_| unsupported("invalid local font name"))?
                    .to_string(),
            );
        }
        words.join(" ")
    };
    parser
        .expect_exhausted()
        .map_err(|_| unsupported("invalid local font name"))?;
    if decoded.trim().is_empty() || decoded.len() > 256 || decoded.chars().any(char::is_control) {
        return Err(unsupported(
            "local font names must be nonempty, control-free and bounded",
        ));
    }
    Ok(())
}

fn font_face(rule: &FontFaceRule<'_>) -> Result<(), Error> {
    let mut family = false;
    let mut source = false;
    let mut optional = BTreeSet::new();
    for property in &rule.properties {
        match property {
            FontFaceProperty::FontFamily(_) if !family => family = true,
            FontFaceProperty::Source(sources) if !source => {
                if let [Source::Url(url)] = sources.as_slice() {
                    if !matches!(url.format, Some(FontFormat::WOFF2)) || !url.tech.is_empty() {
                        return Err(unsupported(
                            "@font-face source requires explicit WOFF2 format and no technology hints",
                        ));
                    }
                } else if sources.is_empty() || sources.len() > 16 {
                    return Err(unsupported(
                        "@font-face requires one WOFF2 URL or 1–16 local font names",
                    ));
                } else {
                    for candidate in sources {
                        let Source::Local(FontFamily::FamilyName(name)) = candidate else {
                            return Err(unsupported(
                                "@font-face cannot mix local names, generic families and URL sources",
                            ));
                        };
                        local_font_name(name)?;
                    }
                }
                source = true;
            }
            FontFaceProperty::FontStyle(_) if optional.insert("font-style") => {}
            FontFaceProperty::FontWeight(_) if optional.insert("font-weight") => {}
            FontFaceProperty::FontStretch(_) if optional.insert("font-stretch") => {}
            FontFaceProperty::UnicodeRange(_) if optional.insert("unicode-range") => {}
            FontFaceProperty::Custom(property) => {
                let Some(name) = custom_font_descriptor(property.name.as_ref(), &property.value.0)?
                else {
                    return Err(unsupported("custom @font-face descriptors are unsupported"));
                };
                if !optional.insert(name) {
                    return Err(unsupported(
                        "duplicate @font-face descriptors are unsupported",
                    ));
                }
            }
            _ => {
                return Err(unsupported(
                    "duplicate @font-face descriptors are unsupported",
                ));
            }
        }
    }
    if !family || !source {
        return Err(unsupported(
            "@font-face requires one family and one supported source descriptor",
        ));
    }
    Ok(())
}

impl<'i> Visitor<'i> for Resources {
    type Error = Error;

    fn visit_types(&self) -> VisitTypes {
        visit_types!(RULES | URLS | IMAGES | FUNCTIONS)
    }

    fn visit_rule(&mut self, rule: &mut CssRule<'i>) -> Result<(), Self::Error> {
        match rule {
            CssRule::Import(_) => Err(unsupported("unresolved CSS imports are unsupported")),
            CssRule::Unknown(_) | CssRule::Custom(_) => {
                Err(unsupported("opaque CSS at-rules are unsupported"))
            }
            CssRule::FontFace(rule) => {
                font_face(rule)?;
                rule.visit_children(self)
            }
            CssRule::Namespace(_) | CssRule::MozDocument(_) => {
                Err(unsupported("resource-bearing CSS at-rules are unsupported"))
            }
            _ => rule.visit_children(self),
        }
    }

    fn visit_url(&mut self, url: &mut Url<'i>) -> Result<(), Self::Error> {
        let value = url.url.as_ref();
        if value.trim().is_empty() || value.chars().any(char::is_control) {
            return Err(unsupported("CSS URL must be nonempty and control-free"));
        }
        // URLs are emitted references, not Sass source dependencies. The caller
        // binds them against the final output address; do not resolve or rewrite.
        if !self.references.contains(value) {
            self.bytes = self
                .bytes
                .checked_add(value.len())
                .filter(|bytes| *bytes <= self.limit)
                .ok_or(Error::Limit("stylesheet reference bytes"))?;
            self.references.insert(value.to_owned());
        }
        Ok(())
    }

    fn visit_image(&mut self, image: &mut Image<'i>) -> Result<(), Self::Error> {
        if let Image::ImageSet(set) = image {
            for option in &mut set.options {
                let (Resolution::Dpi(value) | Resolution::Dpcm(value) | Resolution::Dppx(value)) =
                    option.resolution;
                if !value.is_finite() || value < 0.0 {
                    return Err(unsupported(
                        "CSS image-set resolution must be finite and nonnegative",
                    ));
                }
                if matches!(option.image, Image::ImageSet(_) | Image::None) {
                    return Err(unsupported(
                        "nested or empty CSS image-set candidates are unsupported",
                    ));
                }
                // Quoted candidates are native Url values too. Collect all of
                // them; type/resolution hints never waive output closure or let
                // the compiler choose a browser-specific subset.
                option.image.visit(self)?;
            }
            return Ok(());
        }
        image.visit_children(self)
    }

    fn visit_function(&mut self, function: &mut Function<'i>) -> Result<(), Self::Error> {
        let name = function.name.as_ref();
        if [
            "url",
            "src",
            "image",
            "image-set",
            "-webkit-image-set",
            "paint",
            "element",
            "-moz-element",
            "expression",
            "random",
            "unique-id",
        ]
        .iter()
        .any(|blocked| name.eq_ignore_ascii_case(blocked))
        {
            return Err(unsupported(
                "unresolved CSS resource or nondeterministic function is unsupported",
            ));
        }
        function.visit_children(self)
    }
}
