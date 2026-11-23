//! Standard locations projected from current SDK-owned source associations.

use gazette_sdk::authoring::session::Session;
use gazette_sdk::source::Sha256Digest;
use lsp_server::ErrorCode;
use lsp_types::{Location, Position, Range, TextDocumentPositionParams};
use serde_json::Value;

use crate::positions;

pub(super) fn links(
    state: &Session,
    id: &str,
    maximum: usize,
) -> Result<Value, (ErrorCode, &'static str)> {
    let unavailable = || {
        (
            ErrorCode::InvalidRequest,
            "current resolved document links are unavailable",
        )
    };
    let document = state
        .files()
        .get(id)
        .ok_or((ErrorCode::InvalidParams, "document is not selected"))?;
    let analysis = state.analysis().as_ref().map_err(|_| unavailable())?;
    let links = analysis
        .source_links(document.analysis_path(), document.text(), maximum)
        .map_err(|_| unavailable())?;
    let mut result = Vec::new();
    for link in links {
        let (target_id, target) = state
            .files()
            .iter()
            .find(|(_, target)| target.analysis_path() == link.definition.path)
            .ok_or_else(unavailable)?;
        if link.definition.source_sha256 != Sha256Digest::of(target.text().as_bytes()) {
            return Err(unavailable());
        }
        let target_range = positions::range(target.text(), link.definition.location.span)
            .ok_or_else(unavailable)?;
        let range =
            positions::range(document.text(), link.occurrence.span).ok_or_else(unavailable)?;
        let target_uri = format!(
            "{target_id}#L{},{}",
            target_range.start.line + 1,
            target_range.start.character + 1
        );
        result.push(lsp_types::DocumentLink {
            range, target: Some(target_uri.parse().map_err(|_| unavailable())?),
            tooltip: Some(format!("Open local source: {}", target.path())),
            data: Some(serde_json::json!({"generation":state.generation(), "source_sha256":Sha256Digest::of(document.text().as_bytes()), "definition":link.definition})),
        });
    }
    let mut bounded = gazette_sdk::render::LimitedWriter::new(std::io::sink(), maximum);
    serde_json::to_writer(&mut bounded, &result).map_err(|_| unavailable())?;
    if bounded.exceeded() {
        return Err(unavailable());
    }
    serde_json::to_value(result).map_err(|_| unavailable())
}

pub(super) fn query(
    state: &Session,
    params: TextDocumentPositionParams,
    references: Option<bool>,
    output_bytes: usize,
) -> Result<Value, (ErrorCode, &'static str)> {
    let invalid = |reason| (ErrorCode::InvalidParams, reason);
    let unavailable = || {
        (
            ErrorCode::InvalidRequest,
            "current source navigation is unavailable",
        )
    };
    let document = state
        .files()
        .get(params.text_document.uri.as_str())
        .ok_or_else(|| invalid("document is not selected"))?;
    let source = document.text();
    let offset = positions::offset(source, params.position)
        .map_err(|_| invalid("position is outside an exact UTF-16 source boundary"))?;
    let analysis = state.analysis().as_ref().map_err(|_| unavailable())?;
    if references.is_none() {
        let resource = state
            .page_resource_navigation(params.text_document.uri.as_str(), offset)
            .map_err(|_| unavailable())?;
        if let Some(resource) = resource {
            if resource.path != document.path()
                || resource.source_sha256 != Sha256Digest::of(source.as_bytes())
                || resource.occurrence.span.slice(source).is_none()
            {
                return Err(unavailable());
            }
            let (id, target) = state
                .page_inputs()
                .iter()
                .find(|(_, target)| target.path() == resource.companion.path())
                .ok_or_else(unavailable)?;
            if Some(target.source_sha256()) != resource.companion.source_sha256() {
                return Err(unavailable());
            }
            let location = Location {
                uri: id.parse().map_err(|_| unavailable())?,
                // Page inputs are retained opaque bytes. The client owns opening
                // their selected URI; Gazette never fabricates source coordinates.
                range: Range::new(Position::new(0, 0), Position::new(0, 0)),
            };
            let mut bounded =
                gazette_sdk::render::LimitedWriter::new(std::io::sink(), output_bytes);
            serde_json::to_writer(&mut bounded, &Some(&location)).map_err(|_| unavailable())?;
            if bounded.exceeded() {
                return Err(unavailable());
            }
            return serde_json::to_value(Some(location)).map_err(|_| unavailable());
        }
    }
    let navigation = analysis
        .source_navigation(document.analysis_path(), source, offset)
        .map_err(|_| unavailable())?;
    let Some(navigation) = navigation else {
        return Ok(if references.is_some() {
            Value::Array(Vec::new())
        } else {
            Value::Null
        });
    };
    if navigation.path != document.analysis_path()
        || navigation.source_sha256 != Sha256Digest::of(source.as_bytes())
        || navigation.occurrence.span.slice(source).is_none()
    {
        return Err(unavailable());
    }
    let mut locations = Vec::new();
    let mut retained = 0usize;
    let mut add =
        |location: gazette_sdk::authoring::SourceLocation<'_>| -> Result<(), (ErrorCode, &'static str)> {
            let (id, target) = state.files().iter()
                .find(|(_, target)| target.analysis_path() == location.path)
                .ok_or_else(unavailable)?;
            let source = target.text();
            if location.source_sha256 != Sha256Digest::of(source.as_bytes())
                || location.location.span.slice(source).is_none()
            {
                return Err(unavailable());
            }
            // Bound URI amplification before cloning; full JSON framing is checked later.
            retained = retained
                .checked_add(id.len())
                .and_then(|n| n.checked_add(std::mem::size_of::<Location>()))
                .filter(|n| *n <= output_bytes)
                .ok_or_else(unavailable)?;
            locations.push(Location {
                uri: id.parse().map_err(|_| unavailable())?,
                range: positions::range(source, location.location.span).ok_or_else(unavailable)?,
            });
            Ok(())
        };
    match references {
        None => add(navigation.definition)?,
        Some(include_declaration) => {
            for reference in navigation.references {
                add(reference)?;
            }
            if include_declaration {
                add(navigation.definition)?;
            }
        }
    }
    locations.sort_by(|left, right| {
        let key = |location: &Location| {
            (
                location.range.start.line,
                location.range.start.character,
                location.range.end.line,
                location.range.end.character,
            )
        };
        left.uri
            .as_str()
            .cmp(right.uri.as_str())
            .then_with(|| key(left).cmp(&key(right)))
    });
    locations.dedup();
    let mut bounded = gazette_sdk::render::LimitedWriter::new(std::io::sink(), output_bytes);
    let encoded = if references.is_some() {
        serde_json::to_writer(&mut bounded, &locations)
    } else {
        serde_json::to_writer(&mut bounded, &locations.first())
    };
    if encoded.is_err() || bounded.exceeded() {
        return Err(unavailable());
    }
    if references.is_some() {
        serde_json::to_value(locations).map_err(|_| unavailable())
    } else {
        serde_json::to_value(locations.first()).map_err(|_| unavailable())
    }
}
