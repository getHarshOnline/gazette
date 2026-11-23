//! Compile explicitly selected browser modules from retained inputs, without native effects.
//!
//! This is syntax compilation and TypeScript erasure, not type checking or a
//! security sandbox. Parser AST allocations are not bounded by the output writer.
//! Source ownership and capability selection remain with the caller.

mod emit;
mod modules;
mod syntax;
mod validation;

use std::collections::{BTreeMap, BTreeSet, HashMap};

use gazette_core::resource::{Artifact, Error, Limits, ModuleLinks, Recipe};
use gazette_core::source::TreeInput;
use gazette_ports::resource::ResourceCompiler;
use swc_core::bundler::{BundleKind, Bundler, Config};
use swc_core::common::{FileName, GLOBALS, Globals, SourceMap, sync::Lrc};

/// Rust-native browser ESM compiler over captured files and explicit module links.
#[derive(Clone, Copy, Debug, Default)]
pub struct Compiler;

impl ResourceCompiler for Compiler {
    fn compile(
        &self,
        source: &TreeInput<'_>,
        recipe: &Recipe,
        links: &ModuleLinks,
        limits: Limits,
    ) -> Result<Artifact, Error> {
        let Recipe::BrowserModule { imports, .. } = recipe else {
            return Err(Error::Input {
                path: recipe.entry().clone(),
                reason: "JavaScript compiler requires a browser-module recipe",
            });
        };
        recipe.validate_imports()?;
        let expected: BTreeSet<_> = imports.values().collect();
        if expected != links.keys().collect() {
            return Err(modules::invalid(
                recipe.entry(),
                "module links must match declared identities exactly",
            ));
        }
        validation::links(recipe.entry(), links, limits.output_bytes)?;
        let aliases: BTreeMap<_, _> = imports
            .iter()
            .map(|(alias, key)| (alias.clone(), links[key].clone()))
            .collect();
        limits.validate_input(source.inventory())?;
        let globals = Globals::new();
        GLOBALS.set(&globals, || {
            let cm: Lrc<SourceMap> = Default::default();
            let modules = modules::Modules::prepare(source, recipe.entry(), &aliases, cm.clone())?;
            let dependencies = modules.paths();
            let mut bundler = Bundler::new(
                &globals,
                cm.clone(),
                &modules,
                &modules,
                Config {
                    external_modules: links.values().map(|url| url.as_str().into()).collect(),
                    ..Config::default()
                },
                Box::new(modules::NoMetadata),
            );
            // One entry avoids bundler HashMap entry-order dependence. Its name
            // is fixed and no native path enters the parser's FileName::Real route.
            let entries = HashMap::from([(
                "entry".to_owned(),
                FileName::Custom(recipe.entry().as_str().to_owned()),
            )]);
            let mut bundles = bundler
                .bundle(entries)
                .map_err(|error| Error::Compile(format!("browser bundle: {error:#}")))?;
            if bundles.len() != 1 {
                return Err(Error::Compile(
                    "unexpected additional browser bundles".into(),
                ));
            }
            let bundle = bundles
                .pop()
                .ok_or_else(|| Error::Compile("browser entry emitted no bundle".into()))?;
            if !matches!(&bundle.kind, BundleKind::Named { name } if name == "entry") {
                return Err(Error::Compile(
                    "browser entry emitted an unexpected bundle".into(),
                ));
            }
            let mut references: BTreeSet<_> =
                syntax::imports(&bundle.module)?.into_iter().collect();
            let deferred_references: BTreeSet<_> = syntax::deferred_imports(&bundle.module)
                .into_iter()
                .filter(|url| !references.contains(url))
                .collect();
            references.extend(deferred_references.iter().cloned());
            if references
                .iter()
                .any(|url| !links.values().any(|linked| linked == url))
            {
                return Err(Error::Compile(
                    "emitted module contains an unselected external import".into(),
                ));
            }
            Ok(Artifact {
                input: source.inventory().identity(),
                bytes: emit::javascript(
                    &bundle.module,
                    cm,
                    modules.notices(),
                    limits.output_bytes,
                )?,
                dependencies,
                references,
                deferred_references,
                svg: None,
                image: None,
                tool: "swc_core@80.0.0+oxc@0.95.0/browser-esm-v3-deferred",
            })
        })
    }
}
