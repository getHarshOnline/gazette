# Write Theme templates with Tera {#captured-template-rendering}

Write the HTML templates for a Theme package. Gazette uses Tera 1.20.1 and passes each template the resolved title, body, routes, navigation and presentation settings. Read those values from the template context so that links and page metadata agree with search, feeds and the rest of the site.

## Render a page

Set the Theme manifest's `render.page` to your entry file, for example `page.html`. That path is relative to the Theme's declared template directory. A minimal page template is:

```html
<!doctype html>
<html>
  <head>
    <meta charset="utf-8">
    <title>{{ content.title }}</title>
    <link rel="canonical" href="{{ route.canonical_url }}">
  </head>
  <body>
    <main>{{ body_html | safe }}</main>
  </body>
</html>
```

`content.title` and `route.canonical_url` receive HTML escaping. `body_html` is the compiler's rendered Markdown fragment; `safe` lets that fragment remain markup. Render it once. Do not apply `safe` to arbitrary authored strings.

Use [the Theme contract](theme-reference.md#build-a-page-template) for the complete manifest and context fields. Its [virtual namespace](theme-reference.md#virtual-tera-namespace) describes how to address inherited templates, includes and macros across Theme packages. The [local rendering guide](local-rendering.md) connects that Theme to a publication.

## Escape data for each output format {#escaping-and-output-boundaries}

Tera's default `.html`, `.htm` and `.xml` suffix-based escaping is preserved. Other suffixes are not automatically escaped. The trusted template's `safe` filter deliberately bypasses escaping; already-rendered Markdown must only receive that treatment through the admitted presentation contract. This is not an HTML sanitizer, a URL-policy validator or JavaScript/CSS context-aware encoding. Machine formats require their own typed serializers rather than assuming HTML escaping makes arbitrary JSON or scripts safe.

### JSON script data

Use `json_script` on a supplied JSON value when embedding it as the text of an HTML JSON data block:

```html
<script type="application/json">{{ content | json_script }}</script>
```

It serializes the value once using `serde_json`, with JSON Unicode escapes for `<`, `>`, `&`, apostrophes and the Unicode line/paragraph separators. Keys and nested values receive the same treatment. JSON decoding recovers the original value, including quotes, backslashes, control characters and Unicode. It does not HTML-entity-encode the data, accept a pre-serialized string as raw JSON, or replace only one spelling of a closing script tag. Tera treats the encoded result as safe for this position; no additional `safe` filter is needed.

This is **script-element text encoding only**, not an attribute, URL, CSS or arbitrary JavaScript-program encoder. It creates neither the element nor its attributes. It takes no arguments and cannot fetch data or change its meaning. Use a separately resolved JSON-LD value with `application/ld+json`; changing a data block's MIME type does not make ordinary publication JSON into JSON-LD. The compiler still owns vocabulary, identity and relationship projection.

The expanded JSON buffer is bounded by the configured rendered-output byte ceiling before returning it to Tera. The complete rendered page is independently subject to that same ceiling, including markup and repeated blocks. Overflow is a failed render, not truncated JSON or a successful partial artifact. This does not make Tera's other filters or arbitrary template expressions allocation-bounded.

The encoding addresses the HTML script-state restrictions described in [JSON-LD 1.1 §7.2](https://www.w3.org/TR/json-ld11/#restrictions-for-contents-of-json-ld-script-elements).

### CSS URLs in style-element text

Use `{{ resolved_url | css_url }}` where a trusted template needs one complete quoted CSS `url("...")` token in a style element's raw text. The value must be a string; no arguments are accepted. The encoder escapes CSS quotes, backslashes, control characters and HTML raw-text delimiters with terminated CSS hexadecimal escapes. CSS decoding recovers the supplied string, including existing percent escapes, query separators and Unicode. NUL refuses because CSS replaces both its literal and escaped forms instead of preserving it.

The encoded result bypasses HTML autoescaping only for this position. Do not add `safe`, wrap it in another `url()`, or use it in HTML attributes, scripts, selectors or arbitrary CSS expressions. The filter does not infer its context, validate URL schemes, select resources, build paths or fetch anything. Native resolved resource/asset URLs remain the source of addresses; geometry and preference rules remain the Theme's responsibility.

Intermediate encoding uses the same configured output-byte ceiling as `json_script`, including the wrapper and escape expansion. Hidden assigned results cannot evade that ceiling, and the complete output independently charges repeated calls and markup. A refusal returns no successful partial artifact. The adapter remains a trusted-template renderer, not a context-aware sanitizer.

### Query values

Use `url_query` for one value inside a query string. Keep the destination and parameter name explicit:

```html
<a href="/search/?q={{ query | url_query }}">Search</a>
```

For `query = "notes & figures"`, the value becomes `notes%20%26%20figures`.

`url_query` accepts one supplied string and no arguments, percent-encoding its UTF-8 bytes as one RFC 3986 query value. Only ASCII letters, digits and `-._~` remain literal; spaces are `%20`, never query separators. The filter does not construct, validate or resolve a destination URL, and it does not disable HTML escaping. Expansion shares the adapter output-byte ceiling; non-string inputs and over-limit output fail without a prefix.

### Metadata in Markdown documents

Use `markdown_text` for literal titles and purposes in Theme-owned Markdown text, and `markdown_destination` inside an inline link destination:

```tera
- [{{ page.title | markdown_text }}]({{ page.url | markdown_destination }}): {{ page.description | markdown_text }}
```

Both accept strings only and no arguments. The text encoder collapses whitespace to a single line and escapes Markdown delimiters so metadata cannot create headings, links, HTML or additional entries. Commas, sentence periods, apostrophes, parentheses and hyphens within words stay readable: `science-fiction, for example.` remains unchanged. Leading list markers and entity-looking text remain literal. The destination encoder escapes link delimiters and percent-encodes whitespace, angle brackets, quotes and backslashes while preserving existing percent escapes. Control-bearing destinations refuse. These are explicit Markdown encoders, not HTML/script/CSS encoders or URL resolvers. Use them in a non-HTML template such as `llms.txt`; they do not bypass HTML suffix autoescaping. Intermediate expansion and whole rendered output share the existing byte ceiling, including hidden assigned results.

### Editorial dates

Use `{{ content.published | editorial_date(format="%-d %B %Y") }}` to show a supplied date as `19 November 2026`. Keep the original value in the `<time datetime="...">` attribute. The optional `format` string uses Chrono's strftime directives and defaults to `%-d %B %Y`; the workspace can supply another presentation format through Theme copy.

The filter accepts Gazette's calendar-date or explicit-offset timestamp strings. It keeps the authored day and offset, including timestamps near midnight, and never consults the clock or converts to a local timezone. Formatting a date-only value with time or offset fields fails rather than inventing them. Invalid dates, unsupported formats, non-string inputs and unknown arguments also fail. Output retains normal HTML escaping and shares the rendered-output byte ceiling, including a result assigned to a hidden variable. It does not choose a publication label or change lifecycle.

## Missing and null values

The pinned Tera `default` filter treats an explicitly supplied JSON `null` and an absent value alike: both resolve to the filter's declared fallback. A Theme that needs to distinguish those states must consume a separate typed presence field; it must not rely on `default` to retain explicit `null`.

## Adapter and constraints

This section is for maintainers extending the template adapter. Template authors also need to observe these execution restrictions.

`gazette-adapter-tera` is a presentation adapter, not a second publication compiler. Its `Compiler` implements the inward `TemplateCompiler` port over a verified, borrowed `TreeInput`; the existing `TemplateSet::compile` convenience entry accepts a `CapturedSource`. Both use the same compilation implementation. The input contains only selected template files and their directory entries. Every regular file is parsed; no suffix filter, filesystem glob, Theme discovery or SQL query occurs. The set retains the input tree identity. Its `render` operation takes an exact `EntryPath` and a serialized object supplied by the application, returning complete UTF-8 bytes or an error. It does not write files.

The application owns source selection, typed semantic-to-context projection and admission. Theme owns trusted presentation programs. The adapter owns Tera parsing, inheritance, includes, macros, escaping and bounded output. It never guesses an author, publisher, route, capability, collection or journey. Context data cannot register a callback or expose the filesystem.

Tera is pinned to 1.20.1. Its dependency manifest declares Rust 1.85, compatible with Gazette's 1.94 toolchain. Default features, date-locale and preserve-order extensions are not enabled by this adapter, and moving to Tera 2 is a separate decision. The dependency is MIT-licensed. Gazette exposes no template loading or callback surface of its own.

Ambient `get_env`, clock/random functions and allocating `range` are unavailable, including if another workspace member later unifies Tera features. Calls fail with native adapter errors rather than substitute values. The application should supply bounded collections instead. Caller-provided callbacks are not exposed. The adapter's encoding filters and `editorial_date` formatter operate only on supplied values; any other helper a Theme needs arrives as explicit context data, never as a callback.

Limits cover the template inventory, each file, aggregate source bytes, path depth, serialized context bytes and emitted bytes. The separate `execution_depth` defaults to 64 and admits only values from 1 to 64. It counts template calls and nested AST containers/expressions together, rather than borrowing source path depth or allowing two independently deep call and expression chains. The output writer stops before exceeding its ceiling; a failed render discards its accumulated output.

### How Gazette checks template execution {#state-sensitive-execution-admission}

Before rendering, the adapter checks the native parsed AST for recursive inheritance, macro loading and reachable include/macro/block execution. The iterative loader preserves Tera's import order, cache publication before parent loading and inherited namespaces. Execution admission retains the original entry, active template and block lookup owner; an include selects only its first existing fallback. Uncalled macro definitions remain inert. Literal Boolean branches retain their native reachability; other branches and possible returned block-stack states are conservatively joined. A possible cycle or excessive combined depth is refused, so admission can reject a program whose safe execution depends on correlations it does not prove. Native inheritance and macro-file checks remain active over the same parsed templates.

Bodies proved free of template calls, includes and block operations share one unchanged exact depth graph across incoming loader-cache and block-top states. The analysis restores each caller's incoming state when that body returns. This avoids copying a pure attribute helper's AST analysis for every conditional include combination; bodies with any possible template effect retain the full state-sensitive path. The proof is conservative and memoized by retained AST identity. Fixed storage/work ceilings, complete longest-path depth and cycle checks remain unchanged.

The v6 admission profile reuses effect-free helper closures across loader states that differ only by empty namespace entries. The complete nonempty namespace maps and active block lookup owner remain part of the analysis, and transitive purity requires every possible same-named definition to avoid loader/block effects. Recursive closures are not purity proofs. Discarded equivalent loader-cache clones release their temporary storage charge; cumulative work, execution-depth, recursion and whole-output limits are unchanged. This keeps ordinary shared helpers composable without admitting a wider set of stateful or recursive execution paths.

Native macro/function/filter argument maps can evaluate supplied expressions in a different order after an independent parse. When such a map evaluates more than one supplied expression, admission requires every argument to preserve its incoming block lookup owner and macro-loader cache. State-changing arguments are refused with an explicit order-sensitive-argument diagnostic. Pure multi-argument calls, single supplied arguments and ordered array/binary operands retain native behavior. String concatenation visits native function-call values left to right; their argument expressions participate in the same scope, recursion and complete-depth analysis as a direct function call. The adapter neither sorts native maps nor promises support for every finite Tera program.

Analysis has separate bounded metadata/work accounting derived from the selected source-byte allowance, with a 64 MiB profile ceiling. Admission diagnostics name their retained template but do not invent AST source spans or a native runtime cause. Templates remain trusted programs: native parsing, expensive expressions, intermediate renderer allocations and elapsed CPU are not a hostile-code sandbox or execution-isolation guarantee.

## Connected application path

`CompositionMetadata::compile_templates` uses the declared template directory from the checked Theme source. It removes that directory prefix, preserves nested and empty directories and parses all regular files there, regardless of suffix. It excludes metadata, capability inputs, assets and siblings. The application checks the complete subtree and passes its captured bytes to the rendering port without reopening or copying source files.

The application verifies the adapter's template identity and returns a `CompiledTheme`. The private report links it to the workspace digest, site, Theme/palette, source and tree identities, manifest path/digest, compiler profile and `limits`. The Tera adapter's `tool` is `tera@1.20.1/gazette-captured-templates-v8`. A missing, oversized or non-printable profile fails. The profile describes adapter behavior and does not identify the built executable.

The same immutable limits drive application context/output checks. The evidence passes unchanged into page/site operator reports and durable preparation history; it is not injected into public models or template context. Changing the profile or bounds changes private preparation evidence even when output bytes coincide. `CompiledTheme::render` delegates supplied presentation context and also refuses oversized successful adapter output; the adapter remains responsible for bounded emission. The Rust SDK selects Tera once; the CLI uses the same SDK compilation path.

`CompiledTheme::render_page` additionally joins an exact retained authored object to this selected Theme after checking workspace digest and home site. One compiler projection feeds both Tera and the returned local JSON model; the template never reconstructs relationships or receives private source/configuration evidence. Selected Theme/palette keys are included as presentation settings, not inferred identity facts. See [connected local page rendering](content-model.md#connected-local-page-rendering).
