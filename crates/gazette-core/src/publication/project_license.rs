//! Exact project software-licence expressions, without rights inference or approval.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

/// An owner-evidenced SPDX licence expression for one project's software.
///
/// The exact admitted spelling is retained after strict SPDX parsing. This value
/// does not identify the content licence, establish chain of title or authorize
/// publication. An unknown or held project licence remains absent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ProjectLicense(String);

impl schemars::JsonSchema for ProjectLicense {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ProjectLicense".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "description": "Exact strictly valid SPDX expression supplied by the project's rights owner; not a content licence or permission proof.",
            "minLength": 1
        })
    }
}

/// A project software licence was not a strict SPDX expression.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidProjectLicense;

impl fmt::Display for InvalidProjectLicense {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("expected an exact strictly valid SPDX licence expression")
    }
}

impl std::error::Error for InvalidProjectLicense {}

impl ProjectLicense {
    /// Borrow the exact admitted expression without canonicalizing owner bytes.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for ProjectLicense {
    type Err = InvalidProjectLicense;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.is_empty() || value.trim() != value || spdx::Expression::parse(value).is_err() {
            return Err(InvalidProjectLicense);
        }
        Ok(Self(value.to_owned()))
    }
}

impl TryFrom<String> for ProjectLicense {
    type Error = InvalidProjectLicense;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<ProjectLicense> for String {
    fn from(value: ProjectLicense) -> Self {
        value.0
    }
}

impl fmt::Display for ProjectLicense {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}
