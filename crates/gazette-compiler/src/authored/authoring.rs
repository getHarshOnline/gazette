//! Tolerant frontmatter help over the exact authored metadata contract.

use std::sync::OnceLock;

use serde_json::Value;

use crate::{
    document::{self, Frame},
    workspace::{AuthoringError, AuthoringHelp, StringOccurrence, authoring::TomlAuthoring},
};

/// Recoverable TOML frontmatter facts for one exact authored document buffer.
#[derive(Clone, Debug)]
pub struct AuthoredAuthoring {
    metadata_span: gazette_core::source::Span,
    inner: TomlAuthoring,
    profile: AuthoredSchema,
}

/// Native frontmatter owner selected by an exact current workspace binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthoredSchema {
    /// Complete authored page declaration.
    Page,
    /// Reduced reusable legal source, not an independently site-bound page.
    LegalSource,
}

impl AuthoredAuthoring {
    /// Native schema owner retained for these current-buffer facts.
    pub fn schema(&self) -> AuthoredSchema {
        self.profile
    }
    /// Classify an exact current scalar through the native authored field owner.
    /// Tolerant structural assistance remains available even without strict metadata.
    ///
    /// # Errors
    /// Refuses invalid current source coordinates or native string projections.
    pub fn editorial_field(
        &self,
        source: &str,
        offset: usize,
    ) -> Result<Option<super::EditorialField>, AuthoringError> {
        Ok(self
            .string_occurrence(source, offset)?
            .and_then(|occurrence| editorial_field(&occurrence.path)))
    }
    /// Recover explicit frontmatter without asserting strict document or content validity.
    ///
    /// A plain Markdown document has no authored metadata assistance. An incomplete
    /// closing fence retains current metadata help but remains invalid for compilation.
    ///
    /// # Errors
    /// Refuses document/TOML ceilings or an invalid recoverable syntax projection.
    pub fn parse(source: &str, limits: document::Limits) -> Result<Option<Self>, AuthoringError> {
        Self::parse_with_schema(source, limits, AuthoredSchema::Page)
    }

    /// Recover current frontmatter using the caller's native selected source role.
    ///
    /// # Errors
    /// Refuses the same framing and TOML bounds as ordinary authored assistance.
    pub fn parse_with_schema(
        source: &str,
        limits: document::Limits,
        profile: AuthoredSchema,
    ) -> Result<Option<Self>, AuthoringError> {
        let framing = match document::frame(source, limits) {
            Ok(value) => value,
            Err(document::Error::Framing(_)) => return Ok(None),
            Err(document::Error::SourceLimit) => {
                return Err(AuthoringError::Limit("document source bytes"));
            }
            Err(document::Error::Metadata {
                error: crate::toml::Error::Limit(resource),
                ..
            }) => return Err(AuthoringError::Limit(resource)),
            Err(_) => return Err(AuthoringError::Invalid),
        };
        let metadata_span = match framing {
            Frame::Complete { metadata, .. } | Frame::Incomplete { metadata } => metadata,
        };
        let metadata = metadata_span.slice(source).ok_or(AuthoringError::Invalid)?;
        Ok(Some(Self {
            metadata_span,
            inner: TomlAuthoring::parse(metadata, limits.metadata)?,
            profile,
        }))
    }

    /// Project exact frontmatter fields through the same native schema and source ranges.
    ///
    /// # Errors
    /// Refuses invalid frontmatter or parent source spans and exhausted
    /// assistance or complete-output byte bounds.
    pub fn structure(
        &self,
        source: &str,
        maximum: usize,
    ) -> Result<Vec<crate::workspace::AuthoringStructureField>, AuthoringError> {
        let metadata = self
            .metadata_span
            .slice(source)
            .ok_or(AuthoringError::Invalid)?;
        let mut fields = self
            .inner
            .structure(metadata_schema(self.profile), metadata, maximum)?;
        for field in &mut fields {
            field.location.span = field
                .location
                .span
                .in_parent(source, self.metadata_span)
                .ok_or(AuthoringError::Invalid)?;
        }
        Ok(fields)
    }

    /// Return source-bound frontmatter completion and hover facts at one full-file offset.
    ///
    /// # Errors
    /// Refuses invalid coordinates, schema paths, or oversized complete results.
    pub fn help(
        &self,
        source: &str,
        offset: usize,
        maximum_bytes: usize,
    ) -> Result<Option<AuthoringHelp>, AuthoringError> {
        if offset < self.metadata_span.start || offset > self.metadata_span.end {
            return Ok(None);
        }
        let metadata = self
            .metadata_span
            .slice(source)
            .ok_or(AuthoringError::Invalid)?;
        let local = offset
            .checked_sub(self.metadata_span.start)
            .ok_or(AuthoringError::Invalid)?;
        Ok(Some(
            self.inner
                .help(
                    metadata_schema(self.profile),
                    metadata,
                    local,
                    maximum_bytes,
                )?
                .in_parent(source, self.metadata_span)?,
        ))
    }

