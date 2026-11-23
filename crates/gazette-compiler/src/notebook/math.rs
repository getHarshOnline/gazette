//! Fragment-local syntax and retained typesetting, never a concatenated notebook document.

use gazette_core::markdown::{Identity, Math, MathBinding, Profile};
use serde_json::Value;

use super::{Error, ParsedNotebook};
use crate::{
    corpus::{LocatedMathResult, MathLocation},
    markdown::{self, ParsedMarkdown},
};

pub(super) struct Fragment {
    pub identity: Identity,
    pub facts: Vec<Math>,
    pub bindings: Vec<MathBinding>,
}

impl<'s> ParsedNotebook<'s> {
    /// Parse notebook syntax and each Markdown fragment under one explicit selected profile.
    /// Node limits cover the complete notebook, not an independently renewed cell allowance.
    ///
    /// # Errors
    /// Refuses notebook structure, fragment parsing or aggregate Markdown limits.
    pub fn parse_with_profile(
        source: &'s [u8],
        profile: Profile,
        limits: super::Limits,
        markdown_limits: markdown::Limits,
        adapter: &dyn gazette_ports::markdown::Markdown,
    ) -> Result<Self, Error> {
        let mut notebook = Self::parse(source, limits)?;
        notebook.profile = profile;
        notebook.markdown_limits = markdown_limits;
        let cells = notebook.value["cells"].as_array().ok_or(Error::Structure)?;
        let mut remaining = markdown_limits.nodes;
        for (cell_index, cell) in cells.iter().enumerate() {
            let mut admit = |location, value: &Value| -> Result<(), Error> {
                let source = super::render::multiline(value, markdown_limits.source_bytes)
                    .map_err(|error| match error {
                        super::RenderError::Limit(_) => Error::Limit("Markdown source bytes"),
                        _ => Error::Structure,
                    })?;
                let parsed = ParsedMarkdown::parse_with_profile(
                    &source,
                    profile,
                    markdown::Limits {
                        nodes: remaining,
                        ..markdown_limits
                    },
                    adapter,
                )
                .map_err(Error::Markdown)?;
                remaining = remaining
                    .checked_sub(parsed.node_count())
                    .ok_or(Error::Limit("Markdown nodes"))?;
                notebook.fragments.insert(
                    location,
                    Fragment {
                        identity: parsed.identity(),
                        facts: parsed.math().to_vec(),
                        bindings: Vec::new(),
                    },
                );
                Ok(())
            };
            match cell["cell_type"].as_str() {
                Some("markdown") => admit(
                    MathLocation::NotebookCell { cell: cell_index },
                    &cell["source"],
                )?,
                Some("code") => {
                    for (output, value) in cell["outputs"]
                        .as_array()
                        .ok_or(Error::Structure)?
                        .iter()
                        .enumerate()
                    {
                        if let Some(source) =
                            value.get("data").and_then(|data| data.get("text/markdown"))
                        {
                            admit(
                                MathLocation::NotebookOutput {
                                    cell: cell_index,
                                    output,
                                },
                                source,
                            )?;
                        }
                    }
                }
                _ => (),
            }
        }
        notebook.markdown_nodes = markdown_limits.nodes - remaining;
        Ok(notebook)
    }

    /// Aggregate parsed Markdown nodes for corpus-wide accounting, separate from JSON nodes.
    pub fn markdown_node_count(&self) -> usize {
        self.markdown_nodes
    }

    /// Exact source/profile and native occurrences for independently acquired typesetting.
    pub fn math_sources(&self) -> impl Iterator<Item = (MathLocation, Identity, &[Math])> {
        self.fragments
            .iter()
            .map(|(location, fragment)| (*location, fragment.identity, fragment.facts.as_slice()))
    }

    /// Consume a complete exact emitted-fragment set without partial notebook success.
    /// Generated `MathML` is additionally admitted by the existing renderer before emission.
    ///
    /// # Errors
    /// Refuses duplicate/missing/extra locations, changed source/profile/occurrences or limits.
    pub fn with_math(mut self, results: Vec<LocatedMathResult>) -> Result<Self, Error> {
        let invalid = || {
            Error::Markdown(markdown::Error::Adapter(
                gazette_core::markdown::Error::InvalidPlan,
            ))
        };
        let expected = self
            .fragments
            .values()
            .filter(|fragment| fragment.facts.iter().any(|fact| fact.emitted))
            .count();
        if expected != results.len() {
            return Err(invalid());
        }
        let mut seen = std::collections::BTreeSet::new();
        let mut remaining = self.markdown_limits.output_bytes;
        for located in results {
            if !seen.insert(located.location) {
                return Err(invalid());
            }
            let fragment = self
                .fragments
                .get_mut(&located.location)
                .ok_or_else(invalid)?;
            let result = located.result;
            let facts = fragment.facts.iter().filter(|fact| fact.emitted);
            if result.identity != fragment.identity
                || result.bindings.is_empty()
                || facts.clone().count() != result.bindings.len()
                || facts
                    .zip(&result.bindings)
                    .any(|(fact, binding)| fact != &binding.occurrence)
            {
                return Err(invalid());
            }
            for binding in &result.bindings {
                remaining = remaining
                    .checked_sub(binding.mathml.len())
                    .and_then(|bytes| bytes.checked_sub(binding.occurrence.tex.len()))
                    .ok_or(Error::Limit("math binding bytes"))?;
            }
            fragment.bindings = result.bindings;
        }
        Ok(self)
    }
}
