//! One readable URL component using the same confinement rules as full routes.
use super::{InvalidRoute, RoutePath};
use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// Explicit address label, never derived from an identity or display title.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(try_from = "String", into = "String")]
#[schemars(with = "String")]
pub struct RouteSlug(String);

impl FromStr for RouteSlug {
    type Err = InvalidRoute;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.is_empty() || value.contains('/') {
            return Err(InvalidRoute);
        }
        let path: RoutePath = format!("/{value}/").parse()?;
        Ok(Self(path.as_str().trim_matches('/').to_owned()))
    }
}
impl TryFrom<String> for RouteSlug {
    type Error = InvalidRoute;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}
impl From<RouteSlug> for String {
    fn from(value: RouteSlug) -> Self {
        value.0
    }
}
impl RouteSlug {
    /// Normalized encoded component, without surrounding separators.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl RoutePath {
    /// Append one validated slug beneath a directory, retaining directory form.
    ///
    /// # Errors
    /// A file route cannot contain a child page.
    pub fn join_slug(&self, slug: &RouteSlug) -> Result<Self, InvalidRoute> {
        if !self.as_str().ends_with('/') {
            return Err(InvalidRoute);
        }
        format!("{}{}/", self.as_str(), slug.as_str()).parse()
    }
}
