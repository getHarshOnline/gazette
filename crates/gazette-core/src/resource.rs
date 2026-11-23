//! Retained-source resource compilation, independent of provider selection and output placement.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    str::FromStr,
};

use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

mod svg_layout;
pub use svg_layout::{ImageLayout, SvgLayout};

use crate::source::{self, Content, EntryPath, Inventory, TreeIdentity};

/// Gazette-owned logical resource identity, independent of package and output placement.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ResourceId(String);

/// A logical resource identity lacks a valid lower-kebab kind or local name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidResourceId;

impl ResourceId {
    /// Exact declared spelling; no package, provider, path, or output is inferred from it.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for ResourceId {
    type Err = InvalidResourceId;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if valid_resource_id(value) {
            Ok(Self(value.to_owned()))
        } else {
            Err(InvalidResourceId)
        }
    }
}

impl<'de> Deserialize<'de> for ResourceId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

impl schemars::JsonSchema for ResourceId {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ResourceId".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "description": "Gazette logical resource identity, such as module:commerce-selector.",
            "pattern": "^[a-z][a-z0-9]*(?:-[a-z][a-z0-9]*)*:[a-z][a-z0-9]*(?:-[a-z][a-z0-9]*)*$"
        })
    }
}

impl fmt::Display for ResourceId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl fmt::Display for InvalidResourceId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("invalid ResourceId")
    }
}

impl std::error::Error for InvalidResourceId {}

fn valid_resource_id(value: &str) -> bool {
    value
        .split_once(':')
        .is_some_and(|(kind, name)| valid_plain_id(kind) && valid_plain_id(name))
}

fn valid_plain_id(value: &str) -> bool {
    let mut segments = value.split('-');
    segments.next().is_some_and(valid_segment) && segments.all(valid_segment)
}

fn valid_segment(value: &str) -> bool {
    !value.is_empty()
        && value.as_bytes()[0].is_ascii_lowercase()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
}

/// Workspace-resolved module identities and their canonical output URLs.
/// These are links for generated code, never permission for native or remote reads.
pub type ModuleLinks = BTreeMap<ResourceId, String>;

/// Output semantics derived from a checked recipe, never from its name or destination suffix.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// A compiled ECMAScript module for module-script consumption.
    BrowserModule,
    /// Compiled CSS for stylesheet consumption.
    Stylesheet,
    /// Transformed PNG pixels, independent of the destination suffix.
    PngImage,
    /// Transformed JPEG pixels, independent of the destination suffix.
    JpegImage,
    /// Lossless WebP pixels, independent of the destination suffix.
    WebpImage,
    /// Validated inert SVG bytes for image consumption.
    SvgImage,
    /// Validated retained WOFF2 bytes for webfont consumption.
    Woff2Font,
}

/// Explicit compilation profile; an unordered input set never implies an entrypoint.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Recipe {
    /// Static DOT input requiring an explicitly acquired, retained PNG before pure rendering.
    /// Native renderer dependencies are not a sandboxed or inferred source closure.
    DotImage {
        /// Exact captured DOT entry; the initial profile admits no other declared files.
        #[serde(deserialize_with = "entry_path")]
        entry: EntryPath,
    },
    /// One JavaScript/TypeScript ESM entry and its captured relative imports.
    /// TypeScript syntax erasure is not type checking. No package discovery; dynamic imports require a literal declared alias.
    BrowserModule {
        /// Exact retained file, also required in the compiler's declared input set.
        #[serde(deserialize_with = "entry_path")]
        entry: EntryPath,
        /// Literal non-relative specifiers bound by the workspace to shared modules.
        /// Import declarations name logical identities, not source paths or URLs.
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        imports: BTreeMap<String, ResourceId>,
    },
    /// One retained stylesheet entry and its confined Sass dependency closure.
    Stylesheet {
        /// Exact retained file, also required in the compiler's declared input set.
        #[serde(deserialize_with = "entry_path")]
        entry: EntryPath,
    },
    /// One retained raster transformed without renderer-side native reads.
    RasterImage {
        /// Exact retained input; filenames do not determine the decoder.
        #[serde(deserialize_with = "entry_path")]
        entry: EntryPath,
        /// Explicit encoded input representation.
        input_format: crate::raster::Format,
        /// Pixel operation after selected orientation handling.
        resize: crate::raster::Resize,
        /// Explicit encoded output and alpha handling.
        encoding: crate::raster::Encoding,
        /// Apply the decoder's stored orientation before resizing when true.
        apply_orientation: bool,
    },
    /// One retained SVG admitted against the bounded inert-image profile.
    SvgImage {
        /// Exact retained SVG; the initial profile admits no companion files.
        #[serde(deserialize_with = "entry_path")]
        entry: EntryPath,
    },
    /// One retained WOFF2 font validated through complete container decoding.
    Woff2Font {
        /// Exact retained font; the initial profile admits no companion files.
        #[serde(deserialize_with = "entry_path")]
        entry: EntryPath,
    },
}

fn entry_path<'de, D: Deserializer<'de>>(deserializer: D) -> Result<EntryPath, D::Error> {
    String::deserialize(deserializer)?
        .parse()
        .map_err(D::Error::custom)
}

impl Recipe {
    /// The exact selected entry, independent of declaration ordering.
    pub fn entry(&self) -> &EntryPath {
        match self {
            Self::DotImage { entry }
            | Self::BrowserModule { entry, .. }
            | Self::Stylesheet { entry }
            | Self::RasterImage { entry, .. }
            | Self::SvgImage { entry }
            | Self::Woff2Font { entry } => entry,
        }
    }

