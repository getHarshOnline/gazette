//! Exact logical resolution and one prepared module set shared by bundler callbacks.

use std::collections::{BTreeMap, BTreeSet};

use gazette_core::resource::Error;
use gazette_core::source::{EntryPath, TreeInput};
use swc_core::bundler::{Hook, Load, ModuleData, ModuleRecord, Resolve};
use swc_core::common::comments::Comment;
use swc_core::common::{FileName, SourceFile, SourceMap, Span, sync::Lrc};
use swc_core::ecma::ast::{KeyValueProp, Module};
use swc_core::ecma::loader::resolve::Resolution;
use swc_core::ecma::transforms::base::helpers::Helpers;

use crate::syntax;

struct Prepared {
    file: Lrc<SourceFile>,
    module: Module,
    notices: Vec<Comment>,
}

pub(super) struct Modules {
    local: BTreeMap<EntryPath, Prepared>,
    external: BTreeSet<String>,
    source_map: Lrc<SourceMap>,
}

impl Modules {
    pub(super) fn prepare(
        source: &TreeInput<'_>,
        entry: &EntryPath,
        aliases: &BTreeMap<String, String>,
        cm: Lrc<SourceMap>,
    ) -> Result<Self, Error> {
        let mut modules = BTreeMap::new();
        let mut pending = BTreeSet::from([entry.clone()]);
        while let Some(path) = pending.pop_first() {
            if modules.contains_key(&path) {
                continue;
            }
            let bytes = source
                .file(&path)
                .ok_or_else(|| invalid(&path, "module is not captured"))?;
            let text =
                std::str::from_utf8(bytes).map_err(|_| invalid(&path, "module is not UTF-8"))?;
            let file = cm.new_source_file(
                FileName::Custom(path.as_str().to_owned()).into(),
                text.to_owned(),
            );
            let (mut module, notices) = syntax::parse(&path, &file)?;
            for specifier in syntax::imports(&module)? {
                if !aliases.contains_key(&specifier) {
                    pending.insert(relative(&path, &specifier)?);
                }
            }
            if syntax::deferred_imports(&module)
                .iter()
                .any(|alias| !aliases.contains_key(alias))
            {
                return Err(invalid(
                    &path,
                    "dynamic imports require an explicitly declared shared-module alias",
                ));
            }
            syntax::link(&mut module, aliases);
            modules.insert(
                path,
                Prepared {
                    file,
                    module,
                    notices,
                },
            );
        }
        Ok(Self {
            local: modules,
            external: aliases.values().cloned().collect(),
            source_map: cm,
        })
    }

    pub(super) fn paths(&self) -> BTreeSet<EntryPath> {
        self.local.keys().cloned().collect()
    }

    /// Retain notices from every bundled dependency, including removed exports.
    pub(super) fn notices(&self) -> impl Iterator<Item = &Comment> {
        self.local.values().flat_map(|module| module.notices.iter())
    }
}

impl Load for Modules {
    fn load(&self, file: &FileName) -> Result<ModuleData, anyhow::Error> {
        // SWC visits re-export targets even when marked external. This is a
        // linker placeholder for an exact declared URL, never acquired source.
        if let FileName::Custom(url) = file
            && self.external.contains(url)
        {
            return Ok(ModuleData {
                fm: self
                    .source_map
                    .new_source_file(file.clone().into(), String::new()),
                module: Module::default(),
                helpers: Helpers::new(false),
            });
        }
        let path = logical(file)?;
        let module = self
            .local
            .get(&path)
            .ok_or_else(|| invalid(&path, "module was not prepared"))?;
        Ok(ModuleData {
            fm: module.file.clone(),
            module: module.module.clone(),
            helpers: Helpers::new(false),
        })
    }
}

impl Resolve for Modules {
    fn resolve(&self, base: &FileName, specifier: &str) -> Result<Resolution, anyhow::Error> {
        if self.external.contains(specifier) {
            return Ok(Resolution {
                filename: FileName::Custom(specifier.to_owned()),
                slug: None,
            });
        }
        let path = relative(&logical(base)?, specifier)?;
        if !self.local.contains_key(&path) {
            return Err(invalid(&path, "resolved module was not prepared").into());
        }
        Ok(Resolution {
            filename: FileName::Custom(path.as_str().to_owned()),
            slug: None,
        })
    }
}

pub(super) struct NoMetadata;

impl Hook for NoMetadata {
    fn get_import_meta_props(
        &self,
        _span: Span,
        _module_record: &ModuleRecord,
    ) -> Result<Vec<KeyValueProp>, anyhow::Error> {
        anyhow::bail!("import.meta is not supported")
    }
}

fn logical(file: &FileName) -> Result<EntryPath, anyhow::Error> {
    match file {
        FileName::Custom(path) => path.parse().map_err(Into::into),
        _ => anyhow::bail!("bundler requested a non-logical source"),
    }
}

fn relative(base: &EntryPath, specifier: &str) -> Result<EntryPath, Error> {
    if !(specifier.starts_with("./") || specifier.starts_with("../")) {
        return Err(invalid(
            base,
            "only explicit relative module files are supported",
        ));
    }
    if specifier.contains(['\\', '%', '?', '#']) {
        return Err(invalid(
            base,
            "module specifier is not a literal relative file",
        ));
    }
    let mut components: Vec<_> = base.as_str().split('/').collect();
    components.pop();
    for component in specifier.split('/') {
        match component {
            "." => {}
            ".." => {
                if components.pop().is_none() {
                    return Err(invalid(base, "module import escapes captured source"));
                }
            }
            "" => return Err(invalid(base, "module import contains an empty component")),
            component => components.push(component),
        }
    }
    let path: EntryPath = components
        .join("/")
        .parse()
        .map_err(|_| invalid(base, "invalid relative module file"))?;
    syntax::language(&path)?;
    Ok(path)
}

pub(super) fn invalid(path: &EntryPath, reason: &'static str) -> Error {
    Error::Input {
        path: path.clone(),
        reason,
    }
}
