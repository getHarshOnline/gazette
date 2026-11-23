//! Selected-Theme component facts projected over exact authored source bytes.

use std::borrow::Cow;

use gazette_compiler::{
    landing::UNNAMED_SECTION_REQUIREMENT,
    theme::{Component, Components},
    workspace::{AuthoringHelp, CompletionCandidate, CompletionKind},
};
use gazette_core::{
    markdown::ComponentName,
    render::LimitedWriter,
    source::{EntryPath, Location, Precision, Sha256Digest, Span},
};

use super::{Analysis, NavigationError};

/// One exact selected-Theme component name offered as a source replacement.
#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct ComponentCandidate<'a> {
    /// Theme-local identity; no cross-Theme or publication identity is implied.
    pub name: &'a ComponentName,
    /// Exact spelling to place in the component-name slot.
    pub replacement: Cow<'a, str>,
}

/// Structural component help. Rendering and interaction details stay Theme-owned.
#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct ComponentHover<'a> {
    /// Exact full-file component-name range whose contract is shown.
    pub location: Location,
    /// Exact selected-Theme component identity.
    pub name: &'a ComponentName,
    /// Winning Theme template entry.
    pub template: &'a EntryPath,
    /// Data-only Draft 2020-12 argument contract used by native admission.
    pub arguments: &'a serde_json::Value,
    /// Child section admission for a structured YAML landing component.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub landing_children: Option<LandingChildren<'a>>,
}

/// Native structural choices for nested landing sections.
#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct LandingChildren<'a> {
    /// Component names admitted by the current effective Theme.
    pub named_components: Vec<&'a ComponentName>,
    /// Constraint for ordinary groups without a component name.
    pub unnamed_section_requirement: &'static str,
}

/// Completion in either the component-name slot or its data-only argument table.
#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ComponentCompletion<'a> {
    /// Complete selected-Theme component catalog.
    Name {
        /// Exact full-file component-name slot.
        location: Location,
        /// Exact bytes currently occupying the name slot.
        preimage: &'a str,
        /// Complete selected-Theme catalog in stable component-name order.
        candidates: Vec<ComponentCandidate<'a>>,
    },
    /// Schema-derived property-name or closed-literal replacements.
    Argument {
        /// Exact full-file argument replacement slot.
        location: Location,
        /// Exact bytes currently occupying the replacement slot.
        preimage: String,
        /// Whether candidates replace a field name or literal value.
        completion_kind: CompletionKind,
        /// Complete candidates from the selected component argument schema.
        candidates: Vec<CompletionCandidate>,
    },
}

/// Complete source-bound selected-Theme component assistance.
#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct ComponentAssistance<'a> {
    /// Exact selected authored source.
    pub path: &'a EntryPath,
    /// Complete current source identity.
    pub source_sha256: Sha256Digest,
    /// Exact current completion, absent when the cursor has no schema-derived replacement.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion: Option<ComponentCompletion<'a>>,
    /// Present only when the current name exactly selects one definition.
    pub hover: Option<ComponentHover<'a>>,
}

