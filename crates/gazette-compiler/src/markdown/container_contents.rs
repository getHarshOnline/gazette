//! Admit one structural placement without turning generated navigation into authored text.

use super::{ComponentPresentation, Error, ParsedMarkdown};
use crate::theme::{ComponentSemantic, Components};
use gazette_core::markdown::{
    ComponentBinding, ComponentHost, ComponentSurface, ContainerContentsBinding,
};

impl ParsedMarkdown<'_> {
    pub(crate) fn requests_container_contents(&self, definitions: &Components) -> bool {
        self.document.syntax().directives.iter().any(|directive| {
            Components::header(directive)
                .name
                .parse()
                .ok()
                .and_then(|name| definitions.definition(&name))
                .is_some_and(|definition| {
                    matches!(
                        definition.semantic,
                        Some(ComponentSemantic::ContainerContents)
                    )
                })
        })
    }
}

pub(super) fn bind(
    body: &ParsedMarkdown,
    selected: Option<ComponentPresentation<'_>>,
    host: Option<&ComponentHost>,
    namespace: &str,
    contents: Option<&ContainerContentsBinding>,
    bindings: &mut [ComponentBinding],
) -> Result<bool, Error> {
    let Some(selected) = selected else {
        return Ok(false);
    };
    let mut seen = false;
    for directive in &body.document.syntax().directives {
        let semantic = Components::header(directive)
            .name
            .parse()
            .ok()
            .and_then(|name| selected.definitions.definition(&name))
            .is_some_and(|definition| {
                matches!(
                    definition.semantic,
                    Some(ComponentSemantic::ContainerContents)
                )
            });
        if !semantic {
            continue;
        }
        let fail = |reason| {
            Error::Adapter(gazette_core::markdown::Error::Unsupported {
                location: directive.location,
                reason,
            })
        };
        if seen {
            return Err(fail("container contents may be placed only once"));
        }
        seen = true;
        if !directive.emitted || !namespace.is_empty() {
            return Err(fail(
                "container contents require the main emitted page body",
            ));
        }
        if !host.is_some_and(|host| {
            matches!(
                host.surface,
                ComponentSurface::Collection | ComponentSurface::Journey
            )
        }) {
            return Err(fail("container contents require a collection or journey"));
        }
        let binding = bindings
            .iter_mut()
            .find(|binding| binding.occurrence == *directive)
            .ok_or(Error::InvalidOutput)?;
        if !binding.arguments.is_empty() {
            return Err(fail("container contents do not accept arguments"));
        }
        let contents =
            contents.ok_or_else(|| fail("container contents require resolved page navigation"))?;
        let maximum = body.limits.output_bytes;
        let mut output = gazette_core::render::LimitedWriter::new(std::io::sink(), maximum);
        serde_json::to_writer(&mut output, &contents.presentation)
            .map_err(|_| Error::Limit("container contents context bytes"))?;
        if contents
            .reading_markdown
            .as_ref()
            .is_some_and(|text| text.len() > maximum)
        {
            return Err(Error::Limit("container contents Markdown bytes"));
        }
        binding.container_contents = Some(contents.clone());
    }
    Ok(seen)
}
