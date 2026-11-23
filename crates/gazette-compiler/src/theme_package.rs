//! Pure Theme-package declarations; package capture and graph assembly happen outside this module.

use std::{collections::BTreeSet, fmt, str::FromStr};

use crate::{
    theme::{Component, Components, RenderRecipes},
    toml::{self, ParsedToml},
    workspace::Key,
};
use cssparser::{Parser, ParserInput};
use gazette_core::{
    publication::CapabilityId,
    resource::ResourceId,
    source::{EntryPath, Sha256Digest, Subdirectory},
};
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

/// One complete browser-consumable CSS color value owned by a Theme.
///
/// The exact authored spelling is retained after native CSS Color parsing. Values
/// requiring inherited context, such as `currentcolor`, are rejected because browser
/// chrome has no Theme element from which to resolve them.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct CssColor(String);

/// A string is not one complete context-independent CSS color value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidCssColor;

impl fmt::Display for InvalidCssColor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("expected one context-independent CSS color value")
    }
}

impl std::error::Error for InvalidCssColor {}

impl CssColor {
    /// Exact validated CSS spelling supplied by the owning Theme.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for CssColor {
    type Err = InvalidCssColor;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.is_empty()
            || value.trim() != value
            || value.chars().any(char::is_control)
            || value.len() > 256
        {
            return Err(InvalidCssColor);
        }
        let mut input = ParserInput::new(value);
        let mut parser = Parser::new(&mut input);
        let color = parser
            .parse_entirely(cssparser_color::Color::parse)
            .map_err(|_| InvalidCssColor)?;
        if matches!(color, cssparser_color::Color::CurrentColor) {
            return Err(InvalidCssColor);
        }
        Ok(Self(value.to_owned()))
    }
}

impl<'de> Deserialize<'de> for CssColor {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(D::Error::custom)
    }
}

impl schemars::JsonSchema for CssColor {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "CssColor".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "minLength": 1,
            "maxLength": 256,
            "description": "One context-independent CSS color value validated by the native CSS Color parser."
        })
    }
}

/// Browser chrome colors resolved by the Theme for both supported color modes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BrowserThemeColors {
    /// Theme-owned color for a light browser color scheme.
    pub light: CssColor,
    /// Theme-owned color for a dark browser color scheme.
    pub dark: CssColor,
}

/// One named Theme-owned palette and the browser colors it resolves.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Palette {
    /// Exact palette identity selected by workspace composition.
    pub id: Key,
    /// Browser chrome colors belonging to this palette implementation.
    pub browser_theme_colors: BrowserThemeColors,
}

/// Stable logical mount name for one package in the effective Theme namespace.
///
/// This is presentation vocabulary, not a filesystem path, domain, Theme family,
/// provider, or compatibility identifier.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct Namespace(String);

/// A logical Theme namespace is not a bounded lowercase ASCII identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidNamespace;

impl fmt::Display for InvalidNamespace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(
            "expected 1..=63 lowercase ASCII letters, digits, or hyphens, starting with a letter and ending with a letter or digit",
        )
    }
}

impl std::error::Error for InvalidNamespace {}

impl Namespace {
    /// Exact logical spelling used when constructing the virtual template namespace.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Namespace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Namespace {
    type Err = InvalidNamespace;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let bytes = value.as_bytes();
        if !(1..=63).contains(&bytes.len())
            || !bytes[0].is_ascii_lowercase()
            || !bytes[bytes.len() - 1].is_ascii_lowercase()
                && !bytes[bytes.len() - 1].is_ascii_digit()
            || !bytes
                .iter()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
        {
            return Err(InvalidNamespace);
        }
        Ok(Self(value.to_owned()))
    }
}

impl<'de> Deserialize<'de> for Namespace {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(D::Error::custom)
    }
}

impl schemars::JsonSchema for Namespace {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ThemePackageNamespace".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "minLength": 1,
            "maxLength": 63,
            "pattern": "^[a-z](?:[a-z0-9-]{0,61}[a-z0-9])?$",
            "description": "Stable logical Theme-package mount name; not a filesystem path or provider identity."
        })
    }
}

/// Exact Theme-package identity, intentionally not interchangeable with a
/// capability, provider, source binding, palette, or resource identity.
#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(transparent)]
pub struct ThemePackageId(Key);

impl ThemePackageId {
    /// Exact underlying namespaced spelling without deriving any other identity.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    pub(crate) fn as_key(&self) -> &Key {
        &self.0
    }
}

impl fmt::Display for ThemePackageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for ThemePackageId {
    type Err = gazette_core::publication::InvalidKey;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(Self(value.parse()?))
    }
}

/// One exact package dependency edge.
///
/// Workspace bindings own source selection and application evidence owns the
/// captured-tree digest. Neither can be asserted by a package-authored manifest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Dependency {
    /// Exact dependency package identity.
    pub package: ThemePackageId,
}

