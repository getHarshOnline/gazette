//! Each copy label follows its declaring dependency, without sibling-order precedence.

use super::{Error, Graph, ThemePackageId};
use serde::Serialize;
use std::collections::BTreeMap;

/// One Theme-owned label and the package that declared its winning value.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct LabelDefault {
    /// Declaring package, including inherited Base labels.
    pub package: ThemePackageId,
    /// Literal interface copy; never a destination or capability selection.
    pub value: String,
}

pub(super) fn resolve(graph: &Graph<'_>) -> Result<BTreeMap<String, LabelDefault>, Error> {
    type Providers = BTreeMap<ThemePackageId, LabelDefault>;
    let mut inherited = BTreeMap::<ThemePackageId, BTreeMap<String, Providers>>::new();
    for package in graph.packages() {
        let mut labels = BTreeMap::<String, Providers>::new();
        for dependency in &package.dependencies {
            if let Some(defaults) = inherited.get(&dependency.package) {
                for (field, providers) in defaults {
                    labels
                        .entry(field.clone())
                        .or_default()
                        .extend(providers.clone());
                }
            }
        }
        if let Some(copy) = &package.chrome {
            let fields = copy
                .copy_fields()
                .into_iter()
                .map(|(field, value)| (field.to_owned(), value))
                .chain(
                    copy.regions
                        .iter()
                        .map(|(key, value)| (format!("regions.{key}"), value.as_str())),
                );
            for (field, value) in fields {
                labels.insert(
                    field,
                    BTreeMap::from([(
                        package.package.clone(),
                        LabelDefault {
                            package: package.package.clone(),
                            value: value.to_owned(),
                        },
                    )]),
                );
            }
        }
        inherited.insert(package.package.clone(), labels);
    }
    inherited
        .remove(&graph.root().package)
        .unwrap_or_default()
        .into_iter()
        .map(|(field, providers)| {
            if providers.len() > 1 {
                return Err(Error::CopyCollision {
                    field,
                    packages: providers.into_keys().collect(),
                });
            }
            Ok((
                field,
                providers
                    .into_values()
                    .next()
                    .expect("each label has a declaring package"),
            ))
        })
        .collect()
}
