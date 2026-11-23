//! Provider-neutral authored-manual route, anchor and link projection.

use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
};

use html5ever::{
    tendril::StrTendril,
    tokenizer::{BufferQueue, Tag, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts},
};
use percent_encoding::percent_decode_str;

use crate::{
    documentation::Error,
    markdown::{LinkBinding, LinkKind, Profile, Syntax},
    source::EntryPath,
};

mod catalog;

/// Repository manuals admit native formula facts in both editor and target projection.
pub const MANUAL_PROFILE: Profile = Profile::PublicationMath;

/// Target-only, injective escaping for a provider's reserved anchor namespace.
/// Source definitions and source-fragment validation keep their original IDs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnchorProjection {
    prefix: &'static str,
    reserved: &'static [&'static str],
}

impl AnchorProjection {
    /// Select a target namespace disjoint from the provider's reserved IDs.
    ///
    /// # Errors
    /// Reject an empty/unsafe prefix or a reserved ID inside that namespace.
    pub fn new(prefix: &'static str, reserved: &'static [&'static str]) -> Result<Self, Error> {
        if prefix.is_empty()
            || !prefix
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            || reserved
                .iter()
                .any(|id| id.is_empty() || id.starts_with(prefix))
        {
            return Err(invalid(
                prefix,
                "target anchor namespace must be safe and disjoint from reserved IDs",
            ));
        }
        Ok(Self { prefix, reserved })
    }

    /// Project an admitted source ID without introducing collisions among sources.
    pub fn id(&self, id: &str) -> String {
        if id.starts_with(self.prefix) || self.reserved.contains(&id) {
            format!("{}{id}", self.prefix)
        } else {
            id.to_owned()
        }
    }

    fn fragment(&self, fragment: &str) -> Result<String, Error> {
        let id = decode_fragment(fragment)?;
        Ok(
            if id.starts_with(self.prefix) || self.reserved.contains(&id.as_str()) {
                format!("{}{fragment}", self.prefix)
            } else {
                fragment.to_owned()
            },
        )
    }
}

/// One configured manual page joined to its exact retained editor baseline.
pub struct AuthoringManualPage<'a> {
    /// Exact configured manual-source key which owns this retained page.
    pub source: &'a str,
    /// Capture-relative path within that configured source.
    pub input: &'a EntryPath,
    /// Target-local output route selected by the manual navigation.
    pub route: &'a EntryPath,
    /// Exact retained UTF-8 Markdown bytes supplied by the authoring client.
    pub bytes: &'a [u8],
}

/// One configured generated-reference root whose provider artifact is absent in authoring.
pub struct AuthoringReference<'a> {
    /// Target-local generated-reference root selected by the workspace.
    pub route: &'a EntryPath,
}

/// Result of validating the authoring session's retained documentation closure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthoringClosure {
    /// Every selected documentation route and link is closed by retained inputs.
    Complete,
    /// Manual closure is valid, but selected generator artifacts are not retained.
    GeneratedReferencesIncomplete,
}

/// Target-independent destination mapping for one retained authored-manual source.
#[derive(Default)]
pub struct ManualDestinations {
    destinations: BTreeMap<String, Destination>,
}

enum Destination {
    Page(String),
    Asset(String),
}

impl ManualDestinations {
    /// Register one exact retained Markdown input and its configured target route.
    ///
    /// # Errors
    /// Rejects selecting the same input more than once in one configured source.
    pub fn page(&mut self, input: &EntryPath, route: &EntryPath) -> Result<(), Error> {
        self.insert(
            input,
            Destination::Page(route.to_string()),
            "authored manual source file is selected more than once",
        )
    }

    /// Register one exact retained asset input and its configured target path.
    ///
    /// # Errors
    /// Rejects selecting the same input as more than one manual item.
    pub fn asset(&mut self, input: &EntryPath, target: &str) -> Result<(), Error> {
        self.insert(
            input,
            Destination::Asset(target.to_owned()),
            "manual input is selected as both a page and an asset",
        )
    }

