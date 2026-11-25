//! Static component presentation over already resolved data and captured templates.

use gazette_compiler::{html::Limits, theme::ComponentSemantic};
use gazette_core::markdown::{ComponentBinding, Error, Fragment};
use gazette_ports::markdown::ComponentRenderer;
use serde::Serialize;

use super::CompiledTheme;

#[derive(Serialize)]
struct Context<'a> {
    component: Component<'a>,
    presentation: Presentation<'a>,
}

#[derive(Serialize)]
struct Presentation<'a> {
    chrome: Chrome<'a>,
}
#[derive(Serialize)]
struct Chrome<'a> {
    regions: &'a std::collections::BTreeMap<gazette_core::publication::Key, String>,
}

#[derive(Serialize)]
struct Component<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    container_contents: Option<&'a gazette_core::markdown::ContainerContentsView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<&'a gazette_core::markdown::ComponentContext>,
    name: &'a gazette_core::markdown::ComponentName,
    arguments: &'a std::collections::BTreeMap<String, serde_json::Value>,
    body: Body<'a>,
    excerpt: bool,
}

#[derive(Serialize)]
struct Body<'a> {
    html: &'a str,
    text: &'a str,
    link_destinations: &'a [String],
    images: &'a [gazette_core::markdown::FragmentImage],
}

impl ComponentRenderer for CompiledTheme {
    fn initial_color_mode(&self) -> gazette_core::markdown::InitialColorMode {
        self.initial_color_mode
    }

    fn render(
        &self,
        binding: &ComponentBinding,
        body: &Fragment,
        excerpt: bool,
    ) -> Result<String, Error> {
        let failure = || Error::Unsupported {
            location: binding.occurrence.location,
            reason: "selected component template failed; inspect the captured Theme and its argument contract",
        };
        let definition = self
            .components
            .definition(&binding.name)
            .ok_or_else(failure)?;
        let context = Context {
            presentation: Presentation {
                chrome: Chrome {
                    regions: &self.component_regions,
                },
            },
            component: Component {
                container_contents: binding
                    .container_contents
                    .as_ref()
                    .map(|contents| &contents.presentation),
                context: binding.context.as_ref(),
                name: &binding.name,
                arguments: &binding.arguments,
                body: Body {
                    html: &body.html,
                    text: &body.text,
                    link_destinations: &body.link_destinations,
                    images: &body.images,
                },
                excerpt,
            },
        };
        let bytes = super::serialization::encode(
            &context,
            self.evidence().limits.context_bytes,
            "component context bytes",
        )
        .map_err(|_| failure())?;
        let mut context: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| failure())?;
        context.sort_all_objects();
        let output = self
            .render(&definition.template, &context)
            .map_err(|_| failure())?;
        let output = String::from_utf8(output).map_err(|_| failure())?;
        if !excerpt && let Some(semantic) = &definition.semantic {
            type Validator = fn(
                &ComponentBinding,
                &ComponentSemantic,
                &Fragment,
                &str,
                Limits,
            ) -> Result<(), Error>;
            let validate: Option<Validator> = match semantic {
                ComponentSemantic::Figure | ComponentSemantic::FigureCaption => {
                    Some(gazette_compiler::markdown::validate_figure_html)
                }
                ComponentSemantic::Faq | ComponentSemantic::FaqItem { .. } => {
                    Some(gazette_compiler::markdown::validate_faq_html)
                }
                ComponentSemantic::Catalogue
                | ComponentSemantic::CatalogueEntry
                | ComponentSemantic::NavigationAction
                | ComponentSemantic::ContainerContents => None,
            };
            if let Some(validate) = validate {
                validate(
                    binding,
                    semantic,
                    body,
                    &output,
                    Limits {
                        input_bytes: self.evidence().limits.output_bytes,
                        decoded_bytes: self.evidence().limits.output_bytes,
                        ..Default::default()
                    },
                )?;
            }
        }
        if excerpt || definition.interaction.is_none() {
            return Ok(output);
        }
        let props = super::serialization::encode(
            &binding.arguments,
            self.evidence().limits.context_bytes,
            "component props bytes",
        )
        .map_err(|_| failure())?;
        let props = std::str::from_utf8(&props).map_err(|_| failure())?;
        let mut html = gazette_core::render::LimitedWriter::new(
            Vec::new(),
            self.evidence().limits.output_bytes,
        );
        use std::io::Write;
        write!(html, "<div data-gazette-component=\"{}\"", binding.name).map_err(|_| failure())?;
        if let Some(context) = &binding.context {
            html.write_all(b" data-gazette-component-address=\"")
                .map_err(|_| failure())?;
            escape_attribute(&mut html, &context.address).map_err(|_| failure())?;
            html.write_all(b"\"").map_err(|_| failure())?;
        }
        html.write_all(b" data-gazette-state=\"pending\" data-gazette-props=\"")
            .map_err(|_| failure())?;
        // JSON is data inside a quoted attribute, never executable script text.
        escape_attribute(&mut html, props).map_err(|_| failure())?;
        write!(
            html,
            "\"><div data-gazette-fallback>{output}</div><div data-gazette-mount></div></div>"
        )
        .map_err(|_| failure())?;
        String::from_utf8(html.into_inner()).map_err(|_| failure())
    }
}

fn escape_attribute(output: &mut impl std::io::Write, value: &str) -> std::io::Result<()> {
    for ch in value.chars() {
        match ch {
            '&' => output.write_all(b"&amp;"),
            '"' => output.write_all(b"&quot;"),
            '<' => output.write_all(b"&lt;"),
            '>' => output.write_all(b"&gt;"),
            _ => write!(output, "{ch}"),
        }?;
    }
    Ok(())
}
