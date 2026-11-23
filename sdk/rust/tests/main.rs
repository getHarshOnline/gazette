//! Native workspace lookup, source discovery and user preference behavior.

#[cfg(feature = "authoring")]
mod derived_routes;
#[cfg(feature = "authoring")]
mod source_sites;

#[cfg(feature = "native-configuration")]
mod source_discovery;

#[cfg(feature = "native-configuration")]
mod workspace_location;

#[cfg(feature = "native-local")]
mod theme_working_sources;

#[cfg(all(feature = "native-configuration", feature = "authoring"))]
mod workspace_capture;

#[cfg(all(feature = "native-configuration", feature = "authoring"))]
mod native_authoring;
