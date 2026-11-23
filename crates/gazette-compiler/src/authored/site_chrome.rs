//! Independent workspace navigation and footer projections using the shared route graph.

use gazette_core::publication::{
    Key,
    content::{ContentKind, ContentNode, PageFamily},
};
use serde::Serialize;

use super::{ContentView, ProjectionError, ResolvedAuthored, ResolvedDestination};
use crate::workspace::{
    InvitationSelection, NavigationEntry, ReadingAssistance, ResolvedComposition,
};

/// A page-selected invitation with its existing routed destination.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct InvitationView<'a> {
    /// Shared resolved content and route; preview addressing follows the same graph.
    pub destination: ResolvedDestination<'a>,
}

/// One ordered site-navigation item with workspace-owned visible copy.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct SiteNavigationEntry<'a> {
    /// Optional configured semantic address; the compiler assigns it no layout meaning.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<&'a Key>,
    /// Exact visible label declared by workspace composition.
    pub label: &'a str,
    /// Already-resolved Gazette destination.
    pub destination: ResolvedDestination<'a>,
}

/// Search-control copy plus its optional resolved no-script HTML destination.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct SearchControlView<'a> {
    /// Explicit full-page search destination selected by this publication.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<ResolvedDestination<'a>>,
    /// Label for opening the selected site's search control.
    pub trigger_label: &'a str,
    /// Search control heading.
    pub title: &'a str,
    /// Search field label.
    pub field_label: &'a str,
    /// Copy shown while the selected search corpus loads.
    pub loading_message: &'a str,
    /// Copy shown when a query has no results.
    pub empty_message: &'a str,
    /// Copy shown when selected search is unavailable.
    pub unavailable_message: &'a str,
    /// Existing routed page used when scripts cannot run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub no_script_destination: Option<ResolvedDestination<'a>>,
}

/// Complete settings labels required by a visible settings control.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct SettingsControlView<'a> {
    /// Declared trigger label.
    pub trigger_label: &'a str,
    /// Declared title.
    pub title: &'a str,
    /// Declared color mode label.
    pub color_mode_label: &'a str,
    /// Declared color mode system label.
    pub color_mode_system_label: &'a str,
    /// Declared color mode light label.
    pub color_mode_light_label: &'a str,
    /// Declared color mode dark label.
    pub color_mode_dark_label: &'a str,
    /// Declared motion label.
    pub motion_label: &'a str,
    /// Declared motion system label.
    pub motion_system_label: &'a str,
    /// Declared motion reduce label.
    pub motion_reduce_label: &'a str,
    /// Declared motion allow label.
    pub motion_allow_label: &'a str,
}

/// Shared shell copy resolved for one site.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct ShellView<'a> {
    /// Label for dismissing the active shell tenant.
    pub close_label: &'a str,
    /// Search-control copy, absent when omitted or explicitly disabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search: Option<SearchControlView<'a>>,
    /// Closed settings-control copy.
    pub settings: SettingsControlView<'a>,
}

/// Workspace-selected chrome for one site, retaining declared navigation order.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct SiteChromeView<'a> {
    /// First-focusable link text for the main landmark.
    pub skip_to_content_label: &'a str,
    /// Accessible name of the site-navigation landmark.
    pub navigation_label: &'a str,
    /// Accessible name of the site footer landmark.
    pub footer_label: &'a str,
    /// Shared shell/control copy.
    pub shell: ShellView<'a>,
    /// Ordered, fully resolved navigation entries.
    pub navigation: Vec<SiteNavigationEntry<'a>>,
    /// Independently selected, ordered footer destinations.
    pub footer: Vec<SiteNavigationEntry<'a>>,
    /// Configured invitation targets, independent of visible page invitations.
    /// Destination pages can retain handoff context without inviting themselves.
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub invitation_destinations: std::collections::BTreeMap<Key, ResolvedDestination<'a>>,
    /// Additional generic region copy in stable namespaced-key order.
    pub regions: &'a std::collections::BTreeMap<Key, String>,
    /// Optional validated external handoff selection; no provider request is performed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reading_assistance: Option<&'a ReadingAssistance>,
}

