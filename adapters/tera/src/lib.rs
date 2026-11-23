//! Renders a Theme's captured templates against supplied context into page output.
//!
//! This adapter builds on `gazette-core` and the capability traits in `gazette-ports`;
//! `gazette-sdk` builds on it. It works on an explicit captured template set and
//! supplied context, without native effects. Templates are trusted presentation
//! programs, not a sandbox for hostile code. This adapter neither resolves publication
//! meaning nor exposes native lookups.

use std::{collections::BTreeMap, error::Error as _, io};

use gazette_core::render::LimitedWriter;
pub use gazette_core::render::{Error, Limits};
use gazette_core::source::{
    CapturedSource, Content, EntryPath, Inventory, TreeIdentity, TreeInput,
};
use gazette_ports::render::{CompiledTemplates, TemplateCompiler};
use serde_json::Value;
use tera::{Context, Tera};

mod css_url;
mod diagnostic;
mod editorial_date;
mod json_script;
mod macro_safety;
mod markdown_text;
mod url_query;

use diagnostic::TemplateDiagnostic;

/// Pinned renderer and captured-only behavior profile, not an executable attestation.
///
/// Covers default suffix escaping, disabled ambient functions and the bounded
/// `json_script`, CSS raw-style `css_url`, UTF-8 query-component `url_query`,
/// plain Markdown metadata and supplied editorial-date filters. Exact executable/dependency
/// qualification remains separate.
pub const PROFILE: &str = "tera@1.20.1/gazette-captured-templates-v8";

/// Tera implementation of the inward template-compilation port; no native discovery.
#[derive(Clone, Copy, Debug, Default)]
pub struct Compiler;

impl TemplateCompiler for Compiler {
    fn profile(&self) -> &'static str {
        PROFILE
    }

    fn compile(
        &self,
        source: &TreeInput<'_>,
        limits: Limits,
    ) -> Result<Box<dyn CompiledTemplates>, Error> {
        TemplateSet::compile_input(source.inventory(), |path| source.file(path), limits)
            .map(|templates| Box::new(templates) as Box<dyn CompiledTemplates>)
    }
}

/// Parsed presentation inputs, independent of their former native location.
///
/// No caller-supplied callbacks, SQL, native path readers or mutable renderer access are
/// exposed. Tera retains its mature inheritance, includes, macros and escaping.
pub struct TemplateSet {
    tera: Tera,
    sources: BTreeMap<String, String>,
    identity: TreeIdentity,
    limits: Limits,
}

impl TemplateSet {
    /// Parse every selected regular file as a template; no suffix filtering or discovery.
    ///
    /// # Errors
    /// Rejects resource excess, invalid UTF-8, missing captured payloads or native
    /// Tera syntax/inheritance/macro errors. No partial set is returned.
    pub fn compile(source: &CapturedSource, limits: Limits) -> Result<Self, Error> {
        Self::compile_input(source.inventory(), |path| source.file(path), limits)
    }

