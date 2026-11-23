//! Partial disclosure evidence over immutable output, never certification or approval.

mod css;
mod encoded;
mod font;
mod format;
mod generated_mdx;
mod html;
mod javascript;
mod json;
mod markdown;
mod policy;
mod raster;
pub(crate) mod technical_profile;
mod xml;

pub use policy::{DisclosureClass, DisclosurePattern, DisclosurePolicy, DisclosureRule};

use std::{collections::BTreeMap, fmt};

use gazette_core::{
    publication::Key,
    render::MediaType,
    source::{CapturedSource, Content, EntryPath, Sha256Digest, TreeIdentity},
};
use serde::Serialize;

use super::{RenderedSite, serialization::encode};

/// Explicit selected decoders; missing ports never imply representation coverage.
/// The application remains independent of concrete native adapters.
#[derive(Clone, Copy, Default)]
pub struct DisclosureInspectors<'a> {
    /// Optional retained-image metadata inspector.
    pub raster: Option<&'a gazette_core::raster::InspectMetadata<'a>>,
    /// Optional retained-WOFF2 inspector.
    pub font: Option<&'a gazette_core::font::Inspect<'a>>,
    /// Optional exact-source Markdown parser, with its own recorded profile.
    pub markdown: Option<&'a dyn gazette_ports::markdown::Markdown>,
}

/// Ceilings for one diagnostic probe, independent of the earlier render limits.
#[derive(Clone, Copy, Debug)]
pub struct DisclosureProbeLimits {
    /// Maximum literal rules, checked before rule traversal or encoding.
    pub rules: usize,
    /// Maximum output entries, including directories.
    pub entries: usize,
    /// Compact JSON bytes of all rule identifiers and literal UTF-8 needles.
    pub rules_bytes: usize,
    /// Conservative comparison work: haystack bytes times needle bytes, per rule.
    /// Raw paths/files and all decoded syntax channels consume the same budget.
    pub work_bytes: u64,
    /// Maximum distinct path/rule/representation findings; overflow returns no report.
    pub findings: usize,
    /// Maximum compact JSON bytes of complete findings before their evidence digest.
    pub evidence_bytes: usize,
    /// Per-file HTML tokenizer limits, separate from aggregate literal-search work.
    pub html: gazette_compiler::html::Limits,
    /// Per-file XML parser limits, separate from aggregate literal-search work.
    pub xml: gazette_compiler::xml::Limits,
    /// Per-stylesheet or embedded CSS limits, separate from aggregate comparison work.
    pub css: gazette_compiler::css::Limits,
    /// Per-file or inline JavaScript syntax and decoded-text inspection limits.
    pub javascript: gazette_compiler::javascript::Limits,
    /// Per-file native Markdown input, syntax-depth, node and retained-fact ceilings.
    /// All decoded facts also consume the shared aggregate value/byte budget.
    pub markdown: gazette_core::markdown::Limits,
    /// Per-candidate encoded input and decoded payload ceilings, before allocation.
    pub encoded: gazette_compiler::encoded::Limits,
    /// Per-image container and metadata bounds, independent of pixel rendering limits.
    /// Decoded image streams and fields also consume the whole-output decoded budget.
    pub raster: gazette_core::raster::MetadataLimits,
    /// Per-font WOFF2 and reconstructed SFNT bounds.
    pub font: gazette_core::font::Limits,
    /// Maximum nested decoding layers; values above 32 are refused.
    pub encoded_depth: usize,
    /// Maximum decoded candidates and raster fields, including repeated occurrences.
    pub encoded_values: usize,
    /// Maximum aggregate decoded payload, image-stream and metadata bytes over the output.
    pub encoded_bytes: usize,
}

impl Default for DisclosureProbeLimits {
    fn default() -> Self {
        Self {
            rules: 128,
            entries: 10_000,
            rules_bytes: 64 << 10,
            work_bytes: 1 << 30,
            findings: 10_000,
            evidence_bytes: 1 << 20,
            html: Default::default(),
            xml: Default::default(),
            css: Default::default(),
            javascript: Default::default(),
            markdown: Default::default(),
            encoded: Default::default(),
            raster: Default::default(),
            font: Default::default(),
            encoded_depth: 8,
            encoded_values: 10_000,
            encoded_bytes: 16 << 20,
        }
    }
}

impl DisclosureProbeLimits {
    /// Version of the deterministic resource-limit projection.
    pub const RESOURCE_PROFILE: &'static str = technical_profile::RESOURCE_PROFILE;

    /// Identify every chosen ceiling before scanning, for independent qualification expectations.
    pub fn resource_limits_sha256(self) -> Sha256Digest {
        technical_profile::resource_limits_digest(self)
    }

    /// Check the hard recursion ceiling before reading or rendering inputs.
    /// Zero candidate budgets deliberately refuse any encountered encoded value.
    ///
    /// # Errors
    /// Refuses a recursion setting above the supported stack bound.
    pub fn validate(&self) -> Result<(), DisclosureProbeError> {
        if self.encoded_depth > 32 {
            return Err(DisclosureProbeError::Limit("encoded depth configuration"));
        }
        Ok(())
    }

