# Markdown {#markdown-boundaries}

Gazette uses Comrak 0.54.0 to parse publication Markdown and developer manuals. Gazette then resolves the headings, links, components and resources that depend on its publication model.

| Source profile | What it parses |
| --- | --- |
| `publication` (default) | Tables, task lists, strikethrough, footnotes, heading attributes and GitHub alerts |
| `publication-math` | Everything above, plus inline and displayed mathematics |

Set `markdown_profile = "publication-math"` on the content binding to parse equations. An attached notebook uses its own source's profile. The editor uses the workspace profile for a bound file and ordinary Markdown for an unbound file. To turn equations into rendered output, follow [Typeset mathematics](https://rawthoughts.in/gazette/typeset-mathematics/) or [pass generator options to the renderer](local-rendering.md#select-native-math-or-diagram-generation).

## Headings

Add a heading ID or class using the Markdown adapter's attribute syntax. Quote IDs containing Unicode or punctuation outside the bare form, such as `# Heading {#"café" .wide}`. The DOM ID is `café`, and the link fragment is `#caf%C3%A9`. Malformed attributes remain heading text. Recognized attributes other than IDs and classes fail. Gazette supports one attribute grammar.

## Code blocks

Name a language on a fenced code block to highlight its syntax:

````markdown
```rust
let message = "Hello, reader";
```
````

Website HTML uses Syntect 5.3.0 with the `two-face` 0.5.2+bat-0.26.1 syntax bundle, including TypeScript and TOML. The pure-Rust regex profile supplies the supported definitions; excluded or unrecognized syntaxes use plain text. Recognized language names and extensions produce nested `span` elements with `syntax-` classes; for example, `keyword.control` becomes `class="syntax-keyword syntax-control"`. The existing `pre` and `code` elements retain their attributes, including `language-rust`. Themes supply colours and code-panel presentation; the engine emits no inline styles.

Unknown languages, unlabelled fences and indented code remain escaped plain text. Gazette does not guess a language from the code. Highlighting preserves the code's text, whitespace and reading-text projection; HTML-looking examples remain literal. A syntax-processing error refuses the render rather than inserting raw source. Highlighted markup counts toward the ordinary output limit. Markdown counterparts and repository link rewriting retain code as Markdown.

## Alerts

Put one of the five uppercase GitHub markers alone on the first line of a blockquote:

```markdown
> [!IMPORTANT]
> Keep the original source until its replacement has been checked.
```

Use `NOTE` for useful context, `TIP` for helpful advice, `IMPORTANT` for information the reader needs to succeed, `WARNING` for an urgent problem to avoid, and `CAUTION` for a risk or negative consequence. The body keeps its paragraphs, lists, formatting, headings and checked links. Escaped markers, fenced examples, unknown markers, markers after earlier quoted text and markers followed by prose on the same line remain ordinary text.

Website HTML uses Comrak's `markdown-alert` wrapper, a type class such as `markdown-alert-note`, and a visible `markdown-alert-title`. The five type suffixes are `note`, `tip`, `important`, `warning` and `caution`. Themes supply their appearance; ordinary quotations retain their existing markup. The rendered Markdown alternate carries the visible alert title and its complete body as ordinary Markdown, including nested quotations, lists and links, so it also works in readers without an alert extension. Repository link rewriting keeps the authored Markdown form. [Documentation output](mintlify-formats.md#headings-and-markdown-transport) maps the same source markers to Mintlify's callouts.

## Mathematics

The `publication-math` profile recognizes native dollar expressions, math-code and fenced `math` constructs. For example:

````markdown
The mean rate is $r = n/t$.

```math
r = \frac{n}{t}
```
````

An equation requires a complete result from the configured typesetter before it can render as MathML. Use a code span for literal notation. Dollar text remains ordinary text when the math profile is not selected.

## Footnotes

Put the reference beside the statement it qualifies and define its text elsewhere in the same document:

```markdown
Keep the instrument's resolution with the reading.[^resolution]

[^resolution]: Record the unit and the smallest distinguishable increment.
```

