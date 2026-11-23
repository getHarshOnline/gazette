//! Bounded Markdown mechanisms; publication decisions stay with the compiler.

use gazette_core::markdown::{
    ComponentBinding, Error, Fragment, HeadingBinding, Identity, Limits, LinkBinding, Plan,
    Profile, Rendered, Syntax,
};
use gazette_core::source::Location;

/// Pure selected presentation, composed by the application without exposing parser nodes.
/// Implementations consume only resolved data and the supplied child projection;
/// they must not reacquire sources or introduce independent authored meaning.
pub trait ComponentRenderer: Send + Sync {
    /// Render one bounded static block; excerpt bodies contain only the admitted prefix.
    ///
    /// # Errors
    /// Refuse unavailable definitions, incompatible arguments or incomplete presentation.
    fn render(
        &self,
        binding: &ComponentBinding,
        body: &Fragment,
        excerpt: bool,
    ) -> Result<String, Error>;

    /// Colour mode the selected Theme presents before the reader chooses one.
    /// It decides when an image's dark source applies before scripts run.
    fn initial_color_mode(&self) -> gazette_core::markdown::InitialColorMode {
        gazette_core::markdown::InitialColorMode::System
    }
}

/// Parse exact supplied bytes without paths, ambient configuration or remote reads.
pub trait Markdown: Send + Sync {
    /// Highlight exact code bytes as inert HTML using the adapter's code-fence renderer.
    /// `None` requests the caller's escaped plain-text fallback. No wrapper or source
    /// normalization is added; the caller owns the code block and its language metadata.
    ///
    /// # Errors
    /// Reject exhausted input/output bounds or formatter failure without partial output.
    fn highlight_code(
        &self,
        _code: &str,
        _language: &str,
        _limits: Limits,
    ) -> Result<Option<String>, Error> {
        Ok(None)
    }

    /// Stable parser/version and behavior profile used for pre-render dependency identity.
    fn profile(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Retain exact inputs and bounded facts, not public parser state.
    ///
    /// # Errors
    /// Refuse invalid coordinates, unsupported parser constructs or exhausted bounds.
    fn parse(
        &self,
        source: &str,
        profile: Profile,
        limits: Limits,
    ) -> Result<Box<dyn Document>, Error>;

    /// Encode an already admitted generated static HTML fragment for Markdown
    /// transport, retaining its native tree and decoded text/attribute values.
    /// The caller still owns HTML/publication admission and source binding; this
    /// does not enable authored raw HTML or grant any publication permission.
    /// `location` is containing-source evidence, not offsets in generated HTML.
    ///
    /// # Errors
    /// Refuse malformed native trees, unsafe literal transport boundaries and
    /// exhausted input, tree or output bounds without returning a partial result.
    fn transport_static_html(
        &self,
        html: &str,
        location: Location,
        limits: Limits,
    ) -> Result<String, Error>;

    /// Present admitted generated HTML as readable Markdown. Implementations may
    /// retain inert HTML where Markdown cannot preserve its meaning.
    ///
    /// # Errors
    /// Reject malformed input and exhausted bounds without partial output.
    fn reading_static_html(
        &self,
        html: &str,
        location: Location,
        limits: Limits,
    ) -> Result<String, Error> {
        self.transport_static_html(html, location, limits)
    }
}

/// Immutable source-bound analysis that can be rendered independently across threads.
pub trait Document: Send + Sync {
    /// Whole-source evidence, distinct from normalized emitted meaning.
    fn syntax(&self) -> &Syntax;

    /// Check whole-source formatter limitations before caller resolution can run.
    /// This does not admit publication policy or substitute for complete plan validation.
    ///
    /// # Errors
    /// Report a specifically unsupported formatting construct without hiding its inspection facts.
    fn check_rendering(&self) -> Result<(), Error>;

    /// Format using a complete matching compiler plan and the retained exact inputs.
    ///
    /// # Errors
    /// Reject wrong identity, missing/ambiguous bindings, unsupported syntax and output exhaustion.
    fn render(
        &self,
        plan: &Plan,
        components: Option<&dyn ComponentRenderer>,
    ) -> Result<Rendered, Error>;

    /// Format the same admitted plan as readable Markdown, preserving anchors
    /// and retaining inert HTML only where Markdown cannot preserve meaning.
    /// This is not an export of unadmitted source or a second resolution operation.
    ///
    /// # Errors
    /// Reject the same identity, binding, syntax and bounded-output failures as HTML.
    fn render_markdown(
        &self,
        plan: &Plan,
        components: Option<&dyn ComponentRenderer>,
    ) -> Result<String, Error>;

    /// Project Markdown with exactly one binding per whole-source link/image.
    /// Destination policy belongs to the caller; this does not apply HTML publication policy.
    /// Unchanged bindings preserve original bytes; changed bindings use native formatting.
    ///
    /// # Errors
    /// Refuse wrong identity, incomplete or ambiguous occurrences, unsafe destinations,
    /// constructs the formatter cannot preserve, and exhausted output bounds.
    fn rewrite_links(&self, identity: Identity, links: &[LinkBinding]) -> Result<String, Error>;

    /// Transport retained Markdown into inert MDX with the same exact link bindings.
    /// The caller may replace a matching top-level title heading with its bound
    /// anchor when the target renders that title separately. Code and literal
    /// text remain data; this does not admit JSX, imports or expression execution.
    ///
    /// # Errors
    /// Reject mismatched source facts, unsupported syntax, unsafe transport and
    /// exhausted output bounds without returning partial output.
    fn transport_mdx(
        &self,
        identity: Identity,
        links: &[LinkBinding],
        headings: &[HeadingBinding],
        title: Option<&HeadingBinding>,
    ) -> Result<String, Error>;

    /// Project provider Markdown and its admitted text without reconstructing meaning from output files.
    /// Legacy adapters retain their exact body and explicitly report text as unavailable.
    /// # Errors
    /// Preserves source, target-anchor, admission and output-bound failures.
    fn project_mdx(
        &self,
        identity: Identity,
        links: &[LinkBinding],
        headings: &[HeadingBinding],
        title: Option<&HeadingBinding>,
        anchors: Option<&gazette_core::documentation::projection::AnchorProjection>,
    ) -> Result<gazette_core::markdown::TransportedMarkdown, Error> {
        let body = match anchors {
            Some(anchors) => {
                self.transport_mdx_with_anchors(identity, links, headings, title, anchors)
            }
            None => self.transport_mdx(identity, links, headings, title),
        }?;
        Ok(gazette_core::markdown::TransportedMarkdown { body, text: None })
    }

    /// Qualified Mintlify MDX transport with an explicit target-only anchor policy.
    /// Deep Markdown headings and recognized leading GitHub alerts use fixed
    /// provider-native components. Authored components and unqualified raw HTML
    /// deep headings remain refused.
    /// The default `transport_mdx` does not require this provider component.
    /// Heading bindings
    /// already carry projected IDs; raw admitted id/name attributes use this
    /// same policy. An implementation without this qualification fails closed.
    ///
    /// # Errors
    /// Preserves all ordinary MDX admission and bound failures, and refuses an
    /// unavailable target-anchor implementation rather than returning old IDs.
    fn transport_mdx_with_anchors(
        &self,
        _identity: Identity,
        _links: &[LinkBinding],
        _headings: &[HeadingBinding],
        _title: Option<&HeadingBinding>,
        _anchors: &gazette_core::documentation::projection::AnchorProjection,
    ) -> Result<String, Error> {
        Err(Error::InvalidPlan)
    }
}
