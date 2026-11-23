//! Native field classification and selected declaration evidence for page references.

use super::{ProjectionError, ResolvedAuthored};
use crate::workspace::{
    AuthoringError, AuthoringPathSegment, DeclarationKind, StringOccurrence, WorkspaceSymbol,
};
use gazette_core::{
    publication::Key,
    source::{EntryPath, Location, Sha256Digest, Span},
};

/// Native authored field namespace, independent of key spelling.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AuthoredReferenceField {
    /// Explicit page home site.
    Site,
    /// A separately declared person or organization in an authored attribution.
    AttributionPrincipal,
    /// The explicitly named person or organization described by an About page.
    SubjectPrincipal,
    /// The explicitly named provider of a page's service subject.
    ServiceProvider,
    /// Non-containing related content, including cross-site selected content.
    RelatedContent,
    /// Exact routed index referenced by a page-owned browsing association.
    BrowsingIndex,
}

impl AuthoredReferenceField {
    /// Whether a retained workspace declaration supplies this reference namespace.
    pub fn accepts_declaration(self, declaration: Self) -> bool {
        self == declaration
            || matches!(
                (self, declaration),
                (
                    Self::SubjectPrincipal | Self::ServiceProvider,
                    Self::AttributionPrincipal
                )
            )
    }
}

/// One exact current native TOML string occurrence.
#[derive(Debug)]
pub struct AuthoredReferenceSlot {
    /// Native field classification.
    pub field: AuthoredReferenceField,
    /// Exact whole scalar including quotes and escapes.
    pub location: Location,
    /// Decoded current identity, which may be unresolved.
    pub target: Key,
}

pub(super) fn slot(
    occurrence: Option<StringOccurrence>,
) -> Result<Option<AuthoredReferenceSlot>, AuthoringError> {
    use AuthoringPathSegment::{ArrayItem, Field};
    let Some(occurrence) = occurrence else {
        return Ok(None);
    };
    let field = match occurrence.path.as_slice() {
        [Field(content), Field(field)] if content == "content" && field == "site" => {
            AuthoredReferenceField::Site
        }
        [Field(content), Field(attributions), ArrayItem, Field(field)]
            if content == "content" && attributions == "attributions" && field == "principal" =>
        {
            AuthoredReferenceField::AttributionPrincipal
        }
        [Field(field), ArrayItem] if field == "related" => AuthoredReferenceField::RelatedContent,
        [Field(content), Field(subject), Field(field)]
            if content == "content" && subject == "subject" && field == "principal" =>
        {
            AuthoredReferenceField::SubjectPrincipal
        }
        [Field(content), Field(subject), Field(field)]
            if content == "content" && subject == "subject" && field == "provider" =>
        {
            AuthoredReferenceField::ServiceProvider
        }
        [Field(field), ArrayItem] if field == "browsing" => AuthoredReferenceField::BrowsingIndex,
        _ => return Ok(None),
    };
    let target = occurrence
        .value
        .parse()
        .map_err(|_| AuthoringError::Invalid)?;
    Ok(Some(AuthoredReferenceSlot {
        field,
        location: occurrence.location,
        target,
    }))
}

