//! Independent grammar and lexical/module early-error checks, not type checking.
//!
//! SWC owns transformation and bundling. Oxc only accepts or rejects source and
//! emitted text; its AST is discarded and never becomes a second publication model.
//! Each call adds a bounded-input parse and semantic walk, not a CPU/memory sandbox.

use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;

/// Reuse publication routing's canonical HTTP(S) spelling without reading destinations.
pub(super) fn links(
    entry: &gazette_core::source::EntryPath,
    links: &gazette_core::resource::ModuleLinks,
    maximum: usize,
) -> Result<(), gazette_core::resource::Error> {
    use gazette_core::{
        publication::routing::{PublicBaseUrl, RoutePath},
        resource::Error,
    };
    let mut bytes = 0usize;
    for url in links.values() {
        bytes = bytes
            .checked_add(url.len())
            .ok_or(Error::Limit("module link bytes"))?;
        if bytes > maximum {
            return Err(Error::Limit("module link bytes"));
        }
        let valid = url
            .rsplit_once('/')
            .filter(|(_, name)| !name.is_empty())
            .is_some_and(|(parent, name)| {
                match (
                    format!("{parent}/").parse::<PublicBaseUrl>(),
                    format!("/{name}").parse::<RoutePath>(),
                ) {
                    (Ok(base), Ok(path)) => base.resolve(&path) == *url,
                    _ => false,
                }
            });
        if !valid {
            return Err(crate::modules::invalid(
                entry,
                "module link must be an exact canonical HTTP(S) file URL",
            ));
        }
    }
    Ok(())
}

pub(super) fn module(text: &str, typescript: bool) -> Result<(), String> {
    let allocator = Allocator::default();
    let source_type = if typescript {
        SourceType::ts()
    } else {
        SourceType::mjs()
    };
    let parsed = Parser::new(&allocator, text, source_type).parse();
    if let Some(error) = parsed.errors.first() {
        return Err(error.to_string());
    }
    if parsed.panicked {
        return Err("module parser did not complete".into());
    }
    let program = allocator.alloc(parsed.program);
    let checked = SemanticBuilder::new()
        .with_check_syntax_error(true)
        .build(program);
    if let Some(error) = checked.errors.first() {
        return Err(error.to_string());
    }
    Ok(())
}
