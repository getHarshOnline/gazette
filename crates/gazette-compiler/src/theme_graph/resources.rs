//! Resource defaults follow declared dependencies; sibling traversal order chooses nothing.

use super::{Error, Graph, ResourceId, ThemePackageId};
use crate::workspace::ResourceUse;
use gazette_core::publication::routing::RoutePath;
use serde::Serialize;
use std::collections::BTreeMap;

/// One resource's default output and the Theme package that owns its implementation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct ResourceDefault {
    /// Declaring package, including when inherited through another Theme.
    pub package: ThemePackageId,
    /// Default public file route.
    pub output: RoutePath,
    /// Whether the resource is emitted with the site or reached through another module.
    #[serde(rename = "use")]
    pub use_: ResourceUse,
}

pub(super) fn resolve(graph: &Graph<'_>) -> Result<BTreeMap<ResourceId, ResourceDefault>, Error> {
    type Providers = BTreeMap<ThemePackageId, ResourceDefault>;
    let mut inherited = BTreeMap::<ThemePackageId, BTreeMap<ResourceId, Providers>>::new();
    for package in graph.packages() {
        let mut resources = BTreeMap::<ResourceId, Providers>::new();
        for dependency in &package.dependencies {
            if let Some(defaults) = inherited.get(&dependency.package) {
                for (id, providers) in defaults {
                    resources
                        .entry(id.clone())
                        .or_default()
                        .extend(providers.clone());
                }
            }
        }
        for resource in &package.resources {
            if let Some(output) = &resource.output {
                resources.insert(
                    resource.id.clone(),
                    BTreeMap::from([(
                        package.package.clone(),
                        ResourceDefault {
                            package: package.package.clone(),
                            output: output.clone(),
                            use_: resource.use_.unwrap_or(ResourceUse::Site),
                        },
                    )]),
                );
            }
        }
        inherited.insert(package.package.clone(), resources);
    }
    inherited
        .remove(&graph.root().package)
        .unwrap_or_default()
        .into_iter()
        .map(|(id, providers)| {
            if providers.len() > 1 {
                return Err(Error::ResourceCollision {
                    resource: id,
                    packages: providers.into_keys().collect(),
                });
            }
            let selected = providers
                .into_values()
                .next()
                .expect("each resource has a declaring package");
            Ok((id, selected))
        })
        .collect()
}
