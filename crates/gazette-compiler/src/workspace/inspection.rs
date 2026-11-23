//! One typed parse preserves source facts without admitting an invalid workspace.

use super::{Compilation, Error, WorkspaceNavigation, resolve_declaration};
use crate::toml::{self, ParsedToml};

/// Independent resolution and source-coordinate results from the same input.
/// Invalid meaning never yields a partial compilation; unambiguous typed source
/// facts can still support repair proposals. This opens no native resources.
#[derive(Debug)]
pub struct WorkspaceInspection<'s> {
    pub(super) syntax: ParsedToml<'s>,
    pub(super) compilation: Result<Compilation, Error>,
    pub(super) navigation: Result<WorkspaceNavigation, Error>,
}

impl<'s> WorkspaceInspection<'s> {
    /// Decode once, then independently resolve meaning and retain source facts.
    ///
    /// # Errors
    /// Rejects invalid syntax, schema and parser limits before exposing facts.
    /// Semantic and coordinate failures remain in the respective results.
    pub fn parse(source: &'s str, limits: toml::Limits) -> Result<Self, Error> {
        let syntax = ParsedToml::parse(source, limits).map_err(Error::Toml)?;
        let declaration = super::publication_defaults::read(&syntax)?;
        let navigation = WorkspaceNavigation::build(&syntax, &declaration, limits);
        let compilation = resolve_declaration(&syntax, declaration, limits);
        Ok(Self {
            syntax,
            compilation,
            navigation,
        })
    }

    /// Move the results without retaining syntax or duplicating resolved values.
    pub fn into_parts(
        self,
    ) -> (
        Result<Compilation, Error>,
        Result<WorkspaceNavigation, Error>,
    ) {
        (self.compilation, self.navigation)
    }
}
