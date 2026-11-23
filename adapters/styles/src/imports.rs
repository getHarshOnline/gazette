//! Admit decoded import literals using the same compiler's actual Sass AST.

use std::path::Path;

use gazette_core::source::EntryPath;
use grass_compiler::sass_ast::{AstImport, AstStmt};

pub(crate) fn validate(path: &EntryPath, source: &str) -> Result<(), String> {
    let options = grass_compiler::Options::default()
        .fs(&grass_compiler::NullFs)
        .quiet(true);
    let sheet = grass_compiler::parse_stylesheet(source.to_owned(), path.as_str(), &options)
        .map_err(|error| error.to_string())?;
    let mut bodies = vec![sheet.body.as_slice()];
    while let Some(body) = bodies.pop() {
        for statement in body {
            match statement {
                AstStmt::Use(rule) => reference(path, &rule.url)?,
                AstStmt::Forward(rule) => reference(path, &rule.url)?,
                AstStmt::ImportRule(rule) => {
                    for import in &rule.imports {
                        match import {
                            AstImport::Sass(import) => reference(path, Path::new(&import.url))?,
                            AstImport::Plain(_) => {
                                return Err(format!(
                                    "{path}: external or dynamic CSS imports are unsupported"
                                ));
                            }
                        }
                    }
                }
                AstStmt::If(rule) => {
                    for clause in &rule.if_clauses {
                        bodies.push(&clause.body);
                    }
                    if let Some(body) = &rule.else_clause {
                        bodies.push(body);
                    }
                }
                AstStmt::For(rule) => bodies.push(&rule.body),
                AstStmt::RuleSet(rule) => bodies.push(&rule.body),
                AstStmt::Style(rule) => bodies.push(&rule.body),
                AstStmt::Each(rule) => bodies.push(&rule.body),
                AstStmt::Media(rule) => bodies.push(&rule.body),
                AstStmt::Include(rule) => {
                    if let Some(content) = &rule.content {
                        bodies.push(&content.body);
                    }
                }
                AstStmt::While(rule) => bodies.push(&rule.body),
                AstStmt::FunctionDecl(rule) => bodies.push(&rule.body),
                AstStmt::Mixin(rule) => bodies.push(&rule.body),
                AstStmt::UnknownAtRule(rule) => {
                    if let Some(body) = &rule.body {
                        bodies.push(body);
                    }
                }
                AstStmt::AtRootRule(rule) => bodies.push(&rule.body),
                AstStmt::Supports(rule) => bodies.push(&rule.body),
                AstStmt::Return(_)
                | AstStmt::VariableDecl(_)
                | AstStmt::LoudComment(_)
                | AstStmt::SilentComment(_)
                | AstStmt::ContentRule(_)
                | AstStmt::Warn(_)
                | AstStmt::ErrorRule(_)
                | AstStmt::Extend(_)
                | AstStmt::Debug(_) => {}
            }
        }
    }
    Ok(())
}

fn reference(source: &EntryPath, reference: &Path) -> Result<(), String> {
    let reject = || format!("{source}: unsupported stylesheet import path");
    let text = reference.to_str().ok_or_else(reject)?;
    if text == "sass:meta" {
        return Err(format!(
            "{source}: sass:meta dynamic loading is unsupported"
        ));
    }
    if matches!(
        text,
        "sass:color" | "sass:list" | "sass:map" | "sass:math" | "sass:selector" | "sass:string"
    ) {
        return Ok(());
    }
    let parent = Path::new(source.as_str())
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let resolved = super::captured::logical(&parent.join(reference)).ok_or_else(reject)?;
    // Grass replaces *any* unrecognized extension before probing Fs. Reject it
    // using the retained AST literal, while preserving extensionless Sass imports.
    if reference.extension().is_some() && !super::captured::supported(&resolved) {
        return Err(reject());
    }
    Ok(())
}
