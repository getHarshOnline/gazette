//! Deterministic resolution of one selected Theme-package dependency graph.

mod copy;
mod resources;
pub use copy::LabelDefault;
pub use resources::ResourceDefault;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use crate::{
    theme::{Component, Components, RenderRecipes},
    theme_package::{Manifest, ThemePackageId},
};
use gazette_core::{
    markdown::{ComponentName, InitialColorMode},
    publication::CapabilityId,
    resource::ResourceId,
    source::EntryPath,
};

/// One selected Theme package and its complete dependency-first closure.
#[derive(Debug)]
pub struct Graph<'a> {
    root: &'a Manifest,
    packages: Vec<&'a Manifest>,
}

impl<'a> Graph<'a> {
    /// Resolve an exact root from already parsed package declarations.
    ///
    /// Input order and dependency declaration order do not affect the result.
    /// Packages outside the selected root's closure remain available to another
    /// selection but do not enter this graph.
    ///
    /// # Errors
    ///
    /// Rejects duplicate package identities, a missing selected root, a missing
    /// dependency in the selected closure, or a dependency cycle.
    pub fn resolve<I>(root: &ThemePackageId, manifests: I) -> Result<Self, Error>
    where
        I: IntoIterator<Item = &'a Manifest>,
    {
        let mut available = BTreeMap::new();
        for manifest in manifests {
            if available
                .insert(manifest.package.clone(), manifest)
                .is_some()
            {
                return Err(Error::DuplicatePackage {
                    package: manifest.package.clone(),
                });
            }
        }

        let root_manifest = available
            .get(root)
            .copied()
            .ok_or_else(|| Error::MissingPackage {
                package: root.clone(),
                required_by: None,
            })?;

        let mut states = BTreeMap::new();
        let mut active = Vec::new();
        let mut pending = vec![Frame::new(root_manifest)];
        let mut packages = Vec::new();
        states.insert(root.clone(), State::Visiting(0));
        active.push(root.clone());

        while let Some(frame) = pending.last_mut() {
            if let Some(dependency) = frame.next() {
                let manifest =
                    available
                        .get(&dependency)
                        .copied()
                        .ok_or_else(|| Error::MissingPackage {
                            package: dependency.clone(),
                            required_by: Some(frame.manifest.package.clone()),
                        })?;
                match states.get(&dependency) {
                    Some(State::Complete) => {}
                    Some(State::Visiting(start)) => {
                        let mut cycle = active.get(*start..).unwrap_or_default().to_vec();
                        cycle.push(dependency);
                        return Err(Error::Cycle { packages: cycle });
                    }
                    None => {
                        states.insert(dependency.clone(), State::Visiting(active.len()));
                        active.push(dependency);
                        pending.push(Frame::new(manifest));
                    }
                }
                continue;
            }

            let Some(completed) = pending.pop().map(|frame| frame.manifest) else {
                break;
            };
            let active_package = active.pop();
            debug_assert_eq!(active_package.as_ref(), Some(&completed.package));
            states.insert(completed.package.clone(), State::Complete);
            packages.push(completed);
        }

        Ok(Self {
            root: root_manifest,
            packages,
        })
    }

    /// The exact selected root declaration.
    pub fn root(&self) -> &'a Manifest {
        self.root
    }

    /// Complete selected closure in deterministic dependency-before-dependent order.
    pub fn packages(&self) -> &[&'a Manifest] {
        &self.packages
    }
}

/// One admitted Theme-package closure projected into its virtual namespace.
///
/// This projection owns Theme meaning only: it never selects providers, opens
/// package sources, invokes Tera, or supplies workspace policy.
#[derive(Debug)]
pub struct EffectiveTheme {
    root: ThemePackageId,
    render: RenderRecipes,
    components: Arc<Components>,
    resource_requirements: Vec<ResourceId>,
    resources: BTreeMap<ResourceId, ResourceDefault>,
    labels: BTreeMap<String, LabelDefault>,
    initial_color_mode: InitialColorMode,
    required_capabilities: Vec<CapabilityId>,
    optional_capabilities: Vec<CapabilityId>,
}

impl EffectiveTheme {
    /// Colour mode before the reader chooses one, from the nearest declaring package.
    pub fn initial_color_mode(&self) -> InitialColorMode {
        self.initial_color_mode
    }

