//! Machine-readable schemas projected from the compiler's native declarations.
//!
//! These functions expose the same Rust types consumed by compilation. They do
//! not introduce a second configuration language or promise wire compatibility
//! for Gazette's currently unreleased local contracts.

/// Draft 2020-12 schema for one complete Theme-package manifest.
#[must_use]
pub fn theme_manifest() -> schemars::Schema {
    schemars::generate::SchemaSettings::draft2020_12()
        .for_serialize()
        .into_generator()
        .into_root_schema_for::<crate::theme_package::Manifest>()
}

/// Draft 2020-12 schema for one complete workspace declaration.
#[must_use]
pub fn workspace() -> schemars::Schema {
    crate::workspace::declaration_schema()
}

/// Draft 2020-12 schema for authored publication frontmatter, excluding its fences and body.
#[must_use]
pub fn authored_content() -> schemars::Schema {
    crate::authored::declaration_schema()
}

/// Draft 2020-12 schema for reduced owner-held legal-source frontmatter.
/// Output identity, site and route remain explicit workspace bindings.
#[must_use]
pub fn legal_source() -> schemars::Schema {
    crate::authored::legal_source_schema()
}