    /// Check the declared import namespace before linking or native compilation.
    ///
    /// # Errors
    /// Refuses ambiguous relative/URL-like aliases and unbounded declarations.
    pub fn validate_imports(&self) -> Result<(), Error> {
        let Self::BrowserModule { entry, imports } = self else {
            return Ok(());
        };
        if imports.len() > 512 {
            return Err(Error::Limit("module import bindings"));
        }
        for alias in imports.keys() {
            if alias.is_empty()
                || alias.len() > 256
                || alias.starts_with(['.', '/'])
                || !alias
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"@/_-.".contains(&b))
                || alias.split('/').any(|part| matches!(part, "" | "." | ".."))
            {
                return Err(Error::Input {
                    path: entry.clone(),
                    reason: "module alias must be a bounded literal non-relative specifier",
                });
            }
        }
        Ok(())
    }

    /// The resource's consumer contract, independent of output placement.
    pub fn kind(&self) -> Kind {
        match self {
            Self::DotImage { .. } => Kind::PngImage,
            Self::BrowserModule { .. } => Kind::BrowserModule,
            Self::Stylesheet { .. } => Kind::Stylesheet,
            Self::RasterImage { encoding, .. } => match encoding {
                crate::raster::Encoding::Png => Kind::PngImage,
                crate::raster::Encoding::Webp => Kind::WebpImage,
                crate::raster::Encoding::Jpeg { .. } => Kind::JpegImage,
            },
            Self::SvgImage { .. } => Kind::SvgImage,
            Self::Woff2Font { .. } => Kind::Woff2Font,
        }
    }
}

/// Per-compilation ceilings; these are not a sandbox or execution-time guarantee.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Limits {
    /// Complete declared input closure, including unused prerequisites.
    pub inputs: source::Limits,
    /// Maximum emitted bytes before returning a successful artifact.
    pub output_bytes: usize,
    /// Image input/output dimensions and individual decoded-buffer ceilings.
    pub raster: crate::raster::Limits,
    /// Retained WOFF2 input and reconstructed-font ceilings.
    pub font: crate::font::Limits,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            inputs: source::Limits {
                entries: 512,
                depth: 32,
                file_bytes: 256 << 10,
                total_bytes: 4 << 20,
            },
            output_bytes: 8 << 20,
            raster: crate::raster::Limits::default(),
            font: crate::font::Limits::default(),
        }
    }
}

impl Limits {
    /// Check the complete declared input inventory before invoking a resource compiler.
    ///
    /// # Errors
    /// Rejects entry, depth, per-file or aggregate byte excess, including unused inputs.
    pub fn validate_input(&self, inventory: &Inventory) -> Result<(), Error> {
        if inventory.entries().len() > self.inputs.entries {
            return Err(Error::Limit("resource input entries"));
        }
        if inventory.total_bytes() > self.inputs.total_bytes {
            return Err(Error::Limit("resource input bytes"));
        }
        for entry in inventory.entries() {
            if entry.path.as_str().split('/').count() > self.inputs.depth {
                return Err(Error::Limit("resource input depth"));
            }
            if let Content::File { bytes, .. } = entry.content
                && bytes > self.inputs.file_bytes
            {
                return Err(Error::Limit("resource input file bytes"));
            }
        }
        Ok(())
    }
}

/// Compiler result, checked by the application before it enters output assembly.
#[derive(Clone, Debug)]
pub struct Artifact {
    /// Complete supplied tree identity, not merely the entry's hash.
    pub input: TreeIdentity,
    /// Complete emitted resource, never an uncompiled source fallback.
    pub bytes: Vec<u8>,
    /// Exact reachable source files, including the entry, in deterministic order.
    pub dependencies: BTreeSet<EntryPath>,
    /// Decoded URLs referenced by emitted bytes, distinct from compiler source inputs.
    /// Placement and complete output-target validation belong to the application.
    pub references: BTreeSet<String>,
    /// References reached only by dynamic import, excluding any static edge to the same URL.
    /// This subset is emitted and validated, but omitted from initial preload traversal.
    pub deferred_references: BTreeSet<String>,
    /// Verified SVG root viewport sizing; absent for other resource kinds.
    pub svg: Option<SvgLayout>,
    /// Actual raster geometry; present only for an image recipe, never inferred from its name.
    pub image: Option<crate::raster::Geometry>,
    /// Qualified implementation/profile identity; no wall clock or native path.
    pub tool: &'static str,
}

/// Compilation rejection; no partial successful output is returned.
#[derive(Debug)]
pub enum Error {
    /// A selected file or import violates the supported captured-source profile.
    Input {
        /// Exact logical source file.
        path: EntryPath,
        /// Violated invariant, without native source discovery.
        reason: &'static str,
    },
    /// Caller-selected resource ceiling was exhausted.
    Limit(&'static str),
    /// Native parser, transform or bundler diagnostic.
    Compile(String),
    /// Shared retained-tree validation failed.
    Source(source::Error),
}

impl Error {
    /// Stable interface diagnostic class.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Input { .. } => "resource_input",
            Self::Limit(_) => "resource_limit",
            Self::Compile(_) => "resource_compile",
            Self::Source(error) => error.code(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Input { path, reason } => write!(f, "resource {path}: {reason}"),
            Self::Limit(resource) => write!(f, "resource compilation exceeds {resource}"),
            Self::Compile(error) => write!(f, "resource compilation: {error}"),
            Self::Source(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Source(error) => Some(error),
            _ => None,
        }
    }
}