    /// Validate private literal rules before acquiring or rendering any source.
    ///
    /// Returns the same compact, key-ordered rule digest used by the output probe.
    /// This checks diagnostic shape and bounds only, never Website policy admission.
    /// The output probe repeats this check so direct SDK callers cannot bypass it.
    ///
    /// # Errors
    /// Rejects empty rules/literals and rule count or encoded-byte exhaustion.
    pub fn validate_rules(
        &self,
        rules: &BTreeMap<Key, String>,
    ) -> Result<Sha256Digest, DisclosureProbeError> {
        self.validate()?;
        if rules.len() > self.rules {
            return Err(DisclosureProbeError::Limit("rule count"));
        }
        if rules.is_empty() || rules.values().any(String::is_empty) {
            return Err(DisclosureProbeError::Rules);
        }
        let encoded = encode(rules, self.rules_bytes, "disclosure rules")
            .map_err(|_| DisclosureProbeError::Limit("rule bytes"))?;
        Ok(Sha256Digest::of(&encoded))
    }
}

/// The representation in which a literal occurred, not its public-use meaning.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DisclosureRepresentation {
    /// UTF-8 plain text, without executing or interpreting a markup language.
    Text,
    /// Native Markdown text/code/labels, before separate candidate decoding.
    MarkdownText,
    /// Native effective link/image destination, before candidate decoding.
    MarkdownDestination,
    /// Native effective link/image title, before candidate decoding.
    MarkdownTitle,
    /// Retained native heading attributes or footnote association labels.
    MarkdownMetadata,
    /// One or more bounded percent-decoding layers of a candidate value.
    PercentDecoded,
    /// Exact decoded data-URL payload bytes, not proof of container safety.
    DataPayload,
    /// Exact inventory spelling; no URL or Unicode normalization is claimed.
    Path,
    /// Canonical-decomposed, fully case-folded relative path used by destination
    /// collision checks. Percent signs remain literal and no URL decoding occurs.
    NormalizedPath,
    /// Exact retained bytes, including binary payloads; not container inspection.
    Raw,
    /// A provider-decoded raster metadata field, not pixel meaning or image sanitation.
    RasterMetadata,
    /// Complete reconstructed SFNT bytes from a validated retained WOFF2 container.
    FontDecoded,
    /// A decoded JSON object key or string value, including duplicate members.
    JsonString,
    /// Entity-decoded HTML data or title/textarea text, not DOM-visible text.
    HtmlText,
    /// One entity-decoded HTML attribute value; candidate decoding is reported separately.
    HtmlAttribute,
    /// Raw script/data body before any separately reported language decoding.
    HtmlScript,
    /// Raw style body; no CSS escape decoding or execution.
    HtmlStyle,
    /// Native CSS identifier/function/hash/dimension-unit decoding, not value evaluation.
    CssIdentifier,
    /// One native CSS quoted string before separate candidate decoding, not evaluation.
    CssString,
    /// One native CSS unquoted URL token before candidate decoding, never fetching.
    CssUrl,
    /// Native JavaScript identifier spelling after Unicode escape decoding.
    JavascriptIdentifier,
    /// One native decoded JavaScript string literal, not an evaluated expression.
    JavascriptString,
    /// One native cooked template segment; separate segments are never concatenated.
    JavascriptTemplate,
    /// XML text and adjacent CDATA after native normalization, not embedded languages.
    XmlText,
    /// One decoded XML attribute value before separate candidate decoding.
    XmlAttribute,
    /// An explicitly declared XML namespace URI, even if no name uses it.
    XmlNamespace,
    /// XML comment data, not entity-decoded.
    XmlComment,
    /// XML processing instruction target or data, never executed.
    XmlProcessingInstruction,
}

/// A private diagnostic coordinate, without a payload snippet or separate needle field.
/// Paths and rule identifiers may themselves contain confidential data.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, schemars::JsonSchema)]
pub struct DisclosureFinding {
    /// Exact retained path; this operator evidence itself may be confidential.
    pub path: EntryPath,
    /// Caller-selected rule identifier; not an authority or approval reference.
    pub rule: Key,
    /// Explicit policy purpose; absent for an unclassified diagnostic literal search.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class: Option<DisclosureClass>,
    /// Representation tested by this partial probe.
    pub representation: DisclosureRepresentation,
}

