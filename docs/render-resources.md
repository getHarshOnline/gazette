# Prepare templates and browser resources

Choose the Theme templates, resources, capabilities and heading data that the renderer uses.

## Declare providers and browser resources {#capabilities-providers-and-selected-site-resources}

Capabilities describe typed publication meaning. Providers supply that meaning. Plugins are captured packages that may export providers and independently declare resources. Resource recipes therefore belong to package resource declarations, not to provider declarations or capability selections:

```toml
package = "search-plugin"

[providers.search]
implements = ["publication.search/v1"]

[resources."module:search"]
inputs = ["dep.ts", "main.ts"]
build = { kind = "browser-module", entry = "main.ts" }
```

Workspace composition binds the two axes independently:

```toml
[composition.plugins]
"search-plugin" = { state = "enabled" }

[composition.capabilities]
"publication.search/v1" = { state = "enabled", provider = { kind = "plugin", package = "search-plugin", provider = "search" }, requirement = "optional", allow_absence = "no-native-acquisition" }

[composition.resources]
"module:search" = { state = "enabled", resource = { owner = { kind = "plugin", package = "search-plugin" }, resource = "module:search" }, output = "/assets/search.js", use = "site" }
```

Plugin activation, capability binding and resource selection are three separate atoms. A provider/resource reference does not activate its package; a package selection does not select any export; and neither capability nor resource proves the other. All three inherit through their own workspace/site maps and retain independent selection origins. Gazette acquires provider results before deterministic compilation, while Tera receives only frozen capability outcomes and compiled logical resources; it never invokes a provider. An output names a file route beneath the configured publication base.

Resource `inputs` are an unordered complete retained set. The recipe entry must belong to that set, and every relative source import must resolve within it. Filenames never select a compiler or establish a capability.

The JavaScript adapter uses SWC to parse, erase TypeScript syntax and bundle one ESM entry from captured `.js`, `.mjs` and `.ts` files. Constant propagation and dead-code elimination run before compact emission. The emitter removes formatting whitespace while preserving runtime semantics and selected legal notices (`/*!`, `@license` and `@preserve`); those notices count toward the complete output limit. It does not rename public exports or remove features merely because a first page load does not use them. Oxc checks input and output grammar and lexical/module early errors without retaining another semantic model. This is **not type checking, browser execution or a security sandbox**. Imports must name exact relative files (`./dep.ts`, or confined `../dep.ts`); there is no extension search, filesystem fallback, package installation or network lookup. Unbound aliases (including `@theme`, `@workspace`, `@site` and `@capabilities`), npm resolution, asset imports, JSX/TSX, CommonJS and `import.meta` are unsupported and fail.

#### Import a library from another source {#dependency-resources-across-selected-sources}

Shared libraries are dependency resources, not automatically loaded capabilities:

```toml
[composition.plugins]
"shared-resources" = { state = "enabled" }

[composition.resources]
"module:shared" = { state = "enabled", resource = { owner = { kind = "plugin", package = "shared-resources" }, resource = "module:shared" }, output = "/assets/modules/shared.js", use = "dependency" }
```

The selected package resource owns its local `inputs` and browser-module recipe. Its package has an exact source binding. Site declarations replace the complete atom, and `{ state = "disabled" }` blocks inherited imports. Theme and plugin packages remain parallel sources; selection does not imply a directory-based permission hierarchy.

An importing site or page resource names only the logical dependency:

```toml
build = { kind = "browser-module", entry = "main.ts", imports = { "@shared" = "module:shared" } }
```

