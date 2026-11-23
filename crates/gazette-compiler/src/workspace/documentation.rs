//! Authored-manual and generated-reference selection for one documentation repository.

use super::{Brand, Diagnostic, Key, Source};
use gazette_core::{
    documentation::{Format, ReferencePage, Selection, Target},
    source::EntryPath,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Complete target selection stored once beside the brand-owned repository name/path.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocumentationBuild {
    /// Explicit supported target; the adapter owns detailed compatibility validation.
    #[serde(deserialize_with = "crate::tables::one")]
    pub target: Target,
    /// Navigation order is configured order, independent from source acquisition order.
    #[serde(default, deserialize_with = "crate::tables::many")]
    pub modules: Vec<DocumentationModule>,
}

/// One authored product manual selected from a retained workspace source.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocumentationManual {
    /// Exact existing source binding containing every selected page and asset.
    pub source: Key,
    /// Explicit companion assets; no directory is published merely because it was captured.
    #[serde(default, deserialize_with = "crate::tables::many")]
    pub assets: Vec<DocumentationManualAsset>,
}

/// One explicitly selected authored Markdown page.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocumentationManualPage {
    /// Explicit page navigation label.
    pub title: String,
    /// Extensionless target route; source layout does not choose it.
    pub route: EntryPath,
    /// Exact retained Markdown file.
    #[serde(deserialize_with = "crate::tables::one")]
    pub input: Selection,
}

/// One explicit manual companion selection and destination.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocumentationManualAsset {
    /// Exact retained file or directory.
    #[serde(deserialize_with = "crate::tables::one")]
    pub input: Selection,
    /// Exact target path for a file or target root for a directory.
    pub path: EntryPath,
}

/// One project module in a documentation publication, separate from Gazette content admission.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocumentationModule {
    /// Exact existing project identity.
    pub project: Key,
    /// Target navigation namespace; not a new project identity.
    pub slug: String,
    /// Explicit module navigation text.
    pub title: String,
    /// Optional authored-manual source and assets for this project module.
    #[serde(default)]
    pub manual: Option<DocumentationManual>,
    /// Complete ordered authored-manual sections and generated references.
    #[serde(deserialize_with = "crate::tables::many")]
    pub navigation: Vec<DocumentationNavigation>,
}

/// One ordered project documentation item. Shared placement does not merge source semantics.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum DocumentationNavigation {
    /// Authored human-readable pages from the module's manual source.
    Manual {
        /// Explicit section navigation label.
        title: String,
        /// Complete ordered page selection.
        #[serde(deserialize_with = "crate::tables::many")]
        pages: Vec<DocumentationManualPage>,
    },
    /// One selected native generator output; no format guessing.
    Reference {
        /// Namespace within its project module.
        slug: String,
        /// Explicit navigation text.
        title: String,
        /// Capturing a generated artifact is distinct from executing its generator.
        #[serde(deserialize_with = "crate::tables::one")]
        input: Box<DocumentationInput>,
        /// Optional complete ordered navigation for a selected Markdown directory.
        /// Omission preserves artifact-owned navigation; explicit null is refused.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "reference_pages"
        )]
        #[schemars(with = "Vec<ReferencePage>")]
        pages: Option<Vec<ReferencePage>>,
    },
}

impl DocumentationNavigation {
    /// Borrow a generated-reference input; manual items deliberately have none.
    pub fn reference(&self) -> Option<(&str, &str, &DocumentationInput)> {
        match self {
            Self::Reference {
                slug, title, input, ..
            } => Some((slug, title, input.as_ref())),
            Self::Manual { .. } => None,
        }
    }
}

fn reference_pages<'de, D: serde::Deserializer<'de>>(
    decoder: D,
) -> Result<Option<Vec<ReferencePage>>, D::Error> {
    // Each recursive node owns its map-only boundary in the core model.
    Vec::<ReferencePage>::deserialize(decoder).map(Some)
}

