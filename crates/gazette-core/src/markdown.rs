//! Adapter-neutral Markdown facts and resolved formatting inputs, never a second AST.

use crate::{
    publication::{
        Key,
        content::{ContentKind, PageFamily},
    },
    source::{Location, Sha256Digest},
};
use serde::Serialize;

mod container_contents;
mod project_resources;
pub use container_contents::{
    ContainerContentsArtwork, ContainerContentsBinding, ContainerContentsEntry,
    ContainerContentsEvent, ContainerContentsKind, ContainerContentsPresentation,
    ContainerContentsView,
};
pub use project_resources::{
    ProjectResourceAction, ProjectResourceDestination, ProjectResourceKind,
    ProjectResourcePresentation, ProjectResourcesView,
};
mod name;
pub use name::{ComponentName, InvalidComponentName};
mod image_delivery;
mod inline_svg;
pub use image_delivery::{ImageFetchPriority, ImageLoading};
pub use inline_svg::{InlineSvg, SvgEvent};

/// Gazette-selected syntax, translated by the concrete adapter rather than exposing its options.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum Profile {
    /// `CommonMark`, tables, task lists, strikethrough, footnotes and heading attributes.
    #[default]
    Publication,
    /// Publication syntax with explicitly selected dollar and code-fence TeX.
    /// Recognizing formulas does not establish a typesetter or permission to execute one.
    PublicationMath,
}

/// Logical ceilings; adapter allocation before node/depth inspection is not sandboxed.
#[derive(Clone, Copy, Debug, Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    /// Maximum exact UTF-8 input bytes, checked before parsing.
    #[schemars(range(max = usize::MAX))]
    pub source_bytes: usize,
    /// Maximum syntax nodes, including the document root.
    #[schemars(range(max = usize::MAX))]
    pub nodes: usize,
    /// Maximum node nesting below the document root.
    #[schemars(range(max = usize::MAX))]
    pub depth: usize,
    /// Aggregate strings retained by source analysis, independent of projection bounds.
    #[schemars(range(max = usize::MAX))]
    pub fact_bytes: usize,
    /// Independent ceiling for each inspection, formatting plan and rendered projection.
    #[schemars(range(max = usize::MAX))]
    pub output_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            source_bytes: 1024 * 1024,
            nodes: 100_000,
            depth: 128,
            fact_bytes: 4 * 1024 * 1024,
            output_bytes: 4 * 1024 * 1024,
        }
    }
}

/// Exact inputs to which analysis and subsequent formatting are bound.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    /// Digest of original bytes, not normalized text or output.
    pub source_sha256: Sha256Digest,
    /// Gazette syntax selection used for these bytes.
    pub profile: Profile,
}

/// Navigational and embedded-resource occurrences have different consumers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LinkKind {
    /// A navigational destination.
    Link,
    /// An embedded image destination.
    Image,
}

/// One source-bound heading declaration, before Gazette assigns its final anchor.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct Heading {
    /// Original containing construct.
    pub location: Location,
    /// Heading depth from one through six.
    #[schemars(range(min = 1, max = 6))]
    pub level: u8,
    /// Inline text, code and image labels, without footnote reference labels.
    pub text: String,
    /// Optional authored ID; collision policy remains in the compiler.
    pub id: Option<String>,
    /// Authored presentation classes.
    pub classes: Vec<String>,
    /// Other authored attribute pairs, retained for explicit admission or rejection.
    pub attributes: Vec<(String, String)>,
    /// False for headings in a definition omitted from normalized publication output.
    pub emitted: bool,
}

/// Parser-recognized reference spelling, not destination or publication admission.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LinkForm {
    /// Native Markdown links, autolinks and images.
    Markdown,
    /// Native URL-first double-bracket links; their destination is not yet admitted.
    WikiLink,
}