Source `import { value } from '@shared'` is linked through the native JavaScript AST to the selected canonical URL. Each reachable shared module is emitted once; local relative dependencies remain bundled, while cross-source ESM identity is preserved instead of inlining a separate stateful copy into every consumer. Unreachable dependency resources are not emitted or given script tags. A literal `import('@shared')` may use the same declared alias to load a shared module on demand. Computed specifiers, relative or absolute dynamic imports, and import options are rejected. Deferred edges remain in the complete output and verification graph but are excluded from the initial preload closure; a module also reached by a static import remains eager. Missing/disabled bindings, unselected package resources and output collisions fail explicitly. The private render evidence binds module sources, manifests, recipes, dependencies and output bytes. The application also derives one sorted browser-module graph from those exact emitted references. Site JSON receives the complete graph; each page's Theme context receives the standard import map and only that page's eagerly reachable `modulepreload` closure. Literal aliases appear only when they survived compilation, and one site cannot bind the same alias to two module identities. Theme may inline the map and render preload links, but Gazette never turns either into a module execution script, and emits no standalone preload manifest, guessed priorities or wall-clock timestamps. This is not browser or deployment approval.

#### Compile stylesheets from declared files {#captured-stylesheet-compilation}

The styles adapter uses Grass for an explicit `.scss`, `.sass` or `.css` entry and emits its compressed output. Sass loud comments (`/*! … */`) remain in that output; ordinary source comments do not. Compression preserves significant whitespace in strings and calculations, and the same output-byte and reference checks apply. Variables, mixins, nesting, `@use`, `@forward` and supported Sass imports resolve only against captured files. Extensionless and partial-name resolution follows Grass; decoded import literals are first checked with Grass's own AST, not a second Sass grammar. URL syntax, query/fragment/percent-bearing paths, absolute or escaping paths and unsupported explicit suffixes fail instead of being reinterpreted. There are no native load paths, filesystem fallback or network reads. Consumed sources must be valid UTF-8 and valid for the selected grammar.

The initial profile rejects dynamic imports and `sass:meta`, including dynamic `load-css`; supported deterministic built-in modules include color, list, map, math, selector and string. Declaration-only imported fragments such as `color: red;` are unsupported by the selected Grass parser. Nested imports of complete rules remain supported; Gazette does not manufacture wrapper rules.

Lightning CSS validates emitted CSS without error recovery; it is not another transform or minifier. Parsed URL values, including every quoted or `url(...)` candidate in `image-set` and `-webkit-image-set`, enter the existing resource reference set. The application resolves them against the stylesheet's public output address and requires every target in the final site inventory. External, data, fragment and escaping URLs cannot bypass that same-site closure. Gazette does not select a browser-specific subset or fetch anything while compiling. Native gradients, finite nonnegative resolutions and optional `type(...)` hints are preserved; hints do not certify an image format or waive a candidate's existence. Nested sets and unresolved resource functions, including image sets in opaque custom-property values, remain unsupported. Unresolved CSS `@import`, unsupported resource-loading functions, `@namespace`, `@-moz-document` and opaque/unknown at-rules fail. `@font-face` admits the closed retained-WOFF2 and local-only fallback profiles described in [page resources](page-resources.md); other font-face shapes fail. Random/unique-ID functions cannot produce successful nondeterministic output. Ordinary supported CSS, custom properties and gradients remain available. This bounded profile is not complete CSS compatibility, browser execution or security certification.

#### Resource assembly and Theme context

The application checks the compiler's input identity, dependency membership and output bounds before assembly. It compiles once per exact selected package resource. Distinct resources cannot claim one output, even when their bytes happen to match. Generated resources enter the same checked inventory as pages, static assets, aliases and the model artifact; they cannot overwrite any of them.

Every page's `presentation.capabilities` and the site model's `capabilities` map carry the same typed provider outcomes. They contain no resource URL, recipe or compiled-byte identity. The site model's `resources` map preserves every selected compiled resource. Each page's ordered `presentation.theme_resources` contains only the logical resources declared by the effective Theme, joined to the same public facts: logical `id`, `kind`, `url`, literal `output`, compiled-byte `sha256`, byte count, checked references and image geometry when applicable. Interfaces add no compiler. Theme can consume its declared resources without deriving URLs or kinds:

```html
{% for resource in presentation.theme_resources %}
  {% if resource.kind == "browser-module" %}
  <script type="module" src="{{ resource.url }}"></script>
  {% elif resource.kind == "stylesheet" %}
  <link rel="stylesheet" href="{{ resource.url }}">
  {% endif %}
{% endfor %}
```

