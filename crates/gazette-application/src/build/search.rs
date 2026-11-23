//! One search artifact address shared by presentation and complete-site emission.

use gazette_compiler::{
    authored::ResolvedDestination,
    workspace::{ResolvedContent, SearchField, SearchProfile},
};
use gazette_core::{
    publication::{Key, routing::InvalidRoute, routing::RoutePath},
    render,
    source::EntryPath,
};
use serde::Serialize;

/// Planned local artifact binding, not a ranking provider or proof of deployment.
#[derive(Serialize, schemars::JsonSchema)]
pub(super) struct Search<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<ResolvedDestination<'a>>,
    pub profile: SearchProfile,
    pub fields: &'a [SearchField],
    pub route: RoutePath,
    pub output: EntryPath,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub no_script_destination: Option<ResolvedDestination<'a>>,
}

pub(super) fn binding<'a>(
    content: &ResolvedContent<'a>,
    site: &Key,
    control: Option<&gazette_compiler::authored::SearchControlView<'a>>,
) -> Result<Option<Search<'a>>, render::Error> {
    let Some(selection) = content
        .compilation()
        .workspace
        .publication_for(site)
        .and_then(|publication| publication.machine.search.as_ref())
    else {
        return Ok(None);
    };
    let invalid = |error| render::Error::Context(Box::new(error));
    let base = content
        .base_url(site)
        .ok_or_else(|| invalid(InvalidRoute))?;
    let route: RoutePath = "/search.json".parse().map_err(invalid)?;
    Ok(Some(Search {
        page: control.and_then(|control| control.page.clone()),
        profile: selection.profile(),
        fields: selection.fields(),
        route: route.clone(),
        output: route.output().clone(),
        url: base.resolve(&route),
        no_script_destination: control.and_then(|control| control.no_script_destination.clone()),
    }))
}