/// Extra decoding performed after raw scanning. No variant certifies a file class.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DisclosureDecoding {
    /// Closed generated JSON-string carriers followed by native HTML/`MathML` inspection.
    GeneratedMdxLiteralSyntax {
        /// Exact finite generated grammar; general MDX remains unsupported.
        profile: &'static str,
        /// Entity decoding and the shared `MathML` presentation invariant.
        html_profile: &'static str,
        /// CSS inspection for admitted `MathML` presentation attributes.
        css_profile: &'static str,
    },
    /// Complete WOFF2 decode and required SFNT-table validation, not glyph rendering.
    Woff2Font {
        /// Pinned provider and closed WOFF2/SFNT profile bound to exact encoded and decoded bytes.
        profile: &'static str,
        /// Exact decoded table count recorded by the WOFF2 container.
        tables: u16,
        /// Glyph count from the decoded required `maxp` table.
        glyphs: u16,
    },
    /// Complete selected container/metadata profile, not interpretation of image pixels.
    RasterMetadata {
        /// Explicit selected encoded representation.
        format: gazette_core::raster::Format,
        /// Pinned provider and closed container profile, bound to these exact bytes.
        profile: &'static str,
    },
    /// Plain UTF-8 text and bounded candidate decoding, not MIME or browser validation.
    Utf8Text,
    /// Complete bounded native Markdown facts plus selected embedded HTML inspection.
    MarkdownSyntax {
        /// Application-owned finite disclosure procedure, not publication admission.
        profile: &'static str,
        /// Actual caller-selected native Markdown behavior profile.
        parser_profile: &'static str,
        /// Selected HTML decoding profile for native raw-HTML occurrences.
        html_profile: &'static str,
        /// Selected embedded CSS decoder.
        css_profile: &'static str,
        /// Selected embedded JavaScript decoder.
        javascript_profile: &'static str,
    },
    /// No structural decoder; encoded/embedded values remain unexamined.
    None,
    /// Strict JSON grammar and all decoded keys/strings; candidate decoding is separate.
    JsonStrings,
    /// Bounded HTML tokens/entities and selected embedded syntax, not DOM/browser validation.
    HtmlTokens {
        /// Exact token-decoding profile and pinned provider.
        profile: &'static str,
        /// CSS decoder selected for style elements and entity-decoded style attributes.
        css_profile: &'static str,
        /// JavaScript decoder for recognized inline classic/module script bodies.
        javascript_profile: &'static str,
    },
    /// Bounded XML 1.0 events, not feed, sitemap or SVG schema validation.
    XmlEvents {
        /// Exact lexical-guard and semantic-decoding profile.
        profile: &'static str,
        /// CSS decoder for native namespace-qualified SVG style attributes.
        css_profile: &'static str,
    },
    /// Native CSS tokens and escapes, not property/schema or browser validation.
    CssTokens {
        /// Exact bounded token-decoding profile and pinned provider.
        profile: &'static str,
    },
    /// Native JavaScript strings/identifiers/template segments, not runtime values.
    JavascriptSyntax {
        /// Exact parser/semantic early-error and text-inspection profile.
        profile: &'static str,
        /// File-selected parse goal, not evidence of browser script loading mode.
        goal: gazette_compiler::javascript::Goal,
    },
}

/// Per-file coverage prevents raw-byte traversal from appearing to qualify its format.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct DisclosureCoverage {
    /// Exact retained file.
    pub path: EntryPath,
    /// Exact raw content digest.
    pub sha256: Sha256Digest,
    /// Additional bounded decoder used, if any.
    pub decoding: DisclosureDecoding,
}

/// A decoded candidate tied to its containing file, without copying private payloads.
/// Repeated appearances remain separate budgeted observations; no URL-use claim is made.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct DisclosureEmbeddedCoverage {
    /// Exact enclosing output path, not an invented filesystem path for the payload.
    pub path: EntryPath,
    /// One-based decoding layer beneath the original structured value or path.
    pub depth: usize,
    /// Transformation that produced these bytes.
    pub representation: DisclosureRepresentation,
    /// Exact decoded-byte identity.
    pub sha256: Sha256Digest,
    /// Exact decoded length charged to the whole-output budget.
    pub bytes: usize,
    /// Declared data-URL media type when present; not content sniffing or qualification.
    pub media_type: Option<String>,
    /// Additional syntax inspection; `none` leaves binary/unknown containers unqualified.
    pub decoding: DisclosureDecoding,
}

/// Closed coverage outcome of one disclosure traversal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub enum DisclosureStatus {
    /// Diagnostic traversal without supported-profile completeness.
    #[serde(rename = "partial-evidence")]
    PartialEvidence,
    /// Required traversal completed under the supported scanner profile.
    #[serde(rename = "required-supported-profile-evidence")]
    RequiredSupportedProfileEvidence,
}

/// Immutable disclosure evidence bound to one exact output tree and rule set.
/// Neither status grants authority, publication admission or release permission.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct DisclosureProbe<'a> {
    status: DisclosureStatus,
    scanner: &'static str,
    output_identity: TreeIdentity,
    #[serde(skip_serializing_if = "Option::is_none")]
    rules_sha256: Option<Sha256Digest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    policy_sha256: Option<Sha256Digest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    generic_rules_sha256: Option<Sha256Digest>,
    findings_sha256: Sha256Digest,
    work_bytes: u64,
    encoded_profile: &'static str,
    filename_profile: &'static str,
    resource_profile: &'static str,
    resource_limits_sha256: Sha256Digest,
    encoded_bytes: usize,
    decoded_values: usize,
    embedded: Vec<DisclosureEmbeddedCoverage>,
    files: Vec<DisclosureCoverage>,
    findings: Vec<DisclosureFinding>,
    #[serde(skip)]
    output: &'a CapturedSource,
}

impl DisclosureProbe<'_> {
    /// The same immutable tree that was scanned, without reopening any native path.
    pub fn output(&self) -> &CapturedSource {
        self.output
    }
    /// Deterministic exact findings; no occurrence count or complete disclosure claim.
    pub fn findings(&self) -> &[DisclosureFinding] {
        &self.findings
    }
    /// Raw scanning and additional decoder coverage for every regular file.
    pub fn files(&self) -> &[DisclosureCoverage] {
        &self.files
    }
}

