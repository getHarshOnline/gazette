//! Explain an effective publication field using its original declaration.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Key, OriginError, Workspace};
use crate::toml::{ParsedToml, PathSegment};
use gazette_core::source::{EntryPath, Sha256Digest, Span};

/// A scalar or list in a site's effective publication settings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PublicationOriginQuery {
    /// Site consuming the value.
    pub site: Key,
    /// Field path within the publication, such as `contacts.rights`.
    pub path: Vec<PathSegment>,
}

/// The declared relationship through which a publication field was selected.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", content = "identity", rename_all = "kebab-case")]
pub enum PublicationScope {
    /// Workspace-wide publication default.
    Default,
    /// Default of the site's explicitly named publisher.
    Publisher(Key),
    /// Selected account supplying a profile field; ownership is a separate declaration.
    Account(Key),
    /// Explicit publication override for this site.
    Site(Key),
}

/// Winning value and exact source coordinates from the successful compilation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct PublicationFieldOrigin {
    /// Receiving site.
    pub site: Key,
    /// Site, publisher, workspace default or selected account supplying this value.
    pub scope: PublicationScope,
    /// Named document for a modular workspace; absent for supplied single-document input.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document: Option<EntryPath>,
    /// Digest of the original document bytes.
    pub source_sha256: Sha256Digest,
    /// Path in that original document, including the declaration's array position.
    pub path: Vec<PathSegment>,
    /// Original half-open UTF-8 byte range.
    pub span: Span,
    /// Resolved value, before any template renders it.
    pub value: Value,
}

pub(super) fn origin<'a, 's: 'a>(
    workspace: &Workspace,
    documents: impl IntoIterator<Item = (Option<&'a EntryPath>, &'a ParsedToml<'s>)>,
    query: &PublicationOriginQuery,
) -> Result<PublicationFieldOrigin, OriginError> {
    let site = workspace
        .site(&query.site)
        .ok_or(OriginError::UnknownIdentity)?;
    let publication = workspace
        .publication_for(&query.site)
        .ok_or(OriginError::UnknownIdentity)?;
    let mut effective = serde_json::to_value(publication).expect("publication serializes");
    // An empty selection can be an authored override, even though the output
    // schema omits it. Retain it for both value lookup and source traversal.
    effective["accounts"] =
        serde_json::to_value(&publication.accounts).expect("account selections serialize");
    let value = at(&effective, &query.path)
        .cloned()
        .ok_or(OriginError::NoExplicitField)?;
    // A merged table can contain values from several sources; ask for its fields.
    if value.is_object() || value.is_null() {
        return Err(OriginError::NoExplicitField);
    }
    let documents: Vec<_> = documents.into_iter().collect();
    if let [
        PathSegment::Key(social),
        PathSegment::Key(platform),
        PathSegment::Key(field),
    ] = query.path.as_slice()
        && social == "social"
        && let Some(account) = publication
            .accounts
            .iter()
            .filter_map(|id| workspace.account(id))
            .find(|account| &account.platform == platform)
    {
        for (document, syntax) in &documents {
            let source: Value = syntax
                .deserialize()
                .map_err(|_| OriginError::NoExplicitField)?;
            for (index, declared) in array(&source, "accounts").iter().enumerate() {
                if declared.get("id").and_then(Value::as_str) != Some(account.id.as_str()) {
                    continue;
                }
                let input = if field == "url" && declared.get("url").is_none() {
                    "handle"
                } else {
                    field.as_str()
                };
                let path = vec![key("accounts"), PathSegment::Index(index), key(input)];
                let span = syntax.span_at(&path).ok_or(OriginError::NoExplicitField)?;
                return Ok(PublicationFieldOrigin {
                    site: site.id.clone(),
                    scope: PublicationScope::Account(account.id.clone()),
                    document: document.cloned(),
                    source_sha256: Sha256Digest::of(syntax.source().as_bytes()),
                    path,
                    span,
                    value: value.clone(),
                });
            }
        }
        return Err(OriginError::NoExplicitField);
    }
    let mut winner = None;
    let mut rank = 0;
    for (document, syntax) in documents {
        let source: Value = syntax
            .deserialize()
            .map_err(|_| OriginError::NoExplicitField)?;
        let mut candidates = vec![(
            1,
            PublicationScope::Default,
            vec![key("defaults"), key("publication")],
        )];
        for (index, declared) in array(&source, "sites").iter().enumerate() {
            if declared.get("id").and_then(Value::as_str) == Some(site.id.as_str())
                && declared.get("name").is_some()
            {
                candidates.push((
                    3,
                    PublicationScope::Site(site.id.clone()),
                    vec![key("sites"), PathSegment::Index(index)],
                ));
            }
        }
        for (index, publisher) in array(&source, "publishers").iter().enumerate() {
            if publisher.get("id").and_then(Value::as_str) == Some(site.publisher.as_str()) {
                candidates.push((
                    2,
                    PublicationScope::Publisher(site.publisher.clone()),
                    vec![
                        key("publishers"),
                        PathSegment::Index(index),
                        key("publication"),
                    ],
                ));
            }
        }
        for (brand_index, brand) in array(&source, "brands").iter().enumerate() {
            for (index, publication) in array(brand, "publications").iter().enumerate() {
                if publication.get("site").and_then(Value::as_str) == Some(site.id.as_str()) {
                    candidates.push((
                        3,
                        PublicationScope::Site(site.id.clone()),
                        vec![
                            key("brands"),
                            PathSegment::Index(brand_index),
                            key("publications"),
                            PathSegment::Index(index),
                        ],
                    ));
                }
            }
        }
        for (priority, scope, mut path) in candidates {
            if priority <= rank {
                continue;
            }
            let flat = matches!(path.first(), Some(PathSegment::Key(name)) if name == "sites");
            let mut field = query.path.clone();
            if flat && field.len() == 1 {
                let alias = match &field[0] {
                    PathSegment::Key(name) if name == "public_base_url" => Some("domain"),
                    PathSegment::Key(name) if name == "local_port" => Some("port"),
                    _ => None,
                };
                if let Some(alias) = alias {
                    let mut alias_path = path.clone();
                    alias_path.push(key(alias));
                    if at(&source, &alias_path).is_some() {
                        field[0] = key(alias);
                    }
                }
            }
            let Some(original) = at(&source, &path) else {
                continue;
            };
            let Some(field) = original_path(&effective, original, &field) else {
                continue;
            };
            path.extend(field);
            if at(&source, &path).is_none() {
                continue;
            }
            let Some(span) = syntax.span_at(&path) else {
                continue;
            };
            rank = priority;
            winner = Some(PublicationFieldOrigin {
                site: site.id.clone(),
                scope,
                document: document.cloned(),
                source_sha256: Sha256Digest::of(syntax.source().as_bytes()),
                path,
                span,
                value: value.clone(),
            });
        }
    }
    winner.ok_or(OriginError::NoExplicitField)
}

