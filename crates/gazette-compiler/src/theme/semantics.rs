//! Explicit publication meaning for authored component bodies.

use serde::{Deserialize, Serialize};

/// A component's declared reading role, independent of its name or template path.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ComponentSemantic {
    /// One empty top-level placement of the hosting collection or journey contents.
    ContainerContents,
    /// One authored image with an explicitly nested caption.
    Figure,
    /// The admitted Markdown caption of its enclosing declared figure.
    FigureCaption,
    /// An ordered group of authored questions and answers.
    Faq,
    /// One question argument and the component's admitted Markdown answer body.
    FaqItem {
        /// Exact top-level argument containing the question as plain text.
        question_argument: String,
    },
}