/// A probe could not finish; no partial report is returned as successful evidence.
#[derive(Debug)]
pub enum DisclosureProbeError {
    /// Required decoding encountered an unclassified or unsupported representation.
    Unsupported {
        /// Exact containing path; private operator evidence, omitted from Display.
        path: EntryPath,
        /// Static representation category, never untrusted media or payload text.
        kind: &'static str,
    },
    /// The complete supported-profile traversal found caller-selected forbidden literals.
    Findings {
        /// Exact private rule/path coordinates, without payload snippets.
        findings: Vec<DisclosureFinding>,
    },
    /// A candidate's encoding or declared text profile could not be decoded completely.
    Encoded {
        /// Exact containing output path, potentially confidential.
        path: EntryPath,
        /// Stable redacted reason, never payload or parser diagnostics.
        reason: &'static str,
    },
    /// An empty rule set or empty literal cannot establish useful diagnostic coverage.
    Rules,
    /// An explicit policy lacks required coverage or contains an invalid predicate.
    Policy(&'static str),
    /// An explicit ceiling stopped the operation.
    Limit(&'static str),
    /// A retained output violated its inventory/payload invariant.
    Inventory,
    /// Raster container/metadata inspection failed; pixel decoding is separate evidence.
    Raster {
        /// Exact retained path, potentially confidential operator evidence.
        path: EntryPath,
        /// Static redacted reason; no metadata or native provider error is echoed.
        reason: &'static str,
    },
    /// WOFF2 decode or required SFNT metadata validation failed.
    Font {
        /// Exact retained path, potentially confidential operator evidence.
        path: EntryPath,
        /// Static redacted reason; no font data or provider diagnostic is echoed.
        reason: &'static str,
    },
    /// CSS tokenization failed or requires an unsupported decoding profile.
    Css {
        /// Exact retained path, potentially confidential operator evidence.
        path: EntryPath,
        /// Stable reason, never source payload or provider error text.
        reason: &'static str,
    },
    /// A JavaScript candidate failed the bounded syntax-text profile.
    Javascript {
        /// Exact retained path, potentially confidential operator evidence.
        path: EntryPath,
        /// Stable reason without source payload or native parser diagnostics.
        reason: &'static str,
    },
    /// An XML candidate failed its profile; no partial report survives.
    Xml {
        /// Exact file, potentially confidential operator evidence.
        path: EntryPath,
        /// Stable reason without source payload or provider error text.
        reason: &'static str,
    },
    /// An HTML candidate failed the bounded profile; no partial report survives.
    Html {
        /// Exact file, potentially confidential operator evidence.
        path: EntryPath,
        /// Stable reason, without source payload or provider error text.
        reason: &'static str,
        /// Provider line when available; not a fabricated position.
        line: Option<u64>,
    },
    /// A JSON file or inline JSON-script body has invalid or unsupported syntax.
    Json {
        /// Exact file, potentially confidential operator evidence.
        path: EntryPath,
        /// Line in the selected JSON input (body-relative for an inline script).
        /// No payload or decoder error chain is exposed.
        line: usize,
        /// Parser position within the line.
        column: usize,
    },
}

impl DisclosureProbeError {
    /// Stable failure class; no successful or certified partial result is implied.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unsupported { .. } => "disclosure_unsupported",
            Self::Findings { .. } => "disclosure_findings",
            Self::Encoded { .. } => "disclosure_probe_encoded",
            Self::Rules => "disclosure_probe_rules",
            Self::Policy(_) => "disclosure_policy",
            Self::Limit(_) => "disclosure_probe_limit",
            Self::Inventory => "disclosure_probe_inventory",
            Self::Raster { .. } => "disclosure_probe_raster",
            Self::Font { .. } => "disclosure_probe_font",
            Self::Json { .. } => "disclosure_probe_json",
            Self::Html { .. } => "disclosure_probe_html",
            Self::Xml { .. } => "disclosure_probe_xml",
            Self::Css { .. } => "disclosure_probe_css",
            Self::Javascript { .. } => "disclosure_probe_javascript",
        }
    }
}

