# Mintlify input formats

Use this reference when your documentation build already has Markdown, an API specification or generated reference. Gazette adapts those files to Mintlify’s input formats and keeps the original files intact. For a complete setup, start with [Add a manual to Field Notes](mintlify.md#configured-local-assembly).

| Your input | Selection | Result and main restriction |
| --- | --- | --- |
| Authored developer manual | Manual `.md` pages and linked or explicit assets | Pages at configured routes; titles and navigation come from configuration. |
| Generated Markdown | `markdown`, file or directory | Ordinary pages; a directory may contain only supported `.md` pages. |
| TypeDoc project JSON | `typedoc-json`, file | Unchanged JSON supplied to Mintlify's SDK conversion; schema version `2.0`. |
| Sphinx JSON-builder output | `sphinx-json`, directory | Selected contexts and supported companions supplied to Mintlify's SDK conversion. |
| Rustdoc HTML | `rustdoc-html`, directory | Static API pages converted to controlled Markdown and inert HTML. Script-supplied API members are restricted. |
| OpenAPI 3 JSON or YAML | `openapi-json` or `openapi-yaml`, file | Unchanged specification supplied as HTTP reference; supported versions are listed below. |
| Swagger 2 JSON or YAML | `swagger-json` or `swagger-yaml`, file | Converted to OpenAPI 3.0.3, then validated. |
| Rustdoc JSON | `rustdoc-json` | Unsupported; target preflight rejects it. |

Imported artifacts and explicitly generated artifacts use the same format profiles. The [generator guide](documentation-generators.md) covers running a generator; [Mintlify integration](mintlify.md) explains embedding or extending the assembler.

## Select files and routes explicitly

Point the module at the file or directory you want to publish:

```toml
selection = { kind = "file", path = "reference.json" }
```

This includes one file and does not export siblings. For a subtree, use `{ kind = "directory", path = "reference" }`. Use `{ kind = "directory" }` for the source root.

Each reference uses its own `<project-slug>/<reference-slug>` prefix. Slugs use lowercase ASCII letters, digits and internal hyphens. `api` and `artifacts` are reserved. Missing input, unsupported input types, duplicate namespaces and page/asset path conflicts fail. Configuration order controls navigation. Several projects get a project dropdown; one project uses the grouped sidebar.

`source::Limits` defaults to 10,000 files and directories, depth 64, 64 MiB per file and 512 MiB total file bytes. Pages, companion files, provider support files, `docs.json` and `.mintignore` all use this budget. Source reads, parsing and format checks have separate limits. Exceeding a limit fails assembly without a truncated page or a partial success.

### Physical page addresses

For selected manual and generated-Markdown pages, Gazette maps a final page basename of `index` to `gazette-page-index`, preventing Mintlify from treating it as the parent page. A final basename already beginning with `gazette-page-` receives that prefix again, so distinct source names remain distinct:

| Logical route | Emitted route |
| --- | --- |
| `client/library/index` | `client/library/gazette-page-index` |
| `client/library/index/index` | `client/library/index/gazette-page-index` |
| `client/library/gazette-page-index` | `client/library/gazette-page-gazette-page-index` |

Only the final component changes. Keep configured manual routes, generated Markdown `pages[].path` selections and relative source links in their original spelling. Emitted `.md` paths, `docs.json` navigation, body links and the private page index use the mapped address. Source identities and original artifact bytes are unchanged. This page-name mapping does not change fragments; the separate [heading-ID mapping](#headings-and-markdown-transport) still applies.

TypeDoc, Sphinx and OpenAPI virtual provider routes and the separate Rustdoc HTML projection do not use this mapping.

## Authored developer manuals

Name the manual source and give each `.md` page a navigation title and extensionless route. A selected `README.md` or `index.md` can use the module root as its route, such as `library`; a component README can use `library/sdk/js`. These pages can coexist with reference and companion paths below them. Configure those routes explicitly: source filenames do not override the workspace’s page addresses or chapter order. Relative image and resource links select companion files during acquisition, before target assembly. Missing files fail assembly, and linked Markdown pages still need their own navigation entries. The assembler never reads outside that capture or publishes unlinked files. Explicit companion selections can supply a different output path or a whole directory. Manual source and navigation are required together.

Use nonempty UTF-8 without NUL or frontmatter. If you write an H1, put it first and use the same title as the navigation entry after trimming. Gazette keeps its anchor and Mintlify displays one title. Without an H1, the configured title is enough. Multiple H1s, a later H1 or a mismatch fail. Dollar and fenced math use the manual's math profile; assembly does not run a typesetter.

Write relative links from the source page. Gazette maps inline links, reference-style links and fragments to the pages or assets you listed. Discovered companions use `<module-slug>/assets/<source-path>`. When several modules link the same captured companion, the lexically first module slug among those readers supplies its single output path. Explicit companion destinations take precedence; collisions with manual pages or reference subtrees fail. Source paths which cannot form a target route need an explicit companion destination. All copied bytes count toward the existing output limits.

A companion destination ending in `.txt` becomes a hidden page, because Mintlify serves static `.txt` files only on its Enterprise plan. Gazette drops `.txt` from the destination and replaces any other character in its final name that is not a letter, digit, `-` or `_` with `-`: `source/AGENTS.md.txt` becomes the route `source/AGENTS-md`. The page carries `noindex: true`, takes the source path as its title and holds the file's text in one fenced code block. The block's language is the source extension, or `text` without one, and its fence is longer than any backtick run in the file, so the text stays literal. Manual links point to the route. A fragment on such a link fails, as do non-UTF-8 bytes and NUL. Hidden pages stay out of navigation and Gazette's page catalog, and a route already taken by a manual page fails. Gazette writes them as `.md`, which Mintlify compiles through its MDX pipeline, because output disclosure admits `.mdx` only in Gazette's generated grammar. Images and other assets keep their configured paths and bytes.

Acquisition reads the configured manual pages, explicit assets and parsed relative companions, retaining necessary directory ancestors. It uses the same Markdown link grammar as assembly; link-like code and public URLs select no native files. Git ignore rules apply at a selected repository root, with tracked files retained. Missing or ignored required inputs fail explicitly. Included-filesystem and filtered-filesystem bounds remain in force, and named roots and branch checks keep their ordinary meanings. Every explicitly included path must still exist; checking an unused include reads its metadata, not its contents.

> [!IMPORTANT]
>
> A pinned manual compares only the selected bytes and directory entries with the exact commit, retaining the parent gitlink checks for a submodule. Its source evidence includes `selected_only = true`. Use that evidence for the captured inputs; it does not verify the complete repository subtree.

A source shared with an imported reference captures the union of the manual inputs and that reference’s explicit selection. A selected generator retains its configured source-input contract, because a build may need files beyond the manual links. Use a separate source binding when those roles need different bounds. Assembly uses retained bytes only and never rereads the source.

Modules share relative destinations only if they use the same source key and capture. Equal bytes under different keys do not join their maps. Missing, escaping or ambiguous destinations fail.

## Formula presentation

Mintlify renders the selected formulas from their TeX. Gazette includes `gazette-provider-math.css` so the provider’s formula styling also loads when JavaScript is disabled. It imports the same KaTeX stylesheet and fonts that the pinned provider uses; it does not replace the renderer or change the formulas. The stylesheet counts toward the target’s output limits and its reserved name cannot be replaced by an authored asset.

The pinned client serves this stylesheet at `/_next/static/media/katex/0.16.47/katex.min.css`. When upgrading Mintlify, check this path and its fonts. The stylesheet uses an origin-relative URL; keep the provider assets at the origin root.

## Headings and Markdown transport

Manual pages and generated Markdown use the shared Markdown parser. Code remains fenced, autolinks become ordinary links, and literal braces remain text. The transport omits invisible HTML comments and serializes admitted inert HTML for JSX, including self-closing void elements. HTML names are normalized and attribute values remain literal data. Imports and expression-looking text are never run.

In the Mintlify target, a standalone GitHub alert marker at the start of a blockquote selects a native callout:

| Source marker  | Mintlify component |
| -------------- | ------------------ |
| `[!NOTE]`      | `Note`             |
| `[!TIP]`       | `Tip`              |
| `[!IMPORTANT]` | `Info`             |
| `[!WARNING]`   | `Warning`          |
| `[!CAUTION]`   | `Danger`           |

```markdown
> [!NOTE]
> Restart the preview after assembling a new directory.
```

The callout body keeps its Markdown structure and checked links. Escaped markers, fenced examples, unknown markers and markers followed by prose on the same line remain ordinary text. Source bytes are unchanged. [Website alerts](markdown.md#alerts) use the same recognition with Comrak's HTML; this component mapping belongs to Mintlify, while default MDX transport keeps readable alert labels. Authored JSX and executable expressions remain unsupported.

H2–H4 receive explicit target IDs. H5/H6 use the target's fixed `Heading` component so the output retains their level, text, inline children and anchors. Authored `Heading` components and raw HTML H5/H6 are refused. A heading class, unsupported attribute or Gazette component directive is not silently discarded.

On selected manual and generated-Markdown routes, source IDs `content` and `page-title` become `gazette-anchor-content` and `gazette-anchor-page-title`. An ID already starting with `gazette-anchor-` receives that prefix again, keeping the mapping unambiguous. Headings, admitted raw `id`/anchor `name` attributes and checked same-page, relative and site-root links use the same mapping. Source and editor IDs remain unchanged. Arbitrary HTTP links and provider-generated routes do not receive this rewrite.

Raw HTML must pass the target's active-content check. Manual and generated Markdown must use checked Markdown links instead of raw `href` or `src` attributes. Executable or resource-bearing elements and event, style or effect attributes are rejected; benign structural markup and admitted named anchors remain available. This is a restricted input profile, not an HTML sanitizer.

Use [documentation layouts](#documentation-layouts) for tabs, steps and disclosures. Raw HTML with these reserved layout names is refused to avoid content loss in the provider.

The target includes `gazette-layout-navigation.js` and `gazette-fragment-navigation.css` when a selected manual or Markdown reference retains a removed title, contains admitted HTML or uses documentation layouts. The stylesheet gives ID and named anchors the provider's heading clearance. The script opens enclosing disclosures, including nested ones, and keeps the visible page title with a leading title alias. It uses Mintlify's navigation hook and native controls while preserving heading addresses and unrelated disclosure state.

Without JavaScript, body anchors retain their clearance and native disclosures remain expandable. Opening a nested target and keeping its preceding title in view require the enhancement; a scriptless title-alias arrival can leave the title above the fixed header. Both support files share the target's output limits and cannot be replaced by authored assets. The compatibility adapter is qualified with `mint` 4.3.8 (`@mintlify/cli` 4.1.8) and `@mintlify/client` 0.0.4101. It sets the provider’s internal `_disabled` flag on generated accordions to suppress automatic address and clipboard changes while keeping native controls and ARIA state. Qualify this behavior again when updating the provider; authors cannot set this flag or supply executable properties.

## Documentation layouts

Use explicit layout blocks when the explanation needs alternatives, a sequence or optional detail. The shared Markdown parser reads their colon fences and children; the Mintlify target supplies the corresponding components.

| Source block | Use |
| --- | --- |
| `tabs` containing `tab "Title"` blocks | Alternative platforms or implementations |
| `steps` containing `step "Title"` blocks | Actions performed in order |
| `code-group` containing titled fenced code blocks | Alternative code samples |
| `accordion "Title"`, optionally inside `accordion-group` | Detail the reader can open when needed |
| `frame` | A figure and its supporting content |
| `card`, optionally inside `card-group` | Navigation choices with a title, destination and short description |

For example, two implementations of the same integration can have their own explanations and checked links:

```markdown
::::tabs
:::tab "Rust"
Use the [Rust assembly API](mintlify.md#assemble-documentation-from-rust).
:::
:::tab "JavaScript"
Use the [Node assembly API](../sdk/js/docs/native-api.md#documentation-repository-assembly).
:::
::::
```

Titles are plain text, expressed as nonempty JSON strings. Container blocks take no arguments. A code group contains only fenced samples, each with a language and explicit title, such as `rust Client.rs`. Tab, step and accordion titles within a group must be distinct. Each block needs its own closing fence; use a longer outer fence when nesting. The body keeps ordinary Markdown links, headings, code and mathematics. Use headings for links to sections; layout labels do not define source anchors.

### Cards

Use a card for a useful next choice. Its first paragraph is one ordinary Markdown link; that link supplies both the title and the destination. Write a short description after it:

```markdown
::::card-group
:::card
[Build from source](install.md#what-a-build-needs)

Build Gazette and continue with your publication.

:::
:::card
[Understand the architecture](architecture.md)

Follow a content change through the engine.

:::
::::
```

The target emits Mintlify `CardGroup` and `Card` components. The title link is checked and projected like other manual links, including its fragment and any local preview address. Cards and card groups take no arguments. The description cannot contain another link, image or layout block, because the whole card is already a link. Keep longer explanations on the destination page.

These layouts apply to Mintlify manuals and generated Markdown. They are separate from publication Theme components and do not enable raw JSX or arbitrary properties. Ordinary Markdown rewriting and default MDX transport keep their existing behavior. Unknown blocks, invalid nesting and incomplete groups fail with a source location instead of dropping their contents.

## Generated Markdown

A file is placed at `<project-slug>/<reference-slug>.md`, subject to the [final page-name mapping](#physical-page-addresses). A directory follows its selected-relative layout beneath that prefix with the same final-name exception. Directory components and `.md` stems permit ASCII letters, digits, hyphens and underscores; case is significant. For example, `types/Client.md` becomes `client/library/types/Client.md`, with navigation route `client/library/types/Client`.

Empty or non-UTF-8 pages, NUL, source frontmatter, hidden files, assets and other extensions reject a Markdown directory. A leading plain H1 supplies the target title and keeps its body anchor. Later H1 sections, such as an API member's Errors section, retain their headings and links. Rich title headings and titles preceded by other content keep their existing body representation.

Directory navigation defaults to lexical page order. An optional `pages` tree can name individual pages or select whole directories inside the groups you want readers to see:

```toml
pages = [
  { kind = "page", path = "index" },
  { kind = "group", title = "JavaScript", pages = [
    { kind = "directory", path = "js" },
  ] },
  { kind = "group", title = "WebAssembly", pages = [
    { kind = "directory", path = "wasm" },
  ] },
]
```

Paths are extensionless and relative to the selected artifact directory. A `directory` entry expands all pages beneath that exact directory prefix; `js` does not select `js-other`. Each directory places its `index` page first, other direct pages in lexical order, then subdirectories in lexical order as nested groups labelled by their directory names. A directory without an index still includes all its pages. This keeps generated symbol lists with their generator while the workspace chooses the named chapters. Use a `page` entry when an individual page needs an explicit position.

Every selected page must appear exactly once after expansion. Missing, repeated or extra pages, overlapping or empty directory selections, empty groups, explicit `null`, more than 64 combined authored and derived nested groups, or an override on another format or a single file fail. Grouping changes navigation, not the selected corpus or page bytes.

Links may use nonempty fragments or explicit HTTP(S)/mailto destinations. Directory pages additionally accept exact extensionless or `.md` relative links, such as `./types/Client.md#method`; the target checks the selected page and fragment, then emits its site-root address so Mintlify keeps navigation within the documentation. Single-file input has no sibling-page scope. Traversal, missing or case-mismatched pages, percent-encoded paths, queries and local embedded images fail. Link-like code remains literal.

Both file and directory inputs can link to actual pages anywhere in the assembled target using site-root routes, for example `/other-project/library/Client`. An optional `.md` or `.mdx` suffix is removed. These links are checked after all modules assemble, so forward and reciprocal links work. Network-path URLs and links to retained `artifacts/` inputs are refused. A TypeDoc, Sphinx or OpenAPI artifact does not establish every page or fragment its provider will generate.

## TypeDoc JSON

Supply one project JSON file with `variant: "project"`, a nonempty `name` and `schemaVersion: "2.0"`. Reflection IDs must be unique nonnegative integers; local reference and group targets must exist. The `-1` unavailable-symbol sentinel is accepted for type references and reference reflections, but not inline comment targets or group members. Other negative or fractional targets fail.

Symbol targets may carry exact `packageName`, `packagePath` and `qualifiedName` fields. Package coordinates must be bounded relative source names; absolute, traversing or URL-shaped paths and extra fields fail. External URLs must be uncredentialed HTTP(S) addresses. Duplicate JSON members, including differently escaped spellings of the same key, fail at every depth.

The original JSON is packaged unchanged, including signatures, comments, examples and source/version metadata. Mintlify performs the symbol-page conversion. Gazette checks this structural profile rather than every TypeDoc extension or the eventual provider-generated symbol URL.

## Sphinx JSON directories

The selected directory must contain `globalcontext.json`, `searchindex.json` and the root document named by `root_doc` or `master_doc`; conflicting root names fail. Page context names must match their `.fjson` paths. Ordinary pages need body, title and table-of-contents strings. Recognized search and index contexts may be bodyless; unknown bodyless extension contexts fail.

The reading catalog follows Mintlify's actual Sphinx page selection: it omits `genindex`, `search`, `py-modindex`, underscore-prefixed path components and contexts with empty bodies. These contexts remain retained and validated artifacts, but they do not become search results or reading links. The configured root must be a page the provider serves.

The target retains `.fjson` pages, global/search contexts, `objects.inv`, and files under `_images/`, `_downloads/`, `_static/` and `_sources/`. Captured originals stay unchanged; the provider-facing page contexts receive only the emphasis correction described below. Other retained files are copied byte for byte. It excludes only root `environment.pickle`, `last_build`, `.buildinfo` and the exact `.doctrees/` subtree. A file such as `_downloads/environment.pickle` is not covered by that exclusion. Unclassified companions fail; no pickle is loaded.

Mintlify’s HTML-to-Markdown conversion can split adjacent italic type tokens into visible punctuation and excessive spaces. Gazette joins directly adjacent, attribute-free `em` elements in a page’s body, title and table of contents, including a type link whose only child is such an element. For example, linked `RetentionPolicy`, italic `|` and italic `None` become one italic run containing the original link and text. Link attributes, identifiers, text and spacing survive; attributed emphasis, intervening text and code examples are not joined. This correction uses balanced HTML fragments, with limits of 128 nested elements and 100,000 tokens. Unsupported fragment structure, obsolete void elements and elements requiring raw-text or other context-sensitive parsing pass through unchanged; exceeding a limit fails assembly. Unchanged contexts keep their exact bytes, and changed contexts retain their other JSON values. Both forms count toward the target’s byte budget.

Local page, asset and fragment references in body/toc fragments must resolve within the selected capture, including `.html` and directory-style routes. Malformed, traversing, missing or case-mismatched targets and unsupported schemes fail. Active elements and effect attributes are rejected before checking ordinary `href`/`src` references. This does not import Python, execute Sphinx or make arbitrary extension output compatible.

## Rustdoc HTML directories

The static profile converts native module/index, function, struct, enum, union, trait, type alias, exported macro, constant, static and reached source pages into controlled `.md` pages with title metadata and escaped inert HTML. It preserves formatted declarations, linked types, descriptions, examples, fields, variants, members, reexports and static implementation sections. Known browser controls are removed without dropping their contained signatures.

Each page also has a provider-only `.mdx` representation derived from the same admitted structure. Literal JSON strings carry code and heading text through Mintlify without changing whitespace, signatures or punctuation. Ordinary prose retains HTML-flow spacing, and MathML retains its existing text and TeX annotation. Exact `.mintignore` entries select the provider representation while preserving the native `.md` bytes; navigation and checked links still name one extensionless page. This does not enable authored expressions, imports or arbitrary MDX. Output disclosure admits only this bounded generated grammar and then inspects its decoded HTML, attributes and joined text.

Mintlify documents hosted [Markdown export](https://www.mintlify.com/docs/ai/markdown-export) and [LLM indexes](https://www.mintlify.com/docs/ai/llmstxt) separately from local preview. Retaining a native `.md` file does not establish that a provider serves it at the corresponding public URL. The qualified classic local preview serves the human page but does not implement the hosted Markdown negotiation or automatic LLM index endpoints; Gazette's existing explicitly selected preview index remains separate.

The selected API tree supplies nested navigation. Reached source pages require matching crate/generator metadata from the same capture; unrelated API crates are not selected. Local links and anchors must resolve. Source-range links retain their fragments without executing Rustdoc's highlighting script. External links remain ordinary references.

Native reexport forwarding pages must match Rustdoc's complete wrapper and lead directly to an API page inside the selection. An exported macro's `macro.name!.html` forwarder must lead to its corresponding `macro.name.html` page. The target maps links and fragments to the selected page without emitting forwarding scripts or duplicate navigation entries. Changed wrappers, escaping paths, redirect chains and missing targets fail. Collapsed field and variant lists keep every declaration while omitting the native expansion label.

Embedded MathML keeps its presentation elements and TeX annotation, checked against Gazette's shared MathML profile. Namespace changes, active attributes, incomplete formulas and unsupported elements fail.

Native notable-trait tooltips become linked static sections containing their type descriptions, trait signatures and references. Repeated tooltip links share one section. The literal JSON map must match the tooltip keys exactly; missing or duplicate data, executable content and conflicting anchors fail. The same bounded HTML reader checks each fragment and closes only Rustdoc's observed trailing `pre`/`code` pair. Its links use the ordinary selected-page and external-link checks.

Literal `trait.impl` data is accepted only when its wrapper, duplicate-free crate entries and fragment metadata match the supported shape. Empty and current-crate entries retain their existing static HTML; nonempty foreign entries are unsupported. An absent optional file does not mean external implementors do not exist. Type aliases retain their declarations and the inherent and trait implementations selected by their exact crate and `data-self-path` from captured `type.impl` data. The complete native wrapper and fragment metadata must agree; no script runs. The bounded HTML reader checks each implementation, preserves its examples and links, and assigns distinct destinations when section IDs repeat. Native capture-root links are checked against the same captured pages. Missing data, unmatched aliases, malformed rows and missing link targets fail. Unresolved Rust intra-doc links or an external implementation’s relative link without its destination remain errors; the adapter does not infer their intended URLs. Unknown active HTML and unsupported markup also fail. Source comments absent from Rustdoc's rendered documentation are not reconstructed. Rustdoc JSON is a separate unsupported format.

Two qualified native versions need one standard-library link correction: Rustdoc 1.94.0 (`4a4ef493e`) and 1.96.0-nightly (`e370b60cf`). Their BTreeMap alias data copies `insert`’s `index.html#insert-and-complex-keys` link, but [the library source](https://github.com/rust-lang/rust/blob/1.94.0/library/alloc/src/collections/btree/map.rs#L1028) intends the section in [standard-library collections](https://doc.rust-lang.org/1.94.0/std/collections/index.html#insert-and-complex-keys). Gazette corrects that exact link only in the captured native BTreeMap implementation data, in an inherent fragment containing `method.insert`, with the qualified generator identity and its existing standard-documentation root. Other owners, versions, fragments and missing destinations still fail ordinary link checks. This changes the emitted destination without changing the original capture or removing explanation. The native main section’s `tabindex="-1"` focus control is omitted with the other browser controls; this does not admit `tabindex` on arbitrary content.

## OpenAPI and Swagger

OpenAPI accepts versions 3.0.0–3.0.4 and 3.1.0–3.1.2. JSON and YAML are explicit formats, not filename guesses. Their original bytes and version are retained. The target keeps retained artifacts excluded from ordinary provider discovery, then adds exact `.mintignore` exceptions for selected OpenAPI files and their parent directories. Mintlify needs those exceptions to discover local HTTP specifications. Other SDK artifacts and their companions remain ignored; selecting an HTTP reference does not admit its siblings. The adapter validates the document and Schema Objects offline, checks local JSON pointers, operation links, discriminator targets and reference-only cycles, and preserves concrete recursive schemas without inlining them. `$ref` inside an example, default, constant, enum, link payload or extension remains data.

OpenAPI 3.0 uses its Draft-04 schema. A schema-position Reference Object's siblings are retained but ignored; alias-only cycles fail. Boolean `additionalProperties` is not a referenceable Schema Object. In 3.1, `$ref` siblings remain constraints and boolean schemas retain their meaning. The only selected 3.1 dialect is `https://spec.openapis.org/oas/3.1/dialect/2024-11-10`, also used when omitted.

OpenAPI 3.2, external references, custom dialects, schema `$id`, anchor/dynamic/ recursive addressing and Path Item `$ref` siblings are unsupported. The profile does not validate every example instance, security scope or path-parameter rule. Assembly neither fetches examples nor calls API servers.

YAML must be one document with unique decoded string keys. An explicit version must be 1.2; tag directives, tags, anchors, aliases, unquoted merge keys and complex keys fail. Quote numeric response keys such as `'200'`. Quoted and block scalars remain strings. Numbers must use JSON spelling and preserve arbitrary precision; non-finite or YAML-only numeric spellings fail. Parsing is bounded by nesting 128, 100,000 materialized values and 300,004 events, in addition to file/tree limits. OpenAPI validation separately has a 100,000-value ceiling.

Swagger 2 requires `swagger-json` or `swagger-yaml`; OpenAPI declarations do not auto-convert it. The adapter validates the source, converts it to OpenAPI 3.0.3 with `swagger2openapi =0.2.0`, then validates the result. Conversion preserves query-bearing OAuth URLs and the supported scalar form media representations; security-scheme keys are mapped when required by the target grammar. Tuple-schema items fail because their ordered heterogeneous meaning cannot survive the target. Array-valued `formData` parameters are refused when the operation consumes `multipart/form-data`: OpenAPI 3.0 cannot preserve their Swagger `collectionFormat`. For `application/x-www-form-urlencoded`, arrays support `csv` (the default), `multi`, `ssv` and `pipes`; other encodings fail. Arrays inside opaque examples remain data.

### Where the schemas come from {#schema-provenance}

The following unmodified official resources live under `adapters/mintlify/src/openapi/`. Their Apache-2.0 notice is retained in that directory's `LICENSE`. The validator serves bundled schemas only; it does not retrieve external resources.

| Resource | Official source | SHA-256 |
| --- | --- | --- |
| `schema-2.0.json` | [Swagger 2](https://spec.openapis.org/oas/2.0/schema/2017-08-27) | `b36871c8016292c5e66dd3b203e69aeff98bfef97e0b3c67c1909036095586a5` |
| `schema-3.0.json` | [OpenAPI 3.0](https://spec.openapis.org/oas/3.0/schema/2024-10-18) | `2385f5bbb8c37878daae73baeabe7f34b2f022a4a8c049329ee61f71796f039c` |
| `schema.json` | [OpenAPI 3.1](https://spec.openapis.org/oas/3.1/schema/2025-11-23) | `1b8ccc6e34234b17536f2dd0eb3597142a32bd108438cd42471a5fca4c1a07ef` |
| `dialect.json` | [Schema Object dialect](https://spec.openapis.org/oas/3.1/dialect/2024-11-10) | `647f32dfff64949d5020a28ecd1af4ffeffb1e9c695f861a52255a8004e07460` |
| `meta.json` | [Base vocabulary](https://spec.openapis.org/oas/3.1/meta/2024-11-10) | `80706a9a404affedbf84ac4dc1328c9ce0d2a00804cdfc4d95c0ddd0053121dd` |

The OpenAPI 3.1 path composes the official document and Schema Object validators and separately enforces root/nested dialect constants. It does not rewrite the upstream schemas. The implementation rationale is beside the validators in `adapters/mintlify/src/openapi.rs`.

## Interpret a rejected input

SDK callers receive `DocumentationError`; read its `code()` and detailed cause.

| Code | Check next |
| --- | --- |
| `documentation_binding` | The selected site's brand publication must name a documentation repository and build. |
| `documentation_generation_required` | Run the explicitly selected generator before assembling; imported bytes are not a fallback. |
| `documentation_artifact` | Check the exact artifact path and file/directory kind. |
| `documentation_manual_input` | Check the selected manual source, pages and assets. |
| `documentation_target_input` | Read the context and reason for the incompatible format, route, title or link. |
| `documentation_target_json` | Inspect the JSON decoding or configuration-encoding failure. |
| `documentation_target_markdown` | Inspect the native Markdown/transport failure. |
| `source_limit` | Narrow the input/output or explicitly raise the relevant allowance. |

Other source and acquisition failures retain their native codes. A format error rejects the complete target; changing a navigation group does not make a failed page optional. See [Mintlify integration](mintlify.md) for the assembly call and the separate output operations.