/// One syntactic reference, not an inferred publication relationship.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    /// Original containing link/image construct.
    pub location: Location,
    /// Reference purpose.
    pub kind: LinkKind,
    /// Parser-recognized source form, independent of the destination's spelling.
    pub form: LinkForm,
    /// Decoded effective URI, including `mailto:` for email autolinks.
    pub destination: String,
    /// Decoded optional title, empty when absent.
    pub title: String,
    /// Native plain-text image label; Some(empty) preserves an authored decorative image.
    pub image_alt: Option<String>,
    /// False for destinations rendered only as image-alt text or omitted definitions.
    pub emitted: bool,
}

/// Untrusted HTML syntax; only compiler-admitted constructs may be formatted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Html {
    /// Containing original construct.
    pub location: Location,
    /// Parser-recognized literal, not trusted markup.
    pub literal: String,
    /// A summary boundary cannot be admitted in these contexts.
    pub in_footnote_or_image: bool,
}

/// Footnote source facts; Gazette owns emitted closure while the formatter owns markup.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct Footnote {
    /// Definition's containing construct.
    pub location: Location,
    /// Parser-normalized association label, not a public DOM ID.
    pub name: String,
    /// Number of emitted references derived from retained source structure.
    #[schemars(range(max = u32::MAX))]
    pub references: u32,
}

/// Located association evidence, distinct from formatter-generated numbering markup.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct FootnoteReference {
    /// Original reference's containing source construct.
    pub location: Location,
    /// Normalized label joining the retained definition fact.
    pub name: String,
    /// One-based ordinal within the emitted reference closure; zero when not emitted.
    #[schemars(range(max = u32::MAX))]
    pub number: u32,
    /// Containing definition label, if the reference occurs inside a footnote.
    pub definition: Option<String>,
    /// Whether the source occurrence is nested inside image-alt content.
    /// Such references remain source evidence but never participate in footnote output.
    pub in_image: bool,
    /// Whether this reference survives normalized rendering.
    pub emitted: bool,
}

/// Located text projection pieces allow a recognized marker to be omitted without offset surgery.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Text {
    /// Containing source construct; structural separators may share its location.
    pub location: Location,
    /// Text/code/alt or an explicit structural separator, not reconstructed HTML.
    pub value: String,
}

/// One native-parser formula, before generation or output admission.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Math {
    /// Original containing construct, never a generated-output coordinate.
    pub location: Location,
    /// Native literal TeX without its Markdown delimiters.
    pub tex: String,
    /// Display layout selected by the source construct, not inferred from TeX.
    pub display: bool,
    /// False inside an image label or an unused footnote definition.
    pub emitted: bool,
}

/// A native block directive; its consumer owns name and argument interpretation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct Directive {
    /// Original containing block, before any normalization or excerpt truncation.
    pub location: Location,
    /// Exact original first-line bytes after the native opening fence.
    pub header: Location,
    /// Unnormalized header, including whitespace and escapes; never adapter-decoded arguments.
    pub info: String,
    /// Whether this block survives normalized publication rendering.
    pub emitted: bool,
}

/// Only consumer-required facts survive analysis; parser trees remain private and disposable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Syntax {
    /// Exact analysis binding.
    pub identity: Identity,
    /// Inspected syntax node count.
    pub node_count: usize,
    /// Whole-source headings in source order.
    pub headings: Vec<Heading>,
    /// Whole-source references, including non-emitted occurrences for safety inspection.
    pub links: Vec<Link>,
    /// Whole-source raw HTML, including potential excerpt markers.
    pub html: Vec<Html>,
    /// All retained definitions, including unused definitions.
    pub footnotes: Vec<Footnote>,
    /// Reference-to-definition provenance, including non-emitted source references.
    pub footnote_references: Vec<FootnoteReference>,
    /// Explicit-profile formulas, including non-emitted source occurrences.
    pub math: Vec<Math>,
    /// Source-bound component candidates; no name or argument admission is implied.
    pub directives: Vec<Directive>,
    /// Whole-source textual inspection, not normalized publication search text.
    pub text: Vec<Text>,
}

/// An exact heading occurrence and its consumer-allocated final DOM ID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeadingBinding {
    /// Complete original fact, checked against retained analysis.
    pub occurrence: Heading,
    /// Validated compiler-owned ID, including any embedding namespace.
    pub id: String,
    /// Emitted hierarchy level (1–6), independent of the retained source occurrence.
    /// Embedded commentary can remain subordinate without rewriting authored bytes.
    pub level: u8,
}

