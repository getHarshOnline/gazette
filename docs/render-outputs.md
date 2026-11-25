# Read the generated site files

Find the graph, discovery files, Markdown copies and search records produced from the shared page model and configured documentation.

## Optional local site graph {#optional-local-site-graph}

Page context exposes optional `content.language` and exact `content.taxonomies` references. Its `classifications` array joins only those references to configured taxonomy and term labels; it is not the workspace's entire catalog. The same array appears at `graph.classifications[content_id]` when the graph is selected. See the [shared classification contract](content-model.md) for the declaration shape and language-scope checks. A template may use `content.language` for an HTML `lang` attribute when present, without inventing a value when it is absent. Labels remain ordinary escaped text; a classification does not create a route, subject relationship, discovery eligibility or publishing permission.

The same retained content graph can supply an explicit Theme view and the site JSON model. Add both flags to the existing local render command:

```sh
gazette render-site --local --site site:one \
  --model-output machine/model.json \
  --graph-output graph/index.html < workspace.toml
```

There is no template/path discovery: selecting graph output requires the exact `render.graph` entry in the selected Theme manifest. With neither an enabled publication graph configuration nor an explicit graph output, no graph view or `graph` field enters site JSON. Rust callers select the same path through `SiteRenderOptions.graph` and `GraphRenderOptions`, which owns output placement and graph limits. The CLI supplies the default graph limits; whole-output options remain separate.

`ResolvedAuthored::project_site_graph` remains complete private structural inspection. The application's `project_discovery_graph` borrows the same content, routes and workspace catalogs to produce `gazette-discovery-site-graph/1`. The application encodes one bounded graph value, flattens it beside the shared typed `presentation` envelope for the graph template, then stores that same presented value under `graph` in `gazette-local-site-model/1`. Neither consumer reparses content, repeats inheritance or queries mutable state. The graph's `presentation.publication` is the same optional resolved public metadata as an ordinary page's `site.metadata`: the configured resolved rights-holder principal (independent from publisher and page attribution), optional supplied render year, brand, declared home, language, contacts and public profiles. The graph's top-level `site` remains its identifier. No foreign metadata or private publication configuration is added. In complete site assemblies, `presentation.graph.url` supplies the exact emitted graph destination to pages and generated views, including the serving base and mount. Disabled or absent graph outputs and standalone page renders omit it. Configured paths preserve their route form; explicit output files retain their exact file address. A failed assembly returns no successful destination or partial tree.

For example, `graph.html` can render local objects directly:

```html
<ul>
{% for id, node in nodes %}
  <li data-id="{{ id }}" data-state="{{ node.lifecycle }}">{{ node.title }}</li>
{% endfor %}
</ul>
```

The projection contains eligible selected-site content without inferring eligibility from content state, existing eligible local routes, incident collection memberships and container-parent edges, outgoing local editorial relationships and incident journey steps. The sole-residence and same-site containment rules still apply. Journey `position` retains its original zero-based ordinal, not a renumbered filtered sequence. Typed project links reuse resolved about/mentions and declaration evidence. Original local project declarations and site/project source provenance remain separate relations. Only projects referenced by local content, the site's publisher and directly attributed principals enter their catalogs; a source placement creates no subject or author.

Foreign content appears only in `references`: exact `id`, borrowed `title` label, home `site` and the existing `canonical_url` when one was declared. The title names the referenced destination; it does not copy a foreign page, body, lifecycle, profile or unrelated edge into the selected site. Discovery-ineligible endpoints do not acquire reference objects or title labels, including through a local canonical-origin reference. Explicit canonical declarations on eligible local records and their resolved routes retain their native meaning; this is not a metadata-redaction API. An absent foreign route remains absent, not an inferred URL; a Theme may show its exact title as plain text. Native paths, source bindings and capture locations stay out of this graph value. These scoping rules apply to this graph projection, not a retroactive public-admission claim for the local page inspector.

The graph remains draft-inclusive. Private/unlisted project axes and archived/draft content remain inspectable; neither `discoverable`, publisher identity nor rendering success grants disclosure or release permission. Local graph, search, JSON-LD, [feeds](feeds.md), sitemap and `llms.txt` output do not establish public-policy admission or public language/taxonomy-aware discovery and search semantics.

Limits cover complete node/catalog/reference counts, relationship counts and encoded graph bytes. Exhaustion fails rather than truncating a successful graph. Template-context, graph-view output and aggregate site limits apply independently. The graph output shares the existing collision-checked inventory with pages, aliases, assets, capabilities and model JSON. Missing templates, invalid output or any collision returns no successful site. Private graph evidence binds the selected template/output and the shared-model and rendered-byte digests; it grants no publication authority.