    /// Interface labels resolved independently through the selected Theme graph.
    pub fn labels(&self) -> &BTreeMap<String, LabelDefault> {
        &self.labels
    }

    /// Resolve namespaced render/component entries and aggregate capability needs.
    ///
    /// # Errors
    ///
    /// Rejects a root without render recipes, duplicate component identities across
    /// the selected closure, or any impossible qualified template entry.
    pub fn resolve(graph: &Graph<'_>) -> Result<Self, Error> {
        let root = graph.root();
        let render = root.render.as_ref().ok_or_else(|| Error::MissingRender {
            package: root.package.clone(),
        })?;
        let llms = inherited_entry(graph, root, "llms", |render| render.llms.as_ref())?;
        let mut kinds = BTreeMap::new();
        let mut modes = BTreeMap::new();
        let recipes = || {
            graph
                .packages()
                .iter()
                .filter_map(|package| package.render.as_ref())
        };
        for kind in recipes()
            .flat_map(|render| render.kinds.keys())
            .collect::<BTreeSet<_>>()
        {
            if let Some(entry) =
                inherited_entry(graph, root, &format!("kinds.{kind:?}"), |render| {
                    render.kinds.get(kind)
                })?
            {
                kinds.insert(*kind, entry);
            }
        }
        for mode in recipes()
            .flat_map(|render| render.modes.keys())
            .collect::<BTreeSet<_>>()
        {
            if let Some(entry) = inherited_entry(graph, root, &format!("modes.{mode}"), |render| {
                render.modes.get(mode)
            })? {
                modes.insert(mode.clone(), entry);
            }
        }
        let social_card = inherited_value(graph, root, "social_card", |package| {
            let Some(recipe) = package
                .render
                .as_ref()
                .and_then(|render| render.social_card.as_ref())
            else {
                return Ok(None);
            };
            let mut recipe = recipe.clone();
            recipe.template = qualify(&package.namespace, &recipe.template)?;
            Ok(Some(recipe))
        })?;
        let render = RenderRecipes {
            social_card,
            not_found: inherited_entry(graph, root, "not_found", |render| {
                render.not_found.as_ref()
            })?,
            page: qualify(&root.namespace, &render.page)?,
            kinds,
            modes,
            home: inherited_entry(graph, root, "home", |render| render.home.as_ref())?,
            collection_directory: inherited_entry(graph, root, "collection_directory", |render| {
                render.collection_directory.as_ref()
            })?,
            llms,
            alias: render
                .alias
                .as_ref()
                .map(|entry| qualify(&root.namespace, entry))
                .transpose()?,
            graph: render
                .graph
                .as_ref()
                .map(|entry| qualify(&root.namespace, entry))
                .transpose()?,
        };

        let mut definitions = BTreeMap::<ComponentName, Component>::new();
        let mut owners = BTreeMap::<ComponentName, ThemePackageId>::new();
        let mut required = BTreeSet::new();
        let mut optional = BTreeSet::new();
        let mut required_resources = BTreeSet::new();
        let mut resource_requirements = Vec::new();
        for package in graph.packages() {
            required.extend(package.capabilities.required.iter().cloned());
            optional.extend(package.capabilities.optional.iter().cloned());
            for resource in &package.resource_requirements {
                if required_resources.insert(resource.clone()) {
                    resource_requirements.push(resource.clone());
                }
            }
            for (name, component) in &package.components {
                if let Some(first) = owners.get(name) {
                    return Err(Error::ComponentCollision {
                        component: name.clone(),
                        first: first.clone(),
                        second: package.package.clone(),
                    });
                }
                let mut component = component.clone();
                component.template = qualify(&package.namespace, &component.template)?;
                owners.insert(name.clone(), package.package.clone());
                definitions.insert(name.clone(), component);
            }
        }
        for capability in &required {
            optional.remove(capability);
        }
        let components =
            Components::compile(&definitions).map_err(|_| Error::ComponentContract {
                package: root.package.clone(),
            })?;

        Ok(Self {
            root: root.package.clone(),
            render,
            components: Arc::new(components),
            resource_requirements,
            resources: resources::resolve(graph)?,
            labels: copy::resolve(graph)?,
            initial_color_mode: inherited_value(graph, root, "initial_color_mode", |package| {
                Ok(package.initial_color_mode)
            })?
            .unwrap_or_default(),
            required_capabilities: required.into_iter().collect(),
            optional_capabilities: optional.into_iter().collect(),
        })
    }