    /// Find an unselected relative companion using the manual link grammar.
    /// Markdown pages keep their explicit navigation selection; external and
    /// target-relative links do not select files from the captured source.
    ///
    /// # Errors
    /// Rejects malformed relative paths and links escaping the captured source.
    pub fn linked_companion(
        &self,
        source: &EntryPath,
        destination: &str,
    ) -> Result<Option<EntryPath>, Error> {
        if destination.starts_with('/')
            || destination.starts_with('#')
            || ["https://", "http://", "mailto:"]
                .iter()
                .any(|scheme| destination.starts_with(scheme))
        {
            return Ok(None);
        }
        let (path, _) = split(destination)?;
        let resolved = resolve(source.as_str(), path)?;
        if self.destinations.contains_key(&resolved)
            || resolved.ends_with(".md")
            || resolved.ends_with(".mdx")
        {
            return Ok(None);
        }
        resolved
            .parse()
            .map(Some)
            .map_err(|_| invalid(destination, "invalid manual companion path"))
    }

    fn insert(
        &mut self,
        input: &EntryPath,
        destination: Destination,
        reason: &'static str,
    ) -> Result<(), Error> {
        if self
            .destinations
            .insert(input.as_str().to_owned(), destination)
            .is_some()
        {
            return Err(invalid(input.as_str(), reason));
        }
        Ok(())
    }
}

/// Exact target route and anchor catalog shared by authoring and final assembly.
#[derive(Default)]
pub struct LinkProjection {
    preview_link_bases: Option<crate::publication::routing::PreviewLinkBases>,
    target_anchors: Option<(AnchorProjection, BTreeSet<String>)>,
    target_pages: BTreeMap<String, String>,
    required_pages: BTreeSet<(String, Option<String>)>,
    files: BTreeSet<String>,
    pages: BTreeMap<String, Page>,
    /// Emitted pages that links may reach but that stay out of navigation and the catalog.
    hidden_pages: BTreeSet<String>,
    descriptions: BTreeMap<String, super::catalog::Description>,
    description_bytes: usize,
}

/// Evidence available for one provider-produced documentation page.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum PageKind {
    /// One physically emitted Markdown/MDX page with exact anchors.
    Physical,
    /// One generator-qualified virtual page; anchors may remain provider-owned.
    QualifiedVirtual,
    /// One selected generator root whose retained provider artifact is unavailable.
    Incomplete,
}

struct Page {
    kind: PageKind,
    anchors: Option<BTreeSet<String>>,
}

/// Complete link-closure result over one target catalog.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LinkClosure {
    incomplete: bool,
}

impl LinkClosure {
    /// Whether closure depends on generated provider output absent from this input set.
    pub fn incomplete(self) -> bool {
        self.incomplete
    }
}

impl LinkProjection {
    /// Bind exact logical physical pages to target-owned output routes.
    /// Source closure and anchor checks keep the logical identities; only output
    /// addresses and physical-existence checks use this injective mapping.
    ///
    /// # Errors
    /// Reject invalid route spelling or duplicate physical destinations.
    pub fn with_page_routes(mut self, routes: BTreeMap<String, String>) -> Result<Self, Error> {
        let mut targets = BTreeSet::new();
        for (source, target) in &routes {
            for route in [source, target] {
                if !route.split('/').all(|part| {
                    !matches!(part, "" | "." | "..")
                        && part.bytes().all(|byte| {
                            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')
                        })
                }) || route.starts_with("artifacts/")
                {
                    return Err(invalid(route, "page projection requires exact safe routes"));
                }
            }
            if !targets.insert(target) {
                return Err(invalid(
                    target,
                    "page projection has duplicate target routes",
                ));
            }
        }
        self.target_pages = routes;
        Ok(self)
    }

    /// Physical address of an exact logical page; unselected routes stay unchanged.
    pub fn page_route<'a>(&'a self, route: &'a str) -> &'a str {
        self.target_pages.get(route).map_or(route, String::as_str)
    }

