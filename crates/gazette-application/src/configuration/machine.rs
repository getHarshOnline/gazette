//! Read named roots from the workspace's already captured machine binding.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use gazette_compiler::toml;
use gazette_core::source::{CapturedSource, EntryPath};

use super::Error;

#[derive(serde::Deserialize)]
struct Binding {
    #[serde(default)]
    roots: BTreeMap<String, String>,
}

pub(super) fn roots(
    workspace: &Path,
    path: &EntryPath,
    captured: &CapturedSource,
    limits: toml::Limits,
) -> Result<BTreeMap<String, PathBuf>, Error> {
    let invalid = |message: &str| toml::Error::Invalid {
        message: message.to_owned(),
        span: None,
    };
    let parse = || -> Result<BTreeMap<String, PathBuf>, toml::Error> {
        let bytes = captured
            .file(path)
            .ok_or_else(|| invalid("selected capture omitted the machine binding"))?;
        let source =
            std::str::from_utf8(bytes).map_err(|_| invalid("machine binding is not UTF-8"))?;
        let syntax = toml::ParsedToml::parse(source, limits)?;
        let binding: Binding = syntax.deserialize()?;
        let binding_path = workspace.join(path.as_str());
        let parent = binding_path
            .parent()
            .expect("workspace-relative binding file");
        binding
            .roots
            .into_iter()
            .map(|(name, value)| {
                if value.is_empty() || value.chars().any(char::is_control) {
                    return Err(invalid(
                        "machine roots must be nonempty native paths without controls",
                    ));
                }
                // Joining an absolute path keeps it absolute; relative values use the binding's parent.
                Ok((name, parent.join(value)))
            })
            .collect()
    };
    parse().map_err(|error| Error::MachineBinding {
        path: path.clone(),
        error,
    })
}