    /// Exact selected root package.
    pub fn root(&self) -> &ThemePackageId {
        &self.root
    }

    /// Root render entries qualified into the same virtual namespace as Tera input.
    pub fn render(&self) -> &RenderRecipes {
        &self.render
    }

    /// Collision-free component contracts with namespaced template entries.
    pub fn components(&self) -> &Arc<Components> {
        &self.components
    }

    /// Logical resource needs in dependency-before-dependent package declaration order.
    pub fn resource_requirements(&self) -> &[ResourceId] {
        &self.resource_requirements
    }

    /// Theme-owned resource defaults, with the package that supplied each declaration.
    pub fn resources(&self) -> &BTreeMap<ResourceId, ResourceDefault> {
        &self.resources
    }

    /// Required capability contracts aggregated across the selected closure.
    pub fn required_capabilities(&self) -> &[CapabilityId] {
        &self.required_capabilities
    }

    /// Optional capability contracts not required by another selected package.
    pub fn optional_capabilities(&self) -> &[CapabilityId] {
        &self.optional_capabilities
    }
}

fn qualify(
    namespace: &crate::theme_package::Namespace,
    entry: &EntryPath,
) -> Result<EntryPath, Error> {
    format!("{namespace}/{entry}")
        .parse()
        .map_err(|_| Error::QualifiedEntry {
            namespace: namespace.to_string(),
            entry: entry.clone(),
        })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Visiting(usize),
    Complete,
}

struct Frame<'a> {
    manifest: &'a Manifest,
    dependencies: Vec<ThemePackageId>,
    next: usize,
}

impl<'a> Frame<'a> {
    fn new(manifest: &'a Manifest) -> Self {
        let mut dependencies = manifest
            .dependencies
            .iter()
            .map(|dependency| dependency.package.clone())
            .collect::<Vec<_>>();
        dependencies.sort_unstable();
        dependencies.dedup();
        Self {
            manifest,
            dependencies,
            next: 0,
        }
    }

    fn next(&mut self) -> Option<ThemePackageId> {
        let dependency = self.dependencies.get(self.next).cloned();
        self.next += usize::from(dependency.is_some());
        dependency
    }
}

// A dependent package may explicitly replace its inherited render entry. Two
// independent providers require a selection; graph traversal order is not policy.
fn inherited_entry(
    graph: &Graph<'_>,
    root: &Manifest,
    name: &str,
    entry: impl Fn(&RenderRecipes) -> Option<&EntryPath>,
) -> Result<Option<EntryPath>, Error> {
    inherited_value(graph, root, name, |package| {
        package
            .render
            .as_ref()
            .and_then(&entry)
            .map(|entry| qualify(&package.namespace, entry))
            .transpose()
    })
}

fn inherited_value<T: Clone>(
    graph: &Graph<'_>,
    root: &Manifest,
    name: &str,
    value: impl Fn(&Manifest) -> Result<Option<T>, Error>,
) -> Result<Option<T>, Error> {
    let mut inherited = BTreeMap::<ThemePackageId, BTreeMap<ThemePackageId, T>>::new();
    for package in graph.packages() {
        let providers = if let Some(value) = value(package)? {
            BTreeMap::from([(package.package.clone(), value)])
        } else {
            let mut providers = BTreeMap::new();
            for dependency in &package.dependencies {
                if let Some(selected) = inherited.get(&dependency.package) {
                    providers.extend(
                        selected
                            .iter()
                            .map(|(id, entry)| (id.clone(), entry.clone())),
                    );
                }
            }
            providers
        };
        inherited.insert(package.package.clone(), providers);
    }
    let selected = inherited.remove(&root.package).unwrap_or_default();
    if selected.len() > 1 {
        return Err(Error::RenderCollision {
            entry: name.into(),
            packages: selected.into_keys().collect(),
        });
    }
    Ok(selected.into_values().next())
}