    fn compile_input<'a>(
        inventory: &'a Inventory,
        file: impl Fn(&EntryPath) -> Option<&'a [u8]>,
        limits: Limits,
    ) -> Result<Self, Error> {
        if !(1..=64).contains(&limits.execution_depth) {
            return Err(Error::Limit("template execution depth must be in 1..=64"));
        }
        if inventory.entries().len() > limits.templates.entries {
            return Err(Error::Limit("template entries"));
        }
        if inventory.total_bytes() > limits.templates.total_bytes {
            return Err(Error::Limit("template bytes"));
        }
        let mut sources = BTreeMap::new();
        let mut parsed = BTreeMap::new();
        for entry in inventory.entries() {
            if entry.path.as_str().split('/').count() > limits.templates.depth {
                return Err(Error::Limit("template path depth"));
            }
            if let Content::File { bytes, .. } = entry.content {
                if bytes > limits.templates.file_bytes {
                    return Err(Error::Limit("template file bytes"));
                }
                let bytes = file(&entry.path).ok_or_else(|| Error::Input {
                    path: entry.path.clone(),
                    reason: "captured payload missing",
                })?;
                let text = std::str::from_utf8(bytes).map_err(|_| Error::Input {
                    path: entry.path.clone(),
                    reason: "template is not UTF-8",
                })?;
                let parsed_template = tera::Template::new(entry.path.as_str(), None, text)
                    .map_err(|error| {
                        Error::Template(Box::new(TemplateDiagnostic::compile(
                            entry.path.as_str(),
                            text,
                            error,
                        )))
                    })?;
                parsed.insert(entry.path.as_str().to_owned(), parsed_template);
                sources.insert(entry.path.as_str().to_owned(), text.to_owned());
            }
        }
        macro_safety::inheritance(&parsed, limits.execution_depth)?;
        let mut tera = Tera::default();
        // Defend against feature unification as well as the current minimal pin.
        // Range allocation is unnecessary: the application supplies bounded lists.
        for function in ["get_env", "now", "get_random", "range"] {
            tera.functions.remove(function);
        }
        tera.register_filter(
            "editorial_date",
            editorial_date::FormatDate {
                output_bytes: limits.output_bytes,
            },
        );
        tera.register_filter(
            "json_script",
            json_script::JsonScript {
                output_bytes: limits.output_bytes,
            },
        );
        tera.register_filter(
            "url_query",
            url_query::UrlQuery {
                output_bytes: limits.output_bytes,
            },
        );
        tera.register_filter(
            "css_url",
            css_url::CssUrl {
                output_bytes: limits.output_bytes,
            },
        );
        for (name, destination) in [("markdown_text", false), ("markdown_destination", true)] {
            tera.register_filter(
                name,
                markdown_text::MarkdownText {
                    output_bytes: limits.output_bytes,
                    destination,
                },
            );
        }
        // Reuse the one native parse while retaining the exact checks performed
        // by add_raw_templates. Bound its recursive inheritance builder first.
        tera.templates.extend(parsed);
        tera.build_inheritance_chains()
            .and_then(|()| tera.check_macro_files())
            .map_err(|error| {
                let template = sources
                    .keys()
                    .next()
                    .map(String::as_str)
                    .unwrap_or("<template-set>");
                let source = sources
                    .get(template)
                    .map(String::as_str)
                    .unwrap_or_default();
                Error::Template(Box::new(TemplateDiagnostic::compile(
                    template, source, error,
                )))
            })?;
        macro_safety::admit(&tera, limits)?;
        Ok(Self {
            tera,
            sources,
            identity: inventory.identity(),
            limits,
        })
    }

    /// Exact captured template tree identity; host paths/timestamps are not added.
    pub fn identity(&self) -> TreeIdentity {
        self.identity
    }

    /// Render one named template with caller-resolved presentation data.
    ///
    /// HTML/HTM/XML suffixes use Tera's default autoescaping; other suffixes do
    /// not. The trusted template's `safe` filter explicitly bypasses escaping.
    /// Default escaping is not sanitization or JavaScript/CSS context-aware encoding.
    /// The explicit `json_script` filter serializes supplied JSON for HTML script
    /// element text only; it is not safe for attributes, CSS or arbitrary JavaScript.
    /// The explicit string-only `css_url` filter emits one quoted CSS URL token
    /// for HTML style-element raw text only; it does not admit or resolve URLs.
    ///
    /// # Errors
    /// Rejects non-object/oversized context, missing entry/includes/data, unsupported
    /// functions/filters, template failures or output overflow. Accumulated bytes
    /// are discarded on failure; no partial successful artifact escapes.
    pub fn render(&self, entry: &EntryPath, context: &Value) -> Result<Vec<u8>, Error> {
        if !context.is_object() {
            return Err(Error::ContextShape);
        }
        let mut size = LimitedWriter::new(io::sink(), self.limits.context_bytes);
        let serialized = serde_json::to_writer(&mut size, context);
        if size.exceeded() {
            return Err(Error::Limit("context bytes"));
        }
        serialized.map_err(|error| Error::Context(Box::new(error)))?;
        let context = Context::from_value(context.clone())
            .map_err(|error| Error::Template(Box::new(error)))?;
        let mut output = LimitedWriter::new(Vec::new(), self.limits.output_bytes);
        let rendered = self.tera.render_to(entry.as_str(), &context, &mut output);
        if output.exceeded() {
            return Err(Error::Limit("output bytes"));
        }
        rendered.map_err(|error| {
            // Native Tera wraps filter failures while retaining their typed cause.
            // A filter may hit its expansion ceiling before the outer writer sees bytes.
            let mut source = error.source();
            while let Some(cause) = source {
                if matches!(
                    cause.downcast_ref::<Error>(),
                    Some(Error::Limit("output bytes"))
                ) {
                    return Error::Limit("output bytes");
                }
                source = cause.source();
            }
            Error::Template(Box::new(TemplateDiagnostic::render(
                entry.as_str(),
                &self.sources,
                error,
            )))
        })?;
        Ok(output.into_inner())
    }
}

impl CompiledTemplates for TemplateSet {
    fn identity(&self) -> TreeIdentity {
        Self::identity(self)
    }

    fn profile(&self) -> &'static str {
        PROFILE
    }

    fn render(&self, entry: &EntryPath, context: &Value) -> Result<Vec<u8>, Error> {
        Self::render(self, entry, context)
    }
}
