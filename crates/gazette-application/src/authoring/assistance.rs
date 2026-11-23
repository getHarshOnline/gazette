//! Source-bound projection of compiler-owned TOML authoring facts.

use gazette_compiler::workspace;
use gazette_core::{
    publication::routing::RoutePath,
    render::LimitedWriter,
    source::{EntryPath, Location, Sha256Digest},
};

use super::{Analysis, NavigationError};

/// Which compiler-owned declaration schema produced the assistance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AssistanceScope {
    /// Workspace configuration declaration.
    Workspace,
    /// Explicit TOML frontmatter in a selected authored document.
    AuthoredFrontmatter,
    /// Reduced owner-held legal source selected by an exact current workspace binding.
    LegalSourceFrontmatter,
}

/// Exact current TOML help shared by SDK, language-server and MCP clients.
#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct Assistance<'a> {
    /// Exact selected source path.
    pub path: &'a EntryPath,
    /// Complete current source identity.
    pub source_sha256: Sha256Digest,
    /// Native schema owner used for these facts.
    pub scope: AssistanceScope,
    /// Compiler-owned tolerant structure and generated schema facts.
    pub help: workspace::AuthoringHelp,
}

/// One explicit authored resource occurrence joined to its retained page companion.
///
/// The compiler owns TOML decoding, page-package membership and resource selection.
/// This association neither opens the file nor treats the compiled output as an
/// editable source document.
#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct PageResourceNavigation<'a> {
    /// Exact queried authored document.
    pub path: &'a EntryPath,
    /// Complete current authored-document identity.
    pub source_sha256: Sha256Digest,
    /// Exact TOML scalar construct beneath the cursor.
    pub occurrence: Location,
    /// Strict resource input selected by the decoded scalar.
    pub input: &'a EntryPath,
    /// Compiler-owned retained companion target.
    pub companion: &'a gazette_compiler::authored::PageCompanion,
    /// Every compiled destination reached by this input in declaration order.
    pub compiled_destinations: Vec<&'a RoutePath>,
}

impl Analysis {
    /// Current source fields for native editor presentation; does not change strict validity.
    ///
    /// # Errors
    /// Refuses changed or unknown source bytes, invalid retained compiler facts and
    /// assistance or structure results that exceed the requested byte bound.
    pub fn structure(
        &self,
        path: &EntryPath,
        source: &str,
        maximum: usize,
    ) -> Result<Vec<workspace::AuthoringStructureField>, NavigationError> {
        if self.inputs.get(path) != Some(&Sha256Digest::of(source.as_bytes())) {
            return Err(NavigationError::InvalidSource);
        }
        if let Some(value) = self.workspace_authoring.get(path) {
            return value
                .as_ref()
                .map_err(map_error)?
                .structure(source, maximum)
                .map_err(|e| map_error(&e));
        }
        match self.authored_authoring.get(path) {
            Some(Ok(Some(value))) => value.structure(source, maximum).map_err(|e| map_error(&e)),
            Some(Err(e)) => Err(map_error(e)),
            _ => Ok(Vec::new()),
        }
    }