/// Failure to assemble one exact Theme-package graph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// More than one declaration claims the same package identity.
    DuplicatePackage {
        /// Conflicting exact identity.
        package: ThemePackageId,
    },
    /// The selected root or one of its dependencies is unavailable.
    MissingPackage {
        /// Unavailable exact identity.
        package: ThemePackageId,
        /// Depending package, or `None` when the selected root is unavailable.
        required_by: Option<ThemePackageId>,
    },
    /// A closed dependency path revisits a package that is still being resolved.
    Cycle {
        /// Deterministic closed path whose final identity equals its first.
        packages: Vec<ThemePackageId>,
    },
    /// The selected root package cannot render ordinary pages.
    MissingRender {
        /// Selected root package.
        package: ThemePackageId,
    },
    /// Independent dependency branches expose competing optional render entries or
    /// inherited manifest defaults, such as `initial_color_mode`.
    RenderCollision {
        /// Logical render role or manifest field.
        entry: String,
        /// Exact competing providers.
        packages: Vec<ThemePackageId>,
    },
    /// Incomparable dependencies supply the same label without a dependent override.
    CopyCollision {
        /// Exact copy field, or namespaced region key after `regions.`.
        field: String,
        /// Packages whose declarations remain in conflict.
        packages: Vec<ThemePackageId>,
    },
    /// Independent dependency branches declare the same resource default.
    ResourceCollision {
        /// Conflicting logical resource.
        resource: ResourceId,
        /// Competing packages; a dependent package can supply an explicit replacement.
        packages: Vec<ThemePackageId>,
    },
    /// Two packages expose the same component without an explicit override contract.
    ComponentCollision {
        /// Conflicting logical component identity.
        component: ComponentName,
        /// First package in dependency order.
        first: ThemePackageId,
        /// Later package attempting to replace it.
        second: ThemePackageId,
    },
    /// A component schema failed while building the effective catalog.
    ComponentContract {
        /// Selected root package.
        package: ThemePackageId,
    },
    /// A package-local template entry could not enter its declared namespace.
    QualifiedEntry {
        /// Declared namespace.
        namespace: String,
        /// Package-local entry.
        entry: EntryPath,
    },
}

impl Error {
    /// Stable failure class for interface projections.
    pub fn code(&self) -> &'static str {
        match self {
            Self::DuplicatePackage { .. } => "theme_package_duplicate",
            Self::MissingPackage { .. } => "theme_package_missing",
            Self::Cycle { .. } => "theme_package_cycle",
            Self::MissingRender { .. } => "theme_package_render",
            Self::RenderCollision { .. } => "theme_package_render_collision",
            Self::CopyCollision { .. } => "theme_package_copy_collision",
            Self::ResourceCollision { .. } => "theme_package_resource_collision",
            Self::ComponentCollision { .. } => "theme_package_component_collision",
            Self::ComponentContract { .. } => "theme_package_component_contract",
            Self::QualifiedEntry { .. } => "theme_package_entry",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicatePackage { package } => {
                write!(f, "duplicate Theme package {package}")
            }
            Self::MissingPackage {
                package,
                required_by: Some(required_by),
            } => write!(
                f,
                "Theme package {required_by} requires missing package {package}"
            ),
            Self::MissingPackage {
                package,
                required_by: None,
            } => write!(f, "selected root Theme package {package} is missing"),
            Self::Cycle { packages } => {
                f.write_str("Theme package dependency cycle: ")?;
                for (index, package) in packages.iter().enumerate() {
                    if index > 0 {
                        f.write_str(" -> ")?;
                    }
                    package.fmt(f)?;
                }
                Ok(())
            }
            Self::MissingRender { package } => {
                write!(
                    f,
                    "selected root Theme package {package} has no render recipes"
                )
            }
            Self::RenderCollision { entry, packages } => {
                write!(
                    f,
                    "Theme render entry {entry} has competing dependency providers: {packages:?}"
                )
            }
            Self::CopyCollision { field, packages } => write!(
                f,
                "Theme copy {field} has conflicting declarations in {packages:?}"
            ),
            Self::ResourceCollision { resource, packages } => write!(
                f,
                "Theme resource {resource} has competing dependency providers: {packages:?}"
            ),
            Self::ComponentCollision {
                component,
                first,
                second,
            } => write!(
                f,
                "Theme component {component} is declared by both {first} and {second}"
            ),
            Self::ComponentContract { package } => {
                write!(
                    f,
                    "Theme package graph rooted at {package} has invalid components"
                )
            }
            Self::QualifiedEntry { namespace, entry } => write!(
                f,
                "Theme namespace {namespace} cannot qualify template entry {entry}"
            ),
        }
    }
}

impl std::error::Error for Error {}
