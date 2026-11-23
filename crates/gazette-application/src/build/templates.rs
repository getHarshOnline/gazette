//! Compile the exact selected Theme subtree from already checked, retained inputs.

use std::{collections::BTreeMap, fmt};

use gazette_compiler::{
    theme::RenderRecipes,
    theme_package::{BrowserThemeColors, Namespace, ThemePackageId},
    workspace::{Key, SourceLocation},
};
use gazette_core::{
    render,
    source::{self, EntryPath, Sha256Digest, Subdirectory, TreeIdentity},
};
use gazette_ports::render::{CompiledTemplates, TemplateCompiler};
use serde::Serialize;
use serde_json::Value;

use super::{CompositionMetadata, theme_namespace};

/// One Theme package's exact capture contribution to the compiled namespace.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct ThemePackageCompilation {
    pub package: ThemePackageId,
    pub namespace: Namespace,
    pub source: Key,
    pub source_location: SourceLocation,
    pub source_tree: TreeIdentity,
    pub manifest: EntryPath,
    pub manifest_sha256: Sha256Digest,
    pub templates: Subdirectory,
    pub mounted_files: Vec<EntryPath>,
}

/// Exact private compilation evidence, not admission, capability execution or release permission.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct TemplateCompilation {
    /// Compiled program's renderer/behavior declaration, not executable attestation or approval.
    pub tool: &'static str,
    /// Exact requested compilation/context/output ceilings, not a hostile-code sandbox.
    pub limits: render::Limits,
    /// Exact workspace bytes that determined the effective selection.
    pub configuration_sha256: Sha256Digest,
    /// Consuming publication site.
    pub site: Key,
    /// Effective Theme identity.
    pub theme: ThemePackageId,
    /// Selected palette name, not proof of its visual implementation.
    pub palette: Option<Key>,
    /// Theme-resolved, CSS-validated browser chrome colors for both color modes.
    pub browser_theme_colors: BrowserThemeColors,
    /// Dependency-first exact package contributions to the virtual namespace.
    pub packages: Vec<ThemePackageCompilation>,
    /// Theme-owned render entrypoints selected by the exact manifest.
    pub render: RenderRecipes,
    /// Complete virtual namespace identity.
    pub template_tree: TreeIdentity,
}

/// Owned compiled presentation; no further native reads or workspace resolution.
pub struct CompiledTheme {
    templates: Box<dyn CompiledTemplates>,
    pub(super) composition: gazette_compiler::workspace::ResolvedComposition,
    render: RenderRecipes,
    evidence: TemplateCompilation,
    pub(super) components: std::sync::Arc<gazette_compiler::theme::Components>,
    pub(super) component_regions:
        std::collections::BTreeMap<gazette_core::publication::Key, String>,
    pub(super) initial_color_mode: gazette_core::markdown::InitialColorMode,
}

impl CompiledTheme {
    /// Operator evidence of consumed selection and bytes; not a public manifest.
    pub fn evidence(&self) -> &TemplateCompilation {
        &self.evidence
    }

    /// Theme-owned ordinary render entries; callers still own output placement.
    pub fn render_recipes(&self) -> &RenderRecipes {
        &self.render
    }

    /// Render supplied presentation context; publication admission is a separate operation.
    ///
    /// # Errors
    /// Preserves renderer diagnostics and refuses oversized successful renderer output.
    pub fn render(&self, entry: &EntryPath, context: &Value) -> Result<Vec<u8>, render::Error> {
        let bytes = self.templates.render(entry, context)?;
        if bytes.len() > self.evidence.limits.output_bytes {
            return Err(render::Error::Limit("output bytes"));
        }
        Ok(bytes)
    }
}

/// Complete template compilation failed; no partial program escapes.
#[derive(Debug)]
pub enum TemplateError {
    /// A supposedly checked selection no longer has its required inward binding.
    Binding(&'static str),
    /// Subtree structure, exact bytes or caller bounds were rejected.
    Input(source::Error),
    /// Theme-package graph capture could not form one virtual namespace.
    Namespace(theme_namespace::Error),
    /// Native template compilation failed.
    Render(render::Error),
    /// Compiled program omitted or malformed its bounded renderer-profile reference.
    Profile,
    /// The renderer did not bind its program to the exact consumed inputs.
    IdentityMismatch {
        /// Application-selected complete identity.
        expected: TreeIdentity,
        /// Renderer-reported identity.
        observed: TreeIdentity,
    },
}

impl TemplateError {
    /// Stable failure class, retaining underlying input/renderer classifications.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Binding(_) => "template_binding",
            Self::Input(error) => error.code(),
            Self::Namespace(error) => error.code(),
            Self::Render(error) => error.code(),
            Self::Profile => "template_profile",
            Self::IdentityMismatch { .. } => "template_identity",
        }
    }
}
impl fmt::Display for TemplateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Binding(reason) => write!(f, "template binding: {reason}"),
            Self::Input(error) => error.fmt(f),
            Self::Namespace(error) => error.fmt(f),
            Self::Render(error) => error.fmt(f),
            Self::Profile => f.write_str(
                "compiled renderer profile must be 1..=256 printable ASCII bytes without spaces",
            ),
            Self::IdentityMismatch { expected, observed } => write!(
                f,
                "template identity mismatch: expected {expected:?}, observed {observed:?}"
            ),
        }
    }
}
impl std::error::Error for TemplateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Input(error) => Some(error),
            Self::Namespace(error) => Some(error),
            Self::Render(error) => Some(error),
            _ => None,
        }
    }
}