/// An exact reference occurrence and its compiler-resolved destination.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkBinding {
    /// Verified responsive candidates for this image's default presentation and its
    /// dark source, when either is declared.
    pub image_sources: Option<ImageSources>,
    /// Explicit loading hint for this image, absent on ordinary links.
    pub image_loading: Option<ImageLoading>,
    /// Explicit relative fetch priority for this image.
    pub image_fetch_priority: Option<ImageFetchPriority>,
    /// Verified SVG viewport sizing, without a raster pixel claim.
    pub svg_layout: Option<crate::resource::SvgLayout>,
    /// Authored display width in CSS pixels, independent from encoded dimensions.
    pub image_width: Option<std::num::NonZeroU32>,
    /// Verified encoded extent for reserving the image's intrinsic aspect ratio.
    pub image_dimensions: Option<crate::raster::Dimensions>,
    /// Strictly admitted static SVG for this image occurrence only.
    pub inline_svg: Option<InlineSvg>,
    /// Optional compiler-owned address for an emitted hosted image, never a source path.
    pub image_address: Option<String>,
    /// Complete original fact; no traversal-position matching.
    pub occurrence: Link,
    /// Already admitted destination URI.
    pub destination: String,
    /// Compiler-proved public destination facts used by the formatter only.
    /// Native source coordinates and unproved authored meaning are never carried here.
    pub annotation: Option<LinkAnnotation>,
}

/// One compiled image URL and its independently observed encoded width.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct ImageCandidate {
    /// Admitted serving URL, never an authored filesystem path.
    pub url: String,
    /// Actual encoded pixel width used as the browser's width descriptor.
    pub width: std::num::NonZeroU32,
}

/// Colour mode a Theme presents before the reader chooses one.
///
/// The formatter uses it only to let the browser select an image's dark source before
/// scripts run; the Theme's stylesheet and settings still own the visible mode.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum InitialColorMode {
    /// Follow the reader's operating-system preference.
    #[default]
    System,
    /// Start light whatever the system preference.
    Light,
    /// Start dark whatever the system preference.
    Dark,
}

impl InitialColorMode {
    /// Media condition under which a dark source applies before the reader chooses.
    pub fn dark_media(self) -> &'static str {
        match self {
            Self::System => "(prefers-color-scheme: dark)",
            Self::Light => "not all",
            Self::Dark => "all",
        }
    }
}

/// An image's dark presentation, selectable by the browser before any script runs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct DarkImageSource {
    /// Theme starting mode that decides the source's initial media condition.
    pub mode: InitialColorMode,
    /// Admitted full dark rendition, used alone when no responsive candidates exist.
    pub url: String,
    /// Dark candidates in strictly increasing width order; empty for one rendition.
    pub candidates: Vec<ImageCandidate>,
}

/// Explicit browser layout sizes with a verified, width-ordered candidate list, and an
/// optional dark source.
///
/// An image with a dark alternative but no responsive candidates carries empty `sizes`
/// and `candidates`, so its `img` keeps only its `src`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct ImageSources {
    /// Authored browser sizes declaration; it does not claim an encoded extent.
    pub sizes: String,
    /// Candidates in strictly increasing encoded width order.
    pub candidates: Vec<ImageCandidate>,
    /// Dark alternative emitted as a `picture` source ahead of the default image.
    pub dark: Option<DarkImageSource>,
}

impl ImageSources {
    /// Admit bounded literal layout syntax, leaving CSS media/length evaluation to the browser.
    pub fn valid_sizes(sizes: &str) -> bool {
        !sizes.trim().is_empty()
            && sizes.len() <= 1024
            && sizes
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b" ()-+*/.,:%".contains(&byte))
    }
}

/// Bounded destination classes available to static HTML consumers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum LinkDestinationKind {
    /// A destination owned by the current Gazette site.
    InternalNative,
    /// A destination owned by another configured Gazette site.
    CrossSite,
    /// A destination outside the configured Gazette site set.
    External,
    /// A compiler-bound static resource or download.
    Resource,
}