impl Analysis {
    /// Inspect one current Markdown component name or argument slot through the
    /// effective selected Theme.
    ///
    /// The result never discovers files, executes templates, exposes interaction
    /// implementation details or treats schema validity as publication approval.
    ///
    /// # Errors
    /// Refuses unknown/changed sources, invalid byte positions or incomplete output.
    pub fn component_assistance<'a>(
        &'a self,
        path: &EntryPath,
        source: &'a str,
        offset: usize,
        maximum_bytes: usize,
    ) -> Result<Option<ComponentAssistance<'a>>, NavigationError> {
        let (path, digest) = self
            .inputs
            .get_key_value(path)
            .ok_or(NavigationError::InvalidSource)?;
        if *digest != Sha256Digest::of(source.as_bytes()) || !source.is_char_boundary(offset) {
            return Err(NavigationError::InvalidSource);
        }
        if let Some(context) = self.landing_components.get(path) {
            return landing_assistance(context, path, *digest, source, offset, maximum_bytes);
        }
        let Some(context) = self.components.get(path) else {
            return Ok(None);
        };
        let Some(body) = context.body_span.slice(source) else {
            return Err(NavigationError::InvalidSource);
        };
        let mut selected = None;
        for directive in &context.directives {
            let header = Components::header(directive);
            let relative = directive
                .header
                .span
                .start
                .checked_add(header.name_range.start)
                .zip(
                    directive
                        .header
                        .span
                        .start
                        .checked_add(header.name_range.end),
                )
                .and_then(|(start, end)| Span::checked(body, start..end))
                .ok_or(NavigationError::InvalidSource)?;
            let full = relative
                .in_parent(source, context.body_span)
                .ok_or(NavigationError::InvalidSource)?;
            let header_full = directive
                .header
                .span
                .in_parent(source, context.body_span)
                .ok_or(NavigationError::InvalidSource)?;
            if header_full.start <= offset && offset <= header_full.end {
                let arguments = Span::checked(
                    body,
                    directive.header.span.start + header.arguments_range.start
                        ..directive.header.span.start + header.arguments_range.end,
                )
                .and_then(|span| span.in_parent(source, context.body_span))
                .ok_or(NavigationError::InvalidSource)?;
                selected = Some((header, full, arguments));
                break;
            }
        }
        let Some((header, name_span, arguments_span)) = selected else {
            return Ok(None);
        };
        let Some(definitions) = context.definitions.as_ref() else {
            return Ok(None);
        };
        let selected_name = header.name.parse::<ComponentName>().ok();
        let hover = selected_name.as_ref().and_then(|name| {
            definitions.definitions().find_map(
                |(candidate, definition): (&ComponentName, &Component)| {
                    (candidate == name).then(|| {
                        component_hover(
                            candidate,
                            definition,
                            Location {
                                span: name_span,
                                precision: Precision::Exact,
                            },
                        )
                    })
                },
            )
        });
        let completion = if name_span.start <= offset && offset <= name_span.end {
            Some(name_completion(
                definitions,
                source,
                Location {
                    span: name_span,
                    precision: Precision::Exact,
                },
                false,
            )?)
        } else if arguments_span.start <= offset && offset <= arguments_span.end {
            match selected_name {
                Some(name) => match definitions
                    .argument_help(
                        &name,
                        header.arguments,
                        offset - arguments_span.start,
                        context.authoring_limits,
                        maximum_bytes,
                    )
                    .map_err(|error| match error {
                        gazette_compiler::workspace::AuthoringError::Limit(_) => {
                            NavigationError::Limit
                        }
                        gazette_compiler::workspace::AuthoringError::Invalid => {
                            NavigationError::Unavailable
                        }
                    })? {
                    Some(help) => argument_completion(help, source, arguments_span)?,
                    None => None,
                },
                None => None,
            }
        } else {
            None
        };
        bounded_assistance(
            ComponentAssistance {
                path,
                source_sha256: *digest,
                completion,
                hover,
            },
            maximum_bytes,
        )
    }
}

