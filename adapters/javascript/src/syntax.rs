//! Parse once, reject unsupported runtime seams, and erase TypeScript with SWC.

use gazette_core::resource::Error;
use gazette_core::source::EntryPath;
use swc_core::common::comments::{Comment, SingleThreadedComments};
use swc_core::common::{Mark, SourceFile, SyntaxContext};
use swc_core::ecma::ast::{
    CallExpr, Callee, EsVersion, Expr, Ident, Lit, MetaPropExpr, MetaPropKind, Module, ModuleDecl,
    ModuleItem, Pass, Program,
};
use swc_core::ecma::parser::{EsSyntax, Lexer, Parser, StringInput, Syntax, TsSyntax};
use swc_core::ecma::transforms::base::{fixer::fixer, hygiene::hygiene, resolver};
use swc_core::ecma::transforms::typescript::strip;
use swc_core::ecma::visit::{Visit, VisitMut, VisitMutWith, VisitWith};

use crate::modules::invalid;

/// Link exact module-source nodes; ordinary authored strings stay intact.
pub(super) fn link(module: &mut Module, aliases: &std::collections::BTreeMap<String, String>) {
    module.visit_mut_with(&mut DeferredLinker(aliases));
    for item in &mut module.body {
        let ModuleItem::ModuleDecl(declaration) = item else {
            continue;
        };
        let source = match declaration {
            ModuleDecl::Import(import) => Some(import.src.as_mut()),
            ModuleDecl::ExportNamed(export) => export.src.as_deref_mut(),
            ModuleDecl::ExportAll(export) => Some(export.src.as_mut()),
            _ => None,
        };
        if let Some(source) = source
            && let Some(url) = source.value.as_str().and_then(|alias| aliases.get(alias))
        {
            source.value = url.as_str().into();
            source.raw = None;
        }
    }
}

pub(super) fn language(path: &EntryPath) -> Result<bool, Error> {
    match path
        .as_str()
        .rsplit_once('.')
        .map(|(_, extension)| extension)
    {
        Some("js" | "mjs") => Ok(false),
        Some("ts") => Ok(true),
        _ => Err(invalid(
            path,
            "only explicit .js, .mjs and .ts module files are supported",
        )),
    }
}

pub(super) fn parse(path: &EntryPath, file: &SourceFile) -> Result<(Module, Vec<Comment>), Error> {
    let typescript = language(path)?;
    crate::validation::module(&file.src, typescript)
        .map_err(|error| Error::Compile(format!("browser module {path}: {error}")))?;
    let syntax = if typescript {
        Syntax::Typescript(TsSyntax::default())
    } else {
        Syntax::Es(EsSyntax::default())
    };
    let comments = SingleThreadedComments::default();
    let lexer = Lexer::new(
        syntax,
        EsVersion::latest(),
        StringInput::from(file),
        Some(&comments),
    );
    let mut parser = Parser::new_from(lexer);
    let module = parser
        .parse_module()
        .map_err(|_| invalid(path, "invalid browser module syntax"))?;
    if !parser.take_errors().is_empty() {
        return Err(invalid(path, "invalid recoverable browser module syntax"));
    }
    if module.body.iter().any(|item| {
        matches!(
            item,
            ModuleItem::ModuleDecl(
                ModuleDecl::TsImportEquals(_) | ModuleDecl::TsExportAssignment(_)
            )
        )
    }) {
        return Err(invalid(
            path,
            "TypeScript CommonJS declarations are not supported",
        ));
    }
    let unresolved = Mark::new();
    let top_level = Mark::new();
    let mut program = Program::Module(module);
    resolver(unresolved, top_level, typescript).process(&mut program);
    let mut guard = Guard {
        unresolved: SyntaxContext::empty().apply_mark(unresolved),
        failure: None,
    };
    program.visit_with(&mut guard);
    if let Some(reason) = guard.failure {
        return Err(invalid(path, reason));
    }
    if typescript {
        strip(unresolved, top_level).process(&mut program);
    }
    hygiene().process(&mut program);
    fixer(None).process(&mut program);
    match program {
        Program::Module(module) => Ok((module, notices(&comments))),
        Program::Script(_) => Err(invalid(path, "parser returned a non-module")),
    }
}

