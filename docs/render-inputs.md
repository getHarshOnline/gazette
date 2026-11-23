# Prepare inputs for the renderer

Bind authored content, Markdown and static files so the renderer reads the files you intend it to use.

## Declare content and project files {#inputs-and-one-source-of-truth}

Use the [workspace format](workspace.md), with a selected Theme and explicitly pinned sources. The brand publication selects its authored input:

```toml
# These tables extend a complete workspace declaring site:one, its publisher,
# source:content and a selected Theme source.
[[brands]]
id = "brand:one"
name = "Example Publication"
[[brands.publications]]
site = "site:one"
public_base_url = "https://example.test/mount/"
authored = [{ source = "source:content", target = "authored/one" }]
```

The logical corpus prefix distinguishes source ownership. It is not a public URL or a second repository path. Every configured authored source and enabled project documentation source is captured so cross-site references resolve against the actual complete declaration. Only the selected site's pages are emitted. Each object's explicit home must agree with its configured source owner; a cross-site relationship does not relocate it.

Destination-bearing page, navigation, search and graph records also carry the home site's optional `presentation` identity: its already-resolved Theme package and optional palette. A crossing therefore has enough information to present a different publication world without guessing from its domain or copying the foreign object. Absence stays absent when that site selected no Theme. This projection is deliberately narrower than the site's full composition: it does not expose capability bindings, provider or plugin state, logical-resource selection, selection origin, package capture readiness or publication approval.

Project-native authored documentation uses the existing placement binding:

```toml
# project:library and source:library-docs must be explicitly declared.
[[placements]]
role = 'primary'
site = "site:one"
project = "project:library"
render = true
discoverable = false
documents = { source = "source:library-docs", target = "native/library" }
```

These retained documents enter the same corpus, graph, route compiler and Theme as publication-authored content. The current strict frontmatter profile below applies to framed documents in either source role. A separate explicit native Markdown profile can supply structural defaults for unframed project documents, as described below. Neither path invents collections, authors or direct about/mentions from a project's placement. `render = false` excludes that placement from capture and output; it is not a failed optional copy. Discovery selection does not suppress this private draft-inclusive rendering or grant public admission. Generated API references and their brand-owned `documentation_repository` remain a separate documentation-target path.

The shared page model includes `provenance.site` and, for project-source input, `provenance.project`. These identify the configured source relationship, not semantic subject matter. Native source keys, repository paths, pins and file paths are not included in that template/JSON projection; exact acquisition and per-page source evidence retain their private association.

### Choose what appears in discovery {#discovery-without-weakening-materialization}

The compiler resolves `discoverable` once from each exact content source binding. Publication-authored input is discoverable; rendered project documentation uses its placement's literal flag. Project lifecycle, visibility, availability and about/mentions are not substitutes for that setting. An unbound or wrong-site source fails this emitted selection instead of inheriting a default.

All selected pages, aliases, resources and static assets still materialize. Missing required input remains a failure even when its placement is undiscoverable. The flag instead controls automatic page listings, search, feeds, sitemap, site JSON-LD, graph endpoints and generated navigation. The site asset catalog uses the same source flag without dropping the actual asset files. A site may therefore render pages while having an empty automatic discovery set. Curated `llms.txt` references must also be discoverable; an ineligible configured reference fails rather than silently disappearing. Markdown alternates instead follow canonical materialization, including undiscoverable rendered pages.

Direct pages retain their own authored declaration, journey steps and body links. Discoverability is not privacy: explicit prose may still link to an unlisted page. Generated breadcrumbs or journey recommendations with ineligible endpoints are omitted whole, never spliced into a different ancestry or neighbor sequence. Aggregate page/graph views borrow eligible scalar metadata instead of expanding raw container-member lists. Eligible journey edges keep original positions; project assertions whose evidence includes an ineligible endpoint are omitted whole, not rewritten as a reduced assertion. Independently referenced projects remain valid project facts, even if one documentation placement is undiscoverable.

Private compiler inspection still exposes the complete canonical structure. The emitted views do not mutate that graph, reparse bodies, grant public admission or certify artifacts. Their bytes and the configured flag enter the existing output inventory and exact policy-check input identity.

