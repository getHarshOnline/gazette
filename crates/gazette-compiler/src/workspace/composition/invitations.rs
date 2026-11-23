//! Page invitations are publication choices, independent from footer links.

use super::*;

/// One whole invitation policy, inherited by semantic key and replaced at site scope.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "state", rename_all = "kebab-case", deny_unknown_fields)]
pub enum InvitationSelection {
    /// Suppress an inherited invitation on every page in this publication.
    Disabled {},
    /// Select a routed destination for articles and exact page exceptions.
    Enabled {
        /// Existing content identity; neither a footer label nor a guessed URL.
        destination: Key,
        /// Include authored articles, including articles within collections.
        articles: bool,
        /// Exact page choices override the article default; false suppresses it.
        #[serde(default)]
        pages: BTreeMap<Key, bool>,
    },
}

pub(super) fn resolve(
    defaults: &Composition,
    site: &Site,
    limits: crate::toml::Limits,
    used: &mut usize,
    text: &mut usize,
) -> Result<BTreeMap<Key, Selected<InvitationSelection>>, Error> {
    let local = &site.composition.invitations;
    let mut selected = BTreeMap::new();
    for (key, (value, scope)) in defaults
        .invitations
        .iter()
        .filter(|(key, _)| !local.contains_key(*key))
        .map(|(key, value)| (key, (value, SelectionScope::Workspace)))
        .chain(
            local
                .iter()
                .map(|(key, value)| (key, (value, SelectionScope::Site(site.id.clone())))),
        )
    {
        charge(used, 1, limits.nodes, "invitation selections")?;
        for value in [site.id.as_str(), key.as_str()] {
            charge(
                text,
                value.len(),
                limits.source_bytes,
                "invitation text bytes",
            )?;
        }
        if let InvitationSelection::Enabled {
            destination, pages, ..
        } = value
        {
            charge(
                used,
                pages.len(),
                limits.nodes,
                "invitation page selections",
            )?;
            for id in std::iter::once(destination).chain(pages.keys()) {
                charge(
                    text,
                    id.as_str().len(),
                    limits.source_bytes,
                    "invitation text bytes",
                )?;
            }
        }
        selected.insert(
            key.clone(),
            Selected {
                value: value.clone(),
                origin: SelectionOrigin {
                    scope,
                    field: SelectionField::Invitation {
                        invitation: key.clone(),
                    },
                },
            },
        );
    }
    Ok(selected)
}