/// Existing authored graph evidence that directly relates the source page to a destination.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum LinkRelationship {
    /// Explicit outgoing editorial relatedness.
    Related,
    /// The destination is an explicitly containing collection.
    Collection,
    /// The destination is an explicitly contained collection member.
    CollectionMember,
    /// The destination is an explicitly containing journey.
    Journey,
    /// The destination is an explicitly declared journey member.
    JourneyMember,
}

/// Formatter-facing link annotation assembled from compiler-owned facts.
///
/// Theme package identity is carried as its exact key spelling because the inward Markdown
/// formatter does not own Theme-package declarations. The compiler's public model retains its
/// stronger `SitePresentation` type.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct LinkAnnotation {
    /// Stable page-local occurrence address, independent of native source coordinates.
    pub address: String,
    /// Proven destination class.
    pub kind: LinkDestinationKind,
    /// Known destination content identity, when exact route resolution proves one.
    pub content: Option<Key>,
    /// Known configured destination site, including an unrouted site-local destination.
    pub site: Option<Key>,
    /// Direct authored relationship evidence in stable semantic order; ordinary Markdown supplies none.
    pub relationships: Vec<LinkRelationship>,
    /// Exact effective destination Theme package, when the configured site selects one.
    pub theme: Option<Key>,
    /// Optional Theme-owned destination palette selected with that package.
    pub palette: Option<Key>,
}

/// Safe aliases preserve formatter numbering without allowing label encoding to corrupt fragments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FootnoteBinding {
    /// Complete retained definition fact.
    pub occurrence: Footnote,
    /// Compiler-owned ASCII association alias; the formatter supplies its structural ID prefixes.
    pub alias: String,
}

/// Retained typesetting candidate for one exact native formula.
/// The formatter validates its `MathML` and TeX annotation before emitting any bytes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MathBinding {
    /// Complete source fact, including display mode and original location.
    pub occurrence: Math,
    /// Retained generated fragment; arbitrary authored HTML is not admitted here.
    pub mathml: String,
}

/// Compiler-resolved static presentation over one exact authored invocation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct ComponentBinding {
    /// One admitted container placement; never copied into public component metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container_contents: Option<ContainerContentsBinding>,
    /// Declared semantic text before the body, such as an FAQ item's question.
    /// The compiler selects it from admitted arguments; templates cannot add it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reading_prefix: Option<String>,
    /// Keep this component in HTML while omitting its subtree from reading text and Markdown.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub presentation_only: bool,
    /// Complete original directive fact, checked against the retained analysis.
    pub occurrence: Directive,
    /// Exact Theme-local component name, never a publication identity or template path.
    pub name: ComponentName,
    /// Data-only arguments already checked against the selected definition.
    pub arguments: std::collections::BTreeMap<String, serde_json::Value>,
    /// Compiler-owned occurrence and host context; absent only for unhosted Markdown utilities.
    pub context: Option<ComponentContext>,
}

/// Existing content semantics surrounding one component occurrence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ComponentSurface {
    /// A leaf page retains its explicit page family.
    Page {
        /// Existing authored page-purpose semantics.
        family: PageFamily,
    },
    /// A structural collection landing surface.
    Collection,
    /// An ordered journey landing surface.
    Journey,
}

impl From<&ContentKind> for ComponentSurface {
    fn from(value: &ContentKind) -> Self {
        match value {
            ContentKind::Page { family } => Self::Page {
                family: family.clone(),
            },
            ContentKind::Collection { .. } => Self::Collection,
            ContentKind::Journey { .. } => Self::Journey,
        }
    }
}

/// Minimal semantic host for component evaluation; it contains no source path or byte span.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct ComponentHost {
    /// Exact owning content/page identity.
    pub content: Key,
    /// Exact configured home site.
    pub site: Key,
    /// Existing authored surface kind and page mode.
    pub surface: ComponentSurface,
    /// Shallow resolved resources from exactly one directly declared About project.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_resources: Option<ProjectResourcesView>,
}