fn landing_assistance<'a>(
    context: &'a super::LandingComponentContext,
    path: &'a EntryPath,
    digest: Sha256Digest,
    source: &'a str,
    offset: usize,
    maximum_bytes: usize,
) -> Result<Option<ComponentAssistance<'a>>, NavigationError> {
    let at = |span: Span| span.start <= offset && offset <= span.end;
    let definitions = &context.definitions;
    for slot in &context.slots {
        let (location, completion) = if at(slot.name_location.span) {
            let completion = if slot.name_location.precision == Precision::Exact {
                Some(name_completion(
                    definitions,
                    source,
                    slot.name_location,
                    true,
                )?)
            } else {
                None
            };
            (slot.name_location, completion)
        } else if let Some((argument, key, location)) = slot.arguments.iter().find_map(|argument| {
            if let Some(span) = argument.key.filter(|span| at(*span)) {
                Some((
                    argument,
                    true,
                    Location {
                        span,
                        precision: Precision::Exact,
                    },
                ))
            } else if at(argument.value.span) {
                Some((argument, false, argument.value))
            } else {
                None
            }
        }) {
            let candidates = if location.precision == Precision::Exact {
                match definitions.yaml_argument_candidates(
                    &slot.name,
                    &argument.name,
                    key,
                    slot.arguments.iter().map(|argument| argument.name.as_str()),
                ) {
                    Ok(candidates) => candidates,
                    Err(gazette_compiler::workspace::AuthoringError::Limit(_)) => {
                        return Err(NavigationError::Limit);
                    }
                    Err(gazette_compiler::workspace::AuthoringError::Invalid) => Vec::new(),
                }
            } else {
                Vec::new()
            };
            let completion = if candidates.is_empty() {
                None
            } else {
                Some(ComponentCompletion::Argument {
                    location,
                    preimage: location
                        .span
                        .slice(source)
                        .ok_or(NavigationError::InvalidSource)?
                        .to_owned(),
                    completion_kind: if key {
                        CompletionKind::Field
                    } else {
                        CompletionKind::Literal
                    },
                    candidates,
                })
            };
            (location, completion)
        } else {
            continue;
        };
        let hover = definitions.definitions().find_map(|(name, definition)| {
            (name == &slot.name).then(|| ComponentHover {
                landing_children: Some(LandingChildren {
                    named_components: definitions.definitions().map(|(name, _)| name).collect(),
                    unnamed_section_requirement: UNNAMED_SECTION_REQUIREMENT,
                }),
                ..component_hover(name, definition, location)
            })
        });
        return bounded_assistance(
            ComponentAssistance {
                path,
                source_sha256: digest,
                completion,
                hover,
            },
            maximum_bytes,
        );
    }
    Ok(None)
}

fn component_hover<'a>(
    name: &'a ComponentName,
    definition: &'a Component,
    location: Location,
) -> ComponentHover<'a> {
    ComponentHover {
        location,
        name,
        template: &definition.template,
        arguments: &definition.arguments,
        landing_children: None,
    }
}

fn name_completion<'a>(
    definitions: &'a Components,
    source: &'a str,
    location: Location,
    yaml: bool,
) -> Result<ComponentCompletion<'a>, NavigationError> {
    Ok(ComponentCompletion::Name {
        location,
        preimage: location
            .span
            .slice(source)
            .ok_or(NavigationError::InvalidSource)?,
        candidates: definitions
            .definitions()
            .map(|(name, _)| ComponentCandidate {
                name,
                replacement: if yaml {
                    Cow::Owned(format!("\"{name}\""))
                } else {
                    Cow::Borrowed(name.as_str())
                },
            })
            .collect(),
    })
}

fn bounded_assistance(
    result: ComponentAssistance<'_>,
    maximum_bytes: usize,
) -> Result<Option<ComponentAssistance<'_>>, NavigationError> {
    let mut bounded = LimitedWriter::new(std::io::sink(), maximum_bytes);
    serde_json::to_writer(&mut bounded, &result).map_err(|_| NavigationError::Limit)?;
    if bounded.exceeded() {
        return Err(NavigationError::Limit);
    }
    Ok(Some(result))
}

fn argument_completion(
    help: AuthoringHelp,
    source: &str,
    arguments: Span,
) -> Result<Option<ComponentCompletion<'static>>, NavigationError> {
    let (Some(location), Some(preimage), Some(completion_kind)) = (
        help.completion_location,
        help.completion_preimage,
        help.completion_kind,
    ) else {
        return Ok(None);
    };
    let span = location
        .span
        .in_parent(source, arguments)
        .ok_or(NavigationError::InvalidSource)?;
    Ok(Some(ComponentCompletion::Argument {
        location: Location {
            span,
            precision: location.precision,
        },
        preimage,
        completion_kind,
        candidates: help.candidates,
    }))
}
