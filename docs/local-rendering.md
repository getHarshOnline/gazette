# Render publications locally {#connected-local-site-rendering}

Render one or more sites from the same workspace and read the output inventory before you write anything. Start with a [compiled workspace](workspace.md) that names the sites, their content sources and Themes. The render includes drafts and archived pages unless the recipe supplies a [release candidate](releasing.md#closed-candidates).

| I need to… | Start here |
| --- | --- |
| Build several sites from one capture | [Local site sets](#local-site-sets) |
| Inspect the model or scan the generated files | [Inspect a local build](render-inspection.md) |
| Configure Markdown, discovery rules or static files | [Prepare inputs for the renderer](render-inputs.md) |
| Change the Theme, resource handling or templates | [Prepare templates and browser resources](render-resources.md) |
| Read search records, machine files or the site graph | [Read the generated site files](render-outputs.md) |
| Open the site in a browser or watch files | [Local publication preview](preview.md) |
| Write an existing configured output directory | [Output materialization](output-materialization.md) |

Use `build::render_publication` for one in-memory site. For several sites, the Rust SDK supplies `build::acquire_publications` and `build::render_publications`. `build::stage_publication` or the single-site command’s `--output-parent` writes a fresh private stage. Rendering grants no permission to release the site.

## Local site sets

For a workspace on disk, keep its usual choices in `gazette.toml`:

```toml
[build]
sites = ["site:one", "site:other"]
local_drafts = "include"
corpus_bytes = 536870912
math = true
```

```sh
gazette render-sites --local --workspace-root /absolute/path/to/configuration-root --allow-execute
```

`--workspace` selects a different root-relative configuration document. The default is `gazette.toml`. `--allow-execute` permits this call's configured math process; the workspace cannot grant that permission. Omit it for a build that needs no native math execution. Rendering remains in memory.

Use a JSON request when the invocation needs explicit site options or a caller-owned configuration buffer. Save the following as `render.json`, replacing the absolute workspace root and the site IDs. Omitting `sites` selects `[build].sites`; an explicit list overrides it.

```json
{
  "format": "gazette-local-publication-set/1",
  "workspace": {
    "kind": "local",
    "root": "/absolute/path/to/configuration-root",
    "workspace": "gazette.toml"
  },
  "sites": [
    {"site": "site:one", "model_output": "model.json"},
    {"site": "site:other", "model_output": "model.json", "graph": {"output": "graph/index.html"}}
  ]
}
```

Run the build without writing a stage. This workspace enables math, so permit the configured typesetter for this invocation:

```sh
gazette render-sites --local --allow-execute < render.json
```

Read the inventory and check the status of each site. `gazette check build --local` runs the same build with the same request, result and failure rules. Use it when the goal is validation. Neither command creates a stage or release record, approves output or deploys it. Use plain `gazette check` to check the selected workspace and its pages and `check --publication` to [inspect the resolved model](render-inspection.md#inspect-semantics-without-rendering) without running a Theme.

### Name the workspace and sites {#select-workspace-bytes-and-sites}

The tagged workspace input is either the explicit local document set shown above, or `{"kind":"supplied","source":"...exact TOML..."}` for a caller-owned buffer. The local root must be absolute and the workspace path root-relative; Gazette reads only that root document and its exact non-recursive include set. It never searches the current directory, ancestors or siblings. Site options name reserved output paths; the selected Theme manifest owns exact page, alias and graph entries. Unknown fields/versions, duplicate object members and empty, duplicate or over-limit site selections fail before native acquisition. Cross-links never add render targets. Configured foreign content still participates in the shared semantic graph, even when its publication is not in the requested output set.

### Set the year, model file or candidate {#select-render-options}

An optional request-level `render_year` is a positive calendar year in `1..=9999`, independent from article publication/modification dates. An explicit value wins over a boundary default. The direct CLI render and preview commands capture the current UTC year once when no command default is supplied; `--render-year` supplies a reproducible command default. The MCP boundary also captures the current UTC year once; an explicit recipe value takes precedence there as well. Rust SDK callers supply their own value: the compiler and renderer do not read a clock. The same exact value reaches page metadata, generated listing metadata and graph presentation metadata, and participates in build cache identity. An omitted SDK value remains omitted.

`model_output` is optional. Omit it when the site does not need a standalone JSON model artifact, for example `{"site":"site:one"}`. The shared model still feeds Theme rendering and every selected machine output. Gazette does not invent a destination for the omitted artifact; render evidence records a model path only when one was selected. Selecting a path retains the ordinary collision checks and changes the build-cache identity. The single-site `render-site` command has the same behavior when `--model-output` is omitted.

An optional `selection` object supplies one complete content/asset disposition map for the whole captured corpus, as described in [closed candidates](releasing.md#closed-candidates). It is not a site-local filter or publication approval. Omission retains private unfiltered rendering; explicit null, unknown fields, repeated keys or malformed identities/paths fail before acquisition. Candidate completeness and relationship closure are checked against the captured model before any site program runs.

### Allow the math or diagram program {#select-native-math-or-diagram-generation}

For the author’s setup and a first formula, return to [Typeset mathematics](https://rawthoughts.in/gazette/typeset-mathematics/).

The workspace chooses the math typesetter or DOT renderer and its exact source or executable pins. `[build].math` selects math generation; `--math true` or `--math false` overrides it for one CLI call. The recipe also accepts `math` and `allow_execute`. Selection and execution permission are separate.

Math defaults are a 2 MiB request, a 30-second deadline, 4,096 formulas and 64 MiB aggregate output. Per-formula and per-fragment limits still apply. A `math_generation` object selects math explicitly and can override individual defaults; `{"math_generation":{"allow_execute":true}}` is enough to use them. The following example deliberately chooses smaller limits and also enables diagrams:

```json
{
  "math_generation": {
    "allow_execute": true,
    "max_input_bytes": 1048576,
    "timeout_ms": 30000,
    "formulas": 64,
    "output_bytes": 1048576
  },
  "diagram_generation": {
    "allow_execute": true,
    "max_input_bytes": 1048576,
    "timeout_ms": 30000,
    "diagrams": 16,
    "output_bytes": 1048576,
    "diagnostic_bytes": 16384
  }
}
```

Add only the required field to the complete recipe, rather than submitting this fragment alone. Omitted math selection follows `[build].math`, then defaults off; omitted `diagram_generation` stays off. `math: false` conflicts with an explicit `math_generation` object. Selecting a renderer in the workspace does not grant execution permission, and permission to read sources or open preview listeners does not grant it either. The [Markdown reference](markdown.md) explains the retained typesetting result; [page resources](page-resources.md#theme-resources-can-use-retained-shared-inputs) describes DOT resource binding. The current named fields and bounds also appear in the native recipe schema at MCP `render_sites`'s `recipe_json.contentSchema`.

Acquisition runs the selected program once and retains its input/output binding; rendering consumes that result without rerunning it. In development preview a later source capture may need a new invocation. These native processes are not sandboxed: their filesystem, network or other effects are separate from Gazette's in-memory rendering. Inspect failed or interrupted execution before deciding whether to retry.

### Read results for every site {#read-complete-set-results}

Gazette compiles the workspace, captures the files needed by the requested sites and resolves one content model. The sites share those immutable input bytes. Each site's Theme and resources render against that model without reopening files. Sites have independent output paths, so each can produce `index.html`. A collision within a site still fails.

| Field | What the private result tells you |
| --- | --- |
| `status` | `local_set_rendered` when all sites succeeded, otherwise `local_set_incomplete` |
| `sites` | Every requested site. Each has `rendered` with its inventory and render details, or `failed` with its original error code and message. |
| `compilation`, `sources` | One copy of the shared configuration, Git pins and captured tree identities |
| `source_bytes` | Actual retained payload sizes: `captured_total` counts each source key once, `shared_content` is the common graph input, and `sites` gives each site's input footprint and additional presentation bytes. Site footprints overlap; their sum is not a disk-read total. `missing_sources` identifies captures whose sizes remain unknown. |
| `selection` | The complete candidate decision, including exclusions, or null for an unfiltered render. It appears even if every site failed. |
| `source_failures` | The errors for all presentation-only sources that failed, including shared inputs. A site's error uses its first failed source in key order. |

The CLI prints the complete structured result and exits unsuccessfully if any site failed. It keeps successful siblings in the result. Keep the report private because it can include paths and diagnostics. It is separate from the emitted files and release records.

### Size the request, input and output limits {#bound-acquisition-and-output}

| Limit | What it counts |
| --- | --- |
| `--max-source-bytes` | Entire JSON request before decoding, 64 KiB by default |
| Acquisition defaults | Up to 128 requested sites and 256 different source keys under one source budget |
| `--max-corpus-bytes` | Aggregate authored files and opaque assets before parsing; invocation override, then `[build].corpus_bytes`, then 16 MiB |
| `--max-total-output-bytes` | Combined output, 1 GiB by default |
| `--max-total-output-entries` | Combined output, 100,000 entries by default |

Gazette reads content sources before presentation-only sources, in stable order in each group. A source used by both counts as shared content and is read once. Failed sources hold no payload and are not retried. Successful captures still count against the budget. The budget counts bytes kept, not all attempted filesystem reads or Git work. Each attempt has its own read limits.

`render-site`, `render-sites`, MCP and preview resolve the same workspace corpus setting. It also reaches math/diagram acquisition and preview rebuilds. Use `--max-corpus-bytes 67108864` to override it for one CLI call, including with a smaller ceiling. It does not change acquisition, per-document, parser, output or transport limits.

Per-site output ceilings are separate. Sites render in key order and can use the remaining combined allowance. A failed site returns no partial tree. Insertion order in the request cannot decide which site gets a remaining allowance. These limits count bytes and entries, without measuring CPU time or exact allocator use.

### Render several sites from Rust {#use-the-sdk-set-entrypoints}

Rust callers use `build::acquire_publications` and `build::render_publications` with explicit set limits and exactly matching option keys. `PublicationSetRenderOptions` combines those site recipes with the optional compiler-owned selection. The result retains the actual immutable per-site trees, not only its inspection report. Single-site rendering uses the same acquisition, metadata, compiler and output owners. Acquired site entries are explicit success/failure values. Successful views share one immutable packet; source failures do not produce a partially valid view. Later SDK staging still protects every selected source root, including failed captures. An unresolved root can therefore block staging without undoing a successful in-memory render; missing input is not permission to write there.

### Distinguish shared and per-site failures

Gazette performs no output staging, database writes, listening, Git mutation or deployment in this command. Separately permitted native generators can have their own effects, as described above. Shared configuration, required content acquisition and complete content syntax/semantic errors still stop the whole operation before rendering. A failed presentation-only source instead fails each dependent site while unaffected sites continue. Without candidate selection, if all sites fail acquisition the report keeps their exact failures and shared captured-input evidence without parsing content, calling renderers or claiming valid semantics. With selection, shared syntax, semantics and candidate closure are still checked from the retained required content; failed presentation cannot bypass them. Unfiltered output remains draft-inclusive; candidates reject included drafts and retain published/archived distinctions. The result does not authorize publication or record release-set settlement.

## Continue by task

<a id="execute-and-inspect"></a> <a id="keep-acquisition-syntax-and-resolved-state"></a> <a id="inspect-semantics-without-rendering"></a> <a id="project-the-complete-private-registry"></a> <a id="match-a-review-response"></a> <a id="output-and-failure-boundaries"></a> <a id="private-output-disclosure-probe"></a> <a id="media-selection-and-json"></a> <a id="html-channels"></a> <a id="javascript-channels"></a> <a id="css-channels"></a> <a id="xml-channels"></a> <a id="encoded-candidates-and-embedded-coverage"></a> <a id="probe-report-and-comparison-limits"></a> <a id="required-supported-profile-check"></a> <a id="typed-disclosure-policy"></a> <a id="raster-metadata-in-policy-checks"></a> <a id="direct-cli-inspection"></a>

### Inspect a local build

[Inspect a local build](render-inspection.md). Inspect parsed content, review pages before templating, or scan the generated files.

<a id="review-exact-retained-surfaces"></a>

[Review pages before running the Theme](render-inspection.md#review-exact-retained-surfaces).

<a id="inputs-and-one-source-of-truth"></a> <a id="discovery-without-weakening-materialization"></a> <a id="generated-taxonomy-navigation"></a> <a id="plain-project-native-markdown"></a> <a id="source-relative-documentation-links"></a> <a id="static-assets-from-configured-sources"></a>

### Prepare inputs for the renderer

[Prepare inputs for the renderer](render-inputs.md). Bind authored content, Markdown and static files so the renderer reads the files you intend it to use.

<a id="capabilities-providers-and-selected-site-resources"></a> <a id="dependency-resources-across-selected-sources"></a> <a id="captured-stylesheet-compilation"></a> <a id="resource-assembly-and-theme-context"></a> <a id="resource-compilation-limits"></a> <a id="optional-capability-absence"></a> <a id="shared-heading-model"></a> <a id="per-page-template-selection"></a>

### Prepare templates and browser resources

[Prepare templates and browser resources](render-resources.md). Choose the Theme templates, resources, capabilities and heading data that the renderer uses.

<a id="optional-local-site-graph"></a> <a id="generated-output-reference"></a> <a id="markdown-counterparts-are-materialized-not-guessed"></a> <a id="llmstxt-uses-built-pages-or-an-explicit-reading-guide"></a> <a id="llms-txt-uses-built-pages-or-an-explicit-reading-guide"></a> <a id="shared-model-and-native-evidence"></a>

### Read the generated site files

[Read the generated site files](render-outputs.md). Find the graph, discovery files, Markdown copies and search records produced from the shared page model.

<a id="site-discovery-outputs"></a>

[Find the discovery files](render-outputs.md#site-discovery-outputs).

<a id="search-documents"></a>

[Read the search records](render-outputs.md#search-documents).