Gazette allocates emitted note numbers and backlinks. Unused definitions do not appear in the rendered body. An article-owned `notes.md` is a separate companion for longer commentary; see [article Notes](content-model.md#article-notes-stay-with-their-article).

## Links in tables

Inside a Markdown table, escape the separator in an ecosystem reference:

```markdown
| Next task | Guide |
| --- | --- |
| Start writing | [[site:example.test/guide/\|Read the guide]] |
```

Outside a table, write the separator as `|` without the backslash. An unescaped separator inside a table starts another cell, so the intended link can become truncated text instead of a link. The [reference contract](links-and-crossings.md) defines ecosystem destinations and their resolution.

## Use a static block component {#selected-static-block-components}

The captured Theme manifest owns each local component definition, its template entry and its data-only argument schema. `ComponentName` is not a publication `Key`: it starts with an ASCII letter, followed by ASCII letters, digits, hyphens or underscores. Spelling is exact; `callout` does not create an entity or select an arbitrary template path.

```toml
[components.callout]
template = 'components/callout.html'
arguments = { type='object', required=['tone'], additionalProperties=false, properties={tone={type='string', enum=['note','warning']}} }
```

`template` is relative to that Theme's selected captured template directory. `arguments` is a Draft 2020-12 JSON Schema embedded in TOML. Local schema references are supported; external retrieval is forbidden. Omitting the schema permits only an empty argument object. No second workspace configuration or adapter lookup supplies missing definitions.

An authored block selects the local name and one TOML inline argument table:

```markdown
::::callout {tone = "note"}
Ordinary **Markdown** body.

:::callout {tone = "warning"}
Nested body.
:::
::::
```

The outer fence is longer than the nested fence. A block inside a fenced code example remains literal. Arguments are decoded from the original retained header bytes, not Comrak's entity/backslash-normalized information string. For example, an argument containing `&amp;` retains that string rather than silently becoming `&`. Unknown names, malformed tables, duplicate arguments, schema-invalid values, missing templates and exhausted bounds refuse the complete affected render rather than returning a successful prefix.

Editor assistance uses the component's Draft 2020-12 schema for names, argument properties and closed-value completion. The shared TOML analysis supplies the current source ranges; interfaces do not discover or parse theme files independently. Notebook Markdown cells and stored Markdown outputs use the same component path. Generated Markdown origins are not yet supported, and rendering a notebook does not execute it.

<a id="initial-component-islands"></a>

## Component islands

A selected Theme component may explicitly enhance its mandatory static template:

```toml
[components.callout.interaction]
module = 'module:callout'
export = 'mount'
mount = 'load'
```

`module` denotes an exactly selected logical browser module, not a path or URL. `export` selects one exact export; no default or first-export fallback is inferred. Only `mount = 'load'` is supported. The schema-validated component arguments become JSON props as data, not generated executable source. The static template remains the no-JavaScript fallback; excerpts remain static and receive no mounting wrapper. This is enhancement, not framework hydration.

The page's shared `presentation.interaction` contains that browser-module resource. The Theme must include its URL as a module script; emitting the resource does not itself load it in a browser. The runtime invokes the selected export with `{element, props}`, using a separate mount slot and retaining the static fallback. It marks `data-gazette-state` as pending, mounting, mounted or failed. A missing or noncallable export, invalid props/target, or rejected mount clears the mount slot and produces `gazette:island-failure` with bounded component/reason data, not raw exception contents.

The browser runtime keeps the static content when mounting fails. It does not sandbox a module, recover a failed module import or execute notebook code.

## Limits

Source, node and output ceilings bound accepted work; they do not isolate CPU or allocator behavior. See the [Theme contract](theme-reference.md#markup-and-execution-gates) for the distinct markup and template execution profiles.

## How one document supplies several views {#one-owned-source-multiple-projections}

This section describes the compiler/adapter contract for maintainers. Authoring syntax above uses these same source, heading and component facts.

Core owns source identity, location precision, bounded syntax facts and resolved formatting inputs. The Markdown port returns an immutable source-bound document. Its private adapter retains exact source and consumer-required facts, not a public parser handle or a copied AST. It drops the analysis arena and reparses the same bytes for normalized formatting through public upstream APIs.

The compiler assigns anchors, admits the explicit excerpt marker and resolves references before submitting a complete formatting plan. Identity and complete occurrence bindings must match; source order alone is not a binding. Tera and machine projections consume the resolved page result rather than reparsing.

Whole-source inspection retains unused definitions and references. Safety checks cover that source before resolution callbacks, including content after an excerpt. Published body text comes from the normalized emitted structure used for HTML, so omitted definition text cannot silently enter search or feeds.

Adapter locations identify containing constructs unless exact lexical bytes were separately verified. UTF-8 range validity is not edit authorization. Decoded notebook-cell text is not raw notebook JSON; adding a parent offset cannot establish that relationship. Generated-origin mapping remains separate work. Versioned authored-source editor sessions reuse these facts through the [shared LSP/MCP authoring contract](lsp.md); they do not manufacture missing origin evidence or grant edit authority.

### Math generation and source binding

The math profile recognizes native dollar, math-code and fenced `math` constructs. Recognition does not run a tool. A complete retained result must bind the exact body digest, profile, original occurrences, TeX and display mode before the common formatter can emit MathML. Framed documents retain distinct whole-file and body coordinates. Notebook fragments retain explicit cell/output locations and their own decoded Markdown identities; their spans are never interpreted as raw JSON offsets. Missing, extra or stale results reject the complete binding operation; formatting also validates fragment structure. Ordinary dollar text is unchanged when the math profile is not selected.

Workspace `math_typesetter` selects a pinned native interpreter plus an exact configured source and capture-relative entrypoint. Publication acquisition retains that complete renderer/import tree once. The process adapter materializes only those retained bytes in a fresh private tree, prepends the retained entrypoint to the configured arguments, verifies the materialized tree after execution and removes it before returning success. The original native resource path is never reopened. The shared local publication/preparation recipe independently selects `math_generation`, whose `allow_execute` and finite request, time, formula and output limits govern the effect. Omission performs no acquisition. CLI site-set rendering, SDK rendering and preview use this acquisition phase before pure compilation; renderers never invoke the tool. Results and exact request/response observations are retained once for the complete captured corpus and reused across site views.

The capture evidence carries the typesetter tree identity and entrypoint independently from the interpreter/command digest. An interpreter pin alone still does not attest scripts or imports; the retained resource binding supplies that separate evidence. Retention does not establish generator admission or publication approval.

### Use the opening heading {#opening-heading-projection}

When the first top-level emitted block is an H1, the same formatter records the byte immediately after its closing tag. It preserves the resolved anchor, inline formatting and link annotations. A later, nested or lowered heading does not become the opening. This constant-size boundary is internal evidence, not a source coordinate or an HTML-parser instruction. The compiler can expose borrowed `opening.heading_html` and `opening.remainder_html` from the complete `body_html`; neither a second rendering nor a second retained body is required. Ordinary output limits still apply, and complete serialized models charge both representations.

### Render a component {#component-projection}

The existing Markdown parser and compiler resolve child headings, links, footnotes, text and excerpts before static presentation. Tera receives only `component.name`, `component.arguments`, `component.body.html`, `component.body.text` and `component.excerpt`; it receives no parser AST. An excerpt inside a component supplies its balanced child prefix. Excerpt headings omit duplicate IDs while retaining resolved link destinations. Template execution consumes captured inputs, never live source acquisition.

A Theme may declare an empty component with `semantic = { kind = "container-contents" }` to place a collection's children or a journey's steps inside its body. Use it once, at the top level, without arguments or body text. For example, place `:::guides` followed by its closing `:::` before an authored FAQ when `guides` has that semantic declaration. Nested slots, duplicate slots, nonempty slots and slots on ordinary pages fail. Without a slot, the existing appended contents remain.

The slot renders the existing resolved navigation at that position in static HTML and native Markdown. It adds no authored text, search terms or FAQ claims. Markdown keeps its existing discovery policy: in particular, a journey with an undiscoverable member does not gain a generated reading list merely because the direct HTML page shows its explicit steps. Excerpts omit the generated navigation.

Ordered `{name, arguments}` records pass from the same `RenderedMarkdown` into shared `PageData`. Theme/model/search and private review retain those authored values without private source coordinates or template-derived meaning. Search and feeds use the same authored text projection, not text scraped from the component's rendered wrapper. Private review and candidate review validate the selected captured Theme contracts and project semantic children; they do not execute Tera templates or establish presentation approval.

### Interaction compilation

The retained publication build compiles a deterministic page-local bootstrap through the existing JavaScript compiler and shared resource evidence validator. Its imports bind to the exact selected module closure; missing or disabled bindings fail rather than trigger discovery. The generated bootstrap has separate private `islands` evidence: its generated tree identity, source digest, compiler profile and resource record are not forged captured-authored provenance.

### Find which footnotes appear {#emitted-footnote-closure}

Gazette derives an emitted footnote closure from retained source structure rather than trusting adapter counts that include references inside unused definitions or image labels. Reachable nested definitions retain deterministic ordinals and backlinks; unreachable and image-label occurrences remain inspectable source evidence but cannot create rendered notes or orphan backlinks. Duplicate lexical attributes and consumed reference definitions still require source-level evidence beyond a normalized adapter field.

### Images in the rendered model

Emitted Markdown image uses have a separate bounded projection from static assets. `RenderedMarkdown.images` preserves source traversal, exact resolved destination, plain image-label text (including a deliberately empty alt), optional title and an optional hosted occurrence address. A hosted image carries the same address in `data-gazette-image`. Only emitted image constructs enter this list; an unused definition, ordinary download link, package sibling or compiled resource alone does not. This is not media inspection or a claim about remote resource existence.

### Declare image renditions

Ordinary Markdown images keep their usual destination, alt text and optional title. To give one image explicit light and dark versions, add an `images` declaration at the frontmatter root, before `[content]`:

```toml
[[images]]
default = "figure.svg"
light = "figure.svg"
dark = "figure-dark.svg"
width = 480

[content]
# The page's existing fields follow.
```

The body still contains one image and its existing caption:

```markdown
![The three steps and their connections](figure.svg "Process diagram")

Each step passes its result to the next.
```

Structured landing YAML uses the same fields:

```yaml
images:
  - default: figure.svg
    light: figure.svg
    dark: figure-dark.svg
    width: 480
```

Only `default` is required. `light` and `dark` are optional explicit file references; Gazette does not infer pairs from filenames or transform colors. A transparent image that works in both modes needs no alternatives. Optional positive integer `width` expresses the intended display width in CSS pixels, independently of the stored pixel dimensions; a Theme may constrain it to the available reading width. The native formatter includes it in the initial HTML; it does not resize the stored image. It also emits `data-gazette-image-sized` only for an authored display width, so Themes can distinguish that choice from verified intrinsic sizing.

Optional `loading = "eager"` or `"lazy"` and `fetch_priority = "high"`, `"low"` or `"auto"` select browser delivery hints for matching image occurrences. The native formatter emits `loading` and `fetchpriority` unchanged; omission leaves the choice to the browser. No page route, theme or traversal position supplies a default. These hints are independent from light/dark variants and CSS width. A [bound raster resource](page-resources.md#bind-an-original-image-to-its-compiled-rendition) supplies verified intrinsic dimensions after compilation, without changing the original image references.

References resolve relative to the owning page within its selected content source. They cannot name remote URLs, root-relative URLs, query strings, fragments, retained-only inputs or files outside that source. Supported raster files use the static asset plan. SVG files use an automatic leaf-page companion or an explicit `svg-image` page resource and pass its existing inert-vector validation. Duplicate declarations resolving to the same default source file are rejected, including differently encoded or `./` spellings.

Every matching emitted image receives the declaration, preserving its own occurrence address, alt, title and position. This includes landing sections, article notes and file-referenced notebook Markdown; notebook references resolve relative to the notebook before matching, and cell-scoped addresses stay distinct. Stored embedded notebook images retain their separate rendering and inspection path. An occurrence's query or fragment is preserved on its resolved alternatives. Declarations also validate and select their inputs when unused; they never create an image occurrence by themselves.

The resolved image keeps `destination` as the default URL. Optional `variants` contains only `light` and/or `dark`, and optional `width` carries the display hint. All destinations use the same public or preview base as other page assets. The default body HTML keeps a single `img` with its `data-gazette-image` occurrence address, so a reader can see it without scripts. With a `dark` alternative, that `img` sits in a `picture` after one dark source whose `media` follows the Theme's [starting colour mode](theme-reference.md#initial-color-mode), so the browser fetches the dark rendition directly when the page starts dark. Use a readable default, normally the light rendition when preparing a pair for print. The Theme owns saved preference, system and world-default selection, responsive width, enlargement and print presentation; the compiler supplies the declared choices and their common occurrence identity.

For a static SVG whose live text and colours should inherit the page's fonts and tokens, set `inline = true` on its `images` declaration. Keep the same Markdown image and its existing SVG resource or leaf-page companion. This opt-in accepts one SVG without light/dark alternatives; a matching Markdown occurrence cannot add a query or fragment. Other images retain their ordinary external rendering.

Gazette validates the SVG against the [inline SVG profile](page-resources.md#inline-svg-figures), scopes its IDs to each image occurrence, and emits it inside a `span` with `data-gazette-image-ref` equal to that occurrence's address. The original `img` remains a separate hidden sibling with `data-gazette-image`, its resolved `src`, alt and title. The complete drawing therefore works without JavaScript, while a viewer can move and restore the exact wrapper and retain its inherited presentation. The wrapper uses the image's alt and title; an empty alt keeps it decorative. The public image model and Markdown alternate retain the original image address, alt, title and external SVG URL.