// Resolved records may be sorted by identity. Keep their original array positions
// when explaining a nested field, rather than attributing it to a neighbour.
fn original_path(
    effective: &Value,
    original: &Value,
    path: &[PathSegment],
) -> Option<Vec<PathSegment>> {
    // Short site aliases name only scalar values and need no resolved traversal.
    if matches!(path, [PathSegment::Key(name)] if matches!(name.as_str(), "domain" | "port")) {
        return Some(path.to_vec());
    }
    let mut resolved = effective;
    let mut authored = original;
    let mut result = Vec::new();
    for part in path {
        let selected = match part {
            PathSegment::Index(index) => {
                let item = resolved.get(*index)?;
                if let Some(id) = item.get("id") {
                    PathSegment::Index(
                        authored
                            .as_array()?
                            .iter()
                            .position(|candidate| candidate.get("id") == Some(id))?,
                    )
                } else {
                    part.clone()
                }
            }
            _ => part.clone(),
        };
        resolved = at(resolved, std::slice::from_ref(part))?;
        authored = at(authored, std::slice::from_ref(&selected))?;
        result.push(selected);
    }
    Some(result)
}

fn key(value: &str) -> PathSegment {
    PathSegment::Key(value.into())
}

fn array<'a>(value: &'a Value, field: &str) -> &'a [Value] {
    value
        .get(field)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

fn at<'a>(mut value: &'a Value, path: &[PathSegment]) -> Option<&'a Value> {
    for part in path {
        value = match part {
            PathSegment::Key(key) => value.get(key)?,
            PathSegment::Index(index) => value.get(*index)?,
        };
    }
    Some(value)
}