impl fmt::Display for DisclosureProbeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported { kind, .. } => write!(
                f,
                "disclosure check requires supported {kind} decoding; inspect retained output privately"
            ),
            Self::Findings { findings } => write!(
                f,
                "disclosure check found {} rule matches; inspect private findings; output is unchanged",
                findings.len()
            ),
            Self::Encoded { reason, .. } => write!(
                f,
                "disclosure probe encoding {reason}; inspect retained output privately"
            ),
            Self::Rules => f.write_str("disclosure probe needs nonempty literal rules"),
            Self::Policy(reason) => {
                write!(f, "disclosure policy {reason}; inspect private selection")
            }
            Self::Limit(resource) => write!(
                f,
                "disclosure probe {resource} ceiling exceeded; no report produced"
            ),
            Self::Inventory => f.write_str(
                "disclosure probe output payload missing; inspect retained output integrity",
            ),
            Self::Raster { reason, .. } => write!(
                f,
                "disclosure probe raster {reason}; inspect retained output privately"
            ),
            Self::Font { reason, .. } => write!(
                f,
                "disclosure probe font {reason}; inspect retained output privately"
            ),
            Self::Xml { reason, .. } => write!(
                f,
                "disclosure probe XML {reason}; inspect the selected output privately"
            ),
            Self::Css { reason, .. } => write!(
                f,
                "disclosure probe CSS {reason}; inspect the selected output privately"
            ),
            Self::Javascript { reason, .. } => write!(
                f,
                "disclosure probe JavaScript {reason}; inspect the selected output privately"
            ),
            Self::Json { line, column, .. } => write!(
                f,
                "disclosure probe JSON invalid at line {line}, column {column}; inspect the selected output privately"
            ),
            Self::Html { reason, line, .. } => {
                write!(f, "disclosure probe HTML {reason}")?;
                if let Some(line) = line {
                    write!(f, " at line {line}")?;
                }
                f.write_str("; inspect the selected output privately")
            }
        }
    }
}
impl std::error::Error for DisclosureProbeError {}

impl RenderedSite {
    /// Probe with explicitly selected native decoders, without editing output.
    ///
    /// # Errors
    /// Rejects unavailable decoders, malformed syntax and exhausted bounds.
    pub fn probe_disclosure_with_inspectors(
        &self,
        rules: &BTreeMap<Key, String>,
        inspectors: DisclosureInspectors<'_>,
        limits: DisclosureProbeLimits,
    ) -> Result<DisclosureProbe<'_>, DisclosureProbeError> {
        probe_disclosure_with_inspectors(
            self.output(),
            self.media_types(),
            rules,
            inspectors,
            limits,
        )
    }

    /// Require supported decoding and no findings with selected native decoders.
    ///
    /// # Errors
    /// Rejects unavailable decoders, findings, malformed syntax and exhausted bounds.
    pub fn check_disclosure_with_inspectors(
        &self,
        rules: &BTreeMap<Key, String>,
        inspectors: DisclosureInspectors<'_>,
        limits: DisclosureProbeLimits,
    ) -> Result<DisclosureProbe<'_>, DisclosureProbeError> {
        check_disclosure_with_inspectors(
            self.output(),
            self.media_types(),
            rules,
            inspectors,
            limits,
        )
    }

    /// Require complete policy coverage with explicitly selected native decoders.
    ///
    /// # Errors
    /// Rejects incomplete policy, unavailable decoders, findings and exhausted bounds.
    pub fn check_disclosure_policy_with_inspectors(
        &self,
        policy: &DisclosurePolicy,
        inspectors: DisclosureInspectors<'_>,
        limits: DisclosureProbeLimits,
    ) -> Result<DisclosureProbe<'_>, DisclosureProbeError> {
        check_disclosure_policy_with_inspectors(
            self.output(),
            self.media_types(),
            policy,
            inspectors,
            limits,
        )
    }

    /// Apply a complete explicit policy through the same immutable decoder pipeline.
    ///
    /// # Errors
    /// Rejects incomplete policy, unsupported decoding, findings and exhausted bounds.
    pub fn check_disclosure_policy(
        &self,
        policy: &DisclosurePolicy,
        metadata: &gazette_core::raster::InspectMetadata<'_>,
        limits: DisclosureProbeLimits,
    ) -> Result<DisclosureProbe<'_>, DisclosureProbeError> {
        check_disclosure_policy(self.output(), self.media_types(), policy, metadata, limits)
    }

    /// Apply complete policy with selected raster and WOFF2 inspectors.
    ///
    /// # Errors
    /// Rejects incomplete policy, unsupported decoding, findings and exhausted bounds.
    pub fn check_disclosure_policy_with_font(
        &self,
        policy: &DisclosurePolicy,
        metadata: &gazette_core::raster::InspectMetadata<'_>,
        font: &gazette_core::font::Inspect<'_>,
        limits: DisclosureProbeLimits,
    ) -> Result<DisclosureProbe<'_>, DisclosureProbeError> {
        check_disclosure_policy_with_font(
            self.output(),
            self.media_types(),
            policy,
            metadata,
            font,
            limits,
        )
    }

    /// Require supported decoding and no literal-rule findings over this exact output.
    /// See [`check_disclosure`] for the bounded profile and remaining certification gates.
    ///
    /// # Errors
    /// Rejects unsupported representations, findings, malformed syntax and exhausted bounds.
    pub fn check_disclosure(
        &self,
        rules: &BTreeMap<Key, String>,
        limits: DisclosureProbeLimits,
    ) -> Result<DisclosureProbe<'_>, DisclosureProbeError> {
        check_disclosure(self.output(), self.media_types(), rules, limits)
    }

    /// Search every retained path and file for exact case-sensitive UTF-8 literals.
    ///
    /// Generated media declarations take precedence over filenames. Undeclared
    /// retained assets use suffix-selected syntax candidates. See [`probe_disclosure`]
    /// for the exact diagnostic boundary shared with independently retained output.
    ///
    /// Rules are private caller inputs, not an admitted Website policy. This pure
    /// operation neither edits the output nor grants publication/release permission.
    ///
    /// # Errors
    /// Rejects empty rules, resource limits and malformed/unsupported selected syntax.
    /// JSON's native recursion limit remains enabled. Work is charged before each
    /// comparison; escaped JSON strings may be allocated by the bounded raw-input
    /// parser before their decoded-string work is charged.
    pub fn probe_disclosure(
        &self,
        rules: &BTreeMap<Key, String>,
        limits: DisclosureProbeLimits,
    ) -> Result<DisclosureProbe<'_>, DisclosureProbeError> {
        probe_disclosure(self.output(), self.media_types(), rules, limits)
    }
}