    /// Inspect exact current workspace or authored-frontmatter source at one byte position.
    ///
    /// Strict parsing remains the sole publication-validity owner. Tolerant help
    /// is retained for the same current bytes when strict syntax or semantics fail;
    /// no prior successful analysis, inferred filename, or transport catalog is used.
    ///
    /// # Errors
    /// Refuses unknown or changed sources, invalid byte coordinates, incomplete
    /// compiler facts, unknown schema paths, and oversized complete responses.
    pub fn assistance<'a>(
        &'a self,
        path: &EntryPath,
        source: &str,
        offset: usize,
        maximum_bytes: usize,
    ) -> Result<Option<Assistance<'a>>, NavigationError> {
        let (path, digest) = self
            .inputs
            .get_key_value(path)
            .ok_or(NavigationError::InvalidSource)?;
        if *digest != Sha256Digest::of(source.as_bytes()) || !source.is_char_boundary(offset) {
            return Err(NavigationError::InvalidSource);
        }
        let (scope, help) = if self.workspace_documents.contains(path) {
            let authoring = self
                .workspace_authoring
                .get(path)
                .ok_or(NavigationError::Unavailable)?
                .as_ref()
                .map_err(map_error)?;
            (
                AssistanceScope::Workspace,
                Some(
                    authoring
                        .help(source, offset, maximum_bytes)
                        .map_err(|error| map_error(&error))?,
                ),
            )
        } else {
            let Some(authoring) = self.authored_authoring.get(path) else {
                return Ok(None);
            };
            let Some(authoring) = authoring.as_ref().map_err(map_error)? else {
                return Ok(None);
            };
            (
                match authoring.schema() {
                    gazette_compiler::authored::AuthoredSchema::Page => {
                        AssistanceScope::AuthoredFrontmatter
                    }
                    gazette_compiler::authored::AuthoredSchema::LegalSource => {
                        AssistanceScope::LegalSourceFrontmatter
                    }
                },
                authoring
                    .help(source, offset, maximum_bytes)
                    .map_err(|error| map_error(&error))?,
            )
        };
        let Some(help) = help else {
            return Ok(None);
        };
        if help.completion_location.is_none() && help.hover.is_none() {
            return Ok(None);
        }
        let result = Assistance {
            path,
            source_sha256: *digest,
            scope,
            help,
        };
        let mut bounded = LimitedWriter::new(std::io::sink(), maximum_bytes);
        serde_json::to_writer(&mut bounded, &result).map_err(|_| NavigationError::Limit)?;
        if bounded.exceeded() {
            return Err(NavigationError::Limit);
        }
        Ok(Some(result))
    }

    /// Resolve an authored `resources[].inputs[]` or `resources[].build.entry`
    /// string to the exact retained page-package companion it selects.
    ///
    /// Strict resource declarations and page-package ownership remain the only
    /// semantic owners. Tolerant TOML facts merely locate and decode the current
    /// scalar; arbitrary strings and resources outside the package return none.
    ///
    /// # Errors
    /// Refuses unknown or changed source, invalid coordinates, unavailable native
    /// authoring evidence or an invalid current page-package binding.
    pub fn page_resource_navigation<'a>(
        &'a self,
        path: &EntryPath,
        source: &str,
        offset: usize,
    ) -> Result<Option<PageResourceNavigation<'a>>, NavigationError> {
        use workspace::AuthoringPathSegment::{ArrayItem, Field};

        let (path, digest) = self.navigation_source(path, source, offset)?;
        let Some(authoring) = self.authored_authoring.get(path) else {
            return Ok(None);
        };
        let Some(occurrence) = authoring.as_ref().map_err(map_error)?.as_ref() else {
            return Ok(None);
        };
        let Some(occurrence) = occurrence
            .string_occurrence(source, offset)
            .map_err(|error| map_error(&error))?
        else {
            return Ok(None);
        };
        let input_position = matches!(
            occurrence.path.as_slice(),
            [Field(resources), ArrayItem, Field(inputs), ArrayItem]
                if resources == "resources" && inputs == "inputs"
        );
        let entry_position = matches!(
            occurrence.path.as_slice(),
            [Field(resources), ArrayItem, Field(build), Field(entry)]
                if resources == "resources" && build == "build" && entry == "entry"
        );
        if !input_position && !entry_position {
            return Ok(None);
        }
        let Ok(selected) = occurrence.value.parse::<EntryPath>() else {
            return Ok(None);
        };
        let Some(ownership) = self.page_package_ownership(path, digest)? else {
            return Ok(None);
        };
        let mut input = None;
        let mut compiled_destinations = Vec::new();
        for resource in ownership.resources {
            let selected_input = if entry_position {
                (resource.build.entry() == &selected).then_some(resource.build.entry())
            } else {
                resource
                    .inputs
                    .iter()
                    .find(|candidate| *candidate == &selected)
            };
            if let Some(selected_input) = selected_input {
                input.get_or_insert(selected_input);
                compiled_destinations.push(&resource.output);
            }
        }
        let Some(input) = input else {
            return Ok(None);
        };
        let prefix = ownership
            .package
            .root()
            .map_or_else(String::new, |root| format!("{root}/"));
        let Some(relative) = input.as_str().strip_prefix(&prefix) else {
            return Ok(None);
        };
        let companion = ownership
            .package
            .companions()
            .iter()
            .find(|companion| companion.relative().as_str() == relative);
        let Some(companion) = companion else {
            return Ok(None);
        };
        Ok(Some(PageResourceNavigation {
            path,
            source_sha256: digest,
            occurrence: occurrence.location,
            input,
            companion,
            compiled_destinations,
        }))
    }
}

fn map_error(error: &workspace::AuthoringError) -> NavigationError {
    match error {
        workspace::AuthoringError::Limit(_) => NavigationError::Limit,
        workspace::AuthoringError::Invalid => NavigationError::Unavailable,
    }
}