    /// Bind target anchor escaping only to an exact selected physical route set.
    pub fn with_anchor_projection(
        mut self,
        projection: AnchorProjection,
        routes: BTreeSet<String>,
    ) -> Self {
        self.target_anchors = Some((projection, routes));
        self
    }

    /// Selected target policy for one exact physical page; never a guessed prefix.
    pub fn anchor_projection(&self, route: &str) -> Option<&AnchorProjection> {
        self.target_anchors
            .as_ref()
            .filter(|(_, routes)| routes.contains(route))
            .map(|(projection, _)| projection)
    }

    /// Project an owned fragment while preserving its encoded source spelling.
    ///
    /// # Errors
    /// Reject invalid fragment spelling through the existing source validator.
    pub fn project_fragment(&self, route: &str, fragment: &str) -> Result<String, Error> {
        match self.anchor_projection(route) {
            Some(projection) => projection.fragment(fragment),
            None => {
                decode_fragment(fragment)?;
                Ok(fragment.to_owned())
            }
        }
    }
    /// Select explicit local bases for ordinary manual hyperlinks only.
    pub fn with_preview_link_bases(
        mut self,
        bases: Option<crate::publication::routing::PreviewLinkBases>,
    ) -> Self {
        self.preview_link_bases = bases;
        self
    }
    /// Register one manual page and project every retained Markdown destination.
    ///
    /// Returned bindings remain bound to the supplied syntax identity and must be
    /// applied through the same retained Markdown document.
    ///
    /// # Errors
    /// Rejects unsafe raw HTML, ambiguous routes, missing selected relative inputs,
    /// invalid fragments and duplicate emitted anchors.
    pub fn manual(
        &mut self,
        source: &EntryPath,
        route: &str,
        destinations: &ManualDestinations,
        syntax: &Syntax,
    ) -> Result<Vec<LinkBinding>, Error> {
        let anchors = manual_anchors(syntax, source.as_str()).map_err(|failure| failure.error)?;
        self.register(route, PageKind::Physical, Some(anchors.ids))?;
        let mut bindings = Vec::with_capacity(syntax.links.len());
        for link in &syntax.links {
            let destination = &link.destination;
            if destination.chars().any(char::is_control) {
                return Err(invalid(
                    source.as_str(),
                    "control-containing Markdown destination",
                ));
            }
            if ["https://", "http://", "mailto:"].iter().any(|scheme| {
                destination
                    .strip_prefix(scheme)
                    .is_some_and(|tail| !tail.is_empty())
            }) {
                bindings.push(LinkBinding {
                    image_sources: None,
                    svg_layout: None,
                    image_width: None,
                    image_dimensions: None,
                    image_loading: None,
                    image_fetch_priority: None,
                    inline_svg: None,
                    image_address: None,
                    occurrence: link.clone(),
                    destination: if link.kind == LinkKind::Link {
                        self.external_link(destination)?
                    } else {
                        destination.clone()
                    },
                    annotation: None,
                });
                continue;
            }
            if destination.starts_with('#') && destination.len() > 1 {
                self.project(&format!("/{route}{destination}"))?;
                bindings.push(LinkBinding {
                    image_sources: None,
                    svg_layout: None,
                    image_width: None,
                    image_dimensions: None,
                    image_loading: None,
                    image_fetch_priority: None,
                    inline_svg: None,
                    image_address: None,
                    occurrence: link.clone(),
                    destination: format!("#{}", self.project_fragment(route, &destination[1..])?),
                    annotation: None,
                });
                continue;
            }
            let projected = if destination.starts_with('/') {
                if link.kind == LinkKind::Image {
                    self.project_file(destination)?
                } else {
                    self.project(destination)?
                }
            } else {
                let (path, fragment) = split(destination)?;
                let source_target = resolve(source.as_str(), path)?;
                match destinations.destinations.get(&source_target) {
                    Some(Destination::Page(target)) if link.kind != LinkKind::Image => {
                        let destination = match fragment {
                            Some(fragment) => format!("/{target}#{fragment}"),
                            None => format!("/{target}"),
                        };
                        self.project(&destination)?
                    }
                    Some(Destination::Asset(target)) => {
                        if fragment.is_some() {
                            return Err(invalid(
                                destination,
                                "manual asset link cannot contain a fragment",
                            ));
                        }
                        self.project_file(&format!("/{target}"))?
                    }
                    Some(Destination::Page(_)) => {
                        return Err(invalid(destination, "manual image cannot target a page"));
                    }
                    None => {
                        return Err(invalid(
                            destination,
                            "relative manual link has no exact selected page or asset",
                        ));
                    }
                }
            };
            bindings.push(LinkBinding {
                image_sources: None,
                svg_layout: None,
                image_width: None,
                image_dimensions: None,
                image_loading: None,
                image_fetch_priority: None,
                inline_svg: None,
                image_address: None,
                occurrence: link.clone(),
                destination: projected,
                annotation: None,
            });
        }
        Ok(bindings)
    }