/// Exact input acquisition mode; unsupported generator execution is never simulated.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum DocumentationInput {
    /// Read already generated output through an existing exact-pin source binding.
    Artifact {
        /// Source namespace, retaining repository/gitlink verification separately.
        source: Key,
        /// Explicit file or directory beneath the selected source capture.
        selection: Selection,
        /// Native generator format, not a hosting-provider name.
        format: Format,
    },
    /// Invoke one explicitly selected native tool from the retained project source.
    Generated {
        /// Exact source binding, not an ambient repository search.
        source: Key,
        /// Portable executable and explicit literal/path environment; target conversion stays separate.
        #[serde(deserialize_with = "crate::tables::one")]
        recipe: gazette_core::documentation::generator::Recipe<super::RuntimeArgument>,
    },
}

impl DocumentationInput {
    /// Source selected for either import or native generation.
    pub fn source(&self) -> &Key {
        match self {
            Self::Artifact { source, .. } | Self::Generated { source, .. } => source,
        }
    }
}

pub(super) fn validate(
    brands: &BTreeMap<Key, Brand>,
    projects: &BTreeMap<Key, super::ProjectDeclaration>,
    sources: &BTreeMap<Key, Source>,
    errors: &mut Vec<Diagnostic>,
) {
    for brand in brands.values() {
        for publication in &brand.publications {
            let Some(build) = publication
                .documentation_repository
                .as_ref()
                .and_then(|r| r.build.as_ref())
            else {
                continue;
            };
            let field = format!(
                "brands[{:?}].publications[{:?}].documentation_repository.build",
                brand.id.as_str(),
                publication.site.as_str()
            );
            if build.modules.is_empty() {
                super::resolve::report(errors, "workspace_documentation_empty", &field, None);
            }
            let mut ids = BTreeSet::new();
            let mut slugs = BTreeSet::new();
            for (i, module) in build.modules.iter().enumerate() {
                let field = format!("{field}.modules[{i}]");
                if !projects.contains_key(&module.project) {
                    super::resolve::report(
                        errors,
                        "workspace_reference",
                        &format!("{field}.project"),
                        Some(&module.project),
                    );
                }
                if !ids.insert(&module.project) || !slugs.insert(&module.slug) {
                    super::resolve::report(
                        errors,
                        "workspace_duplicate",
                        &field,
                        Some(&module.project),
                    );
                }
                labels(&module.slug, &module.title, &field, errors);
                if module.navigation.is_empty() {
                    super::resolve::report(errors, "workspace_documentation_empty", &field, None);
                }
                let has_manual = module
                    .navigation
                    .iter()
                    .any(|item| matches!(item, DocumentationNavigation::Manual { .. }));
                match (&module.manual, has_manual) {
                    (Some(manual), true) => validate_manual(manual, sources, &field, errors),
                    (None, true) | (Some(_), false) => super::resolve::report(
                        errors,
                        "workspace_documentation_manual_binding",
                        &format!("{field}.manual"),
                        None,
                    ),
                    (None, false) => {}
                }
                validate_navigation(module, sources, &field, errors);
            }
        }
    }
}

fn validate_manual(
    manual: &DocumentationManual,
    sources: &BTreeMap<Key, Source>,
    field: &str,
    errors: &mut Vec<Diagnostic>,
) {
    let field = format!("{field}.manual");
    if !sources.contains_key(&manual.source) {
        super::resolve::report(
            errors,
            "workspace_reference",
            &format!("{field}.source"),
            Some(&manual.source),
        );
    }
    let mut assets = BTreeSet::new();
    for (asset_index, asset) in manual.assets.iter().enumerate() {
        let asset_field = format!("{field}.assets[{asset_index}]");
        if !assets.insert(asset.path.as_str()) {
            super::resolve::report(errors, "workspace_duplicate", &asset_field, None);
        }
    }
}