/// Inspect one immutable captured output using the same probe as [`RenderedSite`].
///
/// Producer media declarations select syntax before suffix fallback. An empty map
/// is appropriate for imported trees with no retained producer declarations; it
/// does not prove their actual serving types. Supplied declarations must name
/// retained regular files, but this operation does not authenticate their issuer.
///
/// Every path and raw file is searched. JSON, HTML, XML, CSS and JavaScript receive
/// bounded syntax decoding; plain text requires UTF-8. Eligible complete text
/// values receive percent/data-URL candidate decoding, with recognized textual
/// payloads passed to the same syntax owners and shared aggregate budgets.
/// Duplicate JSON members are visited, not discarded by map materialization.
///
/// This is not runtime URL discovery, browser evaluation, CSS property validation,
/// filesystem normalization, binary/container metadata inspection, or policy
/// admission. Unknown formats remain raw-only. Rules and reports are private
/// operator evidence; no output is rewritten and status is always partial.
///
/// # Errors
/// Rejects invalid rules/declarations, exhausted ceilings and malformed or
/// unsupported selected syntax. No partial successful report escapes a failure.
pub fn probe_disclosure<'a>(
    output: &'a CapturedSource,
    media_types: &BTreeMap<EntryPath, MediaType>,
    rules: &BTreeMap<Key, String>,
    limits: DisclosureProbeLimits,
) -> Result<DisclosureProbe<'a>, DisclosureProbeError> {
    probe_disclosure_with_inspectors(
        output,
        media_types,
        rules,
        DisclosureInspectors::default(),
        limits,
    )
}

/// Probe immutable output using explicit selected decoders and shared finite budgets.
/// Missing Markdown never falls back to raw or plain-text evidence.
///
/// # Errors
/// Rejects invalid rules, unavailable Markdown, malformed syntax and exhausted bounds.
pub fn probe_disclosure_with_inspectors<'a>(
    output: &'a CapturedSource,
    media_types: &BTreeMap<EntryPath, MediaType>,
    rules: &BTreeMap<Key, String>,
    inspectors: DisclosureInspectors<'_>,
    limits: DisclosureProbeLimits,
) -> Result<DisclosureProbe<'a>, DisclosureProbeError> {
    inspect(
        output,
        media_types,
        Rules::Literals(rules),
        inspectors,
        limits,
        Coverage::Diagnostic,
    )
}

/// Check the same immutable output traversal without permitting raw-only representations.
///
/// Every retained file and encountered data payload must have a supported decoder;
/// percent-decoded text must be UTF-8. Known uninterpreted embedded contexts are refused.
/// Any literal-rule finding fails the check. No file is removed or rewritten.
///
/// The returned evidence remains partial: supported lexical decoding does not prove
/// complete browser behavior, filesystem equivalence, serving types, policy authority
/// or publication permission. This is a mechanical prerequisite, not certification.
/// Producer declarations are subject to the same trust boundary as [`probe_disclosure`].
///
/// # Errors
/// Rejects unsupported representations, findings and every ordinary probe failure.
/// Neither an unsupported suffix nor zero findings can manufacture a passing result.
pub fn check_disclosure<'a>(
    output: &'a CapturedSource,
    media_types: &BTreeMap<EntryPath, MediaType>,
    rules: &BTreeMap<Key, String>,
    limits: DisclosureProbeLimits,
) -> Result<DisclosureProbe<'a>, DisclosureProbeError> {
    check_disclosure_with_inspectors(
        output,
        media_types,
        rules,
        DisclosureInspectors::default(),
        limits,
    )
}

/// Require supported decoding and no findings with explicitly selected decoders.
///
/// # Errors
/// Rejects unavailable decoders, findings, malformed syntax and exhausted bounds.
pub fn check_disclosure_with_inspectors<'a>(
    output: &'a CapturedSource,
    media_types: &BTreeMap<EntryPath, MediaType>,
    rules: &BTreeMap<Key, String>,
    inspectors: DisclosureInspectors<'_>,
    limits: DisclosureProbeLimits,
) -> Result<DisclosureProbe<'a>, DisclosureProbeError> {
    checked(inspect(
        output,
        media_types,
        Rules::Literals(rules),
        inspectors,
        limits,
        Coverage::Required,
    )?)
}

