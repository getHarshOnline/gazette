//! Authored metadata syntax boundary; both formats lower the same native schema.
use super::{FileError, Metadata};
use crate::{
    corpus::File,
    document::ParsedDocument,
    landing::ParsedLanding,
    toml::{self, PathSegment},
};
use gazette_core::source::{Location, Precision, Span};

impl Metadata<Option<gazette_core::publication::Key>> {
    pub(super) fn home_site(
        &self,
        compilation: &crate::workspace::Compilation,
        path: &gazette_core::source::EntryPath,
    ) -> Option<gazette_core::publication::Key> {
        self.content.site.clone().or_else(|| {
            compilation
                .workspace
                .content_source(path)
                .map(|source| source.site.clone())
        })
    }
}

pub(super) enum Input<'a, 's> {
    Markdown(&'a ParsedDocument<'s>),
    Landing(&'a ParsedLanding<'s>),
}
impl<'a, 's> Input<'a, 's> {
    pub(super) fn from_file(file: &'a File<'s>) -> Option<Self> {
        match file {
            File::Document(document) => Some(Self::Markdown(document)),
            File::Landing(landing) => Some(Self::Landing(landing)),
            _ => None,
        }
    }
    pub(super) fn read(
        &self,
    ) -> Result<Metadata<Option<gazette_core::publication::Key>>, FileError> {
        match self {
            Self::Markdown(document) => document
                .metadata()
                .deserialize()
                .map_err(FileError::Metadata),
            Self::Landing(landing) => landing.metadata().map_err(FileError::YamlMetadata),
        }
    }
    pub(super) fn declaration_span(&self) -> Span {
        match self {
            Self::Markdown(document) => document.metadata_span(),
            Self::Landing(landing) => landing
                .span_at(&[PathSegment::Key("content".into())])
                .unwrap_or(Span {
                    start: 0,
                    end: landing.source().len(),
                }),
        }
    }
    pub(super) fn field(&self, path: &[PathSegment]) -> Option<Location> {
        Some(match self {
            Self::Markdown(document) => Location {
                span: document
                    .metadata()
                    .span_at(path)?
                    .in_parent(document.source(), document.metadata_span())?,
                precision: Precision::Exact,
            },
            Self::Landing(landing) => Location {
                span: landing.span_at(path)?,
                precision: Precision::Construct,
            },
        })
    }
    pub(super) fn string(&self, path: &[PathSegment]) -> Option<(Location, &str)> {
        match self {
            Self::Markdown(document) => {
                let (span, value) = document.metadata().string_at(path)?;
                Some((
                    Location {
                        span: span.in_parent(document.source(), document.metadata_span())?,
                        precision: Precision::Exact,
                    },
                    value,
                ))
            }
            Self::Landing(landing) => {
                let (span, value) = landing.string_at(path)?;
                Some((
                    Location {
                        span,
                        precision: Precision::Construct,
                    },
                    value,
                ))
            }
        }
    }
    pub(super) fn error_span(&self, error: &FileError) -> Option<Span> {
        match (self, error) {
            (Self::Markdown(document), FileError::Metadata(toml::Error::Invalid { span, .. })) => {
                span.and_then(|span| span.in_parent(document.source(), document.metadata_span()))
            }
            (Self::Landing(_), FileError::YamlMetadata(error)) => error.span,
            _ => None,
        }
    }
}