/// Select actual syntax comments, never marker-like strings, regexes or source maps.
fn notices(comments: &SingleThreadedComments) -> Vec<Comment> {
    let (leading, trailing) = comments.borrow_all();
    let mut selected: Vec<_> = leading
        .values()
        .chain(trailing.values())
        .flatten()
        .filter(|comment| {
            comment.text.trim_start().starts_with('!')
                || comment.text.contains("@license")
                || comment.text.contains("@preserve")
        })
        .cloned()
        .collect();
    selected.sort_by_key(|comment| comment.span.lo);
    selected.dedup();
    selected
}

pub(super) fn imports(module: &Module) -> Result<Vec<String>, Error> {
    let mut imports = Vec::new();
    for item in &module.body {
        let ModuleItem::ModuleDecl(declaration) = item else {
            continue;
        };
        let source = match declaration {
            ModuleDecl::Import(import) => {
                if import.with.is_some() {
                    return Err(Error::Compile("import attributes are not supported".into()));
                }
                Some(import.src.as_ref())
            }
            ModuleDecl::ExportNamed(export) => {
                // SWC attaches internal metadata to synthesized local exports;
                // only a source-bearing export can have runtime attributes.
                if export.src.is_some() && export.with.is_some() {
                    return Err(Error::Compile("export attributes are not supported".into()));
                }
                export.src.as_deref()
            }
            ModuleDecl::ExportAll(export) => {
                if export.with.is_some() {
                    return Err(Error::Compile("export attributes are not supported".into()));
                }
                Some(export.src.as_ref())
            }
            _ => None,
        };
        if let Some(source) = source {
            imports.push(
                source
                    .value
                    .as_str()
                    .ok_or_else(|| Error::Compile("non-Unicode module specifier".into()))?
                    .to_owned(),
            );
        }
    }
    Ok(imports)
}

struct Guard {
    unresolved: SyntaxContext,
    failure: Option<&'static str>,
}

impl Visit for Guard {
    fn visit_call_expr(&mut self, expression: &CallExpr) {
        if matches!(expression.callee, Callee::Import(_)) && literal_import(expression).is_none() {
            self.failure =
                Some("dynamic imports require one literal declared alias without attributes");
        }
        expression.visit_children_with(self);
    }

    fn visit_meta_prop_expr(&mut self, expression: &MetaPropExpr) {
        if expression.kind == MetaPropKind::ImportMeta {
            self.failure = Some("import.meta is not supported");
        }
    }

    fn visit_ident(&mut self, identifier: &Ident) {
        if identifier.ctxt == self.unresolved
            && matches!(identifier.sym.as_ref(), "require" | "module" | "exports")
        {
            self.failure = Some("CommonJS bindings are not supported");
        }
    }
}

fn literal_import(expression: &CallExpr) -> Option<&str> {
    if expression.args.len() != 1 || expression.args[0].spread.is_some() {
        return None;
    }
    match expression.args[0].expr.as_ref() {
        Expr::Lit(Lit::Str(source)) => source.value.as_str(),
        _ => None,
    }
}

/// Actual deferred edges, including calls nested inside functions.
pub(super) fn deferred_imports(module: &Module) -> Vec<String> {
    struct Imports(Vec<String>);
    impl Visit for Imports {
        fn visit_call_expr(&mut self, call: &CallExpr) {
            if matches!(call.callee, Callee::Import(_))
                && let Some(source) = literal_import(call)
            {
                self.0.push(source.to_owned());
            }
            call.visit_children_with(self);
        }
    }
    let mut imports = Imports(Vec::new());
    module.visit_with(&mut imports);
    imports.0
}

struct DeferredLinker<'a>(&'a std::collections::BTreeMap<String, String>);
impl VisitMut for DeferredLinker<'_> {
    fn visit_mut_call_expr(&mut self, call: &mut CallExpr) {
        if matches!(call.callee, Callee::Import(_))
            && let Some(url) = literal_import(call).and_then(|alias| self.0.get(alias))
            && let Expr::Lit(Lit::Str(source)) = call.args[0].expr.as_mut()
        {
            source.value = url.as_str().into();
            source.raw = None;
        }
        call.visit_mut_children_with(self);
    }
}