The Theme requirement does not select a resource, activate a plugin, select a provider or establish ownership. The independently resolved composition must bind and enable every required logical resource before compilation.

Private capability evidence retains the owner-qualified provider, selection origin, failure policy and exact provider outcome. Separate resource evidence retains the package/source identity, selection origin, use, manifest/input identities, recipe, reachable dependency hashes, compiler profile and output hash. These facts do not establish authorship, runtime behavior or publication permission. All declared inputs affect private input identity; an unused prerequisite need not alter the compiled program or public output.

#### Resource compilation limits

Resource compilation has independent retained-input and emitted-program ceilings. `PublicationOutputLimits.resource_metadata_bytes` defaults to 4 MiB and charges capability evidence, site resources and page-local resources together, before per-selection receipt/model fanout. The convenience render operation receives it through `PublicationRenderLimits.output`. Complete-site output limits and template-context limits still apply separately. These are logical budgets, not a bound on SWC's internal AST allocation or CPU time. For styles, Grass returns a complete CSS string before the emitted-byte limit can be checked. Grass's admission/compiler parses and Lightning CSS's validation also allocate intermediate state. Neither that limit nor the captured-source boundary promises bounded intermediate memory, execution time or complete hostile-input panic isolation.

#### Optional capability absence

An optional capability may permit exactly the observed `allow_absence = "no-native-acquisition"` outcome when no admitted provider runtime produced a value. A required capability cannot carry that permission. The shared public outcome is typed `unavailable` and contains no resource URL. Resource selection and compilation remain independent: missing recipes, inputs, outputs, compiler tools, invalid bytes, compiler failures and exceeded budgets fail the resource path and are never relabeled as capability absence. Page-local script/style selection follows the bounded [page-package profile](page-resources.md), including its unambiguous top-level entry selection or explicit resource declarations. Arbitrary discovery outside an owned package, richer notebook capabilities and platform-specific browser scheduling remain unsupported. The separate [static notebook profile](notebooks.md) consumes configured attachments.

### Shared heading model