/// Require all policy classes and supported decoding over the exact immutable output.
///
/// Policy and generic rule-profile identities are recorded separately. Missing classes,
/// invalid predicates, unknown containers, decoding failures and matches fail closed.
/// Authored examples are not exempted and no output is scrubbed. The instance owner
/// still owns selection adequacy and approval; this is not certification or permission.
///
/// # Errors
/// Rejects incomplete/invalid policy and every required-scan failure before a report escapes.
pub fn check_disclosure_policy<'a>(
    output: &'a CapturedSource,
    media_types: &BTreeMap<EntryPath, MediaType>,
    policy: &DisclosurePolicy,
    metadata: &gazette_core::raster::InspectMetadata<'_>,
    limits: DisclosureProbeLimits,
) -> Result<DisclosureProbe<'a>, DisclosureProbeError> {
    check_disclosure_policy_with_font(
        output,
        media_types,
        policy,
        metadata,
        &gazette_core::font::unavailable,
        limits,
    )
}

/// Require complete policy coverage with explicit raster and WOFF2 inspectors.
///
/// # Errors
/// Rejects incomplete policy, unsupported decoding, findings and exhausted bounds.
pub fn check_disclosure_policy_with_font<'a>(
    output: &'a CapturedSource,
    media_types: &BTreeMap<EntryPath, MediaType>,
    policy: &DisclosurePolicy,
    metadata: &gazette_core::raster::InspectMetadata<'_>,
    font: &gazette_core::font::Inspect<'_>,
    limits: DisclosureProbeLimits,
) -> Result<DisclosureProbe<'a>, DisclosureProbeError> {
    check_disclosure_policy_with_inspectors(
        output,
        media_types,
        policy,
        DisclosureInspectors {
            raster: Some(metadata),
            font: Some(font),
            markdown: None,
        },
        limits,
    )
}

/// Require complete policy coverage through one selected-decoder pipeline.
///
/// # Errors
/// Rejects incomplete policy, unavailable decoders, findings and exhausted bounds.
pub fn check_disclosure_policy_with_inspectors<'a>(
    output: &'a CapturedSource,
    media_types: &BTreeMap<EntryPath, MediaType>,
    policy: &DisclosurePolicy,
    inspectors: DisclosureInspectors<'_>,
    limits: DisclosureProbeLimits,
) -> Result<DisclosureProbe<'a>, DisclosureProbeError> {
    checked(inspect(
        output,
        media_types,
        Rules::Policy(policy),
        inspectors,
        limits,
        Coverage::Required,
    )?)
}

fn checked(report: DisclosureProbe<'_>) -> Result<DisclosureProbe<'_>, DisclosureProbeError> {
    if !report.findings.is_empty() {
        return Err(DisclosureProbeError::Findings {
            findings: report.findings,
        });
    }
    Ok(report)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Coverage {
    Diagnostic,
    Required,
}

fn inspect<'a>(
    output: &'a CapturedSource,
    media_types: &BTreeMap<EntryPath, MediaType>,
    rules: Rules<'_>,
    inspectors: DisclosureInspectors<'_>,
    limits: DisclosureProbeLimits,
    coverage: Coverage,
) -> Result<DisclosureProbe<'a>, DisclosureProbeError> {
    let (rules_sha256, policy_sha256, generic_rules_sha256) = match rules {
        Rules::Literals(values) => (Some(limits.validate_rules(values)?), None, None),
        Rules::Policy(policy) => (
            None,
            Some(policy.validate(limits)?),
            Some(policy::generic_rules_digest()),
        ),
    };
    if output.inventory().entries().len() > limits.entries {
        return Err(DisclosureProbeError::Limit("output entries"));
    }
    if media_types.keys().any(|path| output.file(path).is_none()) {
        return Err(DisclosureProbeError::Inventory);
    }
    let mut scan = Scan {
        rules,
        metadata: inspectors.raster,
        font: inspectors.font,
        markdown: inspectors.markdown,
        limits,
        coverage,
        work: 0,
        findings: Default::default(),
        depth: 0,
        encoded_bytes: 0,
        values: 0,
        embedded: Vec::new(),
    };
    let mut files = Vec::new();
    let mut normalized_names = BTreeMap::new();
    for entry in output.inventory().entries() {
        let normalized = crate::source::collision_key(entry.path.as_str());
        if normalized_names
            .insert(normalized.clone(), entry.path.clone())
            .is_some()
        {
            return Err(DisclosureProbeError::Inventory);
        }
        scan.text(
            &entry.path,
            entry.path.as_str().as_bytes(),
            DisclosureRepresentation::Path,
        )?;
        if normalized != entry.path.as_str() {
            scan.text(
                &entry.path,
                normalized.as_bytes(),
                DisclosureRepresentation::NormalizedPath,
            )?;
        }
        if let Content::File { sha256, .. } = entry.content {
            // CapturedSource construction proves payload/inventory parity.
            let bytes = output
                .file(&entry.path)
                .ok_or(DisclosureProbeError::Inventory)?;
            scan.search(&entry.path, bytes, DisclosureRepresentation::Raw)?;
            let format =
                format::Format::for_file(&entry.path, media_types.get(&entry.path).copied());
            if matches!(format, format::Format::Raw) {
                scan.unsupported(&entry.path, "file")?;
            }
            let decoding = format.scan(&mut scan, &entry.path, bytes)?;
            files.push(DisclosureCoverage {
                path: entry.path.clone(),
                sha256,
                decoding,
            });
        }
    }
    let findings: Vec<_> = scan.findings.into_iter().collect();
    let findings_sha256 = Sha256Digest::of(
        &encode(&findings, limits.evidence_bytes, "disclosure findings")
            .map_err(|_| DisclosureProbeError::Limit("findings evidence bytes"))?,
    );
    Ok(DisclosureProbe {
        status: match coverage {
            Coverage::Diagnostic => DisclosureStatus::PartialEvidence,
            Coverage::Required => DisclosureStatus::RequiredSupportedProfileEvidence,
        },
        scanner: technical_profile::SCANNER,
        output_identity: output.inventory().identity(),
        rules_sha256,
        policy_sha256,
        generic_rules_sha256,
        findings_sha256,
        work_bytes: scan.work,
        encoded_profile: technical_profile::ENCODED,
        filename_profile: technical_profile::FILENAME_PROFILE,
        resource_profile: technical_profile::RESOURCE_PROFILE,
        resource_limits_sha256: technical_profile::resource_limits_digest(limits),
        encoded_bytes: scan.encoded_bytes,
        decoded_values: scan.values,
        embedded: scan.embedded,
        files,
        findings,
        output,
    })
}