/// One package-owned presentation resource and its complete declared input closure.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Resource {
    /// Package-local logical identity.
    pub id: ResourceId,
    /// Exact retained inputs relative to the captured package root.
    pub inputs: Vec<EntryPath>,
    /// Deterministic compiler recipe for these retained inputs.
    pub build: gazette_core::resource::Recipe,
    /// Default public file route. A workspace may replace or disable this resource.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<gazette_core::publication::routing::RoutePath>,
    /// Emission policy for the default output; omission emits it with the site.
    #[serde(default, rename = "use", skip_serializing_if = "Option::is_none")]
    pub use_: Option<crate::workspace::ResourceUse>,
}

/// Capability promises consumed by this Theme; the workspace selects implementations.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct CapabilityNeeds {
    /// Every capability must be available for successful Theme admission.
    pub required: Vec<CapabilityId>,
    /// Explicitly degradable capability needs; absence remains an observable outcome.
    pub optional: Vec<CapabilityId>,
}

/// Self-description of one captured Theme package.
///
/// The declaration contains no provider selection, source discovery, semver range,
/// site identity, Theme family, publication authority, or runtime effect.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Exact package identity.
    pub package: ThemePackageId,
    /// Package's own stable mount namespace.
    pub namespace: Namespace,
    /// Template root relative to this captured package.
    pub templates: Subdirectory,
    /// Root-capable rendering entries. Dependencies may omit them; graph admission
    /// decides which package supplies the effective entries.
    pub render: Option<RenderRecipes>,
    /// Default interface labels; workspace and site copy may override each label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chrome: Option<crate::workspace::SiteChrome>,
    /// Browser colors used when the workspace selects no named palette.
    /// Root-package admission requires this value for that selection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser_theme_colors: Option<BrowserThemeColors>,
    /// Colour mode shown before the reader chooses one: `system`, `light` or `dark`.
    /// A package without it inherits its nearest declaring dependency; the graph
    /// defaults to `system`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_color_mode: Option<gazette_core::markdown::InitialColorMode>,
    /// Theme-owned palette declarations; selection remains workspace-owned.
    #[serde(default)]
    pub palettes: Vec<Palette>,
    /// Static-first component contracts backed by this package's templates.
    #[serde(default)]
    pub components: std::collections::BTreeMap<gazette_core::markdown::ComponentName, Component>,
    /// Exact dependency graph edges. Declaration order has no semantic meaning.
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
    /// Package-owned resources. Declaration order has no semantic meaning.
    #[serde(default)]
    pub resources: Vec<Resource>,
    /// Ordered logical resource needs; workspace composition selects concrete bindings.
    #[serde(default)]
    pub resource_requirements: Vec<ResourceId>,
    /// Required and optional capability contracts, never provider choices.
    #[serde(default)]
    pub capabilities: CapabilityNeeds,
}

/// Syntax or complete declaration rejection; no partial package is admitted.
#[derive(Debug)]
pub enum Error {
    /// Shared bounded TOML parser/deserializer failure.
    Syntax(toml::Error),
    /// Invalid complete package declaration.
    Declaration {
        /// Exact owning package.
        package: ThemePackageId,
        /// Stable logical field name.
        field: &'static str,
        /// Violated invariant.
        reason: &'static str,
    },
}

impl Error {
    /// Stable class shared by future interface projections.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Syntax(error) => error.code(),
            Self::Declaration { .. } => "theme_package_declaration",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax(error) => error.fmt(f),
            Self::Declaration {
                package,
                field,
                reason,
            } => write!(f, "Theme package {package} field {field}: {reason}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Syntax(error) => Some(error),
            Self::Declaration { .. } => None,
        }
    }
}

/// Retained native TOML plus one validated package declaration.
#[derive(Debug)]
pub struct ParsedManifest<'s> {
    syntax: ParsedToml<'s>,
    manifest: Manifest,
    components: std::sync::Arc<Components>,
}

impl<'s> ParsedManifest<'s> {
    /// Parse and validate the complete Theme-package declaration.
    ///
    /// # Errors
    /// Rejects syntax/bounds, invalid namespaces, duplicate or ambiguous graph
    /// declarations, duplicate logical requirements, and provider or compatibility fields.
    pub fn parse(source: &'s str, limits: toml::Limits) -> Result<Self, Error> {
        let syntax = ParsedToml::parse(source, limits).map_err(Error::Syntax)?;
        let mut manifest: Manifest = syntax.deserialize().map_err(Error::Syntax)?;
        validate(&mut manifest)?;
        let components =
            std::sync::Arc::new(Components::compile(&manifest.components).map_err(|error| {
                Error::Declaration {
                    package: manifest.package.clone(),
                    field: "components",
                    reason: error.reason(),
                }
            })?);
        Ok(Self {
            syntax,
            manifest,
            components,
        })
    }

