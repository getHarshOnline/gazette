//! Standard workspace symbols from the sole current selected-source search owner.

use gazette_sdk::{
    authoring::{SymbolKind, session::Session},
    source::Sha256Digest,
};
use lsp_server::ErrorCode;
use lsp_types::{Location, SymbolInformation};
use serde_json::Value;

use crate::positions;

pub(super) fn query(
    state: &Session,
    query: &str,
    maximum: usize,
) -> Result<Value, (ErrorCode, &'static str)> {
    let unavailable = || {
        (
            ErrorCode::InvalidRequest,
            "complete current workspace symbols are unavailable",
        )
    };
    let result = state
        .search_symbols(query, maximum)
        .map_err(|_| unavailable())?;
    if result.generation != state.generation() {
        return Err(unavailable());
    }
    let mut symbols = Vec::new();
    let mut retained = 0usize;
    for symbol in result.symbols {
        let document = state.files().get(symbol.document).ok_or_else(unavailable)?;
        let source = document.text();
        if document.path() != symbol.source.path
            || document.version() != symbol.version
            || symbol.source.source_sha256 != Sha256Digest::of(source.as_bytes())
            || symbol.source.location.span.slice(source).is_none()
        {
            return Err(unavailable());
        }
        retained = retained
            .checked_add(std::mem::size_of::<SymbolInformation>())
            .and_then(|n| n.checked_add(symbol.name.len()))
            .and_then(|n| n.checked_add(symbol.document.len()))
            .filter(|n| *n <= maximum)
            .ok_or_else(unavailable)?;
        #[allow(deprecated)]
        symbols.push(SymbolInformation {
            name: symbol.name.to_owned(),
            kind: match symbol.kind {
                SymbolKind::Declaration => lsp_types::SymbolKind::OBJECT,
                SymbolKind::Heading => lsp_types::SymbolKind::NAMESPACE,
            },
            tags: None,
            deprecated: None,
            container_name: None,
            location: Location {
                uri: symbol.document.parse().map_err(|_| unavailable())?,
                range: positions::range(source, symbol.source.location.span)
                    .ok_or_else(unavailable)?,
            },
        });
    }
    let mut bounded = gazette_sdk::render::LimitedWriter::new(std::io::sink(), maximum);
    if serde_json::to_writer(&mut bounded, &symbols).is_err() || bounded.exceeded() {
        return Err(unavailable());
    }
    serde_json::to_value(symbols).map_err(|_| unavailable())
}