Explicit [content attributions](content-model.md#direct-attribution-not-identity-invention) bind author and rights-holder keys against the same workspace principal catalog. The ordered `attributions` view supplies the exact declared roles to Theme and local JSON. It does not include an unrelated global author list, infer a byline from `site.publisher`, or convert a declaration into identity/rights approval.

Templates can embed supplied structured values in HTML data blocks using the adapter's [JSON script encoding](tera.md#json-script-data), for example `<script type="application/json">{{ content | json_script }}</script>`. The same page metadata remains the source; the filter does not reconstruct it. The resulting data remains local and unadmitted. Selected [JSON-LD](json-ld.md) comes from the compiler's borrowed page projection; the filter only encodes that supplied value for script-element text.

An authored Markdown file in the selected capture can contain:

```toml
+++
[content]
id = "article:introduction"
site = "site:one"
title = "Keep the observation with its context"
description = "Record what happened while the details that explain it are still available."
lifecycle = "draft"
route = { path = "/introduction/", aliases = ["/start/"] }
[content.structure]
kind = "page"
family = { kind = "article" }
+++
# Keep the observation with its context

Write what you noticed before deciding what it means. Include the place,
conditions and any detail that could change your interpretation when you return
to the note.

Keep the original measurement, sketch or quotation beside the explanation
instead of replacing it with a confident conclusion.
```

The fences above are literal `+++` lines in a Markdown file. See the [content model](content-model.md) for collections, contained journeys, typed project relations, keywords and article-owned notes, and [page resources](page-resources.md) for bundle companions and their explicit compilation limits. These structural and rendering contracts do not establish public admission or publisher approval.

### Generated taxonomy navigation

The brand publication can select index and term pages using exact catalog keys, routes and retained Theme templates. These are generated navigation, not new authored objects or a second taxonomy catalog:

```toml
# Append to the selected brands.publications table. The catalog must already
# declare taxonomy:topic and term:systems.
[[brands.publications.taxonomy_pages]]
taxonomy = "taxonomy:topic"
route = "/topics/"
template = "root/taxonomy.html"

[[brands.publications.taxonomy_pages]]
taxonomy = "taxonomy:topic"
term = "term:systems"
route = "/topics/systems/"
template = "root/taxonomy.html"
```

An index groups only classified, discoverable pages from that site's selected corpus. A term page selects that exact term; an explicitly selected empty term page remains empty. Foreign pages and undiscoverable sources are not pulled into the listing. Labels never become identity joins or inferred slugs. A term link exists only when its route is explicitly configured.

The template receives `taxonomy`, optional `term`, `title`, `canonical_url`, `output`, `groups` and the shared typed `presentation` envelope. Each group carries its exact `term`, optional `canonical_url` and member `pages` with `id`, `title`, `canonical_url` and optional `language`. The same value enters `taxonomy_pages` in the site model. Selected JSON-LD adds the compiler's `CollectionPage` view to this value and `site.jsonld`; the sitemap uses the same configured listing URLs. Selected Markdown alternates also cover these generated views. They do not automatically enter curated `llms.txt`, whose entries name selected content identities. These pages do not manufacture content lifecycle, authorship, feed entries or publisher permission. Their paths share the ordinary output collision checks. Template and output digests remain private render evidence.

The presentation envelope has the same shape used by page, alias, taxonomy, graph and mirror rendering. Its closed `surface.kind` is respectively `page`, `alias`, `taxonomy`, `graph` or `mirror`; Themes need not infer this from payload fields or template names. The envelope also carries the effective Theme and palette, that selection's CSS-validated light/dark `browser_theme_colors`, typed workspace chrome, frozen capability outcomes, compiled Theme resources, the site resource/module inventory and applicable interaction/search bindings. Search's optional `no_script_destination` is the already-resolved page and canonical route, not renderer-authored HTML. Values remain contextual: authored pages retain their exact page resource closure, while generated site surfaces receive the stable site-level inventory. This presentation transport does not activate a plugin, select a provider or resource, change inheritance origin, or add facts to the taxonomy payload.

### Publish plain project Markdown {#plain-project-native-markdown}

Add this field to the owning `[[placements]]` table when its pinned documentation source contains ordinary Markdown without publication frontmatter:

```toml
native_markdown = { route_prefix = "/docs/library/", lifecycle = "published" }
```

Both fields are required. `route_prefix` is an explicit directory route, ending in `/`, beneath the site's configured public base. `lifecycle` selects a content state (`draft`, `published` or `archived`); even `published` here grants no public admission or release authority. The profile requires a `documents` binding and does not enable a disabled placement. Publication-authored inputs do not use it.

The compiler keeps native bytes and their parsed body unchanged and derives a documentation node:

- Home site and project provenance come from the placement. No author, subject, collection, journey or rights claim is added.
- Identity is `native-document:<sha256>` over a versioned coordinate containing the project key, source key and literal capture-relative entry. Body/title edits, changed commit or checkout location, site changes, mounts and logical prefixes do not change it. Renaming the native entry or changing either key does. Mirrors and identity migration are not implemented by this coordinate rule.
- Title is the first nonempty H1's parsed text, otherwise the literal filename stem. `derivation.title` records `kind = "heading"` with the original heading byte span, or `kind = "filename"`. This is derived metadata, not an authored field.
- `README.md` and `index.md` map to their parent directory beneath the prefix; other Markdown files map to a directory named by their stem. Matching these two index stems and Markdown extensions is ASCII case-insensitive. With the example prefix, `nested/README.md` becomes `/docs/library/nested/` and `nested/guide.md` becomes `/docs/library/nested/guide/`.

Native path components are encoded through the shared route codec. Spaces and Unicode are preserved; unsupported literal percent signs, query/fragment markers and unsafe paths fail rather than changing identity. `README.md` and `index.md` in the same directory conflict: the compiler never silently chooses one.

Framed TOML documents retain their explicit IDs, metadata and routes; the native profile never overwrites them or rescues malformed metadata. An unsupported YAML or ambiguous `---` opening fails, preventing a draft/privacy declaration from being discarded as body text. Without the profile, unframed documents still fail structural lowering.

### Source-relative documentation links

Within project-bound documents, actual Markdown `.md`/`.markdown` links resolve relative to the referring native entry, inside the same configured source, to the target object's compiled canonical URL. This works between plain and framed documents, preserves query suffixes and does not rewrite inline/fenced code examples. It uses the retained AST, not regular-expression replacement or a second source read.

Missing targets, traversal beyond the selected source and invalid encoded paths fail explicitly. Fragments such as `guide.md#section` resolve against the retained target's exact heading/footnote index. Same-document `#section` references work in both authored and native documents; native `./#section` uses that same index and emits a suffix-only `#section` so file-shaped routes do not navigate to a different parent page. The same normalization preserves a supplied query suffix. Malformed percent escapes, invalid UTF-8 and missing anchors fail before output. Fragment lookup decodes once while emitted links retain the original encoded suffix and query spelling. Empty fragments remain valid; no implicit `top` heading is invented. External and root-relative URLs retain their spelling; this is not proof of destination validity. Static references use the shared source coordinate described below. Other code-file URLs and repository-platform URL generation remain separate work. Publication-authored document links do not adopt the native Markdown-to-route grammar.

### Static assets from configured sources

Each authored or project-document `ContentBinding` can explicitly select a static destination alongside its existing source and logical target:

```toml
authored = [{ source = "source:articles", target = "articles", static_assets = { route_prefix = "/media/articles/" } }]
```

The same optional `static_assets` field is valid inside `placements.documents`. Its route must end in `/`. Every supported opaque file in that configured source is transported beneath this prefix with its source-relative filename, including unreferenced files. No page slug, output route, template or adapter invents a second destination. The site's configured public base supplies the domain and mount prefix; the output path remains site-relative.

The current transport suffixes are `png`, `jpg`, `jpeg`, `gif`, `webp`, `avif`, `pdf`, `bib`, `txt`, `csv`, `zip`, `mp4`, `webm` and `vtt`, case-insensitively. This selects byte-preserving local transport, **not MIME/payload validation, image transformation, rights, disclosure approval or safe public hosting**. SVG, HTML, scripts, styles, notebooks and other unsupported inputs remain explicit uncompiled obligations. Video transport preserves the original clip without transcoding. WebVTT captions are transported unchanged and served as `text/vtt; charset=utf-8`; this does not validate caption timing or content. Preview serves MP4 as `video/mp4` and WebM as `video/webm`, with bounded single-byte-range support. This does not decode the video or certify container metadata: strict output qualification still refuses these video files until an explicit supported video inspection profile exists.

Selecting a capability still requires its real compiler; placing its source beside a static image does not bypass that requirement.

Actual Markdown images and links to supported static suffixes resolve relative to the referring authored or native document, confined to its exact source. They must identify an entry in the selected site's retained asset plan. Missing or uncompiled targets, invalid percent encoding, encoded traversal/separators, empty components and escaping `..` fail before output. Literal Unicode and spaces produce encoded URLs and literal output filenames using the existing route codec. Literal `%`, `?` and `#` in source filenames remain unsupported. Queries and fragments are preserved, but anchors inside opaque assets are not validated. Code examples are never rewritten. Links/images nested inside image alt text remain label text, not emitted references or page asset dependencies; syntax inspection still records them and the original safety gate still applies. External and root-relative destinations are neither fetched nor checked by this resolver. Email autolinks retain their `mailto:` destination; an address ending in a supported asset suffix is not a source-relative download.

One cached `AssetPlan` per site supplies the Markdown mapping and output writer. The page's `assets` array contains its referenced records, deduplicated in URL order; the site model's `assets` object contains all planned records keyed by URL. Each record carries `url`, literal `output`, `sha256`, byte count and site/project `provenance`. Tera and JSON receive the same records. Logical input paths and source digests are retained separately in private asset evidence. Native source is not reopened, and wall-clock or remote observations do not enter asset identity.

Assets use the same output inventory, collision checks and file/directory/byte budgets as pages, aliases and the model artifact. A failed plan or collision returns no complete site. Per-site plan record/text ceilings reuse the configured content limits independently of the content graph; foreign assets do not consume that site's plan budget. Unbound opaque files remain unresolved obligations, not an inferred source or silently omitted file.

[Back to local rendering](local-rendering.md).