    /// One normalized immutable declaration; logical requirement order remains intact.
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// Compiled component argument contracts from the same immutable package declaration.
    pub fn components(&self) -> &std::sync::Arc<Components> {
        &self.components
    }

    /// Shared retained syntax for editor and diagnostic projections.
    pub fn syntax(&self) -> &ParsedToml<'s> {
        &self.syntax
    }

    /// Exact manifest-byte identity, distinct from the captured package-tree digest.
    pub fn source_sha256(&self) -> Sha256Digest {
        Sha256Digest::of(self.syntax.source().as_bytes())
    }
}

fn validate(manifest: &mut Manifest) -> Result<(), Error> {
    let package = manifest.package.clone();
    let failure = |field, reason| Error::Declaration {
        package: package.clone(),
        field,
        reason,
    };

    if let Some(recipe) = manifest
        .render
        .as_ref()
        .and_then(|render| render.social_card.as_ref())
    {
        recipe
            .validate()
            .map_err(|(field, reason)| failure(field, reason))?;
    }

    if let Some(copy) = &manifest.chrome {
        if copy.reading_assistance.is_some()
            || copy.shell.search.as_ref().is_some_and(|search| {
                search.no_script_destination.is_some() || search.enabled.is_some()
            })
        {
            return Err(failure(
                "chrome",
                "Theme defaults may supply copy, not destinations, external handoffs or control selection",
            ));
        }
        if !copy.valid_labels() {
            return Err(failure(
                "chrome",
                "copy must contain bounded nonempty labels without control characters",
            ));
        }
    }

    let mut dependency_packages = BTreeSet::new();
    for dependency in &manifest.dependencies {
        if dependency.package == manifest.package {
            return Err(failure("dependencies", "package cannot depend on itself"));
        }
        if !dependency_packages.insert(dependency.package.clone()) {
            return Err(failure("dependencies", "duplicate dependency package"));
        }
    }
    manifest
        .dependencies
        .sort_unstable_by(|left, right| left.package.cmp(&right.package));

    manifest
        .palettes
        .sort_unstable_by(|left, right| left.id.cmp(&right.id));
    if manifest
        .palettes
        .windows(2)
        .any(|pair| pair[0].id == pair[1].id)
    {
        return Err(failure("palettes", "duplicate palette identity"));
    }

    let mut resource_ids = BTreeSet::new();
    for resource in &mut manifest.resources {
        if resource
            .output
            .as_ref()
            .is_some_and(|path| path.as_str().ends_with('/'))
        {
            return Err(failure(
                "resources.output",
                "resource output must be a file route",
            ));
        }
        if resource.use_.is_some() && resource.output.is_none() {
            return Err(failure(
                "resources.use",
                "resource use requires a default output",
            ));
        }
        if resource.use_ == Some(crate::workspace::ResourceUse::Dependency)
            && !matches!(
                resource.build,
                gazette_core::resource::Recipe::BrowserModule { .. }
            )
        {
            return Err(failure(
                "resources.use",
                "dependency resource must be a browser module",
            ));
        }
        if !resource_ids.insert(resource.id.clone()) {
            return Err(failure("resources", "duplicate resource identity"));
        }
        if resource.inputs.is_empty() {
            return Err(failure(
                "resources.inputs",
                "resource inputs must be nonempty",
            ));
        }
        resource.inputs.sort_unstable();
        if resource.inputs.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(failure("resources.inputs", "duplicate resource input"));
        }
        resource
            .build
            .validate_imports()
            .map_err(|_| failure("resources.build", "invalid module import bindings"))?;
        if resource
            .inputs
            .binary_search(resource.build.entry())
            .is_err()
        {
            return Err(failure(
                "resources.build",
                "build entry must belong to declared resource inputs",
            ));
        }
    }
    manifest
        .resources
        .sort_unstable_by(|left, right| left.id.cmp(&right.id));

    let mut requirements = BTreeSet::new();
    for resource in &manifest.resource_requirements {
        if !requirements.insert(resource.clone()) {
            return Err(failure(
                "resource_requirements",
                "duplicate logical resource requirement",
            ));
        }
    }

    unique(
        &manifest.package,
        "capabilities.required",
        &mut manifest.capabilities.required,
    )?;
    unique(
        &manifest.package,
        "capabilities.optional",
        &mut manifest.capabilities.optional,
    )?;
    if manifest.capabilities.required.iter().any(|capability| {
        manifest
            .capabilities
            .optional
            .binary_search(capability)
            .is_ok()
    }) {
        return Err(failure(
            "capabilities",
            "capability cannot be both required and optional",
        ));
    }
    Ok(())
}

fn unique<T: Ord>(
    package: &ThemePackageId,
    field: &'static str,
    values: &mut [T],
) -> Result<(), Error> {
    values.sort_unstable();
    if values.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(Error::Declaration {
            package: package.clone(),
            field,
            reason: "duplicate declaration",
        });
    }
    Ok(())
}