    /// Project an external hyperlink using the selected preview bases.
    ///
    /// # Errors
    /// Refuses unsafe mapped paths and loopback hyperlinks in public documentation.
    pub fn external_link(&self, destination: &str) -> Result<String, Error> {
        if self.preview_link_bases.is_none()
            && crate::publication::routing::is_loopback_link(destination)
        {
            return Err(invalid(
                destination,
                "public documentation cannot link to loopback",
            ));
        }
        self.preview_link_bases
            .as_ref()
            .map(|bases| bases.project(destination))
            .transpose()
            .map_err(|_| {
                invalid(
                    destination,
                    "unsafe path under a selected preview link base",
                )
            })
            .map(|projected| {
                projected
                    .flatten()
                    .unwrap_or_else(|| destination.to_owned())
            })
    }

    /// Project one root-relative page destination into the target route profile.
    ///
    /// # Errors
    /// Rejects traversal, reserved namespaces, queries and invalid fragments.
    pub fn project(&mut self, destination: &str) -> Result<String, Error> {
        let (path, fragment) = match destination.split_once('#') {
            Some((_, "")) => return Err(invalid(destination, "empty Markdown fragment")),
            Some((path, fragment)) => (path, Some(fragment)),
            None => (destination, None),
        };
        let path = path
            .strip_prefix('/')
            .ok_or_else(|| invalid(destination, "expected root-relative page route"))?;
        let path = path
            .strip_suffix(".mdx")
            .or_else(|| path.strip_suffix(".md"))
            .unwrap_or(path);
        if path.starts_with("artifacts/")
            || !path.split('/').all(|part| {
                !matches!(part, "" | "." | "..")
                    && part.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')
                    })
            })
        {
            return Err(invalid(
                destination,
                "root links require an exact emitted page route, without traversal, aliases or queries",
            ));
        }
        let projected = match fragment {
            Some(fragment) => format!(
                "/{}#{}",
                self.page_route(path),
                self.project_fragment(path, fragment)?
            ),
            None => format!("/{}", self.page_route(path)),
        };
        let fragment = fragment.map(decode_fragment).transpose()?;
        self.required_pages.insert((path.to_owned(), fragment));
        Ok(projected)
    }

    /// Register one physically emitted Markdown page with exact anchor evidence.
    ///
    /// # Errors
    /// Rejects duplicate routes and invalid or duplicate heading anchors.
    pub fn register_markdown(
        &mut self,
        route: &str,
        syntax: &Syntax,
        raw_anchors: BTreeSet<String>,
    ) -> Result<(), Error> {
        self.register(
            route,
            PageKind::Physical,
            Some(heading_anchors(syntax, raw_anchors)?),
        )
    }

    /// Register one generated provider route and its available anchor evidence.
    ///
    /// # Errors
    /// Rejects duplicate routes and physical pages without exact anchors.
    pub fn register_provider(
        &mut self,
        route: &str,
        kind: PageKind,
        anchors: Option<BTreeSet<String>>,
    ) -> Result<(), Error> {
        if kind == PageKind::Physical && anchors.is_none() {
            return Err(invalid(
                route,
                "physical provider page requires an exact emitted anchor catalog",
            ));
        }
        self.register(route, kind, anchors)
    }

    /// Register one emitted page that links may reach without a catalog entry or anchors,
    /// such as a hidden page carrying a text file. Fragment links to it are refused.
    ///
    /// # Errors
    /// Rejects a route already registered as any other page.
    pub fn register_hidden(&mut self, route: &str) -> Result<(), Error> {
        if self.pages.contains_key(route) || !self.hidden_pages.insert(route.to_owned()) {
            return Err(invalid(
                route,
                "documentation page route was registered more than once",
            ));
        }
        Ok(())
    }

    fn register(
        &mut self,
        route: &str,
        kind: PageKind,
        anchors: Option<BTreeSet<String>>,
    ) -> Result<(), Error> {
        if self.hidden_pages.contains(route)
            || self
                .pages
                .insert(route.to_owned(), Page { kind, anchors })
                .is_some()
        {
            return Err(invalid(
                route,
                "documentation page route was registered more than once",
            ));
        }
        Ok(())
    }

    /// Project one root-relative retained asset destination.
    ///
    /// # Errors
    /// Rejects fragments, queries, traversal and reserved artifact paths.
    pub fn project_file(&mut self, destination: &str) -> Result<String, Error> {
        if destination.contains('#') || destination.contains('?') {
            return Err(invalid(
                destination,
                "manual asset links do not accept fragments or queries",
            ));
        }
        let path = destination
            .strip_prefix('/')
            .ok_or_else(|| invalid(destination, "expected root-relative asset path"))?;
        if path.is_empty()
            || path.starts_with("artifacts/")
            || path.split('/').any(|part| matches!(part, "" | "." | ".."))
        {
            return Err(invalid(
                destination,
                "asset link requires an exact emitted target path",
            ));
        }
        self.files.insert(path.to_owned());
        Ok(format!("/{path}"))
    }

    /// Close all retained destinations against the exact emitted target catalog.
    ///
    /// # Errors
    /// Rejects missing physical pages or files, unqualified generated pages and
    /// fragments without exact anchor evidence.
    pub fn validate(
        self,
        page_exists: impl FnMut(&str) -> bool,
        file_exists: impl FnMut(&str) -> bool,
    ) -> Result<LinkClosure, Error> {
        self.check_links(page_exists, file_exists)
    }

    fn check_links(
        &self,
        mut page_exists: impl FnMut(&str) -> bool,
        mut file_exists: impl FnMut(&str) -> bool,
    ) -> Result<LinkClosure, Error> {
        let mut closure = LinkClosure::default();
        for (route, fragment) in &self.required_pages {
            if self.hidden_pages.contains(route) {
                if fragment.is_some() || !page_exists(route) {
                    return Err(invalid(
                        route,
                        "hidden page link needs an emitted page and accepts no fragment",
                    ));
                }
                continue;
            }
            let Some(page) = self.pages.get(route) else {
                if self.pages.iter().any(|(root, page)| {
                    page.kind == PageKind::Incomplete
                        && route
                            .strip_prefix(root)
                            .is_some_and(|suffix| suffix.starts_with('/'))
                }) {
                    closure.incomplete = true;
                    continue;
                }
                return Err(invalid(
                    route,
                    "root Markdown link has no exact emitted or qualified provider page in the complete documentation target",
                ));
            };
            if page.kind == PageKind::Physical {
                if !page_exists(
                    self.target_pages
                        .get(route)
                        .map_or(route.as_str(), String::as_str),
                ) {
                    return Err(invalid(
                        route,
                        "root Markdown link has no exact emitted page in the complete documentation target",
                    ));
                }
            } else if page.kind == PageKind::Incomplete {
                closure.incomplete = true;
            }
            if let Some(fragment) = fragment {
                match (&page.anchors, page.kind) {
                    (Some(anchors), _) if anchors.contains(fragment) => {}
                    (_, PageKind::Incomplete) => closure.incomplete = true,
                    (Some(_), _) => {
                        return Err(invalid(
                            format!("{route}#{fragment}"),
                            "Markdown link fragment has no exact emitted page anchor",
                        ));
                    }
                    (None, PageKind::QualifiedVirtual) => {
                        return Err(invalid(
                            format!("{route}#{fragment}"),
                            "provider-backed page has no qualified anchor catalog; symbol anchors are not guessed",
                        ));
                    }
                    (None, PageKind::Physical) => {
                        unreachable!("physical page registration requires an exact anchor catalog")
                    }
                }
            }
        }
        for path in &self.files {
            if !file_exists(path) {
                return Err(invalid(
                    path,
                    "manual asset link has no exact emitted file in the complete documentation target",
                ));
            }
        }
        Ok(closure)
    }
}