## Generated output reference

### Site discovery outputs

To configure discovery for your publication and inspect what readers receive, return to [Search, feeds and discovery](https://rawthoughts.in/gazette/help-readers-find-the-work/).

Select discovery files in the existing brand/publication settings:

```toml
[[brands.publications]]
site = "site:one"
public_base_url = "https://example.test/mount/"

[brands.publications.machine]
sitemap = true

[brands.publications.machine.markdown_alternates]
enabled = true
directory_page = "index.md"
file_page = "append-md"
media_type = "text/markdown"
discovery_rel = "alternate"

[brands.publications.machine.llms]
enabled = true
description_from = "article:introduction"
representation = "markdown-alternate"
include_human_link = true
sections = [
  { title = "Start here", contents = ["article:introduction"] },
]
```

These fields belong to a complete workspace declaration, not an independent command input. `render-site --local`, the Rust SDK and exact-set `render-sites --local` honor the same selections. Omission disables each output. The `llms` and `markdown_alternates` fields are strict named tables, not booleans or positional arrays. Each table requires every field shown above; unsupported values and unknown fields fail schema checking. `enabled = false` disables that table's output while preserving its explicit selection. Enabled `llms` requires enabled Markdown alternates. Conventional `sitemap.xml` and `llms.txt` paths are relative to the site's output root; no hostname, repository name or filesystem path is inferred from the brand name.

The same resolved page and route model supplies HTML, Markdown alternates, sitemap and curated reading links. Local rendering retains all content states; these selections do not confer public admission. Collections and journeys are actual routed objects, not guessed directories. No automatically generated author catalogue, contact directory, timestamp, remote observation or independent registry query supplies these files.

The sitemap uses the [Sitemap protocol](https://www.sitemaps.org/protocol.html): UTF-8 XML, the sitemap namespace, escaped canonical URLs, no invented `lastmod`. When the complete selected set fits, `sitemap.xml` retains its ordinary `urlset` representation. Otherwise the same path becomes a `sitemapindex`, with root-level `sitemap-00001.xml`, `sitemap-00002.xml` and subsequent files in canonical-URL order. Index addresses use the configured publication base, including its mount prefix; aliases, mirrors and foreign pages do not become extra discovery records.

The optional `machine.sitemap_urls_per_file` tunes packaging only: an integer from 1 through 50,000, requiring `sitemap = true`; omission uses 50,000. Files also split at the smaller of the configured per-file ceiling and 52,428,800 uncompressed bytes, measuring actual escaped XML and complete framing. The index independently obeys those byte and 50,000-child limits. A single unfit URL/entry or index fails; there is no truncation, second index layer or implicit compression. URLs must remain shorter than 2,048 characters.

Aggregate byte/entry exhaustion is a complete-output failure, not a new partition threshold. Every generated member enters the existing collision-checked site inventory as XML. Occupied root/index/child paths, an exhausted budget or any other site failure produce no successful partial tree or silently smaller site set. Empty discovery still has one empty `urlset`. These local mechanics are not crawler submission, public admission, deployment or release evidence.

#### Publish Markdown counterparts {#markdown-counterparts-are-materialized-not-guessed}

An enabled `markdown_alternates` profile emits one `.md` counterpart for each rendered canonical page, plus generated taxonomy views and listing continuation pages. A directory route `/introduction/` maps to `/introduction/index.md`; a file route `/guide.html` maps to `/guide.html.md`. The configured public or local-preview base supplies the domain and deployment prefix. Literal output paths remain relative to the site tree. Aliases, mirror redirects and noncanonical syndicated copies receive no counterpart.

The page representation comes from its resolved projection and retained native Markdown plan, not a copy of frontmatter, stripped Theme HTML or flattened search text. It carries the serving HTML link and the names of explicitly attributed authors and rights holders. Internal identity keys stay in machine metadata. Resolved body links, headings, footnotes and article-owned commentary reuse the existing owners. `notes.md` stays a subordinate, labelled part of its article representation, without a second content identity or public route. Generated lists use the same ordered items and pagination as their HTML views. Collection counterparts also expose their eligible direct children, and journey counterparts retain the complete declared step order. Articles carry eligible reading paths and immediate journey neighbors. These references borrow the resolved serving addresses and discovery selection; they do not derive routes from source folders. A journey with undiscoverable members does not become a shortened public sequence, and unavailable paths do not invent a standalone home. The native adapter lowers admitted component output to readable headings, paragraphs, links, lists, tables, code and TeX math. It retains visible component arguments and notes, while omitting layout classes, presentation programs and explicitly decorative components. Allocated anchors remain minimal HTML anchors. Notebook cells retain their order, execution counts and literal outputs. A construct without a complete Markdown representation, such as a merged-cell table or an independent SVG drawing, retains bounded inert HTML rather than losing meaning; this does not grant unrestricted raw-HTML authoring. The separate exact HTML transport capability remains available to consumers that need tree fidelity. Unsupported selected representation or exhausted bounds fails instead of dropping the construct.

Theme receives `presentation.markdown_alternate` on the exact canonical surface: `html_url`, `route`, `url`, literal `output`, `media_type` and `discovery_rel`. Its UI is a Theme choice, for example:

```html
{% if presentation.markdown_alternate is defined %}
  <a rel="{{ presentation.markdown_alternate.discovery_rel }}"
     type="{{ presentation.markdown_alternate.media_type }}"
     href="{{ presentation.markdown_alternate.url }}">Read Markdown</a>
{% endif %}
```

Disabled alternates omit both this binding and the files. Alias and mirror contexts do not borrow the canonical page's alternate binding. A page-only projection describes a planned address; complete-site materialization proves that its bytes exist. Counterparts enter the same collision-checked inventory with the producer media type `text/markdown`, including limits and read-back.

#### `llms.txt` uses built pages or an explicit reading guide {#llms-txt-uses-built-pages-or-an-explicit-reading-guide}

The [llms.txt proposal](https://llmstxt.org/) supplies the Markdown structure, not a requirement to repeat the sitemap. The H1 comes from the configured brand name. `description_from` names a selected, same-site, discoverable canonical content page whose explicit, nonempty `description` supplies the opening blockquote. That page is not automatically an entry.

With `coverage = 'all-pages'`, the exact built-page selection replaces a manually maintained catalogue. Canonical discoverable pages use their authored descriptions. Listing continuations and alternate orderings are omitted from this guide; their HTML and Markdown files remain reachable through the canonical index. Aliases, mirrors, noncanonical copies and identity-free companions do not create duplicate entries. Configured taxonomy views retain their labels and routes without a manufactured purpose. Missing required page descriptions and complete byte exhaustion refuse the build.

A selected Theme's optional `render.llms` formats the native `llms` model, inherited through its explicit package dependencies. Independent competing inherited entries require an explicit selection. Theme receives `title`, single-line authored `description`, `coverage`, `include_human_link`, flat URL-ordered `pages` and curated `sections`. Page entries contain `id` when one exists, `title`, optional authored `description`, reading `url` and serving `html_url`. Authored pages use their Markdown counterpart as the reading address; provider documentation uses its actual reading endpoint. When both addresses are the same, the writer emits one link. Built-page entries have no continuation numbers or alternate-order fields. Sections contain `title` and the same `pages` shape. The native Markdown encoders preserve text and link boundaries. No template catalogue or body-summary generation is involved. Without a Theme entry, the bounded native writer remains available.

In the default `coverage = 'curated'` mode, Each `sections` item supplies a literal heading and exact `contents` identities. Section and content order are preserved, including intentional repetition. No page-kind groups, author list, taxonomy catalogue, ranking or unconfigured content is added. Titles, headings and descriptions retain ordinary prose punctuation. Characters that could introduce Markdown formatting, links or HTML are escaped so the supplied words remain literal text. With `include_human_link = true`, an entry pairs the actual Markdown counterpart with its serving HTML address:

```markdown
- [Introduction](https://example.test/mount/introduction/index.md) ([HTML](https://example.test/mount/introduction/))
```

Setting that field to `false` emits only the Markdown link. The writer resolves every reference against the exact selected graph: missing, excluded, undiscoverable, foreign, mirror-only or noncanonical syndicated entries fail with `site_llms_reference`; a description source without a nonempty description fails with `site_llms_description_required`. An invalid section heading or complete byte exhaustion also fails. No reference is silently omitted to produce a smaller successful guide. A pure workspace check validates the table schema; content eligibility and materialized output require the connected publication path.

These files share the existing collision-checked inventory, per-file and aggregate limits, exact digest calculation and private staging path. A required writer or collision failure produces no successful partial site. In an exact site set the failed site remains explicit; another site's output is not silently substituted. These are **local, draft-inclusive, unadmitted artifacts**. Public discovery must later use the common admission result, not a separate filter inside either writer.

### Search documents

`machine = { search = 'lexical' }` on the brand/publication binding emits `search.json`. The alternative `english` profile is an explicit consumer selection; no language tag, domain or Theme implies it. Both profiles receive the same neutral corpus. The strict alternative `{ language = 'english', fields = ['title', 'notes'] }` selects explicit ranking fields using the [workspace contract](workspace.md). Concise profiles select `title`, `body`, `summary`, `projects` and `keywords`; neither profile implicitly indexes notes or substitutes an inferred description. Its JSON array has one complete record per discoverable canonical page, in canonical URL order. Redirect aliases and foreign linked objects do not become extra search documents. A related foreign object may remain exact relationship metadata inside a local record. Language is an explicit field; omission does not select English.

The compiler's `PageData` is the single typed metadata owner. Private inspection uses its borrowed `SearchDocument`; emitted search uses the shared discovery-safe listing metadata plus `body`: retained Markdown text, including code examples, image alt text, decoded entities and block boundaries. It does not reparse rendered HTML, scan templates, strip code examples, truncate text, infer descriptions or copy private source coordinates. The existing Markdown renderer still rejects raw HTML and unsafe or unresolved references before a page succeeds. Text is data, never safe HTML; browser consumers must use safe text insertion.

The page's Theme/JSON model includes `body_text` from that same AST text owner. Search metadata retains exact content states, canonical routes, classifications, role-scoped attributions, collection/journey ordering, editorial relations and project relationships with declaration origins and separate state axes. It contains no main-page `body_html` or presentation program; subordinate notes retain their existing rendered/text fields. The corresponding page model and search result metadata can be compared field-for-field without another identity or relationship resolver. Emitted records also carry a separate `indexed` map of the selected plain ranking values, borrowed from that same retained page. Unselected values remain result metadata where applicable; they are not silently deleted to match the ranking selection. Selected notes use their subordinate text without adding another content identity. Missing optional text produces an empty ranking value, not an inferred description or body. Rust page renders expose the optional metadata record through `RenderedPage::search()`; configured site emission adds the shared selected ranking projection.

When selected, `presentation.search` exposes `profile`, `fields`, `route`, `output` and `url` to the page's Theme and JSON model, plus the optional resolved `no_script_destination` selected by chrome copy. The complete-site emitter uses the same artifact-path owner. `output` is relative to the generated site tree; `url` uses the effective publication base, including its mount and any explicit local-preview override. Disabled search omits this binding. Selected browser capabilities can consume the supplied URL without guessing a root-relative filename or embedding a second index. A page-only render describes the planned address; only a successful complete-site render proves the record file was generated.

Per-document context limits, a complete-dataset byte limit, and the shared output inventory's file/count/aggregate limits all apply. A collision or limit failure returns no successful partial site and creates no stage. Disabled search emits no record file. Like the other current local outputs, this includes drafts and is not a public-admission result.

With `reuse_body = true`, the selected `indexed.body` is the JSON literal `true`. Consumers resolve it to the unchanged top-level `body` for ranking. Without this opt-in, the ranking field continues to contain a string where selected.

This is implementation-neutral search data, not an executable `window.searchIndex` payload or a prebuilt ranking index. A consumer can search `content.title`, `body`, or explicit project/classification fields. Navigate to `route.url`, which uses the selected serving address, and retain `route.canonical_url` for public identity. The [browser search example](../sdk/js/docs/browser.md#search-the-emitted-corpus) shows how to use those records. Search UI, ranking, query expansion, cross-site acquisition and their permission/performance contracts remain Theme/workspace work; this operation neither installs nor selects a provider.

### Documentation reading pages

A configured documentation build supplies a separate catalog of its actual reading pages. Local rendering includes pages belonging to public projects in search and `coverage = 'all-pages'` reading guides when the documentation repository has a public base URL. Private and unlisted projects do not enter these outputs. Curated reading guides keep their explicit selection.

A documentation search record uses its canonical provider URL as `content.id` and `route.canonical_url`. Its `route.url` is the reading address, including an explicitly selected local documentation preview address. `documentation_page` retains the module, route, optional provider title, label source and whether body text is available. A missing title uses the literal route as its display label; missing body text stays empty. The project reference identifies the configured owner. Gazette does not invent an authored content key, lifecycle, author, editorial relationship or Markdown counterpart for these pages. Complete catalog, search and reading-guide limits still apply; an incomplete provider catalog fails instead of producing a smaller successful result.

Imported references need no generator permission. For configured generated references, select an existing private staging parent as well as execution permission:

```sh
gazette render-site --local --site site:one \
  --documentation-generation-parent /absolute/private/stages \
  --allow-execute < workspace.toml
```

The staging parent must be separate from every captured source and configuration root. `--output-parent`, when selected, retains the website independently. Successful generation adds the original documentation receipts to the render report. A later render or staging failure retains those receipts; a lost output pipe returns failure and writes the same evidence to stderr. Inspect those records before retrying. Omitting the generation parent does not run configured tools merely because `--allow-execute` is present. See [documentation generators](documentation-generators.md) for configured commands and provider formats.

For `render-sites`, `preview-sites` and `production-preview`, `--max-total-source-entries` selects the complete source entry ceiling, which defaults to 10,000. Documentation generation also checks the combined staged inputs against that ceiling: two jobs using the same captured source each need their own staged copy. A refusal reports observed and maximum counts before staging. If the selected inputs legitimately exceed the default, pass an explicit higher ceiling, such as `--max-total-source-entries 16384`; source byte limits and execution permissions still apply separately.

### Which files and reports use the page model {#shared-model-and-native-evidence}

One typed projection per selected object supplies its primary Theme rendering and borrowed discovery listing in the site JSON artifact (`gazette-local-site-model/1`). The direct page retains its own authored structure and steps; listings omit raw container endpoint expansion and ineligible records. Generated navigation shares the same discovery predicates in both roles. Structural content records carry optional `resolved_route` alongside their unchanged authored `route`; templates consume the compiled serving URL without reconstructing paths. Generated listing items carry an optional literal authored `description`, never a body-derived excerpt. Primary/alias output paths come from the shared route compiler, including public-base mount prefixes. The local JSON format is not a signature ABI or a substitute for feeds or admitted discovery/navigation. Selected [JSON-LD](json-ld.md) is projected from the same typed page metadata; the listing role is retained as `pages[id].json_ld` and as the identical site JSON-LD entry. Direct-page JSON-LD retains explicit declarations while using safe generated navigation. Search documents and sitemap/`llms.txt` share its resolved page/route inputs rather than reverse-engineering this JSON. Its optional local graph uses the same source-bound discovery selection. Private compiler inspection remains available without altering the canonical graph.

Selected [Atom, RSS and JSON feeds](feeds.md) borrow the original typed page projections retained by this same render pass. They do not decode the site JSON to reconstruct dates, links or relationships. The complete site inventory owns their paths, byte limits and collision checks; feed failure cannot yield a successful partial site or authorize publication.

Private evidence stays outside the generated tree. It records input origins, configuration and Theme identities, chosen template entries, page-model and HTML digests, static-asset origins keyed by output path and capability compilation receipts keyed by capability. The output inventory covers every emitted byte, including assets, compiled browser modules/stylesheets, aliases and the JSON artifact. No timestamp, stage directory or mutable remote observation is inserted into the output. Public disclosure still requires separate validation.

## Home share cards

A Theme can declare a `render.social_card` recipe for the publication's selected Home. The card uses that Home's rendered body, its first H1 text (or the existing page title when no H1 is present), and the one media element selected by the recipe. An explicitly configured `identity_assets.link_preview` takes precedence. A declared recipe still has to resolve its template and resource requirements when capture is skipped.

The recipe's `resource_requirements` names ordinary, independently enabled resources. Select card-only modules and styles with `use = "dependency"`; they are supplied to the card context without adding them to the normal page's scripts, styles or preload roots. The normal world resources and the same module import map remain available. See [Theme recipes](theme-reference.md) and [workspace renderer bindings](workspace.md) for those declarations.

Browser execution requires explicit invocation permission and pinned Node and browser executables. The engine supplies the capture helper; the workspace cannot replace its script, arguments or environment. The helper loads retained resources, keeps the source body inert, inserts only the selected media, waits for the Theme's completion event, and returns a PNG. A video recipe supplies its media kind and optional seek time explicitly. Treatment declared on the selected media or its nearest source ancestor is preserved; no replacement artwork is generated.

Pure Rust render APIs do not start a browser. Use `render_publications_with_generations` or `check_and_render_publications_with_generations` with explicit `SocialCardGenerationOptions` when native capture is intended. The checked path runs candidate checks once before compiling the Theme and capturing the card; capture failure never repeats those checks. Local render and preview requests accept `social_card_generation` for bounded overrides, while `--allow-execute` grants the generation permission separately from workspace renderer selection. Preview capture uses the URLs of its already held listeners.

```json
{
  "social_card_generation": {
    "allow_execute": true,
    "timeout_ms": 30000,
    "cards": 6
  }
}
```

This is a local-render request fragment. Preparation and qualification requests retain their complete-object decoding rules. Capture limits separately bound each request, aggregate requests and assets, complete responses, and decoded PNG pixels. The output joins the ordinary site inventory and its collision and disclosure checks; a browser result alone does not establish publication readiness. Native process and capture evidence stays in private render inspection, outside the generated site.

[Back to local rendering](local-rendering.md).
