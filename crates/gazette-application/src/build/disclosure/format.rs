//! One format-selection and dispatch owner for files and decoded data payloads.

use gazette_compiler::javascript::Goal;
use gazette_core::{render::MediaType, source::EntryPath};

use super::{DisclosureDecoding, DisclosureProbeError, DisclosureRepresentation, Scan};

#[derive(Clone, Copy)]
pub(super) enum Format {
    Raw,
    Text,
    Markdown,
    GeneratedMdx,
    Json,
    Html,
    Xml,
    Css,
    Javascript(Goal),
    Raster(gazette_core::raster::Format),
    Woff2,
}

impl Format {
    pub(super) fn for_file(path: &EntryPath, declared: Option<MediaType>) -> Self {
        if let Some(declared) = declared {
            return match declared {
                MediaType::Html => Self::Html,
                MediaType::Json | MediaType::JsonLd | MediaType::WebManifest => Self::Json,
                MediaType::Xml | MediaType::Rss | MediaType::Atom | MediaType::Svg => Self::Xml,
                MediaType::Text => Self::Text,
                MediaType::Markdown => Self::Markdown,
                MediaType::JavaScript => Self::Javascript(Goal::Module),
                MediaType::Css => Self::Css,
                MediaType::Png => Self::Raster(gazette_core::raster::Format::Png),
                MediaType::Jpeg => Self::Raster(gazette_core::raster::Format::Jpeg),
                MediaType::Webp => Self::Raster(gazette_core::raster::Format::Webp),
                MediaType::Woff2 => Self::Woff2,
            };
        }
        let suffix = path
            .as_str()
            .rsplit_once('.')
            .map_or("", |(_, suffix)| suffix);
        match suffix.to_ascii_lowercase().as_str() {
            "json" | "jsonld" | "map" => Self::Json,
            "html" | "htm" => Self::Html,
            "xml" | "rss" | "atom" | "svg" => Self::Xml,
            "css" => Self::Css,
            "txt" => Self::Text,
            "md" | "markdown" => Self::Markdown,
            "mdx" => Self::GeneratedMdx,
            "png" => Self::Raster(gazette_core::raster::Format::Png),
            "webp" => Self::Raster(gazette_core::raster::Format::Webp),
            "jpg" | "jpeg" => Self::Raster(gazette_core::raster::Format::Jpeg),
            _ => super::javascript::file_goal(suffix).map_or(Self::Raw, Self::Javascript),
        }
    }

    pub(super) fn for_media(media: &str) -> Self {
        match media {
            "text/plain" => Self::Text,
            "text/markdown" => Self::Markdown,
            "text/html" => Self::Html,
            "application/json" => Self::Json,
            "application/xml" | "text/xml" => Self::Xml,
            "text/css" => Self::Css,
            "image/png" => Self::Raster(gazette_core::raster::Format::Png),
            "image/webp" => Self::Raster(gazette_core::raster::Format::Webp),
            "image/jpeg" => Self::Raster(gazette_core::raster::Format::Jpeg),
            "text/javascript"
            | "application/javascript"
            | "text/ecmascript"
            | "application/ecmascript" => Self::Javascript(Goal::Unambiguous),
            media if media.ends_with("+json") => Self::Json,
            media if media.ends_with("+xml") => Self::Xml,
            _ => Self::Raw,
        }
    }

    pub(super) fn coverage(self) -> DisclosureDecoding {
        match self {
            Self::Raw | Self::Raster(_) | Self::Woff2 | Self::Markdown | Self::GeneratedMdx => {
                DisclosureDecoding::None
            }
            Self::Text => DisclosureDecoding::Utf8Text,
            Self::Json => DisclosureDecoding::JsonStrings,
            Self::Html => DisclosureDecoding::HtmlTokens {
                profile: gazette_compiler::html::PROFILE,
                css_profile: gazette_compiler::css::PROFILE,
                javascript_profile: gazette_compiler::javascript::PROFILE,
            },
            Self::Xml => DisclosureDecoding::XmlEvents {
                profile: gazette_compiler::xml::PROFILE,
                css_profile: gazette_compiler::css::PROFILE,
            },
            Self::Css => DisclosureDecoding::CssTokens {
                profile: gazette_compiler::css::PROFILE,
            },
            Self::Javascript(goal) => DisclosureDecoding::JavascriptSyntax {
                profile: gazette_compiler::javascript::PROFILE,
                goal,
            },
        }
    }

    pub(super) fn scan(
        self,
        scan: &mut Scan<'_>,
        path: &EntryPath,
        bytes: &[u8],
    ) -> Result<DisclosureDecoding, DisclosureProbeError> {
        match self {
            Self::Raster(format) => return super::raster::scan(scan, path, format, bytes),
            Self::Woff2 => return super::font::scan(scan, path, bytes),
            Self::Markdown => return super::markdown::scan(scan, path, bytes),
            Self::GeneratedMdx => return super::generated_mdx::scan(scan, path, bytes),
            Self::Raw => Ok(()),
            Self::Text => {
                std::str::from_utf8(bytes).map_err(|_| DisclosureProbeError::Encoded {
                    path: path.clone(),
                    reason: "text requires UTF-8",
                })?;
                scan.text(path, bytes, DisclosureRepresentation::Text)
            }
            Self::Json => super::json::scan(scan, path, bytes),
            Self::Html => super::html::scan(scan, path, bytes),
            Self::Xml => super::xml::scan(scan, path, bytes),
            Self::Css => {
                super::css::scan(scan, path, gazette_compiler::css::Input::Stylesheet(bytes))
            }
            Self::Javascript(goal) => super::javascript::scan(scan, path, bytes, goal),
        }?;
        Ok(self.coverage())
    }
}
