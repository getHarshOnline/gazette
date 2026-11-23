# Publish saved notebook results

<a id="retained-notebooks-and-static-publication"></a>

Attach an `.ipynb` file to a page to show its saved Markdown, code and supported results. Gazette renders the saved cells without a kernel, following the [saved-result model](architecture.md#trusted-code-and-saved-results). Follow the [attachment declaration](#explicit-page-attachment) and check whether your result appears in the [static output profile](#native-static-profile).

Gazette checks the case-insensitive file extension and validates nbformat 4.5. It does not convert older formats, repair missing cell IDs or run code. The Rust SDK exposes `notebook::ParsedNotebook` for the same parsing and checking used during a publication build. For the author's workflow, see [Figures, notes and math](https://rawthoughts.in/gazette/figures-notes-and-math/).

Run this to check the notebook and other files from a site's configured source:

```sh
gazette check --stdin --acquire-site example:site --parse-documents < workspace.toml
```

## What Gazette reads and keeps {#one-retained-representation}

Gazette keeps the original notebook bytes and one decoded JSON tree. The tree keeps cell order, source strings or arrays, saved outputs, MIME values, attachments and allowed metadata. The Serde decoder rejects duplicate decoded keys, preserves arbitrary-precision numbers and treats Serde's private token names as ordinary keys. Reading raw slices may scan a subtree more than once. Input size and depth limit that work.

The upstream schema defines the JSON structure. Gazette also requires version 4.5, unique cell IDs and explicit cell metadata names. It does not generate UUIDs, guess languages or turn kernel metadata into authorship, language, rights or approval. A valid structure may still contain metadata conventions Gazette does not check.

Debug and inspection output contains counts and byte/schema identities. Errors give classes and decoder-local coordinates without showing notebook values. A nested decoder location may differ from a complete-file offset. Explicit source and decoded-value accessors can expose the private input. The public Theme context offers the notebook file only when its page [selects a download](#offer-the-notebook-file) and the pre-public scan passes.

| Default limit | Value |
| --- | --- |
| Notebook source | 1 MiB |
| JSON values in one notebook | 100,000 |
| Cells in one notebook | 4,096 |
| JSON nesting | 128 |
| Notebook values across the corpus | 250,000, in addition to the shared source-byte ceiling |

Gazette checks source bytes before JSON allocation and values/cells before schema validation. Parser and schema allocation overhead is additional. These limits do not cap all process memory or CPU. The SDK accepts other explicit limits; the CLI uses these defaults.

## Explicit page attachment

An authored page (including framed project documentation) declares ordered capture-relative inputs at the frontmatter root, before `[content]`:

```toml
notebooks = ["analysis/results.ipynb", "analysis/method.ipynb"]
```

The paths resolve through that page's existing workspace content-source binding, not relative to its Markdown filename or an ambient working directory. They must identify retained, validated notebooks in that exact capture. Missing, wrong-kind, repeated, escaping and cross-source substitutes fail. Declaration order is retained; notebooks do not acquire new content identities, routes, attribution or language from their metadata.

An entry may instead be a table that also offers the notebook file to readers; see [offer the notebook file](#offer-the-notebook-file). Its only keys are `path` and `download`:

```toml
notebooks = ["analysis/results.ipynb", { path = "analysis/method.ipynb", download = true }]
```

Candidate selection independently includes each notebook input. Including its page while excluding a required notebook fails before rendering. Excluding the page does not silently decide the notebook's disposition.

## Supported static outputs {#native-static-profile}

The built-in `gazette.notebook-static/1;nbformat/4.5` renderer consumes the retained JSON tree. Its Markdown uses the same explicitly supplied Comrak adapter and compiler admission path as ordinary documents, not an ambient Python runtime or a second Markdown implementation. Notebook source/schema identities, renderer profile and exact HTML/text hashes remain private per-page build evidence. Stored raster evidence additionally records each exact encoded image identity, dimensions and decoder.

The current profile displays Markdown and code cells, stored streams/errors, and supported plain-text/Markdown/inert-HTML/JSON/PNG/JPEG results without invoking a kernel. Code and textual outputs are escaped. Cell order and stored output order are preserved; unsupported alternative MIME representations are not hidden by selecting a convenient `text/plain` fallback. Notebook Markdown uses the shared renderer's namespaced headings, footnotes and fragment links. Local companion references use the owning source's explicit static-asset mapping, as described below. Literal HTTP(S) Markdown links/image references remain references: no remote bytes are acquired or validated, and emitting their URL is not image admission.

This is the complete Gazette 0.1 static profile, not arbitrary notebook execution or nbconvert compatibility. Raw cells, active or out-of-profile HTML, SVG, widgets and other unqualified MIME formats fail explicitly. They are deliberate profile exclusions rather than silently deferred advertised capabilities. Mathematical typesetting is available only through retained Markdown content; interactive execution capabilities are not inferred.

### Link companion files {#retained-companion-assets}

Markdown images and supported static downloads resolve relative to the notebook's own retained file, not the embedding page. The target must stay within that page's exact configured source capture and occur in its selected `static_assets` plan. The existing asset resolver owns decoding, confinement, public addresses and retained-byte lookup; notebook rendering adds no second resource registry. Percent-encoded filenames and query/fragment suffixes use the same rules as ordinary authored assets. Cross-source substitutes, traversal, unsupported, missing, unmapped or excluded companions fail the complete render before staging. Candidate selection cannot restore an excluded companion through a notebook.

Repeated notebook/body references share one page asset relationship and one site asset record/output. Theme and machine projections receive those same records. Static transport preserves exact captured bytes; it does not establish raster decoding, MIME safety, disclosure or rights approval. Authored data URIs remain unsupported. The standalone notebook renderer stays strict unless its caller explicitly supplies retained-source mappings.

### Cell-local image attachments

A Markdown cell may supply its own `attachments` dictionary and refer to an image with `![Authored description](attachment:plot.png)`. The exact `plot.png` key is looked up only in that cell. It is not a filesystem path, a workspace asset, a cross-cell lookup or an instruction to fetch anything. This bounded profile uses exact names; percent-encoded aliases are not inferred.

Each dictionary entry must contain exactly one `image/png` or `image/jpeg` representation. The existing Base64 and raster decoder described below validates every entry, including unused ones. Empty, unsupported or multiple-representation bundles fail rather than silently choosing a representation. Missing references and corrupt images fail the complete notebook; ordinary links cannot use the image-only attachment exception.

The shared Markdown renderer binds each emitted image occurrence to a private, byte-verified image transport. It preserves image position, authored alt text, titles and surrounding formatting; code examples are not rewritten. Only referenced images are emitted. All validated attachments retain private decoder evidence, while search receives the Markdown alternative text, not Base64 or an invented caption. Identical attachment names in different cells remain independent. Ordinary Markdown and arbitrary resolver-returned data URLs retain their existing refusal rules. No new renderer, image parser, native read or public-admission shortcut is added.

### Stored Markdown outputs

Stored `text/markdown` in `display_data` and `execute_result` uses the same Markdown parser, admission rules, renderer and retained-source resolver as Markdown cells. String arrays concatenate exactly, without inserted separators. Each output receives a namespace beneath its cell ordinal and original stored output ordinal; intervening streams count toward that ordinal. Headings, footnotes and fragment links therefore cannot collide with another cell or output. Markdown syntax-node limits are shared across all Markdown cells and outputs; the existing whole-notebook HTML/text ceilings still apply. Both projections retain every supported MIME alternative, not just Markdown. Raw HTML, unsafe links, unresolved companion references, malformed Markdown representations and any unsupported alternative fail the complete notebook. Code-output Markdown has no cell-local attachment namespace. Fragment links are namespaced, not independently certified as existing destinations. No kernel, filesystem read or remote fetch is introduced, and rendered output remains separate from publication approval.

### Stored inert HTML outputs

Stored `text/html` uses the existing pinned html5ever tokenizer in a separate rejecting projection, not the disclosure token probe or an HTML sanitizer. Balanced paragraphs, divisions, blockquotes, simple lists, inline emphasis/code, line breaks and tables are supported. Tables require explicit row groups and balanced rows/cells; caption/header/body/footer ordering is checked. Nested tables and browser-repair-dependent markup are refused.

HTML and text are produced together from the same decoded tokens. Text entities are decoded once and escaped again for HTML; table/list/block boundaries remain separated in searchable text. Class tokens, bounded positive cell spans and header scope are the only supported attributes. No IDs, URLs, resources, event handlers, inline styles, scripts, foreign SVG/MathML, comments or doctypes are accepted. Unsupported content fails the complete notebook, even alongside a plaintext alternative; it is never silently removed. This is not arbitrary HTML or automatic compatibility with generator-specific styling.

Input, token, decoded-text, nesting and per-representation output bounds apply before the existing whole-notebook/page output bounds. No native reads, kernels, network calls or new public-admission permission are introduced.

### Stored raster outputs

Stored `image/png` and `image/jpeg` representations use the same inward raster contract as the SDK's `raster::inspect`. The application explicitly supplies a pure decoder function to projection and review; compiler code has no concrete image-library dependency. The Rust SDK selects the adapter once for its local publication paths. Pure callers that select `raster::unavailable` fail explicitly when an image is required; no unchecked image or text fallback is accepted.

The notebook transport joins source arrays, removes only CR/LF from standard Base64, and rejects other whitespace, malformed padding, URL-safe encoding and data-URI prefixes. After actual pixel decoding and exact evidence binding, the renderer emits the unchanged encoded image as a generated data URI, with checked numeric dimensions and no notebook-metadata attributes. This does not enable arbitrary authored data URIs. Text alternatives remain in their own original order; a searchable stored-image marker is not OCR or an invented description.

The adapter uses pinned `png` 0.18.0 and `zune-jpeg` 0.5.16-rc2 with `zune-core` 0.5.3. PNG is checksum-checked, pixel-decoded and read through its end chunk; animated PNG is rejected rather than silently reduced to one frame. JPEG uses strict decoder options before parsing and decodes into an explicitly bounded pixel buffer. Neither path opens a source file or transforms pixels.

Default per-image ceilings are 4 MiB encoded bytes, 64 MiB decoded pixels, and 8,192 pixels per dimension. Notebook source bounds further constrain encoded input, and the complete page's output ceiling still applies to inline images. PNG also receives a decoder allocation allowance; JPEG has no equivalent total allocation limiter. Decoder scratch allocations and execution time are not a process sandbox. Original EXIF/ICC/text metadata and decoder-tolerated trailing bytes remain in the image: decoding is not sanitation, exhaustive stream conformance, privacy approval or certification. The separate disclosure gate must account for these bytes before any public release.

## Where notebooks appear {#notebooks-in-notes}

Declared notebooks join the page's [Notes](content-model.md#article-notes-stay-with-their-article), after any `notes.md` content. A page with notebooks and no `notes.md` still has Notes. The essay's `body_html`, `body_text`, `headings`, excerpt, feeds and JSON-LD text hold only the essay.

Each notebook appears once in `notes.notebooks`, in declaration order, as structured cells. `notes.body_html` stays the `notes.md` HTML. Notebook text joins `notes.body_text`, so search ranks it through the `notes` field rather than the primary `body`. Notebook links, components and images join `notes.links`, `notes.components` and `notes.images`. The reading Markdown alternate carries the notebooks under its Article notes heading.

```json
{"id": "gazette-notebook-<sha256 of the logical path>",
 "cells": [
   {"kind": "markdown", "cell_id": "intro", "html": "<h2 id=\"…\">…</h2>…"},
   {"kind": "code", "cell_id": "code", "execution_count": 3, "language": "python",
    "source_html": "<pre class=\"notebook-source\">…</pre>",
    "outputs_html": "<div class=\"notebook-stream\">…</div>…"}],
 "download": null}
```

Fragments are the renderer's output without the cell wrapper. The Theme supplies the wrapper and can use `cell_id` for `data-cell-id`, for example to place `source_html` in a native `<details>`. `execution_count` and `language` may be null. Heading and footnote IDs keep the notebook namespace, so existing anchors still resolve and cannot collide with the essay or `notes.md`. When the notebook's Markdown has an H1, its headings are lowered one level, the same rule `notes.md` follows. Notebook headings are not added to `notes.headings`. Whole page records carry `notes.notebooks`: the page's Theme context, listing contexts and the machine model. Search documents and feed metadata keep the Notes without the cells.

## Offer the notebook file {#offer-the-notebook-file}

Set `download = true` on a page's notebook entry to publish the exact retained bytes beside that page, at its route plus the notebook's file name. The file goes through the site's static asset inventory: one record, and a collision with another output fails the build. `notes.notebooks[].download` is then `{url, bytes, media_type}`, with media type `application/x-ipynb+json`. Without the option it is null and no file is emitted. If several pages select the same notebook, the first page in identity order supplies the address.

Before emitting the file, Gazette scans the notebook metadata and every cell source, metadata, attachment and stored output, including tracebacks. It refuses the download when it finds:

| Class | Examples |
| --- | --- |
| `local_path` | `/Users/…`, `/home/…`, `/tmp/…`, `C:\…`, UNC shares, and kernel or execution metadata such as `path`, `cwd`, `argv` or `connection_file` holding a path |
| `identity` | Email addresses, `user@host:` prompts, and metadata keys such as `user` or `username` |
| `secret` | `sk-`, `ghp_`, `github_pat_`, `AKIA` and `xox` keys, PEM private-key blocks, and long high-entropy values assigned to names such as `token`, `secret`, `key` or `password` |
| `private_link` | `file:` URLs, `localhost` and loopback addresses, and private-network or `.local` hosts |

A refusal withholds only the file: the notebook still renders and `download` stays null. The page's build evidence records `download_refusal` on that notebook with the class, the cell index and the field, such as `cells[3].outputs[0].traceback`. It never contains the matched text. Base64 image bytes are scanned as text, not decoded, so the scan does not inspect pixels or embedded image metadata. Passing the scan is not disclosure or rights approval.

## Attach each notebook to a page {#rendering-and-public-admission-remain-separate}

Every notebook selected for a site must be attached to a page through `notebooks` and render successfully. A selected notebook with no owning rendered page fails the site build with `site_uncompiled_asset`; Gazette does not quietly omit it. Attach it through the page's `notebooks` declaration, or remove it from that site's selected inputs. Static asset configuration cannot copy a notebook into public output; only a page's `download` option can.

Notebook requirements for another site remain scoped to that site, while whole-corpus syntax checking still checks every selected input. Disabled sources are not acquired.

The bounded profile includes cell attachments, retained companion assets, stored Markdown, inert HTML, JSON and validated PNG/JPEG output. Public admission, complete-output disclosure and explicit optional-degradation policy remain separate gates; a source download is emitted only when a page selects it and the scan passes. An unavailable renderer fails the selected build; Gazette does not substitute placeholder HTML or a download.

## Where the notebook schema comes from {#upstream-schema-provenance}

The byte-identical schema is [`nbformat.v4.5.schema.json`](../crates/gazette-compiler/src/notebook/nbformat.v4.5.schema.json), from the nbformat 5.10.4 distribution, path `nbformat/v4/nbformat.v4.5.schema.json`. Its SHA-256 is `523e3578ddbfcad52933d2423dc5951114ef73f48df180b6a601498ba5ca071a`. The distribution's [BSD-3-Clause notice](../crates/gazette-compiler/src/notebook/LICENSE) is retained beside it and must accompany redistributed schema bytes, including those embedded in binaries. The schema is used unmodified.

`jsonschema =0.49.2` supplies Draft-4 evaluation with default features disabled and matching arbitrary-precision support for schema integer/numeric checks. The explicit retriever refuses external resources even if feature unification later enables network/file support. Only the bundled schema is compiled; input `$ref` metadata is data. Exact transitive versions are pinned in `Cargo.lock`.