/// Validate exact authored-manual bytes before structural Markdown projection.
///
/// # Errors
/// Rejects non-UTF-8, empty, NUL-containing or frontmatter-driven manual bodies.
pub fn manual_source<'a>(bytes: &'a [u8], source: &EntryPath) -> Result<&'a str, Error> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| invalid(source.as_str(), "authored manual Markdown must be UTF-8"))?;
    if text.trim().is_empty() || text.contains('\0') {
        return Err(invalid(
            source.as_str(),
            "authored manual Markdown is empty or contains NUL",
        ));
    }
    let first = text
        .strip_prefix('\u{feff}')
        .unwrap_or(text)
        .lines()
        .next()
        .unwrap_or_default()
        .trim();
    if matches!(first, "---" | "+++") {
        return Err(invalid(
            source.as_str(),
            "manual title and route are configuration-owned; authored frontmatter is unsupported",
        ));
    }
    Ok(text)
}

/// Validate one retained HTML fragment and return its exact authored anchors.
///
/// # Errors
/// Rejects malformed or active markup, unsafe attributes, unverified references
/// and duplicate or invalid anchors.
pub fn manual_html_anchors(fragment: &str, context: &str) -> Result<BTreeSet<String>, Error> {
    validate_html(fragment, context, HtmlReferences::Forbidden)
}