/// One retained declaration; generated identities retain construct, not scalar precision.
#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct AuthoredReferenceDeclaration {
    /// Exact native identity, never a name or route-derived lookup.
    pub identity: Key,
    /// Native namespace supplying this declaration.
    pub field: AuthoredReferenceField,
    /// Exact selected logical source containing the native definition.
    pub path: EntryPath,
    /// Complete current declaration source identity.
    pub source_sha256: Sha256Digest,
    /// Native definition location; derived content IDs have construct precision.
    pub location: Location,
    replacement: String,
    #[serde(skip)]
    #[schemars(skip)]
    browsing_index: bool,
}
impl AuthoredReferenceDeclaration {
    /// Retain one already-parsed workspace declaration using its exact original literal.
    ///
    /// # Errors
    /// Refuses non-site/principal declarations or inconsistent native source coordinates.
    pub fn workspace(
        symbol: &WorkspaceSymbol,
        path: &EntryPath,
        source: &str,
    ) -> Result<Option<Self>, AuthoringError> {
        let Some(declaration) = symbol.declaration() else {
            return Ok(None);
        };
        let field = match declaration.kind {
            DeclarationKind::Site => AuthoredReferenceField::Site,
            DeclarationKind::Principal => AuthoredReferenceField::AttributionPrincipal,
            _ => return Ok(None),
        };
        let replacement = symbol
            .replacement(source)
            .map_err(|_| AuthoringError::Invalid)?;
        Ok(Some(Self {
            identity: declaration.identity.clone(),
            field,
            path: path.clone(),
            source_sha256: Sha256Digest::of(source.as_bytes()),
            location: declaration.location,
            replacement: replacement.into_owned(),
            browsing_index: false,
        }))
    }
    /// Complete valid TOML string scalar, not an unquoted identity.
    pub fn replacement(&self) -> &str {
        &self.replacement
    }
    /// Whether the exact native target is a routed, nonrepublished index page.
    pub fn is_browsing_index(&self) -> bool {
        self.browsing_index
    }
    /// Complete retained fact size charged to the shared navigation budget.
    pub fn fact_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.identity.as_str().len()
            + self.path.as_str().len()
            + self.replacement.len()
    }
}
impl ResolvedAuthored<'_, '_> {
    /// Every current selected content identity, including composed legal outputs.
    /// Uses existing graph/source associations; notes never become candidates.
    ///
    /// # Errors
    /// Refuses impossible retained associations or exhausted complete-result bounds.
    pub fn authored_reference_targets(
        &self,
        occurrence_limit: usize,
        byte_limit: usize,
    ) -> Result<Vec<AuthoredReferenceDeclaration>, ProjectionError> {
        let mut result = Vec::new();
        let mut bytes = 0usize;
        for node in self.content().graph().nodes() {
            let invalid = || ProjectionError::Binding {
                id: node.id.clone(),
                reason: "authored reference target has no retained definition",
            };
            let file = self.file(&node.id).ok_or_else(invalid)?;
            let (path, mut location) = self
                .reference_definition(&node.id, "", Span { start: 0, end: 0 })
                .map_err(|_| invalid())?;
            let replacement = if let Some(origin) = self.origin(
                &node.id,
                &[
                    crate::toml::PathSegment::Key("content".into()),
                    crate::toml::PathSegment::Key("id".into()),
                ],
            ) && let Some(span) = origin.span
            {
                let source = file.source().ok_or_else(invalid)?;
                let structured = matches!(file, crate::corpus::File::Landing(_));
                location = Location {
                    span,
                    precision: if structured {
                        gazette_core::source::Precision::Construct
                    } else {
                        gazette_core::source::Precision::Exact
                    },
                };
                if structured {
                    serde_json::to_string(&node.id).map_err(ProjectionError::Encoding)?
                } else {
                    span.slice(source).ok_or_else(invalid)?.to_owned()
                }
            } else {
                // Key is natively validated; JSON string escaping is valid TOML basic
                // string syntax. Derived IDs never claim this literal was authored.
                serde_json::to_string(&node.id).map_err(ProjectionError::Encoding)?
            };
            let target = AuthoredReferenceDeclaration {
                identity: node.id.clone(),
                field: AuthoredReferenceField::RelatedContent,
                path: path.clone(),
                source_sha256: file.source_sha256(),
                location,
                replacement,
                browsing_index: matches!(
                    node.structure,
                    gazette_core::publication::content::ContentKind::Page {
                        family: gazette_core::publication::content::PageFamily::Index
                    }
                ) && node
                    .route
                    .as_ref()
                    .is_some_and(|route| route.canonical.is_none()),
            };
            bytes = bytes
                .checked_add(target.fact_bytes())
                .filter(|&n| n <= byte_limit)
                .ok_or(ProjectionError::Limit("authored reference target bytes"))?;
            if result.len() >= occurrence_limit {
                return Err(ProjectionError::Limit(
                    "authored reference target occurrences",
                ));
            }
            result.push(target);
        }
        Ok(result)
    }
}