/// Stable public identity and proven surrounding context for one emitted invocation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct ComponentContext {
    /// Compiler-owned address formed from the owning content identity and traversal occurrence.
    pub address: String,
    /// One-based occurrence in the exact rendered Markdown region.
    #[schemars(range(min = 1, max = usize::MAX))]
    pub occurrence: usize,
    /// Existing semantic owner and surface facts.
    pub host: ComponentHost,
}

/// Explicit compiler-admitted excerpt boundary and continuation anchor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Excerpt {
    /// Complete recognized HTML occurrence, not an arbitrary source offset.
    pub marker: Html,
    /// Validated compiler-owned continuation DOM ID.
    pub continuation: String,
}

/// Complete formatting choices for one exact analysis, without arbitrary tree callbacks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    /// Must match the retained source/profile exactly.
    pub identity: Identity,
    /// Exactly one binding per retained heading.
    pub headings: Vec<HeadingBinding>,
    /// Exactly one binding per emitted reference.
    pub links: Vec<LinkBinding>,
    /// Exactly one safe association alias per definition.
    pub footnotes: Vec<FootnoteBinding>,
    /// Exactly one retained fragment per emitted formula in the selected math profile.
    pub math: Vec<MathBinding>,
    /// Exactly one resolved binding per emitted directive.
    pub components: Vec<ComponentBinding>,
    /// No implicit excerpt when absent.
    pub excerpt: Option<Excerpt>,
}

/// One admitted provider transport and optional text from that same native syntax.
/// Text is unavailable for adapters which implement only the legacy string transport.
#[derive(Debug, PartialEq, Eq)]
pub struct TransportedMarkdown {
    /// Complete inert provider Markdown/MDX output.
    pub body: String,
    /// Bounded emitted meaning, without whole-source definitions or transport escapes.
    pub text: Option<String>,
}

/// Human and machine projections from the same normalized emitted structure.
#[derive(Debug, PartialEq, Eq)]
pub struct Fragment {
    /// Bounded HTML; no Theme or publication certification is implied.
    pub html: String,
    /// Text from that emitted structure, not whole-source inspection or HTML stripping.
    pub text: String,
}

/// Full body and optional balanced prefix share one resolved plan.
#[derive(Debug, PartialEq, Eq)]
pub struct Rendered {
    /// Complete normalized body.
    pub body: Fragment,
    /// End of a first top-level H1 in the emitted HTML, never a source or guessed HTML range.
    pub opening: Option<OpeningBoundary>,
    /// Explicit prefix without heading IDs or footnote machinery.
    pub excerpt: Option<Fragment>,
}

/// Formatter-proved boundary for a leading title; the full body remains authoritative.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpeningBoundary {
    /// UTF-8 byte offset immediately after the emitted heading and its trailing newline.
    pub heading_end: usize,
}

/// Markdown mechanism failures preserve honest refusal rather than partial successful output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// A named logical ceiling was exceeded.
    Limit(&'static str),
    /// Source coordinates or parser structure could not be validated.
    InvalidSource,
    /// Wrong source/profile, incomplete bindings or ambiguous/mismatched occurrences.
    InvalidPlan,
    /// A specifically unqualified parser construct, with containing source evidence.
    Unsupported {
        /// Source evidence; never an invented exact token edit.
        location: Location,
        /// Stable bounded explanation.
        reason: &'static str,
    },
    /// Formatting failed without a partial output.
    Formatting,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Limit(name) => write!(f, "Markdown {name} ceiling exceeded"),
            Self::InvalidSource => {
                f.write_str("Markdown parser could not prove its source coordinates")
            }
            Self::InvalidPlan => {
                f.write_str("Markdown formatting plan does not match its complete source analysis")
            }
            Self::Unsupported { location, reason } => write!(
                f,
                "Markdown construct at {}..{} is unsupported: {reason}",
                location.span.start, location.span.end
            ),
            Self::Formatting => {
                f.write_str("Markdown formatting failed; no partial output returned")
            }
        }
    }
}

impl std::error::Error for Error {}
