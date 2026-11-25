# Configure navigation and discovery

Set which content appears in reading indexes and what the site emits for search engines and other readers.

Start with a [taxonomy catalog](#taxonomy-catalog-and-language-scope) if you want to list pages by topic. Use [generated reading indexes](#generated-reading-indexes) for site navigation, [machine-readable outputs](#machine-readable-publication-outputs) for discovery files and [preferred source invitations](#preferred-source-invitations) for an optional reader action.

## Taxonomy catalog and language scope

Append explicit catalog declarations to the workspace example:

```toml
[[taxonomies]]
id = "taxonomy:subjects"
label = "Subjects"
language = "EN-us"
terms = [
  { id = "term:systems", label = "Systems" },
  { id = "term:publishing", label = "Publishing" },
]
```

Taxonomy and term records reject unknown fields. IDs are exact keys; labels must be nonblank and control-free, and never generate identities or slugs. Taxonomy IDs cannot collide with principal, site, project or brand identities. A term's identity is the pair `(taxonomy, term)`: the same term key may occur in another taxonomy, but repeated taxonomy IDs or term IDs within one taxonomy fail. Omitted catalogs and term lists remain empty. Resolution sorts catalogs and terms by ID; `Workspace::taxonomies`, `taxonomy` and `Taxonomy::term` share those validated records without label or sibling-taxonomy fallback.

`language` uses the shared BCP 47 syntax validator and normalizes subtag casing (`EN-us` becomes `en-US`). It is a **content-language constraint**, not merely the label's language: content assigned to this taxonomy must explicitly resolve to the exact same normalized tag. Missing content language, `en` or `en-GB` does not match `en-US`. An omitted taxonomy language is neutral and accepts content with any or no language; it supplies no default. Malformed tags fail, and the application bounds each tag to 255 input bytes. This does not perform language detection, registry alias canonicalization or translation matching.

Content declares exact membership pairs as described in the [content model](content-model.md#language-and-explicit-classifications). All content states are checked; unknown taxonomies/terms and mismatched language return errors, never dropped classifications or a successful partial graph. This does not establish taxonomy hierarchy, term routes, indexes, filters or publication admission.

The same retained workspace AST supplies taxonomy origins, for example:

```sh
gazette check --stdin --origin '{"kind":"taxonomy","identity":"taxonomy:subjects","field":["terms",0,"label"]}' < workspace.toml
```

That coordinate addresses the original `Systems` label even after ID sorting; the language origin retains the original `EN-us` bytes. Omitted fields have no fabricated span.

## Generate reading indexes {#generated-reading-indexes}

A publication can bind `listings` to an existing index page with an empty Markdown body. The index keeps its authored identity, title, site, route and selected template; the listing supplies its entries from the existing content graph.

Selection considers every captured Content binding and applies the discovery rules shared by the other outputs. It does not create another collection or a second maintained catalogue.

`brands.publications.search_page` may select one existing, same-site, canonically routed Page independently of the shared chrome atom. Its resolved destination is available at `presentation.search.page` when search is configured, and at `presentation.chrome.shell.search.page` alongside the shared search-control copy. The legacy chrome `no_script_destination` remains an independent fallback; it does not opt a publication into the full-page search role. Missing, foreign-site, unrouted or republished search pages fail binding rather than inventing `/search/`.

For a complete browse fallback, bind that declaration-only index to `select = { kind = "all-pages" }`. This selects the publication's discoverable, canonically routed Page, Collection and Journey records from the same captured graph. It excludes the index itself, foreign-site records and republished copies; assets and generated continuation routes never become content records. The configured search-page identity is also omitted from the emitted search corpus, so it does not appear as its own result. The same listing ordering, shallow metadata, pagination and independent resource bounds apply. It does not invent editorial containment or add search to article browsing locations. Search ranking may use only the publication's configured fields; the empty-query browse index is a complete discovery view rather than ranked results. The Theme chooses its search layout by the resolved destination's content identity, while keeping the selected world presentation. No route or site-name convention is part of the engine contract.

```toml
[[brands.publications.listings]]
content = "content:blog"
select = { kind = "standalone-articles" }
order = { by = "published", direction = "descending", undated = "last-title-then-id" }
pagination = { page_size = 15, next_pages = "page/{number}/" }
alternate_order = { order = { by = "published", direction = "ascending", undated = "last-title-then-id" }, path = "oldest" }

[[brands.publications.listings]]
content = "content:collections"
select = { kind = "root-collections" }
order = { first = ["collection:foundations"], remaining = "title-then-id" }
pagination = { page_size = 20, next_pages = "page/{number}/" }
```

A product's related writing uses a separate explicit selector, not an editorial container or a copied article directory:

```toml
[[brands.publications.listings]]
content = "content:example-mentions"
select = { kind = "project-articles", project = "project:example", scope = "ecosystem", residence = "all", explore = "content:blog" }
order = { by = "published", direction = "descending", undated = "last-title-then-id" }
alternate_order = { order = { by = "published", direction = "ascending", undated = "last-title-then-id" }, path = "oldest" }
pagination = { page_size = 15, next_pages = "page/{number}/" }
```

The landing must be a canonical project-family page or root collection with a direct `about` assertion for that exact project.

`scope` defaults to `site`; `ecosystem` considers canonical eligible articles across the captured publications, including collection and journey articles. An article qualifies through its own direct `about` or `mentions` declaration, or an actually emitted resolved body/Notes link to the landing. Unused definitions, images, inherited container context, labels, folders and coincidental URL text do not qualify. Candidate discovery applies before reference expansion.

All references share the native complete-view edge/text bounds; exhaustion fails rather than returning a shorter list. The subject view retains each article's own canonical site and route, and does not become its global browsing ancestry.

Optional `residence` defaults to `all`, preserving existing selections; `standalone` keeps articles with no declared editorial parent and `collected` keeps articles with a collection or journey parent. It never uses directory paths.

Optional `explore` names an existing configured unfiltered index in the view's own publication. It resolves to a shallow native destination even when the related set is empty; foreign, missing, non-index or already project-filtered targets fail instead of becoming a guessed fallback.

`standalone-articles` selects article-family pages with no editorial parent; hiding a parent from discovery never makes its articles standalone. `root-collections` selects editorial collections with no parent collection. Article selectors, including `project-articles`, exclude pages classified as documentation through their collection purpose or documentation family. The [content purpose contract](content-model.md#product-documentation-collections) describes inheritance and the independent search/all-page behavior. Republished copies are excluded. Priority identities must name unique same-site candidates of the selected kind, even when a candidate is not discoverable. Remaining candidates sort by literal authored title and stable identity.

Publication ordering uses only the authored `published` value, with explicit `descending` (newest first) or `ascending` (oldest first) direction. Each complete eligible corpus is sorted before slicing; changing direction never reverses just the current page. Neither `modified`, filesystem dates nor content lifecycle supplies chronology. Local inspection remains draft-inclusive; an explicitly narrowed candidate/discovery selection governs its own outputs.

A reading index can mix calendar dates and timestamps. It orders calendar days in the requested direction, using the authored day for a date-only value and the UTC day for a timestamp. Within each day, timestamps come first and compare their actual instants in that direction; entries with an unknown time follow them in both directions. This is a display convention, not a claim that an unknown time was midnight or later than the known times. Equal instants and equal date-only values use title then identity; entries without a date come last, also by title then identity. Original date precision and offsets stay intact. For example, `2026-10-06T00:30:00+14:00` belongs to the UTC day `2026-10-05`, alongside the equal instant `2026-10-04T20:30:00-14:00`. The optional `latest` item uses that same editorial-day convention: it considers only the newest day, selects the latest timestamp or uses title then identity for equal dates, and remains absent if that day mixes date-only values and timestamps. Older mixed precision cannot suppress a clear newest-day choice. This identifies an editorial-day winner without inventing times or timezones for calendar dates; it does not establish an absolute instant for them.

Pagination requires a positive page size and a confined relative directory with exactly one `{number}` placeholder. An index at `/blog/` with `page/{number}/` produces continuations such as `/blog/page/2/`. The public or preview base supplies their serving URLs; capture paths do not. Empty selections retain one empty index view. Collisions with content routes, aliases, mirrors, taxonomy outputs or file ancestors fail the complete build. Whole-projection page, link and text ceilings apply; Gazette never truncates a successful listing to fit.

An optional `alternate_order` admits only the opposite `published` direction over the same candidates and pagination. Its confined relative `path` is joined beneath the original index: `oldest` gives `/blog/oldest/` and `/blog/oldest/page/2/`. The original index and continuation URLs remain unchanged. Every generated view has the same authored index identity, with a native `variant` discriminator; alternate page one is a generated output, not another owner or article. Both ordering roots and every continuation share the same collision and aggregate expansion checks. Article browsing locations retain the primary ordering.

The Theme receives `listing` with ordered `items`, counts and optional previous/next URLs. Native `pages` contains first/last/current plus at most two neighbors on each side, at most seven distinct endpoints, with `current` and `gap_before` facts. Optional `sorts` links each available chronology to its own page one, resetting pagination. Rendering these same facts twice supplies consistent top and bottom controls without client-side sorting or template route construction. Continuations reuse the index's template and identity, replace only the serving route and slice, and do not become additional articles or search records. The site model's output-keyed `listing_pages`, site graph, sitemap and JSON-LD carry the applicable generated views. See [the Theme contract](theme-reference.md) for the context fields.

## Machine-readable publication outputs

`machine` selects site-root `sitemap.xml`, `llms.txt`, `search.json` and `site.jsonld`, plus canonical-page Markdown counterparts beneath the configured public or local-preview base. Omission disables each output. `sitemap` and `json_ld` are booleans; search is omitted, a concise `lexical` or `english` profile, or a strict `{ language, fields }` selection. For example:

```toml
# Inside the existing publication's machine table.
search = { language = "english", fields = ["title", "description", "keywords", "route", "body", "notes"] }
graph = { enabled = true, path = "/graph/" }
```

Search `language` selects the token-processing profile, not the page language. `fields` must be a nonempty distinct list drawn from `title`, `description`, `keywords`, `route`, `body`, `notes`, `summary` and `projects`. Concise profiles select `title`, `body`, `summary`, `projects` and `keywords`. Each emitted search record retains its complete discovery-safe metadata alongside the selected plain ranking values in `indexed`; selecting notes does not create another search identity or merge commentary into the main body.

Set `reuse_body = true` on an explicit selection to emit the selected ranking body as a reference to the retained text. Enable it with a consumer that supports the [body reference](render-outputs.md#search-documents).

The optional graph table selects a view of the existing discovery graph through the selected Theme's declared graph entry, not a second graph or a template selector. `/graph/` maps to `graph/index.html` using the ordinary route compiler. `enabled = false` emits no graph view and refuses an invocation that requests one. Enabled configuration fixes the destination; a different invocation output is refused. Omission leaves the explicit invocation-level graph selection available. Missing Theme entries, unsafe routes and output collisions fail the site rather than silently dropping its view.

`markdown_alternates` and `llms` are strict named tables with required fields, not booleans or positional arrays. Unknown fields, unsupported profile values and wrong types fail parsing. These are publication representations, not another repository/path convention or a generated-reference destination.

The alternate profile's supported values are exactly `directory_page = 'index.md'`, `file_page = 'append-md'`, `media_type = 'text/markdown'` and `discovery_rel = 'alternate'`. With `enabled = true`, `/guide/` materializes `guide/index.md`, while `/guide.html` materializes `guide.html.md`; each URL uses the site's resolved base. This covers rendered canonical pages, generated taxonomy views and listing continuations, independently of discoverability. Aliases, mirror redirects and noncanonical syndicated copies do not receive alternates. `enabled = false` omits their files and the optional `presentation.markdown_alternate` Theme binding.

Enabled `llms` requires enabled alternates. Its `description_from` names a selected, same-site, discoverable canonical page with an explicit nonempty `description`, used as the guide's introduction. `representation` is exactly `markdown-alternate`; `include_human_link = true` pairs each Markdown link with its actual serving HTML link, while `false` omits that pair.

`coverage = 'all-pages'` selects every eligible built canonical page from this publication's actual render selection. Each entry uses the existing authored title and nonempty description. Listing continuations and alternate orderings are omitted from the guide; their HTML and Markdown files remain available through the canonical index. Newly authored pages therefore enter the guide without a second catalogue. Aliases, mirror redirects, noncanonical syndicated copies, undiscoverable content and identity-free companions do not become duplicate entries. Configured taxonomy views also enter the catalogue with their exact labels and routes; their catalogue has no authored purpose field, so no summary is invented. The complete model and emitted guide are bounded; a missing page purpose or exhausted bound fails rather than silently shrinking coverage.

The default `coverage = 'curated'` preserves the existing explicit reading path: `sections[].contents` supplies exact identities and section/item order, including intentional repetition. All-pages mode requires `sections` to be absent or empty; a stale curated catalogue cannot quietly override complete coverage. Omission or `enabled = false` produces no `llms.txt`. The selected Theme may provide an inherited `render.llms` entry to format the native model; without one the native bounded Markdown writer supplies the neutral guide. Its text encoder preserves ordinary prose punctuation and escapes characters that could become Markdown formatting, links or HTML.

The example assumes `content:home` exists with that declared description. Pure workspace checking validates the schema, not captured content eligibility. During publication rendering, missing, excluded, undiscoverable, foreign, mirror-only or noncanonical curated references refuse the site rather than silently shrinking its guide. A missing description refuses it too. The same resolved routed pages feed HTML, alternates and curated links; no second catalogue or live template read is involved. Selection is not public admission: current local renders include drafts and archived content. See [discovery outputs](render-outputs.md#site-discovery-outputs) for scope, format limits and explicit failure behavior. Search selects token processing and ranking fields, not a browser library, query algorithm or cross-site fetch. See [search documents](render-outputs.md#search-documents). `identity_assets` assigns semantic roles to site-used resources already enabled by the site's effective composition. A dependency-only module cannot fill an identity role. The role never selects or names a file by convention. Site icons may use admitted PNG, JPEG or inert SVG resources; link previews use PNG or JPEG. Theme receives the resolved resource records through `presentation.identity_assets`. JSON-LD also supplies the page's `json_ld` template/model field from the same resolved facts. See its [projection and ownership contract](json-ld.md).

## Preferred source invitations

This optional publication setting supplies a link for a reader to choose the publication as a preferred Google source. Gazette derives `https://www.google.com/preferences/source?q=<canonical-host>` from the publication's canonical domain or subdomain root, independently of local serving URLs. It selects a source host, not an article or a `/blog/` subtree. The invitation loads no provider SDK and contacts Google only when the reader follows the link.

```toml
[brands.publications.processing.preferred_sources]
enabled = true
provider = "google"
articles = true
label = "Prefer this source on Google"
description = "Find more writing from this publication."

[brands.publications.processing.preferred_sources.pages."article:one"]
enabled = false
```

Omission or publication-level `enabled = false` emits no invitation, even if a page override enables it. `articles = true` includes the article family regardless of collection nesting. Exact `pages` overrides take precedence over that article default and optionally replace `label` and `description`. Utility and index pages need an explicit opt-in. Overrides must name real, routed content in that publication; missing or foreign references fail resolution.

Labels and descriptions are nonempty plain text without control characters, bounded to 160 and 1,024 UTF-8 bytes respectively. A publication can declare at most 1,024 page overrides. Copy and article keywords are not instructions sent to Google. The canonical source must be a public domain root without a port; IPs and local hosts are refused. A page projects `preferred_source` with `href`, `label` and `description` through the existing page and discovery views. Templates render that supplied action and its explanatory copy without another JavaScript configuration, domain list or selection rule.

The [compiler integration tests](../crates/gazette-compiler/tests/preferred_sources.rs) exercise page selection and exclusion, invalid copy and source scope, unresolved page references, and canonical targeting during local preview. The [host tests](../crates/gazette-compiler/tests/unit/preferred_sources.rs) cover domain and subdomain targets and rejected preview or article addresses. A theme check should also verify placement, one accessible link with its description, external-link attributes, no provider script and omission from unselected pages.

Google handles the account flow, source eligibility, confirmation and cancellation. A local render cannot establish live eligibility, and a click cannot establish that a reader added the source. Before release, check eligibility through Google's own tool and review the exact rendered invitation. Existing structured data, discovery, analytics and consent settings retain their separate contracts; the invitation reports no preference completion or conversion.

[Back to workspace](workspace.md).