impl ResolvedAuthored<'_, '_> {
    fn validate_invitations(
        &self,
        site: &Key,
        composition: &ResolvedComposition,
    ) -> Result<(), ProjectionError> {
        for selection in composition.invitations().values() {
            let InvitationSelection::Enabled {
                destination, pages, ..
            } = selection.value()
            else {
                continue;
            };
            self.chrome_destination(destination, false)?;
            for id in pages.keys() {
                let node =
                    self.content()
                        .graph()
                        .node(id)
                        .ok_or_else(|| ProjectionError::Binding {
                            id: id.clone(),
                            reason: "invitation page is absent from the resolved content graph",
                        })?;
                if &node.site != site {
                    return Err(ProjectionError::Binding {
                        id: id.clone(),
                        reason: "invitation override must select content in its publication",
                    });
                }
                self.chrome_destination(id, false)?;
            }
        }
        Ok(())
    }

    pub(super) fn page_invitations<'a>(
        &'a self,
        page: &ContentNode,
    ) -> Result<std::collections::BTreeMap<Key, InvitationView<'a>>, ProjectionError> {
        let mut invitations = std::collections::BTreeMap::new();
        let Some(composition) = self
            .content()
            .compilation()
            .workspace
            .composition_for(&page.site)
        else {
            return Ok(invitations);
        };
        self.validate_invitations(&page.site, composition)?;
        for (key, selection) in composition.invitations() {
            let InvitationSelection::Enabled {
                destination,
                articles,
                pages,
            } = selection.value()
            else {
                continue;
            };
            let article = matches!(
                page.structure,
                ContentKind::Page {
                    family: PageFamily::Article
                }
            );
            if pages.get(&page.id).copied().unwrap_or(*articles && article) {
                invitations.insert(
                    key.clone(),
                    InvitationView {
                        destination: self.chrome_destination(destination, false)?,
                    },
                );
            }
        }
        Ok(invitations)
    }

    fn chrome_entries<'a>(
        &'a self,
        entries: Option<&'a [NavigationEntry]>,
    ) -> Result<Vec<SiteNavigationEntry<'a>>, ProjectionError> {
        entries
            .unwrap_or_default()
            .iter()
            .map(|entry| {
                Ok(SiteNavigationEntry {
                    key: entry.key.as_ref(),
                    label: entry.label.as_str(),
                    destination: self.chrome_destination(&entry.destination, false)?,
                })
            })
            .collect()
    }

    fn chrome_destination<'a>(
        &'a self,
        id: &Key,
        page_required: bool,
    ) -> Result<ResolvedDestination<'a>, ProjectionError> {
        let node = self
            .content()
            .graph()
            .node(id)
            .ok_or_else(|| ProjectionError::Binding {
                id: id.clone(),
                reason: "selected chrome destination is absent from the resolved content graph",
            })?;
        if page_required && !matches!(node.structure, ContentKind::Page { .. }) {
            return Err(ProjectionError::Binding {
                id: id.clone(),
                reason: "no-script search destination must select an existing page",
            });
        }
        let route = self
            .content()
            .route(id)
            .ok_or_else(|| ProjectionError::Binding {
                id: id.clone(),
                reason: "selected chrome destination has no configured canonical route",
            })?;
        Ok(ResolvedDestination {
            content: ContentView::new(
                node,
                self.content().presentation_for(&node.site),
                Some(route),
                self.content().graph().is_documentation(&node.id),
                self.content().is_listing(node),
            ),
            route,
        })
    }

    /// Resolve workspace-only copy and destinations before Theme admission.
    ///
    /// Absence remains absence. Every configured destination must already exist and
    /// be routed; the optional no-script search destination must be a page.
    ///
    /// # Errors
    ///
    /// Returns a projection binding error when a visible control lacks copy, a
    /// selected destination is missing or unrouted, or the no-script destination
    /// is not a page.
    pub fn site_chrome<'a>(
        &'a self,
        site: &Key,
    ) -> Result<Option<SiteChromeView<'a>>, ProjectionError> {
        let Some(composition) = self.content().compilation().workspace.composition_for(site) else {
            return Ok(None);
        };
        self.site_chrome_with_composition(site, composition)
    }

    /// Resolve copy from an admitted composition, including declared Theme defaults.
    ///
    /// # Errors
    /// Rejects missing visible-control labels and invalid routed destinations.
    pub fn site_chrome_with_composition<'a>(
        &'a self,
        site: &Key,
        composition: &'a ResolvedComposition,
    ) -> Result<Option<SiteChromeView<'a>>, ProjectionError> {
        self.validate_invitations(site, composition)?;
        let required = |value: &'a Option<String>| {
            value.as_deref().ok_or_else(|| ProjectionError::Binding {
                id: site.clone(),
                reason: "visible chrome requires a declared value for every control label",
            })
        };
        let Some(selected) = composition.chrome() else {
            if composition.navigation().is_some() || composition.footer().is_some() {
                return Err(ProjectionError::Binding {
                    id: site.clone(),
                    reason: "selected navigation or footer requires explicit shared chrome copy",
                });
            }
            return Ok(None);
        };
        let chrome = selected.value();
        let navigation = self.chrome_entries(
            composition
                .navigation()
                .map(|value| value.value().as_slice()),
        )?;
        let footer =
            self.chrome_entries(composition.footer().map(|value| value.value().as_slice()))?;
        let search = chrome
            .shell
            .search
            .as_ref()
            .filter(|copy| copy.enabled != Some(false))
            .map(|copy| {
                Ok(SearchControlView {
                    page: self
                        .content()
                        .publication_search_page(site)
                        .map_err(|error| ProjectionError::Binding {
                            id: error.id,
                            reason: error.reason,
                        })?
                        .map(|id| self.chrome_destination(id, true))
                        .transpose()?,
                    trigger_label: required(&copy.trigger_label)?,
                    title: required(&copy.title)?,
                    field_label: required(&copy.field_label)?,
                    loading_message: required(&copy.loading_message)?,
                    empty_message: required(&copy.empty_message)?,
                    unavailable_message: required(&copy.unavailable_message)?,
                    no_script_destination: copy
                        .no_script_destination
                        .as_ref()
                        .map(|id| self.chrome_destination(id, true))
                        .transpose()?,
                })
            })
            .transpose()?;
        Ok(Some(SiteChromeView {
            skip_to_content_label: required(&chrome.skip_to_content_label)?,
            navigation_label: required(&chrome.navigation_label)?,
            footer_label: required(&chrome.footer_label)?,
            shell: ShellView {
                close_label: required(&chrome.shell.close_label)?,
                search,
                settings: SettingsControlView {
                    trigger_label: required(&chrome.shell.settings.trigger_label)?,
                    title: required(&chrome.shell.settings.title)?,
                    color_mode_label: required(&chrome.shell.settings.color_mode_label)?,
                    color_mode_system_label: required(
                        &chrome.shell.settings.color_mode_system_label,
                    )?,
                    color_mode_light_label: required(
                        &chrome.shell.settings.color_mode_light_label,
                    )?,
                    color_mode_dark_label: required(&chrome.shell.settings.color_mode_dark_label)?,
                    motion_label: required(&chrome.shell.settings.motion_label)?,
                    motion_system_label: required(&chrome.shell.settings.motion_system_label)?,
                    motion_reduce_label: required(&chrome.shell.settings.motion_reduce_label)?,
                    motion_allow_label: required(&chrome.shell.settings.motion_allow_label)?,
                },
            },
            navigation,
            footer,
            invitation_destinations: composition
                .invitations()
                .iter()
                .filter_map(|(key, selection)| {
                    let InvitationSelection::Enabled { destination, .. } = selection.value() else {
                        return None;
                    };
                    Some(
                        self.chrome_destination(destination, false)
                            .map(|destination| (key.clone(), destination)),
                    )
                })
                .collect::<Result<_, _>>()?,
            regions: &chrome.regions,
            reading_assistance: chrome.reading_assistance.as_ref(),
        }))
    }
}