/// Validate generated markup whose ordinary adapter has already checked references.
///
/// # Errors
/// Rejects malformed or active markup, unsafe attributes and duplicate or invalid anchors.
pub fn checked_html_anchors(fragment: &str, context: &str) -> Result<BTreeSet<String>, Error> {
    validate_html(fragment, context, HtmlReferences::Checked)
}

fn split(destination: &str) -> Result<(&str, Option<&str>), Error> {
    if destination.contains('?') {
        return Err(invalid(
            destination,
            "manual relative links do not accept queries",
        ));
    }
    match destination.split_once('#') {
        Some((_, "")) => Err(invalid(destination, "empty Markdown fragment")),
        Some((path, fragment)) if !path.is_empty() => Ok((path, Some(fragment))),
        None if !destination.is_empty() => Ok((destination, None)),
        _ => Err(invalid(destination, "manual relative link has no path")),
    }
}

fn resolve(origin: &str, target: &str) -> Result<String, Error> {
    let mut components: Vec<_> = origin.split('/').collect();
    components.pop();
    for component in target.split('/') {
        match component {
            "." => {}
            ".." => {
                if components.pop().is_none() {
                    return Err(invalid(target, "manual link escapes its retained source"));
                }
            }
            "" => {
                return Err(invalid(
                    target,
                    "manual link contains an empty path segment",
                ));
            }
            component => components.push(component),
        }
    }
    Ok(components.join("/"))
}