struct Scan<'a> {
    rules: Rules<'a>,
    metadata: Option<&'a gazette_core::raster::InspectMetadata<'a>>,
    font: Option<&'a gazette_core::font::Inspect<'a>>,
    markdown: Option<&'a dyn gazette_ports::markdown::Markdown>,
    limits: DisclosureProbeLimits,
    coverage: Coverage,
    work: u64,
    findings: std::collections::BTreeSet<DisclosureFinding>,
    depth: usize,
    encoded_bytes: usize,
    values: usize,
    embedded: Vec<DisclosureEmbeddedCoverage>,
}

#[derive(Clone, Copy)]
enum Rules<'a> {
    Literals(&'a BTreeMap<Key, String>),
    Policy(&'a DisclosurePolicy),
}

enum Predicate<'a> {
    Literal(&'a str),
    Pattern(&'a DisclosurePattern),
}

impl Predicate<'_> {
    fn factor(&self) -> usize {
        match self {
            Self::Literal(value) => value.len(),
            Self::Pattern(pattern) => pattern.work_factor(),
        }
    }

    fn matches(&self, bytes: &[u8]) -> bool {
        match self {
            Self::Literal(value) => gazette_compiler::disclosure::contains_literal(bytes, value),
            Self::Pattern(pattern) => pattern.matches(bytes),
        }
    }
}

impl<'a> Rules<'a> {
    fn iter(self) -> impl Iterator<Item = (&'a Key, Option<DisclosureClass>, Predicate<'a>)> {
        let (literals, policy) = match self {
            Self::Literals(values) => (Some(values), None),
            Self::Policy(policy) => (None, Some(policy)),
        };
        literals
            .into_iter()
            .flat_map(|values| values.iter())
            .map(|(id, value)| (id, None, Predicate::Literal(value)))
            .chain(
                policy
                    .into_iter()
                    .flat_map(|policy| policy.rules.iter())
                    .map(|(id, rule)| (id, Some(rule.class), Predicate::Pattern(&rule.pattern))),
            )
    }
}

impl Scan<'_> {
    fn decoded(&mut self, bytes: usize, values: usize) -> Result<(), DisclosureProbeError> {
        self.encoded_bytes = self
            .encoded_bytes
            .checked_add(bytes)
            .filter(|bytes| *bytes <= self.limits.encoded_bytes)
            .ok_or(DisclosureProbeError::Limit("decoded aggregate bytes"))?;
        self.values = self
            .values
            .checked_add(values)
            .filter(|values| *values <= self.limits.encoded_values)
            .ok_or(DisclosureProbeError::Limit("decoded aggregate values"))?;
        Ok(())
    }

    fn unsupported(
        &self,
        path: &EntryPath,
        kind: &'static str,
    ) -> Result<(), DisclosureProbeError> {
        if self.coverage == Coverage::Required {
            return Err(DisclosureProbeError::Unsupported {
                path: path.clone(),
                kind,
            });
        }
        Ok(())
    }

    fn text(
        &mut self,
        path: &EntryPath,
        bytes: &[u8],
        representation: DisclosureRepresentation,
    ) -> Result<(), DisclosureProbeError> {
        self.search(path, bytes, representation)?;
        encoded::scan(self, path, bytes)
    }

    fn search(
        &mut self,
        path: &EntryPath,
        bytes: &[u8],
        representation: DisclosureRepresentation,
    ) -> Result<(), DisclosureProbeError> {
        for (rule, class, predicate) in self.rules.iter() {
            let cost = (bytes.len() as u64)
                .checked_mul(predicate.factor() as u64)
                .and_then(|cost| self.work.checked_add(cost))
                .filter(|&work| work <= self.limits.work_bytes)
                .ok_or(DisclosureProbeError::Limit("comparison work"))?;
            self.work = cost;
            if predicate.matches(bytes) {
                let finding = DisclosureFinding {
                    path: path.clone(),
                    rule: rule.clone(),
                    class,
                    representation,
                };
                if !self.findings.contains(&finding) {
                    if self.findings.len() >= self.limits.findings {
                        return Err(DisclosureProbeError::Limit("findings"));
                    }
                    self.findings.insert(finding);
                }
            }
        }
        Ok(())
    }
}