    /// Decode the exact current TOML string scalar beneath one full-file offset.
    ///
    /// This exposes source evidence only. The generated-schema path and strict
    /// authored model remain responsible for deciding whether the value is a
    /// resource input, an entrypoint or an unrelated string.
    ///
    /// # Errors
    /// Refuses invalid coordinates or a string occurrence that cannot be lifted
    /// into the complete authored source.
    pub fn string_occurrence(
        &self,
        source: &str,
        offset: usize,
    ) -> Result<Option<StringOccurrence>, AuthoringError> {
        if offset < self.metadata_span.start || offset > self.metadata_span.end {
            return Ok(None);
        }
        let metadata = self
            .metadata_span
            .slice(source)
            .ok_or(AuthoringError::Invalid)?;
        let local = offset
            .checked_sub(self.metadata_span.start)
            .ok_or(AuthoringError::Invalid)?;
        let Some(mut occurrence) = self.inner.string_occurrence(metadata, local)? else {
            return Ok(None);
        };
        occurrence.location.span = occurrence
            .location
            .span
            .in_parent(source, self.metadata_span)
            .ok_or(AuthoringError::Invalid)?;
        Ok(Some(occurrence))
    }

    /// Exact native page-reference scalar, including unresolved identities.
    /// Reduced legal sources have no page site, principal or related declarations.
    ///
    /// # Errors
    /// Refuses invalid current native coordinates or decoded identity spelling.
    pub fn reference_slot(
        &self,
        source: &str,
        offset: usize,
    ) -> Result<Option<super::AuthoredReferenceSlot>, AuthoringError> {
        if self.profile == AuthoredSchema::LegalSource {
            return Ok(None);
        }
        super::reference_authoring::slot(self.string_occurrence(source, offset)?)
    }

    /// Locate one unambiguous content-reference value, ignoring titles and other strings.
    /// Repeated references retain the enclosing diagnostic location because a target
    /// identity alone does not say which occurrence violated a relationship rule.
    /// A selected editorial field excludes other valid references to the same target.
    ///
    /// # Errors
    /// Refuses invalid source coordinates or decoded native reference identities.
    pub fn content_reference_location(
        &self,
        source: &str,
        target: &gazette_core::publication::Key,
        field: Option<super::EditorialField>,
    ) -> Result<Option<gazette_core::source::Location>, AuthoringError> {
        use super::AuthoredReferenceField::{BrowsingIndex, RelatedContent};
        if self.profile != AuthoredSchema::Page {
            return Ok(None);
        }
        let metadata = self
            .metadata_span
            .slice(source)
            .ok_or(AuthoringError::Invalid)?;
        let mut found = None;
        for occurrence in self.inner.strings(metadata) {
            let occurrence = occurrence?;
            if occurrence.value != target.as_str() {
                continue;
            }
            let mut location = occurrence.location;
            let editorial = editorial_field(&occurrence.path);
            let reference = match field {
                Some(field) => editorial == Some(field),
                None => {
                    editorial.is_some()
                        || super::reference_authoring::slot(Some(occurrence))?.is_some_and(|slot| {
                            matches!(slot.field, RelatedContent | BrowsingIndex)
                        })
                }
            };
            if !reference {
                continue;
            }
            if found.is_some() {
                return Ok(None);
            }
            location.span = location
                .span
                .in_parent(source, self.metadata_span)
                .ok_or(AuthoringError::Invalid)?;
            found = Some(location);
        }
        Ok(found)
    }
}

fn editorial_field(
    path: &[crate::workspace::AuthoringPathSegment],
) -> Option<super::EditorialField> {
    use super::EditorialField;
    use crate::workspace::AuthoringPathSegment::{ArrayItem, Field};
    match path {
        [Field(field), ArrayItem] if field == "collections" => {
            Some(EditorialField::CollectionResidence)
        }
        [Field(content), Field(structure), Field(field), ArrayItem]
            if content == "content" && structure == "structure" && field == "member_order" =>
        {
            Some(EditorialField::CollectionOrder)
        }
        [Field(content), Field(structure), Field(field)]
            if content == "content" && structure == "structure" && field == "parent" =>
        {
            Some(EditorialField::CollectionParent)
        }
        [Field(content), Field(structure), Field(field)]
            if content == "content" && structure == "structure" && field == "collection" =>
        {
            Some(EditorialField::JourneyCollection)
        }
        [
            Field(content),
            Field(structure),
            Field(members),
            ArrayItem,
            Field(field),
        ] if content == "content"
            && structure == "structure"
            && members == "members"
            && field == "content" =>
        {
            Some(EditorialField::JourneyMember)
        }
        [
            Field(content),
            Field(structure),
            Field(navigation),
            rest @ ..,
        ] if content == "content"
            && structure == "structure"
            && navigation == "navigation"
            && rest.len() >= 2
            && rest.len() % 2 == 0
            && matches!(rest.first(), Some(ArrayItem))
            && matches!(rest.last(), Some(Field(field)) if field == "content")
            && rest[1..rest.len() - 1]
                .chunks_exact(2)
                .all(|pair| matches!(pair, [Field(field), ArrayItem] if field == "pages")) =>
        {
            Some(EditorialField::DocumentationNavigation)
        }
        _ => None,
    }
}

fn metadata_schema(profile: AuthoredSchema) -> &'static Value {
    static SCHEMA: OnceLock<Value> = OnceLock::new();
    static LEGAL: OnceLock<Value> = OnceLock::new();
    match profile {
        AuthoredSchema::Page => SCHEMA.get_or_init(|| {
            serde_json::to_value(super::declaration_schema())
                .expect("generated authored metadata schema serializes")
        }),
        AuthoredSchema::LegalSource => LEGAL.get_or_init(|| {
            serde_json::to_value(super::legal_source_schema())
                .expect("generated legal source schema serializes")
        }),
    }
}
