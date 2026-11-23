//! Complete selected-buffer symbol search shared by editor and agent interfaces.

use super::{NavigationError, SourceLocation, session::Session};
use crate::source::{Location, Sha256Digest};
use serde::Serialize;

/// Navigation category, not a new publication object type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SymbolKind {
    /// A compiler-owned workspace declaration.
    Declaration,
    /// A Markdown heading construct, not a whole section or rename range.
    Heading,
}

/// A match bound to its actual selected document, current revision and native span.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct SearchSymbol<'a> {
    /// Opaque selected client identifier; never opened by the SDK.
    pub document: &'a str,
    /// Current overlay version, or none for the retained baseline.
    #[schemars(range(min = i32::MIN, max = i32::MAX))]
    pub version: Option<i32>,
    /// Exact source and full-file construct coordinates.
    pub source: SourceLocation<'a>,
    /// Original declaration identity or heading text, without normalization.
    pub name: &'a str,
    /// Native navigation category.
    pub kind: SymbolKind,
}

/// Complete matches at one synchronous selected-source generation.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct SymbolSearch<'a> {
    /// Changes only when the session accepts a source update.
    #[schemars(range(max = u64::MAX))]
    pub generation: u64,
    /// Logical-path/construct order, never a silently truncated ranked subset.
    pub symbols: Vec<SearchSymbol<'a>>,
}

impl Session {
    /// Search current selected workspace declarations and Markdown headings without I/O.
    ///
    /// Matching uses Unicode lowercase substring comparison; it does not normalize
    /// identities or promise linguistic case folding. An empty query selects all symbols.
    /// Invalid current syntax fails the complete query, even when another file has matches.
    /// Structural symbols do not imply that publication semantics have passed admission.
    ///
    /// # Errors
    /// Refuses unavailable current symbols, invalid source coordinates, and a complete
    /// result or query exceeding `maximum` bytes. No stale or partial result is returned.
    pub fn search_symbols(
        &self,
        query: &str,
        maximum: usize,
    ) -> Result<SymbolSearch<'_>, NavigationError> {
        if query.len() > maximum {
            return Err(NavigationError::Limit);
        }
        let query = query.to_lowercase();
        if query.len() > maximum {
            return Err(NavigationError::Limit);
        }
        let analysis = self
            .analysis()
            .as_ref()
            .map_err(|_| NavigationError::Unavailable)?;
        let mut result = SymbolSearch {
            generation: self.generation(),
            symbols: Vec::new(),
        };
        let mut retained = 0usize;
        for (id, document) in self.files() {
            let text = document.text();
            let digest = Sha256Digest::of(text.as_bytes());
            let mut append = |name, kind, location: Location| {
                if location.span.slice(text).is_none() {
                    return Err(NavigationError::InvalidSource);
                }
                let name: &str = name;
                if !name.to_lowercase().contains(&query) {
                    return Ok(());
                }
                retained = retained
                    .checked_add(std::mem::size_of::<SearchSymbol<'_>>())
                    .and_then(|n| n.checked_add(id.len()))
                    .and_then(|n| n.checked_add(document.path().as_str().len()))
                    .and_then(|n| n.checked_add(name.len()))
                    .filter(|n| *n <= maximum)
                    .ok_or(NavigationError::Limit)?;
                result.symbols.push(SearchSymbol {
                    document: id,
                    version: document.version(),
                    name,
                    kind,
                    source: SourceLocation {
                        path: document.path(),
                        source_sha256: digest,
                        location,
                    },
                });
                Ok(())
            };
            if let Some(symbols) = analysis.workspace_symbols(document.analysis_path(), text)? {
                for symbol in symbols {
                    append(symbol.name(), SymbolKind::Declaration, symbol.location())?;
                }
            } else if let Some(headings) =
                analysis.heading_symbols(document.analysis_path(), text)?
            {
                for heading in &headings.values {
                    let span = heading
                        .span
                        .in_parent(text, headings.body_span)
                        .ok_or(NavigationError::InvalidSource)?;
                    append(
                        if heading.text.is_empty() {
                            &heading.id
                        } else {
                            &heading.text
                        },
                        SymbolKind::Heading,
                        Location {
                            span,
                            precision: heading.precision,
                        },
                    )?;
                }
            }
        }
        result.symbols.sort_by(|a, b| {
            a.source
                .path
                .cmp(b.source.path)
                .then_with(|| {
                    a.source
                        .location
                        .span
                        .start
                        .cmp(&b.source.location.span.start)
                })
                .then_with(|| a.name.cmp(b.name))
        });
        let mut writer = crate::render::LimitedWriter::new(std::io::sink(), maximum);
        serde_json::to_writer(&mut writer, &result).map_err(|_| NavigationError::Limit)?;
        Ok(result)
    }
}