fn decode_fragment(value: &str) -> Result<String, Error> {
    let fragment = percent_decode_str(value)
        .decode_utf8()
        .map_err(|_| invalid(value, "Markdown fragment is not valid UTF-8"))?;
    if fragment.is_empty() || fragment.chars().any(|character| character.is_control()) {
        return Err(invalid(
            value,
            "Markdown fragment is empty or contains controls",
        ));
    }
    Ok(fragment.into_owned())
}

/// Allocate source-bound manual headings with the same policy as link closure.
///
/// # Errors
/// Reject malformed HTML, invalid or colliding IDs and exhausted suffix counters.
pub fn manual_heading_bindings(
    syntax: &Syntax,
    context: &str,
) -> Result<Vec<crate::markdown::HeadingBinding>, Error> {
    manual_anchors(syntax, context)
        .map(|anchors| anchors.headings)
        .map_err(|failure| failure.error)
}

/// Admitted manual anchor names and their source-bound headings.
pub struct ManualAnchors {
    /// All named anchors, including HTML anchors which are not headings.
    pub ids: BTreeSet<String>,
    /// Emitted headings using the same allocation as manual link closure.
    pub headings: Vec<crate::markdown::HeadingBinding>,
}

/// A rejected manual declaration at its original source construct.
#[derive(Debug)]
pub struct ManualAnchorError {
    /// Exact containing HTML or heading construct in the original Markdown.
    pub location: crate::source::Location,
    /// The documentation owner's admission failure.
    pub error: Error,
}

/// Admit manual HTML and allocate headings once under the manual owner's policy.
///
/// # Errors
/// Rejects malformed or active HTML and invalid or colliding anchor declarations.
pub fn manual_anchors(syntax: &Syntax, context: &str) -> Result<ManualAnchors, ManualAnchorError> {
    let mut raw = BTreeSet::new();
    for html in &syntax.html {
        let fail = |error| ManualAnchorError {
            location: html.location,
            error,
        };
        for id in manual_html_anchors(&html.literal, context).map_err(fail)? {
            if !raw.insert(id) {
                return Err(fail(invalid(context, "duplicate manual HTML anchor")));
            }
        }
    }
    let (ids, headings) = allocate_headings(syntax, raw)?;
    Ok(ManualAnchors { ids, headings })
}

fn heading_anchors(syntax: &Syntax, reserved: BTreeSet<String>) -> Result<BTreeSet<String>, Error> {
    allocate_headings(syntax, reserved)
        .map(|anchors| anchors.0)
        .map_err(|failure| failure.error)
}

fn allocate_headings(
    syntax: &Syntax,
    mut reserved: BTreeSet<String>,
) -> Result<(BTreeSet<String>, Vec<crate::markdown::HeadingBinding>), ManualAnchorError> {
    for heading in &syntax.headings {
        let fail = |reason| ManualAnchorError {
            location: heading.location,
            error: invalid(heading.id.as_deref().unwrap_or_default(), reason),
        };
        if let Some(id) = &heading.id {
            if id.is_empty()
                || id
                    .chars()
                    .any(|character| character.is_whitespace() || character.is_control())
            {
                return Err(fail(
                    "heading anchor must be nonempty without whitespace or controls",
                ));
            }
            if !reserved.insert(id.clone()) {
                return Err(fail("duplicate explicit heading anchor"));
            }
        }
    }
    let mut suffixes = BTreeMap::<String, usize>::new();
    let mut emitted = reserved.clone();
    let mut bindings = Vec::new();
    for heading in &syntax.headings {
        let id = if let Some(id) = &heading.id {
            id.clone()
        } else {
            let base = match slug::slugify(&heading.text) {
                value if value.is_empty() => "section".to_owned(),
                value => value,
            };
            let next = suffixes.entry(base.clone()).or_default();
            loop {
                let candidate = if *next == 0 {
                    base.clone()
                } else {
                    format!("{base}-{next}")
                };
                *next = next.checked_add(1).ok_or_else(|| ManualAnchorError {
                    location: heading.location,
                    error: invalid(&base, "heading anchor suffix counter overflowed"),
                })?;
                if reserved.insert(candidate.clone()) {
                    break candidate;
                }
            }
        };
        if heading.emitted {
            emitted.insert(id.clone());
            bindings.push(crate::markdown::HeadingBinding {
                occurrence: heading.clone(),
                id,
                level: heading.level,
            });
        }
    }
    Ok((emitted, bindings))
}

