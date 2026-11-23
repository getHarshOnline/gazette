//! Site interface copy, independent of navigation and capability availability.

use super::{Key, ReadingAssistance};

mod inheritance;
pub use inheritance::ResolvedChrome;
pub(super) use inheritance::{inherit_theme, resolve};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Declared site-interface copy. Omitted labels inherit from the next declared layer.
///
/// These strings are presentation copy only. They establish no identity, rights,
/// source ownership, capability availability or publication permission.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SiteChrome {
    /// First-focusable link text for reaching the main landmark.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skip_to_content_label: Option<String>,
    /// Accessible name of the site-navigation landmark.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub navigation_label: Option<String>,
    /// Accessible name of the site footer landmark.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub footer_label: Option<String>,
    /// Shared command-shell copy.
    #[serde(default)]
    pub shell: ShellCopy,
    /// Additional generic Theme-region copy, keyed by an owner namespace.
    ///
    /// Gazette validates and transports these strings but assigns no region
    /// meaning, world, layout or visual priority.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub regions: BTreeMap<Key, String>,
    /// Optional explicit external handoffs for reader-selected article text.
    /// Selection declares endpoints and copy, not remote provider availability or approval.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reading_assistance: Option<ReadingAssistance>,
}

/// Copy for the shared command shell and its closed control families.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ShellCopy {
    /// Label for dismissing the active shell tenant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub close_label: Option<String>,
    /// Search-control copy; omitted labels inherit, while `enabled = false` suppresses it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search: Option<SearchControlCopy>,
    /// Settings controls available independently from search.
    #[serde(default)]
    pub settings: SettingsControlCopy,
}

/// Closed copy and fallback destination for the Gazette search control.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchControlCopy {
    /// Explicitly suppress inherited search copy without changing provider selection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Label for opening search.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_label: Option<String>,
    /// Search tenant heading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Search field label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field_label: Option<String>,
    /// Honest state while the selected search corpus is loading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loading_message: Option<String>,
    /// Honest state when a query returns no results.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub empty_message: Option<String>,
    /// Honest state when selected search cannot run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unavailable_message: Option<String>,
    /// Optional complete HTML page used when scripts are unavailable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_script_destination: Option<Key>,
}

/// Independently inherited labels for settings controls.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SettingsControlCopy {
    /// Label for opening settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_label: Option<String>,
    /// Settings tenant heading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Color-mode fieldset label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_mode_label: Option<String>,
    /// Follow-platform color-mode option.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_mode_system_label: Option<String>,
    /// Light color-mode option.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_mode_light_label: Option<String>,
    /// Dark color-mode option.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_mode_dark_label: Option<String>,
    /// Motion-preference fieldset label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub motion_label: Option<String>,
    /// Follow-platform motion option.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub motion_system_label: Option<String>,
    /// Reduced-motion option.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub motion_reduce_label: Option<String>,
    /// Full-motion option.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub motion_allow_label: Option<String>,
}

