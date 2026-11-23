//! Machine-readable contract schemas projected from their native typed owners.

use std::process::ExitCode;

use clap::{Args as ClapArgs, Subcommand};
use gazette_sdk::schema::{self, Contract};

#[derive(ClapArgs)]
pub(crate) struct Args {
    #[command(subcommand)]
    contract: Selection,
}

#[derive(Clone, Copy, Subcommand)]
enum Selection {
    /// Complete Theme-package manifest accepted by the compiler.
    ThemeManifest,
    /// Exact page context serialized for Theme templates.
    TemplateContext,
    /// Complete unresolved workspace declaration accepted by the compiler.
    Workspace,
    /// Exact authored publication frontmatter accepted by the compiler.
    AuthoredContent,
    /// Reduced owner-held legal-source frontmatter, separate from site output bindings.
    LegalSource,
}

impl From<Selection> for Contract {
    fn from(value: Selection) -> Self {
        match value {
            Selection::ThemeManifest => Self::ThemeManifest,
            Selection::TemplateContext => Self::TemplateContext,
            Selection::Workspace => Self::Workspace,
            Selection::AuthoredContent => Self::AuthoredContent,
            Selection::LegalSource => Self::LegalSource,
        }
    }
}

pub(crate) fn run(input: Args) -> ExitCode {
    super::write_json(&schema::contract(input.contract.into()))
}