#[derive(Clone, Copy)]
enum HtmlReferences {
    Checked,
    Forbidden,
}

struct HtmlState<'a> {
    context: &'a str,
    references: HtmlReferences,
    anchors: BTreeSet<String>,
    failure: Option<Error>,
}

impl HtmlState<'_> {
    fn fail(&mut self, reason: &'static str) {
        if self.failure.is_none() {
            self.failure = Some(invalid(self.context, reason));
        }
    }

    fn start(&mut self, tag: Tag) {
        let name = tag.name.as_ref();
        if matches!(
            name,
            "script"
                | "style"
                | "iframe"
                | "object"
                | "embed"
                | "form"
                | "input"
                | "button"
                | "textarea"
                | "select"
                | "option"
                | "link"
                | "meta"
                | "base"
                | "audio"
                | "video"
                | "source"
                | "track"
                | "canvas"
                | "svg"
                | "math"
                | "frame"
                | "frameset"
        ) {
            self.fail("active generated HTML element is unsupported");
            return;
        }
        for attribute in &tag.attrs {
            let key = attribute.name.local.as_ref();
            if !attribute.name.ns.is_empty()
                || attribute.name.prefix.is_some()
                || key.starts_with("on")
                || matches!(
                    key,
                    "style" | "srcdoc" | "srcset" | "action" | "formaction" | "poster" | "ping"
                )
            {
                self.fail("active generated HTML attribute is unsupported");
                return;
            }
            if matches!(self.references, HtmlReferences::Forbidden) && matches!(key, "href" | "src")
            {
                self.fail("raw generated HTML cannot bypass checked references");
                return;
            }
            if key == "id" || (name == "a" && key == "name") {
                let value = attribute.value.as_ref();
                if value.is_empty()
                    || value
                        .chars()
                        .any(|character| character.is_whitespace() || character.is_control())
                {
                    self.fail("generated HTML anchor is empty or contains whitespace or controls");
                    return;
                }
                if !self.anchors.insert(value.to_owned()) {
                    self.fail("generated HTML anchor is duplicated");
                    return;
                }
            }
        }
    }
}

struct HtmlSink<'a>(RefCell<HtmlState<'a>>);

impl TokenSink for HtmlSink<'_> {
    type Handle = ();

    fn process_token(&self, token: Token, _: u64) -> TokenSinkResult<Self::Handle> {
        let mut state = self.0.borrow_mut();
        if state.failure.is_some() {
            return TokenSinkResult::Continue;
        }
        match token {
            Token::TagToken(tag) if tag.kind == html5ever::tokenizer::TagKind::StartTag => {
                state.start(tag);
            }
            Token::ParseError(_) => state.fail("malformed generated HTML fragment"),
            _ => {}
        }
        TokenSinkResult::Continue
    }
}

fn validate_html(
    fragment: &str,
    context: &str,
    references: HtmlReferences,
) -> Result<BTreeSet<String>, Error> {
    let sink = HtmlSink(RefCell::new(HtmlState {
        context,
        references,
        anchors: BTreeSet::new(),
        failure: None,
    }));
    let tokenizer = Tokenizer::new(
        sink,
        TokenizerOpts {
            exact_errors: true,
            discard_bom: false,
            ..Default::default()
        },
    );
    let input = BufferQueue::default();
    input.push_back(StrTendril::from(fragment));
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    let state = tokenizer.sink.0.into_inner();
    state.failure.map_or(Ok(state.anchors), Err)
}

fn invalid(context: impl Into<String>, reason: &'static str) -> Error {
    Error::Invalid {
        context: context.into(),
        reason,
    }
}