impl CompositionMetadata<'_> {
    /// Compile every regular file in the selected template directory, regardless of suffix.
    ///
    /// Only retained bytes are borrowed. Names are relative to the selected directory,
    /// enabling native template includes without leaking surrounding metadata or assets.
    /// No content meaning, palette execution or capability output is manufactured here.
    ///
    /// # Errors
    /// Rejects missing bindings, tighter subtree limits, invalid templates, malformed
    /// renderer profiles or renderer input-identity mismatch.
    pub fn compile_templates(
        &self,
        compiler: &impl TemplateCompiler,
        limits: render::Limits,
    ) -> Result<CompiledTheme, TemplateError> {
        let acquired = self.acquired;
        let compilation = acquired.compilation();
        let selected = compilation
            .workspace
            .composition_for(acquired.site())
            .and_then(|composition| composition.theme())
            .ok_or(TemplateError::Binding("selected Theme absent"))?
            .value();
        let graph = self.theme_graph();
        let effective = self.effective_theme();
        let captures = self
            .theme_order()
            .iter()
            .map(|package| {
                self.theme_capture(package)
                    .map(|capture| (package.clone(), capture))
                    .ok_or(TemplateError::Binding("admitted Theme capture absent"))
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        let namespace =
            theme_namespace::ThemeNamespace::assemble(&graph, &captures, limits.templates)
                .map_err(TemplateError::Namespace)?;
        let tree = namespace.as_input().map_err(TemplateError::Input)?;
        let components = effective.components().clone();
        for entry in effective.render().entries() {
            if tree.file(entry).is_none() {
                return Err(TemplateError::Binding(
                    "declared render entry is absent from the virtual Theme namespace",
                ));
            }
        }
        for (_, definition) in components.definitions() {
            if tree.file(&definition.template).is_none() {
                return Err(TemplateError::Binding(
                    "declared component template is absent from the virtual Theme namespace",
                ));
            }
        }
        if let Some(bindings) = compilation
            .workspace
            .composition_for(acquired.site())
            .and_then(|composition| composition.page_templates())
        {
            for entry in bindings.value().values() {
                if tree.file(entry).is_none() {
                    return Err(TemplateError::Binding(
                        "configured page template is absent from the virtual Theme namespace",
                    ));
                }
            }
        }
        let templates = compiler
            .compile(&tree, limits)
            .map_err(TemplateError::Render)?;
        let expected = namespace.inventory().identity();
        let observed = templates.identity();
        if observed != expected {
            return Err(TemplateError::IdentityMismatch { expected, observed });
        }
        let tool = templates.profile();
        if tool.is_empty() || tool.len() > 256 || !tool.bytes().all(|byte| byte.is_ascii_graphic())
        {
            return Err(TemplateError::Profile);
        }
        let packages = namespace
            .packages()
            .iter()
            .map(|evidence| {
                let binding = self
                    .theme_binding(evidence.package())
                    .ok_or(TemplateError::Binding("admitted Theme binding absent"))?;
                let source = compilation
                    .workspace
                    .source(&binding.source)
                    .ok_or(TemplateError::Binding("admitted Theme source absent"))?;
                let manifest = self
                    .theme_manifest(evidence.package())
                    .ok_or(TemplateError::Binding("admitted Theme manifest absent"))?;
                Ok(ThemePackageCompilation {
                    package: evidence.package().clone(),
                    namespace: evidence.namespace().clone(),
                    source: binding.source.clone(),
                    source_location: source.location.clone(),
                    source_tree: evidence.source_tree(),
                    manifest: binding.manifest.clone(),
                    manifest_sha256: manifest.source_sha256(),
                    templates: evidence.templates().clone(),
                    mounted_files: evidence.files().to_vec(),
                })
            })
            .collect::<Result<Vec<_>, TemplateError>>()?;
        Ok(CompiledTheme {
            templates,
            composition: self.composition().clone(),
            render: effective.render().clone(),
            components,
            component_regions: self
                .composition()
                .chrome()
                .map(|chrome| chrome.value().regions.clone())
                .unwrap_or_default(),
            initial_color_mode: effective.initial_color_mode(),
            evidence: TemplateCompilation {
                tool,
                limits,
                configuration_sha256: compilation.source_sha256,
                site: acquired.site().clone(),
                theme: selected.package.clone(),
                palette: selected.palette.clone(),
                browser_theme_colors: self.browser_theme_colors().clone(),
                packages,
                render: effective.render().clone(),
                template_tree: expected,
            },
        })
    }
}
