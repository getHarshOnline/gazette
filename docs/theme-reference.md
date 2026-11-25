# Theme contract reference

A Theme template receives one page context containing the writing, resolved URLs, navigation and resources it needs to render. The workspace chooses the Theme, palette, capabilities and files. Gazette resolves those choices before it invokes Tera.

Use this reference to find what a template can read and what belongs in the workspace.

| Template question | Reference |
| --- | --- |
| What files and entrypoint does a page need? | [Build a page template](#build-a-page-template), [virtual namespace](#virtual-tera-namespace) |
| Which template is selected for a content identity? | [Exact page-template bindings](#exact-page-template-bindings) |
| How do I render body, listings, search and navigation? | [Generated reading context](#generated-reading-context) |
| How do I use project, citation, image or redirect links? | [Project resources](#hosted-product-resources), [citation](#authored-citation-tools), [images](#emitted-reading-images), [redirects](#redirect-destination-facts) |
| How are recommendations chosen? | [Ranked recommendations](#ranked-recommendations) |
| What can template code execute, and what are its bounds? | [Markup and execution](#markup-and-execution-gates), [resource ceilings](#resource-ceilings), [Tera adapter](tera.md) |
| Where are the complete typed fields? | [Native schemas](#native-schemas) |

## Build a page template

Start with a Theme source that has a manifest and a template directory.

::::steps

:::step "Declare a page entry"

Put this in the Theme's root manifest.

```toml
package = "theme:root"
namespace = "root"
templates = "templates"
browser_theme_colors = { light = "#ffffff", dark = "#20221f" }
render = { page = "page.html" }
```

:::

:::step "Write the page template"

Save this as `templates/page.html`:

```html
<!doctype html>
<html>
  <head>
    <meta charset="utf-8">
    <title>{{ content.title }}</title>
    <link rel="canonical" href="{{ route.canonical_url }}">
  </head>
  <body><main>{{ body_html | safe }}</main></body>
</html>
```

:::

:::step "Render a page"

Choose the Theme through [workspace composition](workspace-presentation.md#theme-plugin-capability-provider-and-resource-composition) and [render a publication](local-rendering.md). Gazette resolves `root/page.html` inside the Theme's namespace. The output should use the content title and canonical URL, then show the body.

:::

::::

`content.title` and `route.canonical_url` use ordinary HTML escaping. Use `safe` here only for the compiler's `body_html`, which already includes an authored opening heading. The template reads the supplied context without opening the source. This manifest has no alias or graph entry.

[Your first publication](https://rawthoughts.in/gazette/first-publication/) includes a fuller Theme with collection and journey navigation. Look at `theme/theme.toml`, `theme/templates/page.html` and the workspace that chooses them.

When a filesystem source is used only by Themes and its root is a Git working repository, Gazette leaves out Git-ignored tooling such as an ignored `node_modules` directory before opening its contents. Git supplies the ignore rules, including nested rules and exceptions; tracked inputs remain included. A configured `subdirectory` uses that repository root's rules. Literal exclusions still apply, and an `included-filesystem` selection reads exactly the files or trees you named, even when Git ignores them. Sources shared with content, manuals, generated reference, plugins or policy inputs keep their ordinary selection, as do pinned repository sources and directories without a Git repository. A nonignored link or special file still fails capture.

## Virtual Tera namespace

Give every Theme package one stable lowercase namespace. Gazette mounts its template directory under that namespace and compiles the dependency graph together. A package named `theme:root` with namespace `root` and `render.page = "page.html"` has entry `root/page.html`. A dependency with namespace `foundation` can supply `foundation/layout.html`.

The root package supplies the page, alias and graph entries. An omitted `render.llms`, `render.not_found` or `render.social_card` can come from an explicit dependency. A local declaration overrides it. Two independent providers fail instead of using whichever package comes first. The namespace of the defining package makes it possible for several roots to use one Base template.

Dependencies, includes, imports and inheritance resolve only in this virtual namespace. Gazette rejects cycles, undeclared packages, namespace collisions, missing entries and duplicate identities. It does not search other directories or derive a fallback from the site name.

## Supply interface copy

A Theme may declare default labels in its manifest’s `[chrome]` tables. Use the [site chrome fields](workspace-presentation.md#site-chrome-and-control-copy) for skip links, navigation, footer, search and settings, and `[chrome.regions]` for namespaced component copy. Dependencies provide omitted labels; a dependent Theme, workspace or site can override individual phrases. Gazette retains each label’s declaring package or configuration origin and rejects unresolved conflicts between dependency branches.

Themes cannot supply reading handoffs, no-script content destinations or search-control enablement through these defaults. Those remain explicit workspace or site choices. The compiled `presentation.chrome` contains the effective copy, and hosted components receive the same resolved region labels.

## Choose the template for one page {#exact-page-template-bindings}

Set `sites.composition.page_templates` to map a content ID to a path in the Theme namespace, such as `field-notes/product.html`. The site map replaces the workspace map as a whole. Omit it to inherit the workspace map or give an empty map to clear it.

Gazette checks in this order:

1. The composing site's binding for this content ID.
2. The source's optional `template` field.
3. The root Theme's ordinary `render.page` entry.

Use the composition origin API to see which setting won. The source declaration remains available. Gazette does not use a title, URL, filename or collection to guess a template. It rejects unknown content IDs, IDs on another site and template paths that are missing, directories or outside the Theme namespace. A generated listing continuation uses its owning content's template and ID.

## Generated reading context

### Body and opening heading

An ordinary page may carry `opening = {heading_html, remainder_html}` when its Markdown starts with a top-level H1. These are exact formatter-produced slices; `body_html` remains the complete body for existing consumers. Declared notebooks are not part of the body: they arrive in `notes.notebooks` as structured cells, described in [notebooks in Notes](notebooks.md#notebooks-in-notes). To compose a reading header, render `opening.heading_html` once with the native metadata and render `opening.remainder_html` once as the body. Never also render the full body in that composition. When `opening` is absent, preserve the complete `body_html`; do not promote a later heading or invent one from the document title. Heading IDs, inline links, Notes, search text and JSON-LD meaning remain independently unchanged.

### Publication metadata and navigation

Shared publication metadata includes an optional resolved `rights_holder` principal selected by the site, never a publisher fallback, and optional `render_year` supplied by rendering context. Page and generated-listing Themes read `site.metadata`; a graph Theme reads the same metadata through `presentation.publication`. Named navigation and footer entries retain their optional namespaced `key` and native destination facts. The Theme may use these public semantic addresses; it never resolves a URL from a label.

### Product-documentation outline

On a product-documentation collection and its descendants, optional `documentation_navigation` supplies the resolved manual outline. It contains a `root` navigation link, `current` for that entrance and balanced `events`. Render those events in their supplied order; no recursive template or route lookup is needed. A minimal page-template consumer is:

```html
{% if documentation_navigation is defined %}
<nav aria-label="{{ documentation_navigation.root.node.title }}">
  {% if documentation_navigation.root.route %}
  <a href="{{ documentation_navigation.root.route.url }}"{% if documentation_navigation.current %} aria-current="page"{% endif %}>{{ documentation_navigation.root.node.title }}</a>
  {% endif %}
  <ul>
  {% for item in documentation_navigation.events %}
    {% if item.kind == "group-open" %}
    <li><span>{{ item.title }}</span><ul>
    {% elif item.kind == "group-close" %}
    </ul></li>
    {% elif item.kind == "page-open" %}
    <li>
      {% if item.destination.route %}
      <a href="{{ item.destination.route.url }}"{% if item.current %} aria-current="page"{% endif %}>{{ item.destination.node.title }}</a>
      {% else %}<span>{{ item.destination.node.title }}</span>{% endif %}
      {% if item.has_children %}<ul>{% endif %}
    {% elif item.kind == "page-close" %}
      {% if item.has_children %}</ul>{% endif %}</li>
    {% endif %}
  {% endfor %}
  </ul>
</nav>
{% endif %}
```

`ancestor` on a page opening can mark the current branch. A page without a route remains a label, rather than receiving a guessed address. Group labels and page titles receive ordinary HTML escaping. Link classification, responsive controls and styling remain Theme decisions.

The [content model](content-model.md#order-and-group-a-product-manual) owns outline selection, complete coverage, inheritance and filtering. With no authored order, the compiler supplies structural order; standalone documentation outside a collection has no outline. Discovery projections omit ineligible destinations and empty groups and recompute `has_children`, so render each view's own balanced events rather than sharing a cached tree between private and discovery output.

### Search destination

A publication-selected Search page is addressed through `presentation.search.page` (also `presentation.chrome.shell.search.page`). Match its native content identity to the current page when selecting the search view, rather than testing a route spelling or publication name. An `all-pages` listing provides the complete admitted, paginated browse fallback using the same `ListingLink` fields below. The shared chrome labels inherit individually through the Theme, workspace and site declarations; each publication owns its independent `search_page` destination. A Theme may progressively enhance this native browse view with the configured search index and SDK, preserving usable ordinary links without JavaScript.

### Listing identity and pagination

A configured declaration-only index receives optional `listing`. Gazette selects and sorts its items before rendering. Render the supplied endpoints and order; the Theme does not derive routes, select candidates or maintain pagination state.

| Fields | Meaning |
| --- | --- |
| `content`, `title`, `route`, `url`, `output` | The existing index identity and the compiled destination for this view. |
| `page_number`, `page_size`, `page_count`, `total_items` | One-based current slice, configured slice size, total slices and complete item count. |
| `items` | Ordered existing-item references for this slice. |
| Optional `previous`, `next` | Resolved serving URLs for adjacent slices. |
| `variant` | `primary` or `alternate`. |
| Optional `direction` | Authored `ascending` or `descending` chronology; absent for priorities. |
| `pages` | At most seven `{number, url, current, gap_before}` entries: first, last and current plus/minus two. |
| Optional `sorts` | `{direction, url, current}` for each configured chronology, always linking to that ordering's page one. |

### Listing items, attribution and artwork

Each item refers to existing content with `id`, authored `title`, canonical `url` and optional declared `language`, `published`, `modified` and destination `presentation`. Optional `attributions` preserve the item's direct role order and use the same resolved principal shape as a page:

| Attribution field | Meaning |
| --- | --- |
| `principal`, `role` | The resolved principal and its directly declared role. |
| Optional `website.url` | Selected serving URL. |
| Optional `website.canonical_url` | Stable public canonical URL. |
| `website.destination` | Native link kind, optional site and destination presentation relative to the listing's owning site, present with the selected website. |

An empty attribution remains absent; neither the index nor the publisher supplies an author. A Theme can render `published` as `<time>` and an author's `website.url` with `rel="author"` without catalogue joins or per-page URL settings.

Optional `artwork` contains a required `default` URL and only explicitly authored `light`/`dark` URLs. These decorative URLs come from the eligible item owner's selected static asset plan; private source keys and entries are not exposed. A missing mode variant uses the default; a missing artwork field stays absent. The Theme owns preference dispatch, geometry and decorative markup, using these resolved URLs. Rendition occurrences and URL text share the complete listing bounds.

### Latest item

Optional `listing.latest` is one existing-item reference with the same shape as `items`. It is selected from the complete eligible, discoverable population before pagination and independently of display order. Selection uses the index's editorial-day convention: an authored calendar day for date-only values and the UTC day for timestamps. Within the newest day, timestamps compare by instant; equal instants or date-only values use title then identity as stable ties. Undated items are excluded.

The field is absent when no dated item exists or the newest day mixes date-only values and timestamps. Different precision on older days does not suppress a clear newest-day choice. This is an editorial-day selection, not an inferred time or timezone for a calendar date. Every continuation and alternate ordering receives the same reference within the shared projection bounds, so a Theme can link to the latest publication without fetching another rendered page. Themes should use this reference rather than guessing from the first item of a paginated or differently ordered list.

### Project article views

A `project-articles` view additionally supplies `listing.subject`:

| Field | Meaning |
| --- | --- |
| `project`, optional `name` | The exact subject project and configured display name. |
| `residence` | The explicit `all`, `standalone` or `collected` choice. |
| `landing` | `{id, title, url, destination}` for the selected project landing. |
| Optional `explore` | The configured own-publication unfiltered index in that same link shape, including in an empty view. |

Every item carries its exact `site` and native `destination` facts, preserving cross-publication link treatment for ecosystem articles. Use these fields for contextual copy and onward links. Subject context survives both chronologies and all continuation pages; generated JSON-LD identifies the same project and its landing. These are relevance views, so they add no article `browsing_locations`.

### Generated listing views

Primary page one is the original index. An alternate ordering's page one and all continuations are generated output views of that same content identity. They use the same template and presentation with a compiler-resolved serving route and listing slice. Each view's JSON-LD describes its collection-page view and ordered item references; it creates no additional authored page or search identity.

The shared site model exposes continuation contexts under output-keyed `listing_pages`. Private rendering evidence binds each exact template and emitted model/HTML digest separately. Generated indexes retain the authored document title and current navigation without needing a redundant visible or concealed page heading. Ordinary article bodies and article-owned Notes keep their separate heading contracts.

Declare selection, chronology, priority and pagination in [workspace listings](workspace-discovery.md#generated-reading-indexes), rather than template loops or source-folder conventions.

### Browsing locations

Article and container contexts may receive `browsing_locations`. Each shallow entry supplies a resolved browsing path:

| Field | Meaning |
| --- | --- |
| `index`, `title`, `url` | Existing index identity, literal index title and exact serving URL. |
| `via` | The listed identity: this page or one of its declared editorial ancestors. |
| `kind` | `listing` for a generated listing location, or `declared` for an authored `browsing` association. |
| `page_number`, `page_count` | Present for generated locations, identifying the slice containing the listed item. Absent for declared locations. |

Generated locations join against the same bounded, discovery-selected listing pages used for index rendering. They do not select or sort a second catalogue. Declared locations use the declaring page as `via`. Both kinds use resolved serving URLs, including the selected preview base. Multiple explicit paths stay multiple paths; no primary is inferred, and empty locations are omitted.

The complete generated-plus-declared array shares one occurrence and encoded-text ceiling, with an independent final render-model bound. A Theme may match `index` to configured navigation by identity, retain its label, mark the current location and expose the supplied URL. Preserve `via` when interpreting an ancestor's location. These entries establish browsing paths, not editorial parentage or the reader's arrival history.

### Project landing links

Project context can include optional `projects[].landing = {id, title, url, destination}` from the project’s explicitly selected introduction. Use that exact URL and destination metadata to link an existing project name; keep the name plain when the field is absent. The page’s About/Mentions relationship and its evidence remain separate and unchanged. This projection borrows the retained title and route; it does not copy a body or choose a manual/index as a substitute.

## Hosted product resources

A project-family page with exactly one direct `about` declaration receives optional `project_resources`. Hosted components receive the same value at `component.context.host.project_resources`; `projects[].resources` uses the same resolver for its exact project.

| Field | Meaning |
| --- | --- |
| `project`, optional `name` | Exact project identity and configured display name. |
| `actions` | Ordered `{kind, url, destination}` records for available destinations. |

Action kinds are `code-documentation`, `learn`, `github`, `mentions`, followed by any independently configured `blogs`, `thoughts` and `available-through`. Absent destinations are not fabricated. Each `destination` carries the native link kind, known content/site identity, relationship facts and selected destination presentation. The Theme uses the supplied links without joining catalogues or constructing routes.

Project facts may come from the selected authored landing or a complete inline workspace record; both produce the same joined view. Docs can derive from the exact configured documentation module. Learn omits links to the current page or the project's own landing. `available-through` points to the exact content destination declared for gated access, including another publication; it does not imply a generic purchase or trial action. See [project facts and resources](content-model.md#keep-project-facts-on-their-landing) for ownership and validation.

Hosted components also receive copy at `presentation.chrome.regions`: Theme defaults completed with the workspace's and site's individual label overrides, within the same region limits. Missing chrome is an empty map. Shared component templates can render resource anatomy with these supplied labels. The native Markdown transport uses the same component formatter, preserving the resource links without appending a second body.

## Authored citation tools

When supplied, top-level `citation` contains optional plain `text`, ordered `formats` (`label`, plain `text`), and optional resolved `bibtex` / `pdf` URLs. Use these URLs rather than the authored source references in `content.citation`. Each present URL has corresponding optional `bibtex_destination` / `pdf_destination` facts in the native `LinkDestination` shape, so shared link markup can classify it without parsing an href. Render text with normal escaping, never `safe`; omit unavailable tools. Presence is independent of headings, Notes, Markdown alternates and article subtype. No citation formatting algorithm or download generation is implied. See the content-model contract.

## Emitted reading images

`images[]` describes actual body image occurrences; `notes.images[]` describes the separately rendered Notes occurrences, including file-referenced notebook Markdown images. Each entry has one-based `occurrence`, exact resolved `destination`, native plain-text `alt` and optional `title`. Hosted entries have `address`, also emitted on the matching `img` as `data-gazette-image`. Addresses are page/namespace occurrence identities, not source paths. Empty alt stays empty. Optional `caption` is the admitted reading text from an explicitly declared [figure caption](#bind-an-image-to-its-authored-caption); no caption or visual claim is inferred. An image may also carry `variants.light` and/or `variants.dark`, resolved through the same source and public/preview addressing as `destination`; `destination` remains the default fallback. Optional positive integer `width` is the authored display width in CSS pixels. Optional `intrinsic` carries independently verified raster display width/height. Optional `svg` carries separate verified viewport geometry. Native HTML reserves width and proportional height before enhancement when that evidence permits it; authored width is marked with `data-gazette-image-sized`. Optional `loading` and `fetch_priority` preserve explicit authored browser delivery hints. A compiled page resource’s `rendition_of` maps the original serving URL to its derived `url`, without private input coordinates. A viewer matches the occurrence address and one of those declared URLs, and enlarges the rendition currently displayed. An opted-in inline SVG has a separate `span[data-gazette-image-ref]` with that same address beside its hidden source `img`; move and restore the complete wrapper to preserve its scoped IDs and live presentation. Do not infer a relationship from sibling position. Notebook cell addresses remain distinct from the primary body and Notes. See [image renditions](markdown.md#declare-image-renditions) for declarations and Theme responsibilities. This projection includes compiled SVG image uses while leaving those resources out of `assets`; static asset inventory is not an image-use or MIME registry.

### Dark image sources and the starting colour mode {#initial-color-mode}

An image occurrence with a declared dark alternative is wrapped in `picture[data-gazette-image-modes]`. One `source[data-gazette-image-mode="dark"]` precedes the unchanged `img`. Its `srcset` holds the dark responsive candidates with the image's `sizes`, or the dark URL alone when the image has no candidates. The `img` keeps the default `src`, `srcset` and `sizes`, so the browser fetches only the rendition for the mode it shows. Images without a dark alternative keep their unwrapped markup.

The source's `media` follows the Theme's `initial_color_mode` manifest value, which names the colour mode shown before the reader chooses one:

| `initial_color_mode` | Dark source `media` | Use it when |
| --- | --- | --- |
| `system` (default) | `(prefers-color-scheme: dark)` | the stylesheet follows the operating system, as with `color-scheme: light dark` |
| `light` | `not all` | the Theme starts light whatever the system preference |
| `dark` | `all` | the Theme starts dark whatever the system preference |

A package without the value inherits it from its nearest declaring dependency; when independent dependencies each declare it, the dependent package must declare its own. Only those three values are admitted. The value describes the Theme's own stylesheet, so declare it beside the rule that pins the starting mode.

A Theme that lets the reader choose a mode keeps each dark source in step with that choice: `media="all"` for dark, `media="not all"` for light, and `(prefers-color-scheme: dark)` for a choice to follow the system. Without a choice, restore the emitted `media`. A Theme that prints in light mode sets `not all` while printing. The `picture` adds no layout box only if the Theme makes it `display: contents`; selectors that expect the image as a direct child also need to match `picture > img`.

## Resolved body-link attributes

When the compiler resolves an authored Markdown link into the publication graph, the Markdown adapter emits the ordinary `href` plus Gazette-owned data attributes. They expose the already-resolved destination to Theme CSS and browser behavior; they are not a second resolver and must not be used to infer publication authority:

| Attribute | Meaning |
| --- | --- |
| `data-gazette-link` | Stable address of this exact link occurrence. |
| `data-gazette-destination-kind` | One of `internal-native` (the page's own site), `cross-site` (another configured site), `external` (outside the configured sites) or `resource` (a compiled asset or download). |
| `data-gazette-destination-content` | Destination content identity, when the target is content. |
| `data-gazette-destination-site` | Destination site identity, when one is resolved. |
| `data-gazette-destination-relationships` | Space-separated typed relationship names attached to this edge. |
| `data-gazette-destination-theme` | Effective Theme identity of the destination site, when applicable. |
| `data-gazette-destination-palette` | Effective destination palette identity, when applicable. |

Optional attributes are absent when the compiler has no corresponding fact. Theme code may classify or style the link from these values, but must not reconstruct a route, content identity, relationship, Theme or palette from URL spelling.

## Redirect destination facts

Alias and mirror contexts add optional `presentation.redirect_destination` in the same native `LinkDestination` shape (`kind`, optional `content`, `site`, `presentation`, and `relationships`). It identifies the already compiled canonical content target relative to the actual serving site. A local alias is `internal-native`; a canonical copy or mirror targeting another publication is `cross-site`. `route.canonical_url` remains the destination URL for the canonical link, refresh and ordinary anchor. These facts do not create another content identity or imply an HTTP redirect. Ordinary page, site and graph contexts omit this field. Theme code must not reconstruct the target from hostnames, paths, page names or mirror labels.

## Optional reading assistance

`presentation.chrome.reading_assistance`, when selected by the composing workspace, contains `label`, `prompt_template`, `selection_limit`, `context_limit` and ordered `providers: [{id, label, endpoint, query_parameter}]`. It is the same validated native chrome object for page and graph rendering; absent configuration supplies no actions. The compiler contains no provider names, activation behavior or prompt execution. See [the workspace contract](workspace-presentation.md#site-chrome-and-control-copy) for exact slot syntax, endpoint validation and Unicode character bounds.

A Theme owns the selection controls and opens an external handoff only after a reader chooses it. It uses the native page title/route and publication name/home coordinates, bounds selected text and nearby context by the configured character limits, substitutes only documented slots, and sets the configured prompt query parameter through a URL encoder. Provider identities, destinations and visible copy come from the selected chrome object. They are not authored components, region-map JSON, schema.org claims, or inferred relationships. The contract neither promises remote prompt-prefill support nor authorizes automatic transmission.

## Emitted discovery destinations

In complete site assemblies, `presentation.discovery` contains the successfully emitted site entrypoints. Each record has `kind` (`sitemap`, `llms`, `web-manifest` or `tdm-policy`), resolved `url` and `media_type`. The sitemap record points to `sitemap.xml`, whether it is a single map or the index of partitioned maps. It does not list individual partitions. A `tdm-policy` record appears when the publication [reserves text and data mining](deployment-output.md#terms-of-use); its `url` is the terms page, so a Theme can emit `<meta name="tdm-reservation" content="1">` and `<meta name="tdm-policy">` with that address.

These records use the selected serving base, including local preview ports and mount prefixes. The `tdm-policy` address is the exception: it always names the production terms page. They are shared by authored pages, aliases, listing continuations, taxonomy, graph and mirror views. Disabled outputs and standalone page renders omit the corresponding records; an empty list is absent. Failed output generation or a later collision fails the entire assembly.

The workspace supplies human labels and footer placement. Themes render the resolved URLs; they do not infer them from publication domains or configuration flags. Emitted feeds retain their separate `presentation.feeds` records and their existing empty-feed policy.

## Machine reading guide

An optional `render.llms = "llms.txt"` declares a captured Theme entry for the publication's generated machine reading guide. The application owns `llms.txt` placement and supplies one bounded `llms` object: `title`, `description`, `coverage`, `include_human_link`, `pages` and `sections`. `pages` is the complete serving-URL-ordered built catalogue for `all-pages`; `sections` holds the curated or navigation reading path, with `title` and `pages` per section. Each page has authored `title`, optional authored `description`, actual Markdown `url`, serving `html_url`, an existing `id` when one exists, and optional generated-view `page_number`/`page_count`, `listing_variant` and `listing_direction`. Themes format this data using the native `markdown_text` and `markdown_destination` encoders; they do not discover pages or summarize bodies. The `.txt` entry does not use HTML suffix escaping. Without an entry the existing bounded native Markdown writer supplies the guide. See [workspace machine outputs](workspace-publications.md#brand-publication-settings) and [the native generation contract](local-rendering.md).

## Ranked recommendations

Recommendations are how a publication's authored structure becomes what it suggests to a reader next: collections, journeys, editorial links and keywords admit and rank each candidate, so suggestions follow the connections the authors declared.

Every native page context includes `recommendations = { algorithm, items }`, independently of authored `related`. `algorithm` is `gazette-related/5`. `items` contains zero to five shallow destinations with `id`, `site`, authored `title`, optional authored `description`, compiled `route`, optional destination `presentation`, native `destination` facts relative to the recommending page, integer `score` and ordered `reasons`. They also carry the same bounded shallow listing metadata: optional decorative `artwork`, authored `language`, `published` and `modified`, direct ordered `attributions`, and exact `classifications`. Missing values stay absent; no page body, structural inventory, publisher-as-author or inferred image is copied. The shared listing builder resolves principal websites and artwork from the existing catalog/assets; native destination kinds remain relative to each source consumer. Templates render the supplied order and use `route.url`; they do not rank, fill missing items, infer addresses or turn suggestions into authored relationships. The same value enters direct-page, discovery, search and shared machine contexts through one page projection. An absent relevant candidate produces an empty list.

### Candidate eligibility

Version 5 considers only positively source-bound, discoverable, canonically routed articles, documentation, project pages, collections and journeys in the compiled corpus. Utility, index and extension pages, self, unrouted records and republished route copies are excluded. A cross-site candidate must meet the same native conditions; there is no same-site bonus. Local previews use their compiled serving addresses. Discoverability is not publication approval, and this algorithm adds no lifecycle, rights or availability claim. A source-unbound inspection has no automatic suggestions for its unbound pages.

Admission requires an eligible editorial/structural graph relationship, an existing authored page link (including notes), or an exact normalized authored keyword phrase. Word overlap in titles, descriptions or keyword fragments cannot admit a candidate, even when several discriminating words agree: those words can have different meanings. Topical metadata only strengthens the ranking of candidates admitted by the other evidence categories. The compiler does not invent editorial links to compensate for missing evidence.

### How recommendations score {#evidence-scores}

Scores add independent evidence categories, once per category unless stated otherwise:

| Evidence | Points | Reason |
| --- | --: | --- |
| Current page explicitly relates to destination | 120 | `outgoing-related` |
| Destination explicitly relates to current page | 100 | `incoming-related` |
| Current page links to the destination | 90 | `outgoing-reference` |
| Destination links to the current page | 70 | `incoming-reference` |
| Shared exact declared journey | 80 | `shared-journey` |
| Shared immediate collection context | 50 | `shared-collection` |
| Shared resolved project assertion | 60 About/About; 35 About/Mentions; 15 Mentions/Mentions | `shared-project` |
| Exact normalized authored keyword phrase | 24 each, at most three | `shared-keywords` |
| Discriminating topical metadata | At most 48 | `topical-metadata` |

Multiple shared projects use the strongest pair, rather than inflating a score with catalogue breadth. A journey contributes only when its declaration and every member are discoverable. Collection context means a direct collection membership or an exact participating journey's containing collection, never a broad recursive ancestor. Hidden containers contribute nothing. Project assertions use the graph's already resolved About/Mentions meaning only when every declaration origin is discoverable; source provenance, author, publisher, project placement and site identity add no points.

### How topical words affect the score {#topical-metadata-normalization-and-bounds}

Topical metadata uses Unicode NFKC normalization and lowercase, splitting at nonalphanumeric characters and retaining tokens of at least three characters. For a record whose explicit prose `language` has primary subtag `en`, the retained English search provider's stop-word vocabulary is removed from these tokens before corpus frequencies, anchors and scores are computed. The compiler uses parity-checked native lexical data; it does not execute the browser provider, apply stemming or inherit `machine.search` profile selection. Missing or other prose languages retain lexical tokens without an English fallback. Exact normalized keyword phrases and editorial/structural evidence remain independent and unchanged. A token's strongest field weight is 3 for title or keywords and 1 for description. For `N` eligible candidates and `df` records containing a shared token, its points are `ceil((source_weight + destination_weight) * (N - df) / N)`; tokens appearing in every candidate contribute nothing. This category requires at least two distinct shared anchors occurring in the title or authored keywords of both records, as well as at least eight points before its 48-point cap. Anchors use the same normalized token vocabulary and `df < N` eligibility as scored tokens; raw phrase intersections or generic description overlap cannot supply an anchor. Descriptions can strengthen the ranking of an admitted, anchored match but cannot manufacture eligibility. This conservative rule may omit related prose without a graph relationship or exact shared keyword phrase. Keyword phrases normalize Unicode, case and whitespace. No body, private notes or external service is consulted.

### Order and limit recommendations {#result-order-and-projection-limits}

Results with positive evidence sort by descending total score, then exact content ID. No irrelevant item is added to reach five. Reasons follow the table order and express ranking evidence, not confidence, truth, an additional editorial edge or a reading sequence. Metadata features are cached once per resolved corpus; route values remain borrowed from its current native route table. Retained endpoints and encoded destination bytes share the existing complete navigation projection ceilings; exhaustion fails the projection rather than silently truncating a resource failure into a successful list.

## Publication-home social card recipe

The optional `resource_requirements` list preserves declared order and rejects duplicate logical resources. Every requirement must resolve to an independently enabled selection; `use = "dependency"` keeps these resources out of ordinary page roots. Only the card context adds these resource projections and their compiled module closure to its existing presentation. No resource identity is inferred from a filename.

A Theme may declare one `render.social_card` recipe for the publication's configured home. An explicit `identity_assets.link_preview` takes precedence and skips generation; it does not exempt an invalid recipe or missing template from package validation. Other pages do not acquire generated cards from this recipe.

```toml
[render.social_card]
resource_requirements = ["stylesheet:social-card", "module:social-card-media"]
template = "social-card.html"
width = 1200
height = 630
media_selector = "[data-social-media]"
media_kind = "image"
```

`template` is package-local and enters the retained template closure. The recipe inherits as a whole from the nearest declaring dependency, with its template qualified in that package's namespace. A dependent declaration replaces all capture settings. Independent providers conflict until a dependent package chooses a recipe; matching filenames do not settle the conflict.

Both dimensions are required integers from 1 through 4096 pixels. `media_selector` is required, contains 1 through 1024 UTF-8 bytes, and has no control characters or surrounding whitespace. The fixed browser helper checks selector syntax, requires one matching element and checks its declared media kind. `media_kind` is exactly `image` or `video`; a video recipe may also declare `video_time_ms` from 0 through 600000. An omitted seek selects the helper's initial-frame behavior. Image recipes reject a seek field.

The application supplies the full resolved home-page context, its presentation and `card.width` and `card.height`. A recipe owns presentation and capture settings, not executable paths, shell arguments, environment variables or helper source files. Native programs are selected separately through the workspace's [social-card renderer binding](workspace.md#social-card-renderer).

For an image selected by the recipe, capture binds its `data-gazette-image` occurrence to the native page image declaration. After the isolated card’s styles load, its resolved CSS `color-scheme` selects the declared light or dark rendition; a mixed or unspecified scheme follows the browser preference. A missing rendition keeps the original destination. The selected declaration’s destinations must all be retained media assets. Capture neither guesses variant filenames nor executes the source page’s scripts.

## Component and module names

Components are declared at the Theme-manifest root, for example `[components.callout]`, and their template paths are package-local. An optional `[components.callout.interaction]` selects a logical browser module, an exact export and the supported mount convention. The export is never guessed:

- it is 1–128 ASCII bytes;
- the first byte is an ASCII letter, `_` or `$`;
- remaining bytes are ASCII letters, digits, `_` or `$`;
- `default` is valid only when written explicitly.

A browser-module recipe may declare at most 512 import aliases. Each alias is a nonempty 1–256-byte ASCII literal, must not start with `.` or `/`, may use only ASCII letters, digits, `@`, `/`, `_`, `-` and `.`, and may not contain an empty, `.` or `..` slash component. Aliases are exact non-relative specifiers mapped by the workspace to logical resource identities; Gazette does not search package managers, the filesystem or the network.

When an enhancement needs an SVG filter chain, construct its SVG nodes inside the [`element` mount slot](markdown.md#component-islands) supplied to the selected browser export. Keep the complete CSS or still-image fallback in the component’s static template. The [emitted-HTML profile](#markup-and-execution-gates) accepts only its limited inert SVG vocabulary; filter definitions and filter primitives are not admitted inline.

### Place container contents within the authored body

Declare a component with `semantic = { kind = "container-contents" }` and a package-local template. Its local name is up to the Theme. A collection or journey may use one empty, top-level occurrence without arguments; the compiler rejects other placements. The template receives `component.container_contents` with `kind` (`collection` or `journey`), `documentation`, ordered `entries` and an optional balanced `outline`. Each entry carries `id`, `site`, `title` and optional `description`, `url`, `presentation` and `artwork`. Presentation contains the selected `theme` and optional `palette`; artwork contains `default` and optional `light` and `dark` renditions. These are the existing resolved child or step facts, including the selected public or preview address. An absent URL remains unlinked.

An outline preserves authored documentation groups over the direct entries. Its events are `{kind: "group-open", title}`, `{kind: "entry", content}` and `{kind: "group-close"}`; an entry refers to the matching identity in `entries`. The payload contains no recursively copied content. Use `component.context.host.site` as the serving site when passing entries to the Theme's existing link or card renderer.

When the slot is present, the direct page model sets `container_contents_placed = true`. Guard the Theme's existing appended container deck with this flag so the deck appears once. The field is absent otherwise, preserving the existing default. Generated navigation is omitted from excerpts, public component-use metadata and search text. Native Markdown places the compiler's existing discovery-filtered reading list at the slot and suppresses its old append; that reading fragment is private and never exposed to the template. HTML keeps direct journey membership, so it can legitimately show explicit steps that the discovery reading list omits.

### Render FAQ text and structured data from one list

Declare an FAQ group and its question items explicitly. The component names are local to the Theme; the `semantic` declaration supplies their meaning:

```toml
[components.faq]
template = "components/faq.html"
semantic = { kind = "faq" }

[components.faq-item]
template = "components/faq-item.html"
semantic = { kind = "faq-item", question_argument = "question" }
arguments = { type = "object", required = ["question"], additionalProperties = false, properties = { question = { type = "string", minLength = 1 } } }
```

Each item takes its question from that argument and its answer from its Markdown body. A landing page can author the list once:

```yaml
- component: faq
  heading: { level: 2, text: Before starting }
  sections:
    - component: faq-item
      arguments: { question: Can we begin with one workflow? }
      body: Yes. Choose one workflow and agree what a useful result would look like.
```

The item template renders the question with ordinary escaping and the admitted `component.body.html` as its answer. The group template retains its complete body. Gazette compares each template's decoded words with those inputs and refuses changed, missing or additional wording. Formatting and whitespace may differ. This check does not establish browser visibility; test the actual disclosure, keyboard and no-script views too.

When JSON-LD is selected, each group contributes a [FAQPage](https://schema.org/FAQPage) under the page's existing `hasPart` entries. Its ordered `Question` entries use the same question arguments; each `acceptedAnswer.text` carries the exact admitted answer HTML, including resolved links and formatting. The surrounding `CollectionPage` or other page type, description, navigation and listing `mainEntity` stay intact. Generated listing summaries do not copy a page's FAQ claims. There is no second authored answer list or promise of search placement.

FAQ items require an enclosing declared group, a nonempty question and an answer body. Nested FAQ groups or items, empty groups and FAQs inside presentation-only payloads fail rather than producing incomplete structured data. Ordinary components without a semantic declaration keep their existing behavior.

### Bind an image to its authored caption

Declare the figure and caption roles in the Theme. Their local names can differ; the semantic roles establish the relationship:

```toml
[components.figure]
template = "components/figure.html"
semantic = { kind = "figure" }

[components.figure-caption]
template = "components/figure-caption.html"
semantic = { kind = "figure-caption" }
```

The figure template renders `<figure>{{ component.body.html | safe }}</figure>`; the caption template renders `<figcaption>{{ component.body.html | safe }}</figcaption>`. Inert wrapper attributes and surrounding whitespace are allowed. Gazette checks the wrapper separately and requires its complete admitted body to remain unchanged, including the image destination, alt, title, caption formatting and links. The existing Markdown, MathML and inline SVG admission rules continue to govern that body; the figure guard does not reinterpret it through the smaller SVG profile used for general Theme HTML.

A presentation-only component can read `component.body.link_destinations` in authored order. These addresses use the same resolved link bindings as HTML. A playback template can serialise the array with `json_script` inside a non-executable JSON block and render the scene’s stills and captions separately.

```markdown
::::figure
![A diagram of the shared interface](./shared-interface.svg)

:::figure-caption
Different models use the same interface while retaining their own state.
:::
::::
```

Each declared figure requires exactly one emitted Markdown image and one nested caption with nonempty reading text. Linked images retain their surrounding link. The caption's admitted plain text becomes that occurrence's optional `images[].caption` and its JSON-LD `ImageObject.caption`; the original Markdown supplies HTML, search text and the Markdown alternate once. Alt text and image titles keep their separate meanings. Ordinary images and following paragraphs do not acquire a caption automatically.

Nested figures, orphan or repeated captions, images inside captions, semantic components inside captions and presentation-only figure content are rejected. A figure can appear inside an FAQ answer without changing the FAQ's question-and-answer relationship. Notebook attachment figures keep their visible caption and reading text, while embedded data URIs remain outside the public image-occurrence metadata.

### Keep playback payloads out of reading outputs

Set `presentation_only = true` on a component definition when its entire body supplies playback data or decoration rather than something to read. For example, a film may carry hundreds of frame resource links in a separate component:

```toml
[components.animation-frames]
template = "components/animation-frames.html"
presentation_only = true
```

Gazette still validates every child, resolves its resources and renders the complete HTML. It omits that component subtree from search text and Markdown alternatives, including notes referenced only from the omitted subtree. The setting defaults to `false`; it does not depend on component names, link labels, CSS visibility or whether JavaScript runs.

> [!IMPORTANT]
>
> Keep the film’s meaningful description, still-image alternatives, captions and actions outside the presentation-only component. Mark only the playback payload; marking the enclosing film would remove its readable account too.

## Resource ceilings

The default resource-compilation profile admits at most 512 declared input entries, 32 path components per entry, 256 KiB per retained file and 4 MiB for the complete declared input tree. A successful emitted artifact is at most 8 MiB. Raster and font recipes have their own additional typed limits. These ceilings bound accepted work; they are not a process sandbox or publication certification.

## What each markup profile permits {#markup-and-execution-gates}

| Markup | What to expect |
| --- | --- |
| Authored Markdown | Its parser and compiler reject raw HTML. |
| Notebook HTML and MathML | Separate profiles accept documented forms under their own limits. |
| Tera templates | The workspace trusts the captured template code. Autoescaping follows the template suffix. Use `safe` only for compiler-produced fragments such as `body_html`. Tera is not a sandbox. |
| Final HTML/XML, CSS, JavaScript and resource links | Separate structural and disclosure checks apply. Resource links must match the allowed inventory. |
| Output prepared for release | Inline event handler attributes fail. Policy checks, technical checks and approval still apply. |

The font profile permits one emitted WOFF2 resource with the documented `@font-face` descriptors. Unsupported font shapes and at-rules fail.

### Style the Markdown table wrapper {#native-table-boundaries}

Native Markdown HTML places each unchanged `table` inside `<div data-gazette-table>`. The marker identifies one table's presentation boundary without assigning a theme, width, scrolling rule, ARIA role or focus stop. Themes may make that boundary locally scrollable while leaving the table, header groups, row groups, cell alignment and inline content native. Markdown alternates retain ordinary Markdown tables. The wrapper exists without JavaScript; conditional overflow hints and keyboard enhancement remain Theme responsibilities.

## Print the Theme and template schemas {#native-schemas}

The schemas are generated from the same Rust types that parse or serialize the native contracts; they are not separately maintained copies:

```sh
gazette schema theme-manifest
gazette schema template-context
gazette schema workspace
gazette schema authored-content
gazette schema legal-source
```

MCP exposes the same owner as `contract_schema`, whose `contract` value is `theme-manifest`, `template-context`, `workspace`, `authored-content` or `legal-source`. Schema discovery performs no native read or mutation. The schemas describe the current Gazette 0.1 contract; they do not create an independent compatibility promise.

The Theme-manifest schema covers the package identity, namespace, template root, optional root render recipes, browser colors and palettes, the initial colour mode, components, exact package dependencies, declared resources, ordered resource requirements and capability needs. The workspace schema covers unresolved declarations before native path acquisition. The template-context schema describes the exact page value sent to Tera, including the shared `presentation` envelope and optional compiler-owned JSON-LD value.

## CLI outcome contract

When a Theme development tool invokes Gazette's CLI for schema inspection or local rendering, interpret the exit status separately from the operation result. The process uses these stable status classes:

- `0`: the selected application operation completed successfully. A deliberately handled broken output pipe may also terminate successfully because the consumer closed a read-only stream.
- `1`: application, encoding or I/O failure. For effectful work, retained evidence may say that an external outcome is unknown; this status never implies safe retry.
- `2`: invocation or argument error reported by the command parser, including an explicitly invalid operation relationship.

Exact effect state and recovery guidance belong to the operation result and retained evidence, not to exit-code guesswork. MCP and VS Code project the same canonical operation metadata rather than maintaining a second lifecycle model.

See [local rendering](local-rendering.md), [page resources](page-resources.md), [Markdown](markdown.md), [browser capabilities](browser-capabilities.md) and [MCP](mcp.md) for the behavior behind each contract.