Every Markdown HTML render uses the retained body's single heading allocation. The page's `headings` array exposes those same source-ordered records: `level`, `text`, `id`, URL-ready `fragment`, original body-byte `span`, location `precision` and `parent` (the nearest preceding lower-level heading's array index, or null). Skipped heading levels do not manufacture hierarchy nodes. Theme may choose a table-of-contents presentation without reparsing text, allocating IDs or encoding fragments again:

```html
<nav>
{% for heading in headings %}
  <a href="{{ heading.fragment }}">{{ heading.text }}</a>
{% endfor %}
</nav>
```

All parser-recognized explicit IDs and emitted footnote/reference IDs reserve names before automatic allocation. Automatic headings use the pinned `slug` library's ASCII transliteration, then source-ordered `-1`, `-2` collision suffixes; an empty slug uses `section`. Gazette promises no other implementation's slug algorithm. Explicit IDs remain exact and case-sensitive; `fragment` encodes them for URL use (`café` → `#caf%C3%A9`). Heading text includes text, inline code and image alt text, with spaces for breaks; markup and footnote-reference labels are excluded.

Duplicate parser-recognized explicit IDs, whitespace/control-bearing IDs and unsupported heading attributes fail. The native heading-attribute grammar supports `#id` and `.class`; arbitrary attributes do not acquire raw-HTML permission. Gazette checks the exact source extent of an attribute block already recognized by the native Markdown adapter and rejects a second ID token at that token's exact byte span; it does not introduce a parallel Markdown or heading-attribute grammar. Footnote labels use injective ASCII aliases, with upstream `fn-` and `fnref-` structural prefixes. Percent and Unicode labels are not rejected merely to retain an old adapter workaround. Gazette derives the emitted footnote closure from retained source structure: root references admit reachable definitions recursively, references in image labels and unreachable definitions remain source evidence without creating backlinks, and emitted occurrences receive deterministic source-order ordinals. Raw HTML remains unsupported.

These IDs are unique within one Markdown body, not across arbitrary Theme markup. Explicit embedded namespaces are checked against the final emitted IDs before reference callbacks; callers still own distinct namespaces across bodies. This is local structural navigation, not certified public navigation or publication permission. Anchor allocation is lazy, cached and bounded independently from HTML and inspection output; a failure returns no partial heading index or page.

The retained presentation manifest names a template directory and package namespace. For example, a root package with namespace `root` can expose `page.html` as the qualified virtual entry `root/page.html`:

```html
<html>
  <head><link rel="canonical" href="{{ route.canonical_url }}"></head>
  <body><main data-id="{{ content.id }}">{{ body_html | safe }}</main></body>
</html>
```

`body_html` already includes the authored heading. See the [page-template example](theme-reference.md#build-a-page-template) for that layout.

A root Theme resolves browser chrome colors either directly for its unpaletted selection or on each named palette. Both values are parsed as complete CSS color values during manifest admission; template literals, site/world names, `currentcolor` and unresolved custom properties are not substitutes:

```toml
browser_theme_colors = { light = "#ffffff", dark = "oklch(20% 0.02 250)" }

palettes = [
  { id = "palette:night", browser_theme_colors = { light = "#f8fafc", dark = "#0f172a" } },
]
```

### Choose a template for one page {#per-page-template-selection}

Framed authored input can select an exact entry in the retained virtual Theme namespace, independently of its content family:

```toml
+++
template = 'root/guides/article.html'
[content]
# The complete ordinary content declaration follows.
+++
```

Here `root` is the selected Theme package's declared namespace. Place `template` at the metadata root, not inside `[content]`. The effective page entry is selected in this order:

1. The composing site's effective `page_templates` binding for that exact content identity.
2. The retained source's `template` value, when present.
3. The selected root Theme's `render.page` entry.

The site inherits the workspace template map when its own map is omitted. An explicit site map replaces the complete workspace map; an empty map clears it. Use [per-content presentation selection](workspace-presentation.md#per-content-presentation-selection) to configure these bindings without changing the source body.

Plain native Markdown has no source `template` value, but a composition binding for its exact derived content identity still takes precedence over `render.page`. Gazette does not infer templates from families, filenames or collections. Generated listing continuations reuse their owning index's effective entry.

Configured entries are checked against the retained namespace before template compilation. Escaping source selectors fail structural parsing; a missing effective source entry fails rendering without a partial site tree or stage. The Theme's optional `render.alias` recipe remains separate: an alias requested without that entry fails instead of using the page recipe.

The compiler retains the selector and its source coordinates; private render evidence records the actual entry. It is presentation selection, not another content identity or JSON-LD fact. Candidate exclusion removes its selector along with the excluded page. This selection grants no Theme or publication approval.

Here `body_html` comes from Gazette's bounded Markdown renderer, not arbitrary raw input. Theme remains responsible for context-appropriate presentation. The Theme-owned alias entry receives the same model, including the canonical target. Alias and mirror contexts expose the existing target as `presentation.redirect_destination`, with native identity, site, destination presentation and link kind relative to the actual serving site. The existing `route.canonical_url` remains the URL; themes need no URL parsing to classify the ordinary anchor. Gazette does not invent redirect markup or claim a template implements an HTTP redirect. Alias-output validity beyond structural/UTF-8 checks still needs Theme and certification evidence.

[Back to local rendering](local-rendering.md).

The native CLI exposes the per-file resource input ceiling as `--max-resource-file-bytes` on `render-sites`, its preview commands and `materialize-sites`. The equivalent MCP publication input is `max_resource_file_bytes`, including operations that embed that input. Both default to 256 KiB. Increasing this field leaves the resource aggregate input ceiling, decoder pixel and encoded-byte limits, and emitted-output ceilings unchanged. Set it to the largest input the selected publication needs; an explicit 4 MiB ceiling admits images within the default raster encoded-byte profile.