fn validate_navigation(
    module: &DocumentationModule,
    sources: &BTreeMap<Key, Source>,
    field: &str,
    errors: &mut Vec<Diagnostic>,
) {
    let mut routes = BTreeSet::new();
    let mut references = BTreeSet::new();
    for (item_index, item) in module.navigation.iter().enumerate() {
        let item_field = format!("{field}.navigation[{item_index}]");
        match item {
            DocumentationNavigation::Manual { title, pages } => {
                labels("section", title, &item_field, errors);
                if pages.is_empty() {
                    super::resolve::report(
                        errors,
                        "workspace_documentation_empty",
                        &item_field,
                        None,
                    );
                }
                for (page_index, page) in pages.iter().enumerate() {
                    let page_field = format!("{item_field}.pages[{page_index}]");
                    labels(page.route.as_str(), &page.title, &page_field, errors);
                    if !matches!(page.input, Selection::File { .. })
                        || !routes.insert(page.route.as_str())
                    {
                        super::resolve::report(
                            errors,
                            "workspace_documentation_manual_page",
                            &page_field,
                            None,
                        );
                    }
                }
            }
            DocumentationNavigation::Reference {
                slug, title, input, ..
            } => {
                labels(slug, title, &item_field, errors);
                if !references.insert(slug) {
                    super::resolve::report(errors, "workspace_duplicate", &item_field, None);
                }
                let source = input.source();
                if !sources.contains_key(source) {
                    super::resolve::report(
                        errors,
                        "workspace_reference",
                        &format!("{item_field}.input.source"),
                        Some(source),
                    );
                }
                if let DocumentationInput::Generated { recipe, .. } = input.as_ref()
                    && validate_generator_recipe(recipe).is_err()
                {
                    super::resolve::report(
                        errors,
                        "workspace_documentation_generator",
                        &format!("{item_field}.input.recipe"),
                        None,
                    );
                }
            }
        }
    }
}

fn validate_generator_recipe(
    recipe: &gazette_core::documentation::generator::Recipe<super::RuntimeArgument>,
) -> Result<(), gazette_core::process::Error> {
    super::RuntimeCommand::coordinate(&recipe.program)?;
    for argument in recipe
        .arguments
        .iter()
        .chain(recipe.validation_arguments.iter().flatten())
    {
        if let gazette_core::documentation::generator::Argument::Path { path } = argument {
            super::RuntimeCommand::coordinate(path)?;
        }
    }
    recipe
        .try_map_runtime_values(|value, _| match value {
            super::RuntimeArgument::Literal(value) => Ok(value.clone()),
            super::RuntimeArgument::Path { path } => {
                super::RuntimeCommand::coordinate(path)?;
                Ok(path.clone())
            }
        })?
        .validate_text()
}

fn labels(slug: &str, title: &str, field: &str, errors: &mut Vec<Diagnostic>) {
    if slug.is_empty()
        || title.trim().is_empty()
        || slug.chars().chain(title.chars()).any(char::is_control)
    {
        super::resolve::report(errors, "workspace_documentation_label", field, None);
    }
}

/// Resolve a project's exact configured module entrance, without consulting hosting or files.
pub(super) fn project_url(
    workspace: &super::Workspace,
    project: &Key,
) -> Result<Option<String>, &'static str> {
    let mut selected = None;
    for publication in workspace.brands().flat_map(|brand| &brand.publications) {
        let Some(repository) = &publication.documentation_repository else {
            continue;
        };
        let Some(base) = &repository.public_base_url else {
            continue;
        };
        let Some(build) = &repository.build else {
            continue;
        };
        for module in build
            .modules
            .iter()
            .filter(|module| &module.project == project)
        {
            if selected.is_some() {
                return Err("project_documentation_ambiguous");
            }
            let Some(DocumentationNavigation::Manual { pages, .. }) = module.navigation.first()
            else {
                return Err("project_documentation_entrance");
            };
            let page = pages.first().ok_or("project_documentation_entrance")?;
            let route: EntryPath = build
                .target
                .page_route(page.route.as_str())
                .parse()
                .map_err(|_| "project_documentation_entrance")?;
            let root: gazette_core::publication::routing::RoutePath =
                "/".parse().map_err(|_| "project_documentation_entrance")?;
            let route = root
                .join_file(&route)
                .map_err(|_| "project_documentation_entrance")?;
            selected = Some(base.resolve(&route));
        }
    }
    Ok(selected)
}