impl SiteChrome {
    pub(crate) fn copy_fields(&self) -> Vec<(&'static str, &str)> {
        let mut fields = Vec::new();
        if let Some(value) = &self.skip_to_content_label {
            fields.push(("skip_to_content_label", value.as_str()));
        }
        if let Some(value) = &self.navigation_label {
            fields.push(("navigation_label", value.as_str()));
        }
        if let Some(value) = &self.footer_label {
            fields.push(("footer_label", value.as_str()));
        }
        if let Some(value) = &self.shell.close_label {
            fields.push(("shell.close_label", value.as_str()));
        }
        if let Some(value) = &self.shell.settings.trigger_label {
            fields.push(("shell.settings.trigger_label", value.as_str()));
        }
        if let Some(value) = &self.shell.settings.title {
            fields.push(("shell.settings.title", value.as_str()));
        }
        if let Some(value) = &self.shell.settings.color_mode_label {
            fields.push(("shell.settings.color_mode_label", value.as_str()));
        }
        if let Some(value) = &self.shell.settings.color_mode_system_label {
            fields.push(("shell.settings.color_mode_system_label", value.as_str()));
        }
        if let Some(value) = &self.shell.settings.color_mode_light_label {
            fields.push(("shell.settings.color_mode_light_label", value.as_str()));
        }
        if let Some(value) = &self.shell.settings.color_mode_dark_label {
            fields.push(("shell.settings.color_mode_dark_label", value.as_str()));
        }
        if let Some(value) = &self.shell.settings.motion_label {
            fields.push(("shell.settings.motion_label", value.as_str()));
        }
        if let Some(value) = &self.shell.settings.motion_system_label {
            fields.push(("shell.settings.motion_system_label", value.as_str()));
        }
        if let Some(value) = &self.shell.settings.motion_reduce_label {
            fields.push(("shell.settings.motion_reduce_label", value.as_str()));
        }
        if let Some(value) = &self.shell.settings.motion_allow_label {
            fields.push(("shell.settings.motion_allow_label", value.as_str()));
        }
        if let Some(search) = &self.shell.search {
            if let Some(value) = &search.trigger_label {
                fields.push(("shell.search.trigger_label", value.as_str()));
            }
            if let Some(value) = &search.title {
                fields.push(("shell.search.title", value.as_str()));
            }
            if let Some(value) = &search.field_label {
                fields.push(("shell.search.field_label", value.as_str()));
            }
            if let Some(value) = &search.loading_message {
                fields.push(("shell.search.loading_message", value.as_str()));
            }
            if let Some(value) = &search.empty_message {
                fields.push(("shell.search.empty_message", value.as_str()));
            }
            if let Some(value) = &search.unavailable_message {
                fields.push(("shell.search.unavailable_message", value.as_str()));
            }
        }
        fields
    }

    pub(crate) fn valid_labels(&self) -> bool {
        self.regions.len() <= 256
            && self
                .copy_fields()
                .iter()
                .all(|(_, value)| !super::invalid_copy(value))
            && self
                .regions
                .values()
                .all(|value| !super::invalid_copy(value))
    }

    fn label_slot(&mut self, field: &str) -> Option<&mut Option<String>> {
        match field {
            "skip_to_content_label" => Some(&mut self.skip_to_content_label),
            "navigation_label" => Some(&mut self.navigation_label),
            "footer_label" => Some(&mut self.footer_label),
            "shell.close_label" => Some(&mut self.shell.close_label),
            "shell.settings.trigger_label" => Some(&mut self.shell.settings.trigger_label),
            "shell.settings.title" => Some(&mut self.shell.settings.title),
            "shell.settings.color_mode_label" => Some(&mut self.shell.settings.color_mode_label),
            "shell.settings.color_mode_system_label" => {
                Some(&mut self.shell.settings.color_mode_system_label)
            }
            "shell.settings.color_mode_light_label" => {
                Some(&mut self.shell.settings.color_mode_light_label)
            }
            "shell.settings.color_mode_dark_label" => {
                Some(&mut self.shell.settings.color_mode_dark_label)
            }
            "shell.settings.motion_label" => Some(&mut self.shell.settings.motion_label),
            "shell.settings.motion_system_label" => {
                Some(&mut self.shell.settings.motion_system_label)
            }
            "shell.settings.motion_reduce_label" => {
                Some(&mut self.shell.settings.motion_reduce_label)
            }
            "shell.settings.motion_allow_label" => {
                Some(&mut self.shell.settings.motion_allow_label)
            }
            "shell.search.trigger_label" => {
                Some(&mut self.shell.search.get_or_insert_default().trigger_label)
            }
            "shell.search.title" => Some(&mut self.shell.search.get_or_insert_default().title),
            "shell.search.field_label" => {
                Some(&mut self.shell.search.get_or_insert_default().field_label)
            }
            "shell.search.loading_message" => {
                Some(&mut self.shell.search.get_or_insert_default().loading_message)
            }
            "shell.search.empty_message" => {
                Some(&mut self.shell.search.get_or_insert_default().empty_message)
            }
            "shell.search.unavailable_message" => Some(
                &mut self
                    .shell
                    .search
                    .get_or_insert_default()
                    .unavailable_message,
            ),
            _ => None,
        }
    }
}
